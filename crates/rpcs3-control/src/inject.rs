//! Failure injection for the e2e harness's `/api/_test/inject_load`: the mock
//! and guacamole's native driver both take queued outcomes (openspec AA,
//! design decision 4).

use std::thread::sleep;
use std::time::Duration;

use anyhow::{Result, anyhow};

/// An outcome for a driver's next load or placement, consumed FIFO. With
/// nothing queued the driver behaves normally.
#[derive(Debug, Clone)]
pub enum InjectedOutcome {
    /// Lets one call through ahead of later queued failures.
    Ok,
    /// Simulate the Windows shell "file is in use" TaskDialog path.
    FileInUse { message: String },
    /// Simulate an RPCS3 QMessageBox like "Failed to open the skylander file!".
    QtModal { message: String },
    /// Sleep past the driver's load timeout so the outer loop times out.
    Timeout,
}

impl InjectedOutcome {
    /// `Ok` for [`Self::Ok`]; the failure the outcome stands for otherwise.
    /// `Timeout` blocks for 11 s first, past the real UIA driver's ~10 s.
    pub fn into_result(self) -> Result<()> {
        match self {
            Self::Ok => Ok(()),
            Self::FileInUse { message } => Err(anyhow!("Windows file in use: {message}")),
            Self::QtModal { message } => Err(anyhow!("RPCS3 reported: {message}")),
            Self::Timeout => {
                sleep(Duration::from_secs(11));
                Err(anyhow!("timeout"))
            }
        }
    }
}

/// A driver that takes injected outcomes for its upcoming loads or placements.
pub trait LoadInjector: Send + Sync {
    /// Queue `outcomes` behind any already queued.
    fn queue_load_outcomes(&self, outcomes: Vec<InjectedOutcome>);
}
