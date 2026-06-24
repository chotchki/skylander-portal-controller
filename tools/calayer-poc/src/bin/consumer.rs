//! CONSUMER — hosts a remote layer tree (published by `producer`) inside its
//! own `winit` window via the private `CALayerHost` class.
//!
//! Usage: `consumer <CONTEXT_ID>`  (the id printed by the producer).
//!
//! macOS only.

use std::ffi::c_void;
use std::ptr::NonNull;

use objc2::rc::{Allocated, Retained};
use objc2::{class, msg_send, Message};
use objc2_app_kit::NSView;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use objc2_quartz_core::CALayer;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

// ---------------------------------------------------------------------------
// PRIVATE API BINDING
// ---------------------------------------------------------------------------

/// Thin wrapper over the private `CALayerHost` class (QuartzCore).
///
/// `CALayerHost` is a `CALayer` subclass that mirrors the layer tree published
/// by a `CAContext` in another process. You point it at a published tree with
/// `-setContextId:` (a `u32`); after that it renders that tree live, including
/// any CoreAnimation running on the remote side.
struct CALayerHost {
    /// Retained CALayerHost instance. We keep it as a `Retained<CALayer>`
    /// because CALayerHost IS-A CALayer, so all CALayer methods (setFrame:,
    /// addSublayer:, etc.) apply directly.
    layer: Retained<CALayer>,
}

impl CALayerHost {
    /// `[[CALayerHost alloc] init]`.
    fn new() -> Self {
        let cls = class!(CALayerHost);
        // alloc + init. CALayerHost IS-A CALayer, so we type the allocation as
        // Allocated<CALayer> and get back a Retained<CALayer> from -init.
        let layer: Retained<CALayer> = unsafe {
            let allocated: Allocated<CALayer> = msg_send![cls, alloc];
            msg_send![allocated, init]
        };
        Self { layer }
    }

    /// `-[CALayerHost setContextId:]` — bind to the producer's CAContext id.
    fn set_context_id(&self, context_id: u32) {
        unsafe { msg_send![&*self.layer, setContextId: context_id] }
    }

    /// `-[CALayerHost contextId]` -> `u32` (read-back, for sanity logging).
    fn context_id(&self) -> u32 {
        unsafe { msg_send![&*self.layer, contextId] }
    }

    /// As a plain CALayer (for addSublayer:, setFrame:).
    fn as_layer(&self) -> &CALayer {
        &self.layer
    }
}

// ---------------------------------------------------------------------------

struct App {
    context_id: u32,
    window: Option<Window>,
    /// Kept alive for the window's lifetime.
    host: Option<CALayerHost>,
}

impl App {
    fn new(context_id: u32) -> Self {
        Self {
            context_id,
            window: None,
            host: None,
        }
    }

    /// Pull the `NSView*` out of the winit window via raw-window-handle and
    /// re-wrap it as an objc2 0.6 `NSView`.
    ///
    /// We do NOT share objc2 types with winit (winit uses objc2 0.5) — we only
    /// take the raw pointer and bring it into our own objc2 0.6 world here.
    fn ns_view(window: &Window) -> Retained<NSView> {
        let handle = window
            .window_handle()
            .expect("no window handle")
            .as_raw();
        let ns_view_ptr: NonNull<c_void> = match handle {
            RawWindowHandle::AppKit(h) => h.ns_view,
            other => panic!("expected AppKit handle, got {other:?}"),
        };
        // The pointer is a `+0` borrowed `NSView*`. Retain it to get an owned
        // Retained<NSView> in our objc2 0.6 runtime.
        let view_ref: &NSView = unsafe { ns_view_ptr.cast::<NSView>().as_ref() };
        view_ref.retain()
    }

    fn install_host(&mut self) {
        let window = self.window.as_ref().expect("window not created");
        let view = Self::ns_view(window);

        // Make the view layer-backed and grab its backing CALayer.
        view.setWantsLayer(true);
        let view_layer: Retained<CALayer> = view
            .layer()
            .expect("view has no layer after setWantsLayer(true)");

        // Build the CALayerHost, bind it to the remote context, and parent it.
        let host = CALayerHost::new();
        host.set_context_id(self.context_id);
        eprintln!(
            "[consumer] CALayerHost bound to contextId={} (read-back={})",
            self.context_id,
            host.context_id()
        );

        view_layer.addSublayer(host.as_layer());
        self.size_host_to_view(&view, host.as_layer());

        self.host = Some(host);
    }

    /// Size the host layer to the view's bounds. (Swap `view.bounds()` for a
    /// centered sub-rect to prove positioning.)
    fn size_host_to_view(&self, view: &NSView, host_layer: &CALayer) {
        let bounds: NSRect = view.bounds();
        // Centered sub-rect (75% of the view) to *prove* positioning, with a
        // margin all around. Comment the next 3 lines + use `bounds` directly
        // to fill the whole window instead.
        let inset_w = bounds.size.width * 0.125;
        let inset_h = bounds.size.height * 0.125;
        let frame = NSRect::new(
            NSPoint::new(inset_w, inset_h),
            NSSize::new(
                bounds.size.width - 2.0 * inset_w,
                bounds.size.height - 2.0 * inset_h,
            ),
        );
        host_layer.setFrame(frame);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(format!("CALayerHost consumer — ctx {}", self.context_id))
            .with_inner_size(winit::dpi::LogicalSize::new(640.0, 480.0));
        let window = event_loop
            .create_window(attrs)
            .expect("failed to create window");
        self.window = Some(window);
        self.install_host();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) => {
                // Keep the host sized to the (possibly new) view bounds.
                if let (Some(window), Some(host)) = (self.window.as_ref(), self.host.as_ref()) {
                    let view = Self::ns_view(window);
                    self.size_host_to_view(&view, host.as_layer());
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let context_id: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("usage: consumer <CONTEXT_ID>  (the u32 the producer printed)");

    eprintln!("[consumer] hosting context id {context_id}");

    let event_loop = EventLoop::new().expect("failed to build event loop");
    let mut app = App::new(context_id);
    event_loop.run_app(&mut app).expect("event loop error");
}
