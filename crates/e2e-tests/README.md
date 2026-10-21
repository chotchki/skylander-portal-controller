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
4. Provide a **firmware pack** with `.sky` files (product runs only;
   `E2E_APP=guacamole` needs none, see below). The harness needs a
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
- **On macOS, drop a matching driver at**
  `~/Library/Application Support/skylander-portal-controller/chromedriver-<major>`
  (e.g. `chromedriver-155` for Chrome 155.x, from
  <https://googlechromelabs.github.io/chrome-for-testing/>). The harness reads the
  installed Chrome's major version and picks that file before PATH / Homebrew, so
  no env var is needed. Or:
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
test via `cargo run -p skylander-server --features test-hooks` (or
`-p guacamole`, see `E2E_APP` below) with a temp working directory
containing a generated env file.

### Single-test runs

```
cargo test -p skylander-e2e-tests --test smoke -- --ignored --nocapture
```

Use one of the test files in `tests/` as the `--test` argument.
Suite-wide `--nocapture` is recommended — the harness multiplexes the
spawned server's stdout/stderr through `eprintln!` so you can see the
server's `INFO` / `WARN` / `ERROR` lines interleaved with test
progress.

### Against guacamole (`E2E_APP`)

`E2E_APP` picks the binary `TestServer::spawn` / `spawn_with_env_lines`
run: `product` (the default) or `guacamole` (anything else panics, so a
typo can't silently test the wrong app). openspec AA.10.

```
E2E_APP=guacamole cargo test -p skylander-e2e-tests -- --ignored --nocapture
```

Under `guacamole` the harness runs `cargo run -p guacamole --features
test-hooks` with no firmware-pack lookup. The env file it generates in
the temp working dir points `GUAC_DUMP_DIRS` at a temp dir of synthetic
dumps, each a `PS3_GAME/PARAM.SFO` built in code (BLUS30968 unless a test
asks for other titles), and sets `WINDOW_MODE=desktop`. The Giants dump
is an `IntroFixture` with no intros: a finished extract
(`GUAC_EXTRACT_DIR`, the dump trusted through `GUAC_TEST_KNOWN_DUMP`)
whose roster, `E2E_ROSTER` in [`src/lib.rs`](src/lib.rs), holds ten
figures under their disc ids (every one the tests place, plus Legendary
Ignitor as a variant), each with a flat-colour PNG portrait, so guacamole
lists them from the start (openspec AE.4). No
`DATA_ROOT`: guacamole reads no wiki data. `BUILD_TOKEN=e2e-test` is
pinned the same way, so the phone bundle from step 3 serves both apps.
`spawn_live` and `spawn_ipc_*` bail under it: they drive RPCS3.

[`tests/guacamole_tv.rs`](tests/guacamole_tv.rs) (25 tests) is
guacamole-only and skips with a message otherwise. It reads what the TV
shows through `/api/_test/tv`, a JSON mirror of the laid-out Bevy UI
tree (screen, join line, player count, each card's lines, portrait,
owner pip and rect, and `focus`, the focused choice's id:
`choose-folder`, `continue`, `close` or null; openspec AP.6), not pixels.
`POST /api/_test/tv/press` with `{"button": "south"}` (or `east`,
`mode`, `up`, `down`, `left`, `right`) hands the TV's input router a
press as a pad would (202; an unknown name or a body without `button`
is a 400), `press_tv` in [`src/lib.rs`](src/lib.rs); it's a test hook
under `test-hooks`, not a phone remote. `no_giants_dump_offers_no_game`
waits for `focus` to read `choose-folder` without pressing it (that
would open the folder sheet), and `pad_presses_leave_the_join_code_up`
posts South, East and Mode on the join screen and finds it unchanged
(AP.5). `POST /api/_test/tv/pad` (`pad_tv`) drives one of four pads as a
hand would, one act a body: `{"pad": 0, "connect": {"name", "vendor",
"product"}}` (each optional), `"disconnect": true`, `"stick": [x, y]`
(raw, each −1 to 1, held until the next) or `"press": "start"` (a face
button by position, `start`, `select`, `mode` or a D-pad direction; down
a frame, then up), through Bevy's own gamepad systems, so the level's
layer (openspec AG.20) reads it as a real pad; the mirror's `input`
reports the pads in the order they connected (name, ids, the resolver's
`labels`, the raw `stick`, `walking`), the `layer` that takes them, the
level's `pause` and its `overlay` as drawn, and `hero_stick`.
`a_posted_pad_reads_raw` checks a stick value inside Bevy's own dead zone
lands as posted. `placing_a_character_enters_the_level_at_its_start`
(openspec AG.21) runs over a walking fixture
(`TestServer::spawn_guacamole_walking`: `E2E_WALKER`, Eruptor, on the
load file guacamole walks, with a synthetic model of its own; in every
other fixture he keeps his portrait, so no other test's placement of him
enters the level), whose made-up chapter 1 is a floor ending at a wall
`E2E_WALL` (6 m) ahead of `213_Init_Location`'s first waypoint
(`E2E_START`). Placing him takes the TV into the level: the mirror's
`mode` (`portal`, `level`, or `paused` while it waits) and `hero` (slot,
figure, load file, his model's way to the level, and in it his `position`,
`heading` and the `camera` as drawn) show him at the start facing 0°,
settled 5 mm above the floor, the camera at camera 0's rest pose off his
feet, waiting for a pad; a posted pad's press takes him over and its
stick walks him along the camera's 15° to the wall, which holds him 5 mm
off. The walking fixture carries a hero side too (openspec AG.30: walk,
run and falling clips on the synthetic skeleton, each holding its top
joint at its own `E2E_GAIT_X`), and the mirror's `hero.gait` (the
heaviest `clip`, the `speed_ft` his walk read, `run_share`, `weights`
idle, walk, run and fall, the walk's and run's `rates`, `air`) shows him
in his idle at the start, running pushed along the wall at full stick
(21.5 ft/s, the run at 102.4 %) and back in his idle let go;
`GET /api/_test/tv/gaits?since=<step>` (`gaits_tv`) hands out every
step's (`step`, the fixed clock's `time`, the `stick` and its
`strength`, `speed_ft`, `run_share`, `weights`, `rates`, `clip`, `air`)
after `since` with the last step's number as `now`, and the test checks
each step's run share is AG.11's of its speed. The mirror's `warm` is the turntables' pre-warm (openspec AE.14:
`state` `idle`, `reading`, `compiling` or `done`, and its key counts),
and `GET /api/_test/tv/frames?since_ms=<ms>` hands out the TV's frame
log, `[ended_ms, took_ms, pipelines_compiled]` per frame after
`since_ms` with the last frame's end as `now_ms`; no test reads it yet,
AE.14's stall check drives it by hand.

Its figure-progress section (openspec AH.3) drives the phone's editor on
guacamole's figure rows: the details' strip and the edit sheet's caps from
the extract's own table and clamp (`E2E_LEVEL_XP`, `E2E_GOLD_CAP`), an
edit refused while the viewer's own figure is on the portal (read in the
sheet's kid's words), a sibling's session refused, reset to fresh, a
second phone's strip following an edit, and RESET on the details of a
figure standing on the portal going to the slot reset.
`a_never_placed_figure_edited_is_kept_across_a_restart` restarts
guacamole with `TestServer::restart_guacamole`, which kills it and starts
it again in the same temp dir (the same `.env.dev`, port, database and
extract); phones connected before see the connection drop.

[`tests/guacamole_intro.rs`](tests/guacamole_intro.rs) (5 tests,
openspec AB.11, AB.21, AB.22, AB.23 and AP.5) is guacamole-only too and needs ffmpeg (`brew install
ffmpeg`; found on PATH or in Homebrew's folders). Each test builds an
`IntroFixture` first: a synthetic Giants dump (a PARAM.SFO and a text
file) and a finished extract of it, `E2E_ROSTER` included, whose intros
are ffmpeg's `testsrc`
picture over a `sine` tone, encoded at run time to guacamole's own
target, so no game bytes are involved anywhere. guacamole gets the
extract as `GUAC_EXTRACT_DIR` and trusts the dump through
`GUAC_TEST_KNOWN_DUMP` (title id, version, manifest hash; read only by a
`test-hooks` build), finds the extract current and runs no ffmpeg
itself. The tests read `intro` and `intro_queue` off `/api/_test/tv` and
skip through `/api/_test/tv/skip`, or, in
`a_pad_press_skips_to_the_queued_intro`, with a South posted to
`/api/_test/tv/press`, which the intro takes as its skip before anything
under it sees it. AB.21's resume test re-unlocks through
`/api/_test/unlock_session`, the hook that offers "resume last setup?"
the way a PIN unlock does (`set_session_profile` binds the profile
without offering it), and checks each RESUME's loads at the server over
a WebSocket of its own (a Loading then a Loaded per slot, no error), so
a resume that sent nothing can't pass for one that replayed nothing.
That socket opens before the phone's: the unlock hook binds the newest
session. AB.22's act in the same test binds a second profile, Bob, to
that socket's session to place a figure (the server admits two sessions
and the socket and the phone hold both), then resumes him on the phone
after Alice locks it, and checks his RESUME listed and loaded only his
own figure with no `slot_changed` on Alice's. AB.23's beat locks Bob,
fills his saved slot with Alice's figure from the socket's session and
checks his RESUME is refused with the phone's toast naming his figure
and hers, and no `slot_changed` at all. Every guacamole the
harness spawns plays its intros muted (`GUAC_MUTE=1`) and takes only the
pads `/api/_test/tv/pad` posts (`GUAC_TEST_POSTED_PADS_ONLY=1`, AG.52): a
pad paired with the Mac never connects, so it can't walk the hero first.

Product-only tests skip with `SKIPPED under E2E_APP=guacamole,
product-only: <why>` instead of failing:

- [`figure_edit.rs`](tests/figure_edit.rs) (needs a `.sky` figure
  file): `edit_level_and_gold_round_trips_through_stats_strip`,
  `gold_chevrons_jump_to_bounds_and_max_round_trips`,
  `stats_button_disabled_while_figure_on_portal`.
- [`working_copies.rs`](tests/working_copies.rs) (needs a `.sky`
  working copy): `load_uses_canonical_name_not_filename`,
  `resume_prompt_offers_prior_layout`.
- [`live_integration.rs`](tests/live_integration.rs) (drives a live
  RPCS3): `phone_drives_real_rpcs3_load_and_clear`,
  `live_clear_then_load_different`, `live_spam_click_same_slot`,
  `live_resume_after_reload`, `live_concurrent_edits_both_phones`,
  `live_independent_profiles_loads`.
- [`live_lifecycle_switch.rs`](tests/live_lifecycle_switch.rs) (live
  RPCS3): `launch_stop_relaunch_switch`.
- iOS Simulator: `ios_pwa_hint_shows_on_iphone_only`,
  `ios_smoke_game_picker_renders`,
  `ios_two_phone_independent_profiles`.
- [`screenshot_tour.rs`](tests/screenshot_tour.rs):
  `capture_full_phone_tour` (it writes the product's tracked gallery,
  `docs/assets/screens/`).

Every other mock-harness test runs against both apps. On the Mac
(2026-10-18, serial, `--skip ios_`, after AB.21 and AA.20, and the same
again after AE.4 moved guacamole onto the fixture roster): guacamole
55 of 55 ok, 42 run for real (13 TV + 4 intro + 25 shared) and 13
product-only skips; the product 52 of 55 ok, 31 run for real (28
passed, 3 failed), 7 live-RPCS3 tests skip without
`RPCS3_TEST_SERIAL` / `RPCS3_SKY_TEST_PATH`, plus 17 guacamole-only
skips. Its 3 failures already failed at the pre-AA base (46866ce):
`edit_level_and_gold_round_trips_through_stats_strip`,
`gold_chevrons_jump_to_bounds_and_max_round_trips` and
`resume_prompt_offers_prior_layout`. All three are product-only `.sky`
tests, so under the product freeze they stay failing until chotchki
calls a fix or G1 removes them with the `.sky` code.

After AP added `pad_presses_leave_the_join_code_up` and
`a_pad_press_skips_to_the_queued_intro` (2026-10-18, serial, `--skip
ios_`, ffmpeg on PATH), guacamole runs 57 of 57 ok, 44 for real (14 TV
+ 5 intro + 25 shared) and the same 13 product-only skips; a product run
lists those two among its guacamole-only skips, 19 now (not rerun: AP
touches only `crates/guacamole` and these tests). Under load
`multi_phone.rs` has failed on "phone never received Event::Welcome"
in a whole-suite run and passed alone; rerun a test that fails that way
by itself before calling it a regression.

After AH.3 (2026-10-20, serial, `--skip ios_`, `--no-fail-fast`, the
phone built with `BUILD_TOKEN=e2e-test`): guacamole 66 of 66 ok, 53 for
real (23 TV + 5 intro + 25 shared) and the same 13 product-only skips;
the product 63 of 66 ok, 31 for real (28 passed, the same 3 failed), 7
live-RPCS3 skips and 28 guacamole-only skips. Without `--no-fail-fast`
cargo stops at the first failing test binary (`figure_edit.rs` under the
product), so a product run needs it to reach the rest.

After AH.37 (2026-10-20, the same flags): guacamole 67 of 67 ok, 54 for
real (23 TV + 5 intro + 25 shared + AH.37's) and the same 13
product-only skips; the product 64 of 67 ok, 31 for real (28 passed, the
same 3 failed), 7 live-RPCS3 skips and 29 guacamole-only skips. AH.37's
`a_second_phone_of_one_profile_cant_place_her_figure_twice`
([`tests/multi_phone.rs`](tests/multi_phone.rs), guacamole-only) needs a
PLACE the phone's own view lets through, so it holds the second phone's
page in one synchronous script: a synchronous XHR poll of `/api/portal`,
during which no WebSocket message reaches that phone's view. Its view
keeps her Spyro in slot 1 while the first phone swaps him for her
Eruptor, and once the server shows Eruptor there the script presses
PLACE, which asks for slot 2. The test reads the server's log line too,
so a refusal by the phone's own check can't pass for the server's.

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
- [`tests/smoke.rs`](tests/smoke.rs)'s
  `every_menu_action_is_reachable_in_a_short_viewport` (openspec AA.20)
  opens the kebab menu with a profile and a game, in Chrome's phone
  emulation at 375x548 and 390x664 (a phone browser with its toolbars
  up), and requires every action fully on screen once a finger scrolls
  to it, with a tap at its centre landing on it, and the close button on
  top and tappable, see-through at rest and filled once the content has
  scrolled under it (the 375x548 menu scrolls). The probe is
  `MENU_REACH_JS` in `src/lib.rs`, shared with
  [`tests/ios_menu_reach.rs`](tests/ios_menu_reach.rs), which runs it in
  the iOS Simulator's Safari (a tab, not the PWA). Both run against
  either app.
- `tests/ios_simulator_smoke.rs`, `tests/ios_two_phone.rs` —
  Mac-only iOS-Simulator-driven tests. See "iOS Simulator lane"
  below.
- [`tests/guacamole_tv.rs`](tests/guacamole_tv.rs) — guacamole's TV
  cards and setup, run only under `E2E_APP=guacamole` (see above).
- [`tests/guacamole_intro.rs`](tests/guacamole_intro.rs) — guacamole's
  character intros on a synthetic extract (`IntroFixture` in
  [`src/lib.rs`](src/lib.rs), needs ffmpeg), run only under
  `E2E_APP=guacamole` (see above).

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

Drives real iOS Safari inside the Simulator via the external
[`ios-inspect`](https://github.com/chotchki/ios-inspect) crate,
consumed as a git dependency pinned to tag `v0.0.1` (PLAN 10.4).
Lives alongside the
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
