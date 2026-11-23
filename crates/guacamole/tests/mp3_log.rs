//! openspec AS.823: the mp3 demuxer's duration estimate, logged as the TV
//! opens a sound, kept out of the log by the app's own filter. A CBR file
//! with no Xing or VBRI header (what the extract's sounds are) is opened
//! through the TV's own check, [`guacamole::voice::opens`], with `log`
//! bridged into tracing as `guac_server::logging::init` bridges it, and the
//! demuxer's line counted under the app's filter and under plain INFO.
//! Nothing plays: no audio device is opened.

use std::sync::{Arc, Mutex};

use tracing::Subscriber;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry};

/// Shared, so the test reads the count after the subscriber took the layer.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<usize>>);

/// A bridged `log` record's own target: its event's metadata says `log`,
/// the record's target rides in the `log.target` field.
#[derive(Default)]
struct Target(String);

impl tracing::field::Visit for Target {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "log.target" {
            self.0 = value.to_owned();
        }
    }

    fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
}

impl<S: Subscriber> Layer<S> for Seen {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        let mut target = Target::default();
        event.record(&mut target);
        if target.0 == "symphonia_bundle_mp3::demuxer"
            && *event.metadata().level() == tracing::Level::INFO
        {
            *self.0.lock().unwrap() += 1;
        }
    }
}

#[test]
fn opening_a_cbr_mp3_logs_no_duration_estimate_under_the_apps_filter() {
    let (app, all) = (Seen::default(), Seen::default());
    // One process-wide subscriber (the bridge is global): this file's only
    // test.
    Registry::default()
        .with(
            app.clone()
                .with_filter(EnvFilter::new(guac_server::logging::DEFAULT_FILTER)),
        )
        .with(all.clone().with_filter(EnvFilter::new("info")))
        .try_init()
        .expect("the subscriber and its log bridge");
    let bytes: Arc<[u8]> = guacamole::voice::tests_support::silent_mp3(40).into();
    assert!(guacamole::voice::opens(&bytes), "the fixture opens");
    let (app, all) = (*app.0.lock().unwrap(), *all.0.lock().unwrap());
    assert!(all >= 1, "plain INFO sees the demuxer's estimate: {all}");
    assert_eq!(app, 0, "the app's filter drops it");
}
