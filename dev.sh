#!/usr/bin/env bash
# One-shot dev bringup, so the served phone SPA never drifts from source.
#
# THE FOOTGUN: `cargo run -p skylander-server` serves the PRE-BUILT phone/dist
# via tower_http ServeDir. Nothing rebuilds that bundle when phone/src (or
# phone/styles) changes — so a bare `cargo run` silently serves a STALE phone
# app, and you debug against code that isn't running. This wrapper rebuilds the
# phone WASM bundle first (matching CI's `trunk build --release`), then launches
# the server. Pair with build.sh in the rpcs3 clone for the C++ side.
#
# Usage:  bash dev.sh          # rebuild phone + launch server
#         bash dev.sh --skip-phone   # launch server only (phone already fresh)
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

# THE CACHE-BUSTER COUPLING — two requirements that BOTH must hold:
#   (1) the token MUST change whenever source changes, or the phone's PWA /
#       service-worker / browser cache keeps serving the STALE wasm bundle (the
#       version check is exactly what trips a forced reload); AND
#   (2) the server and the phone must bake the SAME token, or the phone's
#       /api/version check raises a (false) "out of date" overlay.
# build.rs's compute_token() (git short-hash + `-dirty`) satisfies (2) per commit
# but FAILS (1) for uncommitted work: `-dirty` is a binary flag (one string for
# any dirty tree) and the scripts only re-bake on .git/HEAD|index moves, so
# editing a tracked source file does NOT change the baked token -> stale cache.
# A FIXED token would be even worse — it never busts the cache at all.
# So: derive a CONTENT fingerprint of exactly what's about to compile (HEAD +
# the porcelain status + the full working-tree diff), and hand the SAME value to
# both builds. It changes on any tracked edit / add / remove / commit (busting
# the cache), is stable when nothing changed (fast restarts — rerun-if-env-changed
# then skips the rebake), and is identical across the two halves. Override by
# exporting BUILD_TOKEN yourself (e.g. to mirror a real release token).
# Caveat: content edits to an UNTRACKED file only register once it's `git add`ed
# (status shows its name, not its bytes). Add new source files and you're golden.
if [[ -z "${BUILD_TOKEN:-}" ]]; then
  fp="$( { git rev-parse HEAD; git status --porcelain=v1; git diff HEAD; } 2>/dev/null | sha1sum | cut -c1-12 )"
  export BUILD_TOKEN="dev-${fp:-local}"
fi
echo "[dev] BUILD_TOKEN=$BUILD_TOKEN (content-derived; shared by server + phone; changes when source does)"

if [[ "${1:-}" != "--skip-phone" ]]; then
  echo "[dev] rebuilding phone bundle (trunk build --release)…"
  ( cd phone && BUILD_TOKEN="$BUILD_TOKEN" trunk build --release )
else
  echo "[dev] --skip-phone: serving existing phone/dist as-is (token must already be $BUILD_TOKEN)"
fi

echo "[dev] launching server (reads .env.dev; state -> ./dev-data, logs -> ./logs)…"
exec cargo run -p skylander-server
