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

## Phase T - RPCS3 pin bump (927e2492e → 09d602fd5; drop SPU patches → clean P1–P8)

Unblocks the Windows side of the v1.9.13 signed release (was R.3). New pin = latest master
09d602fd5 (2026-06-24): includes the SPU-Giga fix (#18935, replaces local 0004/0005) + 2
SPU-LLVM perf commits. The 4 new commits are all SPU-only, orthogonal to the P-patch seams.

- [x] T.1 - Rebase P1–P8 onto pin `09d602fd5` (drop SPU 0004/0005 — now upstream #18935); P8 cherry-picked past a local CRLF quirk on `swapchain_macos.hpp`
- [x] T.2 - Regenerate `rpcs3-patches/` (clean P1–P8, 8 patches) + reset the gitlink to pristine `09d602fd5`
- [x] T.3 - Bump the pin docs (rpcs3-patches/README + patch-list, research strategy, release.yml download tag, memory)
- [x] T.4 - Fix `rpcs3-patched.yml` apply-clean allowlist → P1–P8 file set (LC_ALL=C deterministic sort)
- [x] T.5 - Commit the bump + trigger the gated `rpcs3-patched.yml` full build (= R.3) → `rpcs3-patched-09d602fd5`
- [ ] T.6 - Verify the build green (compile proof) + the bundled prerelease published
- [ ] T.7 - Cut v1.9.13 — fully green (mac signing + Windows bundled-RPCS3 download)

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

- [x] 14.1 - **Apple Developer prereqs (on the Mac).**
  - [x] 14.1.1 - Confirm Apple Developer membership active at <https://developer.apple.com> → Membership. Capture the 10-char Team ID — feeds `APPLE_TEAM_ID` in 14.3.2.
  - [x] 14.1.2 - Generate (or confirm) the "Developer ID Application" cert (Xcode → Settings → Accounts → Manage Certificates → `+`). Distinct from the auto-created "Apple Development" cert; the Developer ID flavor is what Gatekeeper trusts.
  - [x] 14.1.3 - Generate an app-specific password at <https://appleid.apple.com> → Sign-in and Security → App-Specific Passwords. Label `spc-notarize`. Copy immediately. Feeds `APPLE_APP_PASSWORD`.
- [x] 14.2 - **Export cert + collect secret values.**
  - [x] 14.2.1 - Keychain Access → My Certificates → right-click `Developer ID Application: Christopher Hotchkiss (TEAMID)` → Export as `.p12` with a strong passphrase (`MACOS_CERT_PASSWORD`).
  - [x] 14.2.2 - `base64 -i <export.p12> | pbcopy` → `MACOS_CERT_P12_BASE64`. Stash the passphrase before moving on.
  - [x] 14.2.3 - `security find-identity -v -p codesigning` → copy the full identity line into `MACOS_CERT_IDENTITY` (the `(TEAMID)` suffix must match exactly).
  - [x] 14.2.4 - `openssl rand -base64 32` → throwaway `KEYCHAIN_PASSWORD` for the CI keychain.
- [ ] 14.3 - **GitHub Environment + 7 secrets.**
  - [x] 14.3.1 - Repo Settings → Environments → New `release`. Deployment tag rule: Selected → `v*.*.*`. Optionally add yourself as Required Reviewer.
  - [x] 14.3.2 - Add 7 secrets per `docs/dev/release-signing.md`: `MACOS_CERT_P12_BASE64`, `MACOS_CERT_PASSWORD`, `MACOS_CERT_IDENTITY`, `KEYCHAIN_PASSWORD`, `APPLE_ID` (chris@hotchkiss.io), `APPLE_APP_PASSWORD`, `APPLE_TEAM_ID`.
  - [ ] 14.3.3 - Delete the local `.p12` export (cert stays in your login keychain for local builds).
- [ ] 14.4 - **Local end-to-end dry run.**
  - [ ] 14.4.1 - `SIGN_IDENTITY="Developer ID Application: Christopher Hotchkiss (TEAMID)" tools/build-macos-app.sh` → signed `dist/*.dmg`; `codesign -dvv dist/*.dmg` reports the signature.
  - [ ] 14.4.2 - `xcrun notarytool submit dist/*.dmg --apple-id chris@hotchkiss.io --team-id TEAMID --password <app-specific> --wait` (catches team-id / password mismatches locally).
  - [ ] 14.4.3 - `xcrun stapler staple dist/*.dmg` + `spctl -a -t open --context context:primary-signature -vv dist/*.dmg`. Green here = green in CI.
- [x] 14.5 - **Re-enable the disabled CI steps.**
  - [x] 14.5.1 - Flip the four `if: false` guards in `release.yml` back to the repo-owner check. Steps: Import cert / Build signed .app+.dmg / Notarize and staple / Upload signed DMG.
  - [x] 14.5.2 - Add `${{ env.RELEASE_DMG }}` back to the macOS "Publish release" `files:` list.
  - [x] 14.5.3 - Update the disabled-signing comment block in `release.yml` — drop "secrets pending"; reference `docs/dev/release-signing.md` as the live runbook.
- [ ] 14.6 - **Validate signing via a real patch release** (hard no-delete-tags rule — fix forward, no throwaway tags).
  - [ ] 14.6.1 - Cut the next patch tag `v1.9.12` (matches the `v*.*.*` env rule, so secrets flow with no loosening).
  - [ ] 14.6.2 - Watch the macOS job; the 5 signing steps should be green (~10–15 min).
  - [ ] 14.6.3 - Download the signed `.dmg`, confirm `spctl -a` green on a clean Mac. If signing fails, fix forward + cut the next patch.
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

## Backlog (not yet phased)

**Near-term / real:**
- **Bump RPCS3 pin past #18935 → drop the 2 local SPU-Giga patches** (back to clean P1–P8) (2026-06-25).
- **Spike (stretch): live 2× flip while a game runs** (ex S.8) — uncertain; needs an IPC `SURFACE_SCALE` command + a runtime size flag (not the env) + mid-render swapchain recreation + CAMetalLayer extent update + controller re-fit. The swapchain-recreate-under-load is the resize path the 720p pin avoids (top-left-subrect risk). Attempt only once Phase S's boot-time toggle is solid.
- **Phase S 2× sustained/thermal validation** (ex S.6) — the toggle is shipped + smoke-tested (cold-boot + recorder narrative clean, surface confirmed 2560×1440, no crash). Remaining: a long *warm* session on the M3 Max — sustained framerate + thermals vs 720p. Non-blocking (default off); needs real kid-play time on the laptop, which ties up the work machine — hence deferred.
**Phone UI polish** (ex 4.18.x / 9.8 — all defer-quality nice-to-haves):
- Service worker for PWA cache + update detection (4.18.1c) — gates an iOS browser smoke re-test.
- Profile "last used N days ago" subtext (4.18.10); per-card tagline + "currently playing" marker (4.18.12).
- Figure-detail ghost-grid backdrop (4.18.20); ResumeModal element-tinted bezels (4.18.22) + relative-time subtext (4.18.23); MenuOverlay post-action transitions (4.18.24).
- CSS component consolidation sweep (9.8).
**Stat parsing** (ex 6.2.x — needs real-dump samples):
- Trap / Vehicle / CYOS payload parse + display (6.2.1/.3/.4/.5); investigate 10 CRC-failing samples (6.2.8); pin Vehicle + CYOS `figure_id` ranges against real dumps (6.2.9).
- Suppress RPCS3 menu-nav window flicker (6.1 — UIA fallback only).
**iOS device validation:**
- Paired visual-regression snapshots (10.4.5); real-iPhone Bonjour check (11.8.3); iPad + iPhone sim parity (11.8.4).
**Demo-reel extras** (post-A.6):
- Extra scenario flows: co-op / eviction, stat-edit, appearance-cycle, admin-tour (ex 15.6.x); PWA NarrationOverlay (15.4.2); retire `screenshot_tour` (15.8.x); CI hook to upload generated MP4s as release assets (15.9.x).
**Misc:**
- Collapse positional-slot model server-side (16.5.3); mute HEAT5150 SelfReg DLL warnings in the release log (18.8).
