//! `slug.net.tcp` on typed native bindings, built as clutch staging builds
//! it. Every test serves and connects on loopback with port 0, so tests run
//! in parallel without colliding. Unix-only, as the clutch is.
#![cfg(unix)]

use std::{
    env, fs,
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

/// A `SLUG_HOME` with the TCP clutch, built from the repository's sources,
/// and the repository's `slug.builtin` and `slug.channel`.
struct Home(PathBuf);

impl Home {
    fn new() -> Self {
        let home = env::temp_dir().join(format!(
            "slug-typed-net-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        let source = workspace_root().join("clutch/slug.net.tcp.clutch");
        let repository = home.join("clutch");
        let root = repository.join("slug.net.tcp.clutch");
        for file in [
            "clutch.toml",
            "modules/tcp.slug",
            "native/source/tcp.c",
            "native/source/slug_net_tcp.h",
        ] {
            let target = root.join(file);
            fs::create_dir_all(target.parent().expect("clutch file has a parent"))
                .expect("create clutch directory");
            fs::copy(source.join(file), target).expect("copy TCP clutch file");
        }
        fs::write(
            repository.join("manifest.toml"),
            "[modules]\n\"slug.net.tcp\" = \"slug.net.tcp.clutch\"\n",
        )
        .expect("write repository manifest");
        let manifest = NativeManifest::from_clutch_source(
            &fs::read_to_string(root.join("clutch.toml")).expect("read TCP manifest"),
        )
        .expect("valid TCP manifest");
        build_dynamic(&root, &manifest, "slug_net_tcp", platform())
            .unwrap_or_else(|error| panic!("build TCP clutch: {error}"));
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
