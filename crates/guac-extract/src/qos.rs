//! The extractor's place below the TV (openspec AG.298). On macOS a thread
//! runs at a quality-of-service class, and guacamole puts its extraction's
//! thread at [`Qos::Utility`], under the TV's frame and its level load, which
//! run at the default and above. What that thread starts doesn't follow it on
//! its own (measured on the M3 Max, 2026-11-01): a thread it spawns starts at
//! the default class, and a process it spawns starts its main thread at user
//! interactive, ffmpeg's encoder threads at the default. So each thread the
//! extractor starts takes its spawner's class ([`follow`]), and the
//! transcodes' ffmpeg runs under `taskpolicy -c`, which clamps every thread
//! of the process to that class ([`command`]): `posix_spawnattr_set_qos_class_np`
//! would set the main thread's alone, and `std::process::Command` can't pass
//! it. A thread at the default class or above changes nothing, so the CLI run
//! from a terminal runs as it always did. Elsewhere (Windows) every call is a
//! no-op and [`current`] reads [`Qos::Unspecified`].

use std::ffi::OsStr;
use std::process::Command;

/// A thread's quality-of-service class, macOS's names, highest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Qos {
    UserInteractive,
    UserInitiated,
    Default,
    Utility,
    Background,
    /// None asked, or not macOS.
    Unspecified,
}

impl Qos {
    /// Below the default: what [`follow`] and [`command`] pass on.
    pub fn is_below_default(self) -> bool {
        matches!(self, Self::Utility | Self::Background)
    }

    /// `taskpolicy -c`'s name for it, for a class below the default.
    fn clamp(self) -> Option<&'static str> {
        match self {
            Self::Utility => Some("utility"),
            Self::Background => Some("background"),
            _ => None,
        }
    }
}

/// macOS's own, by its whole path, so nothing earlier on `PATH` stands in
/// for it.
pub const TASKPOLICY: &str = "/usr/sbin/taskpolicy";

#[cfg(target_os = "macos")]
mod mac {
    use super::Qos;

    unsafe extern "C" {
        // <pthread/qos.h>; read as the raw class so a value the header adds
        // later can't make a bad enum.
        fn qos_class_self() -> u32;
    }

    pub fn current() -> Qos {
        // SAFETY: no arguments, reads the calling thread's own class.
        match unsafe { qos_class_self() } {
            0x21 => Qos::UserInteractive,
            0x19 => Qos::UserInitiated,
            0x15 => Qos::Default,
            0x11 => Qos::Utility,
            0x09 => Qos::Background,
            _ => Qos::Unspecified,
        }
    }

    pub fn set_current(q: Qos) -> Result<(), i32> {
        use libc::qos_class_t::*;
        let class = match q {
            Qos::UserInteractive => QOS_CLASS_USER_INTERACTIVE,
            Qos::UserInitiated => QOS_CLASS_USER_INITIATED,
            Qos::Default => QOS_CLASS_DEFAULT,
            Qos::Utility => QOS_CLASS_UTILITY,
            Qos::Background => QOS_CLASS_BACKGROUND,
            Qos::Unspecified => QOS_CLASS_UNSPECIFIED,
        };
        // SAFETY: sets the calling thread's own class; a class it refuses
        // comes back as the error number.
        match unsafe { libc::pthread_set_qos_class_self_np(class, 0) } {
            0 => Ok(()),
            e => Err(e),
        }
    }
}

/// The calling thread's class.
pub fn current() -> Qos {
    #[cfg(target_os = "macos")]
    {
        mac::current()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Qos::Unspecified
    }
}

/// Puts the calling thread at `q`; `Err` holds the error number macOS gave.
/// A no-op elsewhere.
pub fn set_current(q: Qos) -> Result<(), i32> {
    #[cfg(target_os = "macos")]
    {
        mac::set_current(q)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = q;
        Ok(())
    }
}

/// Puts the calling thread at `spawner`'s class when that's below the
/// default; a thread the extractor starts calls it first with
/// [`current`] as its spawner read it.
pub fn follow(spawner: Qos) {
    if spawner.is_below_default() {
        // Refused, it runs at the default class, as it did before AG.298.
        let _ = set_current(spawner);
    }
}

/// A [`Command`] for `program` whose every thread runs at the calling
/// thread's class when that's below the default: under [`TASKPOLICY`]`-c`
/// on macOS when it's there, else `program` as it is.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    clamped(
        program,
        current(),
        std::path::Path::new(TASKPOLICY).is_file(),
    )
}

/// [`command`] at `qos`, with `taskpolicy` there or not.
fn clamped(program: impl AsRef<OsStr>, qos: Qos, taskpolicy: bool) -> Command {
    match qos.clamp() {
        Some(clamp) if cfg!(target_os = "macos") && taskpolicy => {
            let mut c = Command::new(TASKPOLICY);
            c.arg("-c").arg(clamp).arg(program);
            c
        }
        _ => Command::new(program),
    }
}

#[cfg(test)]
pub(crate) mod tests;
