# E2E tests

Integration tests that drive the full stack (server + phone SPA) via a real
browser (fantoccini → chromedriver → Chrome). Tests use the mock
`PortalDriver` with failure injection via `/api/_test/inject_load`, so they
do not require RPCS3 to be running.

## One-time setup

1. Install Chrome (stable channel).
2. Install ChromeDriver matching your Chrome version:
   - Download from https://googlechromelabs.github.io/chrome-for-testing/
   - Or `winget install --id=Chromium.ChromeDriver`
3. Start the chromedriver daemon in a dedicated terminal before running tests:
   ```
   chromedriver --port=4444
   ```
4. Build the phone SPA at least once so `phone/dist/` exists:
   ```
   cd phone && trunk build
   ```

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
