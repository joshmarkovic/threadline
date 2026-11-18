fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS");
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV");
    if target_os.as_deref() == Ok("windows") && target_env.as_deref() == Ok("msvc") {
        // GPUI composition needs more stack headroom than the Windows linker default.
        // Reserve address space for the executable while retaining demand-committed pages.
        println!("cargo::rustc-link-arg-bin=threadline=/STACK:8388608");
    }
}
