# skylander-portal-controller

Remote-control the RPCS3 emulated Skylanders portal from a phone or iPad over your local Wi-Fi. Family-safe alternative to a physical Skylanders portal where the kids' save data lives somewhere safer than a pile of plastic figures on the living-room floor.

The Windows app boots from Steam Big Picture and shows a QR code on the TV; phones scan in, pick a profile (PIN-gated), pick a game, and drive RPCS3's emulated portal slot-by-slot.

**Latest release:** [v1.9.8](https://github.com/chotchki/skylander-portal-controller/releases/tag/v1.9.8) — **installs on a clean PC + a resizable desktop window mode.** Two things: (1) The app now **runs on a PC that has never had the Visual C++ runtime installed** — it previously failed to launch with a missing-DLL error (`0xC0000135`) on a clean machine; the required runtime now **ships alongside the app** (this is the winget-validation fix). (2) New **Desktop window mode**: a first-launch setup choice between the living-room / TV experience (fullscreen, phone-driven) and a **resizable desktop window** for use at a desk — the emulator window fits inside the launcher and tracks it as you move/resize. **Your figure progress is untouched.** Everything below is unchanged from v1.9.7.

[v1.9.7](https://github.com/chotchki/skylander-portal-controller/releases/tag/v1.9.7) — **runs on more PCs + a no-emulator demo mode.** Two additions: (1) **The launcher now starts even on a PC with no dedicated graphics card** — or a locked-down / remote-desktop setup — where it previously opened *nothing at all*. It quietly falls back to a built-in **software renderer** so the setup screen and launcher always appear. (2) New **demo mode**: try the whole phone-and-TV interface with a built-in **dummy portal** — no RPCS3, firmware, or games needed. Tap **"TRY DEMO MODE"** on the first-launch setup screen (it offers this automatically on a PC without a graphics card), or start the app with the `SKYLANDER_PORTAL_DRIVER=mock` environment variable for a scripted / recorded run. Actually launching games and driving a *real* portal still needs a graphics card plus your own RPCS3 install. **Your figure progress is untouched.** Everything below is unchanged from v1.9.6.

[v1.9.6](https://github.com/chotchki/skylander-portal-controller/releases/tag/v1.9.6) — **stability: games stop freezing.** The big one: Skylanders games (especially *Spyro's Adventure*) no longer **freeze or "flap"** — figures dropping off the portal and popping back every couple of minutes. The cause was the emulator's **network setting**: with networking off, the game hammered a dead network probe until it pinned the CPU and the watchdog had to restart it. The app now sets the one **proven-stable network configuration** on your existing RPCS3 install automatically, so games connect cleanly and just play. And if a game *does* still crash, the launcher now **auto-recovers** RPCS3's own *frozen* state — a ~15-second reconnect instead of a dead screen — where before it only caught a fully-dead emulator, not a frozen-but-alive one. Also: removed a **Windows Firewall prompt** that could stall the emulator on first launch, and the **Open in Browser / Exit to Desktop** buttons now sit side-by-side so they clear the TV's cropped bottom edge. Includes v1.9.5's art refresh. **Your figure progress is preserved** — saved levels / gold / Imaginators live in per-profile working copies these changes never touch. Everything below is unchanged from v1.9.0.

[v1.9.5](https://github.com/chotchki/skylander-portal-controller/releases/tag/v1.9.5) — **art refresh.** New logo, app/portal icon, and launcher + Steam library artwork (family-contributed). Cosmetic only — no behaviour change.

[v1.9.4](https://github.com/chotchki/skylander-portal-controller/releases/tag/v1.9.4) — **launch fixes.** Three issues from v1.9.3: (1) the bundled emulator now finds your installed PS3 **firmware** — it was looking one folder too high (a trailing-slash quirk in how RPCS3 reads its config dir) and falling into RPCS3's first-run "welcome" setup screen; (2) the launcher no longer opens a stray **terminal window** beside itself (release builds are now a GUI-subsystem app); (3) the **"Open in Browser"** connect-from-PC button is visible on the TV again — it was being pushed into the overscan-cropped bottom edge. Builds on v1.9.3's upgrade fix (figures not loading onto the portal): the app self-corrects a *pre-IPC* config on every launch, driving the **bundled patched RPCS3 over IPC** while reading firmware + games from your existing RPCS3 install.

[v1.9.3](https://github.com/chotchki/skylander-portal-controller/releases/tag/v1.9.3) — **upgrade fix: figures not loading onto the portal.** On a PC that already had an older version installed, the first-launch setup is skipped (your config is already there) — but a config written by a *pre-IPC* build pinned the app to the **legacy GUI-automation driver pointed at your *stock* RPCS3**, which has no IPC listener and so never loads figures onto the emulated portal. The app now self-corrects on every launch: it drives the **bundled patched RPCS3 over IPC** regardless of what an old config said, while still reading firmware + games from your existing RPCS3 install.

[v1.9.2](https://github.com/chotchki/skylander-portal-controller/releases/tag/v1.9.2) — **install polish.** Start Menu / Desktop shortcut icons now render (the MSI's advertised shortcuts were missing an explicit icon reference), and the install ships a first-pass **Steam library artwork** set in `steam/` (grid capsule, hero, logo, header) — apply it via Steam's right-click → Manage → Set Custom Artwork. The macOS build is paused (Windows-only artifacts for now).

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
