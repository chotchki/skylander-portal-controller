# Building the patched RPCS3 on macOS (Apple Silicon)

> Status: **proven 2026-05-31.** The patched RPCS3 builds and runs on macOS
> (`rpcs3 --version` → exit 0, IPC patch compiled in). This reframes the old
> "macOS = mock-only, patched mac RPCS3 is an explicit non-goal" stance
> (PLAN 16.8.1 / CLAUDE.md): the Phase-16 AF_UNIX IPC pivot removed the Windows
> lock-in (UI Automation), so the *same* control path works on macOS/Linux.

## TL;DR — one command

```sh
./.ci-local/build-mac.sh
# → vendor/rpcs3/build/bin/rpcs3.app/Contents/MacOS/rpcs3  (ad-hoc signed)
```

Prereqs: Homebrew + Xcode command line tools. The script installs everything
else and is idempotent. ~20–40 min cold; minutes warm (ccache at
`/tmp/ccache_dir`). It mirrors upstream `vendor/rpcs3/.ci/build-mac.sh` but uses
Homebrew's Qt instead of the fragile `qt-downloader`, and static-links
Homebrew's `llvm@21` (so the vendored LLVM/opencv/SDL submodules are skipped).

## Why "how bad is it" turned out **not bad**

The control layer was already cross-platform before any of this:

| Layer | State on macOS |
| --- | --- |
| `IpcPortalDriver` (AF_UNIX, the portal control transport) | **Already cross-platform + proven green** — `cargo test -p skylander-rpcs3-control --test ipc_loopback` passes (real driver ↔ real socket). |
| P1 patch (emulator-side AF_UNIX listener) | Portable POSIX sockets behind `#ifdef _WIN32`; socket path/env (`SKYLANDER_IPC_PATH`, `/tmp/rpcs3-skylander.sock`) **matches the Rust `default_socket_path()` exactly**. |
| P2 patch (borderless window + handle) | Qt-portable; the only Windows-specific bit (`SetWindowPos` z-order) is `#ifdef _WIN32`. |
| Building the patched binary | The actual work — pure environment plumbing, now captured in `.ci-local/build-mac.sh`. |

The remaining code gap *was* the **process lifecycle** (`RpcsProcess`), which
was Windows-only and `bail!`'d on non-Windows. **Closed 2026-06-01 (PLAN
16.11):** `UnixRpcsProcess` is now wired into the `RpcsProcess` enum (the `Unix`
variant) and `BootDirect`'s IPC spawn path is cross-platform, so
`SKYLANDER_PORTAL_DRIVER=ipc` drives the patched Mac binary the same way it
drives Windows. See "Running the server against the patched Mac binary" below.

## The build gotchas (all handled by the script)

These cost real iteration; the script encodes the fixes:

1. **Submodules (~2.4 GB).** A full recursive checkout also pulls RPCS3's
   vendored LLVM (~1.9 GB) which we **don't need** (we static-link brew
   `llvm@21`). The script pulls only the needed submodules (upstream's
   `awk … !/llvm/ && !/opencv/ && !/SDL/` filter).

2. **Homebrew Qt is split kegs.** RPCS3's `find_package(Qt6 COMPONENTS …)`
   resolves sub-components as *siblings of `Qt6_DIR`* with `NO_DEFAULT_PATH`, so
   every component's cmake config must co-locate. Fix: `brew unlink qt` (the
   monolithic umbrella) + `brew link --overwrite --force` each split formula
   (`qtbase qtsvg qtmultimedia qtdeclarative qtimageformats qttools`), then point
   `Qt6_DIR` at the **aggregate** `$(brew --prefix)/lib/cmake/Qt6`. Symptom if
   wrong: `cmake` configures but the *generate* step fails with
   `Qt6::Multimedia … target was not found`.

3. **Vendored vs brew protobuf/ffmpeg/fmt.** RPCS3 compiles pre-generated
   `*.pb.h` gencode (protobuf **33.4**) against its vendored protobuf. Homebrew's
   protobuf (35.0) leaks via `$(brew --prefix)/include` ahead of the vendored
   copy → `error: "Protobuf C++ gencode is built with an incompatible version"`.
   Fix: `brew unlink protobuf ffmpeg fmt` so the vendored copies win
   (`USE_SYSTEM_FFMPEG=OFF`). **Configure with these already unlinked** — a stale
   `build/CMakeCache.txt` that baked `/opt/homebrew/bin/protoc` re-triggers it.

4. **MoltenVK Vulkan loader.** cmake's Vulkan check wants
   `$VULKAN_SDK/lib/libvulkan.dylib`; the script symlinks the real loader into
   the MoltenVK keg (upstream does the same).

5. **macdeployqt leaves an invalid signature → `SIGKILL (Code Signature
   Invalid)` at launch.** The `rpcs3` target's `macdeployqt` POST_BUILD step
   (rpcs3/CMakeLists.txt) copies Qt + MoltenVK/abseil/brotli into
   `Contents/Frameworks` and rewrites their install names, which **invalidates
   those dylibs' existing (brew) code signatures**. dyld then SIGKILLs the
   process while mapping a tampered page (crash report: `EXC_BAD_ACCESS …
   SIGKILL (Code Signature Invalid)`, `namespace CODESIGNING`) — this is the
   "rpcs3 closed unexpectedly" dialog. **Fix: re-sign the whole bundle ad-hoc**
   after the build (`codesign --force --deep --sign - rpcs3.app`); the script
   does this (plus an idempotent `install_name_tool -add_rpath
   @executable_path/../Frameworks` so the bundle's own copies are preferred).
   Distribution (Developer ID) signing is a separate TODO.

6. **Duplicate OpenMP runtime → `OMP: Error #15` abort.** With the signature
   fixed, `rpcs3 --version` first aborted (exit 134): `multiple copies of the
   OpenMP runtime have been linked` — RPCS3's static `llvm@21` OpenMP + the
   bundle's `libomp.dylib`. **Fix (verified): `KMP_DUPLICATE_LIB_OK=TRUE` in the
   launch env → `rpcs3 --version` exits 0** (`RPCS3 0.0.40-local_build Alpha`).
   `UnixRpcsProcess::spawn` sets it on macOS so the spawned emulator inherits it;
   the clean fix is to de-duplicate `libomp` in the bundle (distribution polish).
   This is an upstream-RPCS3-macOS runtime quirk, **not** part of the IPC control
   work — it gated a clean launch, not the build or the architecture.

## Known non-blocking warning: deployment target

Link warning: `object file () was built for newer 'macOS' version (14.4) than
being linked (11.0)`.

- The produced binary is stamped `minos 11.0` (inherited from Homebrew's static
  libs, which target 11.0), while our objects compile at 14.4.
- **Harmless on current Macs** — verified: `rpcs3 --version` exits 0 here
  (sdk 26.2). It would only matter if the binary were run on macOS 11–13 (claims
  11.0 but contains 14.4 objects → possible missing-symbol crash on old OS).
- We don't ship the mac RPCS3 build today, so it's not a blocker. **If/when we
  distribute**, pin one consistent `CMAKE_OSX_DEPLOYMENT_TARGET` (11.0 = broad
  compat and silences the warning; or force 14.4 everywhere).

## What's proven vs pending (on this Mac)

Proven:
- ✅ Patched RPCS3 **builds** (`.ci-local/build-mac.sh` → exit 0).
- ✅ Binary is **patched** (`rpcs3-skylander.sock` present) and **runs**
  (`--version` exit 0). `.app` bundles Qt6 + MoltenVK via `@rpath`.
- ✅ Controller half end-to-end (`ipc_loopback` green) + socket path agreement.

Pending (needs a Skylanders game + firmware, which this Mac lacks):
- ⏳ Full emulator-side `LOAD`/`CLEAR` handshake. The P1 listener only starts in
  the `usb_device_skylander` ctor — i.e. **after a Skylanders game boots** and
  opens the USB peripheral. Without a game the socket never binds, so a live
  `LOAD`/`CLEAR` can't be exercised here. Provable on the HTPC / a Mac with a
  game (mirror `tests/live_ipc.rs`).

## Running the server against the patched Mac binary

The wiring (PLAN 16.11) is done — `SKYLANDER_PORTAL_DRIVER=ipc` on macOS spawns
and supervises the real patched binary over AF_UNIX, exactly like Windows. The
macOS *compiled default* stays `Mock` (Mac doesn't bundle a patched RPCS3 yet),
so IPC is **opt-in via env**. In `.env.dev` (dev-tools build):

```sh
SKYLANDER_PORTAL_DRIVER=ipc
# The patched binary built by .ci-local/build-mac.sh:
RPCS3_EXE=/abs/path/to/vendor/rpcs3/build/bin/rpcs3.app/Contents/MacOS/rpcs3
# RPCS3's data/config root: holds dev_flash (firmware) + config/games.yml.
# May live apart from the exe; defaults to the exe's parent if unset.
RPCS3_CONFIG_DIR=/abs/path/to/your/rpcs3-config-root
DATA_ROOT=./dev-data
```

Then `cargo run -p skylander-server`. On `/api/launch` the server runs
`rpcs3 --no-gui <EBOOT.BIN>` with `SKYLANDER_IPC_PATH` + `RPCS3_CONFIG_DIR` set,
waits on the IPC `STATE` liveness signal (status=running + frames advancing +
compile complete), then drives load/clear over the socket. `RPCS3_CONFIG_DIR`
gets a trailing separator appended automatically (the 16.9.5a `get_config_dir`
gotcha is cross-platform — RPCS3 normalises `\`→`/` then strips the last
component, so a bare dir would otherwise lose a level and trigger the firmware
"welcome" wizard).

**Out of scope on macOS (PLAN 16.11):** window coordination (z-order /
positioning) is Win32-only, so the borderless `SKYLANDER_BORDERLESS` env is *not*
set on Mac — RPCS3 shows a normal decorated, movable game window that coexists
with the egui launcher as a plain sibling. Portal control over IPC is the
cross-platform value; window choreography stays Windows-only.

## Remaining follow-ons (backlog)

1. **Live test** against a real game (a Mac with a Skylanders title) to close the
   `LOAD`/`CLEAR` gap end-to-end — mirror `crates/rpcs3-control/tests/live_ipc.rs`
   / `live_launch.rs`. The P1 listener only binds after a Skylanders game boots
   and opens the USB peripheral, so a live handshake needs game + firmware.
2. **Distribution polish:** pin one `CMAKE_OSX_DEPLOYMENT_TARGET`, de-dup
   `libomp` in the bundle (drops the `KMP_DUPLICATE_LIB_OK` workaround), and
   Developer-ID `.app` codesigning / notarization.
