# Stack Smoke Test (Phase 1 spike 1e)

## Decision

**The stack works.** Axum + eframe coexist cleanly in one binary on Windows; Leptos compiles to WASM via trunk. The pattern is safe to lock in for Phase 2.

## What was verified

### Axum + eframe coexistence (main crate)

- `src/main.rs` spawns a dedicated OS thread that owns a multi-threaded tokio runtime. That thread runs the Axum server. The main thread runs `eframe::run_native`, which takes ownership of the event loop for the duration of the program.
- Shared state between server and UI is `Arc<AtomicUsize>` (WebSocket client count). Simple and lock-free for this smoke. Phase 2 will introduce a richer shared-state pattern (channel + app-state struct) when the portal state machine lands.
- Axum routes under test:
  - `GET /` — embedded HTML (WS ping client).
  - `GET /api/status` — JSON `{ clients, server }`.
  - `GET /ws` — WebSocket echo that increments/decrements the counter on connect/disconnect.
- QR code rendered inside the eframe window by feeding the URL to the `qrcode` crate, upsampling the bit matrix into an `egui::ColorImage`, and loading it as a texture.
- Server auto-binds to the first non-loopback IPv4 via `local_ip_address::local_ip()`, falling back to loopback. This matches the spec's network-picking policy.

**Observed behavior:** on launch, eframe window appears; Axum logs `serving on http://192.168.1.162:8765`; `curl http://192.168.1.162:8765/api/status` returns the expected JSON; the HTML client opens a WebSocket and round-trips messages. No deadlocks, no tokio-runtime panics, no UI stalls from the server thread.

### Leptos → WASM build (tools/phone-smoke/)

- Standalone crate at `tools/phone-smoke/`, isolated from the root workspace via its own `[workspace]` marker.
- Leptos 0.7 with `csr` feature. `wasm-bindgen` (0.2) + `web-sys` for WebSocket access.
- `trunk build` succeeded. Trunk auto-downloaded `wasm-bindgen-cli` on first build.
- Output in `tools/phone-smoke/dist/`:
  - `index.html` (1.6 KB, post-processed — trunk injects the JS loader)
  - `phone-smoke-<hash>.js` (31 KB)
  - `phone-smoke-<hash>_bg.wasm` (768 KB **debug** build — release build with `opt-level = "z"` + `lto = true` + `wasm-opt -Oz` will be materially smaller)
- Component mounts to `<body>`, connects WebSocket to `/ws` on the same host/scheme, provides a "send ping" button and a log of outgoing/incoming messages.

**Not yet wired to the server** on purpose. Serving the SPA is a one-liner with `tower_http::services::ServeDir::new("tools/phone-smoke/dist")`, but pulling it into the spike would conflate "can the stack coexist" (yes) with "what's the final SPA serving strategy" (decide in Phase 2).

## Deps added to the main crate (spike-specific; to be pruned)

```toml
axum = { version = "0.7", features = ["ws"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["fs", "trace"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
futures-util = "0.3"
eframe = { version = "0.29", default-features = false, features = ["default_fonts", "glow", "wayland", "x11"] }
egui = "0.29"
qrcode = "0.14"
image = { version = "0.25", default-features = false, features = ["png"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
local-ip-address = "0.6"
```

`wayland` and `x11` eframe features are harmless on Windows (no-op) and keep the crate portable if we ever cross-compile. Drop them if binary size becomes a concern.

## Recommended Phase 2 layout

When we switch from spike to real structure, split into a Cargo workspace:

```
skylander-portal-controller/
├── Cargo.toml              # [workspace] root, no package
├── crates/
│   ├── server/             # binary: Axum + eframe (current src/main.rs evolves here)
│   ├── core/               # library: portal state, figure model, RPCS3 trait
│   ├── indexer/            # library: firmware pack walker (promotes tools/inventory)
│   └── rpcs3-control/      # library: UI Automation driver
├── phone/                  # Leptos SPA (promotes tools/phone-smoke)
│   ├── Cargo.toml          # its own workspace
│   └── Trunk.toml          # dist copied into server's embedded assets at build time
└── tools/
    ├── wiki-scrape/        # one-shot scraper
    └── ...
```

Build pipeline for the server binary:
1. `cd phone && trunk build --release` → `phone/dist/`.
2. Server's `build.rs` or a `include_dir!` macro captures `phone/dist/` at compile time.
3. Axum's `ServeDir` (or a static-handler that reads the embedded tree) serves the SPA at `/`.
4. Dev mode: feature-flag a live `ServeDir::new("phone/dist")` so the server picks up new builds without rebuilding itself.

## Risks (resolved / open)

- **R-stack-1 (resolved):** Axum's tokio runtime can coexist with eframe's winit/glow event loop — verified.
- **R-stack-2 (resolved):** Leptos builds to wasm32-unknown-unknown on this machine — verified.
- **R-stack-3 (open, low):** Binary size after including the WASM bundle + all assets may grow noticeably. Mitigation: measure at Phase 2 MVP; switch to lazy/remote loading only if it becomes a problem.
- **R-stack-4 (open, low):** Leptos 0.7 is the latest major; API churn has historically been brisk. Mitigation: pin minor version in `Cargo.toml`; re-evaluate when we have material SPA code.

## How to reproduce

```bash
# main crate (Axum + eframe spike)
cargo run

# phone smoke (Leptos WASM build)
cd tools/phone-smoke
trunk build
# artifacts land in tools/phone-smoke/dist/
```

For the eframe window the user's Windows desktop is required (remote desktop fine). For `cargo run` in headless CI, eframe will fail — wrap the eframe launch in a feature flag before we care about CI.
