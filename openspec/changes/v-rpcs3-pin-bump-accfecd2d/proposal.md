# Phase V - RPCS3 pin bump (09d602fd5 -> accfecd2d; 3-month upstream catch-up)

## Why

Pin went stale over the summer: `09d602fd5` is 2026-06-24, upstream master is **516 commits**
ahead at `accfecd2d` (2026-09-20, past the `v0.0.42` tag). Churn on the P-patch seams is
almost nil (1 commit on `Skylander.cpp`, 5 on `gs_frame.cpp`, 3 on `sys_usbd.cpp`, 0 on
`pad_thread`) so the patch-depth thesis holds — 516 commits and only 2 hunks needed hands
(see V.1). Mirrors Phase T.

## What Changes

- Landed and published on 2026-09-20. Every claim below was re-checked against the repo and gh today.
  - V.1/V.2:
    - `28b0bc7` regenerated `rpcs3-patches/0001`-`0008` and left the gitlink at a pristine `accfecd2d` (`git ls-tree HEAD vendor/rpcs3`).
    - Both hand-resolved hunks are in the patches. P1 passes the array to `load_skylander(const std::array<u8, 0x40 * 0x10>&, …)` (`0001-P1-…patch:79`, `:225`). P5's `register_ldd_and_connect` sits after upstream's new `set_usb_device_attached` (`0005-P5-…patch:50-106`).
    - The preamble's churn counts reproduce: `git rev-list --count 09d602fd5..accfecd2d` per file gives 1, 5, 3 and 0.
  - V.3: `release.yml:117` and `:513` name `rpcs3-patched-accfecd2d`. `rpcs3-patches/README.md:100` and `docs/research/rpcs3-integration-strategy.md:10` carry the pin.
  - V.4: the allowlist is the same 12 files (`rpcs3-patched.yml:88`).
  - V.5/V.11: `apply.sh` stages every series-touched file and commits only when the index moved, never `commit -a` (`rpcs3-patches/apply.sh:56-74`, `1aaea09`). `build-mac.sh`'s already-patched probe is at `:45-53`.
  - V.6: `4715512` cleared the rust 1.98 clippy drift, and `ipc_loopback.rs` holds the 12 tests the box counts.
  - V.7: push run 35535742626 (apply-clean, green on `28b0bc7`), then three dispatches:
    - 35536851380: Windows died at "Apply patch series" (V.11), macOS cancelled.
    - 35537953773: macOS green and published, Windows red.
    - 35539244118: Windows only, green.

    The prerelease carries all four assets: the Windows zip (59.3 MB), the macOS tarball (66.3 MB) and two `.sha256`. Each has exactly one download, which is the v1.10.0 release run pulling it.
  - V.10: `cfg_attr(windows, allow(dead_code))` on the four macOS-only fields (`crates/server/src/ui/mod.rs:156-177`, `0286acf`). The box undercounts the red, though.
    - `ci.yml`'s Windows job failed on every push from `633206a` (2026-06-24, P8's launcher-pane commit; `ba3a703` was the last green) through `609b7ab` (2026-06-28), and again on `28b0bc7`. That's 41 failed runs in June (two more were cancelled), so all of T and U landed on a red CI.
    - Every one of the 42 failures died at the `cargo clippy --workspace --all-targets` step (the job step records survive). The June logs have expired (HTTP 410), so which lint is unverified; the same dead code is the likely one.
    - CI has passed on every push since `1aaea09`; the latest is 35545845388 on `bcc1bce`.
  - V.12/V.13:
    - `LLVM_VER` 22.1.8 and `QT_VER` 6.11.2 (`rpcs3-patched.yml:115-118`).
    - The drift guard (`:137-166`) and its README step 3b (`rpcs3-patches/README.md:158`).
    - The `targets` input (`rpcs3-patched.yml:39-46`).
  - V.9: both memory notes carry the bump (`project_rpcs3_ipc_fork` and items 4-7 of `feedback_rpcs3_patched_build_ci`, all dated 2026-09-20).
- V.14.1-V.14.3 shipped `v1.10.0` (2026-09-20) as a prerelease.
  - The tag points at `e9e79e2`, whose CI run (35543184235) passed. Release run 35543190949 was green on both lanes and published the zip (117 MB), the MSI (103 MB), the dmg (104 MB) and the tarball (25.9 MB).
  - It's still a prerelease today, and `/releases/latest` still resolves to v1.9.21. V.14.3's aside about `gh release list` is stale: it now prints "Pre-release" beside v1.10.0 and "Latest" beside v1.9.21.
  - It reaches people anyway. The Releases page lists it first, and so far the MSI and the dmg have 3 downloads each (the zip and the tarball have 0).
- V.8 (OPEN, chotchki's hardware). Nothing shows a boot of the `accfecd2d` binary anywhere.
  - The `rpcs3-patched-accfecd2d` assets have one download each, all from the release run.
  - On the dev Mac the installed app is still v1.9.19, its release-mode log stops at 2026-06-27 and the dev `logs/` stop at 2026-06-29. RPCS3's own log there holds only the `0.0.42-local_build` header from V.5's `--version` check (2026-09-20), so the local build never booted a game either.
  - Whether one of v1.10.0's MSI or dmg downloads was chotchki starting V.8 is unverified.
  - The box mixes up two machines. Its artifact is the Windows zip or MSI, but it asks for the P8 surface composite, which is macOS only (`0008-P8-macOS-surface-embed…`). On Windows the launcher places the game window itself with `SetWindowPos` on the handle P2 publishes. So V.8 is really two runs:
    - The HTPC: IPC, LOAD/CLEAR and window placement.
    - A Mac on the v1.10.0 dmg: the P8 composite. That run also closes U.7.
  - A plain boot never exercises one of the two hunks the box says to suspect.
    - P5's `register_ldd_and_connect` runs only from the `RECONNECT` handler (`0005-P5-…patch:124`). The server sends `RECONNECT` only after a save-state boot (`crates/server/src/state.rs:1644-1652`, gated on `SKYLANDER_BOOT_SAVESTATE` at `:1588`), and kid play never does one. The only `RECONNECT` test is the loopback fake (`ipc_loopback.rs:393`).
    - The collision itself looks textual. Upstream's new `set_usb_device_attached` (`rpcs3/Emu/Cell/lv2/sys_usbd.cpp:1035` at the pin, added in upstream `1b3e02044`) wraps the same `connect_usb_device`/`disconnect_usb_device` pair that P5's `reconnect_device` calls, and its only caller is the G27 wheel (`LogitechG27.cpp:355`).
    - So clearing P5 takes one save-state boot, or an explicit decision that kids can't reach it. P1's hunk is on every LOAD.
  - The `#[ignore]`d lanes are the procedure, pointed at the extracted v1.10.0 `rpcs3/`: `live_launch.rs` (`no_gui_launch_reaches_playable`, `:60`) and `live_ipc.rs` (`patched_rpcs3_ipc_load_clear_state`, `:38`). No doc writes that down: the README's rebase step 4 says "Rebuild + smoke-test (tools/rpcs3-ipc/)" before committing, and its Tests section names only `live_ipc.rs`, neither against the published artifact.
- V.14.4 (OPEN) waits on V.8, and it reaches further than the box says.
  - Our winget auto-PR is still off (`release.yml:388`, `if: false`; owned by 14.6.4), yet winget has 1.9.21. A third-party bot filed it (microsoft/winget-pkgs#418590, 2026-08-16, seven weeks after the release).
  - The README sends Windows users to winget first, and v1.9.21's MSI has 232 downloads.
  - So promoting v1.10.0 to Latest likely puts the new emulator on winget without any PR from us.
  - That the bot skips prereleases is inferred, not verified: it hasn't filed 1.10.0 in four weeks, but it took seven for 1.9.21.
- Stale text from V.3 (`28b0bc7`):
  - `rpcs3-patches/README.md:25` gives the history as "`c11979d` -> `09d602fd5` (109 commits)", which drops the `927e2492e` hop (`8db5663`, 2026-06-22). By `git rev-list --count`, `c11979d..927e2492e` is 109 and `927e2492e..09d602fd5` is 4.
  - `docs/dev/rpcs3-fork-htpc-bringup.md:18` says the pin moved "three times" and lists two hops.
- V then reduces to V.8 (two runs) and V.14.4.

## Capabilities

### New Capabilities

None. This change came over from PLAN.md on 2026-10-17 as a task list, and a pin bump plus its release changes no requirement, so `.openspec.yaml` carries `skip_specs: true`.

### Modified Capabilities

None.

## Impact

- Code and config: nothing left to write unless V.8 fails.
  - On a failure, fix `0001` (the P1 call site) or `0005` (P5's `sys_usbd`) and regenerate per the README's rebase procedure.
  - Then run a gated build. It re-uploads under the same tag with `--clobber` (`rpcs3-patched.yml:256`, `:355`).
  - Then fix forward with a v1.10.x release.
- Tests: `crates/rpcs3-control/tests/live_launch.rs` and `live_ipc.rs`, `#[ignore]`d and run by hand on the HTPC and on a Mac. `ipc_loopback.rs` is CI's half of the IPC contract, and it passes.
- Owning docs:
  - `rpcs3-patches/README.md`: the pin's one home, plus the rebase checklist including step 3b.
  - `docs/research/rpcs3-integration-strategy.md`.
  - `docs/dev/rpcs3-fork-htpc-bringup.md`: the HTPC toolchain, not validation. No doc owns the post-bump live check; extending the README's rebase step 4 (today a pre-commit smoke test via `tools/rpcs3-ipc/`) is the natural home.
- Track: product. Release reach: v1.10.0 is already public as a prerelease. V.14.4 moves only the Latest pointer, and through it (likely) winget.
- One-way doors:
  - Promotion is the only one left. It likely reaches winget through a bot we don't control, and a pushed tag is never deleted, so a bad emulator means a v1.10.1, not a rollback.
  - The pin is already 304 commits behind upstream master (`gh api …/compare/accfecd2d...master`, today), past `v0.0.43` (2026-09-30). Bumping again before V.8 would restart validation, so don't.
- Guacamole: the patched RPCS3 at this pin is also Guacamole's development oracle until G2. V.8's two runs are the first proof that the oracle boots Giants at `accfecd2d`, on the two machines the oracle work uses (the AE rung in `docs/native/guacamole-design.md` measures run-to-run divergence on both the Mac and the HTPC).
- External:
  - Upstream RPCS3 master.
  - The LLVM 22.1.8 and Qt 6.11.2 that upstream's Windows build pins.
  - GitHub's `windows-2025` and `macos-15` images.
  - microsoft/winget-pkgs and the bot that files there.
