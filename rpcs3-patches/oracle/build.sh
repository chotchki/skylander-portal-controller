#!/usr/bin/env bash
# Build the oracle (openspec AF.4) on the Mac: RPCS3 at the product's pin, the
# product's P1-P8, then the O-series, built by .ci-local/build-mac.sh in a checkout
# OUTSIDE this repo, and installed where GUAC_ORACLE_EXE points. Dev-only: nothing
# here reaches a product build, CI lane or release.
#
#   GUAC_ORACLE_EXE=<dir>/RPCS3.app/Contents/MacOS/rpcs3 \
#     rpcs3-patches/oracle/build.sh [--no-brew] <checkout>
#
# <checkout> is cloned when missing (from the submodule's objects when vendor/rpcs3 is
# initialized, else from upstream) and put on the pin, which is read from the
# vendor/rpcs3 gitlink. At the pin it gets both series (rpcs3-patches/apply.sh, then
# oracle/apply.sh); one that already carries them (`oracle/apply.sh --check`) builds
# as it stands; anything else, or a dirty tree, is refused rather than reset.
# --no-brew passes BUILD_MAC_NO_BREW=1 through (see build-mac.sh).
#
# The identity goes to stdout and to oracle-build.txt beside the installed .app:
# upstream base, the repo commit the series came from, the source tree, every patch's
# SHA-256, the binary's SHA-256, and the MoltenVK, LLVM and Qt it was built against.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
upstream="https://github.com/RPCS3/rpcs3.git"
usage="usage: GUAC_ORACLE_EXE=<dir>/RPCS3.app/Contents/MacOS/rpcs3 rpcs3-patches/oracle/build.sh [--no-brew] <checkout>"

no_brew=0
if [[ "${1:-}" == "--no-brew" ]]; then
  no_brew=1
  shift
fi
co="${1:?$usage}"

# ---- where the .app goes, checked before an hour of compiling ----------------
exe="${GUAC_ORACLE_EXE:?$usage}"
app="${exe%/Contents/MacOS/rpcs3}"
if [[ "$app" == "$exe" || "$app" != *.app ]]; then
  echo "error: GUAC_ORACLE_EXE must name <bundle>.app/Contents/MacOS/rpcs3 (got $exe)" >&2
  exit 1
fi
out_dir="$(dirname "$app")"
mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd -P)"
app="$out_dir/$(basename "$app")"
# No oracle inside a work tree: not this repo's, and not the RPCS3 checkout's either.
if git -C "$out_dir" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "error: $out_dir is inside a git work tree; put the oracle outside every repo" >&2
  exit 1
fi
if [[ -e "$app" && ! -e "$app/Contents/MacOS/rpcs3" ]]; then
  echo "error: $app exists and isn't an RPCS3 bundle; refusing to replace it" >&2
  exit 1
fi

# ---- the checkout ------------------------------------------------------------
pin="$(git -C "$repo" ls-tree HEAD vendor/rpcs3 | awk '{print $3}')"
[[ -n "$pin" ]] || { echo "error: no vendor/rpcs3 gitlink at $repo HEAD" >&2; exit 1; }

fresh=0
if [[ ! -e "$co/.git" ]]; then
  fresh=1
  if [[ -e "$repo/vendor/rpcs3/.git" ]]; then
    src="$(git -C "$repo/vendor/rpcs3" rev-parse --absolute-git-dir)"
  else
    src="$upstream"
  fi
  echo "==> cloning $src into $co"
  git clone --no-checkout "$src" "$co"
  git -C "$co" remote set-url origin "$upstream"
fi
co="$(cd "$co" && pwd -P)"
if [[ "$co" == "$repo"/* ]]; then
  echo "error: the checkout sits inside this repo ($co); the oracle builds outside it" >&2
  exit 1
fi
git -C "$co" cat-file -e "$pin^{commit}" 2>/dev/null || git -C "$co" fetch origin "$pin"
if (( fresh )); then
  git -C "$co" checkout -q --detach "$pin"
fi

if ! git -C "$co" diff --quiet --ignore-submodules=all HEAD; then
  echo "error: $co has uncommitted changes; the oracle builds only what the series makes" >&2
  exit 1
fi
if [[ "$(git -C "$co" rev-parse HEAD)" == "$pin" ]]; then
  bash "$repo/rpcs3-patches/apply.sh" "$co"
  bash "$here/apply.sh" "$co"
elif ! bash "$here/apply.sh" --check "$co"; then
  echo "error: $co HEAD is neither the pin ($pin) nor the pin plus both series." >&2
  echo "       Reset it yourself: git -C '$co' checkout --detach $pin" >&2
  exit 1
fi
git -C "$co" merge-base --is-ancestor "$pin" HEAD || {
  echo "error: the series in $co doesn't sit on the pin $pin" >&2
  exit 1
}

# ---- build -------------------------------------------------------------------
brew_prefix="$(brew --prefix)"
# find_package takes whichever OpenCV is linked (5.x here since 2026-10-17), not the
# opencv@4 build-mac.sh installs, so pin the 4.x keg. ccache sits beside the checkout.
export OpenCV_DIR="${OpenCV_DIR:-$brew_prefix/opt/opencv@4/lib/cmake/opencv4}"
export CCACHE_DIR="${CCACHE_DIR:-$(dirname "$co")/ccache}"
if (( no_brew )); then
  export BUILD_MAC_NO_BREW=1
fi

load_before="$(sysctl -n vm.loadavg)"
start="$(date +%s)"
bash "$repo/.ci-local/build-mac.sh" "$co"
seconds=$(( $(date +%s) - start ))
load_after="$(sysctl -n vm.loadavg)"

built="$co/build/bin/rpcs3.app"
[[ -x "$built/Contents/MacOS/rpcs3" ]] || { echo "error: no binary at $built" >&2; exit 1; }

# ---- install + identity ------------------------------------------------------
# A run (another lane's, a live test's) may be using the bundle: replacing it under a running
# process swaps its frameworks out from under it. The built .app stays where it is for a rerun.
if running="$(pgrep -f -- "$app/Contents/MacOS/rpcs3")"; then
  echo "error: rpcs3 from $app is running (pid $(echo $running)); stop it, then rerun to install" >&2
  exit 1
fi
rm -rf "$app"
ditto "$built" "$app"
sha="$(shasum -a 256 "$app/Contents/MacOS/rpcs3" | awk '{print $1}')"
[[ "$sha" == "$(shasum -a 256 "$built/Contents/MacOS/rpcs3" | awk '{print $1}')" ]] || {
  echo "error: the installed binary doesn't match the built one" >&2
  exit 1
}

keg() { basename "$(readlink "$brew_prefix/opt/$1")"; }
series_commit="$(git -C "$repo" log -1 --format=%H -- rpcs3-patches)"
dirty=""
git -C "$repo" diff --quiet HEAD -- rpcs3-patches || dirty=" (rpcs3-patches/ modified since)"
identity="$out_dir/oracle-build.txt"
{
  echo "oracle build (openspec AF.4), $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "binary        $app/Contents/MacOS/rpcs3"
  echo "sha256        $sha"
  echo "upstream base $pin"
  echo "series commit $series_commit$dirty"
  echo "checkout HEAD $(git -C "$co" rev-parse HEAD) (tree $(git -C "$co" rev-parse 'HEAD^{tree}'))"
  echo "MoltenVK      $(keg molten-vk) (Homebrew's, loaded through $brew_prefix/etc/vulkan/icd.d; not bundled)"
  echo "LLVM          $(keg "llvm@21") (static)"
  echo "Qt            $(keg qtbase)"
  echo "OpenCV        $OpenCV_DIR"
  echo "build         ${seconds} s; load average $load_before -> $load_after"
  echo "patches:"
  for p in "$repo"/rpcs3-patches/0*.patch "$here"/0*.patch; do
    echo "  $(shasum -a 256 "$p" | awk '{print $1}')  ${p#"$repo"/}"
  done
} | tee "$identity"
echo "==> identity written to $identity"
