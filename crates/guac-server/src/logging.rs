//! `tracing` setup. Dev mode logs to `./logs/` with daily rotation (7-day
//! retention is a future concern — tracing-appender's rolling appender keeps
//! everything for now). Release mode lands in `%APPDATA%/skylander-portal-controller/logs/`.

use std::path::PathBuf;

use anyhow::Result;
pub use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// The filter when `RUST_LOG` is unset. Targets are prefix-matched:
/// `skylander` covers the product's crates, `guac` this one (its modules
/// moved out of `skylander_server` in AA.1). symphonia's mp3 demuxer logs
/// an INFO line for every CBR sound it opens (1,439 of 2,200 lines in
/// chotchki's 2026-11-22 playtest captures, openspec AS.823): its module
/// alone goes to WARN, the rest of symphonia stays at INFO.
pub const DEFAULT_FILTER: &str =
    "info,skylander=debug,guac=debug,symphonia_bundle_mp3::demuxer=warn";

pub fn init(log_dir: &PathBuf) -> Result<WorkerGuard> {
    std::fs::create_dir_all(log_dir)?;
    let appender = tracing_appender::rolling::daily(log_dir, "server.log");
    let (file_writer, guard) = tracing_appender::non_blocking(appender);

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));

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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing::Subscriber;
    use tracing_subscriber::layer::{Context, Layer};

    use super::*;

    #[derive(Clone, Default)]
    struct Seen(Arc<Mutex<Vec<(String, tracing::Level)>>>);

    impl<S: Subscriber> Layer<S> for Seen {
        fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
            let m = event.metadata();
            self.0
                .lock()
                .unwrap()
                .push((m.target().to_string(), *m.level()));
        }
    }

    /// AS.823: the demuxer's INFO is dropped, its WARN kept, and nothing
    /// else of symphonia's (or ours) moves.
    #[test]
    fn the_mp3_demuxers_info_is_silenced_and_nothing_else() {
        let seen = Seen::default();
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::new(DEFAULT_FILTER))
            .with(seen.clone());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "symphonia_bundle_mp3::demuxer", "estimate");
            tracing::warn!(target: "symphonia_bundle_mp3::demuxer", "bad frame");
            tracing::info!(target: "symphonia_bundle_mp3::decoder", "other module");
            tracing::info!(target: "symphonia_core::probe", "other crate");
            tracing::debug!(target: "guac_server::http", "ours");
            tracing::debug!(target: "symphonia_core::probe", "not ours");
        });
        let seen = seen.0.lock().unwrap().clone();
        let targets: Vec<(&str, tracing::Level)> =
            seen.iter().map(|(t, l)| (t.as_str(), *l)).collect();
        assert_eq!(
            targets,
            vec![
                ("symphonia_bundle_mp3::demuxer", tracing::Level::WARN),
                ("symphonia_bundle_mp3::decoder", tracing::Level::INFO),
                ("symphonia_core::probe", tracing::Level::INFO),
                ("guac_server::http", tracing::Level::DEBUG),
            ]
        );
    }
}
