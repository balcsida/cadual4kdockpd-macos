// The Windows window build (`--features window`) gets its manifest: themed controls and per-monitor
// DPI. Only the MSVC linker can embed it this way; a cross build from a Mac runs without it.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=packaging/windows.manifest");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc");
    if msvc && std::env::var_os("CARGO_FEATURE_WINDOW").is_some() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging").join("windows.manifest");
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
    }
}
