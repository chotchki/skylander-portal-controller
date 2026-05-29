# RPCS3 fork — HTPC bringup + P1 live-test handoff

Self-contained handoff for continuing PLAN Phase 16 on the Windows HTPC, in case
the Mac session that did the 16.1.1 spike does not resume. Strategy + rationale:
`docs/research/rpcs3-integration-strategy.md`. This doc is the **what to do next**.

## State as of this handoff

- ✅ **16.1.1 done** (seam location). Probed RPCS3 master `c11979d` (2026-05-29).
  Verdict: **strong go** — both patch seams are shallow + additive.
- ⏭️ **Next: 16.1.2** (P1 live feed) then **16.1.3** (P2 window) — both need the
  HTPC (real games, GPU, Windows window behavior).
- **Pin decision:** latest master. Re-clone fresh on the HTPC; record the exact
  commit you build in this doc when you start (master moves — `c11979d` was the
  spike commit, build whatever is current and note it).
- **Build record (HTPC, 2026-05-28):** cloned to `D:\workspace\rpcs3`; HEAD =
  `c11979d1245509478145da11d7fcbe4e8815dd15` (master had not moved — identical to
  the spike commit). Following upstream **CI's MSVC route** (`.github/workflows/rpcs3.yml`
  `Windows_Build`), not the CMake preset: build `rpcs3.sln` with `msbuild` using
  **precompiled LLVM libs** (`llvmlibs_mt.7z`, LLVM 19.1.7) dropped into
  `build/lib_ext/Release-x64` — the CMake `msvc` preset sets `BUILD_LLVM=ON` and
  would compile LLVM from source (hours). Pins from CI: Qt **6.11.1** `msvc2022_64`
  (modules qtbase/qtdeclarative/qttools/qtmultimedia/qtsvg/qttranslations), Vulkan
  SDK **1.4.341.1** (exact), submodules init **excluding llvm/FAudio/feralinteractive**.

## The two seams (verified — reuse, don't reverse-engineer)

### P1 — portal control (`rpcs3/Emu/Io/Skylander.h`, 57 lines, stable)
A process-global singleton with the exact API, already public + mutex-guarded:

```cpp
extern sky_portal g_skyportal;
class sky_portal {
  u8   load_skylander(u8* buf, fs::file in_file);  // load → returns portal slot
  bool remove_skylander(u8 sky_num);               // clear slot
  void activate(); void deactivate();
  skylander skylanders[8];                          // 8-slot model
};
```

The GUI (`rpcs3/rpcs3qt/skylander_dialog.cpp`) drives the portal **entirely**
through this global — see `clear_skylander()` / `load_skylander_path()`:
`g_skyportal.load_skylander(data.data(), std::move(sky_file))` and
`g_skyportal.remove_skylander(cur_slot)`. **The IPC server calls the same two
functions on the same singleton.** USB `control_transfer`/`interrupt_transfer`
stay untouched.

- `load_skylander` takes ownership of an `fs::file` and keeps the handle to write
  on-tag changes back (`skylander::save()`). Pass it a path to a working-copy
  `.sky` (the controller already manages those).

### P2 — window (`rpcs3/Emu/RSX/GSFrameBase.h` + `rpcs3/rpcs3qt/gs_frame.cpp`)
`GSFrameBase` already exposes `virtual display_handle_t handle() const = 0;`
(plus `show/hide/shown/client_width/toggle_fullscreen`). Concrete window =
`gs_frame`.

**`--no-gui` is the launch mode** (game window, no main GUI/menus/Skylanders
dialog; pair with `--fullscreen`). `--headless` = NO render window — not what we
want. **A first merged experience may need no P2 patch at all** — stock
`--no-gui --fullscreen` already gives a clean menu-free game window. Borderless /
supplied-geometry / `SetParent` nesting + emitting `handle()` over IPC is polish
(16.4 / 16.6.2), not a 16.1.2 prerequisite.

## HTPC bringup steps

### 0. Prereqs (one time)
- Visual Studio 2022 (Desktop C++ workload), CMake, Git, Qt (RPCS3's
  `BUILDING.md` lists the exact Qt version). Follow upstream `BUILDING.md` —
  don't improvise the toolchain.

#### Toolchain as actually installed on the HTPC (2026-05-28) — verified working
The MSVC sln route (what CI uses) needs these; versions are pinned by upstream
(`.github/workflows/rpcs3.yml` `Windows_Build` env + `BUILDING.md`):

| Tool | Version | How it was installed | Notes |
|---|---|---|---|
| VS 2022 Community | 17.14 | (pre-existing) | Desktop C++ workload; provides MSBuild + cl + Windows SDK |
| Git | 2.53 | (pre-existing) | |
| Python | 3.13.13 | `winget install Python.Python.3.13 --scope user` | no admin; needed for aqt + cmake/ninja pip pkgs |
| Qt | **6.11.1** `msvc2022_64` | **direct 7z download** (see gotcha) → `D:\workspace\Qt\6.11.1\msvc2022_64` | `QTDIR` points here |
| Vulkan SDK | **1.4.341.1** (exact) | LunarG installer, **run elevated** → `D:\workspace\VulkanSDK\1.4.341.1` | `VULKAN_SDK` points here |
| LLVM libs | 19.1.7 precompiled | `llvmlibs_mt.7z` → `build/lib_ext/Release-x64` | skips from-source LLVM |
| CMake | 3.31.6 | `pip install cmake==3.31.6` (in Python `Scripts\`) | **needed on PATH** by 3rdparty MakeFile projects |
| Ninja | 1.13 | `pip install ninja` (in Python `Scripts\`) | **needed on PATH** (protobuf/openal use `-G Ninja`) |
| NuGet | latest | single `nuget.exe` download | `nuget restore rpcs3.sln` (test project's packages.config) |

Build env (see `D:\workspace\rpcs3\build_rpcs3.bat`): prepend Python `Scripts\`
(cmake+ninja) and `…\Microsoft Visual Studio\Installer` (vswhere) to PATH, set
`QTDIR` + `VULKAN_SDK`, then
`msbuild rpcs3.sln /p:Configuration=Release /p:Platform=x64 /m`.

**Gotchas hit (carry into the 16.2 CI lane):**
1. **`aqtinstall` 3.3.0 can't resolve Qt 6.11.1** — the 6.11.x repo layout moved
   the arch into the inner folder (`…/desktop/qt6_6111/qt6_6111_msvc2022_64/Updates.xml`),
   but aqt still computes the old doubled `qt6_6111/qt6_6111/`. 3.3.0 is the latest
   on PyPI, so no upgrade fixes it. **Mirror CI instead**: download the per-arch 7z
   archives directly (qtbase/qtdeclarative/qttools/qttranslations/qtsvg + d3dcompiler
   + opengl32sw from `qt.qt6.6111.win64_msvc2022_64/`, and qtmultimedia from
   `qt.qt6.6111.addons.qtmultimedia.win64_msvc2022_64/`), each prefixed with the build
   date `6.11.1-0-202605090529`, verified against `${url}.sha1`. Extract bare into
   `…/6.11.1/msvc2022_64/`.
2. **Vulkan SDK installer refuses to self-elevate in headless mode** ("Cannot elevate
   access rights while running from command line"). Launch it already-elevated
   (`Start-Process -Verb RunAs`) so it never needs to self-elevate; `--root <dir>`
   sets the target so it can live on D:.
3. **3rdparty MakeFile projects need cmake/ninja/vswhere on PATH.** `glslang`,
   `protobuf_build`, `openal-soft` shell out to `vsdevcmd.bat` + cmake + ninja. Without
   them: `'vswhere.exe' is not recognized` and exit 9009. The VS "C++ CMake tools for
   Windows" component would also supply these; pip cmake/ninja avoids touching the VS install.
4. **Pin CMake to 3.x** (3.31.6) — CMake 4.x hard-errors on `cmake_minimum_required < 3.5`
   in some deps; CI uses VS-integrated ~3.29.
5. The CMake `msvc` preset sets `BUILD_LLVM=ON` (from-source). Do **not** use it; the
   sln route + precompiled `llvmlibs_mt.7z` is what CI does and skips the LLVM compile.
   `llvm_build` has no `.Build.0` in the sln's Release|x64, so "Build Solution" won't
   compile LLVM.

### 1. Clone + record the pin
```
git clone https://github.com/RPCS3/rpcs3.git
cd rpcs3
git submodule update --init --recursive   # full build needs submodules (3rdparty)
git rev-parse HEAD                          # ← record this commit in this doc
```
(The 16.1.1 spike used a `--depth 1 --no-tags` clone with NO submodules — that was
read-only seam location. A real build needs the submodules.)

### 2. Baseline build + sanity boot (before any patch)
Build per `BUILDING.md`. Then confirm stock no-gui direct-boot works — this
isolates "does RPCS3 run my game" from "does my patch work":
```
rpcs3.exe --no-gui "<game_dir>\PS3_GAME\USRDIR\EBOOT.BIN"
```
Use a known game from the dev pack. If a game crashes/freezes here, that's the
upstream-stability problem (Phase 16.7 supervisor territory) — note which game +
commit, it's data for the per-game pin.

### 3. P1 patch — IPC listener calling g_skyportal (16.1.2 / 16.3.1)
Minimal proof-of-life patch (keep it shallow — that's the whole thesis):
- Add a small listener (loopback TCP or a Windows named pipe) started when the
  Skylander USB device is instantiated (`usb_device_skylander` ctor) or behind a
  flag in emu init.
- Protocol (prototype): a line/length-framed command set —
  `LOAD <slot> <path-to-.sky>` → call `g_skyportal.load_skylander(buf, fs::file)`;
  `CLEAR <slot>` → `g_skyportal.remove_skylander(slot)`; `STATUS` → read
  `g_skyportal.skylanders[]`. Mirror what `load_skylander_path()` does (read file
  into the 0x40*0x10 buf, then call the singleton).
- **Acceptance (16.1.2):** boot a Skylanders game with `--no-gui`, send LOAD over
  the socket, and the **game itself** reacts (figure appears on the in-game
  portal) with ZERO dialog interaction; CLEAR removes it. That proves the entire
  architecture.

### 4. P2 prototype — borderless + handle (16.1.3) — only after P1 passes
- In `gs_frame` ctor: apply borderless/undecorated flags + the
  controller-supplied geometry; avoid focus-steal.
- Emit `handle()` over the P1 IPC channel on window creation.
- **Acceptance:** controller receives the native handle and can position the
  window (and on Windows, `SetParent` it under the egui launcher).

### 5. Record go/no-go (16.1.4)
Append results to `docs/research/rpcs3-integration-strategy.md`: did P1 drive the
game? how shallow was the patch in practice? then tick 16.1.4 and proceed to 16.2
(submodule + CI).

## Gotchas to carry over (from CLAUDE.md)
- **IPC is NOT session-bound** (unlike UIA/SendInput) — the old "must run on the
  interactive desktop, SSH session 0 can't see windows" rule is a UIA limitation;
  a loopback socket to RPCS3 works regardless. You still want to be physically at
  the HTPC to *watch a game boot*, but portal control itself isn't session-gated.
- `RPCS3.buf` singleton lockfile next to `rpcs3.exe` survives a forced kill →
  next launch fails. Delete it on forced shutdown (the controller's
  `RpcsProcess::shutdown_graceful` already does this for the wrapped path).
- Disable Settings → Advanced → "Automatically check for updates at startup" so
  the update popup doesn't interfere at boot.

## Known dev paths (from CLAUDE.md)
- RPCS3 dev install: `C:\emuluators\rpcs3` (sic — real path).
- Firmware pack: `C:\Users\chris\workspace\Skylanders Characters Pack for RPCS3`.
- Serial → game dir map: `<rpcs3>/config/games.yml`; EBOOT at
  `<game_dir>/PS3_GAME/USRDIR/EBOOT.BIN`.
