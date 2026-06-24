# calayer-poc — cross-process CALayer sharing PoC (macOS)

**Throwaway proof-of-concept.** Proves that one process can publish a live
CoreAnimation layer tree and a *separate* process can composite it into its own
window, using the private **CARemoteLayer** mechanism (`CAContext` +
`CALayerHost`) that Safari and Chrome use to pull their GPU process's rendering
into the browser window.

This gates an architecture decision: if it works, we can let RPCS3 (or any
helper process) render into a layer we host inside our own egui/AppKit window
instead of juggling sibling top-level windows.

Not part of the main cargo workspace — its `Cargo.toml` has an empty
`[workspace]` table so `cargo` treats it as an isolated workspace root.

## What it does

- **`producer`** — brings up a headless `NSApplication`, creates a root
  `CALayer` whose `backgroundColor` ping-pongs **red ⇄ blue forever** via a
  repeating `CABasicAnimation` (CoreAnimation runs that animation in the render
  server, so it keeps moving with no per-frame work from us — and it animates
  *across* the process boundary). It then publishes the layer through a private
  `CAContext` and prints `CONTEXT_ID=<u32>`. No Metal needed for this phase.

- **`consumer`** — takes that context id on the command line, opens a bare
  `winit` window, makes the window's `NSView` layer-backed, creates a private
  `CALayerHost`, binds it to the context id (`-setContextId:`), and adds it as a
  sublayer sized to a **centered 75% sub-rect** of the view (the inset proves
  positioning; it also re-sizes on window resize).

## Build

```sh
cd tools/calayer-poc
cargo build            # builds + links both binaries; macOS only
```

## Run (two terminals)

**Terminal 1 — producer:**

```sh
cargo run --bin producer
# => prints something like:
# CONTEXT_ID=4308927
# [producer] running — layer is animating; Ctrl-C to quit.
```

Copy the integer after `CONTEXT_ID=`.

**Terminal 2 — consumer (paste the id):**

```sh
cargo run --bin consumer 4308927
```

## Success criteria

A separate `winit` window appears. Inside it — in a centered rectangle inset
from the window edges — the producer's layer is composited **live**: the
red ⇄ blue color cycle from the *producer* process animates inside the
*consumer's* window.

Specifically, it's a PASS if **all** of these hold:

1. **Live cross-process motion.** The color cycles continuously in the consumer
   window, driven by the producer process. (Kill the producer with Ctrl-C and
   the hosted content should stop updating / go blank — confirming it was
   genuinely the remote tree, not a local copy.)
2. **Correct positioning.** The hosted layer sits in the centered sub-rect, not
   the whole window and not the wrong corner — proving `CALayerHost.frame`
   places the remote tree where we ask.
3. **Survives resize.** Drag the window larger/smaller; the hosted rectangle
   tracks the new bounds (the `WindowEvent::Resized` handler re-frames the
   host) and keeps animating.

To make the host **fill** the whole window instead of the inset rect, edit
`size_host_to_view` in `src/bin/consumer.rs` to call
`host_layer.setFrame(bounds)` directly.

## Notes / caveats

- **macOS only**, and uses **private APIs** (`CAContext`,
  `CALayerHost`, `CGSMainConnectionID`). No App Store; fine for an internal
  decision-gating spike.
- **objc2 version split is intentional.** `winit 0.30` internally uses objc2
  **0.5**; our own code uses objc2 **0.6** (objc2-foundation/app-kit/quartz-core
  0.3.2). We never share objc2 *types* with winit — we only pull a raw
  `*mut NSView` out of `raw-window-handle` and re-wrap it with our own objc2 0.6
  `NSView`, so the two object runtimes don't collide.
- Throwaway: the producer leaks its CAContext/CALayer/animation on purpose
  (the process runs until Ctrl-C), and there's no error toast / retry / polish.
