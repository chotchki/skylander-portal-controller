# Skylander Portal Controller — Working Notes

Compact reference for this project. SPEC.md is the authoritative long-form requirements + Q&A log. PLAN.md tracks execution, new tasks should always be numbered and have a checkbox so it is traceable.

## What it does

A Windows app that wraps RPCS3 (PS3 emulator) so kids can manage the emulated Skylanders portal from a phone/iPad over Wi-Fi. Launched from Steam Big Picture. Shows a QR code on the TV → phone connects → pick a game → portal-control view with the family's figure collection.

## Tech stack (committed)

- **Language:** Rust.
- **HTTP/WS server:** Axum.
- **Phone SPA:** Leptos (WASM). JS fallback acceptable if touch/UX forces it.
- **PC-side launcher window:** egui via `eframe`, fullscreen, sized for 86" TV at 10 ft.
- **DB:** SQLite via `sqlx` (async, compile-time-checked queries).
- **GUI automation for RPCS3:** UI Automation (Windows accessibility API) first. Image/OCR second. Raw coordinates are a last resort.
- **QR code:** any standard Rust crate.
- **E2E tests:** pure-Rust WebDriver (fantoccini or thirtyfour — no preference).

## Architecture

- Cargo workspace:
  - `crates/core/` — shared types (Figure, SlotState, Command, Event). No I/O. Public/private split enforced via `Figure::to_public()`.
  - `crates/indexer/` — walks the firmware pack, emits `Vec<Figure>` with stable SHA-256-truncated IDs.
  - `crates/rpcs3-control/` — `PortalDriver` trait, `UiaPortalDriver` (Windows UI Automation), `MockPortalDriver` (feature `mock`). Off-screen hiding via Win32 `SetWindowPos`.
  - `crates/server/` — the binary: Axum + eframe + driver worker + config + logging. `dev-tools` feature on by default.
- Separate from the workspace: `phone/` is a Leptos CSR crate that builds to WASM via trunk. Server's `tower_http::services::ServeDir` serves `phone/dist/`.
- Threading: main OS thread owns eframe. Dedicated background thread hosts the tokio multi-thread runtime (Axum + driver worker).
- Driver worker: single tokio task drains `mpsc<DriverJob>`; each load/clear runs inside `spawn_blocking`. Portal state lives in `Arc<Mutex<[SlotState; 8]>>`; changes broadcast through `broadcast::Sender<Event>`.
- Phone is a dumb client. REST for commands (return 202), WS for state (snapshot on connect, `SlotChanged` per event).

## Data & paths

- **First-launch config** (PC keyboard, one time): RPCS3 executable path, firmware-pack root. Games auto-detected by scanning RPCS3's library for known serials; missing games don't delete their per-game settings.
- **Firmware pack** layout: `{Game}/{Element}/[Alternate types/]{Name}.sky`. Top-level `Items`, `Adventure Packs` (per-game subfolders), `Sidekicks` (top-level is a duplicate — ignore it; Giants' internal `Sidekicks` is authoritative). Ignore `desktop.ini`, posters, element-symbol PNGs (reuse as element icons), readme `.txt` files.
- **Runtime state roots** (resolved once at startup, gated by the `dev-tools` Cargo feature — release builds physically can't pick the dev path):
  - Release: `%APPDATA%\skylander-portal-controller\` — `db.sqlite`, `working/<profile_id>/<figure_id>.sky`, `scanned/<uid>.sky`, `logs/` (daily rotation, 7-day retention).
  - Dev: `./dev-data/` — same layout, plus `./logs/` next to it. Both are gitignored. Never write runtime state outside these roots.
  - Dev `DATA_ROOT` must be set to `./dev-data` in `.env.dev` to unify the scanner's output dir with the profile db (the compile-time default is `./data`, a leftover from earlier layouts).
- **Scanned-figure layout (PLAN 6.5.3):** `<data_root>/scanned/<uid>.sky`, where `<uid>` is the 8-char uppercase hex of the 4-byte Mifare NUID (e.g. `7FC1ADA3.sky`). Each *physical* tag is its own file — two copies of Spyro in the same household remain distinct so their independent gold / level state is preserved. A re-scan of the same tag overwrites the file (letting later scans pick up on-tag changes) and the server emits `Event::FigureScanned { is_duplicate: true }`. Working-copy semantics for scanned figures (one copy per (profile, uid)? per (profile, canonical figure)?) are deferred to 6.5.5 alongside the broader `FigureId` rekey.
- **Known dev RPCS3 install:** `C:\emuluators\rpcs3` (note the typo; it's the real path).
- **Known dev firmware pack:** `C:\Users\chris\workspace\Skylanders Characters Pack for RPCS3`.
- **Phone never sees file paths or filenames** — only stable figure IDs.

## Profiles & PINs

- Up to 4 profiles. Each has a 4-digit PIN (anti-sibling, not real security). PINs stored in SQLite.
- Per profile: working copies of `.sky` files (copy-on-first-use from fresh pack), last portal layout.
- Profile picker is the first screen after connect ("welcome, portal master"). Game picker comes after unlock.
- Guest mode = post-MVP.
- Imaginators creation crystals: per-profile, never auto-reset without confirmation.

## Portal behavior

- Portal is an 8-slot screen regardless of game (game will complain if overloaded — we don't pre-validate).
- Portal view is a "drawer" separate from the browse/collection view. Tap figure → Add button. Tap slot's figure → Remove button. No drag-drop.
- On session unlock: prompt "resume last setup?" then auto-drive the GUI.
- Working copy auto-loads when a figure is picked. Fork from fresh on first-ever use. Reset-to-fresh is an explicit user action.
- Figure file shared across games (one working copy per profile+figure).

## Concurrency & takeover

- **Up to 2 concurrent phone sessions** (matches co-op player count). Each unlocks its own profile with its own PIN; unlocks do not propagate between phones.
- Portal is shared free-for-all — either phone can touch any slot. The single driver worker serialises operations naturally; no per-slot arbitration.
- 3rd connection evicts the **oldest** session (FIFO) — evicted phone sees the Kaos "taken over" screen with a "kick back" button.
- 1-minute cooldown applies only to forced eviction (anti-ping-pong). Joining into a free slot has no cooldown.
- Evicted session's kick-back inherits game/portal state but the profile re-locks; PIN re-entry required.
- Any connected phone can display the join QR in-app ("show join code") so existing players can hand it to a new joiner.
- Portal view shows an **ownership indicator** per occupied slot (profile colour/initial) so players can tell whose figure is whose. Ownership = the profile that placed the figure into that slot.
- Post-disconnect figure-cleanup semantics (2-player case) are deliberately deferred — revisit alongside the reconnect-overlay phase.

## Security

- Trusted LAN only, HTTP.
- Phase 1: unsigned commands. Phase 1.5 (immediately after protocol stabilizes): HMAC signing, key embedded in QR.
- Strict input validation. No filesystem paths ever leave the server. Canonical figure names only (no user-entered names).

## Aesthetic

- Match Skylanders game UI: starfield blue backgrounds, circular gold-bezeled figure portraits, bold white titles with gold outline, cartoony feel. Reference: `docs/aesthetic/ui_style_example.png`.
- Implement via CSS (wiki asset resolution isn't enough for high-res phones).
- Phone UI is theme-able (prepping for the Kaos "mind magic" takeover skin — dark purple/pink).
- **Mocks live in `docs/aesthetic/mocks/`** as standalone HTML files — open directly or via a local server. `docs/aesthetic/mocks/index.html` lists every mock grouped by flow.
- **Review mocks on a real iPhone, not just desktop preview.** Safe-area insets, Dynamic Island collisions, mobile Safari viewport behavior (address-bar hiding, pinch-zoom, orientation lock) all differ from desktop devtools. Serve with `python3 -m http.server 8089` from `docs/aesthetic/mocks/` and open `http://<mac-en0-ip>:8089/` on the iPhone (`ipconfig getifaddr en0` to find the IP). Requires the iPhone and Mac to share a network — Mac-as-hotspot (Internet Sharing) works; iPhone-as-hotspot blocks incoming connections to the Mac.
- **Safe-area pattern for top-of-screen padding:** `max(Npx, calc(env(safe-area-inset-top) + 12px))` where N is the desktop-preview value. Preserves desktop look; adapts on devices with a notch/island. Same pattern applies to L/R when content hugs screen edges.
- **Claude-driven iOS repro (`tools/ios-inspect/`).** Mac-only CLI that drives the iOS Simulator + Safari via `xcrun simctl` and the WebKit Web Inspector protocol (through `ios-webkit-debug-proxy`). Use it for **layout/CSS probes** — `safe-area-inset-*` values, computed styles on specific selectors, DOM-subtree dumps, web-content-only screenshots. Use the chromedriver e2e harness for **functional regressions** that don't depend on iOS-specific rendering. One-shot bringup from the repo root: `cargo build --manifest-path tools/ios-inspect/Cargo.toml && alias ios-inspect=$PWD/tools/ios-inspect/target/debug/ios-inspect && (cd phone && trunk serve --address 0.0.0.0 --port 8090 &) && ios-inspect boot && ios-inspect open http://$(ipconfig getifaddr en0):8090/`. Then iterate with `ios-inspect eval '<js>'`, `ios-inspect computed-style <selector> --filter …`, `ios-inspect screenshot --web-only -o /tmp/x.png`. Proxy lifecycle is self-healing — no manual restart mid-session. **Sim fidelity gap to remember**: non-standalone sim Safari reports `safe-area-inset-bottom = 0` (vs ~34px on real Dynamic Island hardware); bugs that depend on the bottom inset need PWA-standalone mode (Add-to-Home-Screen in the sim) or a real-device fallback. See `tools/ios-inspect/README.md` for full usage + limitations.

### CSS escape-hatch policy (PLAN 9.5)

The phone is on Tailwind v4 (PLAN 9.x). When adding or editing styles, default to inline utilities; reach for an `@apply` rule only when readability suffers; reach for raw CSS only for things Tailwind can't shorten. Three tiers:

- **Inline utilities (default).** Layout (`flex`, `grid`, `absolute`), spacing (`p-4`, `gap-3`), typography (`font-display`, `text-body-italic`), colour (`text-gold-bright`, `bg-sf-1`), single-layer effects (`rounded-xl`, `shadow-sm`). Put them on the element in the Leptos `view! {}` macro. This is the home of 95%+ of styling.

- **`@apply` in a per-component CSS file.** Reach for this when an inline class string would exceed roughly **12 utility classes on a single element**, when the same cluster repeats across siblings (an action button group, a row of swatches), or when the element uses pseudo-element trickery (`::before` / `::after`) that needs to share styling vocabulary with the parent's utilities. Files live at `phone/styles/components/<component>.css`, are imported from `phone/styles/input.css`, and are wrapped in `@layer components { … }` so they slot beneath utility specificity.

- **Raw CSS (rare).** Reserve for things Tailwind genuinely can't help with: `@keyframes` (named animations), `@font-face` (font loading), multi-layer pseudo-element decoration with stacked-gradient `background-image`, `:has()` selectors driving cross-component layout, masked SVG silhouettes (`-webkit-mask: url(…)`), `mix-blend-mode` overlays, complex shadow stacks beyond what `shadow-[…]` arbitrary values reasonably express. Co-locate raw CSS with the component that uses it (next to its `@apply` block in the same file), not in a global stylesheet.

The migration's reference exemplars: `gold_bezel.css` (mixes all three tiers — `@apply` for layout primitives, raw `box-shadow` stacks + radial gradients), `kaos_overlay.css` (raw-heavy: six stacked layers, seven keyframes, masked SVG sigil), `framed_panel.css` (a clean two-rule file with one `::before` decoration). When in doubt, look at how a similar component handled it.

Hidden costs to keep in mind: every `@apply` in a component file means class names that don't appear inline on the element, which makes the element harder to grep for visually. Prefer `@apply` only when the readability gain on the markup side outweighs that. The 12-class rule of thumb is just a heuristic — a 6-class string with three repetitions across siblings is more @apply-worthy than an unrepeated 14-class one-off.

## Kaos feature (LAST — post-MVP)

- Wall-clock timer: 20-min warmup, then random within every hour window.
- Text-only overlay (Kaos catchphrases from wiki — text avoids copyright issues).
- 1-for-1 swap of a portal figure with a randomly-chosen compatible figure from the owned collection.
- Compatibility rule (heuristic, can enhance later): figure works in its game of origin and all later games, with known exceptions (vehicles only in SuperChargers, etc.).
- Reposes: collapsed in browse view with a "N variants" badge. Cycle button on card for variant swap.

## Testing

- (1) Unit tests for pure logic (figure indexer, protocol, state machine).
- (2) Integration tests for DB + filesystem.
- (3) E2E: pure-Rust WebDriver drives a headless browser against the phone SPA; test harness reads the QR URL from the log file by pattern. Run locally, not in CI.
- CI deferred until the app works.

## Dev mode (`dev-tools` feature flag)

- Logs to `./logs/`, verbose level.
- Skip first-launch config by reading paths from a `.env.dev`.
- E2E harness can inject a profile and bypass PINs.
- Release builds physically cannot take these shortcuts.

## Error handling

- GUI-drive failure: silent retry up to N times, then error toast on phone. Start simple, iterate.

## Distribution

- GitHub Releases zip. Do **not** bundle RPCS3 or `.sky` files (no piracy).
- Users supply their own RPCS3 install and firmware backups.
- Steam Big Picture shell behavior is a compatibility-pass concern, not a day-1 constraint.

## Git workflow (pre-1.0)

- **Commit + push directly to `main`.** This is a solo project with no external developer coordination; GitHub PRs are pure friction at this stage. Skip them.
- Reserve PR ceremony for post-1.0 or for cases where a human reviewer genuinely adds value (e.g. first-time CI-bring-up or a dangerous rewrite where the diff view is the point).
- Concurrent subagents modifying overlapping files → spawn with `isolation: "worktree"` so they don't entangle WIP; merge their branches into `main` locally when done.

## RPCS3 window/menu gotchas (see `docs/research/game-launch-window-mgmt.md`)

- While a game runs, RPCS3 has **two** top-level windows: the **main window** (menu bar, Qt class `Qt6110QWindowIcon`, title prefix `"RPCS3 "`) and the **game viewport** (same Qt class, title prefix `"FPS:"`). The viewport usually covers the main window.
- **UIA `Invoke`/`ExpandCollapse` don't work on Qt 6 menus.** Menu items exist in the UIA tree but have zero children until a real user interaction opens them. We drive the Manage menu with synthesised keystrokes (Alt → arrows → Enter) instead, verifying `HasKeyboardFocus` at each step.
- **Dialog opens once per RPCS3 session** — `open_dialog` navigates the menu on first call, then keeps the Skylanders Manager off-screen for the rest of the session. Subsequent calls short-circuit. If RPCS3 restarts, first `open_dialog` re-does the nav (brief once-per-session flicker during boot).
- **Focus thieves**: the game viewport (minimised during nav), RPCS3's **update-check popup** at boot (can steal foreground mid-nav — tell users to disable Settings → Advanced → "Automatically check for updates at startup").
- `RPCS3.buf` singleton lockfile next to `rpcs3.exe` survives forced kill → next launch fails. `RpcsProcess::shutdown_graceful` deletes it after the `Forced` path. Spawned processes are also wrapped in a Win32 Job Object with `KILL_ON_JOB_CLOSE` so RPCS3's re-exec shims and worker children don't leak across test runs.
- **Booting a game programmatically:** launch `rpcs3.exe` with no arguments (library view), then find the `DataItem` whose name matches the game's serial (e.g. `"BLUS30968"`) under the `game_list_table`. Invoke via `SelectionItemPattern.select()` + `set_focus()` + synthesised `Enter` — UIA Invoke alone doesn't boot. The EBOOT-argument launch path puts RPCS3 into a direct-boot state where the menu bar does not respond to synthesised keystrokes; don't use it.
- **Session isolation:** all UIA + SendInput automation is session-bound. Tests that exercise the real driver must run on the user's interactive desktop — SSH connects in session 0 and cannot see/touch windows in session 2+ at all. `RpcsProcess` launches, `EnumWindows`, UIA tree walk all return empty under SSH. Run RPCS3-live tests on the physical machine.

## Naming gotchas

- Spec originally said "RPS3" — it's **RPCS3**.
- Firmware file extension is `.sky`, not `.dump`.

## See also

- `SPEC.md` — authoritative long-form spec + full Q&A history with decision rationale.
- `PLAN.md` — current execution plan (research-first, phased).
- `docs/aesthetic/` — UI reference images.
