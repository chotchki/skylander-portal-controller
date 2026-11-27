//! The log's lines while a test's closure runs, at a level the test names
//! (openspec AS.1126: which level a noisy line goes out at is what's held).

use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Every line logged at `level` or above while `f` runs on this thread.
pub(crate) fn logged_at(level: tracing::Level, f: impl FnOnce()) -> Vec<String> {
    let buf = Buf::default();
    let out = buf.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(level)
        .with_writer(move || out.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, f);
    let text = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
    text.lines().map(str::to_owned).collect()
}
