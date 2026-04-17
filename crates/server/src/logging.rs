//! `tracing` setup. Dev mode logs to `./logs/` with daily rotation (7-day
//! retention is a future concern — tracing-appender's rolling appender keeps
//! everything for now). Release mode lands in `%APPDATA%/skylander-portal-controller/logs/`.

use std::path::PathBuf;

use anyhow::Result;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

pub fn init(log_dir: &PathBuf) -> Result<WorkerGuard> {
    std::fs::create_dir_all(log_dir)?;
    let appender = tracing_appender::rolling::daily(log_dir, "server.log");
    let (file_writer, guard) = tracing_appender::non_blocking(appender);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,skylander=debug"));

    let fmt_stdout = fmt::layer().with_target(false);
    let fmt_file = fmt::layer()
        .with_target(false)
        .with_ansi(false)
        .with_writer(file_writer);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_stdout)
        .with(fmt_file)
        .try_init()
        .ok();

    Ok(guard)
}
