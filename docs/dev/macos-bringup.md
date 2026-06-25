# macOS dev bringup

Contributor notes for running the server + the phone SPA on macOS,
against the mock RPCS3 driver. Mac is a supported production target
(see PLAN 10.6 release artifact) but only the mock driver works there
— there's no AXUIElement-based equivalent of the Windows UIA driver,
so a Mac binary can't talk to a real RPCS3. For development, the
iOS-Simulator e2e harness (PLAN 10.4), and on-device iPhone/iPad
iteration the mock-only constraint isn't a limitation.

## Prereqs

- Xcode + at least one iOS runtime (Settings → Platforms) — needed for
  [`ios-inspect`](https://github.com/chotchki/ios-inspect) and the iOS-sim e2e lane.
- Rust toolchain with `wasm32-unknown-unknown` target:
  ```
  rustup target add wasm32-unknown-unknown
  ```
- [`trunk`](https://trunkrs.dev/) for building the phone SPA:
  ```
  cargo install trunk
  ```
- (Optional) `brew install ios-webkit-debug-proxy` — only needed for
  iOS Simulator iteration via `ios-inspect`.

No PS3/RPCS3 install on Mac. The mock driver replaces the emulator
end-to-end (`SKYLANDER_PORTAL_DRIVER=mock` in `.env.dev`); the
`RPCS3_EXE` env var becomes a sentinel path the server never opens.

## First-time setup

```sh
cp .env.dev.example .env.dev
```

Edit `.env.dev` to:

```
RPCS3_EXE=/tmp/nonexistent-rpcs3-mock-mode
FIRMWARE_PACK_ROOT=/Users/<you>/workspace/skylander-portal-controller/dev-data/firmware-pack
SKYLANDER_PORTAL_DRIVER=mock
BIND_PORT=8765
```

`RPCS3_EXE` is required-by-shape but never opened under the mock driver.
`FIRMWARE_PACK_ROOT` should point at a directory containing `.sky` files
in the `{Game}/{Element}/` layout (see `docs/research/firmware-inventory.md`);
without it the library is empty and the SPA shows the empty-state copy.

Build the phone SPA at least once so `phone/dist/index.html` exists:

```sh
cd phone && trunk build && cd ..
```

For iterative phone work, prefer `cd phone && trunk serve --address 0.0.0.0 --port 8090`
in a sibling terminal — the server's `phone_dist` reads from disk in
debug builds, so changes show up on a phone reload.

## Run the server

```sh
cargo run -p skylander-server
```

Expected log lines on a clean boot:

```
INFO starting server rpcs3=/tmp/nonexistent-rpcs3-mock-mode pack=… port=8765 driver=Mock
INFO indexed pack figures count=504
INFO phone URL http://<your-localhostname>.local:8765/?k=…
INFO RPCS3 lifecycle ready driver_kind=Mock
INFO loaded Skylanders game catalogue from RPCS3 library installed=6 enumerated=6
INFO serving on http://192.168.1.x:8765
```

The eframe launcher window opens fullscreen on a new Space (Mac
fullscreen is per-Space; swipe with three fingers or `Ctrl-←/→` to
switch back). The `phone URL` line uses the Bonjour-published
`<LocalHostName>.local` (read via `scutil --get LocalHostName`); on
the very rare Mac with a misconfigured Bonjour daemon the line drops
to a raw-IP URL warning instead — both work, only the IP form fails
to survive DHCP-lease changes for pinned PWA bookmarks.

## Connect from a browser

The server binds to the en0 IP, not loopback — connect via the IP
shown in the `serving on` log line:

```sh
open "http://$(ipconfig getifaddr en0):8765/"
```

`http://127.0.0.1:8765` will not respond — that's by design (the
launcher is for phones on the LAN, not the host machine). For probing
endpoints from the same Mac, use the en0 IP.

## Connect from a phone

- Same Wi-Fi network as the Mac.
- Scan the QR shown in the launcher window, or open the `serving on`
  URL directly in Mobile Safari.
- The `?k=<hex>` query param is the HMAC key — phone PWA bookmarks
  preserve it (PLAN 4.18.1a fragment-vs-query decision).

## Connect from the iOS Simulator

Install the CLI with `cargo install --git https://github.com/chotchki/ios-inspect --tag v0.0.1`;
see [its README](https://github.com/chotchki/ios-inspect) for the simulator-driven dev loop.
The simulator and the server share the host's network stack, so
`http://<mac-en0-ip>:8765/` works the same as on a real phone. PWA
standalone mode (Add-to-Home-Screen) inside the sim is the only way
to get a non-zero `safe-area-inset-bottom` for layout work.

## Known caveats

- `force_topmost_via_win32` (`crates/server/src/ui/mod.rs`) is a no-op
  on Mac — eframe windows can be hidden behind other apps. Cmd-Tab
  back to the launcher to bring it forward.
- The mock driver returns a fixed 6-game catalogue (matching the
  `SKYLANDERS_SERIALS` table in `crates/core`). Add or remove
  installed games via the catalogue source, not via the runtime
  `.env.dev`.
- Logs go to `./logs/` next to the working directory, not
  `~/Library/Logs/...` (dev-mode behaviour, mirrors the Windows dev
  layout). Release builds on Mac are not a supported configuration.
