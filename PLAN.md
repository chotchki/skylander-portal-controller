# Skylander Portal Controller — Execution Plan

Active work toward MVP. Closed work lives in [PLAN_ARCHIVE.md](PLAN_ARCHIVE.md) — Phases 0–13, 15–20, A.1–A.5, B (most done in code; Phase 12 struck as a non-goal). Swept 2026-06-24 after a full plan triage.

Conventions:
- `[ ]` pending, `[x]` done, `[~]` in progress, `[?]` blocked / needs discussion.
- New tasks should always be numbered and have a checkbox so they're traceable.
- Don't skip a review checkpoint; the point is to re-plan with new information.

## Non-goals
- No bundling of `.sky` files or game backups (piracy). The patched RPCS3 *itself* IS bundled — GPL, source public — per the v1 distribution model (16.9.0b): Windows today, macOS via Phase U.
- No Linux support — production targets are Windows + macOS.
- No user-entered figure names.
- No audio (text-only Kaos to dodge copyright).
- No live wiki scraping at runtime — data is committed to the repo.
- **No AXUIElement macOS driver** (ex-Phase 12) — IPC to the patched RPCS3 supersedes it.

---

## Phase U - Ship the patched RPCS3 emulator in the macOS release (mock → ipc)

The mac .dmg currently bundles MOCK only, so "launch game" can't boot a real emulator —
wasting the Phase 16 IPC + P8 surface-embed work (live-validated ~60fps on the M3 Max via
`.ci-local/build-mac.sh`). This phase packages that proven runtime into the release: bundle
+ sign the patched RPCS3, flip the mac default mock → ipc, mirroring the Windows v1
distribution model (16.9.0b). The runtime is done — this is distribution plumbing.
Reconnaissance: the Explore done|missing|blocker trace (2026-06-26). Windows is the template
throughout (`release.yml` Windows job + `config.rs::migrate_install_paths`).

- [x] U.1 - Fix the mock-mode game-launch hang: the non-Windows `BootDirect` branch spawns no process + never sets `game_playable`, so the launcher waits on the stable-timer fallback ("loading" hang). DONE — `no_real_process` reveal in `state.rs` + `boot_direct_mock_reveals_game_playable` test (passes, clippy-clean).
- [ ] U.2 - Produce a distributable, RELOCATABLE macOS patched-RPCS3 from CI (THE blocker — `rpcs3-patched.yml`'s upstream mac build uses the flaky qt-downloader; `.ci-local/build-mac.sh` is the proven producer). Draft+verify reshaped this — the real risk is U.2.4.
  - [ ] U.2.1 - Promote `.ci-local/build-mac.sh` to a gated CI lane on `macos-15` (arm64 — avoids the `VirtualApple` fatal-error landmine macos-14 trips; Homebrew Qt + `llvm@21`, filtered submodule checkout, `macdeployqt` + ad-hoc re-sign → `rpcs3.app`) + a `--version` smoke-gate (build-mac.sh only warns on launch failure).
  - [ ] U.2.2 - Publish `rpcs3-patched-<pin>-macos-arm64.tar.gz` + `.sha256` to the `rpcs3-patched-<pin>` prerelease (pin from gitlink, mirror Windows). **Harden the existing Windows lane's `gh release create` against the now-dual-publish race (it `throw`s today; add the `|| gh release view` fallback).**
  - [ ] U.2.3 - Smoke-verify the published artifact BOOTS to playable (extend `live_launch.rs`; `--version` isn't enough — leave unticked until a real boot check).
  - [x] U.2.4 - **Self-containment** — DONE + validated locally. Turned out tiny: `macdeployqt` already relocated ~everything; only 3 bundled dylibs kept their own `/opt/homebrew` `LC_ID_DYLIB` (libbrotlicommon/libgcc_s/libc++abi). Added a general `install_name_tool` relocate pass to `build-mac.sh` → `otool -L` reports ZERO Homebrew refs + `rpcs3 --version` still runs. CI smoke-gates it (no-Homebrew-refs check).
- [ ] U.3 - Bundle the patched RPCS3 into the mac release (`release.yml` build-macos-release + `tools/build-macos-app.sh`): download + sha-verify the macos artifact, nest it under the launcher `.app` (`Contents/Resources/rpcs3/…`), mirroring the Windows `rpcs3/` placement.
- [ ] U.4 - Sign the nested RPCS3 for notarization (highest-risk — a mismatched/unsigned nested binary fails notarization).
  - [ ] U.4.1 - Codesign inside-out with the Developer ID + hardened runtime (dylibs + Qt **plugins** (`Contents/MacOS/share/qt6/plugins`) + frameworks → `RPCS3.app` → launcher binary → launcher `.app`; drop blanket `--deep`). Enumerate Mach-O **by type** not extension; `xattr -cr` first; **JIT entitlements** on the rpcs3 binary (`allow-jit`+`allow-unsigned-executable-memory`+`disable-library-validation`) or it SIGKILLs on first recompile; **staple the `.app`** (not just the `.dmg`); guard `find` against missing dirs under `set -e`.
  - [ ] U.4.2 - Validate notarization accepts the nested bundle (the 14.6 CI notary-log dump surfaces any "Code object is not signed" inline).
- [ ] U.5 - Flip the mac default mock → ipc + wire the mac game-launch.
  - [ ] U.5.1 - `config.rs`: on macOS derive the bundled rpcs3 path + force `DriverKind::Ipc` via `migrate_install_paths_macos` — an `.exists()` gate → Mock fallback until U.3 bundles the binary, so SAFE to land early (no flag day). **Auto-promote existing mock configs to IPC** (chotchki: mock = dev / software-GL winget fallback only; `SKYLANDER_PORTAL_DRIVER=mock` is the demo escape). Needs `#[allow(dead_code)]` on the 4 new helpers (pre-push clippy gate).
  - [x] U.5.2 - **Already wired** — `BootDirect` discriminates by the `driver.ipc_socket_path()` capability probe (not `DriverKind`), and the IPC spawn (`launch_no_gui` → `UnixRpcsProcess`) + `is_playable()` poll is already cross-platform. Clarifying comment + `debug_assert` added (U.1). Goes live once U.5.1 makes the mac driver IPC.
  - [ ] U.5.3 - `release.yml` mac build features: comment-only — IPC is NOT feature-gated (`IpcPortalDriver`/`UnixRpcsProcess` compile unconditionally), so the current `--features sky-stats,mock-driver-runtime` already includes the IPC runtime; `mock-driver-runtime` stays the fallback.
  - [x] U.5.4 - Fix the macOS `data_root` clobber (figure portraits + box-art BROKEN on the CURRENT signed mac release): `config.rs` hard-coded `<exe_parent>/data` = `Contents/MacOS/data` (nonexistent) while assets ship in `Contents/Resources/data`. Routed through new `paths::app_data_root()`. Independent of the emulator — shipped in the U.1 batch. (Flagged by two draft-verifiers.)
- [ ] U.6 - macOS first-launch wizard: prompt for the user's existing RPCS3 data dir (`config_dir` = firmware + `games.yml`) like Windows; the control binary is the bundled patched rpcs3. Remove the macOS wizard short-circuit (`config.rs`).
- [ ] U.7 - End-to-end on a clean Mac (chotchki, real hardware): install the signed/notarized .dmg, run the wizard (point at firmware/games), launch Giants → the bundled emulator boots + composites into the launcher (P8 surface). No Gatekeeper wall.
- [ ] U.8 - Docs: CLAUDE.md "macOS support"/"Distribution" (mac ships the patched RPCS3, ipc default — drop the mock-only caveat); `release-signing.md` nested-app note; `docs/dev/macos-rpcs3-build.md` (CI lane); bank the mac-signing + nested-app gotchas to memory.

## Phase T - RPCS3 pin bump (927e2492e → 09d602fd5; drop SPU patches → clean P1–P8)

Unblocks the Windows side of the v1.9.13 signed release (was R.3). New pin = latest master
09d602fd5 (2026-06-24): includes the SPU-Giga fix (#18935, replaces local 0004/0005) + 2
SPU-LLVM perf commits. The 4 new commits are all SPU-only, orthogonal to the P-patch seams.

- [x] T.1 - Rebase P1–P8 onto pin `09d602fd5` (drop SPU 0004/0005 — now upstream #18935); P8 cherry-picked past a local CRLF quirk on `swapchain_macos.hpp`
- [x] T.2 - Regenerate `rpcs3-patches/` (clean P1–P8, 8 patches) + reset the gitlink to pristine `09d602fd5`
- [x] T.3 - Bump the pin docs (rpcs3-patches/README + patch-list, research strategy, release.yml download tag, memory)
- [x] T.4 - Fix `rpcs3-patched.yml` apply-clean allowlist → P1–P8 file set (LC_ALL=C deterministic sort)
- [x] T.5 - Commit the bump + trigger the gated `rpcs3-patched.yml` full build (= R.3) → `rpcs3-patched-09d602fd5`
- [x] T.6 - Verify the build green (compile proof) + the bundled prerelease published — `rpcs3-patched-09d602fd5` (53.7MB) up; needed apply.sh CRLF normalization (3 iters) + a pin-from-gitlink fix
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
  - [ ] 14.6.4 - Re-enable the winget auto-PR (`release.yml` — restore the tag `if:` on the "Publish to winget" step) once a release goes fully green (signed mac dmg present). Disabled 2026-06-26 so the mac-signing patch iterations don't churn microsoft/winget-pkgs with no-op Windows resubmissions.
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
- [x] R.3 - Trigger the gated `rpcs3-patched.yml` full build (done via Phase T at pin `09d602fd5`) — `rpcs3-patched-09d602fd5` Windows binary published (was 16.12.6).

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
