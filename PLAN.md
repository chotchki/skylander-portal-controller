# Skylander Portal Controller — Execution Plan

Active work toward MVP. Closed work lives in [PLAN_ARCHIVE.md](PLAN_ARCHIVE.md) — Phases 0–13, 15–20, A.1–A.5, B (most done in code; Phase 12 struck as a non-goal). Swept 2026-06-24 after a full plan triage.

Conventions:
- `[ ]` pending, `[x]` done, `[~]` in progress, `[?]` blocked / needs discussion.
- New tasks should always be numbered and have a checkbox so they're traceable.
- Don't skip a review checkpoint; the point is to re-plan with new information.

## Non-goals
- No bundling of RPCS3 or `.sky` files (piracy concern).
- No Linux support — production targets are Windows + macOS.
- No user-entered figure names.
- No audio (text-only Kaos to dodge copyright).
- No live wiki scraping at runtime — data is committed to the repo.
- **No AXUIElement macOS driver** (ex-Phase 12) — IPC to the patched RPCS3 supersedes it.

---

## Phase 21 - ios-inspect de-vendor (external tool landed at github.com/chotchki/ios-inspect)
- [ ] 21.1 - Confirm external ios-inspect exposes the library API e2e imports
- [ ] 21.2 - Repoint e2e-tests dep: ios-inspect path → git
- [ ] 21.3 - Repoint CI e2e-ios-sim lane at the git dep
- [ ] 21.4 - Delete vendored tools/ios-inspect + Cargo exclude
- [ ] 21.5 - Repoint docs at the external ios-inspect repo
- [ ] 21.6 - Verify e2e compiles + iOS-sim lane runs

## Phase A - Auto-generated demo reel (macOS) — LIVE

Pipeline **DONE + chotchki-approved (2026-06-24)**: per-window SCKit capture → `hstack`
2-pane composite (TV-left / phone-right, 60fps, dual title-bar crop) → cursor-hide + gold
tap-ripple → AV1/HEVC dual-encode. Content bugs #25 (Kaos swap) + #26 (portraits) fixed.
Capture script reaps the orphaned emulator/chromedriver after each run. Remaining:

- [ ] A.6 - Site / README embed (was 15.7)
  - [ ] A.6.1 - Per-scenario MP4 dropped to `docs/assets/videos/`
  - [ ] A.6.2 - Scenario-driven HTML gallery generator (per-scene stills via `ffmpeg -ss`, Jekyll page at `docs/tour.md`)
  - [ ] A.6.3 - Site integration on hotchkiss.io (`docs/index.md` hero embeds the reel; `docs/features.md` links scenarios)
- [ ] A.7 - Beat content tweaks (chotchki) — `tools/playthrough/src/beats.rs` tap targets / flow + the timeline's captions / timing. Pipeline needs no changes: edit → re-capture → render.

## Phase 14 - macOS code-signing + notarization — LIVE

mac is now a real publish target, so the Tier-2 signed path is live work (no longer
"v1 ships unsigned"). Goal: provision the Apple Developer cert + the 7 GitHub Environment
secrets, flip the four `if: false` guards back on, and verify a tag push produces a signed +
notarized + stapled `.dmg` Gatekeeper accepts without the "unknown developer" prompt. Most
of this is chotchki executing steps on his Mac + GitHub UI; the code path (`release.yml`,
`tools/build-macos-app.sh`, `docs/dev/release-signing.md`) already exists and is canonical.

- [ ] 14.1 - **Apple Developer prereqs (on the Mac).**
  - [ ] 14.1.1 - Confirm Apple Developer membership active at <https://developer.apple.com> → Membership. Capture the 10-char Team ID — feeds `APPLE_TEAM_ID` in 14.3.2.
  - [ ] 14.1.2 - Generate (or confirm) the "Developer ID Application" cert (Xcode → Settings → Accounts → Manage Certificates → `+`). Distinct from the auto-created "Apple Development" cert; the Developer ID flavor is what Gatekeeper trusts.
  - [ ] 14.1.3 - Generate an app-specific password at <https://appleid.apple.com> → Sign-in and Security → App-Specific Passwords. Label `spc-notarize`. Copy immediately. Feeds `APPLE_APP_PASSWORD`.
- [ ] 14.2 - **Export cert + collect secret values.**
  - [ ] 14.2.1 - Keychain Access → My Certificates → right-click `Developer ID Application: Christopher Hotchkiss (TEAMID)` → Export as `.p12` with a strong passphrase (`MACOS_CERT_PASSWORD`).
  - [ ] 14.2.2 - `base64 -i <export.p12> | pbcopy` → `MACOS_CERT_P12_BASE64`. Stash the passphrase before moving on.
  - [ ] 14.2.3 - `security find-identity -v -p codesigning` → copy the full identity line into `MACOS_CERT_IDENTITY` (the `(TEAMID)` suffix must match exactly).
  - [ ] 14.2.4 - `openssl rand -base64 32` → throwaway `KEYCHAIN_PASSWORD` for the CI keychain.
- [ ] 14.3 - **GitHub Environment + 7 secrets.**
  - [ ] 14.3.1 - Repo Settings → Environments → New `release`. Deployment tag rule: Selected → `v*.*.*`. Optionally add yourself as Required Reviewer.
  - [ ] 14.3.2 - Add 7 secrets per `docs/dev/release-signing.md`: `MACOS_CERT_P12_BASE64`, `MACOS_CERT_PASSWORD`, `MACOS_CERT_IDENTITY`, `KEYCHAIN_PASSWORD`, `APPLE_ID` (chris@hotchkiss.io), `APPLE_APP_PASSWORD`, `APPLE_TEAM_ID`.
  - [ ] 14.3.3 - Delete the local `.p12` export (cert stays in your login keychain for local builds).
- [ ] 14.4 - **Local end-to-end dry run.**
  - [ ] 14.4.1 - `SIGN_IDENTITY="Developer ID Application: Christopher Hotchkiss (TEAMID)" tools/build-macos-app.sh` → signed `dist/*.dmg`; `codesign -dvv dist/*.dmg` reports the signature.
  - [ ] 14.4.2 - `xcrun notarytool submit dist/*.dmg --apple-id chris@hotchkiss.io --team-id TEAMID --password <app-specific> --wait` (catches team-id / password mismatches locally).
  - [ ] 14.4.3 - `xcrun stapler staple dist/*.dmg` + `spctl -a -t open --context context:primary-signature -vv dist/*.dmg`. Green here = green in CI.
- [ ] 14.5 - **Re-enable the disabled CI steps.**
  - [ ] 14.5.1 - Flip the four `if: false` guards in `release.yml` back to the repo-owner check. Steps: Import cert / Build signed .app+.dmg / Notarize and staple / Upload signed DMG.
  - [ ] 14.5.2 - Add `${{ env.RELEASE_DMG }}` back to the macOS "Publish release" `files:` list.
  - [ ] 14.5.3 - Update the disabled-signing comment block in `release.yml` — drop "secrets pending"; reference `docs/dev/release-signing.md` as the live runbook.
- [ ] 14.6 - **CI dry run via `workflow_dispatch`.**
  - [ ] 14.6.1 - Actions → Release → Run workflow (loosen the deployment-tag rule to `main` temporarily, OR push a throwaway `v0.0.0-dryrun` tag — latter is cleaner).
  - [ ] 14.6.2 - Watch the macOS job; the 5 signing steps should be green (~10–15 min).
  - [ ] 14.6.3 - Download the signed `.dmg`, mount on a clean Mac, confirm `spctl -a` green; revert the deployment-rule loosening.
- [ ] 14.7 - **Real-tag validation + close out 10.9.2.**
  - [ ] 14.7.1 - Cut the next real release tag; signed + notarized + stapled dmg lands on the Release.
  - [ ] 14.7.2 - Download from the Release page on a fresh Mac (or `xattr -d com.apple.quarantine`); confirm Gatekeeper allows direct double-click.
  - [ ] 14.7.3 - Mark PLAN 10.9.2's macOS Tier-2 bullet done; update `docs/dev/release-signing.md` Status to "live as of v<X.Y.Z>".
  - [ ] 14.7.4 - CLAUDE.md "macOS support" → "Release artifact" line: replace "ships unsigned, right-click + Open" with the signed+notarized reality.
- [ ] 14.8 - **Rotation reminders (calendar).**
  - [ ] 14.8.1 - App-specific password: 1-year rotation reminder (`APPLE_APP_PASSWORD`).
  - [ ] 14.8.2 - Developer ID cert: 5-year validity; reminder 11 months before expiry (rotation playbook in `docs/dev/release-signing.md`).

## Phase R - Residual fixes (survivors from the 2026-06-24 triage)

- [ ] R.1 - Launcher exit button cut off on the 86" 4K TV (was 10.8.3) — layout fix in the launcher action row at 4K/overscan.
- [ ] R.2 - Distribution docs are winget-first (was 13.5.3 / 13.5.4) — update the `release.yml` comment + CLAUDE.md "Distribution" section (drop the GitHub-Releases-first framing).
- [ ] R.3 - Trigger the gated `rpcs3-patched.yml` full build for the current pin `927e2492e` so the bundled patched RPCS3 binary exists for release (was 16.12.6) — **verify it hasn't already run** before doing.

