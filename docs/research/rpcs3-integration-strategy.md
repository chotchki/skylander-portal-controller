# RPCS3 integration strategy — patched upstream + IPC (decision record)

**Status:** Accepted; spike 16.1.1–16.1.4 **done — GO, validated in-game on the
Windows HTPC (2026-05-28)**. P1 (portal control) + P2 (window handle) both proven
over an AF_UNIX IPC channel with zero dialog/UIA. **16.2 (vendoring) done
(2026-05-29):** RPCS3 vendored at `vendor/rpcs3` (pinned `c11979d`); patch series
in `rpcs3-patches/`; CI patch-apply guard + gated full-build lane; repo relicensed
to GPL-2.0-only. Next: 16.3/16.5 (productionise P1 + the Rust `IpcPortalDriver`).
**Date:** 2026-05-28.
**Pin:** RPCS3 master `c11979d` (2026-05-29) — latest-master pin chosen for
newest game-compat + crash fixes; rebase cadence is cheap because every patch is
shallow + additive (see patch-depth table).
**HTPC handoff:** `docs/dev/rpcs3-fork-htpc-bringup.md`.
**Supersedes:** Phase 12 (Mac AX driver), Phase 6.1 (RPCS3 window-flicker
suppression), and the existing Windows UIA portal driver as the *production*
control path. Those remain the fallback until the IPC path is proven.

## Problem

The controller currently wraps a stock RPCS3 by **driving its GUI** — UI
Automation on Windows (`UiaPortalDriver`), a planned AXUIElement port on macOS
(Phase 12), plus a pile of Win32 window juggling to hide the Skylanders Manager
dialog and suppress flicker (Phase 6.1). Two standing pains:

1. **GUI automation is fragile.** Driving the Skylanders Manager dialog from
   outside (UIA / AX) is brittle, platform-specific, and duplicated per OS.
2. **Some games crash / freeze.** This is core-emulation instability, *not* a
   portal problem — see "Crash resilience" below.

## What we are NOT doing (rejected alternatives)

### A. Strip-down fork — a "Skylanders-only" emulator
Rejected. A Skylanders game is a full PS3 game; running it requires the entire
PS3 stack (PPU + SPU recompilers, RSX/Vulkan, cell memory model, LV2
kernel/syscalls, SELF loader, firmware modules). None of that is optional and
none of it is Skylanders-specific. The only genuinely deletable code (Qt GUI,
other USB peripherals) is the cheap ~5%; you'd keep every expensive piece.
Worse, stripping/refactoring core code **destroys cheap upstream tracking** —
game-compat and crash fixes come *from* upstream, and a divergent fork freezes
you out of them. Stripping increases maintenance; it doesn't reduce it.

### B. Host virtual-USB device + RPCS3 libusb passthrough
Rejected. RPCS3 does support libusb passthrough, but presenting a *fake* Portal
of Power on the host is an OS-kernel problem that is hardest on exactly our ship
targets: Windows needs a **signed kernel-mode virtual USB driver** (or families
disabling driver-signature enforcement — a non-starter); macOS DriverKit needs
Apple-granted entitlements (effectively infeasible). Clean only on Linux, which
we don't ship. It also forces us to re-implement the full portal USB protocol
instead of reusing RPCS3's already-correct emulated device.

### C. In-process / single-binary link (compile RPCS3 into the controller)
Rejected on engineering grounds (licensing is a non-issue for this project — see
below). Three killers:
- **Crash isolation.** Games crash/freeze. As separate processes, a crash is
  *recoverable* (supervisor restarts + restores state). Linked in-process, every
  emulator crash takes down the web server, the phone WS sessions, and the
  launcher with it.
- **Two event-loop owners.** eframe/winit owns the main-thread event loop;
  RPCS3's GS frame wants to own windows/events too. Merging two GUI frameworks'
  event loops in one process is fragile.
- **RPCS3 isn't a library.** Turning it into a linkable unit is a deep
  restructuring patch on churning app/frame code — the expensive-to-rebase tier.

## Decision: patch series on pinned upstream + arm's-length IPC

Keep RPCS3 a **separate process**. Vendor it as a git submodule + a **thin patch
series** applied to a **pinned upstream tag** in CI. Control the portal over a
local **IPC** channel. Run RPCS3 in **no-GUI mode** with direct EBOOT boot.

### The maintenance lever: patch depth × churn

A carried patch is cheap to rebase only when it is **shallow + additive** and
lands on **rarely-changing** code. It becomes fork-grade maintenance when it
**rewrites churning core**. This is the rule that keeps "track upstream cheaply"
true. Every patch must stay in the top two tiers:

| Patch | Depth | Upstream churn | Rebase cost |
|---|---|---|---|
| **P1** — IPC control of the emulated Skylander USB device (load/clear/query) | shallow, one isolated file | near-zero | free |
| **P2** — window-lifecycle hook: create the game window borderless at a supplied geometry, no focus-steal, report its native handle over IPC | shallow, additive, GS-frame seam | low–moderate | cheap |
| **(P3)** — render RPCS3's output *into a host-owned surface* (true pixel-embed) | deep, rewrites GSFrame/swapchain | high + platform-specific | **avoid** |

### Architecture
- **`IpcPortalDriver`** drops in beside `UiaPortalDriver` / `MockPortalDriver`
  behind the existing `PortalDriver` trait, and becomes the production driver on
  **every** platform (Windows + macOS). This is why it supersedes the Mac AX
  driver (Phase 12) entirely: portal control is platform-agnostic over a socket.
- **No-GUI + direct EBOOT boot** → no menu nav, no Skylanders Manager dialog, no
  UIA/AX, minimal windows. P2 gives launch control (geometry, z-order, no flicker)
  without touching the renderer.
- **Window "merge" = single-app feel, not single surface.** Coordinate two
  borderless windows via host-side window management; on Windows optionally nest
  the viewport via `SetParent`. macOS can't embed another process's window, so
  the design degrades to coordinated windows rather than depending on true embed.
  P3 (true embed) stays out of scope unless a single-surface look becomes a hard
  requirement — it's now an engineering call, not a legal one.

### Crash resilience (independent of the portal mechanism)
Games crashing/freezing is upstream emulation quality; the portal redesign does
not fix it. Levers, all in our wheelhouse:
- Pin a **known-good RPCS3 build per game** (we pin anyway for the patch series).
- **Per-game config tuning** (SPU block size, PPU/SPU accuracy, renderer,
  framelimit).
- A **Rust crash/freeze supervisor**: detect exit *and* freeze (process wait +
  a liveness signal), auto-restart, re-boot the game, restore portal slot state,
  drive the phone to a "reconnecting…" overlay. Generalizes the Phase 12.4.x
  crash-detection tasks. This is the highest-value reliability work and is
  independent of which portal path we choose.

## Licensing

The user has stated license is a non-issue for this project; **GPLv2 is
acceptable**. This simplifies the build/repo story: vendor RPCS3 as a submodule +
patch series and distribute the patched build with no boundary-keeping ceremony.
Note the controller would remain a *separate process* regardless — for crash
isolation, not for licensing.

**Acted on (16.2, 2026-05-29): repo relicensed MIT → GPL-2.0-only.** RPCS3 is
GPL-2.0-**only**, and `rpcs3-patches/` is a derivative of its source, so the
combined published work must be GPL-2.0 (it can't be GPLv3 — incompatible with
v2-only). Rather than keep a permissive boundary around the separate-process
controller, we relicensed the whole repo to GPL-2.0-only to match (simplest, and
what the no-boundary-ceremony decision implies). `/LICENSE` is the verbatim GPLv2
text; `Cargo.toml`, `README.md`, `docs/about.md` updated. The separate-process
split stays — for crash isolation, not licensing.

## Spike (the go/no-go gate)

One spike validates the whole architecture before committing:
clone RPCS3 at a pinned tag, locate the emulated Skylander device and the
GS-frame creation point, prototype **P1** (loopback-socket figure feed) and
**P2** (borderless window + handle report), and confirm a no-GUI direct-boot
game sees figure changes and hands back a positionable window with **zero dialog
interaction**. If that works, the patch depth is proven empirically and Phase 16
proceeds; if the seam is deeper than expected, revisit.

## Spike findings — 16.1.1 (seam location)

Probed against RPCS3 `c11979d` (2026-05-29), shallow clone. **Result: strong
go — both seams are shallow + additive, confirming the patch-depth thesis.**

### P1 — portal control (`Emu/Io/Skylander.h`, 57 lines, stable)
The emulated device exposes a process-global singleton and the exact API we need,
already public and mutex-protected (`shared_mutex sky_mutex`):

```cpp
extern sky_portal g_skyportal;          // global singleton
class sky_portal {
  u8   load_skylander(u8* buf, fs::file in_file);  // load → returns portal slot
  bool remove_skylander(u8 sky_num);               // clear
  void activate(); void deactivate(); void set_leds(u8,u8,u8);
  skylander skylanders[8];                          // the 8-slot model
};
```

The GUI (`rpcs3qt/skylander_dialog.cpp`) drives the portal **entirely** through
this global — `g_skyportal.load_skylander(data.data(), std::move(sky_file))` and
`g_skyportal.remove_skylander(cur_slot)`. Our IPC server replicates exactly that,
minus the Qt shell. **USB protocol untouched** — `control_transfer` /
`interrupt_transfer` read from `g_skyportal` as before. P1 patch ≈ a small IPC
listener thread that opens a `.sky` and calls these two functions. Confirmed
cheap-tier (file is tiny and rarely churns).
- Nuance: `load_skylander` takes `fs::file in_file` and keeps the handle to write
  on-tag changes back (`skylander::save()`). The controller already manages
  working-copy `.sky` paths on disk, so passing a path is natural.

### P2 — window lifecycle (`Emu/RSX/GSFrameBase.h` + `rpcs3qt/gs_frame.cpp`)
`GSFrameBase` is a clean 36-line pure-virtual interface that **already exposes the
native handle** we need: `virtual display_handle_t handle() const = 0;` (plus
`show/hide/shown/client_width/client_height/toggle_fullscreen`). The concrete Qt
window is `gs_frame`.

Mode already exists: `rpcs3.cpp` defines `--no-gui` (game window, **no** main GUI
/ menus / Skylanders dialog) and `--fullscreen` ("only useful with no-gui").
no-gui runs the normal `gui_application` with `SetShowGui(false)` — i.e. it still
builds a `gs_frame` for the game. (`--headless` is different: no render window at
all — not what we want.) So **`--no-gui` is our launch mode out of the box.**
P2 patch ≈ borderless + supplied-geometry + no-focus-steal window flags in
`gs_frame`'s ctor, and emit `handle()` over the P1 IPC channel. Confirmed
cheap-tier (additive; `gs_frame.cpp` churns more than `Skylander.h` but the change
is small).
- Staging: a basic merged experience may need *no* P2 at all — stock
  `--no-gui --fullscreen` gives a clean game window; the borderless/geometry/
  `SetParent` nesting is the polish tier (16.4 / 16.6.2).

### Verdict
Go. P1 reuses the exact GUI-proven API on a stable 57-line file; P2's handle is
already exposed and the no-gui mode already ships. Live in-game verification
(16.1.2) + the window prototype (16.1.3) move to the Windows HTPC.

## Live spike results — 16.1.2 + 16.1.3 (HTPC, 2026-05-28) — **GO confirmed**

Built patched RPCS3 (master `c11979d`) on the Windows HTPC and validated both
seams in-game. Build recipe + toolchain gotchas: `docs/dev/rpcs3-fork-htpc-bringup.md`.

### P1 — portal control over IPC ✅ (16.1.2)
- Patch is **one file + 3 one-line accessors**: a self-contained listener in
  `Emu/Io/Skylander.cpp` (started once from the `usb_device_skylander` ctor) plus
  `first_free_slot/slot_loaded/slot_serial` on `sky_portal` in `Skylander.h`. No new
  `.cpp`, no `rpcs3.vcxproj` edit. Confirmed cheap-tier exactly as predicted.
- **Transport = AF_UNIX domain socket**, not loopback TCP — no Windows-firewall
  consent prompt, no port conflicts, native on Win10 1803+/Win11 + macOS (one
  codepath). Mirrors the AF_UNIX server already in `Emu/GDB.cpp`. (Controller side
  on Windows: tokio + a uds-windows shim; see the IPC-transport memory.)
- Commands: `LOAD <path.sky>` → `g_skyportal.load_skylander` (mirrors
  `skylander_dialog::load_skylander_path`), `CLEAR <slot>` → `remove_skylander`,
  `STATUS`, `PING`. **Result:** booted Skylanders Giants `--no-gui` direct-EBOOT;
  over the socket, `LOAD` made Stealth Elf appear on the in-game portal and `CLEAR`
  removed her — **zero dialog interaction, zero UI automation.** USB
  control/interrupt path untouched.
- **Clean emulator-state signal (no log scraping / no shader-compile guessing):**
  `STATE` + a 1 Hz **heartbeat** push `Emu.GetStatus()` (running/paused/frozen/…),
  the `g_progr_*` boot/shader-compile progress, and the RSX `int_flip_index` frame
  counter. Observed frames advancing +60/s while `status=running, progr=8/8`. This
  is exactly the liveness/freeze signal the 16.7 supervisor needs (freeze =
  `running` + frames stalled).

### P2 — window lifecycle ✅ (16.1.3)
- Patch is **two small hunks** in `rpcs3qt/gs_frame.cpp` + one `extern` global:
  env-gated borderless + no-focus-steal flags (`Qt::FramelessWindowHint |
  Qt::WindowDoesNotAcceptFocus` under `SKYLANDER_BORDERLESS`), and publish
  `winId()` to `g_game_window_handle` after `create()`. IPC `WINDOW` command returns
  it. (Global declared inline in the two `.cpp` that touch it — deliberately NOT in
  the widely-included `GSFrameBase.h`, to keep the rebuild tiny.)
- **Result:** controller read `handle=0x120412` over IPC, then `SetWindowPos`
  (SWP_NOACTIVATE) moved/resized the borderless game window `(0,0,1728,894)` →
  `(80,80,1280,720)` — verified by `GetWindowRect` and visually on the TV. Native
  handle is real and positionable; no focus-steal.

### 16.1.4 decision: **GO.**
Both patches landed in the top two maintenance tiers as the thesis predicted (P1 ≈
one file, P2 ≈ two small hunks; both shallow + additive on rarely-churning seams).
No deeper-than-expected surprises. Proceed to **16.2** (vendor RPCS3 as a submodule
at `c11979d` + the patch series + CI lane). Performance note: stock dev config runs
Giants laggy (SPU Block Size `Safe` + verbose logging) — orthogonal to the portal
redesign; addressed by the per-game config strategy (16.7.4 / 16.9).

## IPC driver + protocol live-validated — 16.5.1 (HTPC, 2026-05-29) — **GREEN**

The controller-side Rust `IpcPortalDriver` + the `proto.rs` wire codec, exercised by
the `live_ipc.rs` acceptance test against the **live patched binary** (Giants,
`--no-gui`, over RDP):
- `STATE` frame counter advanced **4716 → 4782** (+66 / ~1.1s ≈ 60fps) — the clean
  liveness signal (no log-scraping); the freeze-detection foundation for 16.7.
- `WINDOW` returned the borderless game-window handle `0x20436` (P2) over the driver.
- `LOAD` (Chop Chop, from a working copy) placed the figure on the in-game portal and
  `CLEAR` removed it — `g_skyportal` driven entirely through the Rust driver, zero
  dialog/UIA. Emulator-assigned slot; the master `.sky` was untouched (loaded a copy).

Both sides of the IPC contract now proven (controller Rust driver ⇄ C++ patch). The
foundation 16.6 (no-GUI launch + window coordination) builds on is confirmed solid.

**16.6.1 no-GUI launch also live-validated (same day)** via `live_launch.rs`:
`RpcsProcess::launch_no_gui` booted Giants `--no-gui` + borderless, the IPC readiness
loop saw it reach playable in ~23s (`status=running frames=15 progr=8/8`) with the
window handle `0x403AC` published — i.e. the server's `BootDirect` IPC path works end
to end (launch → liveness → handle → shutdown), no FPS-title scraping. The
full-through-`/api/launch` run awaits the 16.9 config slice (patched-exe vs
firmware/`games.yml` path split).
