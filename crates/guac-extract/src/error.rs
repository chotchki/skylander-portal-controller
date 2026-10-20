//! What can stop an extraction. Each variant carries a stable `GUAC-E-*` code,
//! so the TV, the CLI and a log line all name the same thing.

use std::io;
use std::path::PathBuf;

use guac_ir::DumpIdentity;

#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// The dump isn't in the known table (design decision 4). Nothing was
    /// extracted; `report` lists every file's name, size and hash.
    #[error(
        "GUAC-E-UNKNOWN-DUMP: {} {} (manifest hash {}) isn't a dump guacamole knows, so nothing \
         was extracted; the file list is in {}",
        identity.title_id, identity.app_ver, identity.manifest_hash, report.display()
    )]
    UnknownDump {
        identity: DumpIdentity,
        report: PathBuf,
    },

    #[error("GUAC-E-DUMP-UNREADABLE: couldn't read the dump at {}: {reason}", path.display())]
    DumpUnreadable { path: PathBuf, reason: String },

    #[error(
        "GUAC-E-EXTRACT-IN-CHECKOUT: the extract folder {} is inside the git working tree at {}; \
         extracted game files may not live inside a source checkout",
        root.display(), repo.display()
    )]
    InCheckout { root: PathBuf, repo: PathBuf },

    #[error(
        "GUAC-E-EXTRACT-OVERLAPS-DUMP: the extract folder {} and the dump {} overlap; nothing is \
         ever written inside a dump",
        root.display(), dump.display()
    )]
    OverlapsDump { root: PathBuf, dump: PathBuf },

    #[error(
        "GUAC-E-NO-EXTRACT-DIR: GUAC_EXTRACT_DIR isn't set; a dev build needs an extract folder \
         outside the checkout and has no default"
    )]
    NoExtractDir,

    /// No ffmpeg in the settings, beside the executable, on PATH or (macOS) in
    /// Homebrew's bin folders; `install` is this OS's one-liner (the spec's
    /// missing-converter message).
    #[error(
        "GUAC-E-NO-FFMPEG: ffmpeg is needed to extract the intros and {}; install it with `{install}`",
        match configured {
            Some(p) => format!("the configured {} isn't a program", p.display()),
            None if crate::ffmpeg::FALLBACK_DIRS.is_empty() =>
                "none was found beside guacamole or on the PATH".to_owned(),
            None => "none was found beside guacamole, on the PATH or in Homebrew's folders"
                .to_owned(),
        }
    )]
    NoFfmpeg {
        configured: Option<PathBuf>,
        install: &'static str,
    },

    #[error("GUAC-E-FFMPEG-UNUSABLE: {}: {reason}", path.display())]
    FfmpegUnusable { path: PathBuf, reason: String },

    #[error("GUAC-E-TRANSCODE-FAILED: intro `{movie}`: {reason}")]
    TranscodeFailed { movie: String, reason: String },

    #[error("GUAC-E-BAD-OUTPUT-PATH: `{path}` isn't a plain relative path inside the extract")]
    BadOutputPath { path: String },

    /// The dump is a known one but its figure roster didn't read (AE.2):
    /// the phone's list stays empty and says so.
    #[error("GUAC-E-ROSTER-UNREADABLE: the figure roster in {} didn't read: {reason}", path.display())]
    RosterUnreadable { path: PathBuf, reason: String },

    /// The progression tables (AH.2) didn't read: the store clamps nothing
    /// and the phone's figure editor stays off, and the next start tries
    /// them again.
    #[error("GUAC-E-PROGRESSION-UNREADABLE: the progression tables in {} didn't read: {reason}", path.display())]
    ProgressionUnreadable { path: PathBuf, reason: String },

    /// A character's model, or the 3D-UI rig in `permanent/global.bld`,
    /// didn't read (AE.6): a model's figures keep their portraits on the TV,
    /// and the other models extract.
    #[error("GUAC-E-MODEL-UNREADABLE: {load_file} didn't read: {reason}")]
    ModelUnreadable { load_file: String, reason: String },

    /// A character's hero side (AG.14) didn't settle: its model extracted,
    /// but nothing says how it walks, so it doesn't.
    #[error("GUAC-E-HERO-UNSETTLED: {load_file}'s hero didn't settle: {reason}")]
    HeroUnsettled { load_file: String, reason: String },

    /// A part of a level guacamole plays (AG.15) didn't read: the parts that
    /// did are in the extract, and the next start tries this one again.
    #[error("GUAC-E-LEVEL-UNREADABLE: {level}'s {part} didn't read: {reason}")]
    LevelUnreadable {
        level: String,
        part: String,
        reason: String,
    },

    /// A part of the fight's script data outside a level (AH.22: the
    /// script entries or the spawn models) didn't read: the other parts
    /// land, and the next start tries this one again.
    #[error("GUAC-E-SCRIPTS-UNREADABLE: the {part} from {archive} didn't read: {reason}")]
    ScriptsUnreadable {
        part: String,
        archive: String,
        reason: String,
    },

    #[error("GUAC-E-EXTRACT-IO: {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl ExtractError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownDump { .. } => "GUAC-E-UNKNOWN-DUMP",
            Self::DumpUnreadable { .. } => "GUAC-E-DUMP-UNREADABLE",
            Self::InCheckout { .. } => "GUAC-E-EXTRACT-IN-CHECKOUT",
            Self::OverlapsDump { .. } => "GUAC-E-EXTRACT-OVERLAPS-DUMP",
            Self::NoExtractDir => "GUAC-E-NO-EXTRACT-DIR",
            Self::NoFfmpeg { .. } => "GUAC-E-NO-FFMPEG",
            Self::FfmpegUnusable { .. } => "GUAC-E-FFMPEG-UNUSABLE",
            Self::TranscodeFailed { .. } => "GUAC-E-TRANSCODE-FAILED",
            Self::BadOutputPath { .. } => "GUAC-E-BAD-OUTPUT-PATH",
            Self::RosterUnreadable { .. } => "GUAC-E-ROSTER-UNREADABLE",
            Self::ProgressionUnreadable { .. } => "GUAC-E-PROGRESSION-UNREADABLE",
            Self::ModelUnreadable { .. } => "GUAC-E-MODEL-UNREADABLE",
            Self::HeroUnsettled { .. } => "GUAC-E-HERO-UNSETTLED",
            Self::LevelUnreadable { .. } => "GUAC-E-LEVEL-UNREADABLE",
            Self::ScriptsUnreadable { .. } => "GUAC-E-SCRIPTS-UNREADABLE",
            Self::Io { .. } => "GUAC-E-EXTRACT-IO",
        }
    }

    pub(crate) fn io(path: impl Into<PathBuf>) -> impl FnOnce(io::Error) -> Self {
        let path = path.into();
        move |source| Self::Io { path, source }
    }

    pub(crate) fn unreadable(path: impl Into<PathBuf>, reason: impl ToString) -> Self {
        Self::DumpUnreadable {
            path: path.into(),
            reason: reason.to_string(),
        }
    }
}

pub type Result<T, E = ExtractError> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_starts_with_its_code() {
        let identity = DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.01".into(),
            manifest_hash: guac_ir::Sha256([0; 32]),
        };
        let errors = [
            ExtractError::UnknownDump {
                identity,
                report: "r.json".into(),
            },
            ExtractError::unreadable("d", "x"),
            ExtractError::InCheckout {
                root: "a".into(),
                repo: "b".into(),
            },
            ExtractError::OverlapsDump {
                root: "a".into(),
                dump: "b".into(),
            },
            ExtractError::NoExtractDir,
            ExtractError::NoFfmpeg {
                configured: None,
                install: "brew install ffmpeg",
            },
            ExtractError::NoFfmpeg {
                configured: Some("f".into()),
                install: "brew install ffmpeg",
            },
            ExtractError::FfmpegUnusable {
                path: "f".into(),
                reason: "x".into(),
            },
            ExtractError::TranscodeFailed {
                movie: "m".into(),
                reason: "x".into(),
            },
            ExtractError::BadOutputPath { path: "..".into() },
            ExtractError::RosterUnreadable {
                path: "character/init_setup.bld".into(),
                reason: "x".into(),
            },
            ExtractError::ProgressionUnreadable {
                path: "character/init_setup.bld".into(),
                reason: "x".into(),
            },
            ExtractError::ModelUnreadable {
                load_file: "009_Eruptor".into(),
                reason: "x".into(),
            },
            ExtractError::HeroUnsettled {
                load_file: "009_Eruptor".into(),
                reason: "x".into(),
            },
            ExtractError::LevelUnreadable {
                level: "level_213_tutotial".into(),
                part: "scene".into(),
                reason: "x".into(),
            },
            ExtractError::ScriptsUnreadable {
                part: "scripts".into(),
                archive: "character/009_eruptor.bld".into(),
                reason: "x".into(),
            },
            ExtractError::io("p")(io::Error::other("x")),
        ];
        for e in errors {
            assert!(e.to_string().starts_with(&format!("{}: ", e.code())), "{e}");
        }
    }
}
