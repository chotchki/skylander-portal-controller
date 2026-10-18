# Phase U - Ship the patched RPCS3 emulator in the macOS release (mock → ipc)

## Why

The mac .dmg currently bundles MOCK only, so "launch game" can't boot a real emulator —
wasting the Phase 16 IPC + P8 surface-embed work (live-validated ~60fps on the M3 Max via
`.ci-local/build-mac.sh`). This phase packages that proven runtime into the release: bundle
+ sign the patched RPCS3, flip the mac default mock → ipc, mirroring the Windows v1
distribution model (16.9.0b). The runtime is done — this is distribution plumbing.
Reconnaissance: the Explore done|missing|blocker trace (2026-06-26). Windows is the template
throughout (`release.yml` Windows job + `config.rs::migrate_install_paths`).

## What Changes

- Shipped, live since v1.9.19 (2026-06-27, release run 28271769860 green on both lanes). That's the first macOS release with the emulator nested: a 102 MiB dmg next to a 25 MiB tarball that doesn't carry it.
  - U.1: the mock `BootDirect` reveal (`no_real_process`, `crates/server/src/state.rs:1754`) and its test `boot_direct_mock_reveals_game_playable` (`:2132`), `87e9b4b`.
  - U.2.1, U.2.2, U.2.4: `rpcs3-patched.yml`'s `build-macos` job runs `.ci-local/build-mac.sh` on `macos-15` (`:269-272`, `:305`). Its relocate pass (`build-mac.sh:144-174`) rewrites the Homebrew install-names, and the smoke gate (`rpcs3-patched.yml:312-319`) fails on a `--version` crash or on any Mach-O still pointing at `/opt/homebrew` or `/usr/local`. Dispatch 28265316693 (`172bd0a`, 2026-06-26) built it green and published the arm64 tarball + `.sha256` to `rpcs3-patched-09d602fd5`. The tag comes from the gitlink (`:290`), and both lanes tolerate a `gh release create` race (`:244-253`, `:349-355`).
  - U.3: `release.yml:507-534` downloads and sha-verifies the tarball, then hands it to `tools/build-macos-app.sh` as `RPCS3_APP_SRC`, which nests it at `Contents/Resources/rpcs3/RPCS3.app` (`:112-122`).
  - U.4: inside-out Developer ID signing with no `--deep`, and JIT entitlements on the rpcs3 binary (`build-macos-app.sh:162-247`). v1.9.19 notarized and stapled on its first run (no failed release run between v1.9.18 and it).
  - U.5: macOS recomputes the driver and control binary every launch (`crates/server/src/config.rs:377`, `migrate_install_paths_macos` at `:485`). It picks IPC whenever `paths::bundled_rpcs3_exe()` (`paths.rs:47`) exists, and Mock only on `SKYLANDER_PORTAL_DRIVER=mock` or a missing emulator. U.5.4's `paths::app_data_root()` (`paths.rs:79`) is the art fix. U.5.3 still holds: both release lanes build `--features sky-stats,mock-driver-runtime` (`release.yml:105`, `:463`), and IPC isn't feature-gated.
  - U.6: the wizard runs on macOS and validates a config dir (`crates/server/src/wizard.rs:55`) with a best-guess prefill (`:160`), `8470670`.
  - U.9: relative `games.yml` entries anchor to the config root (`crates/rpcs3-control/src/games_yml.rs:45`, `:53`, tests `:136` and `:163`). `609b7ab` shipped in v1.9.21.
- U.4.1 is ticked, but one clause of it never shipped. It asks to "staple the `.app` (not just the `.dmg`)", and `build-macos-app.sh:249` still carries `TODO(U.4.3)` deferring exactly that (notarize the .app, staple it, then build the dmg from it). `8470670`'s message says ".app stapling = U.4.3, deferred", but no U.4.3 box was ever written. The effect is standard notarization behavior and untested here: the ticket stays stapled to the dmg, so a `.app` dragged to /Applications verifies only if Gatekeeper can reach Apple on its first launch.
- U.2.3 (OPEN, and U.2 is open only because of it). The method never happened: `crates/rpcs3-control/tests/live_launch.rs` hasn't changed since `eaeb958` (2026-06-23). The intent did happen.
  - chotchki's installed v1.9.19 booted Giants (BLUS30968) on the dev Mac through the nested emulator. It reached "rpcs3 game playable (IPC STATE: compile complete + frames advancing)" twice on 2026-06-27 (00:43:33Z and 01:01:58Z). The record is that install's `server.log.2026-06-27`. It won't rotate out (the logger has no retention yet, `crates/server/src/logging.rs:1-3`, and a `server.log.2026-05-03` still sits beside it), but it lives on one Mac, outside the repo.
  - That nested binary IS the published `rpcs3-patched-09d602fd5` arm64 tarball.
  - The published artifact has since moved to `rpcs3-patched-accfecd2d`, and nothing has booted that one yet (V.8).
- U.7 (OPEN, chotchki's hardware): half-run, and not on a clean Mac.
  - The same v1.9.19 install did the back half on the dev Mac: the IPC driver, Giants playable and the P8 composite (`CompositorHost: CALayerHost attached` at 01:01:49Z). It also found a bug: `1aa187b` ("fix(U.7): macOS surface-embed runs in BOTH window modes, not Desktop-only") shipped in v1.9.20.
  - Not covered:
    - A clean Mac. This one already had firmware under `~/Library/Application Support/rpcs3` and a config.json.
    - The Gatekeeper first-open. No log can show it; CI's `spctl -a` passes (`release.yml:651`).
    - Figures. That run had no pack (`pack=`, `indexed pack figures count=0`), so no LOAD reached the portal.
  - Outside evidence: issue #8 (2026-09-25) is an M5 Mac user who got past install and the wizard, with a config dir loading `games.yml` ("loaded RPCS3 games.yml entries=3"). What stopped them was the game itself, not signing or Gatekeeper: the PAL serial (BLES01689) isn't in our catalogue ("installed=0"), and relabelled as BLUS30968 its `.iso` path got an HTTP 404, because the launch handler looks for an extracted `PS3_GAME/USRDIR/EBOOT.BIN` (`crates/server/src/http.rs:1209-1220`). Which release they ran is unverified. No U box owns PAL serials or ISO dumps (issue #7 asks for non-US support too).
  - The artifact to install is now v1.10.0's dmg. One clean-Mac run of it covers U.7 and V.8's macOS half together.
- U.8 (OPEN, about half done).
  - Done:
    - CLAUDE.md "macOS support" and "Distribution" already say the Mac release bundles the patched RPCS3 and defaults to IPC, with mock as the demo override (`e34c4b9` corrected the last stale notes, 2026-06-28).
    - The gotchas are banked in memory (`project_macos_signed_release_ipc`, 2026-06-28): the per-launch driver recompute, the nested path, the JIT entitlements and the config-dir no-op.
  - Not done:
    - `docs/dev/release-signing.md` has no nested-app note. It never mentions the nested RPCS3, the entitlements or the inside-out order, and was last touched in `98dbdda` (2026-06-25, before U).
    - `docs/dev/macos-rpcs3-build.md` never mentions the CI lane (last touched `6affbac`, 2026-06-01).
  - A gap the box doesn't name: `docs/dev/ci.md`'s lane table.
    - It has no `rpcs3-patched.yml` lanes at all.
    - It lists `macos-14` for jobs that run on `macos-15` (`ci.yml:150`, `release.yml:402`).
    - It describes `build-macos-release` as "`cargo build --release` + tar.gz". That job now also nests, signs, notarizes and staples.
- U.10 (OPEN, not started).
  - Still true at the `accfecd2d` pin: `fs::get_config_dir()` reads `RPCS3_CONFIG_DIR` only under `_WIN32` (`vendor/rpcs3/Utilities/File.cpp:2520`), and macOS hardcodes `$HOME/Library/Application Support/rpcs3/` (`:2562-2574`).
  - `crates/rpcs3-control/src/process_unix.rs:91` sets the variable anyway, so on a Mac it's a no-op.
  - The wizard accepts any existing directory (`wizard.rs:55-63`). A user can pick a config dir whose `games.yml` our code reads while RPCS3 boots without the firmware.
  - No P9 exists; the series is still P1-P8. A P9 touches `Utilities/File.cpp`, which isn't in the apply-clean allowlist (`rpcs3-patched.yml:88`), so the allowlist moves with it.
  - Upstream's `portable/` escape (`File.cpp:2511`) doesn't help. On macOS it looks beside `RPCS3.app` (`get_executable_path` climbs three levels out of the binary, `:2448-2449`), which in our nesting is `Contents/Resources/rpcs3/`, inside the signed launcher bundle.
  - Cheaper interim (proposed): have the macOS wizard warn when the chosen dir isn't `~/Library/Application Support/rpcs3`, the only one RPCS3 honors today. U.9 already made `games.yml` portable; only RPCS3's own firmware and save lookup is location-locked.
- U then reduces to U.2.3, U.7, U.8 and U.10, plus the unbooked U.4.3 (.app stapling). U.2.3 and U.7 now both wait on the same hardware run of the `accfecd2d` artifact that V.8 needs.

## Capabilities

### New Capabilities

None declared yet. This change came over from PLAN.md on 2026-10-17 as a task list, not a spec, so `.openspec.yaml` carries `skip_specs: true`. U.10 is the box that changes observable behavior (where the Mac app's emulator finds firmware and saves). When it starts, name the capability here, write its delta under `specs/` and drop the flag.

### Modified Capabilities

None yet.

## Impact

- Code and config, by box:
  - U.2.3: `crates/rpcs3-control/tests/live_launch.rs` (`#[ignore]`d; reads `RPCS3_EXE`, `RPCS3_EBOOT` and `RPCS3_CONFIG_DIR`).
  - U.4.3 (unbooked): `tools/build-macos-app.sh:249`, and the build/notarize/staple steps in `release.yml` (`:591-651`), reordered to notarize and staple the `.app` before `hdiutil` packs it.
  - U.8: `docs/dev/release-signing.md`, `docs/dev/macos-rpcs3-build.md`, `docs/dev/ci.md`.
  - U.10:
    - A new `rpcs3-patches/0009-*.patch` against `Utilities/File.cpp`.
    - The allowlist at `rpcs3-patched.yml:88`.
    - The series list in `rpcs3-patches/README.md`.
    - For the interim, `crates/server/src/wizard.rs`.
- Tests: U.10's patch is C++, tested from Rust like P1-P8. That means an `#[ignore]`d live run on the Mac, launched with a non-default `RPCS3_CONFIG_DIR`. No CI lane can boot a game.
- Owning docs: CLAUDE.md "macOS support" and "Distribution", `docs/dev/release-signing.md` (the signing runbook), `docs/dev/macos-rpcs3-build.md` (the Mac emulator build) and `rpcs3-patches/README.md` (the P-series, for U.10).
- Track: product. Release reach:
  - A `v*` tag publishes the signed, notarized dmg with the emulator nested and IPC by default. It ALSO publishes an unsigned macOS tarball with no emulator (`release.yml:466-486`), and anyone who takes that tarball gets Mock.
  - U.10 reaches users only through a new patched build plus a release that downloads it.
- One-way doors:
  - Shipped since v1.9.19: every Mac launch auto-promotes a persisted Mock config to IPC (`config.rs:377`). The only way back is `SKYLANDER_PORTAL_DRIVER=mock`.
  - `rpcs3-patched-<pin>` is keyed by the pin alone, and both lanes upload with `--clobber` (`rpcs3-patched.yml:256`, `:355`).
    - A P9 built at `accfecd2d` would replace, under the same tag, the binary v1.10.0 shipped, so a re-run of v1.10.0's build would bundle P9.
    - It has happened once already (harmlessly). U.2.1's dispatch re-uploaded the `09d602fd5` Windows zip at 22:07Z on 2026-06-26, so v1.9.13-v1.9.18 and v1.9.19-v1.9.21 bundle two different builds of the same series.
    - Proposed: name the series in the tag (`rpcs3-patched-<pin>-p9`), or bump the pin along with the patch.
  - Guacamole's G1 takes the bundled RPCS3 out of what ships, which gives U.10 and U.4.3 a shelf life. The patched RPCS3 stays on as the dev oracle until G2, but U.10 only matters to the shipped app.
- External:
  - Apple notarization and Gatekeeper.
  - The Developer ID cert and app-specific password (Phase 14 owns rotation).
  - GitHub's `macos-15` image.
  - The Homebrew Qt6 and llvm@21 that `build-mac.sh` builds against.
