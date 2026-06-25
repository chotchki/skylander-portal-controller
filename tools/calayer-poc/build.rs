// Link the frameworks the private symbols live in.
//
// The objc2-* crates already emit link directives for most of these via their
// own build scripts, but we declare them explicitly so the private C symbols we
// `extern "C"` (CGSMainConnectionID) and the private classes (CAContext,
// CALayerHost) resolve at link time regardless of objc2's internal choices.
fn main() {
    // CGSMainConnectionID lives in CoreGraphics (re-exported through
    // ApplicationServices). CAContext / CALayerHost live in QuartzCore.
    println!("cargo:rustc-link-lib=framework=QuartzCore");
    println!("cargo:rustc-link-lib=framework=CoreGraphics");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=Foundation");
    // producer_metal: MTLCreateSystemDefaultDevice + Metal types live in Metal.
    println!("cargo:rustc-link-lib=framework=Metal");
}
