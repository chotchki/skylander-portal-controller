#!/usr/bin/env bash
# Package the macOS release binary into a .app bundle wrapped in a
# .dmg (PLAN 10.9.4). Mac-only — uses hdiutil + plutil + iconutil
# from the stock macOS toolchain, no extra deps.
#
# Inputs (env vars):
#   BIN      path to the built release binary
#            default: target/release/skylander-portal-controller
#   VERSION  version string for the bundle + dmg filename
#            default: $GITHUB_REF_NAME (CI tag) → `git describe` →
#                     "0.0.0-dev" if the repo has no tags
#   OUT_DIR  where to drop the .dmg
#            default: dist/
#
# Output: $OUT_DIR/Skylander-Portal-Controller-$VERSION.dmg
#
# Bundle layout:
#   Skylander Portal Controller.app/
#     Contents/
#       Info.plist
#       MacOS/
#         skylander-portal-controller   (binary)
#         data/                          (figure portraits + box-art —
#                                         data_root resolves to
#                                         <exe_parent>/data per
#                                         wizard.rs::macos_default, so
#                                         the bundle mirrors the zip
#                                         layout instead of using
#                                         Contents/Resources/.)
#       Resources/
#         icon.icns                      (referenced by CFBundleIconFile)
#
# Local usage:
#   cargo build --release -p skylander-server \
#     --no-default-features --features sky-stats,mock-driver-runtime
#   cargo run -p skylander-installer-bake     # bakes assets/branding/icon.icns
#   tools/build-macos-app.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

BIN="${BIN:-target/release/skylander-portal-controller}"
OUT_DIR="${OUT_DIR:-dist}"

# Version resolution: CI tag → git describe → dev placeholder. The
# workspace Cargo.toml carries `version = "0.1.0"` as a permanent
# placeholder (release tags don't bump it), so reading from there
# gives misleading bundle metadata.
if [[ -n "${VERSION:-}" ]]; then
  :
elif [[ -n "${GITHUB_REF_NAME:-}" ]]; then
  VERSION="${GITHUB_REF_NAME#v}"
elif git_desc=$(git describe --tags --always --dirty 2>/dev/null); then
  VERSION="${git_desc#v}"
else
  VERSION="0.0.0-dev"
fi

NAME="Skylander Portal Controller"
SAFE_NAME="${NAME// /-}"
BUNDLE_ID="io.hotchkiss.skylander-portal-controller"

if [[ ! -x "$BIN" ]]; then
  echo "error: binary not found or not executable: $BIN" >&2
  echo "build it first:" >&2
  echo "  cargo build --release -p skylander-server --no-default-features --features sky-stats,mock-driver-runtime" >&2
  exit 1
fi

ICON="assets/branding/icon.icns"
if [[ ! -f "$ICON" ]]; then
  echo "error: icon not found: $ICON" >&2
  echo "bake it first: cargo run -p skylander-installer-bake" >&2
  exit 1
fi

if [[ ! -d "data" ]]; then
  echo "error: data/ not found at repo root" >&2
  exit 1
fi

# Fresh staging tree — incremental rebuilds leak .DS_Store files and
# stale Info.plist values into the .dmg, which then trips Gatekeeper
# in subtle ways.
STAGE="$OUT_DIR/stage"
APP="$STAGE/$NAME.app"
rm -rf "$STAGE"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp "$BIN" "$APP/Contents/MacOS/skylander-portal-controller"
chmod +x "$APP/Contents/MacOS/skylander-portal-controller"

# data/ next to the binary, not under Resources/ — the running
# binary computes data_root from its own exe location. PLAN 10.8.6.
cp -R data "$APP/Contents/MacOS/"

cp "$ICON" "$APP/Contents/Resources/icon.icns"

PLIST="$APP/Contents/Info.plist"
cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>$NAME</string>
  <key>CFBundleExecutable</key>
  <string>skylander-portal-controller</string>
  <key>CFBundleIconFile</key>
  <string>icon</string>
  <key>CFBundleIdentifier</key>
  <string>$BUNDLE_ID</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>$NAME</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$VERSION</string>
  <key>CFBundleVersion</key>
  <string>$VERSION</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
EOF

# plutil -lint exits non-zero on malformed plist. Catches the
# easy-to-make typo of unbalanced <key>/<string> pairs.
plutil -lint "$PLIST" >/dev/null

# Optional codesigning (PLAN 10.9.2). If SIGN_IDENTITY is set, sign
# the binary + bundle with --options runtime (hardened runtime, a
# notarization prerequisite) and --timestamp (Apple's secure
# timestamp authority — also required for notarization). Without
# this env var the script no-ops, producing an unsigned bundle —
# fine for local dev; CI sets it from the imported Developer ID
# cert. The `--deep` flag walks the bundle and signs every embedded
# Mach-O — for our single-binary layout that's just the binary
# itself, but `--deep` is the conventional choice and covers any
# future helper-tool additions.
if [[ -n "${SIGN_IDENTITY:-}" ]]; then
  codesign --force --options runtime --timestamp \
    --sign "$SIGN_IDENTITY" "$APP/Contents/MacOS/skylander-portal-controller"
  codesign --force --deep --options runtime --timestamp \
    --sign "$SIGN_IDENTITY" "$APP"
  codesign --verify --deep --strict --verbose=2 "$APP"
fi

mkdir -p "$OUT_DIR"
DMG="$OUT_DIR/$SAFE_NAME-$VERSION.dmg"
rm -f "$DMG"
# UDZO = read-only zlib-compressed. Worth the extra second to
# halve the artifact size on a ~20 MB data bundle.
hdiutil create \
  -volname "$NAME" \
  -srcfolder "$STAGE" \
  -ov -format UDZO \
  "$DMG" >/dev/null

# Sign the .dmg too — Gatekeeper checks the dmg signature before
# unpacking. Same identity, no --deep (a dmg has nothing nested to
# walk).
if [[ -n "${SIGN_IDENTITY:-}" ]]; then
  codesign --force --sign "$SIGN_IDENTITY" --timestamp "$DMG"
fi

# Strip staging now that the dmg has the contents — leaves
# `dist/` containing only release artifacts.
rm -rf "$STAGE"

ls -lh "$DMG"
echo "built $DMG"
