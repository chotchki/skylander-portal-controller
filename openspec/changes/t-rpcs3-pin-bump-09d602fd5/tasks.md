# Tasks

## T. RPCS3 pin bump (927e2492e → 09d602fd5; drop SPU patches → clean P1–P8)

- [x] T.1 Rebase P1–P8 onto pin `09d602fd5` (drop SPU 0004/0005 — now upstream #18935); P8 cherry-picked past a local CRLF quirk on `swapchain_macos.hpp`
- [x] T.2 Regenerate `rpcs3-patches/` (clean P1–P8, 8 patches) + reset the gitlink to pristine `09d602fd5`
- [x] T.3 Bump the pin docs (rpcs3-patches/README + patch-list, research strategy, release.yml download tag, memory)
- [x] T.4 Fix `rpcs3-patched.yml` apply-clean allowlist → P1–P8 file set (LC_ALL=C deterministic sort)
- [x] T.5 Commit the bump + trigger the gated `rpcs3-patched.yml` full build (= R.3) → `rpcs3-patched-09d602fd5`
- [x] T.6 Verify the build green (compile proof) + the bundled prerelease published — `rpcs3-patched-09d602fd5` (53.7MB) up; needed apply.sh CRLF normalization (3 iters) + a pin-from-gitlink fix
- [ ] T.7 Cut v1.9.13 — fully green (mac signing + Windows bundled-RPCS3 download)
