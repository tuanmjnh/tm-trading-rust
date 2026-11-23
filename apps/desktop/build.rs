// Configures linker flags for tm-trading-desktop
fn main() {
    // Windows defaults to a 1 MB stack. Dioxus desktop + WebView2 initialization
    // requires a larger stack (8 MB) on Windows MSVC / GNU.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            println!("cargo:rustc-link-arg-bins=/STACK:8388608");
        } else {
            println!("cargo:rustc-link-arg-bins=-Wl,--stack,8388608");
        }
    }
}
