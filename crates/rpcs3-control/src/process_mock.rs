//! Portable fake for `RpcsProcess`. Used under `DriverKind::Mock` so
//! Mac/Linux dev (and Windows mock-driver tests) can satisfy the
//! always-running-RPCS3 contract without an actual emulator.
//!
//! Semantics: always "alive" from construction until
//! `shutdown_graceful` / `wait_for_exit_or_force` is called. `wait_ready`
//! returns immediately. `pid()` returns 0 — no real OS process.

use std::time::Duration;

use anyhow::Result;

use crate::ShutdownPath;

#[derive(Debug)]
pub struct MockRpcsProcess {
    alive: bool,
}

impl MockRpcsProcess {
    pub fn new() -> Self {
        Self { alive: true }
    }

    pub fn pid(&self) -> u32 {
        0
    }

    pub fn wait_ready(&mut self, _timeout: Duration) -> Result<()> {
        Ok(())
    }

    pub fn is_alive(&mut self) -> bool {
        self.alive
    }

    pub fn shutdown_graceful(&mut self, timeout: Duration) -> Result<ShutdownPath> {
        self.wait_for_exit_or_force(timeout)
    }

    pub fn wait_for_exit_or_force(&mut self, _timeout: Duration) -> Result<ShutdownPath> {
        if !self.alive {
            return Ok(ShutdownPath::AlreadyExited);
        }
        self.alive = false;
        Ok(ShutdownPath::Graceful)
    }
}

impl Default for MockRpcsProcess {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_alive_and_wait_ready_returns_immediately() {
        let mut p = MockRpcsProcess::new();
        assert!(p.is_alive());
        p.wait_ready(Duration::from_secs(30)).unwrap();
        assert!(p.is_alive());
    }

    #[test]
    fn shutdown_flips_alive_to_false() {
        let mut p = MockRpcsProcess::new();
        let path = p.shutdown_graceful(Duration::from_secs(1)).unwrap();
        assert_eq!(path, ShutdownPath::Graceful);
        assert!(!p.is_alive());

        let path = p.shutdown_graceful(Duration::from_secs(1)).unwrap();
        assert_eq!(path, ShutdownPath::AlreadyExited);
    }
}
