//! Finding ffmpeg (design decision 8): the path in guacamole's settings
//! (`GUAC_FFMPEG` in dev), else beside the running executable, else PATH.
//! ffprobe for the census is found the same way. Nothing is bundled.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{ExtractError, Result};

/// This OS's one-line ffmpeg install, for the missing-ffmpeg message.
pub const INSTALL_LINE: &str = install_line(std::env::consts::OS);

pub const fn install_line(os: &str) -> &'static str {
    match os.as_bytes() {
        b"macos" => "brew install ffmpeg",
        b"windows" => "winget install Gyan.FFmpeg",
        _ => "sudo apt install ffmpeg",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    Configured,
    BesideExe,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub how: Where,
}

/// `tool` (`ffmpeg`, `ffprobe`) by the discovery order. A configured path wins
/// outright: when it isn't a program the answer is `Err`, never a fallback
/// that hides the broken setting.
pub fn find_tool(
    tool: &str,
    configured: Option<&Path>,
    exe_dir: Option<&Path>,
    path_var: Option<&OsStr>,
) -> Result<Found, Option<PathBuf>> {
    if let Some(p) = configured {
        return if is_program(p) {
            Ok(Found {
                path: p.to_path_buf(),
                how: Where::Configured,
            })
        } else {
            Err(Some(p.to_path_buf()))
        };
    }
    let name = format!("{tool}{}", std::env::consts::EXE_SUFFIX);
    if let Some(p) = exe_dir.map(|d| d.join(&name)).filter(|p| is_program(p)) {
        return Ok(Found {
            path: p,
            how: Where::BesideExe,
        });
    }
    path_var
        .into_iter()
        .flat_map(std::env::split_paths)
        .map(|d| d.join(&name))
        .find(|p| is_program(p))
        .map(|path| Found {
            path,
            how: Where::Path,
        })
        .ok_or(None)
}

/// [`find_tool`] over this process: its executable's folder and its PATH.
pub fn find_tool_here(tool: &str, configured: Option<&Path>) -> Result<Found, Option<PathBuf>> {
    let exe = std::env::current_exe().ok();
    let exe_dir = exe.as_deref().and_then(Path::parent);
    let path = std::env::var_os("PATH");
    find_tool(tool, configured, exe_dir, path.as_deref())
}

fn is_program(p: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(p) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

/// An ffmpeg that runs and has what [`crate::transcode`] asks of it.
#[derive(Debug, Clone)]
pub struct Ffmpeg {
    pub path: PathBuf,
    pub how: Where,
    /// The first line of `ffmpeg -version`.
    pub version: String,
}

impl Ffmpeg {
    /// Finds ffmpeg from this process and checks it: `GUAC-E-NO-FFMPEG` when
    /// there's none, `GUAC-E-FFMPEG-UNUSABLE` when it won't run or lacks a
    /// codec the transcode needs.
    pub fn locate(configured: Option<&Path>) -> Result<Self> {
        let found =
            find_tool_here("ffmpeg", configured).map_err(|configured| ExtractError::NoFfmpeg {
                configured,
                install: INSTALL_LINE,
            })?;
        Self::check(found)
    }

    pub fn check(found: Found) -> Result<Self> {
        let path = found.path;
        let unusable = |reason: String| ExtractError::FfmpegUnusable {
            path: path.clone(),
            reason,
        };
        let version = run_text(&path, &["-version"]).map_err(unusable)?;
        let version = version.lines().next().unwrap_or_default().to_owned();
        let encoders =
            codec_names(&run_text(&path, &["-hide_banner", "-encoders"]).map_err(unusable)?);
        let decoders =
            codec_names(&run_text(&path, &["-hide_banner", "-decoders"]).map_err(unusable)?);
        let t = &crate::transcode::TARGET;
        let missing: Vec<String> = t
            .encoders
            .iter()
            .filter(|c| !encoders.contains(**c))
            .map(|c| format!("encoder {c}"))
            .chain(
                t.decoders
                    .iter()
                    .filter(|c| !decoders.contains(**c))
                    .map(|c| format!("decoder {c}")),
            )
            .collect();
        if !missing.is_empty() {
            return Err(unusable(format!(
                "{version} lacks {}; install it with `{INSTALL_LINE}`",
                missing.join(", ")
            )));
        }
        Ok(Self {
            path,
            how: found.how,
            version,
        })
    }
}

fn run_text(exe: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new(exe)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "`{}` failed ({}): {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The names in `ffmpeg -encoders` or `-decoders`: the second column of every
/// line after the `------` rule.
pub fn codec_names(listing: &str) -> BTreeSet<String> {
    listing
        .lines()
        .skip_while(|l| !l.trim_start().starts_with("------"))
        .skip(1)
        .filter_map(|l| l.split_whitespace().nth(1).map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_lines_per_os() {
        assert_eq!(install_line("macos"), "brew install ffmpeg");
        assert_eq!(install_line("windows"), "winget install Gyan.FFmpeg");
        assert_eq!(install_line("linux"), "sudo apt install ffmpeg");
    }

    #[test]
    fn codec_names_read_the_listing() {
        // ffmpeg 9.0.2's `-encoders`, trimmed.
        let listing = "\
Encoders:
 V..... = Video
 A..... = Audio
 .....D = Supports direct rendering method 1
 ------
 V....D a64multi             Multicolor charset for Commodore 64 (codec a64_multi)
 V....D libx264              libx264 H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10 (codec h264)
 A....D libmp3lame           libmp3lame MP3 (MPEG audio layer 3) (codec mp3)
";
        let names = codec_names(listing);
        assert_eq!(
            names.into_iter().collect::<Vec<_>>(),
            ["a64multi", "libmp3lame", "libx264"]
        );
        assert!(codec_names("no rule here\n V....D libx264 x").is_empty());
    }

    /// A fake `name` in `dir`: a shell script on unix, an empty `.exe` file
    /// elsewhere (Windows has no execute bit to check).
    fn fake(dir: &Path, name: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&p, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    }

    #[test]
    fn discovery_order_is_settings_then_beside_then_path() {
        let tmp = tempfile::tempdir().unwrap();
        let t = tmp.path();
        let settings = fake(&t.join("settings"), "ffmpeg");
        let beside = fake(&t.join("app"), "ffmpeg");
        let path1 = fake(&t.join("p1"), "ffmpeg");
        let path2 = fake(&t.join("p2"), "ffmpeg");
        let path_var = std::env::join_paths([t.join("empty"), t.join("p1"), t.join("p2")]).unwrap();
        let find = |configured: Option<&Path>, exe_dir: Option<&Path>| {
            find_tool("ffmpeg", configured, exe_dir, Some(&path_var))
        };
        let app = t.join("app");
        assert_eq!(
            find(Some(&settings), Some(&app)),
            Ok(Found {
                path: settings.clone(),
                how: Where::Configured
            })
        );
        assert_eq!(
            find(None, Some(&app)),
            Ok(Found {
                path: beside.clone(),
                how: Where::BesideExe
            })
        );
        // Nothing beside: PATH, first folder holding it.
        assert_eq!(
            find(None, Some(&t.join("empty"))),
            Ok(Found {
                path: path1.clone(),
                how: Where::Path
            })
        );
        std::fs::remove_file(&path1).unwrap();
        assert_eq!(find(None, None).map(|f| f.path), Ok(path2.clone()));
        std::fs::remove_file(&path2).unwrap();
        assert_eq!(find(None, Some(&t.join("empty"))), Err(None));
        // A broken setting is the answer, not a fall-through to `beside`.
        let gone = t.join("settings/nope");
        assert_eq!(find(Some(&gone), Some(&app)), Err(Some(gone)));
        // Another tool's name doesn't count.
        assert_eq!(
            find_tool("ffprobe", None, Some(&app), Some(&path_var)),
            Err(None)
        );
    }

    #[cfg(unix)]
    #[test]
    fn discovery_skips_what_isnt_a_program() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let t = tmp.path();
        // A folder named ffmpeg, then a file without the execute bit.
        std::fs::create_dir_all(t.join("p1/ffmpeg")).unwrap();
        let plain = fake(&t.join("p2"), "ffmpeg");
        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();
        let real = fake(&t.join("p3"), "ffmpeg");
        let path_var = std::env::join_paths([t.join("p1"), t.join("p2"), t.join("p3")]).unwrap();
        assert_eq!(
            find_tool("ffmpeg", None, None, Some(&path_var)).map(|f| f.path),
            Ok(real)
        );
        assert_eq!(
            find_tool("ffmpeg", Some(&plain), None, Some(&path_var)),
            Err(Some(plain))
        );
    }

    #[test]
    fn a_missing_ffmpeg_names_the_install_line() {
        let e = ExtractError::NoFfmpeg {
            configured: None,
            install: INSTALL_LINE,
        };
        let text = e.to_string();
        assert!(text.starts_with("GUAC-E-NO-FFMPEG: "), "{text}");
        assert!(text.contains(INSTALL_LINE), "{text}");
        if cfg!(target_os = "macos") {
            assert!(text.contains("`brew install ffmpeg`"), "{text}");
        }
    }
}
