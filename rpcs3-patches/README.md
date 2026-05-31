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
  apply.sh                            apply the series onto a checkout (git am --3way)
  README.md                           this file
```

The submodule **always points at a pristine upstream commit** — it is never
modified in place. The patches live here, in our repo, where the diff is
reviewable. They apply in filename order; **0002 (P2) depends on 0001 (P1)**
(P1 defines the `g_game_window_handle` global that P2 publishes into). **P3 is
independent** — a `sys_net` errno tweak unrelated to the portal device or window.

These three patches are the entire footprint on RPCS3 — all shallow + additive
on rarely-churning seams (the patch-depth thesis in the decision doc). What each
does:

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

## The pin

| | |
|---|---|
| Submodule | `vendor/rpcs3` → `https://github.com/RPCS3/rpcs3.git` |
| Pinned commit | `c11979d1245509478145da11d7fcbe4e8815dd15` (master, 2026-05-29) |
| Patches generated against | that same commit |

Pin is by **commit**, not branch — `.gitmodules` has no `branch =`, so
`git submodule update` never silently moves it. Latest-master was chosen for
newest game-compat + crash fixes; rebasing is cheap because both patches are
shallow + additive.

## Build a patched RPCS3 locally

```bash
git submodule update --init --recursive vendor/rpcs3   # full build needs 3rdparty
rpcs3-patches/apply.sh                                  # → 2 commits on top of the pin
# then build per docs/dev/rpcs3-fork-htpc-bringup.md (Windows: sln + msbuild Release|x64)
```

`apply.sh` uses `git am --3way`, so a clean tree ends two commits ahead of the pin.
To undo and return to pristine: `git -C vendor/rpcs3 checkout <pinned-commit>` (or
`git am --abort` if an apply is in progress).

The **editable home** of these patches is the dev clone at `D:\workspace\rpcs3`,
branch `spike-patches` (= pin + the two commits). Edit there, then re-export (below).

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
