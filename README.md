# skylander-portal-controller

Remote-control the RPCS3 emulated Skylanders portal from a phone or iPad over your local Wi-Fi. Family-safe alternative to a physical Skylanders portal where the kids' save data lives somewhere safer than a pile of plastic figures on the living-room floor.

The Windows app boots from Steam Big Picture and shows a QR code on the TV; phones scan in, pick a profile (PIN-gated), pick a game, and drive RPCS3's emulated portal slot-by-slot.

**Latest release:** [v1.9.1](https://github.com/chotchki/skylander-portal-controller/releases/tag/v1.9.1) — **install polish.** Start Menu / Desktop shortcut icons now render (the MSI's advertised shortcuts were missing an explicit icon reference), and the install ships a first-pass **Steam library artwork** set in `steam/` (grid capsule, hero, logo, header) — apply it via Steam's right-click → Manage → Set Custom Artwork. The macOS build is paused (Windows-only artifacts for now). Everything below is unchanged from v1.9.0.

[v1.9.0](https://github.com/chotchki/skylander-portal-controller/releases/tag/v1.9.0) — **per-game emulator config + connectivity self-help.** Two additions on top of the v1.8.0 recovery work: (1) **Per-game RPCS3 settings, from the phone.** A grown-ups-only **"HOLD FOR GAME SETTINGS"** action in the phone's menu (shown only between games) opens RPCS3's *own* settings GUI on the TV so you can tune a game's Custom Configuration (renderer, SPU/PPU accuracy, framelimit…) with the HTPC keyboard/mouse; RPCS3 saves it and the normal launches pick it up — the controller never edits RPCS3's config files itself. The launcher steps aside while settings are open and the phones show a "configuring on the TV…" overlay. (2) **"Trouble connecting?" self-diagnostics.** If no phone reaches the launcher within a grace window, the TV shows a help card with the **raw-IP address + a scannable raw-IP QR** (a fallback for phones that can't resolve the `.local` name), a same-Wi-Fi reminder, and — on Windows — a **one-click "Fix Firewall"** button that adds the inbound rule for you (handy for the portable-zip install, which otherwise gets no firewall exception).

Earlier headlines — [v1.8.0](https://github.com/chotchki/skylander-portal-controller/releases/tag/v1.8.0): **automatic crash/freeze recovery** — the controller supervises RPCS3 and, on a hard crash *or* a freeze (heartbeat frame counter stalling), covers the screen, relaunches the same game, and re-places the figures that were on the portal, with phones showing a transient "RECONNECTING…" overlay. [v1.7.0](https://github.com/chotchki/skylander-portal-controller/releases/tag/v1.7.0): portal control moved from GUI automation to a **patched-upstream RPCS3 over a local IPC socket (Phase 16)** — direct `g_skyportal` control over AF_UNIX, the game boots `--no-gui` straight into play, the **Windows release bundles the patched RPCS3** (point the wizard at your *existing* install for firmware + games), an **"Open in Browser"** QR fallback, `.dump`/`.dmp`/`.bin` figure-dump support, and the **MIT → GPL-2.0-only** relicense.

For a higher-level pitch see the project site: <https://chotchki.github.io/skylander-portal-controller/>. Source-of-truth docs are in this repo: `SPEC.md` (long-form spec + Q&A), `PLAN.md` (execution checklist), and `CLAUDE.md` (compact working reference). Research writeups are under `docs/research/`.

## Releases (end-user install)

1. Grab the latest **MSI** (preferred — handles upgrades cleanly) or the portable **zip** from <https://github.com/chotchki/skylander-portal-controller/releases>. Both ship the same binary; the MSI installs to `Program Files`, the zip is unpack-anywhere.
2. Bring your own RPCS3 install (see <https://rpcs3.net>) and your own firmware-pack of `.sky` dumps.
3. Launch `skylander-portal-controller.exe`. First-run wizard asks for the RPCS3 path and the firmware-pack root; settings persist to `%APPDATA%\skylander-portal-controller\`.
4. Add the resulting `.exe` to Steam (Add a Non-Steam Game) so it launches in Big Picture mode.
5. **(Optional) Steam library artwork.** Non-Steam games show a blank library card. The install ships a first-pass artwork set next to the app in `steam/` (`library_600x900.png` grid, `library_hero_1920x620.png`, `logo.png`, `library_header_920x430.png`). In Steam, right-click the game → **Manage → Set Custom Artwork** for the grid/portrait, and on the game's library page right-click the hero/logo to set those too. (Steam can't apply non-Steam artwork automatically, so this is a one-time manual step.)

The phone bundle is embedded in the binary; no separate web server, no node, no extra files to ship. Per tag we ship **Windows x86_64** zip + MSI (the production target — patched-RPCS3 IPC driver). **The macOS arm64 build is currently disabled** (2026-05-30): it's mock-driver-only (no real RPCS3) and the lane kept failing while burning 10× CI minutes, so it's commented out in `release.yml`/`ci.yml` until it's worth maintaining again — the macOS dev path still works locally (`SKYLANDER_PORTAL_DRIVER=mock`). (Windows Authenticode signing is tracked but disabled — the winget path in Phase 13 supersedes it for SmartScreen friction.)

## Running in dev

Needs: Rust toolchain (incl. `wasm32-unknown-unknown`), `trunk`. Windows 11 for the UIA driver against a real RPCS3; macOS works with the mock driver (set `SKYLANDER_PORTAL_DRIVER=mock` in `.env.dev`). Full macOS dev bringup steps in `docs/dev/macos-bringup.md`.

1. Copy `.env.dev.example` to `.env.dev` and fill in paths to RPCS3 and your firmware-pack root.
2. Build the phone SPA:
   ```
   cd phone && trunk build
   ```
3. Run the server from the repo root:
   ```
   cargo run
   ```
4. A windowed eframe launcher appears with a QR code + URL. Scan the QR from your phone (must share the LAN). Tap a portal slot → tap a figure → figure loads into RPCS3's emulated portal.

Set `SKYLANDER_PORTAL_DRIVER=mock` in `.env.dev` to swap in the in-memory mock driver (no RPCS3 needed) while iterating on UI.

### `cargo` defaults at the repo root

`Cargo.toml` sets `default-members = ["crates/server"]`, so bare `cargo run` / `cargo build` / `cargo test` / `cargo check` from the repo root operate on **only the server crate**. This is what makes step 3 work without a `--bin` flag (the workspace also contains the one-shot `skylander-wiki-scrape` tool, which would otherwise create bin-ambiguity).

Consequences to keep in mind:

- **Workspace-wide testing requires `--workspace`.** Use `cargo test --workspace` (or `-p <crate>`) when you want to test more than just the server. CI does this on every push, so regressions in indexer/sky-parser/etc. are still caught — but local `cargo test` will silently skip them unless you opt in.
- **The wiki-scrape tool is `-p`-only:** `cargo run -p skylander-wiki-scrape -- …`. It's a one-shot — see `tools/wiki-scrape/README.md`.

## Layout

- `crates/core/` — shared domain types + wire protocol.
- `crates/indexer/` — firmware-pack walker.
- `crates/rpcs3-control/` — `PortalDriver` trait, UIA impl, mock impl.
- `crates/server/` — the binary (Axum + eframe + driver worker).
- `phone/` — Leptos CSR SPA (builds to WASM via trunk).
- `tools/` — one-shot helpers (firmware inventory builder, UIA probe/drive utilities from Phase 1).
- `docs/` — research writeups + aesthetic reference images.

## Tests

```
cargo test --workspace                       # unit + integration
SKYLANDER_PACK_ROOT=… cargo test -p skylander-indexer --test real_pack -- --ignored
RPCS3_SKY_TEST_PATH=… cargo test -p skylander-rpcs3-control --test live -- --ignored
```

The `--ignored` tests require a real firmware pack / interactive RPCS3 and are not run by default.

## License

Copyright © 2026 Christopher Hotchkiss. Licensed under the **GNU General Public License v2.0** (GPL-2.0-only — see `LICENSE`). The project vendors RPCS3 (`vendor/rpcs3`) and carries a patch series against it (`rpcs3-patches/`); RPCS3 is GPLv2, so those patches and the combined work are GPL-2.0 too.

Skylanders characters, images, and trademarks belong to Activision. This project ships no game or firmware content — users supply their own RPCS3 install and firmware backups.
