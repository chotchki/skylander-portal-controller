# skylander-portal-controller

Remote-control the RPCS3 emulated Skylanders portal from a phone or iPad over your local Wi-Fi. Family-safe alternative to a physical Skylanders portal where the kids' save data lives somewhere safer than a pile of plastic figures on the living-room floor.

The Windows app boots from Steam Big Picture and shows a QR code on the TV; phones scan in, pick a profile (PIN-gated), pick a game, and drive RPCS3's emulated portal slot-by-slot.

**Latest release:** [v1.8.0](https://github.com/chotchki/skylander-portal-controller/releases/tag/v1.8.0) — **automatic crash/freeze recovery (Phase 16.7).** The controller now supervises RPCS3 and self-heals: it detects both a hard crash (process exit) and a *freeze* (the patched emulator's heartbeat frame counter stalling while it still reports "running" — certain games love to hang), then auto-recovers without anyone touching the TV. It covers the screen, kills the dead/hung emulator, relaunches the **same** game, and **re-places the figures that were on the portal** in their original slots with ownership preserved. Phones show a transient **"RECONNECTING…"** overlay during recovery instead of a dead-end crash screen, and dismiss it automatically when the game is back; the terminal "GAME CRASHED" screen now only appears if every restart attempt fails. Built on the v1.7.0 IPC architecture (patched-upstream RPCS3 over a local AF_UNIX socket).

Earlier headline — [v1.7.0](https://github.com/chotchki/skylander-portal-controller/releases/tag/v1.7.0): portal control moved from GUI automation to a **patched-upstream RPCS3 over a local IPC socket (Phase 16)** — direct `g_skyportal` control over AF_UNIX (no Skylanders Manager dialog, no window-title scraping, no boot/reveal flicker), the game boots `--no-gui` straight into play, the **Windows release bundles the patched RPCS3** (first-launch wizard points at your *existing* install for firmware + games), an **"Open in Browser"** QR fallback on the launcher, `.dump` / `.dmp` / `.bin` figure-dump support, and the **MIT → GPL-2.0-only** relicense (RPCS3 is GPLv2; the IPC patches are a derivative work).

For a higher-level pitch see the project site: <https://chotchki.github.io/skylander-portal-controller/>. Source-of-truth docs are in this repo: `SPEC.md` (long-form spec + Q&A), `PLAN.md` (execution checklist), and `CLAUDE.md` (compact working reference). Research writeups are under `docs/research/`.

## Releases (end-user install)

1. Grab the latest **MSI** (preferred — handles upgrades cleanly) or the portable **zip** from <https://github.com/chotchki/skylander-portal-controller/releases>. Both ship the same binary; the MSI installs to `Program Files`, the zip is unpack-anywhere.
2. Bring your own RPCS3 install (see <https://rpcs3.net>) and your own firmware-pack of `.sky` dumps.
3. Launch `skylander-portal-controller.exe`. First-run wizard asks for the RPCS3 path and the firmware-pack root; settings persist to `%APPDATA%\skylander-portal-controller\`.
4. Add the resulting `.exe` to Steam (Add a Non-Steam Game) so it launches in Big Picture mode.

The phone bundle is embedded in the binary; no separate web server, no node, no extra files to ship. Per tag we ship: **Windows x86_64** zip + MSI (production target with the UIA driver against a real RPCS3) and **macOS arm64** tar.gz (production target with the mock driver only — no AXUIElement port, useful for demo / family-member play / dev iteration / the iOS-Simulator e2e harness, but it doesn't drive a real RPCS3). The macOS binary isn't notarised; right-click → Open the first time to clear Gatekeeper. (Both Windows Authenticode signing and macOS Developer ID notarisation are tracked but currently disabled — the winget distribution path in Phase 13 supersedes Authenticode for SmartScreen friction; macOS Tier-2 signing is waiting on cert provisioning.)

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
