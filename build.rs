fn main() {
    let is_macos = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos");
    let stream_broadcast_enabled = std::env::var_os("CARGO_FEATURE_STREAM_BROADCAST").is_some();

    if is_macos && stream_broadcast_enabled {
        println!("cargo::rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
