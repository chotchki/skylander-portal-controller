//! First-launch config wizard (release builds only).
//!
//! Runs a small egui viewport to collect the RPCS3 executable path and the
//! firmware-pack root, validates them, and writes `config.json` under the
//! resolved runtime dir. Dev builds skip this entirely — they read
//! `.env.dev` in [`crate::config::load`].
//!
//! The wizard is intentionally plain: PLAN 3.15.5 covers the Skylanders
//! aesthetic pass for the launcher, and this wizard should be easy to
//! re-theme alongside the launcher later.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

// ---- Validation ----------------------------------------------------------

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("path does not exist")]
    NotFound,
    #[error("expected a file, got a directory")]
    ExpectedFile,
    #[error("expected a directory, got a file")]
    ExpectedDirectory,
    #[error("file is not named rpcs3.exe")]
    NotRpcs3Exe,
    #[error("no .sky files found anywhere under this directory")]
    NoSkyFiles,
}

/// True if `p` points at an existing `rpcs3.exe` file.
pub fn validate_rpcs3_path(p: &Path) -> Result<(), ValidationError> {
    if !p.exists() {
        return Err(ValidationError::NotFound);
    }
    if !p.is_file() {
        return Err(ValidationError::ExpectedFile);
    }
    match p.file_name().and_then(|s| s.to_str()) {
        Some(n) if n.eq_ignore_ascii_case("rpcs3.exe") => Ok(()),
        _ => Err(ValidationError::NotRpcs3Exe),
    }
}

/// True if `p` is a directory containing at least one `.sky` file (recursive).
pub fn validate_firmware_pack(p: &Path) -> Result<(), ValidationError> {
    if !p.exists() {
        return Err(ValidationError::NotFound);
    }
    if !p.is_dir() {
        return Err(ValidationError::ExpectedDirectory);
    }
    let found = walkdir::WalkDir::new(p)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .any(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.eq_ignore_ascii_case("sky"))
                    .unwrap_or(false)
        });
    if found {
        Ok(())
    } else {
        Err(ValidationError::NoSkyFiles)
    }
}

// ---- Heuristic defaults --------------------------------------------------

/// Best-guess RPCS3 install path. Returns the first candidate that exists
/// and validates, else None.
pub fn default_rpcs3_path_guess() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    // Chris's known HTPC layout (per CLAUDE.md — "C:\emuluators\rpcs3").
    candidates.push(PathBuf::from(r"C:\emuluators\rpcs3\rpcs3.exe"));
    candidates.push(PathBuf::from(r"C:\emulators\rpcs3\rpcs3.exe"));

    // %PROGRAMFILES%\RPCS3\rpcs3.exe
    if let Some(pf) = std::env::var_os("PROGRAMFILES") {
        candidates.push(PathBuf::from(pf).join("RPCS3").join("rpcs3.exe"));
    }
    if let Some(pf) = std::env::var_os("PROGRAMFILES(X86)") {
        candidates.push(PathBuf::from(pf).join("RPCS3").join("rpcs3.exe"));
    }

    candidates
        .into_iter()
        .find(|p| validate_rpcs3_path(p).is_ok())
}

/// Best-guess firmware-pack path. Returns the first candidate that exists
/// and contains a `.sky` file, else None.
pub fn default_firmware_pack_guess() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    // Chris's known HTPC pack.
    candidates.push(PathBuf::from(
        r"C:\Users\chris\workspace\Skylanders Characters Pack for RPCS3",
    ));

    if let Some(home) = std::env::var_os("USERPROFILE") {
        let home = PathBuf::from(home);
        candidates.push(home.join("Documents").join("Skylanders Characters Pack for RPCS3"));
        candidates.push(home.join("Downloads").join("Skylanders Characters Pack for RPCS3"));
    }

    candidates
        .into_iter()
        .find(|p| validate_firmware_pack(p).is_ok())
}

// ---- Persisted JSON shape ------------------------------------------------

/// On-disk `config.json` shape. Mirrors [`crate::config::Config`] but owns
/// the serde contract so the runtime Config can grow non-persisted fields
/// without invalidating user configs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedConfig {
    pub rpcs3_exe: PathBuf,
    pub firmware_pack_root: PathBuf,
    pub games_yaml: PathBuf,
    pub bind_port: u16,
    pub driver_kind: PersistedDriverKind,
    pub log_dir: PathBuf,
    pub phone_dist_dir: PathBuf,
    pub data_root: PathBuf,
    /// 32-byte HMAC key, serialised as a hex string. `Option` for backwards
    /// compat with pre-3.13 configs; the release `config::load()` path
    /// regenerates and persists if missing.
    #[serde(default, with = "crate::wizard::hex_key_opt")]
    pub hmac_key: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PersistedDriverKind {
    Uia,
    Mock,
}

impl PersistedConfig {
    /// Build a persisted config from the wizard's user-entered paths plus
    /// sensible derived defaults for everything else.
    pub fn from_user_paths(
        rpcs3_exe: PathBuf,
        firmware_pack_root: PathBuf,
        runtime_dir: &Path,
    ) -> Self {
        let games_yaml = rpcs3_exe
            .parent()
            .map(|p| p.join("config").join("games.yml"))
            .unwrap_or_else(|| PathBuf::from("games.yml"));

        let exe_parent = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        PersistedConfig {
            rpcs3_exe,
            firmware_pack_root,
            games_yaml,
            bind_port: 8765,
            driver_kind: PersistedDriverKind::Uia,
            log_dir: runtime_dir.join("logs"),
            phone_dist_dir: exe_parent.join("phone-dist"),
            data_root: exe_parent.join("data"),
            hmac_key: Some(crate::config::generate_hmac_key()),
        }
    }

    pub fn read(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

/// Serde helper for `Option<Vec<u8>>` as a hex string — mirrors
/// `config::hex_key` but optional.
mod hex_key_opt {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        bytes: &Option<Vec<u8>>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(b) => s.serialize_some(&hex::encode(b)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<Vec<u8>>, D::Error> {
        let maybe = Option::<String>::deserialize(d)?;
        match maybe {
            Some(s) => hex::decode(&s).map(Some).map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

// ---- egui wizard (release-only) ------------------------------------------

#[cfg(not(feature = "dev-tools"))]
pub fn run_wizard_blocking(config_path: &Path, runtime_dir: &Path) -> Result<PersistedConfig> {
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Page {
        Welcome,
        Rpcs3,
        FirmwarePack,
        Done,
    }

    struct WizardState {
        page: Page,
        rpcs3_input: String,
        pack_input: String,
        result: Option<PersistedConfig>,
        cancelled: bool,
        runtime_dir: PathBuf,
    }

    impl WizardState {
        fn rpcs3_path(&self) -> PathBuf {
            PathBuf::from(self.rpcs3_input.trim())
        }
        fn pack_path(&self) -> PathBuf {
            PathBuf::from(self.pack_input.trim())
        }
        fn rpcs3_valid(&self) -> Result<(), ValidationError> {
            validate_rpcs3_path(&self.rpcs3_path())
        }
        fn pack_valid(&self) -> Result<(), ValidationError> {
            validate_firmware_pack(&self.pack_path())
        }
    }

    let rpcs3_default = default_rpcs3_path_guess()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let pack_default = default_firmware_pack_guess()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    let state = Arc::new(Mutex::new(WizardState {
        page: Page::Welcome,
        rpcs3_input: rpcs3_default,
        pack_input: pack_default,
        result: None,
        cancelled: false,
        runtime_dir: runtime_dir.to_path_buf(),
    }));

    struct App {
        state: Arc<Mutex<WizardState>>,
    }

    impl eframe::App for App {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            let mut s = self.state.lock().unwrap();
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_space(24.0);
                match s.page {
                    Page::Welcome => {
                        ui.heading("Skylander Portal Controller");
                        ui.add_space(12.0);
                        ui.label("First launch — let's set this up. Takes 30 seconds.");
                        ui.add_space(20.0);
                        ui.label(
                            "We need to know where your RPCS3 install lives, and where \
                             your Skylanders firmware pack is stored.",
                        );
                        ui.add_space(32.0);
                        ui.horizontal(|ui| {
                            if ui.button("Next").clicked() {
                                s.page = Page::Rpcs3;
                            }
                            if ui.button("Cancel").clicked() {
                                s.cancelled = true;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                    }
                    Page::Rpcs3 => {
                        ui.heading("Step 1 of 2 — RPCS3");
                        ui.add_space(12.0);
                        ui.label("Path to rpcs3.exe:");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut s.rpcs3_input)
                                    .desired_width(500.0),
                            );
                            if ui.button("Browse...").clicked() {
                                if let Some(p) = rfd::FileDialog::new()
                                    .add_filter("rpcs3.exe", &["exe"])
                                    .set_title("Select rpcs3.exe")
                                    .pick_file()
                                {
                                    s.rpcs3_input = p.display().to_string();
                                }
                            }
                        });
                        ui.add_space(6.0);
                        match s.rpcs3_valid() {
                            Ok(()) => {
                                ui.colored_label(egui::Color32::GREEN, "Valid — rpcs3.exe found.");
                            }
                            Err(e) => {
                                ui.colored_label(egui::Color32::LIGHT_RED, format!("x  {e}"));
                            }
                        }
                        ui.add_space(20.0);
                        ui.horizontal(|ui| {
                            if ui.button("Back").clicked() {
                                s.page = Page::Welcome;
                            }
                            let enabled = s.rpcs3_valid().is_ok();
                            if ui.add_enabled(enabled, egui::Button::new("Next")).clicked() {
                                s.page = Page::FirmwarePack;
                            }
                        });
                    }
                    Page::FirmwarePack => {
                        ui.heading("Step 2 of 2 — Firmware pack");
                        ui.add_space(12.0);
                        ui.label("Folder containing your .sky files:");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut s.pack_input)
                                    .desired_width(500.0),
                            );
                            if ui.button("Browse...").clicked() {
                                if let Some(p) = rfd::FileDialog::new()
                                    .set_title("Select firmware pack folder")
                                    .pick_folder()
                                {
                                    s.pack_input = p.display().to_string();
                                }
                            }
                        });
                        ui.add_space(6.0);
                        match s.pack_valid() {
                            Ok(()) => {
                                ui.colored_label(
                                    egui::Color32::GREEN,
                                    "Valid — .sky files found.",
                                );
                            }
                            Err(e) => {
                                ui.colored_label(egui::Color32::LIGHT_RED, format!("x  {e}"));
                            }
                        }
                        ui.add_space(20.0);
                        ui.horizontal(|ui| {
                            if ui.button("Back").clicked() {
                                s.page = Page::Rpcs3;
                            }
                            let enabled = s.pack_valid().is_ok();
                            if ui.add_enabled(enabled, egui::Button::new("Finish")).clicked() {
                                let cfg = PersistedConfig::from_user_paths(
                                    s.rpcs3_path(),
                                    s.pack_path(),
                                    &s.runtime_dir,
                                );
                                s.result = Some(cfg);
                                s.page = Page::Done;
                            }
                        });
                    }
                    Page::Done => {
                        ui.heading("All set!");
                        ui.add_space(12.0);
                        ui.label("Config saved. Launching the server...");
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            });
        }
    }

    let state_for_app = state.clone();
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Skylander Portal Controller — Setup")
            .with_inner_size([720.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "skylander-portal-controller-wizard",
        native_options,
        Box::new(move |_cc| Ok(Box::new(App { state: state_for_app }))),
    )
    .map_err(|e| anyhow::anyhow!("wizard eframe error: {e}"))?;

    let guard = state.lock().unwrap();
    if guard.cancelled || guard.result.is_none() {
        anyhow::bail!("Setup cancelled. Re-run when you're ready.");
    }
    let cfg = guard.result.clone().unwrap();
    cfg.write(config_path)?;
    Ok(cfg)
}

// ---- Tests ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rpcs3_validator_accepts_named_exe() {
        let d = tempdir().unwrap();
        let p = d.path().join("rpcs3.exe");
        std::fs::write(&p, b"stub").unwrap();
        assert_eq!(validate_rpcs3_path(&p), Ok(()));
    }

    #[test]
    fn rpcs3_validator_rejects_missing() {
        let d = tempdir().unwrap();
        let p = d.path().join("nope.exe");
        assert_eq!(validate_rpcs3_path(&p), Err(ValidationError::NotFound));
    }

    #[test]
    fn rpcs3_validator_rejects_directory() {
        let d = tempdir().unwrap();
        // Directory passed where file expected.
        assert_eq!(
            validate_rpcs3_path(d.path()),
            Err(ValidationError::ExpectedFile)
        );
    }

    #[test]
    fn rpcs3_validator_rejects_wrong_name() {
        let d = tempdir().unwrap();
        let p = d.path().join("totally-not-rpcs3.exe");
        std::fs::write(&p, b"stub").unwrap();
        assert_eq!(validate_rpcs3_path(&p), Err(ValidationError::NotRpcs3Exe));
    }

    #[test]
    fn firmware_pack_accepts_dir_with_sky() {
        let d = tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("sub").join("nested")).unwrap();
        std::fs::write(d.path().join("sub").join("nested").join("test.sky"), b"").unwrap();
        assert_eq!(validate_firmware_pack(d.path()), Ok(()));
    }

    #[test]
    fn firmware_pack_rejects_empty() {
        let d = tempdir().unwrap();
        assert_eq!(
            validate_firmware_pack(d.path()),
            Err(ValidationError::NoSkyFiles)
        );
    }

    #[test]
    fn firmware_pack_rejects_nonexistent() {
        let p = PathBuf::from(r"C:\definitely\not\a\real\path\zzz");
        assert_eq!(validate_firmware_pack(&p), Err(ValidationError::NotFound));
    }

    #[test]
    fn firmware_pack_rejects_file() {
        let d = tempdir().unwrap();
        let p = d.path().join("a.sky");
        std::fs::write(&p, b"").unwrap();
        assert_eq!(
            validate_firmware_pack(&p),
            Err(ValidationError::ExpectedDirectory)
        );
    }

    #[test]
    fn firmware_pack_rejects_dir_without_sky() {
        let d = tempdir().unwrap();
        std::fs::write(d.path().join("readme.txt"), b"").unwrap();
        std::fs::write(d.path().join("poster.png"), b"").unwrap();
        assert_eq!(
            validate_firmware_pack(d.path()),
            Err(ValidationError::NoSkyFiles)
        );
    }

    #[test]
    fn defaults_return_option() {
        // These probe real filesystem candidates and may return Some or None
        // depending on the test host. Just exercise the candidate list and
        // assert the result is a valid Option (no panic, no error).
        let _ = default_rpcs3_path_guess();
        let _ = default_firmware_pack_guess();
    }

    #[test]
    fn persisted_config_round_trip() {
        let d = tempdir().unwrap();
        let rpcs3 = d.path().join("rpcs3.exe");
        std::fs::write(&rpcs3, b"").unwrap();
        let pack = d.path().join("pack");
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::write(pack.join("a.sky"), b"").unwrap();

        let cfg = PersistedConfig::from_user_paths(rpcs3.clone(), pack.clone(), d.path());
        let json_path = d.path().join("config.json");
        cfg.write(&json_path).unwrap();

        let reloaded = PersistedConfig::read(&json_path).unwrap();
        assert_eq!(cfg, reloaded);
        assert_eq!(reloaded.rpcs3_exe, rpcs3);
        assert_eq!(reloaded.firmware_pack_root, pack);
        assert_eq!(reloaded.bind_port, 8765);
        assert_eq!(reloaded.driver_kind, PersistedDriverKind::Uia);
    }
}
