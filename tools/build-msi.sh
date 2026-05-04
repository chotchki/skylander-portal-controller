#!/usr/bin/env bash
# Local Windows MSI build for HTPC iteration.
#
# Replicates the Windows lane in `.github/workflows/release.yml` so we
# can produce an installable `.msi` in ~60s without burning a CI cycle
# (~14min) for every UI bug. PLAN 10.8.4 + sequel work — most of those
# bugs were "watch the launcher animate / cover RPCS3" symptoms that
# can't be reproduced in unit tests, so a fast local build/install
# cycle is the actual feedback loop.
#
# Usage:
#   tools/build-msi.sh [version] [--install]
#
# Defaults:
#   - version = "99.99.0" (always-newer than any v1.X.Y from CI;
#     MSI MajorUpgrade triggers a clean uninstall+install when applied
#     over a prior install).
#   - no --install: just builds, prints the .msi path.
#   - --install: also runs `msiexec /i <path>` elevated via PowerShell
#     Start-Process -Verb RunAs (UAC prompt fires once).
#
# Prereqs (one-time, already done on the HTPC):
#   - WiX Toolset v3 at C:\Program Files (x86)\WiX Toolset v3.14
#   - cargo install cargo-wix --locked
#   - trunk available on PATH (for phone bundle)
#
# Why a separate script vs reusing release.yml directly: release.yml
# embeds the steps inline in YAML with PowerShell here-strings, GitHub
# Actions env-var plumbing, and tag-only triggers. Extracting to a
# script we can run locally is straight Bash — easier to maintain.

set -euo pipefail

# Locate workspace root regardless of where the script was invoked from.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$WS_ROOT"

VERSION="99.99.0"
DO_INSTALL=0
for arg in "$@"; do
    case "$arg" in
        --install) DO_INSTALL=1 ;;
        --help|-h)
            head -n 30 "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            VERSION="$arg"
            ;;
    esac
done

# WiX 3 is Windows-only; this script is bash-on-Windows (gitbash).
WIX_BIN="/c/Program Files (x86)/WiX Toolset v3.14/bin"
if [[ ! -d "$WIX_BIN" ]]; then
    echo "ERROR: WiX 3 not found at $WIX_BIN" >&2
    echo "Install via: winget install WiXToolset.WiXToolset" >&2
    exit 1
fi
export PATH="$WIX_BIN:$PATH"

if ! command -v cargo-wix >/dev/null 2>&1 && ! cargo wix --version >/dev/null 2>&1; then
    echo "ERROR: cargo-wix not installed" >&2
    echo "Install via: cargo install cargo-wix --locked" >&2
    exit 1
fi

DATA_ABS="$(cygpath -w "$WS_ROOT/data")"

echo "==> [1/4] Phone bundle (trunk build --release)"
(cd phone && trunk build --release >/dev/null)

echo "==> [2/4] Server (cargo build --release, production features)"
cargo build --release -p skylander-server \
    --no-default-features --features sky-stats >/dev/null

echo "==> [3/4] heat harvest data/ → wix/data.wxs"
mkdir -p wix
heat.exe dir data -ag -srd -dr DataDir -cg DataFiles -sfrag -suid \
    -var var.DataSourceDir -out wix/data.wxs >/dev/null

if [[ ! -f wix/steam.wxs ]]; then
    # PLAN 10.8.5 isn't shipped yet — empty fragment so cargo-wix's
    # include list resolves. Replace with a real heat harvest once
    # steam/ artwork lands.
    cat > wix/steam.wxs <<'EOF'
<?xml version='1.0' encoding='windows-1252'?>
<Wix xmlns='http://schemas.microsoft.com/wix/2006/wi'>
  <Fragment>
    <ComponentGroup Id='SteamFiles'/>
  </Fragment>
</Wix>
EOF
fi

echo "==> [4/4] cargo wix → MSI"
# cargo-wix v0.3.9 resolves [package.metadata.wix] include paths
# relative to its CWD, not the manifest dir. Run from crates/server
# so `../../wix/main.wxs` resolves to the workspace root's wix dir.
(cd crates/server && cargo wix -p skylander-server \
    --no-build --nocapture \
    --install-version "$VERSION" \
    -C -ext -C WixFirewallExtension \
    -C "-dDataSourceDir=$DATA_ABS" \
    -L -ext -L WixFirewallExtension >/dev/null)

MSI_PATH="$WS_ROOT/target/wix/skylander-server-${VERSION}-x86_64.msi"
if [[ ! -f "$MSI_PATH" ]]; then
    echo "ERROR: MSI not produced at $MSI_PATH" >&2
    exit 1
fi
MSI_SIZE=$(du -h "$MSI_PATH" | cut -f1)
echo
echo "✓ MSI built: $MSI_PATH ($MSI_SIZE)"

if [[ $DO_INSTALL -eq 1 ]]; then
    echo
    echo "==> Installing (UAC prompt incoming)"
    MSI_WIN="$(cygpath -w "$MSI_PATH")"
    LOG_WIN="$(cygpath -w "$WS_ROOT/target/wix/install.log")"
    powershell -NoProfile -Command "Start-Process msiexec.exe -ArgumentList '/i','\"$MSI_WIN\"','/qb','/l*v','\"$LOG_WIN\"' -Verb RunAs -Wait"
    echo "✓ Install complete (log: target/wix/install.log)"
fi
