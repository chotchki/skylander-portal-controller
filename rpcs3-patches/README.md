# RPCS3 patch series (PLAN 16.2)

The controller drives a **patched** RPCS3 over an AF_UNIX IPC channel (Phase 16).
Rather than fork RPCS3, we vendor it untouched and carry a thin, ordered patch
series here. Decision record + rationale: `docs/research/rpcs3-integration-strategy.md`.

## Layout

```
vendor/rpcs3/                         git submodule → RPCS3/rpcs3, pinned (pristine upstream)
rpcs3-patches/
  0001-P1-…-AF_UN….patch              P1 — IPC portal control (Skylander.cpp + .h)
  0002-P2-…-window-hand….patch        P2 — borderless window + native handle (gs_frame.cpp)
  0003-P3-…-ENETUNREACH.patch         P3 — offline public-IP connect → ENETUNREACH (sys_net)
  0004-SPU-Analyzer-prune-….patch     SPU — prune dead in-range targets, Giga const-prop †
  0005-SPU-Analyzer-fix-divisor-….patch  SPU — fix wrong divisor in Giga const-prop re-decode †
  0006-P4-…-PE-events.patch           P4 — push guest portal-events (PE) over IPC (Skylander.cpp)
  0007-P5-…-RECONNECT.patch           P5 — re-attach the portal after a save-state resume (Skylander.cpp + sys_usbd)
  0008-P6-…-BUTTON_PRESS.patch        P6 — inject gamepad buttons over IPC (Skylander.cpp + pad_thread)
  0009-P7-…-WINDOW_SET.patch          P7 — tile the game window over IPC (gs_frame.cpp + Skylander.cpp)
  0010-P8-…-SURFACE.patch             P8 — macOS surface-embed: publish CAMetalLayer via CAContext (CALayerHost)
  apply.sh                            apply the series onto a checkout (git am --3way)
  README.md                           this file
```
† 0004/0005 are the two local SPU-LLVM Giga crash fixes (merged upstream as RPCS3
  #18935). They sit between P3 and P4 because that's where they landed in the dev
  branch; they are NOT a controller feature, and the next pin bump (past the #18935
  merge) drops them — at which point the series is back to a clean P1–P8.

The submodule **always points at a pristine upstream commit** — it is never
modified in place. The patches live here, in our repo, where the diff is
reviewable. They apply in filename order; **0002 (P2) depends on 0001 (P1)**
(P1 defines the `g_game_window_handle` global that P2 publishes into), **0004 (P4)
builds on 0001 (P1)** — it extends P1's AF_UNIX listener in the same `Skylander.cpp`
with a portal-event push feed — and **0005 (P5) builds on 0001 (P1)** too, adding a
`RECONNECT` command beside it (plus a small additive `sys_usbd` helper). **P3 is
independent** — a `sys_net` errno tweak unrelated to the portal device or window.

The P-patches are the entire *controller* footprint on RPCS3 — all shallow +
additive on rarely-churning seams (the patch-depth thesis in the decision doc).
(0004/0005 are the transient SPU-LLVM Giga crash fixes noted above — not a
controller feature.) What each does:

- **P1** — a self-contained AF_UNIX listener on the emulated Skylander USB device
  (`Emu/Io/Skylander.cpp`) exposing `LOAD/CLEAR/STATUS/STATE/WINDOW/PING` + a 1 Hz
  heartbeat, plus 3 read-only slot accessors on `sky_portal` (`Skylander.h`). No
  new `.cpp`, no `rpcs3.vcxproj` edit.
- **P2** — env-gated (`SKYLANDER_BORDERLESS`) borderless + no-focus-steal flags in
  `gs_frame`'s ctor, and publishes the native `winId()` for the IPC `WINDOW` command.
- **P3** (PLAN 16.10) — a one-line behaviour fix in
  `Emu/Cell/lv2/sys_net/lv2_socket_native.cpp`: when networking is *Disconnected*,
  a public-IP `connect()` returns `SYS_NET_ENETUNREACH` ("network unreachable",
  which games treat as permanent → disable online and play on) instead of
  upstream's `SYS_NET_EADDRNOTAVAIL`, which Skylanders: Spyro's Adventure busy-
  retries into a CPU-pinning freeze. Inert under the controller's shipped
  *Connected* config (where connects succeed, so the branch isn't taken); a fix
  for the offline path. No automated test — see Tests below.
- **P4** (PLAN 15.12) — extends P1's listener in `Emu/Io/Skylander.cpp` with a
  *portal-event push* (`PE cmd=<name>`): the device mirrors what the guest game
  asks of the portal (`activate` / `query` / `write` / `status` presence-poll / …)
  to the IPC peer, drained beside the 1 Hz heartbeat. Lets the controller see
  when the game reaches/polls/reads the portal — used by the play-through
  recorder and to diagnose whether a save-state-resumed game re-reads a *late*
  LOAD. Per-command rate-limited so a steady `status` pulse isn't starved; tested
  from Rust via the loopback (`PE`-skip in `roundtrip` + a `watch_events` tail).
- **P5** (PLAN 15.12) — adds an IPC `RECONNECT` command that re-attaches the emulated
  portal after a **save-state resume**. On resume RPCS3 rebuilds the USB handler + its
  (empty) LDD registry, but the resumed game keeps its stale pipe handles and never
  re-registers its LDD, so the portal never connects and its `sys_usbd_transfer_data`s
  fail `CELL_EINVAL` (the game shows "reconnect the Portal of Power"). `RECONNECT`
  re-registers the LDD (re-connecting the device) then hot-plug-cycles it (DETACH+ATTACH)
  so the guest re-enumerates with fresh handles — `usb_handler_thread::reconnect_device`
  + a `register_ldd_and_connect` free function in `sys_usbd.cpp`/`.h`, called from
  `Skylander.cpp`. Live-verified: a figure LOADed after RECONNECT appears in-game on a
  resumed save state. Driver side: `Command::Reconnect` + `IpcPortalDriver::reconnect`,
  loopback-tested (`reconnect_roundtrips_ok`).
- **P6** (PLAN 15) — extends P1's listener with a `BUTTON_PRESS <button> <ms>` command
  that injects a gamepad button on port 0 via `pad_thread` (hold then release). Lets the
  play-through recorder drive classifier-led menu nav over the same socket — no
  focus-steal, unlike synthesised keystrokes. Loopback-tested (codec + round-trip).
- **P7** (PLAN 16/20) — adds `WINDOW_SET <x> <y> <w> <h>`: the controller tiles the game
  window instead of letting it go fullscreen. A function-pointer hook in `gs_frame`
  marshals the move/resize onto the GUI thread (no Qt types leak into emucore). The
  cross-platform replacement for the Win32 `SetWindowPos` fit. Loopback-tested.
- **P8** (PLAN 16 / macOS surface-embed) — the macOS analogue of P2's window publish:
  instead of a second top-level window, the emulator hands its render layer to the
  launcher. A dedicated off-view `CAMetalLayer` is published through a private `CAContext`;
  the launcher hosts it INSIDE its own egui window via `CALayerHost` (CARemoteLayer SPI).
  `gs_frame::show()` is suppressed and `client_width/height` are pinned to 720p (or 1440p
  under `SKYLANDER_SURFACE_2X` — the opt-in 2× render pass, PLAN S) under
  `SKYLANDER_BORDERLESS` so swapchain == present == the published layer (the game fills it,
  one clean resample on the launcher). The `SURFACE` IPC reply carries the contextId +
  the surface size. macOS-only; inert elsewhere. Loopback-tested (`SURFACE` round-trip)
  + live-validated on an M3 Max (Skylanders Giants composites in the launcher pane).

## The pin

| | |
|---|---|
| Submodule | `vendor/rpcs3` → `https://github.com/RPCS3/rpcs3.git` |
| Pinned commit | `927e2492ef720d2223bd8b149a02af875e11c398` (master, 2026-06-22, `v0.0.40-637`) |
| Patches generated against | that same commit |

Pin is by **commit**, not branch — `.gitmodules` has no `branch =`, so
`git submodule update` never silently moves it. Latest-master was chosen for
newest game-compat + crash fixes; rebasing is cheap because the patches are
shallow + additive.

## Build a patched RPCS3 locally

```bash
git submodule update --init --recursive vendor/rpcs3   # full build needs 3rdparty
rpcs3-patches/apply.sh                                  # → 10 commits on top of the pin
# then build per docs/dev/rpcs3-fork-htpc-bringup.md (Windows: sln + msbuild Release|x64)
```

`apply.sh` uses `git am --3way`, so a clean tree ends ten commits ahead of the pin.
To undo and return to pristine: `git -C vendor/rpcs3 checkout <pinned-commit>` (or
`git am --abort` if an apply is in progress).

The **editable home** of these patches is the dev clone at `D:\workspace\rpcs3`,
branch `spike-patches` (= pin + the ten commits). Edit there, then re-export (below).

## Rebase onto a newer upstream commit (bumping the pin)

When upstream moves and we want newer fixes:

```bash
cd vendor/rpcs3
git fetch origin
NEW=<new-commit-sha>

# 1. Try to replay the series onto the new base.
git checkout "$NEW"
git am --3way ../../rpcs3-patches/0*.patch
#    └─ clean? great. Conflicts? resolve each hunk, `git add`, `git am --continue`.
#       (The seams rarely churn; expect this to be free or near-free.)

# 2. Re-export the (possibly conflict-resolved) series back into the repo.
rm ../../rpcs3-patches/0*.patch
git format-patch "$NEW" -o ../../rpcs3-patches/

# 3. Record the new pin: move the submodule gitlink + update the docs.
cd ../..
git add vendor/rpcs3 rpcs3-patches/
#    update the pin in: this file, docs/research/rpcs3-integration-strategy.md,
#    docs/dev/rpcs3-fork-htpc-bringup.md, and the memory note.

# 4. Rebuild + smoke-test (tools/rpcs3-ipc/) before committing the bump.
```

CI verifies step 1 (apply-clean) on every change to this directory or the pin —
see `.github/workflows/rpcs3-patched.yml`. That lane is the guard against silent
patch-rot when the pin moves; the full patched build (Windows + macOS) is a
manual, gated lane in the same workflow.

## Tests

These C++ patches are tested **from Rust** (no C++ test infra added — keeps the
patch shallow). Two layers pin both sides of the IPC contract:

- **Wire contract (CI, no RPCS3):** `crates/rpcs3-control/src/ipc/proto.rs` (codec
  unit tests) + `crates/rpcs3-control/tests/ipc_loopback.rs` (the real
  `IpcPortalDriver` against an in-process fake P1 server).
- **Real emulator (HTPC, `#[ignore]`d):** `crates/rpcs3-control/tests/live_ipc.rs`
  drives the patched binary over the socket (listener up, `g_skyportal`
  load/clear, STATE frame counter advancing, window handle published).

**If a patch changes wire behaviour, update the fake server in `ipc_loopback.rs`
to match** — it doubles as the executable spec the live binary must satisfy.

**P3 has no dedicated test.** It flips a single guest-visible errno on the
offline `connect()` path, and the bug it fixes is gated on a real game's retry
policy — a unit test would only re-assert the constant. It's validated by the
controller's live freeze-repro (the freeze it cures). Called out here per the
project's "no silent caps" rule rather than faking coverage.

## License

These patches are a derivative work of RPCS3 and are therefore **GPL-2.0** (RPCS3's
license). The whole repository is GPL-2.0 to match — see `/LICENSE`.
