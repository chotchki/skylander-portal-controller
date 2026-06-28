//! Runtime configuration.
//!
//! - Dev builds (`dev-tools` feature, default): read `.env.dev` at startup.
//! - Release builds: read `%APPDATA%/skylander-portal-controller/config.json`,
//!   or kick off the first-launch egui wizard when that file is missing
//!   (see `crate::wizard`).

#[cfg(feature = "dev-tools")]
use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(feature = "dev-tools")]
use anyhow::Context;
use anyhow::Result;
use serde::{Deserialize, Serialize};
#[cfg(not(feature = "dev-tools"))]
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[allow(dead_code)] // used for game launching in Phase 3
    pub rpcs3_exe: PathBuf,
    /// RPCS3 data/config root (holds `config/games.yml`, firmware, dev_hdd0).
    /// Defaults to `rpcs3_exe.parent()`; set independently via `RPCS3_CONFIG_DIR`
    /// when the patched binary lives apart from its config (Phase 16). Used to read
    /// `games.yml` and passed to the patched RPCS3 as `RPCS3_CONFIG_DIR`.
    pub config_dir: PathBuf,
    pub firmware_pack_root: PathBuf,
    pub bind_port: u16,
    pub driver_kind: DriverKind,
    /// Launcher window presentation (PLAN 20) — TV (fullscreen) vs Desktop
    /// (resizable window). Consumed by `main.rs` for the eframe viewport.
    pub window_mode: WindowMode,
    /// 2× (1440p) render pass for the macOS surface-embed (PLAN S). When true the
    /// launcher sets `SKYLANDER_SURFACE_2X` so the patched RPCS3 renders 2560×1440.
    /// Konami-gated admin toggle; applies on next launcher boot.
    pub render_2x: bool,
    /// Directory where the log file(s) live. Differs dev vs release.
    pub log_dir: PathBuf,
    /// Directory containing the phone SPA's built assets.
    pub phone_dist_dir: PathBuf,
    /// Root of committed static data bundles: `images/<figure_id>/{hero,thumb}.png`,
    /// `figures.json`, `figures.manual.json`. Defaults to `<repo>/data/`.
    pub data_root: PathBuf,
    /// 32-byte HMAC-SHA256 key shared with the phone via the TV's QR fragment
    /// (`?k=<hex>` query param). Every mutating REST request carries an HMAC + timestamp
    /// header computed with this key (PLAN 3.13). Stable across restarts —
    /// regenerating invalidates any phone that still has the old QR cached.
    #[serde(with = "hex_key")]
    pub hmac_key: Vec<u8>,
}

/// Serde helper: persist `hmac_key` as a hex string in `config.json`.
mod hex_key {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        hex::decode(&s).map_err(serde::de::Error::custom)
    }
}

/// Generate a fresh 32-byte HMAC key using the OS RNG. Called once at
/// first-launch (dev or release) if the persisted config doesn't have one.
pub fn generate_hmac_key() -> Vec<u8> {
    use rand_core::{OsRng, RngCore};
    let mut key = vec![0u8; 32];
    OsRng.fill_bytes(&mut key);
    key
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // Mock only exists under `dev-tools`; kept for config-file round-trip
pub enum DriverKind {
    /// Legacy GUI-automation driver (UI Automation against the Skylanders Manager
    /// dialog). Demoted to a fallback in Phase 16 (16.5.2.3) — opt-in via
    /// `SKYLANDER_PORTAL_DRIVER=uia`, kept for dev against a *stock* RPCS3.
    Uia,
    /// Patched-RPCS3 IPC driver (Phase 16) — cross-platform AF_UNIX portal control,
    /// no GUI/dialog. **The production default on Windows** (16.5.2.3); the no-GUI
    /// launch + window coordination (PLAN 16.6) is proven on the HTPC.
    Ipc,
    Mock,
}

/// How the launcher presents its window (PLAN 20). Chosen once in the
/// first-launch wizard, persisted in `config.json`, default [`WindowMode::Tv`]
/// so existing installs keep the fullscreen living-room behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    /// Living-room / Steam Big Picture: fullscreen launcher overlaying RPCS3,
    /// the family drives the portal from phones. The original experience.
    #[default]
    Tv,
    /// Desktop app: the launcher is a normal resizable window that still sits
    /// above the emulator (PLAN 20.4). For a user at a desk; portal control via
    /// a browser on the same PC.
    Desktop,
}

/// `software_gl` is the GPU-less signal from [`crate::gl_fallback`] — release
/// builds use it to steer the wizard into demo mode. Dev builds read paths from
/// `.env.dev` and never show the wizard, so they ignore it. `reconfigure`
/// (the `--reconfigure` flag) forces the wizard even when config.json exists
/// (release only; dev ignores it for the same reason it ignores the wizard).
#[cfg(feature = "dev-tools")]
pub fn load(_software_gl: bool, _reconfigure: bool) -> Result<Config> {
    let env = read_env_file(".env.dev").unwrap_or_default();

    let rpcs3_exe = require_path(&env, "RPCS3_EXE")?;
    // `FIRMWARE_PACK_ROOT` is now optional (PLAN 6.5.4): reader-only users
    // don't need a pack, and a zero-collection boot is valid (Imaginators
    // "instant Skylander" flow, for one). Empty / unset collapses to
    // PathBuf::new(); `skylander_indexer::scan()` already returns Ok(vec![])
    // for a missing root, so main.rs's boot path is safe without changes.
    let firmware_pack_root = env
        .get("FIRMWARE_PACK_ROOT")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_default();

    let bind_port: u16 = env
        .get("BIND_PORT")
        .map(|s| s.parse())
        .transpose()
        .context("BIND_PORT must be an integer")?
        .unwrap_or(8765);

    let driver_kind = match env.get("SKYLANDER_PORTAL_DRIVER").map(String::as_str) {
        Some("mock") => DriverKind::Mock,
        Some("ipc") => DriverKind::Ipc,
        Some("uia") => DriverKind::Uia,
        // Phase 16 / Phase U: IPC (patched RPCS3 over AF_UNIX) is the production
        // default on BOTH shipping targets — Windows and macOS now bundle + drive
        // the patched binary, so dev `cargo run` mirrors prod. `=mock` (in-memory
        // demo portal) and `=uia` (legacy GUI automation vs a *stock* RPCS3) are
        // explicit opt-ins (`.env.dev` / `.env.dev.mock` set them). Linux isn't a
        // shipping target, so it keeps the Mock default — CI / non-target builds
        // shouldn't try to spawn an emulator they can't build.
        _ => {
            #[cfg(any(windows, target_os = "macos"))]
            {
                DriverKind::Ipc
            }
            #[cfg(not(any(windows, target_os = "macos")))]
            {
                DriverKind::Mock
            }
        }
    };

    let log_dir = PathBuf::from("logs");
    let phone_dist_dir = env
        .get("PHONE_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("phone/dist"));
    let data_root = env
        .get("DATA_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data"));

    // PLAN 20: dev parity for Desktop window mode via `.env.dev` (`WINDOW_MODE=
    // desktop`). Default Tv — `cargo run` stays fullscreen like the HTPC.
    let window_mode = match env.get("WINDOW_MODE").map(|s| s.as_str()) {
        Some("desktop") => WindowMode::Desktop,
        _ => WindowMode::Tv,
    };
    // PLAN S: dev parity for the 2× render pass via `.env.dev` (`RENDER_2X=1`).
    let render_2x = matches!(env.get("RENDER_2X").map(|s| s.as_str()), Some("1" | "true"));

    // HMAC key lives in `./dev-data/hmac.key` so it survives `cargo clean`
    // but regenerates on `rm -rf dev-data/`. Dev mode doesn't push through
    // the full config.json round-trip because `.env.dev` is the source of
    // truth; the key is the one piece of runtime state that can't live in
    // .env.dev without committing secrets.
    let hmac_key = load_or_create_dev_hmac_key()?;

    // Phase 16: the patched RPCS3 can live apart from its data/config root, so
    // RPCS3_CONFIG_DIR is independent. Default to the exe's parent (a normal
    // install keeps `config/` next to the exe).
    let config_dir = env
        .get("RPCS3_CONFIG_DIR")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            rpcs3_exe
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        });

    Ok(Config {
        rpcs3_exe,
        config_dir,
        firmware_pack_root,
        bind_port,
        driver_kind,
        window_mode,
        render_2x,
        log_dir,
        phone_dist_dir,
        data_root,
        hmac_key,
    })
}

#[cfg(feature = "dev-tools")]
fn load_or_create_dev_hmac_key() -> Result<Vec<u8>> {
    let path = PathBuf::from("dev-data").join("hmac.key");
    if let Ok(raw) = std::fs::read_to_string(&path) {
        let decoded = hex::decode(raw.trim())
            .with_context(|| format!("parse hex from {}", path.display()))?;
        if decoded.len() == 32 {
            return Ok(decoded);
        }
        // Wrong length; regenerate.
    }
    let key = generate_hmac_key();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    std::fs::write(&path, hex::encode(&key))
        .with_context(|| format!("write {}", path.display()))?;
    Ok(key)
}

/// Whether the first-launch wizard should run, given we're already past the
/// demo-mode (`force_mock`) short-circuit: there's no persisted config to read,
/// OR `--reconfigure` forces a re-run over an existing one (A.8.10). Pure +
/// ungated so it's unit-tested in the default (dev-tools) lane even though its
/// only caller is the release `load` below.
#[allow(dead_code)] // sole caller is the release (non-dev-tools) `load`
fn wizard_needed(config_exists: bool, reconfigure: bool) -> bool {
    !config_exists || reconfigure
}

#[cfg(not(feature = "dev-tools"))]
pub fn load(software_gl: bool, reconfigure: bool) -> Result<Config> {
    use crate::paths;
    use crate::wizard::{PersistedConfig, PersistedDriverKind};
    use anyhow::Context;
    // U.6: the wizard now runs on macOS too (it bundles the patched RPCS3 +
    // drives IPC, like Windows), so the import is no longer Windows-only.
    use crate::wizard;

    let config_path = paths::config_json_path()?;

    // Explicit demo-mode override (PLAN 19.2): `SKYLANDER_PORTAL_DRIVER=mock`
    // forces the dummy driver on any machine — no RPCS3, no firmware, no
    // games — for a recorded demo or a quick PC-only look (issue #2). It
    // short-circuits BEFORE reading/writing config.json so it's transient:
    // unset the var and the next launch is back to the real install. The
    // synthesised config already carries an HMAC key, so the block below
    // doesn't persist anything either.
    let force_mock = std::env::var("SKYLANDER_PORTAL_DRIVER")
        .map(|v| v.eq_ignore_ascii_case("mock"))
        .unwrap_or(false);

    let persisted = if force_mock {
        let runtime_dir = paths::resolve_runtime_dir()?;
        PersistedConfig::mock_default(&runtime_dir)
    } else if wizard_needed(config_path.exists(), reconfigure) {
        // Either first launch (no config.json) OR `--reconfigure` forcing a
        // re-run over an existing config (A.8.10). The wizard re-writes
        // config.json from the chosen paths, so a re-run is non-destructive to
        // everything else under the runtime dir.
        let runtime_dir = paths::resolve_runtime_dir()?;
        // U.6: macOS now runs the first-launch wizard too — it bundles the
        // patched RPCS3 and drives IPC, exactly like Windows. The mac wizard
        // prompts for the user's existing RPCS3 *data dir* (config_dir =
        // firmware + games.yml) instead of an rpcs3.exe; portal control uses
        // the bundled nested RPCS3, resolved at runtime. `mock_default` is
        // still reachable via the wizard's DEMO MODE page and the
        // SKYLANDER_PORTAL_DRIVER=mock override above.
        wizard::run_wizard_blocking(&config_path, &runtime_dir, software_gl)?
    } else {
        PersistedConfig::read(&config_path).with_context(|| {
            format!(
                "parse {} — delete it (or pass --reconfigure) to re-run the first-launch wizard",
                config_path.display()
            )
        })?
    };

    // Ensure the persisted config has an HMAC key. The wizard writes a
    // fresh one on first launch; older configs (pre-3.13) won't have the
    // field. `PersistedConfig` keeps `hmac_key` as `Option<Vec<u8>>` with a
    // `#[serde(default)]`, so the `None` case here means a config from a
    // server version before this feature existed — regenerate + persist.
    let hmac_key: Vec<u8> = match persisted.hmac_key {
        Some(k) if k.len() == 32 => k,
        _ => {
            let k = generate_hmac_key();
            let mut updated = persisted.clone();
            updated.hmac_key = Some(k.clone());
            updated.write(&config_path)?;
            k
        }
    };

    // PLAN 10.8.4: data_root + phone_dist_dir are derived from the
    // binary location, not user preferences. Always recompute from
    // `current_exe()` on launch so an MSI upgrade or relocation
    // (e.g. "C:\emuluators\portal_controller\" → "C:\Program Files\
    // Skylander Portal Controller\") doesn't leave the persisted
    // config pointing at the old install dir. The persisted values
    // in config.json are kept (write_through) so external tools can
    // still inspect, but the runtime resolution always wins.
    let exe_parent = paths::app_asset_dir();
    let phone_dist_dir = exe_parent.join("phone-dist");
    // PLAN U.5: macOS bundles `data/` under Contents/Resources/ (codesign won't
    // accept it beside the Mach-O in Contents/MacOS/); `app_data_root()` resolves
    // it there. Windows/Linux: unchanged (`<exe_parent>/data`).
    let data_root = paths::app_data_root();
    if persisted.phone_dist_dir != phone_dist_dir || persisted.data_root != data_root {
        info!(
            old_data_root = %persisted.data_root.display(),
            new_data_root = %data_root.display(),
            "overriding stale data_root/phone_dist_dir from config.json with current_exe-derived paths",
        );
    }

    // Phase 16 bundled-RPCS3 model: on Windows the portal *control* binary is
    // ALWAYS the patched RPCS3 we ship next to the app at `<app>/rpcs3/rpcs3.exe`,
    // driven over IPC. Neither is a user preference — both are determined by the
    // install layout — so, exactly like data_root/phone_dist_dir above (PLAN
    // 10.8.4), recompute them from `current_exe` every launch instead of trusting
    // config.json. This transparently repairs configs written by pre-16.5.2.3 /
    // pre-16.9 versions, which persisted `driver_kind: uia` pointed at the user's
    // *stock* RPCS3: stock RPCS3 has no IPC listener, so after an upgrade-in-place
    // (the wizard is skipped when config.json already exists) those boxes silently
    // run the legacy UIA path and figures never load onto the portal. The user's
    // stock install is still used — as `config_dir` (games.yml + firmware). The
    // per-profile working copies that hold figure progress live under the runtime
    // dir keyed by profile+figure id, so nothing here rewrites a `.sky`.
    #[cfg(not(target_os = "macos"))]
    let (rpcs3_exe, driver_kind, config_dir) = {
        let (rpcs3_exe, driver_kind, config_dir) = migrate_install_paths(
            &persisted.rpcs3_exe,
            &persisted.config_dir,
            persisted.driver_kind,
            &exe_parent,
        );
        if !matches!(persisted.driver_kind, PersistedDriverKind::Mock)
            && (persisted.rpcs3_exe != rpcs3_exe
                || !matches!(persisted.driver_kind, PersistedDriverKind::Ipc))
        {
            info!(
                old_rpcs3_exe = %persisted.rpcs3_exe.display(),
                new_rpcs3_exe = %rpcs3_exe.display(),
                old_driver = ?persisted.driver_kind,
                "config predates the bundled-RPCS3/IPC model — driving the bundled patched RPCS3 over IPC (working copies untouched)",
            );
        }
        (rpcs3_exe, driver_kind, config_dir)
    };
    // U.5.1 — macOS now ships the patched RPCS3 inside the .app (a later U task
    // stages it), so mirror the Windows bundled-RPCS3/IPC model: the control
    // binary + driver are install-layout-derived, not user prefs, so recompute
    // them every launch. Two macOS-only twists vs `migrate_install_paths`:
    //   * the emulator is a `.app` nested under Resources, not a bare exe
    //     (`paths::bundled_rpcs3_exe()` resolves it);
    //   * it may be ABSENT (dev `cargo run --release`, or a build made before the
    //     emulator was staged) → fall back to Mock so the launcher still boots
    //     instead of pointing IPC at a socket the never-launched emulator can't
    //     create.
    // The ONLY deliberate Mock on macOS is the `SKYLANDER_PORTAL_DRIVER=mock` env
    // override (`force_mock`, honoured below). Unlike Windows we do NOT treat a
    // *persisted* Mock as deliberate: every legacy mock-only mac release seeded
    // Mock, and once the emulator is bundled we WANT to auto-promote those to IPC.
    #[cfg(target_os = "macos")]
    let (rpcs3_exe, driver_kind, config_dir) = {
        let bundled = paths::bundled_rpcs3_exe();
        let (rpcs3_exe, driver_kind, config_dir) =
            migrate_install_paths_macos(&persisted.config_dir, &bundled, force_mock);
        let persisted_equiv = match driver_kind {
            DriverKind::Ipc => PersistedDriverKind::Ipc,
            DriverKind::Uia => PersistedDriverKind::Uia,
            DriverKind::Mock => PersistedDriverKind::Mock,
        };
        if persisted.driver_kind != persisted_equiv || persisted.rpcs3_exe != rpcs3_exe {
            info!(
                old_rpcs3_exe = %persisted.rpcs3_exe.display(),
                new_rpcs3_exe = %rpcs3_exe.display(),
                old_driver = ?persisted.driver_kind,
                new_driver = ?driver_kind,
                "macOS: driving the bundled patched RPCS3 over IPC (Mock when the emulator isn't bundled or SKYLANDER_PORTAL_DRIVER=mock)",
            );
        }
        (rpcs3_exe, driver_kind, config_dir)
    };

    Ok(Config {
        rpcs3_exe,
        config_dir,
        firmware_pack_root: persisted.firmware_pack_root,
        bind_port: persisted.bind_port,
        driver_kind,
        window_mode: persisted.window_mode,
        render_2x: persisted.render_2x,
        log_dir: persisted.log_dir,
        phone_dist_dir,
        data_root,
        hmac_key,
    })
}

/// (Windows release) Resolve the install-layout-derived RPCS3 fields from the
/// persisted `rpcs3_exe` / `config_dir` plus the running binary's parent dir.
/// Pure; touches no files.
///
/// Under the Phase-16 bundled-RPCS3 model the control binary is ALWAYS the
/// patched RPCS3 shipped at `<exe_parent>/rpcs3/rpcs3.exe`, and the only driver
/// is IPC — both install-layout-derived, not user prefs — so callers recompute
/// them each launch (mirroring data_root/phone_dist_dir, PLAN 10.8.4). This
/// repairs pre-IPC configs that pinned `driver_kind: uia` to the user's *stock*
/// RPCS3 (no IPC listener → figures never load onto the portal after upgrade).
/// The returned `config_dir` (the user's install: games.yml + firmware) is the
/// persisted `config_dir`, falling back to the persisted stock `rpcs3_exe`'s
/// parent for pre-16.9 configs that have no `config_dir`.
///
/// Returns `(rpcs3_exe, driver_kind, config_dir)`. It deliberately ignores the
/// persisted `rpcs3_exe`/`driver_kind` for the first two outputs; figure progress
/// lives in per-profile working copies under the runtime dir, untouched by this.
#[allow(dead_code)] // sole call site is release + non-macOS (`config::load`)
fn migrate_install_paths(
    persisted_rpcs3_exe: &std::path::Path,
    persisted_config_dir: &std::path::Path,
    persisted_driver: crate::wizard::PersistedDriverKind,
    exe_parent: &std::path::Path,
) -> (PathBuf, DriverKind, PathBuf) {
    use crate::wizard::PersistedDriverKind;
    // A deliberately-chosen Mock config (the no-GPU wizard path or the
    // `SKYLANDER_PORTAL_DRIVER=mock` demo override, PLAN 19) is NOT a stale
    // pre-IPC config to repair — keep it. The mock driver uses no RPCS3 binary
    // and no config dir at all.
    if matches!(persisted_driver, PersistedDriverKind::Mock) {
        return (PathBuf::new(), DriverKind::Mock, PathBuf::new());
    }
    let rpcs3_exe = exe_parent.join("rpcs3").join("rpcs3.exe");
    let config_dir = if persisted_config_dir.as_os_str().is_empty() {
        persisted_rpcs3_exe
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        persisted_config_dir.to_path_buf()
    };
    (rpcs3_exe, DriverKind::Ipc, config_dir)
}

/// (macOS release) RPCS3's conventional config/data root on macOS:
/// `~/Library/Application Support/rpcs3/` (holds `games.yml` + installed
/// firmware). With no interactive entry point here, a user whose RPCS3 data
/// lives elsewhere relies on the U.6 wizard to set `config_dir`; this is only the
/// fallback for a pre-16.9-style config with an empty `config_dir`.
#[cfg(target_os = "macos")]
#[allow(dead_code)] // sole non-test caller is release + the macOS `config::load`
fn default_macos_rpcs3_config_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Library/Application Support/rpcs3"))
        .unwrap_or_default()
}

/// (macOS release) Resolve the install-layout-derived RPCS3 fields, mirroring the
/// Windows [`migrate_install_paths`]. `bundled_rpcs3_exe` is
/// [`crate::paths::bundled_rpcs3_exe`] resolved by the caller (so this stays pure
/// + unit-testable). Returns `(rpcs3_exe, driver_kind, config_dir)`.
///
/// `force_mock` is the `SKYLANDER_PORTAL_DRIVER=mock` env override — the only
/// deliberate Mock on macOS; when set, stay Mock even with a bundled emulator.
/// When the emulator isn't present (`!bundled_rpcs3_exe.exists()` — a build made
/// before it was staged, or a dev `cargo run`), fall back to Mock so the launcher
/// still boots. Otherwise auto-promote to IPC against the bundled patched RPCS3,
/// keeping the persisted `config_dir` (falling back to the macOS default when
/// empty).
#[cfg(target_os = "macos")]
#[allow(dead_code)] // sole non-test caller is release + the macOS `config::load`
fn migrate_install_paths_macos(
    persisted_config_dir: &std::path::Path,
    bundled_rpcs3_exe: &std::path::Path,
    force_mock: bool,
) -> (PathBuf, DriverKind, PathBuf) {
    if force_mock || !bundled_rpcs3_exe.exists() {
        return (PathBuf::new(), DriverKind::Mock, PathBuf::new());
    }
    let config_dir = if persisted_config_dir.as_os_str().is_empty() {
        default_macos_rpcs3_config_dir()
    } else {
        persisted_config_dir.to_path_buf()
    };
    (bundled_rpcs3_exe.to_path_buf(), DriverKind::Ipc, config_dir)
}

#[cfg(feature = "dev-tools")]
fn require_path(env: &HashMap<String, String>, key: &str) -> Result<PathBuf> {
    env.get(key)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("missing {key} in .env.dev"))
}

#[cfg(feature = "dev-tools")]
fn read_env_file(path: &str) -> Result<HashMap<String, String>> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let mut out = HashMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // A.8.10 — the `--reconfigure` gate. The wizard runs on first launch (no
    // config) OR when `--reconfigure` forces a re-run; an existing config with
    // no flag is read as-is. (The demo-mode `force_mock` short-circuit lives
    // upstream of this in `load`, so it isn't a parameter here.)
    #[test]
    fn reconfigure_forces_the_wizard() {
        assert!(wizard_needed(false, false), "first launch → wizard");
        assert!(
            wizard_needed(false, true),
            "first launch, flag set → wizard"
        );
        assert!(
            wizard_needed(true, true),
            "--reconfigure over an existing config → wizard"
        );
        assert!(
            !wizard_needed(true, false),
            "existing config, no flag → read it, no wizard"
        );
    }

    // PLAN 20.6 — the window-mode wire form is a contract shared by config.json,
    // the `/api/launcher/window-mode` request/response bodies, AND the phone's
    // hand-mirrored `model::WindowMode` enum. Pin it so a rename can't silently
    // desync the phone (which can't share this crate).
    #[test]
    fn window_mode_json_wire_form() {
        assert_eq!(serde_json::to_string(&WindowMode::Tv).unwrap(), "\"tv\"");
        assert_eq!(
            serde_json::to_string(&WindowMode::Desktop).unwrap(),
            "\"desktop\"",
        );
        let parsed: WindowMode = serde_json::from_str("\"desktop\"").unwrap();
        assert_eq!(parsed, WindowMode::Desktop);
        // Existing installs (no persisted value) keep the fullscreen TV mode.
        assert_eq!(WindowMode::default(), WindowMode::Tv);
    }

    // The reported bug: a box with an existing (pre-IPC) install. An older app
    // version's wizard persisted the legacy UIA driver pointed at the user's
    // *stock* RPCS3 and no `config_dir`; on upgrade the first-launch wizard is
    // skipped (config.json exists), so without migration the app keeps driving
    // stock RPCS3 over UIA — which has no IPC listener, so figures never load.
    // After migration the control binary is the bundled patched RPCS3 over IPC,
    // and `config_dir` falls back to the stock exe's parent (the user's install:
    // games.yml + firmware).
    #[test]
    fn migrates_pre_ipc_uia_config_to_bundled_ipc() {
        let install = Path::new("stock-install");
        let stock_exe = install.join("rpcs3.exe");
        let exe_parent = Path::new("app-dir");

        // Empty persisted config_dir = a pre-16.9 config.
        let (rpcs3_exe, driver, config_dir) = migrate_install_paths(
            &stock_exe,
            Path::new(""),
            crate::wizard::PersistedDriverKind::Uia,
            exe_parent,
        );

        assert_eq!(
            rpcs3_exe,
            exe_parent.join("rpcs3").join("rpcs3.exe"),
            "control binary must be the bundled patched RPCS3, not the persisted stock path",
        );
        assert_eq!(
            driver,
            DriverKind::Ipc,
            "must drive IPC, not the legacy UIA fallback",
        );
        assert_eq!(
            config_dir, install,
            "config_dir must fall back to the user's stock install (games.yml + firmware)",
        );
    }

    // A current (post-16.9 wizard) config: `config_dir` is set explicitly and is
    // preserved; `rpcs3_exe` is still recomputed to the bundled binary so an MSI
    // relocation of the install is picked up rather than left stale.
    #[test]
    fn keeps_explicit_config_dir_and_rebinds_rpcs3_exe() {
        let exe_parent = Path::new("app-dir");
        let users_install = Path::new("users-rpcs3-install");
        let persisted_exe = exe_parent.join("rpcs3").join("rpcs3.exe");

        let (rpcs3_exe, driver, config_dir) = migrate_install_paths(
            &persisted_exe,
            users_install,
            crate::wizard::PersistedDriverKind::Ipc,
            exe_parent,
        );

        assert_eq!(rpcs3_exe, exe_parent.join("rpcs3").join("rpcs3.exe"));
        assert_eq!(driver, DriverKind::Ipc);
        assert_eq!(config_dir, users_install);
    }

    // PLAN 19.3: a deliberate Mock config (no-GPU wizard path or the
    // `SKYLANDER_PORTAL_DRIVER=mock` demo override) must be preserved, not
    // "repaired" into IPC like a stale pre-IPC UIA config would be.
    #[test]
    fn preserves_deliberate_mock_config() {
        let exe_parent = Path::new("app-dir");
        let (rpcs3_exe, driver, config_dir) = migrate_install_paths(
            Path::new(""),
            Path::new(""),
            crate::wizard::PersistedDriverKind::Mock,
            exe_parent,
        );
        assert_eq!(driver, DriverKind::Mock, "Mock must survive migration");
        assert_eq!(rpcs3_exe, PathBuf::new(), "mock uses no RPCS3 binary");
        assert_eq!(config_dir, PathBuf::new(), "mock uses no config dir");
    }

    // U.5.1 — macOS auto-promotes to IPC when the bundled patched RPCS3 is
    // present in the .app, keeping the persisted `config_dir` verbatim.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_promotes_to_ipc_when_emulator_bundled() {
        let tmp = tempfile::tempdir().unwrap();
        // Simulate the emulator nested under the host .app's Resources.
        let bundled = tmp
            .path()
            .join("Contents/Resources/rpcs3/rpcs3.app/Contents/MacOS/rpcs3");
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::write(&bundled, b"stub").unwrap();

        let (exe, driver, config_dir) =
            migrate_install_paths_macos(Path::new("/users/custom/rpcs3-data"), &bundled, false);
        assert_eq!(
            exe, bundled,
            "control binary = bundled rpcs3.app inner Mach-O"
        );
        assert_eq!(driver, DriverKind::Ipc);
        assert_eq!(config_dir, Path::new("/users/custom/rpcs3-data"));
    }

    // SAFETY property: with no bundled emulator (today, before a later U task
    // stages it) macOS stays Mock — no behavior change.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_falls_back_to_mock_when_emulator_absent() {
        let tmp = tempfile::tempdir().unwrap();
        // A path that is never created on disk.
        let bundled = tmp.path().join("rpcs3/rpcs3.app/Contents/MacOS/rpcs3");
        let (exe, driver, config_dir) = migrate_install_paths_macos(Path::new(""), &bundled, false);
        assert_eq!(driver, DriverKind::Mock, "no bundled emulator → Mock");
        assert_eq!(exe, PathBuf::new());
        assert_eq!(config_dir, PathBuf::new());
    }

    // `SKYLANDER_PORTAL_DRIVER=mock` (the only deliberate Mock on macOS) wins
    // even when the emulator is bundled.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_env_override_forces_mock_even_when_bundled() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp
            .path()
            .join("Contents/Resources/rpcs3/rpcs3.app/Contents/MacOS/rpcs3");
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::write(&bundled, b"stub").unwrap();
        let (_, driver, _) = migrate_install_paths_macos(Path::new(""), &bundled, true);
        assert_eq!(driver, DriverKind::Mock);
    }

    // An empty persisted `config_dir` (a pre-16.9-style mac config) falls back to
    // the conventional `~/Library/Application Support/rpcs3` root.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_empty_config_dir_falls_back_to_default_rpcs3_root() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp
            .path()
            .join("Contents/Resources/rpcs3/rpcs3.app/Contents/MacOS/rpcs3");
        std::fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        std::fs::write(&bundled, b"stub").unwrap();
        let (_, driver, config_dir) = migrate_install_paths_macos(Path::new(""), &bundled, false);
        assert_eq!(driver, DriverKind::Ipc);
        assert_eq!(config_dir, default_macos_rpcs3_config_dir());
    }
}
