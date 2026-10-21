//! `slug.net.tcp` on typed native bindings, built as clutch staging builds
//! it. Every test serves and connects on loopback with port 0, so tests run
//! in parallel without colliding. Unix-only, as the clutch is.
#![cfg(unix)]

use std::{
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
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

    fn run(&self, source: &str) -> Output {
        let program = self.0.join("main.slug");
        fs::write(&program, source).expect("write program");
        Command::new(env!("CARGO_BIN_EXE_slug"))
            .env("SLUG_HOME", &self.0)
            .arg(&program)
            .output()
            .expect("run slug")
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Runs `source` after a prelude that imports the module and defines
/// `failure(work)`, the type of the error `work` throws, and returns its
/// standard output, failing the test when it does not succeed.
fn run(source: &str) -> String {
    let home = Home::new();
    let output = home.run(&format!(
        "val tcp = import(\"slug.net.tcp\")\n\
         val {{await}} = import(\"slug.channel\")\n\
         val {{fromBytes}} = import(\"slug.string\")\n\
         // `text` doubled `times` times.\n\
         val repeat = fn(text:str, times:num):str {{\n\
             if (times == 0) {{ text }} else {{ recur(text + text, times - 1) }}\n\
         }}\n\
         val failure = fn(work:fn):str {{\n\
             defer onerror(error) {{ return error.type }}\n\
             work()\n\
             \"no error\"\n\
         }}\n\
         {source}"
    ));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

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
