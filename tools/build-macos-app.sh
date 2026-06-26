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
#       Resources/
#         icon.icns                      (referenced by CFBundleIconFile)
#         data/                          (figure portraits + box-art —
#                                         lives in Resources/ because
#                                         non-Mach-O content under
#                                         Contents/MacOS/ trips
#                                         `codesign --deep` notarization
#                                         walks. wizard.rs::macos_default
#                                         resolves `Contents/Resources/data`
#                                         when run from inside a bundle.)
#
# Local usage:
#   cargo build --release -p skylander-server \
#     --no-default-features --features sky-stats,mock-driver-runtime
#   cargo run -p skylander-brand-bake -- icon # bakes assets/branding/icon.icns
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
  echo "bake it first: cargo run -p skylander-brand-bake -- icon" >&2
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

# data/ under Resources/ (NOT under MacOS/ — codesign --deep treats
# any non-Mach-O file inside Contents/MacOS/ as an unsealed
# subcomponent and refuses to sign the whole bundle, which is what
# tanked the v1.4.x macOS releases). wizard.rs::macos_default knows
# to resolve Contents/Resources/data when running inside a bundle.
cp -R data "$APP/Contents/Resources/"

cp "$ICON" "$APP/Contents/Resources/icon.icns"

# Phase U.3 — nest the bundled patched RPCS3 (a full, self-contained .app with
# its macdeployqt-bundled Qt/MoltenVK frameworks) under Resources/, mirroring the
# Windows bundle's <app>/rpcs3/rpcs3.exe. Resources/ (not MacOS/) for the same
# reason data/ is. config.rs (U.5) resolves
# Contents/Resources/rpcs3/RPCS3.app/Contents/MacOS/rpcs3 as the IPC control
# binary. No-op when RPCS3_APP_SRC is unset (local mock-only builds) so the
# script still runs without a patched-RPCS3 artifact on hand.
if [[ -n "${RPCS3_APP_SRC:-}" ]]; then
  if [[ ! -x "$RPCS3_APP_SRC/Contents/MacOS/rpcs3" ]]; then
    echo "error: RPCS3_APP_SRC has no runnable Contents/MacOS/rpcs3: $RPCS3_APP_SRC" >&2
    exit 1
  fi
  mkdir -p "$APP/Contents/Resources/rpcs3"
  # ditto = Apple's bundle-aware copy: preserves Versions/Current symlinks,
  # xattrs, and the exact tree macdeployqt produced. Normalizes the name to
  # RPCS3.app regardless of the source's case.
  ditto "$RPCS3_APP_SRC" "$APP/Contents/Resources/rpcs3/RPCS3.app"
fi

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

# Notarization-grade signing (PLAN U.4). Apple REJECTS `--deep` for notarized
# apps and requires: hardened runtime (--options runtime) on EVERY executable, a
# secure --timestamp, ONE Developer ID Application identity throughout, and NO
# unsigned nested Mach-O. So we sign INSIDE-OUT by hand — deepest dylibs /
# framework binaries / Qt plugins / helpers first, then the nested rpcs3 main
# Mach-O (with JIT entitlements), then the nested RPCS3.app, then the launcher's
# own binary, then the OUTER launcher .app LAST. (Refs: Apple TN3147 "Migrating
# to the latest notarization tool"; "Signing a Mac Product For Distribution" —
# sign nested code before its container, never rely on --deep, which re-signs
# nested code WITHOUT per-binary entitlements.) Without SIGN_IDENTITY the script
# no-ops, producing an unsigned bundle — fine for local dev. When there is no
# nested RPCS3.app (local mock-only builds), this falls back to launcher-only
# signing.
if [[ -n "${SIGN_IDENTITY:-}" ]]; then
  # Hardened runtime + secure timestamp + force-replace the ad-hoc ('-')
  # signatures macdeployqt / the rpcs3 build left on the bundled dylibs.
  sign() { codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" "$@"; }

  RPCS3_APP="$APP/Contents/Resources/rpcs3/RPCS3.app"
  if [[ -d "$RPCS3_APP" ]]; then
    MAIN_RPCS3="$RPCS3_APP/Contents/MacOS/rpcs3"

    # ditto can carry com.apple.quarantine / com.apple.provenance xattrs in from
    # extraction; strip them recursively so codesign / Gatekeeper don't trip on
    # the nested app.
    xattr -cr "$RPCS3_APP"

    # RPCS3 is an LLVM-JIT emulator: under the hardened runtime it SIGKILLs at
    # first recompile WITHOUT the JIT entitlements. It also dlopen's the Vulkan
    # loader -> MoltenVK, so disable library validation (everything bundled is
    # re-signed with THIS identity below; this only widens to tolerate the
    # dlopen chain + any dylib we failed to enumerate). Mirrors upstream RPCS3's
    # own notarized-build entitlements. Written OUTSIDE $STAGE — $STAGE is the
    # dmg srcfolder, so a file there would leak into the .dmg root.
    ENT_DIR="$(mktemp -d)"
    RPCS3_ENT="$ENT_DIR/rpcs3.entitlements"
    cat > "$RPCS3_ENT" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.cs.allow-jit</key><true/>
  <key>com.apple.security.cs.allow-unsigned-executable-memory</key><true/>
  <key>com.apple.security.cs.disable-executable-page-protection</key><true/>
  <key>com.apple.security.cs.disable-library-validation</key><true/>
</dict>
</plist>
PLIST

    # 1. Sign EVERY nested Mach-O — loose dylibs, framework binaries (incl.
    #    extension-less Versions/A/<Name>), Qt plugins (which macdeployqt drops
    #    under Contents/MacOS/share/qt6/plugins, NOT Frameworks/), and any helper
    #    tools. Enumerated BY FILE TYPE (`file … | grep Mach-O`), NOT by
    #    extension, so extension-less binaries are caught. The walk is rooted at
    #    the always-present RPCS3.app dir, so `find` can't exit 1 on a missing
    #    path under `set -e`. `find -type f` skips the Versions/Current symlinks;
    #    the real Mach-O under Versions/A is a regular file and is caught. The
    #    main rpcs3 binary is skipped here — it's signed WITH entitlements next.
    while IFS= read -r f; do
      [[ "$f" == "$MAIN_RPCS3" ]] && continue
      if file "$f" | grep -q 'Mach-O'; then
        sign "$f"
      fi
    done < <(find "$RPCS3_APP" -type f)

    # 2. rpcs3's own main Mach-O — WITH the JIT entitlements.
    sign --entitlements "$RPCS3_ENT" "$MAIN_RPCS3"

    # 3. Seal the nested RPCS3.app — WITH the JIT entitlements (re-signs its main
    #    binary with them; its CodeResources now seals every item signed in 1-2).
    sign --entitlements "$RPCS3_ENT" "$RPCS3_APP"

    rm -rf "$ENT_DIR"
  fi

  # 4. Launcher's own inner Mach-O (egui/eframe — no JIT, default entitlements).
  #    Runs in both the nested-RPCS3 and local mock-only cases.
  sign "$APP/Contents/MacOS/skylander-portal-controller"

  # 5. Seal the OUTER launcher .app LAST — NO --deep. Every nested item is
  #    already individually signed above; --deep would re-sign them, dropping
  #    the per-binary entitlements (and is what Apple tells you not to do).
  sign "$APP"

  # Verify the whole tree the way Gatekeeper will (verify MAY use --deep).
  codesign --verify --deep --strict --verbose=2 "$APP"

  # TODO(U.4.3): staple the launcher .app itself (not just the .dmg below) so a
  # user who drags it out of the .dmg gets an offline-verifiable ticket on first
  # launch. Deferred — needs reordering to notarize the .app BEFORE building the
  # .dmg (notarize app → `xcrun stapler staple` app → hdiutil create dmg from
  # the stapled app), which the release.yml flow doesn't do yet.
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
