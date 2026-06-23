# macOS ScreenCaptureKit capture backend for the recorder (PLAN A.1)

Notes on the Phase-A macOS capture backend (`tools/playthrough/src/capture.rs`).
Validated on this M3 Max / macOS 26.5.

## IMPLEMENTED (PLAN A.1): the `screencapturekit` crate (decision B)

Both paths were spiked; the **`screencapturekit` crate** (doom-fish v8, swift-bridge)
won over hand-rolled objc2 — it produced a working capture-to-file in ~25 LOC where
raw objc2's `SCRecordingOutput` / AVAssetWriter kept stalling (the A-path record is
below). The macOS backend captures the **main display** to an HEVC MP4 via the crate's
`SCRecordingOutput`; the crate handles the SCKit threading / run-loop / idle-frame
filtering internally. Per-window + the 2-pane composite are PLAN A.5.

**Dep** (`tools/playthrough/Cargo.toml`, macOS-gated) — the whole thing, no objc2:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
screencapturekit = { version = "8", features = ["macos_15_0"] }
```

**Three integration details, each a real gotcha:**
1. **CG-init.** A bare CLI aborts with `CGS_REQUIRE_INIT` the moment it touches
   `SCStream`. Fixed with a 3-line `NSApplicationLoad()` extern
   (`#[link(name = "AppKit", kind = "framework")]`) — no heavy objc2-app-kit. Must run
   on the main thread; `#[tokio::main(flavor = "current_thread")]` guarantees it.
2. **Swift-runtime rpath.** The swift-bridge dylibs (`libswift_Concurrency`) don't
   resolve at launch without an rpath; `build.rs` emits
   `cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift` on macOS (the crate doesn't set it).
3. **TCC.** Run via `tools/playthrough/run.sh` (codesigns with a stable identity) so the
   Screen Recording grant survives rebuilds — see the gotcha below.

**Verify:** `tools/playthrough/run.sh capture-smoke 3 /tmp/x.mp4` → a valid HEVC MP4.
The `-- capture-smoke <secs> <out>` dev mode exercises the backend with no
server/browser (it captures the main display).

---

## A-path record — raw objc2 (ABANDONED, kept for reference)

The pure-objc2 route got ~80% there (frames flowed) but its capture-to-file step
never landed; see "the open issue" below for why. These crates resolve + link clean
but are **NOT used**:

```toml
# (not in Cargo.toml — the raw-objc2 path we did not ship)
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

## The open issue → the file-writer choice (raw objc2 is fiddly)

Both objc2 capture-to-file routes were tried and **both stall at the encode/write
step despite the raw frame path being proven** (144 CMSampleBuffers delivered):

- **`SCRecordingOutput`** (the macOS-15+ convenience writer): `didStartRecording`
  fires, then `didFailWithError` → *"Failed due to failure to process first sample
  buffer"*, `recordedFileSize == 0`. Identical across every pixel format
  (default/BGRA/420v), file type (.mp4 MPEG4 / .mov QuickTime), and resolution
  (3262×2168 and 1630×1084). Not codec/format/visibility — its internal writer just
  won't take the buffers in this setup.
- **Hand-rolled `AVAssetWriter`** (statics hold writer+input so the off-queue
  `didOutputSampleBuffer` can append): setup works, `startWriting()` returns true,
  but `appendSampleBuffer` is rejected and the writer goes to `status==Failed`
  ("The operation could not be completed"). Two real findings here, both needed by
  *any* writer:
  1. **Filter idle frames.** SCKit emits `SCFrameStatus.Idle/Blank` buffers (no
     image) for an unchanged window; appending an image-less buffer fails the writer.
     Cheap test: `objc2_core_media::CMSampleBufferGetImageBuffer(buf).is_some()`
     (None ⇒ skip). A *static* test window (a terminal) is mostly idle frames — test
     against a genuinely changing window (the recorder's real Chrome SPA + RPCS3
     viewport both animate, so this is largely a scratch-test artifact).
  2. **Writer-state / exceptions.** Once a complete frame reaches `appendSampleBuffer`
     it throws an Obj-C/foreign exception that `objc2::exception::catch` did not tame
     — points at a writer-state race (start/session/append ordering) that needs
     careful serialization on one queue, not the main-thread pump.

**Revised recommendation — re-decide before sinking more objc2 time:**
- **(A) Finish hand-rolled AVAssetWriter in objc2** — pure Rust, ~80% there (frames
  flow). Remaining: SCFrameStatus/image-buffer filter (known) + exception-safe,
  single-queue-serialized `startWriting → startSession(firstPTS) → append → markAsFinished
  → finishWriting`. Real but fiddly; the foreign-exception state bug needs care.
- **(B) `screencapturekit` crate** (doom-fish, v8) — wraps SCRecordingOutput +
  threading + frame-status filtering internally; almost certainly a working
  capture-to-file in far less code. Cost: it's a **swift-bridge** crate → a Swift
  build step. **Already acceptable here**: the macOS RPCS3 build
  (`.ci-local/build-mac.sh`) requires Xcode/Swift on dev machines anyway, and the
  recorder is a dev/CI-only tool (never shipped). This now looks like the
  lower-risk path.

Leaning **(B)** given how layered the raw path proved — but it reverses the
objc2-only stance, so it's chotchki's call. The hard/uncertain parts (TCC, CG-init,
async bridging, window targeting, frame delivery) are all solved either way.

## Gotchas (carry into capture.rs)

- SCKit needs a real logged-in windowserver session — no headless/CI/SSH (same
  class as the documented UIA session-0 gotcha). Local interactive desktop only.
- **Screen Recording (TCC) — sign dev builds; run via `tools/playthrough/run.sh`.**
  TCC keys the grant to the binary's Designated Requirement. An unsigned/ad-hoc cargo
  binary's DR is *just its cdhash*, which changes every `cargo build` → macOS treats
  each build as a new app and re-prompts. `run.sh` codesigns with an Apple
  *Development* identity + a fixed `--identifier`, giving a stable DR
  (`identifier "io.hotchkiss.skylander.playthrough" and …Apple Development…`, no
  cdhash) — grant Screen Recording once and it survives rebuilds. Two caveats: (1)
  macOS Sequoia/Tahoe *also* re-prompts ~monthly for ANY granted app ("Allow For One
  Month") — fires on the terminal regardless of signing, just click allow; it is NOT
  the rebuild thrash. (2) The old "grant attaches to the parent terminal, children
  inherit" holds ONLY while requests are attributed to Ghostty — the default for a
  plain child, but a `.app`/`open`/disclaim path attributes to the binary instead, so
  signing is the robust fix. Recover a confused grant: `tccutil reset ScreenCapture
  com.mitchellh.ghostty` (never hand-edit the SIP-protected `TCC.db`).
- A window that is minimized/occluded/off-screen stops producing frames; the
  recorder must keep the captured windows visible (it already arranges them).
- `CMTime` is `#[repr(packed)]` — copy fields to a local before formatting (no
  references into the packed struct).
