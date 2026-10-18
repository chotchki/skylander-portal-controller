# Phase R - Residual fixes (survivors from the 2026-06-24 triage)

## Why

The 2026-06-24 triage (`9b6bc57`) checked PLAN.md's 191 open boxes against the code and swept everything done or obsolete into PLAN_ARCHIVE.md; R holds the three product survivors that fit no live phase. Two are small and old: a launcher layout bug only the 86" 4K TV shows (10.8.3, filed 2026-05-03), and distribution docs that still lead with GitHub Releases although winget has been the recommended Windows install since 2026-06-10 (13.5.3/13.5.4). The third, the gated patched-RPCS3 build (16.12.6), closed through Phase T.

## What Changes

- R.3 shipped. `rpcs3-patched-09d602fd5` was published 2026-06-26 07:18Z with the Windows zip (run 28221185265, whose macOS leg failed) and was ticked in `5687a22`. The macOS tarball joined it at 21:24Z from U.2's build (run 28265316693, `172bd0a`), which re-uploaded the Windows zip too. It has since been superseded as the shipping pin: both release lanes now pull `rpcs3-patched-accfecd2d` (`release.yml:117`, `:513`; Phase V's V.7).
- R.1 (OPEN) is blocked on the HTPC and may already be fixed.
  - The layout the box names has been reworked repeatedly since it was filed. First came an overscan jump-anchor (v1.9.3 → 16.9.8 → 16.10.6), which `crates/server/src/ui/main_screen.rs:109-110` records as "never worked completely right". Along the way, `2b01102` (16.9.5, v1.9.4) fixed an "Open in Browser" button that had gone invisible on the HTPC's TV. Then `b77b42d` (20.5, 2026-06-09, first shipped in v1.9.8) replaced the anchor outright.
  - That rewrite is deterministic: it reserves the action row's height from the bottom and keeps it above a mode-aware safe line, a 6% TV overscan inset in fullscreen (`TV_OVERSCAN_FRAC`, `main_screen.rs:125-131`) against a fixed 28 px in a desktop window.
  - `dced572` (20.4) hides the row on purpose while a game is live (`main_screen.rs:267-270`), so 10.8.3's "after phone connect" case now only covers the connected, not-yet-in-game QR face.
  - `b77b42d` was "user-confirmed on the dev box", not on the 86" TV. The triage carried R.1 forward two weeks after it shipped, and no commit or box since records a look at the TV.
  - The cheapest close is a look during V.8's session on the same HTPC with v1.10.0. If the Exit button sits fully on screen with a phone connected, tick R.1 with no code change. If not, a photo of the TV is the repro the box never had.
- R.2 (OPEN): one half is superseded, the other half is open, and there's more stale distribution text than the box names.
  - The `release.yml` half is gone. 13.5.3 meant the disabled-signing comment block, which `e392a7e` (14.5.3) replaced with the LIVE block (`release.yml:553-557`). The winget step's own comment (`:375-387`) is current. What's left in that file is its header (`:3-11`: draft releases, a throwaway `v0.0.0-dryrun` tag), which Phase 14's stale-text pass covers.
  - The CLAUDE.md half is still open: the first "Distribution" bullet reads "GitHub Releases zip + winget MSI".
  - README is already winget-first (the Install step leads with `winget install …`, since `28ef0f5`).
  - Stale and unnamed by the box:
    - `docs/setup.md`: `:3` and `:19-21` say the app doesn't bundle the emulator, `:41-44` names GitHub Releases as the only channel with a UIA-driven Windows zip and a mock-only Mac tar.gz, and nothing mentions the MSI or the dmg.
    - `docs/dev/winget-submission.md`: the Status block (`:14-22`, 2026-05-30) still says "Next step is the PR (13.3)", and four versions have merged since (1.9.8, 1.9.11, 1.9.13, 1.9.21). `:83-84` names `winget-releaser@latest`, while `release.yml:389` pins `@v2`.
    - `docs/roadmap.md:57-59` still lists Phase 13 (winget) as planned.
  - winget's own state belongs to 14.6.4. Our auto-PR has been off since 2026-06-26, and a third-party bot has carried winget to 1.9.21 since (winget-pkgs #418590). The doc should say what's true by the time R.2 lands.
- R then reduces to an HTPC look for R.1 (code only if the TV still clips it) and one docs pass across CLAUDE.md "Distribution", `docs/setup.md`, `docs/dev/winget-submission.md` and `docs/roadmap.md`.

## Capabilities

### New Capabilities

None declared yet. This change came over from PLAN.md on 2026-10-17 as a task list, not a spec, so `.openspec.yaml` carries `skip_specs: true`. When work resumes on a box that changes behavior, name the capability here, write its delta under `specs/` and drop the flag.

### Modified Capabilities

None yet.

## Impact

- Code:
  - R.1: `crates/server/src/ui/main_screen.rs`, the `render_main` budget (`:107-134`) and the action row (`:259-322`). `server_error.rs:73-76` and `crashed.rs:56-59` never got the 20.5 rewrite: they still push their button down with a proportional spacer (`remaining * 0.55`, no overscan inset), the kind of placement `2b01102` found cropped on the TV (there `remaining * 0.42`), and their comments still claim to match `render_main`.
  - R.2: docs only.
- Tests: none cover the layout. The budget is plain arithmetic inside `render_main` with no unit test, and only a person looking at the TV can confirm overscan.
- Source of truth:
  - R.1: SPEC.md owns the requirement (fullscreen on the 86" TV at 10 ft, an exit-to-desktop button: `:65`, `:301-302`). The layout rule itself is written down only in the `main_screen.rs:107-118` comment.
  - R.2: CLAUDE.md "Distribution", README "Install", `docs/setup.md` "Distribution" and `docs/dev/winget-submission.md`.
- Track: product.
- Release reach:
  - R.1 reaches users only through a `v*` tag (the Windows zip + MSI and the signed dmg), and only if the TV check finds it still broken. If it turns out already fixed, nothing ships and the box just ticks.
  - R.1 is also the one box that would drop `skip_specs`. A code fix changes what the TV shows, and that's worth writing down as a requirement: the action row stays inside the TV's title-safe area.
  - R.2 ships nothing and archives from main.
- One-way doors: none.
- Guacamole:
  - Both boxes are product-only, and the product binary stays untouched by Guacamole until G1. At G1 the eframe launcher retires and CLAUDE.md "Distribution" gets rewritten when RPCS3 leaves what ships, so R.1 and R.2 both have a shelf life.
  - R.1's lesson carries over regardless. The 86" TV crops its edges, so `guacamole`'s TV surface (the figure cards from AA on) needs the same title-safe margin from day one.
