#!/usr/bin/env bash
# Build the patched RPCS3 on macOS (Apple Silicon), driven from our repo so
# contributors get one command — the macOS counterpart to the Windows build
# (CI job build-windows in .github/workflows/rpcs3-patched.yml + the HTPC doc).
#
# Why this differs from upstream vendor/rpcs3/.ci/build-mac.sh:
#   * Upstream downloads a monolithic Qt via the fragile `qt-downloader` Python
#     script (~1.4 GB). We instead use Homebrew's Qt 6.11 (it's pulled in as a
#     transitive dep of opencv anyway) and work around Homebrew's split-keg
#     layout (see the Qt block below).
#   * We static-link Homebrew's llvm@21 (USE_SYSTEM_*), so the vendored LLVM /
#     opencv / SDL submodules are NOT needed and are skipped at checkout time.
#
# Prereqs: Homebrew + Xcode command line tools. Everything else is installed
# here. Targets the same arm64 macOS the CI's build-macos job uses.
#
# Usage: .ci-local/build-mac.sh   (usually invoked via ../build-rpcs3.sh)
set -euo pipefail

if [ "$(uname -s)" != "Darwin" ]; then
  echo "build-mac.sh is macOS-only (got $(uname -s))" >&2
  exit 1
fi
if ! command -v brew >/dev/null 2>&1; then
  echo "Homebrew is required (https://brew.sh)" >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VENDOR="$REPO_ROOT/vendor/rpcs3"
BUILD_DIR="$VENDOR/build"
BREW="$(brew --prefix)"

# Pin matches CI (.github/workflows/rpcs3-patched.yml build-macos).
LLVM_VER=21

# ---- submodules -------------------------------------------------------------
# Pull every 3rdparty submodule EXCEPT the ones we satisfy from Homebrew
# (llvm/opencv/SDL/feralinteractive) — exactly upstream build-mac.sh's filter.
echo "==> submodule checkout (skipping brew-satisfied llvm/opencv/SDL)"
git -C "$VENDOR" submodule update --init --depth=1 --jobs=8 \
  $(awk '/path/ && !/llvm/ && !/opencv/ && !/SDL/ && !/feralinteractive/ { print $3 }' "$VENDOR/.gitmodules")

# ---- patch series -----------------------------------------------------------
# Apply P1/P2 if the working tree isn't already patched (idempotent — apply.sh
# git-am's onto the pin, so it must only run once per checkout).
if ! git -C "$VENDOR" log --oneline -4 | grep -q "P1: drive the emulated Skylander"; then
  echo "==> applying RPCS3 patch series (P1 + P2)"
  bash "$REPO_ROOT/rpcs3-patches/apply.sh"
else
  echo "==> patch series already applied"
fi

# ---- Homebrew deps ----------------------------------------------------------
export HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_CLEANUP=1 HOMEBREW_NO_ENV_HINTS=1
echo "==> installing build deps via Homebrew"
brew install -f --overwrite --quiet \
  cmake ninja nasm ccache "llvm@${LLVM_VER}" googletest \
  opencv@4 sdl3 vulkan-headers vulkan-loader molten-vk \
  qtbase qtsvg qtmultimedia qtdeclarative qtimageformats qttools

# Homebrew ships Qt as split kegs (qtbase/qtsvg/qtmultimedia/...). The
# monolithic `qt` formula, if present, owns conflicting symlinks; and the split
# kegs may be only partially linked. RPCS3's find_package(Qt6 COMPONENTS ...)
# resolves sub-components as *siblings of Qt6_DIR* with NO_DEFAULT_PATH, so
# every component's cmake config must co-locate in $BREW/lib/cmake. Unlink the
# umbrella and force-link the split formulae to guarantee that.
echo "==> reconciling Homebrew Qt (split-keg layout)"
brew unlink qt 2>/dev/null || true
for f in qtbase qtsvg qtmultimedia qtdeclarative qtimageformats qttools; do
  brew link --overwrite --force "$f" >/dev/null 2>&1 || true
done

# RPCS3 vendors protobuf/ffmpeg/fmt and compiles its pre-generated *.pb.h
# gencode against the vendored protobuf. Homebrew's newer protobuf headers leak
# in via $BREW/include (ahead of the vendored copy) and break the build with
# "incompatible Protobuf C++ gencode". Unlink them so the vendored copies win
# (mirrors upstream build-mac.sh, which uses USE_SYSTEM_FFMPEG=OFF too).
echo "==> unlinking brew protobuf/ffmpeg/fmt (RPCS3 uses its vendored copies)"
brew unlink protobuf ffmpeg fmt >/dev/null 2>&1 || true

# RPCS3's Vulkan detection points at MoltenVK (VULKAN_SDK) but loads the real
# loader's libvulkan.dylib; upstream symlinks it into the MoltenVK keg.
echo "==> linking libvulkan into the MoltenVK keg"
ln -sf "$BREW/opt/vulkan-loader/lib/libvulkan.dylib" \
       "$BREW/opt/molten-vk/lib/libvulkan.dylib"

# ---- configure --------------------------------------------------------------
export CC=clang CXX=clang++
export PATH="$BREW/opt/llvm@${LLVM_VER}/bin:$BREW/bin:$BREW/sbin:$PATH"
export LDFLAGS="-L$BREW/opt/llvm@${LLVM_VER}/lib/c++ -L$BREW/opt/llvm@${LLVM_VER}/lib/unwind -lunwind"
export LLVM_DIR="$BREW/opt/llvm@${LLVM_VER}"
export VULKAN_SDK="$BREW/opt/molten-vk"
export SDL3_DIR="$BREW/opt/sdl3/lib/cmake/SDL3"
export Qt6_DIR="$BREW/lib/cmake/Qt6"          # aggregate dir — all components co-located
export CMAKE_PREFIX_PATH="$BREW"
export CCACHE_DIR="${CCACHE_DIR:-/tmp/ccache_dir}"

echo "==> cmake configure (Ninja, brew Qt6, system MoltenVK/SDL/OpenCV, static LLVM)"
cmake -S "$VENDOR" -B "$BUILD_DIR" -G Ninja \
  -DBUILD_RPCS3_TESTS=OFF -DRUN_RPCS3_TESTS=OFF \
  -DCMAKE_OSX_DEPLOYMENT_TARGET=14.4 \
  -DCMAKE_OSX_SYSROOT="$(xcrun --sdk macosx --show-sdk-path)" \
  -DSTATIC_LINK_LLVM=ON \
  -DUSE_SDL=ON -DUSE_SYSTEM_SDL=ON \
  -DUSE_SYSTEM_MVK=ON \
  -DUSE_SYSTEM_OPENCV=ON \
  -DUSE_DISCORD_RPC=ON \
  -DUSE_AUDIOUNIT=ON \
  -DUSE_SYSTEM_FFMPEG=OFF \
  -DUSE_NATIVE_INSTRUCTIONS=OFF \
  -DUSE_PRECOMPILED_HEADERS=OFF

echo "==> building (ninja)"
# The rpcs3 target's macdeployqt POST_BUILD step (rpcs3/CMakeLists.txt) bundles
# Qt frameworks for *distribution* and can fail ad-hoc-signing a Homebrew dylib
# (e.g. libbrotlicommon.1.dylib — macdeployqt rewrites its install names, which
# invalidates the existing signature). The rpcs3 binary links *before* that
# step and resolves Qt from Homebrew at runtime, so a codesign failure is
# non-fatal for a local build. Tolerate it, then ad-hoc re-sign for local run.
build_rc=0
cmake --build "$BUILD_DIR" --parallel || build_rc=$?

APP="$BUILD_DIR/bin/rpcs3.app"
BIN="$APP/Contents/MacOS/rpcs3"
if [ ! -x "$BIN" ]; then
  echo "ERROR: rpcs3 binary not produced (build rc=$build_rc)" >&2
  exit 1
fi

# macdeployqt copies Qt + MoltenVK/abseil/brotli into Contents/Frameworks and
# rewrites their install names, which INVALIDATES those dylibs' brew code
# signatures → dyld SIGKILLs rpcs3 at launch ("Code Signature Invalid", the
# macOS "closed unexpectedly" dialog). Re-sign the whole bundle ad-hoc to fix it.
# Belt-and-suspenders: keep the bundle's own Frameworks dir on the rpath.
if ! otool -l "$BIN" | grep -q "@executable_path/../Frameworks"; then
  install_name_tool -add_rpath @executable_path/../Frameworks "$BIN" || true
fi

# ---- relocate: make the bundle fully self-contained (Phase U.2.4) -----------
# macdeployqt relocates the rpcs3 binary's deps to @executable_path/../Frameworks
# but leaves a few bundled dylibs' OWN install-name id (LC_ID_DYLIB) pointing at
# their Homebrew keg (libbrotlicommon / libgcc_s / libc++abi). Those refs make the
# .app non-relocatable → it won't run on a Mac without Homebrew, and would fail
# notarization + library validation in the signed release. Rewrite every
# /opt/homebrew|/usr/local id + (bundled) dependency across all Mach-O to
# @executable_path/../Frameworks. A general pass (no hardcoded list) so a Qt/dep
# bump can't silently reintroduce a stray ref; warns on a brew dep whose target
# ISN'T bundled (a genuine gap). Validated: `otool -L` reports zero brew refs
# afterward and `rpcs3 --version` still runs.
echo "==> relocating Homebrew install-names → @executable_path/../Frameworks"
while IFS= read -r f; do
  file "$f" 2>/dev/null | grep -q "Mach-O" || continue
  dylib_id="$(otool -D "$f" 2>/dev/null | tail -n +2 | head -1)"
  case "$dylib_id" in
    /opt/homebrew/*|/usr/local/*)
      install_name_tool -id "@executable_path/../Frameworks/$(basename "$dylib_id")" "$f" ;;
  esac
  while IFS= read -r dep; do
    case "$dep" in
      /opt/homebrew/*|/usr/local/*)
        b="$(basename "$dep")"
        if [ -f "$APP/Contents/Frameworks/$b" ]; then
          install_name_tool -change "$dep" "@executable_path/../Frameworks/$b" "$f"
        else
          echo "    WARNING: unbundled Homebrew dep $dep in ${f#"$APP"/}" >&2
        fi ;;
    esac
  done < <(otool -L "$f" 2>/dev/null | tail -n +2 | awk '{print $1}')
done < <(find "$APP" -type f)

echo "==> ad-hoc signing the bundle for local execution (NOT for distribution)"
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true

# Sanity check. NOTE the OpenMP caveat: RPCS3's static llvm@21 libomp + opencv's
# libomp trip "OMP: Error #15 … multiple copies of the OpenMP runtime" → abort.
# KMP_DUPLICATE_LIB_OK=TRUE is OpenMP's own escape hatch (the launcher should set
# it until libomp is de-duped in the bundle — see docs/dev/macos-rpcs3-build.md).
if ! KMP_DUPLICATE_LIB_OK=TRUE "$BIN" --version >/dev/null 2>&1; then
  echo "==> WARNING: 'rpcs3 --version' did not exit cleanly even with" >&2
  echo "    KMP_DUPLICATE_LIB_OK=TRUE — check ~/Library/Logs/DiagnosticReports" >&2
fi

if [ "$build_rc" -ne 0 ]; then
  echo "==> note: build reported rc=$build_rc — almost certainly the macdeployqt"
  echo "    codesign post-step on a brew dylib. The binary is present and ad-hoc"
  echo "    signed; fine for local dev/test. Distribution signing is a TODO."
fi
echo "==> done: $BIN"
