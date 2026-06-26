#!/usr/bin/env bash
# Apply the RPCS3 patch series (P1 IPC portal control, P2 window lifecycle,
# P3 offline-connect errno, P4 guest portal-event pushes, P5 save-state portal
# re-attach, P6 gamepad BUTTON_PRESS injection, P7 game-window tiling WINDOW_SET,
# P8 macOS CAMetalLayer CAContext publish for cross-process CALayerHost (SURFACE))
# onto a checkout of the pinned
# upstream RPCS3 — the vendored submodule by default.
#
#   rpcs3-patches/apply.sh [path-to-rpcs3-checkout]   # default: vendor/rpcs3
#
# The checkout must sit at the pinned commit (see rpcs3-patches/README.md). Patches
# are applied in filename order via `git am --3way`, leaving one commit per patch on
# top of the pin — i.e. reproducing the dev clone's `spike-patches` branch. Used both
# by the CI lane (.github/workflows/rpcs3-patched.yml) and for local patched builds.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
target="${1:-$here/../vendor/rpcs3}"

if [[ ! -e "$target/.git" ]]; then
  echo "error: '$target' is not a git checkout." >&2
  echo "       Init the submodule first:  git submodule update --init vendor/rpcs3" >&2
  exit 1
fi

cd "$target"
# `git am` needs an author identity; set a throwaway one only if the env has none.
git config user.email >/dev/null 2>&1 || git config user.email "patch-bot@skylander-portal-controller.local"
git config user.name  >/dev/null 2>&1 || git config user.name  "skylander patch-bot"

base="$(git rev-parse --short HEAD)"

# Some RPCS3 sources are stored CRLF (e.g. swapchain_macos.hpp); our format-patch'd
# series is LF, so `git am`'s text match fails on a clean checkout. Normalize the
# EXISTING CRLF files the series touches to LF, committed BELOW the patch commits
# (LF compiles identically). The CI's expected-files check diffs HEAD~<#patches>, so
# this base commit stays out of the patch diff. Auto-detected from the patches'
# `+++ b/<file>` targets — no hardcoded list, so future patches are covered too.
norm=0
for f in $(grep -h '^+++ b/' "$here"/0*.patch | sed 's#^+++ b/##' | sort -u); do
  if [ -f "$f" ] && LC_ALL=C grep -qI $'\r' "$f"; then
    perl -i -pe 's/\r\n/\n/g' "$f"; norm=1
  fi
done
[ "$norm" = 1 ] && git commit -aqm "normalize CRLF->LF on series-touched files (pre-am base)" \
  && echo "Normalized CRLF->LF on series-touched files (base commit below the series)."

count="$(ls "$here"/0*.patch | wc -l | tr -d ' ')"
echo "Applying $count RPCS3 patch(es) onto $base in $target ..."
git am --3way "$here"/0*.patch
echo "OK — $count patch(es) applied; HEAD now $(git rev-parse --short HEAD) (was $base)"
