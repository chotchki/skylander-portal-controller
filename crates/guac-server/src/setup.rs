//! guacamole's config loader and settings (AA design decision 6). Its data
//! lives under [`crate::paths::guac_data_root`]; the controller's `config.json`
//! and `db.sqlite` are never opened.
//!
//! The order the binary follows, so a second copy touches nothing: [`load`]
//! (reads `db.sqlite` only if it exists, and writes nothing), then `boot` with
//! [`MIGRATOR`], then, only if the server came up (a failed bind is `Ok` with a
//! closed `driver_tx`), [`Setup::open`] and [`Setup::keep_pairing_key`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

use crate::config::{Config, DriverKind, WindowMode};
use crate::dumps::{self, DumpScan};

/// guacamole's migration set: the product's profile tables plus `settings`.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// The controller's port too, so the port lock covers both apps.
pub const DEFAULT_PORT: u16 = 8765;

const KEY_LEN: usize = 32;
const DUMP_DIRS: &str = "dump_dirs";
const BIND_PORT: &str = "bind_port";
const PAIRING_KEY: &str = "pairing_key";
const FFMPEG_PATH: &str = "ffmpeg_path";
const OUTPUT_LATENCY_MS: &str = "output_latency_ms";

/// The most an intro's video is held back for the sound path (openspec AB,
/// design decision 7); more is a typo, not a TV.
pub const MAX_OUTPUT_LATENCY_MS: u32 = 1000;

/// What `settings` holds; `None` and empty mean never set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub dump_dirs: Vec<PathBuf>,
    pub bind_port: Option<u16>,
    pub pairing_key: Option<Vec<u8>>,
    /// ffmpeg for the extraction (openspec AB, design decision 8); unset
    /// looks beside the executable, then on PATH.
    pub ffmpeg_path: Option<PathBuf>,
    /// How far the speaker lags Bevy's audio clock, in ms; intros hold their
    /// video back by it (openspec AB, design decision 7). Unset is guacamole's
    /// default.
    pub output_latency_ms: Option<u32>,
}

/// [`load`]'s result.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub config: Config,
    /// `GUAC_DUMP_DIRS` in dev when set, else the stored folders.
    pub dump_dirs: Vec<PathBuf>,
    /// True when `GUAC_DUMP_DIRS` overrode the stored folders.
    pub dump_dirs_from_env: bool,
    /// `GUAC_FFMPEG` in dev when set, else the stored path.
    pub ffmpeg: Option<PathBuf>,
    /// `GUAC_EXTRACT_DIR`, dev only: the extract root outside the checkout.
    /// Release builds never read it.
    pub extract_dir: Option<String>,
    /// `GUAC_OUTPUT_LATENCY_MS` in dev when set, else the stored value.
    pub output_latency_ms: Option<u32>,
}

impl Loaded {
    /// No dumps folder yet: the TV's setup screen asks for one.
    pub fn needs_setup(&self) -> bool {
        self.dump_dirs.is_empty()
    }

    pub fn scan(&self) -> DumpScan {
        dumps::scan(&self.dump_dirs)
    }
}

/// Dev reads `.env.dev` over the stored settings; release reads none.
/// Never requires `RPCS3_EXE` or `FIRMWARE_PACK_ROOT`.
pub fn load(root: &Path) -> Result<Loaded> {
    load_with(root, &env())
}

/// The env map [`load`] reads: `.env.dev` in dev, none in release.
pub fn env() -> HashMap<String, String> {
    if cfg!(feature = "dev-tools") {
        read_env_file(Path::new(".env.dev"))
    } else {
        HashMap::new()
    }
}

/// The log folder; needs no stored settings, so logging can start before [`load`].
pub fn log_dir(root: &Path) -> PathBuf {
    if cfg!(feature = "dev-tools") {
        PathBuf::from("logs")
    } else {
        root.join("logs")
    }
}

/// `WINDOW_MODE=desktop` is a window; anything else is the TV.
pub fn window_mode(env: &HashMap<String, String>) -> WindowMode {
    match env.get("WINDOW_MODE").map(String::as_str) {
        Some("desktop") => WindowMode::Desktop,
        _ => WindowMode::Tv,
    }
}

/// [`load`] over a given env map. Writes nothing; a first start gets a fresh
/// key that [`Setup::keep_pairing_key`] stores once the server is up.
pub fn load_with(root: &Path, env: &HashMap<String, String>) -> Result<Loaded> {
    let stored = read_stored(root)?;
    let dev = cfg!(feature = "dev-tools");

    let env_dirs: Option<Vec<PathBuf>> = env.get("GUAC_DUMP_DIRS").map(|v| {
        std::env::split_paths(v)
            .filter(|p| !p.as_os_str().is_empty())
            .collect()
    });
    let dump_dirs_from_env = env_dirs.is_some();
    let dump_dirs = env_dirs.unwrap_or_else(|| stored.dump_dirs.clone());

    let bind_port = match env.get("BIND_PORT") {
        Some(v) => v.parse().context("BIND_PORT must be a port number")?,
        None => stored.bind_port.unwrap_or(DEFAULT_PORT),
    };
    let window_mode = window_mode(env);
    let path_or = |key: &str, dev_default: &str, release: fn() -> PathBuf| {
        env.get(key).map(PathBuf::from).unwrap_or_else(|| {
            if dev {
                PathBuf::from(dev_default)
            } else {
                release()
            }
        })
    };
    let phone_dist_dir = path_or("PHONE_DIST", "phone/dist", || {
        crate::paths::app_asset_dir().join("phone-dist")
    });
    let data_root = path_or("DATA_ROOT", "data", crate::paths::app_data_root);
    let log_dir = log_dir(root);
    let hmac_key = stored.pairing_key.unwrap_or_else(generate_key);
    let dev_value = |key: &str| env.get(key).filter(|_| dev).cloned();
    let ffmpeg = dev_value("GUAC_FFMPEG")
        .map(PathBuf::from)
        .or(stored.ffmpeg_path);
    let extract_dir = dev_value("GUAC_EXTRACT_DIR");
    let output_latency_ms = match dev_value("GUAC_OUTPUT_LATENCY_MS") {
        Some(v) => Some(latency(&v).with_context(|| {
            format!("GUAC_OUTPUT_LATENCY_MS must be 0 to {MAX_OUTPUT_LATENCY_MS} ms")
        })?),
        None => stored.output_latency_ms,
    };

    Ok(Loaded {
        config: Config {
            rpcs3_exe: PathBuf::new(),
            config_dir: PathBuf::new(),
            firmware_pack_root: PathBuf::new(),
            bind_port,
            driver_kind: DriverKind::Native,
            window_mode,
            render_2x: false,
            log_dir,
            phone_dist_dir,
            data_root,
            hmac_key,
        },
        dump_dirs,
        dump_dirs_from_env,
        ffmpeg,
        extract_dir,
        output_latency_ms,
    })
}

/// A latency in whole ms, 0 to [`MAX_OUTPUT_LATENCY_MS`].
fn latency(value: &str) -> Option<u32> {
    value
        .trim()
        .parse()
        .ok()
        .filter(|ms| *ms <= MAX_OUTPUT_LATENCY_MS)
}

/// `KEY=value` lines, `#` comments; a missing file is an empty map.
pub fn read_env_file(path: &Path) -> HashMap<String, String> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

fn generate_key() -> Vec<u8> {
    use rand_core::{OsRng, RngCore};
    let mut key = vec![0u8; KEY_LEN];
    OsRng.fill_bytes(&mut key);
    key
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build the settings runtime")
}

/// The stored settings without creating or migrating anything; no
/// `db.sqlite`, or one not yet migrated, reads as never set. Opened
/// read-write so a crash's hot journal rolls back instead of failing.
fn read_stored(root: &Path) -> Result<Settings> {
    let db = root.join("db.sqlite");
    if !db.is_file() {
        return Ok(Settings::default());
    }
    let rt = runtime()?;
    rt.block_on(async {
        let opts = SqliteConnectOptions::new()
            .filename(&db)
            .create_if_missing(false);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .with_context(|| format!("open {}", db.display()))?;
        let settings = async {
            let (has,): (bool,) = sqlx::query_as(
                "SELECT EXISTS (SELECT 1 FROM sqlite_master \
                 WHERE type = 'table' AND name = 'settings')",
            )
            .fetch_one(&pool)
            .await
            .context("read settings")?;
            if has {
                read_settings(&pool).await
            } else {
                Ok(Settings::default())
            }
        }
        .await;
        pool.close().await;
        settings
    })
}

/// Every stored setting; a malformed value reads as unset.
pub async fn read_settings(pool: &SqlitePool) -> Result<Settings> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings")
        .fetch_all(pool)
        .await
        .context("read settings")?;
    let mut out = Settings::default();
    for (key, value) in rows {
        match key.as_str() {
            DUMP_DIRS => out.dump_dirs = serde_json::from_str(&value).unwrap_or_default(),
            BIND_PORT => out.bind_port = value.parse().ok().filter(|p| *p != 0),
            PAIRING_KEY => {
                out.pairing_key = hex::decode(&value).ok().filter(|k| k.len() == KEY_LEN)
            }
            FFMPEG_PATH => out.ffmpeg_path = Some(PathBuf::from(value)).filter(|p| p.is_absolute()),
            OUTPUT_LATENCY_MS => out.output_latency_ms = latency(&value),
            _ => {}
        }
    }
    Ok(out)
}

async fn put(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) \
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await
    .with_context(|| format!("write setting {key}"))?;
    Ok(())
}

/// Blocking access to guacamole's settings, for the binary's setup screen
/// and `--setup`. Call it outside any tokio runtime.
pub struct Setup {
    pool: SqlitePool,
    rt: tokio::runtime::Runtime,
}

impl Setup {
    /// Opens `<root>/db.sqlite`, creating and migrating it with [`MIGRATOR`].
    pub fn open(root: &Path) -> Result<Self> {
        let rt = runtime()?;
        let db = crate::paths::db_path_under(root)?;
        let pool = rt.block_on(async {
            let opts = SqliteConnectOptions::new()
                .filename(&db)
                .create_if_missing(true)
                .foreign_keys(true);
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(opts)
                .await
                .with_context(|| format!("open {}", db.display()))?;
            MIGRATOR.run(&pool).await.context("run migrations")?;
            anyhow::Ok(pool)
        })?;
        Ok(Self { pool, rt })
    }

    pub fn settings(&self) -> Result<Settings> {
        self.rt.block_on(read_settings(&self.pool))
    }

    /// True until a dumps folder is stored: `--setup` and the TV ask for one.
    pub fn needs_setup(&self) -> Result<bool> {
        Ok(self.settings()?.dump_dirs.is_empty())
    }

    /// Stores the dumps folders (made absolute) and returns what they hold.
    /// Each must be an existing folder.
    pub fn set_dump_dirs(&self, dirs: &[PathBuf]) -> Result<DumpScan> {
        anyhow::ensure!(!dirs.is_empty(), "choose at least one folder");
        let mut absolute = Vec::with_capacity(dirs.len());
        for dir in dirs {
            anyhow::ensure!(dir.is_dir(), "{} isn't a folder", dir.display());
            absolute.push(std::path::absolute(dir)?);
        }
        let json = serde_json::to_string(&absolute).context("a folder path isn't UTF-8")?;
        self.rt.block_on(put(&self.pool, DUMP_DIRS, &json))?;
        Ok(dumps::scan(&absolute))
    }

    pub fn set_bind_port(&self, port: u16) -> Result<()> {
        anyhow::ensure!(port != 0, "port 0 isn't a port");
        self.rt
            .block_on(put(&self.pool, BIND_PORT, &port.to_string()))
    }

    /// Stores ffmpeg's path; it must be absolute.
    pub fn set_ffmpeg_path(&self, path: &Path) -> Result<()> {
        anyhow::ensure!(path.is_absolute(), "{} isn't absolute", path.display());
        let value = path.to_str().context("ffmpeg's path isn't UTF-8")?;
        self.rt.block_on(put(&self.pool, FFMPEG_PATH, value))
    }

    /// Stores the intro's output latency, 0 to [`MAX_OUTPUT_LATENCY_MS`] ms.
    pub fn set_output_latency_ms(&self, ms: u32) -> Result<()> {
        anyhow::ensure!(
            ms <= MAX_OUTPUT_LATENCY_MS,
            "{ms} ms is past the {MAX_OUTPUT_LATENCY_MS} ms limit"
        );
        self.rt
            .block_on(put(&self.pool, OUTPUT_LATENCY_MS, &ms.to_string()))
    }

    /// The stored pairing key, or `candidate` (the key [`load`] put in the
    /// QR) stored on the first start.
    pub fn keep_pairing_key(&self, candidate: &[u8]) -> Result<Vec<u8>> {
        anyhow::ensure!(candidate.len() == KEY_LEN, "a pairing key is 32 bytes");
        if let Some(key) = self.settings()?.pairing_key {
            return Ok(key);
        }
        self.rt
            .block_on(put(&self.pool, PAIRING_KEY, &hex::encode(candidate)))?;
        Ok(candidate.to_vec())
    }
}

impl Drop for Setup {
    fn drop(&mut self) {
        self.rt.block_on(self.pool.close());
    }
}
