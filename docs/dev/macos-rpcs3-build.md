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

The remaining code gap is the **process lifecycle** (`RpcsProcess`), which is
Windows-only and `bail!`s on non-Windows. See "Next steps" below.

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

## Next steps (code)

The server already supports `DriverKind::Ipc` cross-platform; the missing piece
is a real Unix process lifecycle.

1. **`UnixRpcsProcess`** (`crates/rpcs3-control/src/process_unix.rs`) — **done,
   additive.** Spawns rpcs3 with `SKYLANDER_IPC_PATH` set, `wait_ready` polls the
   socket, `shutdown_graceful` is SIGTERM→SIGKILL. Unit-tested with `/bin/sleep`.
   Not yet wired into the `RpcsProcess` enum (mock is still the non-Windows
   default, so dev flow is unchanged).
2. **Wire it in** (the reviewed change): make the non-Windows `RpcsProcess` an
   enum `Mock | Real(UnixRpcsProcess)`; have `lifecycle.rs::spawn_rpcs3` pick
   `Real` when `driver == Ipc` (it already keys off `driver.ipc_socket_path()`);
   thread `socket_path` + `boot=NoGui` through `LaunchConfig` (config.rs). The
   `DriverKind::Mock` path stays the default and untouched. The BootDirect block
   in `state.rs` is `#[cfg(windows)]` today — its IPC sub-branch is already
   socket-keyed and would move behind a `cfg(any(windows, unix))` guard.
3. **Live test** against a real game (HTPC or a Mac with a Skylanders title) to
   close the `LOAD`/`CLEAR` gap.
