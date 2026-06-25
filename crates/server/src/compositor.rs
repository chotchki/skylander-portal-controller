//! macOS surface-embed (P8 / Phase C): host the running RPCS3 game's render
//! layer **inside** our egui launcher window via the private `CALayerHost`
//! (CARemoteLayer) mechanism — instead of choreographing a second top-level
//! window and tiling it beneath us (the P7 `WINDOW_SET` path, kept as the
//! fallback).
//!
//! The driver publishes the game's `CAContextID` + native size over IPC
//! (`PortalDriver::game_surface`); a `CAContextID` is stable for the whole game
//! session (it survives swapchain recreate / resize / resolution change), so we
//! host it **once** and never re-fetch/re-publish. The native size feeds the
//! scale transform that fits the hosted layer to the launcher's pane —
//! `CALayerHost` renders the remote tree at native size, not the host's bounds.
//!
//! Cross-process layer sharing was proven on this Mac by the throwaway POC at
//! `tools/calayer-poc/`; the objc2 incantation here mirrors its `consumer.rs`.
//!
//! Non-macOS targets get a no-op stub so the server still builds everywhere.

// ---------------------------------------------------------------------------
// macOS implementation
// ---------------------------------------------------------------------------
#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::c_void;
    use std::ptr::NonNull;

    use objc2::rc::{Allocated, Retained};
    use objc2::{Message, class, msg_send};
    use objc2_app_kit::NSView;
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use objc2_quartz_core::{CALayer, CATransaction, CATransform3D};

    /// Owns the `CALayerHost` sublayer that mirrors the game's published render
    /// layer tree inside the launcher's view. Lives on `LauncherApp` (macOS
    /// only); `attach` is idempotent so `update()` can call it every frame.
    pub struct CompositorHost {
        /// The view we parented the host layer into (retained for `detach` +
        /// re-frame). `None` until the first successful `attach`.
        view: Option<Retained<NSView>>,
        /// The `CALayerHost` instance, typed as `Retained<CALayer>` because
        /// `CALayerHost` IS-A `CALayer` — every `CALayer` method (`setFrame:`,
        /// `removeFromSuperlayer`, …) applies directly. `None` until attached.
        host_layer: Option<Retained<CALayer>>,
        /// The `CAContextID` we're currently hosting. Guards `attach` against
        /// redundant re-parenting — re-attaching the same id is a no-op.
        context_id: Option<u32>,
        /// The hosted surface's NATIVE size in points (from the SURFACE reply).
        /// `CALayerHost` renders the remote tree at this size, anchored — it does
        /// NOT scale to the host's bounds. `set_frame` reads this to compute the
        /// scale transform that fits the game to its pane. `None` until attached
        /// (or when the emulator didn't report a size → host unscaled).
        native_size: Option<(f64, f64)>,
        /// Last fit (`pane_w, pane_h, native_w, native_h`, rounded) we logged —
        /// so `set_frame` emits one diagnostic line per *change* instead of one
        /// per frame (the fit only changes on launcher resize / re-attach).
        last_fit: Option<(i64, i64, i64, i64)>,
    }

    impl CompositorHost {
        /// Empty host — nothing parented yet.
        pub fn new() -> Self {
            Self {
                view: None,
                host_layer: None,
                context_id: None,
                native_size: None,
                last_fit: None,
            }
        }

        /// `true` once a `CALayerHost` is parented for `context_id`.
        pub fn is_attached_to(&self, context_id: u32) -> bool {
            self.context_id == Some(context_id)
        }

        /// Host the game's published layer tree (`context_id`) inside `ns_view`,
        /// once. Idempotent: a no-op if already attached to this same
        /// `context_id`. `ns_view` is the launcher window's `NSView*` (a `+0`
        /// borrowed pointer pulled from raw-window-handle by the caller).
        ///
        /// `native_w`/`native_h` are the hosted surface's native size in points
        /// (from the SURFACE reply); `set_frame` uses them to scale the hosted
        /// layer to the launcher's pane. Pass `0.0` when unknown (the host then
        /// renders unscaled, clipped to the pane).
        ///
        /// # Safety
        /// `ns_view` must be a valid, live `NSView*` (e.g. eframe's owned window
        /// handle for the current frame). We retain it, so it stays valid past
        /// this call.
        pub unsafe fn attach(
            &mut self,
            ns_view: *mut c_void,
            context_id: u32,
            native_w: f64,
            native_h: f64,
        ) {
            // Already hosting this context — nothing to do (and we must NOT
            // re-create the host: the contextId is stable for the session).
            if self.context_id == Some(context_id) {
                return;
            }
            // Switching to a different context (shouldn't happen mid-session, but
            // be safe): tear the old host down first.
            if self.host_layer.is_some() {
                self.detach();
            }

            let Some(ptr) = NonNull::new(ns_view.cast::<NSView>()) else {
                return;
            };
            // The pointer is a `+0` borrow; retain it into an owned objc2 0.6
            // `Retained<NSView>` (winit/eframe use objc2 0.5 — we never share
            // objc2 *types* with them, only this raw pointer).
            let view: Retained<NSView> = unsafe { ptr.as_ref().retain() };

            // Make the launcher view layer-backed and grab its backing layer.
            view.setWantsLayer(true);
            let Some(view_layer) = view.layer() else {
                // No layer even after setWantsLayer(true) — bail without state
                // change so a later frame can retry.
                return;
            };

            // Build the CALayerHost and bind it to the game's CAContext.
            // `[[CALayerHost alloc] init]`. CALayerHost IS-A CALayer.
            let host_layer: Retained<CALayer> = unsafe {
                let allocated: Allocated<CALayer> = msg_send![class!(CALayerHost), alloc];
                msg_send![allocated, init]
            };
            // `-[CALayerHost setContextId:]` — the private selector that points
            // this host at the producer's published layer tree. (`let () =` pins
            // the void return so msg_send! doesn't try to infer it.)
            unsafe {
                let () = msg_send![&*host_layer, setContextId: context_id];
            }

            // ---------------------------------------------------------------
            // Z-ORDER — LIVE-TWEAK SPOT #1.
            //
            // The launcher window is transparent (with_transparent(true) +
            // clear_color [0,0,0,0]) and egui draws its chrome via glow/GL. The
            // hosted game layer MUST sit BELOW egui's GL content so the bezel /
            // chrome draws OVER the game and the transparent areas reveal it.
            //
            // We insert the host at sublayer index 0 (the bottom of the view
            // layer's sublayer stack) rather than `addSublayer` (which appends
            // to the top). If live testing shows the game still painting over
            // egui, the alternative lever is `host_layer.setZPosition(-1.0)`.
            // The human confirms which wins live.
            view_layer.insertSublayer_atIndex(&host_layer, 0);

            self.view = Some(view);
            self.host_layer = Some(host_layer);
            self.context_id = Some(context_id);
            self.native_size = (native_w > 0.0 && native_h > 0.0).then_some((native_w, native_h));

            tracing::info!(
                context_id,
                native_w,
                native_h,
                "CompositorHost: CALayerHost attached (game surface hosted in launcher view)"
            );
        }

        /// Position + size the hosted game layer to the target pane within the
        /// launcher's view, in the view's layer coordinate space.
        ///
        /// `(x, y)` is the pane origin (top-left, egui convention) and `(w, h)`
        /// its size, in points (egui points == AppKit points).
        ///
        /// `CALayerHost` renders the hosted remote tree at the producer's NATIVE
        /// point size, anchored — it does NOT stretch the tree to the host's
        /// `bounds` (proven live: a 640×360 game stayed 640×360 in a larger pane
        /// with black fill). So we can't just `setFrame` to the pane. Instead we
        /// host the layer at its native bounds and apply a scale TRANSFORM that
        /// maps native → pane, centred on the pane. Native size comes from the
        /// SURFACE reply (stored at `attach`). When it's unknown we fall back to
        /// the pane size (identity scale) — the host then clips to the pane
        /// rather than mis-scaling.
        pub fn set_frame(&mut self, x: f64, y: f64, w: f64, h: f64) {
            if self.host_layer.is_none() {
                return;
            }
            if w <= 0.0 || h <= 0.0 {
                return;
            }

            // Native (producer) size in points. Fall back to the pane size →
            // identity scale (no native size reported).
            let (nw, nh) = match self.native_size {
                Some((nw, nh)) if nw > 0.0 && nh > 0.0 => (nw, nh),
                _ => (w, h),
            };
            let sx = w / nw;
            let sy = h / nh;

            // -----------------------------------------------------------------
            // PLACEMENT / FLIP — LIVE-TWEAK SPOT #2.
            //
            // We host the layer at its native bounds (0,0,nw,nh), anchored at its
            // CENTRE (0.5,0.5), positioned at the pane's centre, and scaled by the
            // transform. Centring the anchor makes POSITION flip-invariant: a
            // rect's centre is the same point in a top-left- and a bottom-left-
            // origin space, so we only have to flip the centre's y against the
            // view height. (The game IMAGE orientation is already correct upstream
            // — MoltenVK renders it upright into the layer.)
            //
            // The producer pins its remote root layer to frame (0,0,nw,nh)
            // (anchorPoint 0,0 / position 0,0 in surface_publish.mm), so the
            // hosted tree exactly fills our (0,0,nw,nh) bounds; the centred scale
            // then fills the pane. If the game lands off-centre or mis-scaled,
            // this anchor/position pair (or the producer's anchoring) is the one
            // thing to change.
            let view_h = self
                .view
                .as_ref()
                .map(|v| v.bounds().size.height)
                .unwrap_or(0.0);
            let center_x = x + w / 2.0;
            let center_y = view_h - (y + h / 2.0); // top-left y → bottom-left

            // Diagnostic: one line per fit *change* (launcher resize / re-attach),
            // so we can see pane vs native vs the resulting scale without
            // per-frame spam. Keeps for future offset debugging.
            let fit = (
                w.round() as i64,
                h.round() as i64,
                nw.round() as i64,
                nh.round() as i64,
            );
            if self.last_fit != Some(fit) {
                self.last_fit = Some(fit);
                tracing::info!(
                    pane_w = w,
                    pane_h = h,
                    native_w = nw,
                    native_h = nh,
                    sx,
                    sy,
                    center_x,
                    center_y,
                    view_h,
                    "compositor fit (scale hosted surface → pane)"
                );
            }

            let bounds = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(nw, nh));
            // Scale native → pane (no z scale). `CATransform3DMakeScale`.
            let scale = CATransform3D::new_scale(sx, sy, 1.0);
            let host_layer = self
                .host_layer
                .as_ref()
                .expect("host_layer present (checked at top of set_frame)");

            // Wrap the geometry change in a CATransaction with implicit
            // animations disabled — otherwise CoreAnimation would smoothly tween
            // every reposition (the per-frame fit re-asserts would lag the
            // launcher's own move/resize by the default 0.25s).
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            host_layer.setAnchorPoint(NSPoint::new(0.5, 0.5));
            host_layer.setBounds(bounds);
            host_layer.setPosition(NSPoint::new(center_x, center_y));
            host_layer.setTransform(scale);
            CATransaction::commit();
        }

        /// Show or hide the hosted game layer. A CALayer sublayer always
        /// composites ABOVE its superlayer's GL content, so egui's opaque
        /// loading cover can't hide it — we toggle the layer's own visibility
        /// instead. The launcher keeps it hidden behind the badge during the
        /// PPU/SPU/shader compile and shows it only once `game_playable`. No-op
        /// when not attached.
        pub fn set_hidden(&self, hidden: bool) {
            if let Some(host_layer) = self.host_layer.as_ref() {
                CATransaction::begin();
                CATransaction::setDisableActions(true);
                host_layer.setHidden(hidden);
                CATransaction::commit();
            }
        }

        /// Tear the hosted layer out of the view tree. Safe to call when not
        /// attached.
        pub fn detach(&mut self) {
            if let Some(host_layer) = self.host_layer.take() {
                host_layer.removeFromSuperlayer();
                tracing::info!(
                    context_id = self.context_id,
                    "CompositorHost: CALayerHost detached"
                );
            }
            self.view = None;
            self.context_id = None;
            self.native_size = None;
            self.last_fit = None;
        }
    }

    impl Default for CompositorHost {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Drop for CompositorHost {
        fn drop(&mut self) {
            self.detach();
        }
    }
}

// ---------------------------------------------------------------------------
// Non-macOS no-op stub — same surface so the call sites compile everywhere.
// ---------------------------------------------------------------------------
#[cfg(not(target_os = "macos"))]
mod imp {
    use std::ffi::c_void;

    /// No-op compositor host on non-macOS targets. The surface-embed path is
    /// macOS-only (Windows uses Win32 window tiling); this exists purely so the
    /// launcher's call sites are cross-platform.
    #[derive(Default)]
    pub struct CompositorHost;

    impl CompositorHost {
        pub fn new() -> Self {
            Self
        }
        pub fn is_attached_to(&self, _context_id: u32) -> bool {
            false
        }
        /// # Safety
        /// No-op; the pointer is never dereferenced on this platform.
        pub unsafe fn attach(
            &mut self,
            _ns_view: *mut c_void,
            _context_id: u32,
            _native_w: f64,
            _native_h: f64,
        ) {
        }
        pub fn set_frame(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
        pub fn set_hidden(&self, _hidden: bool) {}
        pub fn detach(&mut self) {}
    }
}

pub use imp::CompositorHost;
