//! On macOS, emit a Swift-runtime rpath so the `screencapturekit` swift-bridge
//! dylibs (`libswift_Concurrency`, …) resolve at launch (PLAN A.1 capture
//! backend). The crate itself does not set it, so a bare `cargo run` otherwise
//! `dyld`-fails with "Library not loaded: @rpath/libswift_Concurrency.dylib".
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
