//! PRODUCER — publishes an animating CALayer into a private CAContext and
//! prints the context id so a separate process can host it via CALayerHost.
//!
//! No Metal here: the layer's `backgroundColor` is driven by a repeating
//! `CABasicAnimation`. CoreAnimation evaluates that animation in the render
//! server (WindowServer), so the color keeps cycling *without* our process
//! doing per-frame work — and, crucially, the motion is visible across the
//! CAContext/CALayerHost boundary in the consumer process.
//!
//! macOS only.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{class, msg_send};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_core_graphics::CGColor;
use objc2_foundation::{MainThreadMarker, NSDictionary, NSString};
use objc2_quartz_core::{CABasicAnimation, CALayer, CAMediaTiming};

// ---------------------------------------------------------------------------
// PRIVATE API BINDINGS
// ---------------------------------------------------------------------------

extern "C" {
    /// Private CoreGraphics symbol. Returns the main `CGSConnectionID`
    /// (a.k.a. `_CGSDefaultConnection`) for this process. Lives in
    /// CoreGraphics / ApplicationServices; linked via build.rs.
    fn CGSMainConnectionID() -> u32;
}

/// Thin wrapper around the private `CAContext` class (QuartzCore).
///
/// `CAContext` is the server-side handle that owns a layer tree and exposes it
/// for remote hosting. We construct one with `+contextWithCGSConnection:options:`
/// (the variant Safari/Chrome use), attach our root layer with `-setLayer:`,
/// and read its id with `-contextId` (a `u32`).
struct CAContext {
    obj: Retained<AnyObject>,
}

impl CAContext {
    /// `+[CAContext contextWithCGSConnection:options:]`
    ///
    /// - `connection`: a `CGSConnectionID` (`u32`).
    /// - `options`: an `NSDictionary` (empty is fine).
    ///
    /// Returns an autoreleased CAContext, which we retain for the layer's
    /// lifetime.
    fn with_cgs_connection(connection: u32, options: &NSDictionary) -> Self {
        let cls = class!(CAContext);
        // +contextWithCGSConnection:options: returns an autoreleased instance;
        // msg_send! with a Retained<_> return type retains it for us.
        let obj: Retained<AnyObject> = unsafe {
            msg_send![cls, contextWithCGSConnection: connection, options: options]
        };
        Self { obj }
    }

    /// `-[CAContext setLayer:]` — attach the published layer tree root.
    fn set_layer(&self, layer: &CALayer) {
        unsafe { msg_send![&*self.obj, setLayer: layer] }
    }

    /// `-[CAContext contextId]` -> `u32`. This is the token the consumer needs.
    fn context_id(&self) -> u32 {
        unsafe { msg_send![&*self.obj, contextId] }
    }
}

// ---------------------------------------------------------------------------

fn main() {
    // CAContext / CALayer live on the main thread's run loop; this binary is
    // main-thread only.
    let mtm = MainThreadMarker::new()
        .expect("producer must run on the main thread");

    // Bring up a shared NSApplication so there is a CFRunLoop + the AppKit /
    // CoreAnimation machinery is initialized. `.run()` at the end keeps the
    // run loop spinning so the layer stays alive and the animation keeps
    // playing.
    let app = NSApplication::sharedApplication(mtm);
    // Accessory: no Dock icon / menu bar needed for a headless publisher.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    // --- Root layer ---------------------------------------------------------
    let root: Retained<CALayer> = CALayer::new();
    // Give it a concrete size; CALayerHost in the consumer will size itself to
    // the host view, but a non-zero bounds here is good hygiene.
    root.setBounds(objc2_foundation::NSRect::new(
        objc2_foundation::NSPoint::new(0.0, 0.0),
        objc2_foundation::NSSize::new(512.0, 512.0),
    ));
    // Start opaque red so there's something on screen before / between cycles.
    let red = CGColor::new_srgb(1.0, 0.0, 0.0, 1.0);
    root.setBackgroundColor(Some(&red));

    // --- Animate backgroundColor across the full color wheel ----------------
    // A keyed `values` animation would need a CAKeyframeAnimation; for a
    // dead-simple, obviously-moving proof we use a CABasicAnimation that
    // ping-pongs red <-> blue forever. (red->blue->red is plainly visible and
    // needs no keyframe array boxing.)
    let key_path = NSString::from_str("backgroundColor");
    let anim: Retained<CABasicAnimation> =
        CABasicAnimation::animationWithKeyPath(Some(&key_path));

    let from_color = CGColor::new_srgb(1.0, 0.0, 0.0, 1.0); // red
    let to_color = CGColor::new_srgb(0.0, 0.4, 1.0, 1.0); // blue

    // CGColor is a CF type and is accepted directly as the animation value
    // (CoreAnimation special-cases CGColorRef for backgroundColor). Cast the
    // CFRetained<CGColor> to an AnyObject* for -setFromValue: / -setToValue:.
    unsafe {
        let from_obj = &*(cf_color_as_id(&from_color) as *const AnyObject);
        let to_obj = &*(cf_color_as_id(&to_color) as *const AnyObject);
        anim.setFromValue(Some(from_obj));
        anim.setToValue(Some(to_obj));
    }
    // CAMediaTiming protocol methods:
    anim.setDuration(1.0);
    anim.setAutoreverses(true); // red->blue->red
    anim.setRepeatCount(f32::INFINITY); // forever
    // Add it under a key so it persists on the layer.
    let anim_key = NSString::from_str("colorCycle");
    let anim_as_ca: &objc2_quartz_core::CAAnimation = &anim;
    root.addAnimation_forKey(anim_as_ca, Some(&anim_key));

    // --- Publish via the private CAContext ----------------------------------
    let connection = unsafe { CGSMainConnectionID() };
    let options = NSDictionary::new(); // empty options
    let ctx = CAContext::with_cgs_connection(connection, &options);
    ctx.set_layer(&root);

    // The token the consumer needs. Print it clearly and flush.
    println!("CONTEXT_ID={}", ctx.context_id());
    use std::io::Write as _;
    let _ = std::io::stdout().flush();

    // Keep CAContext + root layer alive for the whole process lifetime. We move
    // them into a leaked box so nothing drops them while the run loop runs.
    // (Throwaway PoC: leaking on a process that runs forever is fine.)
    let _keepalive: &'static Keepalive = Box::leak(Box::new(Keepalive {
        _ctx: ctx,
        _root: root,
        _anim: anim,
    }));

    eprintln!("[producer] running — layer is animating; Ctrl-C to quit.");
    // Spin the run loop forever so CoreAnimation keeps rendering the layer and
    // the CAContext stays serviced.
    app.run();
}

/// Holds the published objects alive for the process lifetime.
struct Keepalive {
    _ctx: CAContext,
    _root: Retained<CALayer>,
    _anim: Retained<CABasicAnimation>,
}

/// Helper: turn a `CFRetained<CGColor>` into a raw pointer usable as an
/// Objective-C `id`. `CFRetained<T>` derefs to `&T`, and a `CGColorRef` IS a
/// valid `id` for CoreAnimation's value APIs.
fn cf_color_as_id(color: &objc2_core_foundation::CFRetained<CGColor>) -> *const NSObject {
    let r: &CGColor = color;
    (r as *const CGColor).cast()
}
