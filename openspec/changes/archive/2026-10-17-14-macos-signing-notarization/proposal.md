# Phase 14 - macOS code-signing + notarization — LIVE

## Why

mac is now a real publish target, so the Tier-2 signed path is live work (no longer
"v1 ships unsigned"). Goal: provision the Apple Developer cert + the 7 GitHub Environment
secrets, flip the four `if: false` guards back on, and verify a tag push produces a signed +
notarized + stapled `.dmg` Gatekeeper accepts without the "unknown developer" prompt. Most
of this is chotchki executing steps on his Mac + GitHub UI; the code path (`release.yml`,
`tools/build-macos-app.sh`, `docs/dev/release-signing.md`) already exists and is canonical.

## What Changes

- Shipped: 14.1, 14.2, 14.3.1–14.3.2 and 14.5 (`e392a7e`, 2026-06-25).
  - The `release` environment (created 2026-05-03) has a single deployment rule, tag `v*.*.*`, and holds all seven secrets (`gh api .../environments/release/secrets`). `APPLE_*` and `KEYCHAIN_PASSWORD` date from 2026-05-03.
  - The three cert secrets were re-set 2026-06-26 16:54–16:55Z from a new leaf cert issued at 16:37Z that day. The trigger was v1.9.16's notarization failing with "binary is not signed with a valid Developer ID certificate" (`6f92c9b`). So 14.1.2 and 14.2.x effectively ran twice, and 14.3.3 may now have two `.p12` exports to clean up (May's and June's).
  - The four signing steps gate on `github.repository == 'chotchki/skylander-portal-controller'` (`release.yml:560`, `:592`, `:606`, `:656`). `RELEASE_DMG` is back in Publish's `files:` (`:683`), and the comment block says LIVE (`:553-557`).
- 14.6.1–14.6.3 are done but unticked. Six tags failed before the first green signed release:
  - v1.9.12 (`98dbdda`) was cut, but run 28208469536 failed both legs before signing (Windows on the patched-RPCS3 download, macOS on `cargo test`, where `apple-metal` needed a newer SDK; fixed in `a86935d`). No release was published.
  - v1.9.13 failed the same `apple-metal` build on macos-15 (`f9142c6`'s runner bump wasn't enough). v1.9.14 hit a SIGTERM race (`fc9bcb7`). v1.9.15 and v1.9.16 failed at notarization: v1.9.15 on a 403 (a required Apple agreement missing or expired), v1.9.16 as Invalid ("binary is not signed with a valid Developer ID certificate"). v1.9.17 notarized, then failed on a `/` in the artifact name (`d35dfb9`).
  - v1.9.18 (run 28253574658, 2026-06-26) went fully green and was the first signed dmg. Every release since carries one: v1.9.19, v1.9.20, v1.9.21 (Latest) and the v1.10.0 prerelease (run 35543190949).
  - Checked today on the dev Mac: `spctl -a -t open --context context:primary-signature` accepts the v1.9.18 dmg as "Notarized Developer ID", and `xcrun stapler validate` passes. v1.10.0's run log shows the same chain on the runner (`status: Accepted`, a clean staple, `source=Notarized Developer ID`). 14.6.3 asked for a clean Mac, and the dev Mac isn't one.
- 14.6.4 (OPEN): the winget step is still `if: false` (`release.yml:388`), although its re-enable condition ("once a release goes fully green") has held since v1.9.18.
  - Our lane last submitted 1.9.13 (winget-pkgs #393437, 2026-06-26). winget isn't stale anyway: a third-party updater bot (`damn-good-b0t`) opened #418590 (1.9.21), which merged 2026-08-16. 1.10.0, a prerelease, isn't in winget.
  - Re-enabling the step as written is a trap. It fires on the tag push, but betas are cut by publishing a regular release and flipping it to prerelease afterward (Publish hard-codes `prerelease: false`, `:368`/`:677`; that's how v1.10.0 shipped). So every beta would go to winget-pkgs, and with the bot active two submitters can race.
  - The box needs a call: gate the step on a non-beta (or a manual dispatch), or descope it and leave winget to the bot.
- 14.7:
  - 14.7.1 is superseded: v1.9.19 through v1.10.0 all ship the signed, notarized, stapled dmg.
  - 14.7.2 is unverified (it needs a fresh Mac or a quarantined download). The closest evidence: the v1.9.18 and v1.9.19 dmgs chotchki downloaded through Safari on 2026-06-26 still carry the quarantine flag and pass `spctl` today, but nobody recorded the double-click. CLAUDE.md's "macOS support" Release-artifact bullet asserts it works.
  - 14.7.3 is half moot: PLAN 10.9.2 doesn't exist anymore. The 2026-06-24 triage (`9b6bc57`) collapsed Phase 10 into a one-line summary in PLAN_ARCHIVE.md, which is frozen. The other half is still open: `docs/dev/release-signing.md:9-14` still says "Pending: first real tag-push validation (PLAN 14.7)".
  - 14.7.4 is done but unticked. `e34c4b9` (2026-06-28) rewrote CLAUDE.md's "macOS support" Release-artifact bullet to the signed reality. Its "live as of v1.9.19" is a release late: signing first shipped on v1.9.18 (a 28.6 MB mock-only dmg), and v1.9.19 is the first dmg with the bundled RPCS3.
- 14.8.2's premise is WRONG, and its deadline is 107 days out.
  - The Developer ID Application cert in the shipped dmg (extracted from v1.10.0) runs 2026-06-26 → 2027-02-01, not five years. It chains to Apple's 2012 "Developer ID Certification Authority" intermediate, which itself expires 2027-02-01, and the leaf is capped at that date.
  - From 2027-02-01 the macOS job can't sign. The Windows job publishes on its own (`release.yml:405-406`), so a tag after that date ships no dmg until a re-issued cert is in the three cert secrets. Dmgs already notarized and stapled keep opening.
  - Re-issuing under Apple's G2 Developer ID sub-CA should get the normal term (unverified here). `docs/dev/release-signing.md:164-165` ("typically 5 years") is wrong for this cert, and 14.8.2's "11 months before expiry" date passed before the cert existed.
  - 14.8.1: `APPLE_APP_PASSWORD` was set 2026-05-03 and never rotated, so its one-year reminder lands 2027-05-03. Whether either calendar entry exists can't be checked from here.
- 14.3.3 (OPEN): unverified, since it lives on chotchki's Mac.
- 14.4 (OPEN) is superseded as written. It existed to catch team-id or password mismatches before CI, and the real-tag chain did that job instead. It has a natural second use, though: running the re-issued cert through `notarytool` locally before the next tag, rather than burning patch tags the way v1.9.15 and v1.9.16 did.
- Stale text, no box:
  - `release.yml:3-11` says releases are drafts (Publish uses `prerelease: false`). It recommends a throwaway `v0.0.0-dryrun` tag, which breaks both the no-delete-tags rule and WiX's numeric-only versions. It also cites a `release-signing.md` section ("Dry-run via workflow_dispatch") that is now "Validating the signed pipeline" (`:64`).
  - `release.yml:403` dates signing live to 2026-06-25, but the first green run was 2026-06-26.
  - `docs/setup.md:44-50` still says the Mac ships a mock-only, unsigned tar.gz: right-click → Open, with signing "post-MVP".
  - `docs/roadmap.md:47` still calls signing disabled pending secrets, and `:61-63` still lists Phase 14 as planned, validated by a dry-run tag.
- Phase 14 then reduces to:
  - the cert re-issue, which is dated and unboxed (the obvious home is 14.8.2, rewritten)
  - 14.6.4's winget call
  - 14.7.3's Status line plus the stale-text pass
  - 14.3.3 and 14.8.1 on chotchki's side
  - 14.4, descoped or repurposed for the re-issue

## Capabilities

### New Capabilities

None declared yet. This change came over from PLAN.md on 2026-10-17 as a task list, not a spec, so `.openspec.yaml` carries `skip_specs: true`. When work resumes on a box that changes behavior, name the capability here, write its delta under `specs/` and drop the flag.

### Modified Capabilities

None yet.

## Impact

- Code and config:
  - `.github/workflows/release.yml`: the macOS job (`:395-684`), with `environment: release` at `:412`, cert import at `:559`, the signed build at `:591`, notarize + staple at `:605-653` and Publish at `:668-684`; and the winget step (`:375-393`).
  - `tools/build-macos-app.sh`, which signs the nested RPCS3 inside-out.
  - The GitHub `release` environment and its seven secrets, which have no file behind them.
- Tests: none at unit level. In CI only a `v*` tag run proves signing, because the environment's tag rule leaves a branch dispatch with no secrets (`docs/dev/release-signing.md:64-81`); 14.4 is the local route. The release-time check is `spctl` + `stapler validate` on the published dmg.
- Source of truth:
  - `docs/dev/release-signing.md`: the runbook. Its Status block is stale and its rotation section is wrong about the term.
  - CLAUDE.md "macOS support": the Release-artifact line.
  - `docs/dev/winget-submission.md` for 14.6.4. Its Status block (`:14-22`, 2026-05-30, "Next step is the PR (13.3)") predates all four merged versions.
- Track: product.
- Release reach:
  - Every `v*` tag runs this job, and the dmg is what macOS users download (single digits per dmg as of 2026-10-17: 4 on v1.9.21, against 232 on its MSI).
  - The open boxes ship nothing, except the cert re-issue, which only a `v*` tag can prove: a patch release that fixes forward.
- One-way doors:
  - A pushed tag is never deleted, so every failed signing attempt costs a version number (v1.9.12–v1.9.17 for one green release).
  - A notarized dmg can't be recalled: per the runbook, revoking the cert doesn't invalidate a stapled ticket (`release-signing.md:173-179`).
  - winget-pkgs submissions are public and go through Microsoft moderation.
- Guacamole: nothing changes until G1. At G1 the nested RPCS3 and its JIT entitlements leave the bundle and `guacamole` becomes the signed app. The cert, the notarization flow and the environment carry over, and `tools/build-macos-app.sh` re-targets.
