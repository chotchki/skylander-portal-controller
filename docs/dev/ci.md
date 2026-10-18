# CI

What runs in `.github/workflows/`, why, and on which runners.

## Lanes

| Lane | Workflow | Runner | What it covers | Triggers |
|---|---|---|---|---|
| `fmt` | `ci.yml` | `ubuntu-latest` | `cargo fmt --check` (workspace + phone) | every PR + main |
| `clippy-build-test` (windows) | `ci.yml` | `windows-latest` | `cargo clippy -- -D warnings`, `cargo build --workspace --all-features`, `cargo test --workspace` | every PR + main |
| `clippy-build-test` (macos) | `ci.yml` | `macos-14` | same as Windows; cross-OS coverage of the workspace under Apple Silicon | every PR + main |
| `phone-wasm` | `ci.yml` | `ubuntu-latest` | `trunk build --release` for the phone SPA | every PR + main |
| `e2e-mock-macos` | `ci.yml` | `macos-14` | mock-driver chromedriver e2e tests (currently `tests/smoke.rs` only — see 10.3.6) | every PR + main |
| `e2e-ios-sim` | `ci.yml` | `macos-14` | iOS-Simulator-driven e2e (PLAN 10.4) | label-gated (`run-ios-sim`) + manual dispatch |
| `build-windows-release` | `release.yml` | `windows-latest` | `cargo build --release` + zip | tag push + manual dispatch |
| `build-macos-release` | `release.yml` | `macos-14` | `cargo build --release` + tar.gz | tag push + manual dispatch |

## Host choice

GitHub-hosted `macos-14` (Apple Silicon) is the default for everything Mac-side, including the iOS-Simulator e2e lane (PLAN 10.5.1).

**Why GH-hosted over self-hosted:**
- Free for public repos (within the macOS minute budget; macOS minutes are 10× the cost of Linux but the runner is fast and the Phase 10 lanes are short).
- Xcode + iOS runtime + Homebrew all pre-installed; no provisioning headache.
- No machine to keep awake.
- One vendor's outage instead of two.

**Tradeoffs accepted:**
- Cold simulator boot is ~30–60 s on a fresh runner (no warm sim cache between runs). The iOS-sim lane is label-gated specifically because of this; running it on every PR would dominate CI wall-clock.
- macOS minutes are budgeted; if the lane usage grows past the free tier we revisit.

**When to switch to self-hosted:**
- iOS-sim lane runs more than once per PR on average (warm cache becomes worth it).
- A workflow needs persistent dev state (e.g. real RPCS3 install for live tests — currently developer-machine-only per CLAUDE.md).
- Free tier exhausts and the team prefers fixed-cost infra to per-minute billing.

## iOS-sim lane gating

The iOS-Simulator e2e suite (`tests/ios_*.rs`) is the most expensive lane: cold-boot of one or two simulators + Safari + WS handshake adds 20–70 s per test, on top of the `cargo build` cost. Running it on every PR is overkill; it gates only when:

1. **Label `run-ios-sim` applied to the PR**, or
2. **Manual `workflow_dispatch`** from the Actions UI.

Skipped by default on `push` to `main` (the assumption is reviewers add the label when a PR touches phone/iOS-sensitive code; if it slips through and breaks main, the next phone-touching PR catches it).

PRs whose diff touches `phone/`, `crates/e2e-tests/`, or `crates/guac-server/src/http.rs` are flagged in the bot reminder (TBD — for now, reviewer judgement).

## Pre-push hook (opt-in)

`.githooks/pre-push` runs `cargo fmt --all --check` (workspace + phone), `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` locally before push, catching the `fmt` lane's failures and the `clippy-build-test` lane's clippy and test failures for the OS it runs on. Two gaps: it never builds `--all-features`, so code behind `test-hooks` compiles only in the lane's build step (and in the e2e harness, which runs with `--features test-hooks`), and a Mac never compiles `cfg(windows)` code. With the lanes triggered as in the table above, no lane runs on a `2.0` push, so today the e2e runs are what build `test-hooks` and nothing builds `cfg(windows)` until the Windows/HTPC bring-up's lane (see the caveats below). Opt-in:

```sh
git config core.hooksPath .githooks
```

The hook is `.sh`-based and runs against whatever cargo / rustup / system Rust is on the developer's PATH. To skip it for a specific push (e.g. WIP branch):

```sh
git push --no-verify
```

(`--no-verify` bypasses **all** hooks, not just pre-push — use sparingly.)

### Bevy's cost (openspec AA.10)

`crates/guacamole` puts Bevy 0.20 in every `--workspace` step, so the hook compiles it: `cargo check` builds the binary's `ui` feature collection only, while `cargo clippy --all-targets` and `cargo test` also build the `hero_view` example (openspec AC.10) and its dev-dependency Bevy features (`bevy_pbr`, `bevy_light`, `ktx2`, `tonemapping_luts`, `zstd_rust`; [`crates/guacamole/Cargo.toml`](../../crates/guacamole/Cargo.toml)). The numbers below predate that: measured on the M3 Max on 2026-10-18 on the `aa8-tv` branch before AC.10 merged into it, so with the `ui` features alone and no example. Default profile, the hook's steps with vs without `--exclude guacamole`:

| Run | With guacamole | Without | Delta |
|---|---|---|---|
| cold, first sample (check / clippy / test) | 106 s (27 / 18 / 61) | 71 s (28 / 5 / 37) | +35 s |
| cold, second sample | 101 s (27 / 18 / 55) | 59 s (20 / 4 / 35) | +42 s |
| warm no-op | 18 s | 19 s | +0 (the test binaries running) |
| warm, guac-server's `lib.rs` touched | 27 s | 24 s | +3 s |
| target dir | 11 GB | 5.4 GB | +5.6 GB |

Caveats: one sample per row, a fresh `CARGO_TARGET_DIR` for the cold rows, other builds sharing the CPU (load 5-23; a third cold run at load 20-54 took 163 s), so read the cold delta as +35 to +42 s at best and load-bound. The second sample's test step exited 101 at load 18 and the failure didn't come back in 3 warm runs and 1 cold run with output kept; if it recurs, keep the output so it names the test. The Windows lane's delta isn't measured; that's the Windows/HTPC bring-up's ([`openspec/backlog.md`](../../openspec/backlog.md)). If the hook gets painful, the fix is its own change, not a quiet skip of the guacamole crate.

The warm rows are only meaningful since AA.10 fixed [`crates/guac-server/build.rs`](../../crates/guac-server/build.rs) and [`phone/build.rs`](../../phone/build.rs) to resolve HEAD and the index through `git rev-parse --git-path`: in a linked worktree `.git` is a file, the old literal `.git/HEAD` path never existed and cargo reran the build script, and rebuilt everything downstream, on every build.
