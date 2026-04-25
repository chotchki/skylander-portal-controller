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

## Screenshot tour as the Tailwind-migration regression contract

`tests/screenshot_tour.rs` walks the canonical first-time-user flow and
saves one PNG per screen to `docs/assets/screens/`. During the Phase 9
Tailwind v4 port (PLAN 9.x) this gallery doubles as the per-tranche
regression contract: the tour fires against a fixed 420×900 headless
viewport (`--window-size=420,900` in `Phone::new`), with deterministic
inputs — three injected profiles in the same order, Giants always
booted, Kaos taunts driven by `/api/_test/fire_*` with fixed strings —
so on a single machine, frame-to-frame the PNGs are byte-stable.

Per-tranche workflow during 9.4:

1. Port the tranche (markup + utility classes; `@apply` only when an
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
   - **No diff** → the tranche is visually neutral; commit the code
     change without touching `docs/assets/screens/`.
   - **Intentional diff** (a deferred bug got fixed, or a design tweak
     was the point of the tranche) → eyeball each screen, then commit
     the PNGs alongside the code with a note in the message.
   - **Unintentional diff** → real regression. Fix in the same tranche
     and re-run; do not commit drift you can't explain.

A few practical caveats:

- The diff is only meaningful when before/after are captured on the
  same machine. GPU compositing, font hinting, and subpixel rendering
  vary between Windows/macOS and across Chrome versions. Capture a
  fresh baseline if you change machines.
- The tour requires the dev firmware-pack root (`SKYLANDER_PACK_ROOT`
  or the standard dev path in CLAUDE.md) so it can launch Giants and
  populate the toy box with real figure metadata; an empty pack
  produces blank cards.
- Animations settle for 550 ms before each capture (`settle()` in the
  test). If a tranche introduces a longer transition, bump that
  constant rather than racing it.
