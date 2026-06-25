#!/usr/bin/env bash
# Kick off the in-game capture flow for the macOS **P8 surface-embed** live test
# (game composited INSIDE the launcher window via CALayerHost). This wraps
# `run.sh narrative ingame` with the env the embed path needs, so the exact
# invocation survives across sessions / context compaction.
#
# Why this exists / what it validates:
#   * The `ingame` narrative is ServerFlavor::IpcCold (A.2.4) — a REAL signed
#     /api/launch cold-boots Giants on the patched RPCS3 over IPC, no save state.
#   * With P8, RPCS3 publishes its render CAMetalLayer over IPC (SURFACE → context
#     + native size) and the launcher hosts it via CALayerHost, scaled to its pane
#     (compositor.rs::set_frame). The RPCS3 window is HIDDEN — there is no second
#     top-level game window.
#   * Because the game lives inside the launcher window now, the classifier
#     (screen.rs::grab_frame → capture.rs::find_window) must target the LAUNCHER
#     window, not RPCS3's (which is hidden, so is_on_screen()==false → not found).
#     That's the SKYLANDER_GAME_WINDOW_{APP,TITLE} override below — the "2 streams
#     not 3" design: the controller window feeds BOTH the video capture AND the
#     classifier; RPCS3 stays hidden.
#
# The launcher binary is `skylander-portal-controller` ([[bin]] name in
# crates/server/Cargo.toml), which is also the SCKit application_name; the egui
# window title is "Skylander Portal Controller" (main.rs ViewportBuilder).
#
# Prereqs (this script does NOT do them):
#   * Patched RPCS3 built:  .ci-local/build-mac.sh   (or an incremental
#     `cmake --build vendor/rpcs3/build --parallel` + ad-hoc re-sign).
#   * Phone SPA built with the e2e token:
#       (cd phone && BUILD_TOKEN=e2e-test trunk build)
#   * Screen Recording permission granted to the terminal (run.sh signs the
#     recorder so the grant sticks across rebuilds).
#
# Usage:
#   tools/playthrough/capture-ingame-embed.sh            # narrative ingame
#   tools/playthrough/capture-ingame-embed.sh beat pick_game   # a single beat
#   tools/playthrough/capture-ingame-embed.sh narrative ingame # explicit
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

# --- 1. Pull RPCS3_EXE / RPCS3_CONFIG_DIR / FIRMWARE_PACK_ROOT from .env.dev ---
# The recorder's IpcCold TestServer reads these from the PROCESS env
# (crates/e2e-tests spawn_ipc_inner). We parse .env.dev ourselves rather than
# `source` it, because dotenv values are unquoted and may contain spaces
# (FIRMWARE_PACK_ROOT, RPCS3_CONFIG_DIR) — bash `source` would choke on those;
# the Rust dotenv parser (and this loop) take the rest-of-line as the value.
ENV_FILE="$ROOT/.env.dev"
if [[ -f "$ENV_FILE" ]]; then
  while IFS= read -r line || [[ -n "$line" ]]; do
    case "$line" in
      ''|\#*) continue ;;   # blank / comment
      *=*)    : ;;          # has a key=value
      *)      continue ;;
    esac
    key="${line%%=*}"
    val="${line#*=}"
    # Only export if not already set in the environment (caller can override).
    if [[ -z "${!key:-}" ]]; then
      export "$key=$val"
    fi
  done < "$ENV_FILE"
else
  echo "capture-ingame-embed.sh: no .env.dev at $ENV_FILE — set RPCS3_EXE +" >&2
  echo "  RPCS3_CONFIG_DIR + FIRMWARE_PACK_ROOT in the environment yourself." >&2
fi

# Default RPCS3_EXE to the just-built patched binary if .env.dev didn't set it.
: "${RPCS3_EXE:=$ROOT/vendor/rpcs3/build/bin/rpcs3.app/Contents/MacOS/rpcs3}"
export RPCS3_EXE

# --- 2. chromedriver (fantoccini drives the phone SPA in a visible Chrome) -----
if [[ -z "${CHROMEDRIVER:-}" ]]; then
  if command -v chromedriver >/dev/null 2>&1; then
    CHROMEDRIVER="$(command -v chromedriver)"
  elif [[ -x /opt/homebrew/bin/chromedriver ]]; then
    CHROMEDRIVER=/opt/homebrew/bin/chromedriver
  fi
fi
export CHROMEDRIVER
[[ -n "${CHROMEDRIVER:-}" ]] || { echo "capture-ingame-embed.sh: no chromedriver found (brew install --cask chromedriver)"; exit 1; }

# --- 3. Point the classifier at the LAUNCHER window (the embedded game) --------
# 2-stream design: RPCS3's own window is hidden under SKYLANDER_BORDERLESS, so
# the classifier reads the game out of the controller window. Caller can override.
export SKYLANDER_GAME_WINDOW_APP="${SKYLANDER_GAME_WINDOW_APP:-skylander-portal-controller}"
export SKYLANDER_GAME_WINDOW_TITLE="${SKYLANDER_GAME_WINDOW_TITLE:-Skylander Portal Controller}"

# --- 4. Run --------------------------------------------------------------------
MODE=("${@:-narrative ingame}")
# If no args, default to `narrative ingame`; otherwise pass args through.
if [[ "$#" -eq 0 ]]; then
  set -- narrative ingame
fi

echo "capture-ingame-embed.sh: launching capture" >&2
echo "  RPCS3_EXE                 = $RPCS3_EXE" >&2
echo "  RPCS3_CONFIG_DIR          = ${RPCS3_CONFIG_DIR:-(unset!)}" >&2
echo "  FIRMWARE_PACK_ROOT        = ${FIRMWARE_PACK_ROOT:-(unset — recorder will resolve)}" >&2
echo "  CHROMEDRIVER              = $CHROMEDRIVER" >&2
echo "  SKYLANDER_GAME_WINDOW_APP = $SKYLANDER_GAME_WINDOW_APP" >&2
echo "  SKYLANDER_GAME_WINDOW_TITLE = $SKYLANDER_GAME_WINDOW_TITLE" >&2
echo "  mode                      = $*" >&2

exec tools/playthrough/run.sh "$@"
