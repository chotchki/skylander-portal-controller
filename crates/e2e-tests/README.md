# E2E tests

Integration tests that drive the full stack (server + phone SPA) via a
real browser (fantoccini → chromedriver → Chrome). Tests use the mock
`PortalDriver` with failure injection via `/api/_test/inject_load`, so
they do not require RPCS3 to be running. Runs cleanly on Windows and
macOS — the mock-driver path is fully cross-platform (PLAN 10.3).

## One-time setup

1. Install **Chrome (stable channel)**.
2. Install **ChromeDriver matching your Chrome version**, on PATH:
   - **macOS:** `brew install --cask chromedriver` (Apple Silicon
     installs to `/opt/homebrew/bin`, Intel to `/usr/local/bin`).
   - **Windows:** `winget install --id=Chromium.ChromeDriver`, or grab
     a matching build from
     <https://googlechromelabs.github.io/chrome-for-testing/>.
3. Build the **phone SPA** at least once so `phone/dist/` exists.
   **Always pass `BUILD_TOKEN=e2e-test`** so the bundle's stale-
   version check matches what the test harness pins on the spawned
   server (otherwise the phone's `<git-hash>-dirty` and the
   server's drift apart whenever you edit anything between builds,
   raising a StaleVersion overlay mid-test):
   ```
   cd phone && BUILD_TOKEN=e2e-test trunk build
   ```
4. Provide a **firmware pack** with `.sky` files. The harness needs a
   real pack so the SPA renders one card per indexed figure.
   Resolution order in `TestServer::spawn`:
   1. `$SKYLANDER_PACK_ROOT` (explicit override).
   2. `<repo>/dev-data/firmware-pack/` (standard contributor layout).
   3. `C:\Users\chris\workspace\Skylanders Characters Pack for RPCS3`
      (Chris's HTPC; kept for back-compat).

The harness spawns its own chromedriver on a free port per
`TestServer` and kills it on drop — no need to run `chromedriver`
manually.

## Version-mismatch gotcha

Chrome and ChromeDriver must be the same major version. If the suite
errors with `session not created: This version of ChromeDriver only
supports Chrome version N`, either:
- **Update Chrome** to match (open Chrome → menu → About Google
  Chrome → it auto-downloads → relaunch), or
- **Pin a specific ChromeDriver** by setting `$CHROMEDRIVER` to a
  matching binary (e.g. one from
  <https://googlechromelabs.github.io/chrome-for-testing/>).

`brew upgrade chromedriver` may run ahead of stable Chrome; if you
hit this often, prefer the chrome-for-testing matched pair.

## Running

```
cargo test -p skylander-e2e-tests -- --ignored --nocapture
```

All tests are `#[ignore]`-gated so they don't run under the default
`cargo test --workspace`. The harness spawns the server binary per
test via `cargo run -p skylander-server --features test-hooks` with a
temp working directory containing a generated `.env.dev`.

### Single-test runs

```
cargo test -p skylander-e2e-tests --test smoke -- --ignored --nocapture
```

Use one of the test files in `tests/` as the `--test` argument.
Suite-wide `--nocapture` is recommended — the harness multiplexes the
spawned server's stdout/stderr through `eprintln!` so you can see the
server's `INFO` / `WARN` / `ERROR` lines interleaved with test
progress.

## macOS specifics

- `phone URL …` log line uses Bonjour (`<host>.local`) and `serving on
  http://<en0-ip>:…` uses the en0 IP. The harness scrapes the
  `serving on …` line for its base URL, so loopback (`127.0.0.1`)
  isn't used.
- `127.0.0.1:<port>` will not respond — use the `serving on` URL.
- Cold cargo build for the test-hooks-flavored server is ~30–60s; the
  harness rebuilds incrementally on subsequent test runs.

## Layout

- `src/lib.rs` — `TestServer::spawn()` / `Phone` helpers used by each
  test. Exposes `inject_*` test-hook helpers and `Phone` selectors.
- `tests/*.rs` — one file per regression scenario from PLAN 3.6.
- `tests/screenshot_tour.rs` — generates the per-scene PNGs that
  populate `docs/assets/screens/`. **Re-baselining is per-OS** —
  Mac CoreText vs. Windows DirectWrite font rendering yields
  non-byte-identical pixels even at the same viewport (PLAN 10.3.4).
  When you intentionally change a screen, regenerate on the
  same OS that committed the existing baseline and `git diff
  docs/assets/screens/` to review.
- `tests/ios_simulator_smoke.rs`, `tests/ios_two_phone.rs` —
  Mac-only iOS-Simulator-driven tests. See "iOS Simulator lane"
  below.

## Screenshot tour as visual-regression contract

`tests/screenshot_tour.rs` walks the canonical first-time-user flow and
saves one PNG per screen to `docs/assets/screens/`. Originally the
per-tranche regression contract for the Phase 9 Tailwind v4 port (PLAN
9.x); the same workflow applies to any visual change. The tour fires
against a fixed 420×900 headless viewport (`--window-size=420,900` in
`Phone::new`), with deterministic inputs — three injected profiles in
the same order, Giants always booted, Kaos taunts driven by
`/api/_test/fire_*` with fixed strings — so on a single machine,
frame-to-frame the PNGs are byte-stable.

Workflow for visual changes:

1. Make the change (markup + utility classes; `@apply` only when an
   element exceeds ~12 utility classes; raw CSS only for keyframes /
   pseudo-element content / `:has()` — see PLAN 9.5).
2. Rebuild the phone bundle: `(cd phone && trunk build)`.
3. Run the tour:
   ```
   cargo test -p skylander-e2e-tests --test screenshot_tour \
       -- --ignored --nocapture
   ```
4. `git diff docs/assets/screens/` and reconcile every changed PNG
   before committing:
   - **No diff** → visually neutral; commit the code change without
     touching `docs/assets/screens/`.
   - **Intentional diff** (a deferred bug got fixed, or a design tweak
     was the point) → eyeball each screen, then commit the PNGs
     alongside the code with a note in the message.
   - **Unintentional diff** → real regression. Fix and re-run; do not
     commit drift you can't explain.

Practical caveats:

- The diff is only meaningful when before/after are captured on the
  same machine. GPU compositing, font hinting, and subpixel rendering
  vary between Windows/macOS and across Chrome versions. Capture a
  fresh baseline if you change machines.
- The tour requires the dev firmware-pack root (`SKYLANDER_PACK_ROOT`
  or the standard dev path in CLAUDE.md) so it can launch Giants and
  populate the toy box with real figure metadata; an empty pack
  produces blank cards.
- Animations settle for 550 ms before each capture (`settle()` in the
  test). If a step introduces a longer transition, bump that constant
  rather than racing it.

## iOS Simulator lane (macOS only)

Drives real iOS Safari inside the Simulator via the standalone
`tools/ios-inspect/` library (PLAN 10.4). Lives alongside the
chromedriver lane but with a different test-engine and an extra
prereq stack — runs the 2-phone product feature end-to-end against
real iPhone + iPad form factors instead of headless Chrome.

### Prereqs (in addition to the chromedriver lane prereqs)

- **Xcode** + at least one iOS runtime with both an iPhone and an
  iPad device available (Xcode → Settings → Platforms).
- **`brew install ios-webkit-debug-proxy`** — the bridge from
  WebKit's Web Inspector socket to a TCP port.
- A built phone bundle (`cd phone && trunk build`) — same as the
  chromedriver lane.

### Running

```sh
cargo test -p skylander-e2e-tests --test ios_simulator_smoke -- --ignored --nocapture
cargo test -p skylander-e2e-tests --test ios_two_phone -- --ignored --nocapture
```

Wall-clock expectations:
- `ios_simulator_smoke` — ~20 s (one iPhone sim cold-boot + SPA
  load + assertion).
- `ios_two_phone` — ~60–70 s (cold-boot of iPad + iPhone in
  sequence + per-device Safari startup + dual session assertion).
- Subsequent runs in the same shell session reuse the cached sim
  device snapshots and run faster.

### How it differs from the chromedriver lane

- Tests use `ios_inspect::{boot_devices, open_url, wait_for_selector,
  query_selector_*}` directly — no separate driver process to
  install or version-pin.
- Selectors are evaluated via `Runtime.evaluate` over the WebKit
  Web Inspector protocol, which uses **innerText** semantics —
  visually-hidden elements report empty (different from
  WebDriver's `getElementText`, which has its own quirks for
  `-webkit-text-stroke` titles like the chromedriver lane hits).
- Each test owns a `TeardownGuard` that runs `shutdown_all` on
  drop, so a panic mid-test still cleans up booted sims + the
  per-device proxies. Manual cleanup if needed:
  ```sh
  xcrun simctl shutdown all
  pkill -f ios_webkit_debug_proxy
  rm -f /tmp/ios-inspect-state.json
  ```

### Known caveats

- Sim Safari reports `safe-area-inset-bottom = 0` even on
  Dynamic-Island iPhones — bugs that depend on the bottom inset
  need PWA-standalone (Add-to-Home-Screen) inside the sim or a
  real device.
- `boot_devices` does substring matching against `xcrun simctl
  list`; same-name devices across runtimes (e.g. "iPhone 17 Pro"
  on iOS 26.0/26.2/26.4) pick the first hit non-deterministically.
  Pass an exact UDID to pin a specific runtime.
- Tests use `tokio::test(flavor = "multi_thread")` so the Drop-
  based teardown can call `block_on` — current-thread runtimes
  would panic in the guard.
