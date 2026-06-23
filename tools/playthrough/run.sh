#!/usr/bin/env bash
# Build, (macOS) codesign with a stable identity, and run the play-through recorder.
#
# Why the codesign step: an unsigned / ad-hoc binary's Designated Requirement is just
# its cdhash, which changes on every `cargo build` — so macOS TCC treats each build as
# a new app and re-prompts for Screen Recording. Signing with an Apple *Development*
# identity + a fixed --identifier gives an identifier-based DR that survives rebuilds,
# so you grant Screen Recording once and it sticks. (The recorder is a dev/CI-only
# tool, so a Development cert — not Developer ID / notarization — is the right tier.)
#
# Note: macOS Sequoia/Tahoe also re-prompts ~monthly for ANY granted app ("Allow For
# One Month"); that fires on the terminal regardless of signing and is unrelated to
# this — just click allow. Signing only fixes the per-rebuild thrash.
#
# Usage:  tools/playthrough/run.sh <mode> [args...]
#   tools/playthrough/run.sh narrative hero
#   tools/playthrough/run.sh render dev-data/raw.mp4
# Env:
#   SKYLANDER_CODESIGN_IDENTITY  override the auto-detected signing identity
#   SKYLANDER_RUN_PROFILE        cargo profile (default: dev → target/debug)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

PROFILE="${SKYLANDER_RUN_PROFILE:-dev}"
if [[ "$PROFILE" == "dev" ]]; then
    cargo build -p skylander-playthrough
    BIN="target/debug/skylander-playthrough"
else
    cargo build -p skylander-playthrough --profile "$PROFILE"
    BIN="target/$PROFILE/skylander-playthrough"
fi

if [[ "$(uname -s)" == "Darwin" ]]; then
    ID="${SKYLANDER_CODESIGN_IDENTITY:-$(security find-identity -v -p codesigning \
        | awk -F'"' '/Apple Development/{print $2; exit}')}"
    if [[ -n "$ID" ]]; then
        codesign --force --sign "$ID" \
            --identifier io.hotchkiss.skylander.playthrough "$BIN"
        echo "run.sh: signed $BIN (identifier io.hotchkiss.skylander.playthrough)" >&2
    else
        echo "run.sh: no Apple Development codesigning identity found — running" >&2
        echo "        unsigned; macOS may re-prompt for Screen Recording per rebuild." >&2
    fi
fi

exec "$BIN" "$@"
