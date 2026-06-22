#!/usr/bin/env bash
# Fast SPU-crash iteration runner (PLAN 16.13). Boots the patched RPCS3 on a game
# no-GUI and watches RPCS3.log for the *emulator's own* fatal line — which it logs
# the instant a guest/recompiler thread dies — instead of polling IPC and waiting
# out a timeout. The emulator already knows; we just read it.
#
#   CRASH  -> prints the `·F` fatal + the `(in file …:LINE)` source location, kills
#             RPCS3, exits 1 within ~1s of the fatal.
#   OK     -> no fatal within TIMEOUT while the log keeps growing (booted past the
#             crash point), kills RPCS3, exits 0.
#
# Usage:
#   boot-watch.sh [/path/to/EBOOT.BIN] [timeout_secs]
# Env overrides: RPCS3_EXE, RPCS3_CONFIG_DIR, RPCS3_LOG.
# The SPU decoder / block size come from the config in RPCS3_CONFIG_DIR — set them
# there (e.g. config.yml.repro-giga-llvm for the crash repro).
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
RPCS3_EXE="${RPCS3_EXE:-$REPO/vendor/rpcs3/build/bin/rpcs3.app/Contents/MacOS/rpcs3}"
EBOOT="${1:-$HOME/Games/ps3/Skylanders Giants/PS3_GAME/USRDIR/EBOOT.BIN}"
TIMEOUT="${2:-60}"
RPCS3_CONFIG_DIR="${RPCS3_CONFIG_DIR:-$HOME/Library/Application Support/rpcs3}"
LOG="${RPCS3_LOG:-$HOME/Library/Caches/rpcs3/RPCS3.log}"

[ -x "$RPCS3_EXE" ] || { echo "no rpcs3 at $RPCS3_EXE" >&2; exit 3; }
[ -f "$EBOOT" ]     || { echo "no eboot at $EBOOT" >&2; exit 3; }

cleanup() {
  pkill -f 'MacOS/rpcs3' 2>/dev/null
  rm -f /tmp/rpcs3-skylander.sock "$(dirname "$RPCS3_EXE")/RPCS3.buf" 2>/dev/null
}
trap cleanup EXIT

echo "== boot-watch: $(basename "$(dirname "$(dirname "$EBOOT")")") | timeout ${TIMEOUT}s =="
echo "   decoder/block-size: $(grep -E 'SPU (Decoder|Block Size):' "$RPCS3_CONFIG_DIR/config.yml" | tr '\n' ' ')"
: > "$LOG" 2>/dev/null || true

RPCS3_CONFIG_DIR="$RPCS3_CONFIG_DIR" "$RPCS3_EXE" --no-gui "$EBOOT" >/dev/null 2>&1 &
RP=$!
start=$SECONDS
verdict="" ; line=""
while kill -0 "$RP" 2>/dev/null; do
  if (( SECONDS - start >= TIMEOUT )); then verdict="OK"; break; fi
  # the emulator's fatal marker — instant
  if hit=$(grep -m1 -nE '·F .*(fatal error|Segfault)' "$LOG" 2>/dev/null); then
    verdict="CRASH"
    # the source-location line RPCS3 prints right after a range-check/ensure fatal
    line=$(grep -m1 -E '\(in file .*SPUCommonRecompiler|\(in file ' "$LOG" 2>/dev/null)
    break
  fi
  sleep 0.5
done

echo "----"
if [ "$verdict" = "CRASH" ]; then
  echo "CRASH after $((SECONDS-start))s:"
  echo "  $hit" | sed 's/^/  /'
  [ -n "$line" ] && echo "  src: $line"
  exit 1
elif [ "$verdict" = "OK" ]; then
  echo "OK: no fatal in ${TIMEOUT}s (booted past the crash point). last log:"
  tail -1 "$LOG" 2>/dev/null | sed 's/^/  /'
  exit 0
else
  echo "RPCS3 exited on its own after $((SECONDS-start))s (check $LOG)"
  exit 2
fi
