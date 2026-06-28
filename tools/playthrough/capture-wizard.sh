#!/usr/bin/env bash
# Capture the first-launch egui wizard for the Tour's `install` opener (A.8.11).
#
# The wizard is RELEASE-build-only (`#[cfg(not(dev-tools))]`) and egui can't be
# driven by WebDriver, so this is a PASSIVE showcase (chotchki: "showcase is
# fine, it's more to stress you have setup work to do before you use this"):
#   1. build the release server (no dev-tools → the wizard code path exists),
#   2. run it with `--reconfigure` so the wizard pops over an existing config —
#      NON-destructively, because we kill it before the Finish page writes
#      config.json,
#   3. capture the wizard window for a few seconds via the recorder's SCKit
#      single-window path (`capture-window`),
#   4. kill the wizard.
#
# Output: a standalone wizard MP4 the render step captions + the Tour concats in
# front of the walkthrough body (see `render-concat`).
#
# Usage:  tools/playthrough/capture-wizard.sh [out.mp4] [seconds]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

OUT="${1:-$ROOT/tools/playthrough/out/playthrough-install.mp4}"
SECS="${2:-6}"
APP="${SKYLANDER_GAME_WINDOW_APP:-skylander-portal-controller}"
TITLE="Skylander Portal Controller — Setup"
SRV="$ROOT/target/debug/skylander-portal-controller"

mkdir -p "$(dirname "$OUT")"

# 1. Build the RELEASE server (no dev-tools → `config::load` runs the wizard;
#    sky-stats + mock-driver-runtime match the shipped macOS feature set).
echo "capture-wizard: building release server (no dev-tools)…" >&2
cargo build -p skylander-server --no-default-features --features sky-stats,mock-driver-runtime

# 2. Sign so macOS doesn't re-prompt (the CAPTURING process — the recorder — is
#    what needs Screen Recording; signing the server is harmless belt-and-braces
#    so its window identity is stable across rebuilds).
if [[ "$(uname -s)" == "Darwin" ]]; then
  ID="${SKYLANDER_CODESIGN_IDENTITY:-$(security find-identity -v -p codesigning \
      | awk -F'"' '/Apple Development/{print $2; exit}')}"
  if [[ -n "${ID:-}" ]]; then
    codesign --force --sign "$ID" --identifier io.hotchkiss.skylander.setup "$SRV" || true
  fi
fi

# 3. Launch the wizard (blocking in config::load, before the server binds).
#    `--reconfigure` forces it even if config.json already exists.
echo "capture-wizard: launching the wizard (--reconfigure)…" >&2
caffeinate -d "$SRV" --reconfigure >/tmp/skylander-wizard.log 2>&1 &
SRV_PID=$!
trap 'kill "$SRV_PID" 2>/dev/null || true' EXIT
sleep 4   # let the eframe wizard window mount + spin-in settle

# 4. Capture the wizard window (run.sh builds + signs the recorder, then execs
#    `capture-window <app> <title> <secs> <out>`).
tools/playthrough/run.sh capture-window "$APP" "$TITLE" "$SECS" "$OUT"

# 5. Kill the wizard — config.json is untouched (we never reached Finish).
kill "$SRV_PID" 2>/dev/null || true
echo "capture-wizard: wrote $OUT" >&2
