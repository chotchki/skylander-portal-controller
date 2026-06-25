//! PRODUCER_METAL — like `producer`, but the published root layer is a
//! **`CAMetalLayer`** that WE render into with Metal every frame, instead of a
//! vanilla `CALayer` whose `backgroundColor` is animated server-side by
//! CoreAnimation.
//!
//! WHY THIS EXISTS
//! ---------------
//! `producer.rs` proves a vanilla CALayer (CoreAnimation-driven) crosses the
//! CAContext -> CALayerHost boundary fine, because WindowServer evaluates the
//! animation server-side. The real target (RPCS3 on macOS) publishes a
//! `CAMetalLayer` — and the hosted layer shows BLANK. This binary reproduces
//! the Metal case in the SIMPLEST possible form to isolate the variable.
//!
//! CRITICAL TEST DESIGN: the `CAMetalLayer` here is **standalone** — it is the
//! CAContext's root layer and nothing else. It is NOT attached to any
//! NSView/NSWindow backing-layer. RPCS3's real Metal layer IS its `gs_frame`
//! NSView's backing layer. So:
//!   - if THIS standalone Metal layer crosses the boundary fine, the real-world
//!     blank is caused by the NSView-parenting (fix: a dedicated CAMetalLayer
//!     render target, off-view);
//!   - if THIS is also blank, CAContext-of-a-CAMetalLayer is fundamentally
//!     insufficient and the fix is to publish the drawable's IOSurface.
//!
//! Frame driver: an `NSTimer` scheduled on the main run loop fires ~60 Hz and
//! renders one Metal frame each tick (clear to an animated hue, present). The
//! run loop is kept alive by `NSApplication::run()`, so the timer keeps firing
//! and the CAContext stays serviced. Main-thread-only, like `producer`.
//!
//! macOS only.

use std::cell::Cell;
use std::ptr::NonNull;
use std::time::Instant;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::{MainThreadMarker, NSDictionary, NSTimer};
use objc2_metal::{
    MTLClearColor, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue,
    MTLCreateSystemDefaultDevice, MTLDevice, MTLLoadAction, MTLPixelFormat,
    MTLRenderPassDescriptor, MTLStoreAction,
};
use objc2_quartz_core::{CALayer, CAMetalDrawable, CAMetalLayer};

use block2::RcBlock;
use objc2::runtime::ProtocolObject;

// ---------------------------------------------------------------------------
// PRIVATE API BINDINGS  (identical to producer.rs)
// ---------------------------------------------------------------------------

extern "C" {
    /// Private CoreGraphics symbol — main `CGSConnectionID` for this process.
    fn CGSMainConnectionID() -> u32;
}

/// Thin wrapper around the private `CAContext` class (QuartzCore). Same binding
/// as `producer.rs`: `+contextWithCGSConnection:options:`, `-setLayer:`,
/// `-contextId`.
struct CAContext {
    obj: Retained<AnyObject>,
}

impl CAContext {
    fn with_cgs_connection(connection: u32, options: &NSDictionary) -> Self {
        let cls = class!(CAContext);
        let obj: Retained<AnyObject> = unsafe {
            msg_send![cls, contextWithCGSConnection: connection, options: options]
        };
        Self { obj }
    }

    /// `-[CAContext setLayer:]`. A `CAMetalLayer` IS-A `CALayer`, so we take it
    /// as `&CALayer` here.
    fn set_layer(&self, layer: &CALayer) {
        unsafe { msg_send![&*self.obj, setLayer: layer] }
    }

    fn context_id(&self) -> u32 {
        unsafe { msg_send![&*self.obj, contextId] }
    }
}

// ---------------------------------------------------------------------------
// Per-frame Metal render state (kept alive for the process lifetime).
// ---------------------------------------------------------------------------

/// Everything the timer block needs to render one frame.
struct RenderState {
    layer: Retained<CAMetalLayer>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    start: Instant,
    frames: Cell<u64>,
}

impl RenderState {
    /// Render a single frame: clear the drawable to an animated hue and present.
    /// No vertex pipeline — a clear is enough to prove Metal content crosses.
    fn render_frame(&self) {
        // Acquire the next drawable from the CAMetalLayer.
        let drawable: Retained<ProtocolObject<dyn CAMetalDrawable>> =
            match self.layer.nextDrawable() {
                Some(d) => d,
                None => {
                    eprintln!("[producer_metal] nextDrawable returned nil; skipping frame");
                    return;
                }
            };
        let texture = drawable.texture();

        // Animated clear color: cycle hue over ~3s so motion is obvious.
        let t = self.start.elapsed().as_secs_f64();
        let (r, g, b) = hue_cycle(t / 3.0);
        let clear = MTLClearColor {
            red: r,
            green: g,
            blue: b,
            alpha: 1.0,
        };

        // Build a one-color-attachment render pass that clears to `clear`.
        let pass = MTLRenderPassDescriptor::renderPassDescriptor();
        let attachments = pass.colorAttachments();
        let color0 = unsafe { attachments.objectAtIndexedSubscript(0) };
        color0.setTexture(Some(&texture));
        color0.setLoadAction(MTLLoadAction::Clear);
        color0.setStoreAction(MTLStoreAction::Store);
        color0.setClearColor(clear);

        // command buffer -> render encoder (clear only) -> end -> present -> commit
        let cmd = match self.queue.commandBuffer() {
            Some(c) => c,
            None => {
                eprintln!("[producer_metal] commandBuffer returned nil; skipping frame");
                return;
            }
        };
        let encoder = match cmd.renderCommandEncoderWithDescriptor(&pass) {
            Some(e) => e,
            None => {
                eprintln!("[producer_metal] renderCommandEncoder returned nil; skipping frame");
                return;
            }
        };
        // The Clear loadAction already paints the whole attachment; just end.
        encoder.endEncoding();

        // CAMetalDrawable IS-A MTLDrawable; present it on this command buffer.
        let mtl_drawable: &ProtocolObject<dyn objc2_metal::MTLDrawable> =
            ProtocolObject::from_ref(&*drawable);
        cmd.presentDrawable(mtl_drawable);
        cmd.commit();

        let n = self.frames.get() + 1;
        self.frames.set(n);
        if n % 60 == 0 {
            eprintln!("[producer_metal] rendered {n} frames (hue r={r:.2} g={g:.2} b={b:.2})");
        }
    }
}

/// Map a phase in [0,1) (wrapping) to an RGB triple sweeping the hue wheel at
/// full saturation/value. Cheap, branchy, good enough for an obvious cycle.
fn hue_cycle(phase: f64) -> (f64, f64, f64) {
    let h = (phase.fract() + 1.0).fract() * 6.0; // [0,6)
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    }
}

// ---------------------------------------------------------------------------

fn main() {
    let mtm = MainThreadMarker::new().expect("producer_metal must run on the main thread");

    // Shared NSApplication => a CFRunLoop the NSTimer can schedule onto, plus
    // initialized AppKit/CoreAnimation/Metal machinery.
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    // --- Metal device + queue -----------------------------------------------
    let device: Retained<ProtocolObject<dyn MTLDevice>> =
        MTLCreateSystemDefaultDevice().expect("no system default Metal device");
    eprintln!("[producer_metal] Metal device: {}", device.name());
    let queue = device
        .newCommandQueue()
        .expect("failed to create Metal command queue");

    // --- CAMetalLayer (STANDALONE — not a view backing layer) ---------------
    let layer: Retained<CAMetalLayer> = CAMetalLayer::new();
    layer.setDevice(Some(&device));
    layer.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
    layer.setFramebufferOnly(true);
    layer.setBounds(objc2_foundation::NSRect::new(
        objc2_foundation::NSPoint::new(0.0, 0.0),
        objc2_foundation::NSSize::new(512.0, 512.0),
    ));
    layer.setDrawableSize(objc2_foundation::NSSize::new(512.0, 512.0));

    // --- Publish via the private CAContext (SAME path as producer.rs) -------
    let connection = unsafe { CGSMainConnectionID() };
    let options = NSDictionary::new();
    let ctx = CAContext::with_cgs_connection(connection, &options);
    ctx.set_layer(&layer);

    println!("CONTEXT_ID={}", ctx.context_id());
    use std::io::Write as _;
    let _ = std::io::stdout().flush();

    // --- Per-frame render state ---------------------------------------------
    let state = Box::new(RenderState {
        layer,
        queue,
        start: Instant::now(),
        frames: Cell::new(0),
    });

    // Render an immediate first frame so there's content before the first tick.
    state.render_frame();

    // --- Drive frames with a main-runloop NSTimer (~60 Hz) ------------------
    // The block borrows `state`; we leak `state` so the borrow is 'static-safe
    // for the process lifetime (throwaway PoC — leaking a forever-process is
    // fine, mirroring producer.rs's Box::leak keepalive).
    let state: &'static RenderState = Box::leak(state);
    let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
        state.render_frame();
    });
    let timer = unsafe {
        NSTimer::scheduledTimerWithTimeInterval_repeats_block(
            1.0 / 60.0, // ~60 fps
            true,       // repeats
            &block,
        )
    };

    // Keep the CAContext + timer (+ implicitly the leaked state) alive forever.
    let _keepalive: &'static Keepalive = Box::leak(Box::new(Keepalive {
        _ctx: ctx,
        _timer: timer,
    }));

    eprintln!(
        "[producer_metal] running — STANDALONE CAMetalLayer rendering ~60fps; \
         Ctrl-C to quit."
    );
    app.run();
}

/// Holds the published objects alive for the process lifetime.
struct Keepalive {
    _ctx: CAContext,
    _timer: Retained<NSTimer>,
}
