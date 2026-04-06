fn main() {
    println!("cargo::rustc-check-cfg=cfg(imads_has_threads)");

    let target = std::env::var("TARGET").unwrap_or_default();
    let has_threads = !target.starts_with("wasm32")
        || target == "wasm32-wasip1-threads"
        || target.starts_with("wasm32-wasip3");
    if has_threads {
        println!("cargo:rustc-cfg=imads_has_threads");
    }
}
