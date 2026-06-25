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

PRs whose diff touches `phone/`, `crates/e2e-tests/`, or `crates/server/src/http.rs` are flagged in the bot reminder (TBD — for now, reviewer judgement).

## Pre-push hook (opt-in)

`.githooks/pre-push` runs `cargo check --workspace` + `cargo test --workspace` locally before push, catching everything the `clippy-build-test` lane would catch on either OS. Opt-in:

```sh
git config core.hooksPath .githooks
```

The hook is `.sh`-based and runs against whatever cargo / rustup / system Rust is on the developer's PATH. To skip it for a specific push (e.g. WIP branch):

```sh
git push --no-verify
```

(`--no-verify` bypasses **all** hooks, not just pre-push — use sparingly.)
