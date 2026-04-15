# skylander-portal-controller

Remote-control the RPCS3 emulated Skylanders portal from a phone or iPad over your local Wi-Fi.

**Status:** Phase 2 MVP — portal control end-to-end with a phone SPA. Profiles, game launching, wiki-backed figure metadata, and the Skylanders aesthetic pass are Phase 3+.

See `SPEC.md` for the long-form requirements, `PLAN.md` for the current execution checklist, and `CLAUDE.md` for a compact working reference. Research writeups from Phase 1 are under `docs/research/`.

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
