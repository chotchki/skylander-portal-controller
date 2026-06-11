# Demo recorder — beats & narrative framework (design)

**Status:** DESIGN — 2026-06-10. Not yet implemented; implement after review.
**Scope:** `tools/playthrough/` (crate `skylander-playthrough`), Windows-focused,
dev/CI-only, never shipped. Builds on the `skylander-e2e-tests` harness.

> Companion to PLAN 15.x. This doc pins down two open questions that the bare
> "beats" idea left unanswered: **capture topology** (one frame vs separate
> windows) and **dead space** (long real-time waits in a continuous capture).

---

## 1. Why

The recorder today (`tools/playthrough/src/main.rs`) has three **monolithic
scenarios** dispatched by `args().nth(1)` — `portal`, `place`, `ingame` — each
re-doing the same boot and a screen-specific drive. That gives whole-flow clips
but:

- no reusable **narrative** (the real user journey: connect → profile → game →
  browse → place → **see it in the actual game** → optional Kaos teaser), and
- no cheap **per-screen** clip (you re-shoot a whole scenario to redo one screen).

The just-completed in-game tier (place from the phone → figure appears on the
game's own portal, via the P5 RECONNECT) is the **narrative climax** and the
single most compelling beat — it deserves to land at the emotional peak of a
flow, not be buried as one isolated clip.

**Goal:** model the demo as ordered **beats**. Render the full **narrative** (one
stitched MP4) *or* a single **beat** (a per-screen clip — per-screen falls out for
free, and re-recording one changed screen is cheap).

---

## 2. Model — beats & narratives

The harness is `&self`-async throughout (`Phone::wait_for/tap_pointer/js_click`,
free fns `inject_load_outcomes`/`unlock_session`/`fire_*` over `&server.url`), so a
beat is a **boxed async closure over a context**. New file
`tools/playthrough/src/beats.rs`:

```rust
pub struct BeatCtx<'a> {
    pub phone: &'a Phone,
    pub server: &'a TestServer,
    pub phone_url: &'a str, // place_figure's reload step needs it
    pub alice: &'a str,     // profile id from inject_profile (don't re-inject per beat)
}

pub type BeatFut<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + 'a>>;
pub type DriveFn = for<'a> fn(&'a BeatCtx<'a>) -> BeatFut<'a>;

pub struct Beat {
    pub name: &'static str,            // CLI key for `-- beat <name>`
    pub drive: DriveFn,                // imperative JS+wait sequence (verbatim from today's scenarios)
    // --- editorial (see §5) ---
    pub realtime_head: Duration,       // keep this much at 1x at the start (the action)
    pub realtime_tail: Duration,       // keep this much at 1x at the end (the reveal)
    pub filler_speed: f32,             // play the dead middle at this speed (e.g. 8.0); 1.0 = no speed-up
    pub crop: Option<CropRect>,        // post-crop framing (None = full desktop frame)
    pub caption: Option<&'static str>, // LATER (title-card text) — reserved, unused in v1
}

pub enum ServerFlavor { Mock, IpcSavestate }   // a narrative is locked to ONE flavor (§7)

pub struct Narrative {
    pub name: &'static str,
    pub flavor: ServerFlavor,
    pub beats: Vec<Beat>,
}
```

Use plain `fn` pointers (not `Box<dyn Fn>`) so `Beat` is `'static` and the registry
is a simple const/builder; each beat is a free `async fn` wrapped by a one-line
`|c| Box::pin(beat_x(c))` shim (handles the HRTB lifetime coercion). **Reject** an
enum-of-step-kinds: the existing drives are imperative `js_click`→`wait_for`→sleep
dances, not declarable as data without rewriting them.

---

## 3. Beat catalog

Extract today's drive bodies into beat fns, almost verbatim:

| beat | drives (from current code) | flavor |
|---|---|---|
| `connect` | wait `.profile-picker` (the QR/connect framing is the hold here) | any |
| `pick_profile` | `unlock_session(&server.url, alice)` (PIN bypass) | any |
| `pick_game` | wait `.game-card` → Spyro-or-first click (real `/api/launch`) | IPC; Mock uses `launch_giants` |
| `open_toybox` | two `tap_pointer(".lid-grabber-p4")` → wait `.fig-card-p4` — **this is the "browse collection" beat** (the grid lives in the drawer) | any |
| `place_figure` | pick-by-name JS → `.detail-btn-primary` → wait `.p4-slot--loaded`. **Two variants:** Mock calls `inject_load_outcomes` first; IPC is a real LOAD (no injection) | mock **or** ipc |
| `see_in_game` | long settle (~16s floor) so the resumed save state re-reads the portal + the figure lands on RPCS3's own window — **the climax** | IPC only |
| `kaos_teaser` | `fire_kaos_taunt` / `fire_takeover` test-hooks (no DOM selector needed) | any (last, optional) |

**Narratives as data:**
- `portal` = `[connect, pick_profile]` (+ hold) — **Mock**
- `place` = `[connect, pick_profile, open_toybox, place_figure(mock)×]` — **Mock**
- `ingame` / **marquee** = `[connect, pick_profile, pick_game, open_toybox, place_figure(ipc), see_in_game]` (+ optional `kaos_teaser`) — **IPC**

---

## 4. Capture topology — **single desktop capture** (decided)

`capture.rs` records `Monitor::primary()` to one MP4 (windows-capture, built-in
H.264, **no ffmpeg at capture time**, background thread). Both surfaces — the egui
launcher (the "TV") and the headed Chrome phone (positioned at `1180,40,470,940`) —
share one frame. The raw H.264 capture is an **intermediate**: the post-render (§5)
**transcodes the final cut to H.265 or AV1** (smaller, modern) in the same ffmpeg
pass that does the speed-ramp / crop / concat — so the deliverable is never the raw
H.264.

**Decision: keep the single full-desktop capture.** Rationale:

1. **Simultaneity is the value prop.** The demo's whole point is *tap on the phone
   → the figure appears on the TV game*, visible in **one frame**. Separate
   captures lose that causal beat unless re-composited as picture-in-picture.
2. **No compositing/sync toolchain.** One capture = one MP4. Two streams need
   frame-sync + a compositor (ffmpeg), which the capture path deliberately avoids.
3. **Per-beat framing is still achievable** without two streams: the recorder
   knows each window's rect, so the **post step can crop** per beat (§5) — e.g.
   crop to just the phone for a UI beat, full frame for the in-game reveal.

This is **provisional**: how the side-by-side framing + per-beat crops actually look
can only be judged from real output, so v1 ships the single full-desktop capture and
we iterate from there (the v2 separate-window door below stays open).

**Deferred to v2 (only if post-crop proves insufficient):** capture the launcher
and Chrome windows *separately* (windows-capture supports a `Window` target, not
just `Monitor`) and composite — for true PiP overlays or per-window resolution.
Not needed for v1; flagged so the door stays open.


---

## 5. Dead space → editorial timeline (the real fix)

**Problem:** a continuous capture includes the long real-time waits — the
save-state resume (`wait_for(".portal-p4", 180s)`) and the `see_in_game` settle
(~16s) — so the raw MP4 has dead frames. As noted: of a 20s beat, ~3s is
interesting; the rest should fast-forward.

**Approach: continuous raw capture + a per-beat editorial manifest + a staged
post-render** that speed-ramps / trims / crops / concatenates. The recorder never
edits video live; it records raw + emits the editorial intent, and a separate
pass renders the final cut.

- **Per-beat editorial intent** lives on `Beat` (§2): `realtime_head` (keep the
  opening action at 1x), `realtime_tail` (keep the closing reveal at 1x — e.g.
  `see_in_game`'s figure-appears moment), `filler_speed` (play the dead middle
  fast, e.g. 8×), `crop` (framing).
- **The recorder emits a manifest** (`<out>.timeline.json`) by stamping
  wall-clock at each beat boundary relative to `DesktopCapture` start:

  ```json
  [ { "beat": "see_in_game", "t_start_ms": 41200, "t_end_ms": 58200,
      "realtime_head_ms": 1000, "realtime_tail_ms": 4000,
      "filler_speed": 8.0, "crop": null } ]
  ```

- **The post-render** (`-- render <raw.mp4> <timeline.json> <final.mp4>`) renders,
  per beat: `[t_start .. t_start+head]` @1×, `[t_start+head .. t_end-tail]` @
  `filler_speed`, `[t_end-tail .. t_end]` @1×, cropped, then concatenates all
  beats. (Richer per-moment marks — a `ctx.mark("reveal")` the drive calls — are a
  v2+ refinement if head/tail brackets prove too coarse.)

**Tooling: ffmpeg (decided).** The no-ffmpeg rule applies only to *capture*
(windows-capture's built-in encoder keeps the capture stage dependency-free). The
**post-render is a separate dev-only stage and uses ffmpeg** — it's already on the
box and in heavy use, and it does everything we need in one invocation: per-beat
speed-ramp (`setpts` / `atempo`), `crop`, `concat`, **and the H.265/AV1 transcode**
of the final cut. So the render pass takes the raw H.264 + `timeline.json` and emits
a single edited, modern-codec MP4. (A pure-Rust video crate was the alternative —
dropped; ffmpeg is simpler and already available.)

**Staging:** v1 emits the manifest (raw MP4 + `timeline.json`); the post-render is
its own phase (§9.4). So even before the renderer exists, every run is annotated.

---

## 6. Rendering modes (CLI)

`main.rs` keeps the shared boot (spawn server *by flavor* → `inject_profile` →
`DesktopCapture::start` → `Phone::new_headed`), then:

- `-- narrative <name>` — **one** `DesktopCapture` spanning all beats, run
  sequentially (`for b in beats { (b.drive)(&ctx).await?; }`), emitting the raw MP4
  + `timeline.json`. **Do not** per-beat-capture inside a narrative — each
  `DesktopCapture::stop()` finalizes a *separate* MP4 (capture.rs), defeating the
  stitch.
- `-- beat <name>` — boot the flavor of that beat's owning narrative, run just that
  beat → a per-screen clip (+ still PNG).
- `-- render <raw.mp4> <timeline.json> <final.mp4>` — apply the editorial timeline
  (§5). v2.
- **Back-compat:** bare `portal` / `place` / `ingame` map to their narratives.

---

## 7. Server-flavor constraint (the primary risk)

A narrative is **locked to one `ServerFlavor`** — beats are *not* freely composable
across Mock and IPC:

- `place_figure` genuinely differs (injected mock outcome vs real IPC `LOAD`).
- `see_in_game` and real-`/api/launch` `pick_game` are **IPC-only** (need the
  patched RPCS3 + save-state boot on the HTPC: `RPCS3_EXE`, `RPCS3_CONFIG_DIR`,
  `SKYLANDER_BOOT_SAVESTATE`).

Encode `flavor` on `Narrative`; select the spawn fn from it
(`spawn_with_env_lines` vs `spawn_ipc_savestate`). **Fail fast** at registry build
if an IPC-only beat is listed under a Mock narrative.

---

## 8. Captions / title-cards (deferred)

The `caption` field is reserved on `Beat` in v1 but unused. Captions are a separate
overlay layer (an always-on-top borderless egui/winit window, or a Chrome data-URL
tab, drawn over the desktop during a beat and captured incidentally — Skylanders
gold-outlined titles). Build *after* the beat skeleton + flow + editorial render
work. Do not build the overlay now.
- Comment: I'm also open to using other tools for the captions/cuts if its better suited.

---

## 9. Phases

1. **Skeleton** — `beats.rs` with the types + the beat fns (existing drives moved
   verbatim, no behavior change) + a narrative registry.
2. **Wire `main.rs`** — mode+name dispatch; flavor-select the spawn fn; one
   `DesktopCapture`; the beat loop; **emit `timeline.json`** (beat-boundary
   timestamps); back-compat aliases.
3. **Marquee narrative** — the IPC `[connect … see_in_game]` flow ending on the
   payoff. Verify `-- narrative ingame` reproduces today's MP4 and `-- beat
   open_toybox` yields a clean per-screen clip.
4. **Post-render** (`-- render`) — speed-ramp / trim / crop / concat per the
   manifest (the dead-space fix). Pick the tool (ffmpeg vs Rust crate).
5. **Later** — `kaos_teaser` beat; then the caption/title-card overlay.

---

## 10. Risks / unknowns

- **Flavor split** (primary): two `place_figure` variants or a flavor branch; guard
  IPC-only beats out of Mock narratives.
- **Continuous-capture dead spans**: mitigated by §5; until the renderer exists,
  keep `realtime_*`/`settle` generous (16s `see_in_game` is the proven floor — 4s
  was too short).
- **Crop-rect accuracy**: post-crop relies on known window positions; the launcher
  (Desktop window mode) + `Phone::new_headed(1180,40,470,940)` rects must be
  stable/recorded.
- **Post-render tooling**: introduces an ffmpeg (or video-crate) dep for the *dev*
  render stage — acceptable, but a new toolchain to document.
- **State threading**: `alice` id + real `/api/launch` (IPC-only) through
  `BeatCtx`; don't re-inject per beat.
- **HRTB coercion**: `for<'a> fn(&'a BeatCtx<'a>) -> BeatFut<'a>` with `async fn`s
  needs the per-beat `Box::pin` shim — well-trodden, low risk.

---

## 11. Files

- `tools/playthrough/src/main.rs` — CLI dispatch + boot + the drive bodies to extract.
- `tools/playthrough/src/beats.rs` — **NEW**: types + beat fns + registry.
- `tools/playthrough/src/capture.rs` — `DesktopCapture::start/stop` (one MP4 per
  capture; `Monitor::primary`); the post-render likely lands near here or a new
  `render.rs`.
- `crates/e2e-tests/src/lib.rs` — the harness (`TestServer::spawn_*`, `Phone`
  `&self` methods, `inject_*`/`unlock_session`/`fire_*`).
- `phone/src/screens/browser.rs` — collection grid selectors
  (`.fig-card-p4`/`.fig-name-p4`, inside the `.portal-p4` toy box).
