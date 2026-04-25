# skylander-portal-controller

Remote-control the RPCS3 emulated Skylanders portal from a phone or iPad over your local Wi-Fi. Family-safe alternative to a physical Skylanders portal where the kids' save data lives somewhere safer than a pile of plastic figures on the living-room floor.

The Windows app boots from Steam Big Picture and shows a QR code on the TV; phones scan in, pick a profile (PIN-gated), pick a game, and drive RPCS3's emulated portal slot-by-slot.

**Latest release:** [v1.1.0](https://github.com/chotchki/skylander-portal-controller/releases/tag/v1.1.0) — sticky disconnects (figures stay on the portal across PWA backgrounding / brief network blips), Kaos mid-game swap (per-profile opt-in), kickback cooldown countdown, kid-friendly empty-portal UI.

For a higher-level pitch see the project site: <https://chotchki.github.io/skylander-portal-controller/>. Source-of-truth docs are in this repo: `SPEC.md` (long-form spec + Q&A), `PLAN.md` (execution checklist), and `CLAUDE.md` (compact working reference). Research writeups are under `docs/research/`.

## Releases (end-user install)

1. Grab the latest `.zip` from <https://github.com/chotchki/skylander-portal-controller/releases>.
2. Unzip somewhere persistent (Steam library, `Documents\…`, etc.).
3. Bring your own RPCS3 install (see <https://rpcs3.net>) and your own firmware-pack of `.sky` dumps.
4. Launch `skylander-portal-controller.exe`. First-run wizard asks for the RPCS3 path and the firmware-pack root; settings persist to `%APPDATA%\skylander-portal-controller\`.
5. Add the resulting `.exe` to Steam (Add a Non-Steam Game) so it launches in Big Picture mode.

The phone bundle is embedded in the exe; no separate web server, no node, no extra files to ship. Only Windows 11 x64 builds are published — the project uses Windows-only UIA + Win32 plumbing for the RPCS3 driver and isn't portable.

## Running in dev

Needs: Rust toolchain (incl. `wasm32-unknown-unknown`), `trunk`, Windows 11 for the UIA driver. Mock driver works on any platform.

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

Public domain. Skylanders characters, images, and trademarks belong to Activision. This project ships no game or firmware content — users supply their own RPCS3 install and firmware backups.
