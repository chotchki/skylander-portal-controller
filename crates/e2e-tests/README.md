# E2E tests

Integration tests that drive the full stack (server + phone SPA) via a real
browser (fantoccini → chromedriver → Chrome). Tests use the mock
`PortalDriver` with failure injection via `/api/_test/inject_load`, so they
do not require RPCS3 to be running.

## One-time setup

1. Install Chrome (stable channel).
2. Install ChromeDriver matching your Chrome version, on PATH:
   - `winget install --id=Chromium.ChromeDriver`, or
   - grab a matching build from https://googlechromelabs.github.io/chrome-for-testing/
3. Build the phone SPA at least once so `phone/dist/` exists:
   ```
   cd phone && trunk build
   ```

The harness spawns its own chromedriver on a free port per `TestServer` and
kills it on drop — no need to run `chromedriver` manually.

## Running

```
cargo test -p skylander-e2e-tests -- --ignored --nocapture
```

All tests are `#[ignore]`-gated so they don't run under the default
`cargo test --workspace`. The harness spawns the server binary per test
via `cargo run -p skylander-server --features test-hooks` with a temp
working directory containing a generated `.env.dev`.

## Layout

- `src/lib.rs` — `TestServer::spawn()` / `Phone` helpers used by each test.
- `tests/*.rs` — one file per regression scenario from PLAN 3.6.
