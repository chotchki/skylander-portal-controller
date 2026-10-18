# Phase T - RPCS3 pin bump (927e2492e → 09d602fd5; drop SPU patches → clean P1–P8)

## Why

Unblocks the Windows side of the v1.9.13 signed release (was R.3). New pin = latest master
09d602fd5 (2026-06-24): includes the SPU-Giga fix (#18935, replaces local 0004/0005) + 2
SPU-LLVM perf commits. The 4 new commits are all SPU-only, orthogonal to the P-patch seams.

## What Changes

- T.1-T.6 landed on 2026-06-25/26, and V has since replaced the parts that were pin-specific.
  - T.1/T.2: `b554716` rebased onto `09d602fd5` and dropped the two local SPU-Giga patches, leaving a clean P1-P8. The preamble's count checks out: `git rev-list --count 927e2492e..09d602fd5` is 4, all SPU (two SPU Analyzer fixes, `064365d6f` and `42242b3c4`, plus two SPU LLVM commits, `87608865b` and `09d602fd5`).
  - The pin moved on. V's `28b0bc7` (2026-09-20) re-pinned to `accfecd2d`, so T.2's gitlink and T.3's `release.yml` tag are history. T.3's doc edits survive as bump history (`rpcs3-patches/README.md:23`, `docs/research/rpcs3-integration-strategy.md:14-16`).
  - T.4: the allowlist is still sorted with `LC_ALL=C` (`rpcs3-patched.yml:79-88`), and V.4 reused it unchanged.
  - T.5/T.6:
    - Three gated dispatches died at "Apply patch series" on CRLF. 28215538883 failed it on every leg (upstream stores `swapchain_macos.hpp` CRLF, and P8 touches it); 28216573271 and 28217624467 failed it on Windows only. `2cb5990`, `a0a832a` and `efd8745` fixed it.
    - The fourth (28218914252) built Windows green but published it as `rpcs3-patched-9ce9d76`. That was the HEAD~<n> bug: 9ce9d76 is apply.sh's CI-local normalization commit, in neither `vendor/rpcs3` nor upstream (gh: "No commit found").
    - `4453907` switched to the gitlink, and dispatch 28221185265 created the real `rpcs3-patched-09d602fd5` at 07:18Z on 2026-06-26. Its macOS leg failed. That leg still ran upstream's recipe, which U.2 replaced, and its logs have expired.
  - Two of T's published artifacts now say the wrong thing:
    - `rpcs3-patched-9ce9d76` is still up (0 downloads) under the title "Patched RPCS3 (Windows) — 9ce9d76 + P1/P2 IPC patches".
    - `rpcs3-patched-09d602fd5` is titled "Patched RPCS3 (Windows) — 09d602fd5 + P1/P2 IPC patches", though it carries U.2.2's macOS tarball and P1-P8. The workflow sets the right title (`rpcs3-patched.yml:247`, `:351`), but only on create.
  - T.6's Windows zip was replaced. U.2.1's dispatch (28265316693) rebuilt both legs and re-uploaded the zip with `--clobber` at 22:07Z on 2026-06-26. So v1.9.13-v1.9.18 bundle T.6's build and v1.9.19-v1.9.21 bundle U.2.1's: the same pin and series, but two builds.
- T.7 (OPEN) is superseded. It can't close as written, and its intent was met.
  - v1.9.13 was cut (2026-06-26, `5687a22`), but release run 28238672102 went green on Windows and red on macOS. The release carries only the Windows zip and MSI.
  - Its Windows side did ship. At that tag `release.yml` bundles `rpcs3-patched-09d602fd5`, the build reached winget (microsoft/winget-pkgs#393437, 2026-06-26) and that MSI has 262 downloads.
  - The macOS lane went green at v1.9.18 (run 28253574658, 2026-06-26). That's the first fully green release on this pin: a signed, notarized dmg AND the Windows bundle download. v1.9.14-v1.9.17 were Phase 14's fix-forward patches, and Windows-only too.
  - v1.9.19-v1.9.21 shipped the same pin, and v1.10.0 (2026-09-20) moved past it.
  - Tags are never deleted, so "v1.9.13, fully green" can never become true. Suggest ticking T.7 as "landed as v1.9.18" and archiving T: every other box is ticked, and the work reached users in v1.9.13 and later.
- T's "green" means the release and `rpcs3-patched.yml` lanes, not `ci.yml`. The Windows CI job failed at its clippy step on every push from `633206a` (2026-06-24) through `609b7ab`, which covers all of T, until V.10 fixed it.
- `openspec/backlog.md:6` ("Bump RPCS3 pin past #18935 → drop the 2 local SPU-Giga patches", carried from PLAN.md) is T.1, and it's done.

## Capabilities

### New Capabilities

None. This change came over from PLAN.md on 2026-10-17 as a task list, and a pin bump changes no requirement, so `.openspec.yaml` carries `skip_specs: true`.

### Modified Capabilities

None.

## Impact

- Code and config: nothing left to write. Closing out touches only this folder and the backlog line, plus two optional `gh release edit` title fixes. Under the no-delete-tags rule, `9ce9d76` gets retitled to point at `09d602fd5`, not deleted.
- Tests: none left. T's proof is the gated build and release runs above.
- Owning docs: `rpcs3-patches/README.md` (the pin and the series) and `docs/research/rpcs3-integration-strategy.md`.
- Track: product. Release reach: done. v1.9.13-v1.9.17 (Windows only) and v1.9.18-v1.9.21 (both platforms) shipped this pin, nothing new ships from T, and v1.10.0 carries V's pin.
- One-way doors: none open. `rpcs3-patched-09d602fd5` has to stay, because every release from v1.9.13 to v1.9.21 downloads from it in `release.yml`, and a re-run without it fails.
- External: none.
