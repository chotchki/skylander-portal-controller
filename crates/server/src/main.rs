// Release builds are a pure GUI app (eframe launcher + first-launch wizard),
// so suppress the console window Windows otherwise spawns alongside the egui
// surface — logs go to files (`logging::init`), not stdout. Gated to the
// production build: `dev-tools` (on for `cargo run`) keeps the console so dev
// iteration still sees tracing output. The release lane builds with
// `--no-default-features` (no `dev-tools`), so the attribute takes effect there.
#![cfg_attr(not(feature = "dev-tools"), windows_subsystem = "windows")]
//! Skylander Portal Controller — entry point.
//!
//! Threading model:
//!  - Main OS thread owns the eframe event loop.
//!  - A dedicated background OS thread hosts the tokio multi-threaded runtime
//!    running the Axum server + the driver worker task.
//!  - Shared state lives inside `Arc<AppState>` and an `AtomicUsize` client
//!    counter that both sides read.

use skylander_server::{config, gl_fallback, logging, ui};

use std::sync::Arc;

use anyhow::{Context, Result};
use guac_server::boot::{BootOptions, BuiltDriver, Catalogue, boot};
use tracing::info;

use crate::config::DriverKind;
use crate::ui::LauncherApp;

fn main() -> Result<()> {
    // Decide the OpenGL backend BEFORE anything else — `config::load()` can
    // pop the wizard window and the server binds a port; both must happen on
    // the right GL stack. On a GPU-less box (winget validation VM, headless
    // RDP) this RE-EXECS into the bundled Mesa software renderer (and the
    // parent then exits), so the window actually appears instead of failing
    // silently. PLAN 19. Runs before `logging::init` (which needs
    // `cfg.log_dir`), so its decision is logged just below once the
    // subscriber is up; if it re-execs, this call never returns.
    let gl = gl_fallback::bootstrap();

    // `--reconfigure` re-runs the first-launch wizard even when config.json
    // already exists (release builds only — dev reads `.env.dev` and ignores
    // it). Without it, re-running setup means hand-deleting config.json, a real
    // gap on the shipped Mac/Windows app where there's no terminal in reach
    // (A.8.10).
    let reconfigure = std::env::args().any(|a| a == "--reconfigure");
    let cfg = config::load(gl.software, reconfigure).context("load config")?;
    let _log_guard = logging::init(&cfg.log_dir)?;

    info!(gl = %gl.detail, software = gl.software, "graphics backend selected");
    info!(
        rpcs3 = %cfg.rpcs3_exe.display(),
        pack = %cfg.firmware_pack_root.display(),
        port = cfg.bind_port,
        driver = ?cfg.driver_kind,
        "starting server",
    );

    // PLAN S: exported before the tokio runtime exists so the patched RPCS3 child inherits it (2560×1440).
    if cfg.render_2x {
        // SAFETY: startup is still single-threaded here — no other thread reads or
        // writes the process env yet (the tokio runtime + RPCS3 spawn come later).
        unsafe { std::env::set_var("SKYLANDER_SURFACE_2X", "1") };
    }

    // The shared server's startup (openspec AA.2): pack index, phone URL + QR,
    // then the tokio thread (bind, driver, db, worker, watchdogs, serve).
    let server = boot(BootOptions {
        config: cfg.clone(),
        runtime_root: skylander_server::paths::runtime_dir_unchecked()?,
        migrator: &skylander_server::MIGRATOR,
        driver_factory: Box::new(build_driver),
        // The product's four `guac_server::hooks` (figure files, display mode,
        // RPCS3 config, settings writer): the product-only code the shared server calls.
        hooks: skylander_server::product_hooks::hooks(),
        extra_routes: None,
        catalogue: Catalogue::FirmwarePack {
            scan: skylander_indexer::scan,
        },
        bind_ip: None,
    })?;

    // --- Fullscreen eframe window on the main thread. ---
    let ui_clients = server.connected_clients.clone();
    let ui_status = server.launcher_status.clone();
    // Both dev and release fullscreen on launch — same visual model, so
    // what you see at `cargo run` is what the HTPC user gets. Release
    // additionally pins always-on-top (Steam Big Picture invocation,
    // game viewport must not peek through); dev skips that so alt-tab
    // works during iteration. Either way the SHUT DOWN button on the
    // phone (4.15.11) is the supported escape — no need for a windowed
    // dev exception now that the kill path is reliable. Transparency is
    // always on so the in-game surface (PLAN 4.15.8) can render a
    // reconnect QR overlay with the game visible through the viewport.
    // PLAN 20.3: Desktop window mode (chosen in the wizard / persisted in config,
    // or `.env.dev` WINDOW_MODE=desktop in dev) makes the launcher a normal
    // resizable window; TV mode stays fullscreen + (release) always-on-top.
    let windowed = matches!(cfg.window_mode, config::WindowMode::Desktop);
    let viewport = {
        let mut vb = egui::ViewportBuilder::default().with_title("Skylander Portal Controller");
        if windowed {
            vb = vb
                .with_inner_size([1100.0, 760.0])
                .with_min_inner_size([640.0, 680.0])
                .with_resizable(true);
        } else {
            vb = vb.with_fullscreen(true);
            if !cfg!(feature = "dev-tools") {
                vb = vb.with_always_on_top();
            }
        }
        // Transparent in every mode. The panel paints an opaque starfield in
        // Main / Crashed / Farewell, so only the in-game path sees through to
        // RPCS3 behind egui — Desktop mode (20.4) needs that too.
        vb = vb.with_transparent(true);
        // Window icon — same gold/Kaos asset the phone PWA pins to the
        // home screen. Without this Windows shows the eframe default
        // "egui e" placeholder in the taskbar / alt-tab. Same
        // `debug_assertions` gate as the favicon swap so dev runs are
        // visually distinct from `cargo run --release`.
        if let Some(icon) = load_window_icon() {
            vb = vb.with_icon(icon);
        }
        vb
    };
    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let url_for_ui = server.phone_url.clone();
    let raw_ip_url_for_ui = server.raw_ip_url.clone();
    let ui_bind_port = server.bind_addr.port();
    let ui_window_mode = cfg.window_mode;
    // PLAN B.2 — the worker's sender, so the launcher can dispatch the macOS
    // Desktop-mode game-window fit (closed if the server died at startup).
    let ui_driver_tx = server.driver_tx.clone();
    eframe::run_native(
        "skylander-portal-controller",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(LauncherApp::new(
                cc,
                ui_clients,
                ui_status,
                url_for_ui,
                raw_ip_url_for_ui,
                ui_bind_port,
                ui_window_mode,
                ui_driver_tx,
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))
}

/// The product's [`guac_server::boot::DriverFactory`], run on the tokio thread.
fn build_driver(kind: DriverKind) -> Result<BuiltDriver> {
    match kind {
        DriverKind::Uia => {
            #[cfg(windows)]
            {
                let d = skylander_rpcs3_control::UiaPortalDriver::new()?;
                Ok(BuiltDriver::new(Arc::new(d)))
            }
            #[cfg(not(windows))]
            anyhow::bail!("UIA driver only available on Windows");
        }
        DriverKind::Ipc => {
            // Patched-RPCS3 IPC driver (PLAN 16.5) — cross-platform (no #[cfg(windows)]).
            // It is the production portal path for Phase 16; the no-GUI launch that
            // gives it a socket to talk to lands in 16.6.
            let d = skylander_rpcs3_control::IpcPortalDriver::new()?;
            Ok(BuiltDriver::new(Arc::new(d)))
        }
        DriverKind::Mock => {
            // Available under EITHER dev-tools (the standard
            // contributor build) OR mock-driver-runtime (the macOS
            // production build that has no UIA path). Gating Mock
            // behind dev-tools alone would have made the Mac release
            // binary refuse to start (PLAN 10.6).
            #[cfg(any(feature = "dev-tools", feature = "mock-driver-runtime"))]
            {
                let mock = Arc::new(skylander_rpcs3_control::MockPortalDriver::new());
                #[cfg(feature = "test-hooks")]
                return Ok(BuiltDriver::with_test_mock(mock));
                // The `return` on the branch below keeps the code symmetric
                // with the `test-hooks` branch above and sidesteps a cfg-type
                // mismatch that would otherwise require an outer `match`
                // rebind. Narrow allow instead of restructuring.
                #[cfg(not(feature = "test-hooks"))]
                #[allow(clippy::needless_return)]
                {
                    return Ok(BuiltDriver::new(mock));
                }
            }
            #[cfg(not(any(feature = "dev-tools", feature = "mock-driver-runtime")))]
            anyhow::bail!(
                "mock driver only available with the dev-tools or mock-driver-runtime feature"
            );
        }
    }
}

/// Decode the embedded PWA icon PNG into the raw-RGBA form eframe wants
/// for the window's taskbar / title-bar / alt-tab icon. The 192px size
/// is the Android PWA standard — large enough that Windows downscales
/// cleanly to the 16/24/32px sizes the OS shell uses, small enough to
/// embed without bloating the binary. Same `debug_assertions` gate as
/// the favicon (`crates/guac-server/src/http.rs` `dev_swapped`) so dev and
/// release builds are visually distinct in the taskbar too.
///
/// PNG bytes are baked at compile time so a missing `phone/assets/icons/`
/// directory is a compile error, not a runtime fallthrough — there's no
/// way to silently end up with the eframe default "egui e" icon again
/// once this lands.
fn load_window_icon() -> Option<egui::IconData> {
    const PROD: &[u8] = include_bytes!("../../../phone/assets/icons/icon-192.png");
    const DEV: &[u8] = include_bytes!("../../../phone/assets/icons/icon-dev-192.png");
    let bytes: &[u8] = if cfg!(debug_assertions) { DEV } else { PROD };
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some(egui::IconData {
        rgba: rgba.into_raw(),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode-roundtrip pin: the embedded PNG bytes must yield a
    /// 192×192 RGBA buffer. If `icon-bake` is rerun and accidentally
    /// resizes (or if someone deletes the file), this test catches it
    /// at `cargo test` time instead of waiting until launcher start.
    #[test]
    fn window_icon_decodes_to_expected_dimensions() {
        let icon = load_window_icon().expect("window icon should decode");
        assert_eq!(icon.width, 192, "icon width should be 192px");
        assert_eq!(icon.height, 192, "icon height should be 192px");
        // 4 bytes per pixel (RGBA).
        assert_eq!(
            icon.rgba.len(),
            (icon.width * icon.height * 4) as usize,
            "rgba buffer size should match width × height × 4"
        );
    }
}
