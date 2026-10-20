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
  0004-P4-…-PE-events.patch           P4 — push guest portal-events (PE) over IPC (Skylander.cpp)
  0005-P5-…-RECONNECT.patch           P5 — re-attach the portal after a save-state resume (Skylander.cpp + sys_usbd)
  0006-P6-…-BUTTON_PRESS.patch        P6 — inject gamepad buttons over IPC (Skylander.cpp + pad_thread)
  0007-P7-…-WINDOW_SET.patch          P7 — tile the game window over IPC (gs_frame.cpp + Skylander.cpp)
  0008-P8-…-SURFACE.patch             P8 — macOS surface-embed: publish CAMetalLayer via CAContext (CALayerHost)
  apply.sh                            apply the series onto a checkout (git am --3way)
  README.md                           this file
  oracle/                             the dev-only O-series (Guacamole's oracle, openspec AF); see below
```
(The two local SPU-LLVM Giga crash fixes that used to sit between P3 and P4 were merged upstream as RPCS3 #18935 and dropped on the 2026-06-24 pin bump to `09d602fd5` — the series is now a clean P1-P8.)

Bump history for the curious: `c11979d` -> `09d602fd5` (109 commits) -> `accfecd2d`
(**516 commits**, 2026-09-20). That last one is the widest window we have replayed and
only two hunks needed hands — a neighbour-function collision in `sys_usbd` and one
call-site signature change in P1. The seams are holding.

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
What each does:

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
| Pinned commit | `accfecd2d13ec634e96d0edf97777e657f2d9359` (master, 2026-09-20, `v0.0.42-353`) |
| Patches generated against | that commit **plus the CRLF->LF normalization `apply.sh` applies** (see below) |

Pin is by **commit**, not branch — `.gitmodules` has no `branch =`, so
`git submodule update` never silently moves it. Latest-master was chosen for
newest game-compat + crash fixes; rebasing is cheap because the patches are
shallow + additive.

## Build a patched RPCS3 locally

```bash
git submodule update --init --recursive vendor/rpcs3   # full build needs 3rdparty
rpcs3-patches/apply.sh                                  # → 8 commits on top of the pin
# then build per docs/dev/rpcs3-fork-htpc-bringup.md (Windows: sln + msbuild Release|x64)
```

`apply.sh` uses `git am --3way`, so a clean tree ends eight commits ahead of the pin.
To undo and return to pristine: `git -C vendor/rpcs3 checkout <pinned-commit>` (or
`git am --abort` if an apply is in progress).

The **editable home** of these patches is the dev clone at `D:\workspace\rpcs3`,
branch `spike-patches` (= pin + the eight commits). Edit there, then re-export (below).

## Rebase onto a newer upstream commit (bumping the pin)

When upstream moves and we want newer fixes:

```bash
cd vendor/rpcs3
git fetch origin
NEW=<new-commit-sha>

# 1. Try to replay the series onto the new base. Use apply.sh, NOT a bare `git am` —
#    it lays down the CRLF->LF normalization commit the series is generated against
#    (see "The CRLF base commit" below). A bare `git am` fails on P8.
git checkout "$NEW"
../../rpcs3-patches/apply.sh .
#    └─ clean? great. Conflicts? resolve each hunk, `git add`, `git am --continue`.
#       (The seams rarely churn; expect this to be free or near-free.)

# 2. Re-export the (possibly conflict-resolved) series back into the repo. Export
#    from the NORMALIZATION commit (HEAD~8 after a clean apply), not from $NEW —
#    exporting from $NEW would emit the normalization as a 9th patch.
BASE=$(git rev-parse HEAD~8)
rm ../../rpcs3-patches/0*.patch
git format-patch "$BASE" -o ../../rpcs3-patches/

# 3. Record the new pin: move the submodule gitlink + update the docs. Stage the
#    gitlink EXPLICITLY — a plain `git add vendor/rpcs3` records whatever the
#    submodule worktree is sitting on, which after step 1 is the PATCHED tip. That
#    breaks the pristine-pin invariant (and CI, which reads the pin from the gitlink).
cd ../..
git update-index --cacheinfo 160000,"$NEW",vendor/rpcs3
git add rpcs3-patches/
#    update the pin in: this file, docs/research/rpcs3-integration-strategy.md,
#    docs/dev/rpcs3-fork-htpc-bringup.md, the TWO hardcoded `rpcs3-patched-<pin>`
#    download tags in .github/workflows/release.yml, and the memory note.
#
# 3b. Sync the Windows build's DEPENDENCY pins. `rpcs3-patched.yml`'s build-windows
#    env block (QT_VER / QT_DATE / LLVM_VER / VULKAN_* / CCACHE_SHA) is a hardcoded
#    copy of upstream's Windows_Build env, and it picks which prebuilt drops
#    `.ci/setup-windows.sh` fetches. It rots on every bump that moves them:
#      git show "$NEW":.github/workflows/rpcs3.yml | sed -n '/^  Windows_Build:/,/steps:/p'
#    A guard step in that job now diffs the two and fails in seconds, so you will be
#    told rather than finding out half an hour into a compile — but updating them
#    here first saves the round trip.

# 4. Rebuild + smoke-test (tools/rpcs3-ipc/) before committing the bump.
```

CI verifies step 1 (apply-clean) on every change to this directory or the pin —
see `.github/workflows/rpcs3-patched.yml`. That lane is the guard against silent
patch-rot when the pin moves; the full patched build (Windows + macOS) is a
manual, gated lane in the same workflow.

### The CRLF base commit

One upstream file in the series set — `rpcs3/Emu/RSX/VK/vkutils/swapchain_macos.hpp` —
is stored CRLF. `git am` strips CR from a patch body before applying (that is
`--no-keep-cr`, the default), so a CRLF source file can never match an LF patch
context and P8 fails with "patch does not apply" on a pristine checkout. `apply.sh`
fixes this the only way that is stable across Windows git-bash and Unix: it normalizes
BOTH sides to LF unconditionally, landing the source normalization in one commit
*below* the eight patch commits. LF compiles identically, and CI's
`HEAD~<#patches>` expected-files check is unaffected because the normalization sits
under that window.

The corollary, and the thing to remember when re-exporting: **the series is generated
against that normalization commit, not against the bare pin.** `git format-patch $PIN`
would emit nine patches. Use `HEAD~8`.

(Before the 2026-09-20 bump the committed patch files still carried CRLF themselves,
so every local `apply.sh` run rewrote them in place and left `rpcs3-patches/` dirty.
They are now pure LF — byte-identical to what `git am` actually consumes, so that
normalization step is a true no-op.)

## The oracle series (dev-only, openspec AF)

Guacamole checks itself against the original game running in a patched RPCS3, the
**oracle** ([`docs/native/guacamole-design.md`](../docs/native/guacamole-design.md), The
oracle). The oracle is this pin + P1-P8 + the **O-series** in `oracle/`, and it is never a
product build. Three walls keep it out of one: the directory (the product's `apply.sh`, its
CI lane's expected-file check and `.ci-local/build-mac.sh` all glob `0*.patch` in THIS dir
only), the build location (a checkout outside the repo, never `vendor/rpcs3/build`, never
uploaded) and an env gate (every O-series IPC verb answers P1's `ERR unknown_cmd` unless
`GUAC_ORACLE=1` is in the emulator's environment). A change in `oracle/` still triggers the
`rpcs3-patched` lane (its filter is `rpcs3-patches/**`), which re-checks P1-P8 and passes.

```
rpcs3-patches/oracle/
  0001-O1-vk-Support-multiple-attachments-on-renderpass-key.patch   upstream c938d3e049
  0002-O2-vk-Fix-renderpass-keys-attached-to-property-blobs.patch   upstream d08d568d5e
  0003-O3-oracle-capture-and-control-verbs-over-the-P1-sock.patch   PAUSE_AT, RESUME, RSXCAP, SCREENSHOT, SAVESTATE
  0004-O4-oracle-frame-locked-input-on-pads-0-and-1-over-th.patch   PAD, PAD_CLEAR, PAD_STATE
  0005-O5-oracle-per-frame-recorder-over-the-P1-socket-GUAC.patch   WATCH, WATCH_READ, WATCH_CLEAR
  apply.sh   check P1-P8 on HEAD by subject, then git am --3way the O-series (--check: verify only)
  build.sh   clone/patch a checkout outside the repo, build it with build-mac.sh, install + identity
```

**O1 + O2 are upstream's fix for the pin's Mac boot crash**, cherry-picked with upstream's
authorship (kd-11, 2026-09-21, the day after the pin) and an `O<n>: ` subject prefix. `7022a322a`,
inside the `09d602fd5..accfecd2d` window, warms the shader interpreter with 2-4 colour attachments
against a 1-attachment render pass; MoltenVK reads past its attachment table and Metal aborts at
boot ("attachmentIndex(220) must be < 8"). They're the only O-patches without the env gate (they're
upstream's code), and the next pin bump past `d08d568d5e` drops them. chotchki's call, 2026-10-18
(AF design, Open question 1 (b)).

**O3 is the oracle's capture and control verbs** (AF.5) on P1's listener, each answering
`ERR unknown_cmd` unless `GUAC_ORACLE=1` is in the emulator's environment: `PAUSE_AT <flip>`,
`RESUME`, `RSXCAP`, `SCREENSHOT` and `SAVESTATE`, every flip being the `frames=` counter `STATE`
already reports. It touches `Skylander.cpp` beside P1 and three spots in `RSXThread.cpp`: the
`PAUSE_AT` target, its reset with each RSX session and the check in `rsx::thread::flip` beside
Start Paused's (an absolute target checked on the RSX thread, because a relative count set from the
IPC thread races the RSX thread's decrement). The other four set the flags the game window's
shortcuts set, or run its savestate path on the main thread; nothing in them knows the game. The
wire shapes and what RPCS3 writes for each are in
[`tools/oracle-harness/src/verbs.rs`](../tools/oracle-harness/src/verbs.rs), and
[`tools/oracle-harness/tests/verbs_loopback.rs`](../tools/oracle-harness/tests/verbs_loopback.rs) is the
fake listener they're specified against, the same two-layer rule as P1 (Tests, below).

**O4 is the oracle's frame-locked input** (AF.7), behind the same gate: `PAD <port> <flip> <d1>
<d2> <lx> <ly> <rx> <ry>` schedules what port 0 or 1 reads from a flip on (`CELL_PAD_CTRL_*` words
in hex and the four stick bytes), `PAD_CLEAR` hands both back to the host's input and `PAD_STATE
<port> [<flip>]` reads back what the guest read, flip by flip. It touches `cellPad.cpp` (the
schedule and the readback, applied and recorded inside `pad_get_data` under `pad::g_pad_mutex`, so
every read of a flip sees that flip's entry whatever the host's pad poll did) and `Skylander.cpp`
beside O3. A new emulation session drops both. Its shapes are in `verbs.rs` with O3's, and
[`tools/oracle-harness/tests/pad_loopback.rs`](../tools/oracle-harness/tests/pad_loopback.rs) is its
fake. `build.sh` refuses to replace the installed bundle while an `rpcs3` from it runs.

**O5 is the oracle's per-frame recorder** (AG.1, AG.35), behind the same gate: `WATCH <frames>
<port> <addr> <len> ...` arms a ring of records of the guest ranges it names (up to 64, 16 KiB a
record, 16 MiB for the ring), taken at every read of pad port `<port>` (0-6); `WATCH_READ [<first>
<count>]` hands back its summary or its records at a pause, and `WATCH_CLEAR` drops it. The first
thing `pad_get_data` does, under `pad::g_pad_mutex` on the guest thread making the read, is the
record: every read of the watched port copies the ranges, so a game that reads that port once a
frame is recorded every frame at the same point in it without a pause, which a `PAUSE_AT`
checkpoint can't give (AF.11: a pause moves the run). Each record's 32-byte header carries the flip,
the host's clock, a mask of unreadable ranges and which of the port's reads in that flip it was, so
a flip that ran two game frames holds two records (reads 0 and 1); AG.1's O5 recorded only each
flip's first read on any port and lost the second (AG.35). Recording starts on a whole flip. What
to watch, and on which port, is the client's: Giants' addresses and its port 0 are the harness's
([`tools/oracle-harness/src/watch.rs`](../tools/oracle-harness/src/watch.rs)).
It touches `cellPad.cpp` and `Skylander.cpp` beside O3 and O4. A new emulation session drops the
watch. Its shapes are in `verbs.rs` with the others', and
[`tools/oracle-harness/tests/watch_loopback.rs`](../tools/oracle-harness/tests/watch_loopback.rs) is its
fake.

Build it on the Mac:

```bash
GUAC_ORACLE_EXE=~/workspace/guacamole-research/rpcs3-oracle-accfecd2d/RPCS3.app/Contents/MacOS/rpcs3 \
  rpcs3-patches/oracle/build.sh [--no-brew] ~/workspace/guacamole-research/oracle-src/rpcs3
```

`build.sh` clones the checkout when it's missing (from the submodule's objects, else upstream),
puts it on the pin read from the `vendor/rpcs3` gitlink, applies `../apply.sh` then
`oracle/apply.sh`, builds with `.ci-local/build-mac.sh <checkout>` and copies the `.app` to the
bundle `GUAC_ORACLE_EXE` names (refused inside any work tree). A checkout that already carries both
series builds as it stands; any other HEAD, or a dirty tree, is refused rather than reset.
`--no-brew` passes `BUILD_MAC_NO_BREW=1`, which leaves Homebrew alone: the script's `brew install`
upgrades whatever the local tap calls outdated and its unlink pulls `ffmpeg` out of `PATH`, both
machine-wide. The script pins `OpenCV_DIR` to the `opencv@4` keg (find_package otherwise takes the
linked `opencv`, 5.x since 2026-10-17). It prints the build's identity and writes it to
`oracle-build.txt` beside the `.app`: upstream base, the repo commit the series came from, the
checkout's tree, every patch's SHA-256, the binary's SHA-256 and the MoltenVK (Homebrew's, through
its ICD file, not bundled), LLVM and Qt it was built against. The source tree is the reproducible
part (commit hashes carry the apply time); the binary's hash isn't, so a rebuild records its own.

The O-patches touch `VKRenderPass.h`, which upstream stores CRLF, so `oracle/apply.sh` lays
down its own normalization commit between P8 and O1, as `../apply.sh` does under P1 (The CRLF base
commit, above). To edit or add an O-patch: commit on top of the oracle checkout, then re-export from
that normalization commit, `git format-patch HEAD~<#O-commits> -o rpcs3-patches/oracle/`. A pin
bump replays both series: `../apply.sh`, then `oracle/apply.sh`, resolve, re-export each.

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

**The O-series follows the same rule**, in `tools/oracle-harness`: the codec's unit
tests in [`src/verbs.rs`](../tools/oracle-harness/src/verbs.rs) and the fake listeners
[`tests/verbs_loopback.rs`](../tools/oracle-harness/tests/verbs_loopback.rs) (O3),
[`tests/pad_loopback.rs`](../tools/oracle-harness/tests/pad_loopback.rs) (O4) and
[`tests/watch_loopback.rs`](../tools/oracle-harness/tests/watch_loopback.rs) (O5) run in
`cargo test`; the `#[ignore]`d `tests/live_*.rs` drive the oracle build on the Mac
(`live_gate` boots it without `GUAC_ORACLE=1` and checks every O-verb answers
`ERR unknown_cmd`, `live_verbs`, `live_pad` and `live_watch` drive O3, O4 and O5 in a
level). O1 and O2
are upstream's code and have no test of their own beyond the oracle booting in the
default shader mode.

**P3 has no dedicated test.** It flips a single guest-visible errno on the
offline `connect()` path, and the bug it fixes is gated on a real game's retry
policy — a unit test would only re-assert the constant. It's validated by the
controller's live freeze-repro (the freeze it cures). Called out here per the
project's "no silent caps" rule rather than faking coverage.

## License

These patches are a derivative work of RPCS3 and are therefore **GPL-2.0** (RPCS3's
license). The whole repository is GPL-2.0 to match — see `/LICENSE`.
