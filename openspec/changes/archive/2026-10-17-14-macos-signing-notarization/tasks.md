# Tasks

## 14. macOS code-signing + notarization — LIVE

- [x] 14.1 **Apple Developer prereqs (on the Mac).**
  - [x] 14.1.1 Confirm Apple Developer membership active at <https://developer.apple.com> → Membership. Capture the 10-char Team ID — feeds `APPLE_TEAM_ID` in 14.3.2.
  - [x] 14.1.2 Generate (or confirm) the "Developer ID Application" cert (Xcode → Settings → Accounts → Manage Certificates → `+`). Distinct from the auto-created "Apple Development" cert; the Developer ID flavor is what Gatekeeper trusts.
  - [x] 14.1.3 Generate an app-specific password at <https://appleid.apple.com> → Sign-in and Security → App-Specific Passwords. Label `spc-notarize`. Copy immediately. Feeds `APPLE_APP_PASSWORD`.
- [x] 14.2 **Export cert + collect secret values.**
  - [x] 14.2.1 Keychain Access → My Certificates → right-click `Developer ID Application: Christopher Hotchkiss (TEAMID)` → Export as `.p12` with a strong passphrase (`MACOS_CERT_PASSWORD`).
  - [x] 14.2.2 `base64 -i <export.p12> | pbcopy` → `MACOS_CERT_P12_BASE64`. Stash the passphrase before moving on.
  - [x] 14.2.3 `security find-identity -v -p codesigning` → copy the full identity line into `MACOS_CERT_IDENTITY` (the `(TEAMID)` suffix must match exactly).
  - [x] 14.2.4 `openssl rand -base64 32` → throwaway `KEYCHAIN_PASSWORD` for the CI keychain.
- [x] 14.3 **GitHub Environment + 7 secrets.** (2026-10-17 review: the Environment + secrets exist; 14.3.3 moved to the backlog)
  - [x] 14.3.1 Repo Settings → Environments → New `release`. Deployment tag rule: Selected → `v*.*.*`. Optionally add yourself as Required Reviewer.
  - [x] 14.3.2 Add 7 secrets per `docs/dev/release-signing.md`: `MACOS_CERT_P12_BASE64`, `MACOS_CERT_PASSWORD`, `MACOS_CERT_IDENTITY`, `KEYCHAIN_PASSWORD`, `APPLE_ID` (chris@hotchkiss.io), `APPLE_APP_PASSWORD`, `APPLE_TEAM_ID`.
  - ~~14.3.3 Delete the local `.p12` export (cert stays in your login keychain for local builds).~~ (2026-10-17 review: moved to openspec/backlog.md as a personal chore)
- ~~14.4 **Local end-to-end dry run.**~~ (2026-10-17 review: superseded, real tags v1.9.15/v1.9.16 found the notary problems instead)
  - ~~14.4.1 `SIGN_IDENTITY="Developer ID Application: Christopher Hotchkiss (TEAMID)" tools/build-macos-app.sh` → signed `dist/*.dmg`; `codesign -dvv dist/*.dmg` reports the signature.~~ (2026-10-17 review: superseded with 14.4)
  - ~~14.4.2 `xcrun notarytool submit dist/*.dmg --apple-id chris@hotchkiss.io --team-id TEAMID --password <app-specific> --wait` (catches team-id / password mismatches locally).~~ (2026-10-17 review: superseded with 14.4)
  - ~~14.4.3 `xcrun stapler staple dist/*.dmg` + `spctl -a -t open --context context:primary-signature -vv dist/*.dmg`. Green here = green in CI.~~ (2026-10-17 review: superseded with 14.4)
- [x] 14.5 **Re-enable the disabled CI steps.**
  - [x] 14.5.1 Flip the four `if: false` guards in `release.yml` back to the repo-owner check. Steps: Import cert / Build signed .app+.dmg / Notarize and staple / Upload signed DMG.
  - [x] 14.5.2 Add `${{ env.RELEASE_DMG }}` back to the macOS "Publish release" `files:` list.
  - [x] 14.5.3 Update the disabled-signing comment block in `release.yml` — drop "secrets pending"; reference `docs/dev/release-signing.md` as the live runbook.
- [x] 14.6 **Validate signing via a real patch release** (hard no-delete-tags rule — fix forward, no throwaway tags). (2026-10-17 review: signing validated by fixing forward from v1.9.12 to v1.9.18)
  - [x] 14.6.1 Cut the next patch tag `v1.9.12` (matches the `v*.*.*` env rule, so secrets flow with no loosening). (2026-10-17 review: v1.9.12 tagged at 98dbdda (its run failed, no release))
  - [x] 14.6.2 Watch the macOS job; the 5 signing steps should be green (~10–15 min). (2026-10-17 review: watched; v1.9.12-v1.9.17 each failed and were fixed forward)
  - [x] 14.6.3 Download the signed `.dmg`, confirm `spctl -a` green on a clean Mac. If signing fails, fix forward + cut the next patch. (2026-10-17 review: v1.9.18 dmg passed spctl + stapler validate on the dev Mac)
  - ~~14.6.4 Re-enable the winget auto-PR (`release.yml` — restore the tag `if:` on the "Publish to winget" step) once a release goes fully green (signed mac dmg present). Disabled 2026-06-26 so the mac-signing patch iterations don't churn microsoft/winget-pkgs with no-op Windows resubmissions.~~ (2026-10-17 review: cancelled, the product is frozen)
- [x] 14.7 **Real-tag validation + close out 10.9.2.** (2026-10-17 review: v1.9.19-v1.10.0 all carry signed, notarized, stapled dmgs)
  - [x] 14.7.1 Cut the next real release tag; signed + notarized + stapled dmg lands on the Release. (2026-10-17 review: v1.9.19, v1.9.20, v1.9.21 and v1.10.0)
  - ~~14.7.2 Download from the Release page on a fresh Mac (or `xattr -d com.apple.quarantine`); confirm Gatekeeper allows direct double-click.~~ (2026-10-17 review: cancelled, the product is frozen, and no double-click was ever recorded)
  - ~~14.7.3 Mark PLAN 10.9.2's macOS Tier-2 bullet done; update `docs/dev/release-signing.md` Status to "live as of v<X.Y.Z>".~~ (2026-10-17 review: cancelled, the product is frozen; the 10.9.2 half was already moot)
  - [x] 14.7.4 CLAUDE.md "macOS support" → "Release artifact" line: replace "ships unsigned, right-click + Open" with the signed+notarized reality. (2026-10-17 review: e34c4b9)
- ~~14.8 **Rotation reminders (calendar).**~~ (2026-10-17 review: cancelled, the product is frozen; the Developer ID cert expires 2027-02-01 (noted on the backlog line))
  - ~~14.8.1 App-specific password: 1-year rotation reminder (`APPLE_APP_PASSWORD`).~~ (2026-10-17 review: cancelled, the product is frozen)
  - ~~14.8.2 Developer ID cert: 5-year validity; reminder 11 months before expiry (rotation playbook in `docs/dev/release-signing.md`).~~ (2026-10-17 review: cancelled, the product is frozen)
