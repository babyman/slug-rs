//! Gives the executables' main thread the 8 MiB stack macOS and Linux already give it. Windows
//! reserves 1 MiB, which the type checker's recursion outgrows in debug builds. The CLI stays on
//! the main thread, rather than moving to a spawned thread with a larger stack, because native
//! clutches such as `slug.arcade` open windows, which macOS allows only on the main thread.

const STACK_BYTES: u32 = 8 * 1024 * 1024;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:{STACK_BYTES}");
    } else {
        println!("cargo:rustc-link-arg-bins=-Wl,--stack,{STACK_BYTES}");
    }
}
