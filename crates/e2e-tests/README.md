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
3. Build the **phone SPA** at least once so `phone/dist/` exists:
   ```
   cd phone && trunk build
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
