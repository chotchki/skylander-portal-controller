# Skylander Portal Controller — Working Notes

Compact reference for this project. SPEC.md is the authoritative long-form requirements + Q&A log. PLAN.md tracks execution.

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
- **Release runtime state:** `%APPDATA%\skylander-portal-controller\` — SQLite DB, per-profile working copies of `.sky` files, logs (daily rotation, 7-day retention).
- **Dev runtime state:** `./` (repo workspace) — logs under `./logs/`, DB and working copies under `./dev-data/`. Gated by `dev-tools` Cargo feature.
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

## Takeover

- "Last device wins" with a 1-minute cooldown between takeovers.
- Silent cutover. Kicked device shows a Chaos-themed "taken over" screen offering a "kick back" action.
- Takeover inherits game/portal state but re-locks the profile; new user must enter PIN.

## Security

- Trusted LAN only, HTTP.
- Phase 1: unsigned commands. Phase 1.5 (immediately after protocol stabilizes): HMAC signing, key embedded in QR.
- Strict input validation. No filesystem paths ever leave the server. Canonical figure names only (no user-entered names).

## Aesthetic

- Match Skylanders game UI: starfield blue backgrounds, circular gold-bezeled figure portraits, bold white titles with gold outline, cartoony feel. Reference: `docs/aesthetic/ui_style_example.png`.
- Implement via CSS (wiki asset resolution isn't enough for high-res phones).
- Phone UI is theme-able (prepping for the Chaos "mind magic" takeover skin — dark purple/pink).

## Chaos feature (LAST — post-MVP)

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

## Naming gotchas

- Spec originally said "RPS3" — it's **RPCS3**.
- Firmware file extension is `.sky`, not `.dump`.

## See also

- `SPEC.md` — authoritative long-form spec + full Q&A history with decision rationale.
- `PLAN.md` — current execution plan (research-first, phased).
- `docs/aesthetic/` — UI reference images.
