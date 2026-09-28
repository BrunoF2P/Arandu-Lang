fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // The CLI parses and JIT-compiles user programs on its main thread.
        // Windows' default executable stack is only 1 MiB, which is too small
        // for ordinary nested source that fits on Linux and macOS stacks.
        println!("cargo:rustc-link-arg-bin=arandu_cli=/STACK:8388608");
    }
}
