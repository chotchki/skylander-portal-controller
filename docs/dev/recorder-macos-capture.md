# macOS ScreenCaptureKit capture backend for the recorder (PLAN A.1)

Working notes + proven groundwork for the Phase-A macOS per-window capture backend
(`tools/playthrough/src/capture.rs`, today a no-op stub on non-Windows). Validated
on this M3 Max / macOS 26.5 with Screen Recording permission granted to the
terminal (Ghostty).

## Decision: objc2-screen-capture-kit (pure Rust), per-window, capture-to-file

Crates (macOS-gated), all resolve + link clean here:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.6"                 # 0.6.4
objc2-foundation = { version = "0.3", features = ["NSString","NSArray","NSURL","NSError","NSGeometry","NSEnumerator","NSRunLoop","NSDate","NSThread"] }
objc2-app-kit = { version = "0.3", features = ["NSApplication","NSResponder"] }
objc2-screen-capture-kit = { version = "0.3", features = ["block2","SCStream","SCRecordingOutput","SCShareableContent","SCError"] }
objc2-core-media = "0.3"
objc2-av-foundation = { version = "0.3", features = ["AVVideoSettings","AVMediaFormat"] }
block2 = "0.6"
dispatch2 = "0.3"
```

(`SCContentFilter`/`SCStreamConfiguration`/`SCWindow` live under the `SCStream`
feature — there are no separate features for them.)

## What is PROVEN working (full scratch prototype, runs green)

1. **CG init for a bare CLI.** A plain `cargo run` binary hits
   `Assertion failed: (did_initialize) … CGS_REQUIRE_INIT` the moment it touches
   SCStream. Fix: on the main thread, up front:
   ```rust
   let mtm = MainThreadMarker::new().unwrap();
   let app = NSApplication::sharedApplication(mtm);
   unsafe { app.finishLaunching(); }
   ```
2. **Enumerate windows** via `SCShareableContent::getShareableContentWithCompletionHandler`
   — the completion fires on an off-main GCD queue, so bridge it: the block retains
   `content` (`Retained::retain(ptr)`), boxes the raw pointer as `usize`, sends it
   over an `mpsc` channel; the main thread `recv`s and `Retained::from_raw`s it
   (SCKit objects are not `Send`, so move the raw pointer, not the `Retained`).
   Got 182 real windows with `title` / `owningApplication().applicationName()` /
   `frame()` / `windowID()` / `isOnScreen()`.
3. **Target a window** by `owningApplication` + title (see `WindowTarget`/`TitleMatch`
   in the blueprint — RPCS3 viewport is title-prefix `"FPS:"`, Chrome app-mode is
   the exact document title).
4. **Per-window filter** `SCContentFilter::initWithDesktopIndependentWindow(alloc, &win)`.
5. **Config**: `SCStreamConfiguration::new()`, `setWidth/Height` from
   `filter.contentRect().size * filter.pointPixelScale()` (force **even** — H.264),
   `setMinimumFrameInterval(CMTime{value:1,timescale:30,flags:CMTimeFlags(1),epoch:0})`,
   `setShowsCursor(true)`.
6. **Frame delivery WORKS**: add an `SCStreamOutput` (objc2 `define_class!`) on a
   real background `dispatch2::DispatchQueue::new("…", None)` via
   `addStreamOutput_type_sampleHandlerQueue_error(&out, SCStreamOutputType::Screen, Some(&queue))`
   → **144 CMSampleBuffers delivered in 4s (~36 fps)** for a visible window.
7. **Stream start/stop** via `startCaptureWithCompletionHandler(Some(&block))` /
   `stopCaptureWithCompletionHandler(Some(&block))` — completions fire (need the
   run loop alive; `NSRunLoop::currentRunLoop().runUntilDate(...)` services them).
8. **No-op delegates** (`SCStreamDelegate`, `SCRecordingOutputDelegate`) compile via
   `define_class!`; all `SCRecordingOutputDelegate` methods are `#[optional]`.

## The ONE open issue → the file-writer choice

`SCRecordingOutput` (the macOS-15+ convenience capture-to-file) **fails on the first
buffer** here: `recordingOutputDidStartRecording` fires, then
`recordingOutput:didFailWithError:` → *"Failed due to failure to process first
sample buffer"*, `recordedFileSize == 0`, no file — **even though the raw
SCStreamOutput on the same stream receives 144 frames fine**, across every pixel
format (default/BGRA/420v), file type (.mp4 MPEG4 / .mov QuickTime), and resolution
(retina 3262×2168 and logical 1630×1084). So it is not codec/format/visibility —
`SCRecordingOutput`'s internal AVAssetWriter just won't take the buffers in this
main-thread setup.

**Next step (decision):**
- **(A) AVAssetWriter ourselves** — pure objc2, uses the *proven* flowing buffers.
  Bindings confirmed: `AVAssetWriter::initWithURL_fileType_error`, `addInput`,
  `startWriting`, `startSessionAtSourceTime(firstPTS)`, `finishWritingWithCompletionHandler`;
  `AVAssetWriterInput::initWithMediaType_outputSettings` (NSDictionary of
  `AVVideoCodecKey→AVVideoCodecTypeH264`, `AVVideoWidthKey`, `AVVideoHeightKey`),
  `isReadyForMoreMediaData` / `appendSampleBuffer` / `markAsFinished`. The delegate
  holds the writer+input; **serialize all writer ops on the capture dispatch queue**
  (append in `didOutputSampleBuffer`, dispatch `markAsFinished`+`finishWriting` to the
  same queue on `stop()`); start the session on the first buffer's PTS. Recommended —
  stays pure-Rust, no Swift toolchain.
- **(B) `screencapturekit` crate** (doom-fish, v8) — wraps SCRecordingOutput +
  threading; likely a working capture-to-file in far less code, but it's a
  **swift-bridge** crate (adds a Swift build step), so it doesn't compose with the
  objc2 work above and pulls a toolchain dep into a dev-only tool.

Recommendation: **A** — finish the AVAssetWriter writer on top of the proven
SCStreamOutput frame path. The hard/uncertain parts (TCC, CG-init, async bridging,
window targeting, frame delivery) are all solved; the writer is mechanical objc2.

## Gotchas (carry into capture.rs)

- SCKit needs a real logged-in windowserver session — no headless/CI/SSH (same
  class as the documented UIA session-0 gotcha). Local interactive desktop only.
- Screen Recording (TCC) grant attaches to the *parent terminal*; children inherit,
  so `cargo run` rebuilds reuse it. Takes effect after a full relaunch.
- A window that is minimized/occluded/off-screen stops producing frames; the
  recorder must keep the captured windows visible (it already arranges them).
- `CMTime` is `#[repr(packed)]` — copy fields to a local before formatting (no
  references into the packed struct).
