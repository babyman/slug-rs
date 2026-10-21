//! `slug.net.tcp` on typed native bindings, built as clutch staging builds
//! it. Every test serves and connects on loopback with port 0, so tests run
//! in parallel without colliding. Unix-only, as the clutch is.
#![cfg(unix)]

use std::{
    env,
    fmt::Write as _,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};

use slug_native_manifest::{NativeManifest, build_dynamic};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn platform() -> &'static str {
    match (env::consts::OS, env::consts::ARCH) {
        ("macos", "aarch64") => "macos-aarch64",
        ("macos", "x86_64") => "macos-x86_64",
        ("linux", "x86_64") => "linux-x86_64",
        (os, architecture) => panic!("unsupported native test platform {os}-{architecture}"),
    }
}

/// Copies `source` to `target`, leaving out staged platform builds.
fn copy_clutch(source: &Path, target: &Path) {
    fs::create_dir_all(target).expect("create clutch directory");
    for entry in fs::read_dir(source).expect("read clutch directory") {
        let entry = entry.expect("read clutch entry");
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("macos-") || name.starts_with("linux-") || name.starts_with("windows-")
        {
            continue;
        }
        let destination = target.join(entry.file_name());
        if entry.file_type().expect("read entry type").is_dir() {
            copy_clutch(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).expect("copy clutch file");
        }
    }
}

/// A `SLUG_HOME` with the TCP and core clutches, built from the repository's
/// sources, and the repository's `slug.builtin` and `slug.channel`.
struct Home(PathBuf);

impl Home {
    fn new() -> Self {
        let home = env::temp_dir().join(format!(
            "slug-typed-net-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        let repository = home.join("clutch");
        let mut index = String::from("[modules]\n");
        for (clutch, provider) in [
            ("slug.net.tcp.clutch", "slug_net_tcp"),
            ("slug.core.clutch", "slug_core"),
        ] {
            let root = repository.join(clutch);
            copy_clutch(&workspace_root().join("clutch").join(clutch), &root);
            let text = fs::read_to_string(root.join("clutch.toml")).expect("read clutch manifest");
            let manifest =
                NativeManifest::from_clutch_source(&text).expect("valid clutch manifest");
            build_dynamic(&root, &manifest, provider, platform())
                .unwrap_or_else(|error| panic!("build {clutch}: {error}"));
            // The modules the clutch provides, from its `[modules]` table.
            for line in text.lines().skip_while(|line| *line != "[modules]").skip(1) {
                if line.starts_with('[') {
                    break;
                }
                if let Some((module, _)) = line.split_once(" = ") {
                    let _ = writeln!(index, "{module} = \"{clutch}\"");
                }
            }
        }
        fs::write(repository.join("manifest.toml"), index).expect("write repository manifest");
        let library = home.join("lib/slug");
        fs::create_dir_all(&library).expect("create SLUG_HOME library");
        for module in ["builtin.slug", "channel.slug"] {
            fs::copy(
                workspace_root().join("lib/slug").join(module),
                library.join(module),
            )
            .expect("copy library module");
        }
        Self(home)
    }

    /// The command that runs `source`, with the prelude `run` uses.
    fn command(&self, source: &str) -> Command {
        let program = self.0.join("main.slug");
        fs::write(&program, format!("{PRELUDE}{source}")).expect("write program");
        let mut command = Command::new(env!("CARGO_BIN_EXE_slug"));
        command.env("SLUG_HOME", &self.0).arg(&program);
        command
    }

    fn run(&self, source: &str) -> Output {
        self.command(source).output().expect("run slug")
    }
}

/// A Slug server started from `source`, which prints its port on the first
/// line of its output.
struct Server {
    child: Child,
    port: u16,
    output: BufReader<std::process::ChildStdout>,
    _home: Home,
}

impl Server {
    fn start(source: &str) -> Self {
        let home = Home::new();
        let mut child = home
            .command(source)
            .stdout(Stdio::piped())
            .spawn()
            .expect("start slug server");
        let mut output = BufReader::new(child.stdout.take().expect("server output"));
        let mut line = String::new();
        output.read_line(&mut line).expect("read server port");
        let port = line
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("server port, got {line:?}"));
        Self {
            child,
            port,
            output,
            _home: home,
        }
    }

    fn connect(&self) -> TcpStream {
        let stream = TcpStream::connect(("127.0.0.1", self.port)).expect("connect to server");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("set read timeout");
        stream
    }

    /// The rest of the server's output, once it has exited successfully.
    fn finish(mut self) -> String {
        let mut rest = String::new();
        self.output
            .read_to_string(&mut rest)
            .expect("read server output");
        let status = self.child.wait().expect("wait for server");
        assert!(status.success(), "server failed: {status}\n{rest}");
        rest
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Runs `source` after `PRELUDE` and returns its standard output, failing the
/// test when it does not succeed.
fn run(source: &str) -> String {
    let home = Home::new();
    let output = home.run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Imports the module and defines `failure(work)`, the type of the error
/// `work` throws, and `repeat(text, times)`.
const PRELUDE: &str = "val tcp = import(\"slug.net.tcp\")\n\
         val {await} = import(\"slug.channel\")\n\
         val {fromBytes} = import(\"slug.string\")\n\
         // `text` doubled `times` times.\n\
         val repeat = fn(text:str, times:num):str {\n\
             if (times == 0) { text } else { recur(text + text, times - 1) }\n\
         }\n\
         val failure = fn(work:fn):str {\n\
             defer onerror(error) { return error.type }\n\
             work()\n\
             \"no error\"\n\
         }\n";

// Serving and connecting in one program needs `spawn`.
#[cfg(feature = "concurrency")]
#[test]
fn a_program_serves_and_connects_to_itself() {
    assert_eq!(
        run("val listener = tcp.listen(\"127.0.0.1\", 0)\n\
             val port = tcp.localPort(listener)\n\
             println(port > 0 && port < 65536)\n\
             val server = spawn {\n\
                 val connection = tcp.accept(listener)\n\
                 if (connection != nil) { tcp.close(connection) }\n\
                 connection != nil\n\
             }\n\
             val client = tcp.connect(\"127.0.0.1\", port)\n\
             tcp.close(client)\n\
             println(await(server))\n\
             tcp.close(listener)\n\
             println(tcp.accept(listener))\n"),
        "true\ntrue\nnil\n"
    );
}

// Serving and connecting in one program needs `spawn`.
#[cfg(feature = "concurrency")]
#[test]
fn one_listener_accepts_many_connections_in_order_of_arrival() {
    assert_eq!(
        run("val listener = tcp.listen(\"localhost\", 0)\n\
             val port = tcp.localPort(listener)\n\
             val server = spawn {\n\
                 val loop = fn(count:num):num {\n\
                     if (count == 25) { return count }\n\
                     match tcp.accept(listener) {\n\
                         nil => count\n\
                         connection => { tcp.close(connection); recur(count + 1) }\n\
                     }\n\
                 }\n\
                 loop(0)\n\
             }\n\
             val connect = fn(i:num) {\n\
                 if (i < 25) { tcp.close(tcp.connect(\"localhost\", port)); recur(i + 1) }\n\
             }\n\
             connect(0)\n\
             println(await(server))\n\
             tcp.close(listener)\n"),
        "25\n"
    );
}

#[test]
fn reports_refused_connections_bad_addresses_and_ports() {
    assert_eq!(
        run("val listener = tcp.listen(\"127.0.0.1\", 0)\n\
             val port = tcp.localPort(listener)\n\
             tcp.close(listener)\n\
             println(failure(fn() { tcp.connect(\"127.0.0.1\", port) }))\n\
             println(failure(fn() { tcp.connect(\"127.0.0.1\", 70000) }))\n\
             println(failure(fn() { tcp.listen(\"127.0.0.1\", 1.5) }))\n\
             println(failure(fn() { tcp.listen(\"not an address\", 0) }))\n\
             println(failure(fn() { tcp.connect(\"no-such-host.invalid\", 80, timeout = 2000) }))\n"),
        "net.refused\nnet.address\nnet.address\nnet.address\nnet.address\n"
    );
}

// Serving and connecting in one program needs `spawn`.
#[cfg(feature = "concurrency")]
#[test]
fn accept_times_out_and_closing_the_listener_wakes_a_waiting_accept() {
    assert_eq!(
        run("val listener = tcp.listen(\"127.0.0.1\", 0)\n\
             println(failure(fn() { tcp.accept(listener, timeout = 50) }))\n\
             val waiting = spawn { tcp.accept(listener) }\n\
             select { after 50 /> fn(_) { nil } }\n\
             tcp.close(listener)\n\
             println(await(waiting))\n\
             // Closing twice does nothing.\n\
             tcp.close(listener)\n\
             println(failure(fn() { tcp.localPort(listener) }))\n"),
        "net.timeout\nnil\nnative.resource_closed\n"
    );
}

// Serving and connecting in one program needs `spawn`.
#[cfg(feature = "concurrency")]
#[test]
fn listens_and_connects_over_ipv6() {
    assert_eq!(
        run("val listener = tcp.listen(\"::1\", 0)\n\
             val server = spawn { tcp.accept(listener) != nil }\n\
             tcp.close(tcp.connect(\"::1\", tcp.localPort(listener)))\n\
             println(await(server))\n\
             tcp.close(listener)\n"),
        "true\n"
    );
}

#[cfg(feature = "concurrency")]
#[test]
fn echoes_a_mebibyte_across_many_chunks() {
    assert_eq!(
        run("val listener = tcp.listen(\"127.0.0.1\", 0)\n\
             val server = spawn {\n\
                 val connection = tcp.accept(listener)\n\
                 if (connection != nil) {\n\
                     defer tcp.close(connection)\n\
                     val echo = fn(chunks:num):num {\n\
                         match tcp.read(connection) {\n\
                             nil => chunks\n\
                             data => { tcp.write(connection, data); recur(chunks + 1) }\n\
                         }\n\
                     }\n\
                     echo(0)\n\
                 }\n\
             }\n\
             val client = tcp.connect(\"127.0.0.1\", tcp.localPort(listener))\n\
             val sent = repeat(\"0123456789abcdef\", 16)\n\
             val sender = spawn { tcp.write(client, sent); tcp.shutdownWrite(client) }\n\
             val collect = fn(received:bytes):bytes {\n\
                 match tcp.read(client, timeout = 5000) {\n\
                     nil => received\n\
                     data => recur(received + data)\n\
                 }\n\
             }\n\
             val received = collect(0x\"\")\n\
             await(sender)\n\
             println(len(received), fromBytes(received) == sent, (await(server) ?? 0) > 1)\n\
             tcp.close(client)\n\
             tcp.close(listener)\n"),
        "1048576 true true\n"
    );
}

#[test]
fn half_closes_and_reports_each_peer() {
    assert_eq!(
        run("val listener = tcp.listen(\"::1\", 0)\n\
             val client = tcp.connect(\"::1\", tcp.localPort(listener))\n\
             val server = tcp.accept(listener)\n\
             if (server != nil) {\n\
                 val port = tcp.localPort(listener)\n\
                 println(tcp.peer(client) == \"[::1]:$port\", tcp.peer(server) != tcp.peer(client))\n\
                 tcp.write(client, \"request\")\n\
                 tcp.shutdownWrite(client)\n\
                 println(fromBytes(tcp.read(server) ?? 0x\"\"), tcp.read(server))\n\
                 // The client stopped sending, but still reads.\n\
                 tcp.write(server, \"reply\")\n\
                 tcp.close(server)\n\
                 println(fromBytes(tcp.read(client) ?? 0x\"\"), tcp.read(client))\n\
             }\n\
             tcp.close(client)\n\
             tcp.close(listener)\n"),
        "true true\nrequest nil\nreply nil\n"
    );
}

#[cfg(feature = "concurrency")]
#[test]
fn read_times_out_and_closing_a_connection_ends_a_waiting_read() {
    assert_eq!(
        run("val listener = tcp.listen(\"127.0.0.1\", 0)\n\
             val client = tcp.connect(\"127.0.0.1\", tcp.localPort(listener))\n\
             val server = tcp.accept(listener)\n\
             if (server != nil) {\n\
                 println(failure(fn() { tcp.read(server, timeout = 50) }))\n\
                 val reader = spawn { tcp.read(server) }\n\
                 select { after 50 /> fn(_) { nil } }\n\
                 tcp.close(server)\n\
                 println(await(reader))\n\
             }\n\
             tcp.close(client)\n\
             tcp.close(listener)\n"),
        "net.timeout\nnil\n"
    );
}

#[test]
fn writing_to_a_closed_peer_fails_without_a_signal() {
    assert_eq!(
        run("val listener = tcp.listen(\"127.0.0.1\", 0)\n\
             val client = tcp.connect(\"127.0.0.1\", tcp.localPort(listener))\n\
             val server = tcp.accept(listener)\n\
             if (server != nil) { tcp.close(server) }\n\
             val chunk = repeat(\"x\", 16)\n\
             // The first writes may land in the buffers before the reset arrives.\n\
             val until = fn(tries:num):str {\n\
                 val outcome = failure(fn() { tcp.write(client, chunk) })\n\
                 if (outcome != \"no error\" || tries == 0) { outcome } else { recur(tries - 1) }\n\
             }\n\
             println(until(1000))\n\
             tcp.close(client)\n\
             tcp.close(listener)\n"),
        "net.error\n"
    );
}

#[test]
fn a_write_the_peer_does_not_take_times_out() {
    assert_eq!(
        run("val listener = tcp.listen(\"127.0.0.1\", 0)\n\
             val client = tcp.connect(\"127.0.0.1\", tcp.localPort(listener))\n\
             // The server never reads, so the buffers fill.\n\
             val server = tcp.accept(listener)\n\
             tcp.setWriteTimeout(client, 200)\n\
             println(failure(fn() { tcp.write(client, repeat(\"x\", 24)) }))\n\
             println(failure(fn() { tcp.setWriteTimeout(client, -1) }))\n\
             if (server != nil) { tcp.close(server) }\n\
             tcp.close(client)\n\
             tcp.close(listener)\n"),
        "net.timeout\nnet.error\n"
    );
}

#[cfg(feature = "concurrency")]
#[test]
fn a_peer_that_stops_reading_does_not_hold_up_other_connections() {
    assert_eq!(
        run("val listener = tcp.listen(\"127.0.0.1\", 0)\n\
             val port = tcp.localPort(listener)\n\
             val stalled = tcp.connect(\"127.0.0.1\", port)\n\
             val stalledServer = tcp.accept(listener)\n\
             val prompt = tcp.connect(\"127.0.0.1\", port)\n\
             val promptServer = tcp.accept(listener)\n\
             // A nil test narrows one binding at a time.\n\
             if (stalledServer != nil) { if (promptServer != nil) {\n\
                 tcp.setWriteTimeout(stalledServer, 1000)\n\
                 // This task waits for a peer that never reads, then times out.\n\
                 val flood = spawn { failure(fn() { tcp.write(stalledServer, repeat(\"x\", 24)) }) }\n\
                 tcp.write(prompt, \"ping\")\n\
                 println(fromBytes(tcp.read(promptServer, timeout = 500) ?? 0x\"\"))\n\
                 tcp.write(promptServer, \"pong\")\n\
                 println(fromBytes(tcp.read(prompt, timeout = 500) ?? 0x\"\"))\n\
                 println(await(flood))\n\
                 tcp.close(stalledServer)\n\
                 tcp.close(promptServer)\n\
             } }\n\
             tcp.close(stalled)\n\
             tcp.close(prompt)\n\
             tcp.close(listener)\n"),
        "ping\npong\nnet.timeout\n"
    );
}

#[cfg(feature = "concurrency")]
#[test]
fn closing_a_connection_first_sends_what_is_queued() {
    assert_eq!(
        run("val listener = tcp.listen(\"127.0.0.1\", 0)\n\
             val client = tcp.connect(\"127.0.0.1\", tcp.localPort(listener))\n\
             val server = tcp.accept(listener)\n\
             val sent = repeat(\"0123456789abcdef\", 18)\n\
             val sender = spawn {\n\
                 if (server != nil) { tcp.write(server, sent); tcp.close(server) }\n\
             }\n\
             val collect = fn(received:bytes):bytes {\n\
                 match tcp.read(client, timeout = 5000) {\n\
                     nil => received\n\
                     data => recur(received + data)\n\
                 }\n\
             }\n\
             val received = collect(0x\"\")\n\
             await(sender)\n\
             println(len(received), fromBytes(received) == sent)\n\
             tcp.close(client)\n\
             tcp.close(listener)\n"),
        "4194304 true\n"
    );
}

/// Reads from `stream` until end of stream.
#[cfg(feature = "concurrency")]
fn read_all(stream: &mut TcpStream) -> Vec<u8> {
    let mut received = Vec::new();
    stream
        .read_to_end(&mut received)
        .expect("read to end of stream");
    received
}

#[cfg(feature = "concurrency")]
#[test]
fn serves_200_concurrent_clients_each_their_own_data() {
    use std::net::Shutdown;

    let server = Server::start(
        "val listener = tcp.listen(\"127.0.0.1\", 0)\n\
         println(tcp.localPort(listener))\n\
         val serve = fn(connection:tcp.Connection) {\n\
             defer tcp.close(connection)\n\
             val echo = fn() {\n\
                 match tcp.read(connection) {\n\
                     nil => nil\n\
                     data => { tcp.write(connection, data); recur() }\n\
                 }\n\
             }\n\
             echo()\n\
         }\n\
         val loop = fn(count:num, tasks:list):list {\n\
             if (count == 200) { return tasks }\n\
             match tcp.accept(listener) {\n\
                 nil => tasks\n\
                 connection => recur(count + 1, tasks :+ spawn { serve(connection) })\n\
             }\n\
         }\n\
         val tasks = loop(0, [])\n\
         val join = fn(i:num) { if (i < len(tasks)) { await(tasks[i]); recur(i + 1) } }\n\
         join(0)\n\
         tcp.close(listener)\n\
         println(len(tasks))\n",
    );
    let port = server.port;
    let clients = (0..200)
        .map(|client: u32| {
            thread::spawn(move || {
                let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
                stream
                    .set_read_timeout(Some(Duration::from_secs(20)))
                    .expect("set read timeout");
                // Each client's data differs, and spans several chunks.
                let sent: Vec<u8> = (0..40_000)
                    .map(|i| ((i * 7 + client) % 251) as u8)
                    .collect();
                stream.write_all(&sent).expect("send");
                stream.shutdown(Shutdown::Write).expect("shut down sending");
                read_all(&mut stream) == sent
            })
        })
        .collect::<Vec<_>>();
    // A client that panicked counts as not echoed.
    let echoed = clients
        .into_iter()
        .map(|client| client.join().unwrap_or(false))
        .filter(|echoed| *echoed)
        .count();
    assert_eq!(echoed, 200);
    assert_eq!(server.finish(), "200\n");
}

#[test]
fn a_peer_reset_is_a_read_error() {
    let server = Server::start(
        "val listener = tcp.listen(\"127.0.0.1\", 0)\n\
         println(tcp.localPort(listener))\n\
         val connection = tcp.accept(listener)\n\
         if (connection != nil) {\n\
             tcp.write(connection, repeat(\"x\", 16))\n\
             println(failure(fn() {\n\
                 val drain = fn() { if (tcp.read(connection) != nil) { recur() } }\n\
                 drain()\n\
             }))\n\
             tcp.close(connection)\n\
         }\n\
         tcp.close(listener)\n",
    );
    let mut stream = server.connect();
    stream.write_all(b"hello").expect("send");
    // Closing with the server's data unread makes the system reset the
    // connection rather than end it.
    let mut first = [0; 1];
    stream
        .read_exact(&mut first)
        .expect("receive the first byte");
    thread::sleep(Duration::from_millis(100));
    drop(stream);
    assert_eq!(server.finish(), "net.error\n");
}

#[test]
fn exits_with_connections_and_a_listener_still_open() {
    let home = Home::new();
    let started = Instant::now();
    let output = home.run(
        "val listener = tcp.listen(\"127.0.0.1\", 0)\n\
         val client = tcp.connect(\"127.0.0.1\", tcp.localPort(listener))\n\
         val server = tcp.accept(listener)\n\
         tcp.write(client, \"never read\")\n\
         println(\"done\")\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "done\n");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn opening_and_closing_connections_releases_their_descriptors() {
    let home = Home::new();
    let command = home.command(
        "val listener = tcp.listen(\"127.0.0.1\", 0)\n\
         val port = tcp.localPort(listener)\n\
         val cycle = fn(i:num):num {\n\
             if (i == 400) { return i }\n\
             val client = tcp.connect(\"127.0.0.1\", port)\n\
             val server = tcp.accept(listener)\n\
             tcp.write(client, \"ping\")\n\
             if (server != nil) { tcp.read(server); tcp.close(server) }\n\
             tcp.close(client)\n\
             recur(i + 1)\n\
         }\n\
         println(cycle(0))\n\
         tcp.close(listener)\n",
    );
    // With 64 descriptors, a leak of one socket a cycle fails long before
    // the 400th.
    let program = command.get_args().next().expect("program path").to_owned();
    let output = Command::new("sh")
        .arg("-c")
        .arg("ulimit -n 64 && exec \"$0\" \"$1\"")
        .arg(env!("CARGO_BIN_EXE_slug"))
        .arg(program)
        .envs(
            command
                .get_envs()
                .filter_map(|(key, value)| Some((key, value?))),
        )
        .output()
        .expect("run slug with few descriptors");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "400\n");
}

#[cfg(feature = "concurrency")]
#[test]
fn a_slow_reader_bounds_what_a_fast_peer_can_send() {
    use std::net::Shutdown;

    let server = Server::start(
        "val listener = tcp.listen(\"127.0.0.1\", 0)\n\
         println(tcp.localPort(listener))\n\
         val connection = tcp.accept(listener)\n\
         if (connection != nil) {\n\
             // Read nothing for a while, then everything.\n\
             select { after 1500 /> fn(_) { nil } }\n\
             val count = fn(total:num):num {\n\
                 match tcp.read(connection) {\n\
                     nil => total\n\
                     data => recur(total + len(data))\n\
                 }\n\
             }\n\
             println(count(0))\n\
             tcp.close(connection)\n\
         }\n\
         tcp.close(listener)\n",
    );
    let mut stream = server.connect();
    let total = 64 << 20;
    let chunk = vec![7_u8; 1 << 16];
    // While the server is not reading, only its 16 chunks and the system's
    // buffers can take what is sent.
    stream.set_nonblocking(true).expect("nonblocking");
    let mut buffered = 0;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(1) {
        match stream.write(&chunk) {
            Ok(written) => buffered += written,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("send: {error}"),
        }
    }
    assert!(
        buffered < 16 << 20,
        "the server buffered {buffered} bytes unread"
    );
    stream.set_nonblocking(false).expect("blocking");
    let mut sent = buffered;
    while sent < total {
        let length = chunk.len().min(total - sent);
        stream.write_all(&chunk[..length]).expect("send the rest");
        sent += length;
    }
    stream.shutdown(Shutdown::Write).expect("shut down sending");
    assert_eq!(server.finish(), format!("{total}\n"));
}
