//! The oracle's isolated HOME (openspec AF, design decision 5). RPCS3 on the Mac
//! keeps its config, hdd0, hdd1 and savestates under
//! `$HOME/Library/Application Support/rpcs3/` and its logs and caches under
//! `$HOME/Library/Caches/rpcs3/`, so launching it with `HOME` pointed here keeps
//! every write of an oracle run, a savestate load's wipe of hdd0 and hdd1
//! included, away from the dev's own RPCS3.
//!
//! `init` copies `dev_flash` from the dev's RPCS3 dir (a copy, never a link: a
//! link would carry a write back), points `games.yml` at the Giants dump and
//! writes the [`crate::template`] files. It refuses a home inside any work tree
//! of this repo and one that overlaps the dev's RPCS3 dirs.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::template::{self, ConfigTemplate};

/// The file that marks a directory as an oracle home. A launch refuses a HOME
/// without it, so the oracle can't be started against a real one by mistake.
pub const MARKER: &str = ".guac-oracle-home";

/// RPCS3's config dir under a HOME, as `fs::get_config_dir` resolves it.
pub fn rpcs3_config_dir(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/rpcs3")
    } else {
        home.join(".config/rpcs3")
    }
}

/// RPCS3's cache dir (logs, the lockfile, PPU/SPU/shader caches) under a HOME.
pub fn rpcs3_cache_dir(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Caches/rpcs3")
    } else {
        home.join(".cache/rpcs3")
    }
}

/// A home `init` built: the marker is there and it sits outside every work tree.
#[derive(Debug, Clone)]
pub struct OracleHome {
    root: PathBuf,
    giants_dump: PathBuf,
}

impl OracleHome {
    /// Opens a home `init` built, refusing one inside a work tree of this repo.
    pub fn open(root: &Path) -> Result<Self> {
        let root = rrc_dump::output::refuse_repo_output(root)?;
        let marker = root.join(MARKER);
        let text = fs::read_to_string(&marker).with_context(|| {
            format!(
                "{} isn't an oracle home (no {MARKER}); run `oracle home init` first",
                root.display()
            )
        })?;
        let giants_dump = text
            .lines()
            .find_map(|l| l.strip_prefix("giants_dump="))
            .map(PathBuf::from)
            .with_context(|| format!("{} has no giants_dump= line", marker.display()))?;
        Ok(Self { root, giants_dump })
    }

    /// The directory RPCS3 gets as `HOME`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn config_dir(&self) -> PathBuf {
        rpcs3_config_dir(&self.root)
    }

    pub fn cache_dir(&self) -> PathBuf {
        rpcs3_cache_dir(&self.root)
    }

    /// The Giants dump root, the folder holding `PS3_GAME`.
    pub fn giants_dump(&self) -> &Path {
        &self.giants_dump
    }

    /// Giants' EBOOT on the dump, the path a cold boot passes RPCS3.
    pub fn giants_eboot(&self) -> PathBuf {
        self.giants_dump.join("PS3_GAME/USRDIR/EBOOT.BIN")
    }
}

#[derive(Debug, Clone)]
pub struct InitOptions {
    /// The dev's real HOME: its RPCS3 config dir is where `dev_flash` comes from,
    /// and its two RPCS3 dirs are what the oracle home may never overlap.
    pub real_home: PathBuf,
    /// The Giants dump root (`GUAC_GIANTS_DUMP`).
    pub giants_dump: PathBuf,
    pub config: ConfigTemplate,
}

/// Builds (or refreshes) the oracle home at `home`. `dev_flash` is copied once;
/// the config files are rewritten every time, so a re-init picks up a new
/// template. hdd0 and hdd1 are left as RPCS3 or a savestate load left them.
pub fn init(home: &Path, opts: &InitOptions) -> Result<OracleHome> {
    let root = rrc_dump::output::refuse_repo_output(home)?;
    let real_dirs = [
        rpcs3_config_dir(&opts.real_home),
        rpcs3_cache_dir(&opts.real_home),
    ];
    refuse_overlap(&root, &real_dirs)?;
    if cfg!(windows) {
        bail!("the oracle home is Mac only until the Windows/HTPC bring-up");
    }
    let eboot = opts.giants_dump.join("PS3_GAME/USRDIR/EBOOT.BIN");
    if !eboot.is_file() {
        bail!(
            "no PS3_GAME/USRDIR/EBOOT.BIN under the Giants dump {}",
            opts.giants_dump.display()
        );
    }
    let source_flash = real_dirs[0].join("dev_flash");
    if !source_flash.is_dir() {
        bail!(
            "no dev_flash in the dev's RPCS3 dir {} (install the PS3 firmware there first)",
            real_dirs[0].display()
        );
    }

    let config = rpcs3_config_dir(&root);
    let cache = rpcs3_cache_dir(&root);
    for dir in [&config, &cache] {
        refuse_link_on_the_way(&root, dir)?;
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    // Physical paths now that they exist: a link anywhere above them would show.
    let physical = [config.canonicalize()?, cache.canonicalize()?];
    for dir in &physical {
        refuse_overlap(dir, &real_dirs)?;
    }

    let flash = config.join("dev_flash");
    if is_link(&flash) {
        bail!("{} is a link; an oracle home holds a copy", flash.display());
    }
    if !flash.exists() {
        let partial = config.join("dev_flash.partial");
        if partial.exists() {
            fs::remove_dir_all(&partial)?;
        }
        copy_tree(&source_flash, &partial)?;
        fs::rename(&partial, &flash)?;
    }
    for name in ["dev_hdd0", "dev_hdd1"] {
        let dir = config.join(name);
        if is_link(&dir) {
            bail!(
                "{} is a link; a savestate load would wipe what it points at",
                dir.display()
            );
        }
        fs::create_dir_all(&dir)?;
    }

    write(&config.join("config.yml"), &opts.config.render())?;
    write(&config.join("ipc.yml"), template::ipc_yml())?;
    write(
        &config.join("games.yml"),
        &template::games_yml(&opts.giants_dump),
    )?;
    write(
        &config.join("input_configs/global/Default.yml"),
        template::input_config(),
    )?;
    write(
        &config.join("GuiConfigs/CurrentSettings.ini"),
        template::gui_settings(),
    )?;
    write(
        &root.join(MARKER),
        &format!(
            "# An oracle home (openspec AF): RPCS3 runs with HOME set here.\ngiants_dump={}\n",
            opts.giants_dump.display()
        ),
    )?;
    OracleHome::open(&root)
}

/// Refuses `path` when it and any of `dirs` contain one another (or are equal).
fn refuse_overlap(path: &Path, dirs: &[PathBuf]) -> Result<()> {
    for dir in dirs {
        let dir = dir.canonicalize().unwrap_or_else(|_| dir.clone());
        if path.starts_with(&dir) || dir.starts_with(path) {
            bail!(
                "refusing {}: it overlaps the dev's own RPCS3 dir {}, which an oracle run must never write",
                path.display(),
                dir.display()
            );
        }
    }
    Ok(())
}

/// Refuses a symlink at any existing component of `dir` below `root`.
fn refuse_link_on_the_way(root: &Path, dir: &Path) -> Result<()> {
    let rel = dir.strip_prefix(root)?;
    let mut at = root.to_path_buf();
    for part in rel.components() {
        at.push(part);
        if is_link(&at) {
            bail!("{} is a link; an oracle home holds real dirs", at.display());
        }
    }
    Ok(())
}

fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// Copies a tree file by file, refusing links in it: what lands is the bytes.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if kind.is_symlink() {
            bail!("refusing to copy the link {}", src.display());
        } else if kind.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            fs::copy(&src, &dst).with_context(|| format!("copying {}", src.display()))?;
        }
    }
    Ok(())
}

fn write(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    /// A fake dev HOME with an RPCS3 dir holding a two-file `dev_flash` and a
    /// Giants dump with an EBOOT, both in a tempdir outside any work tree.
    struct Fixture {
        tmp: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let real = rpcs3_config_dir(&tmp.path().join("real-home"));
            write(&real.join("dev_flash/vsh/module/a.sprx"), "flash a").unwrap();
            write(&real.join("dev_flash/sys/b.self"), "flash b").unwrap();
            write(&real.join("dev_hdd0/game/save"), "the dev's save").unwrap();
            write(
                &tmp.path().join("dump/PS3_GAME/USRDIR/EBOOT.BIN"),
                "not an eboot",
            )
            .unwrap();
            Self { tmp }
        }

        fn opts(&self) -> InitOptions {
            InitOptions {
                real_home: self.tmp.path().join("real-home"),
                giants_dump: self.tmp.path().join("dump"),
                config: ConfigTemplate::default(),
            }
        }

        fn real_config(&self) -> PathBuf {
            rpcs3_config_dir(&self.tmp.path().join("real-home"))
        }
    }

    #[test]
    fn init_builds_a_home_rpcs3_can_run_in() {
        let fx = Fixture::new();
        let home = init(&fx.tmp.path().join("oracle"), &fx.opts()).unwrap();
        let config = home.config_dir();
        for file in [
            "config.yml",
            "ipc.yml",
            "games.yml",
            "input_configs/global/Default.yml",
            "GuiConfigs/CurrentSettings.ini",
            "dev_flash/vsh/module/a.sprx",
        ] {
            assert!(config.join(file).is_file(), "{file}");
        }
        assert!(home.cache_dir().is_dir());
        assert!(
            fs::read_to_string(config.join("config.yml"))
                .unwrap()
                .contains("Start Paused: true")
        );
        assert_eq!(
            home.giants_eboot(),
            fx.tmp.path().join("dump/PS3_GAME/USRDIR/EBOOT.BIN")
        );
        // Re-init is fine and keeps the copy.
        init(home.root(), &fx.opts()).unwrap();
        let reopened = OracleHome::open(home.root()).unwrap();
        assert_eq!(reopened.giants_dump(), fx.tmp.path().join("dump"));
    }

    #[test]
    fn hdd0_and_dev_flash_are_copies_never_links() {
        let fx = Fixture::new();
        let home = init(&fx.tmp.path().join("oracle"), &fx.opts()).unwrap();
        let config = home.config_dir();
        for name in ["dev_flash", "dev_hdd0", "dev_hdd1"] {
            let meta = fs::symlink_metadata(config.join(name)).unwrap();
            assert!(meta.is_dir() && !meta.file_type().is_symlink(), "{name}");
        }
        // Own inodes: a write to the oracle's copy can't reach the dev's file.
        for file in ["vsh/module/a.sprx", "sys/b.self"] {
            let ours = fs::metadata(config.join("dev_flash").join(file)).unwrap();
            let theirs = fs::metadata(fx.real_config().join("dev_flash").join(file)).unwrap();
            assert_ne!(
                (ours.dev(), ours.ino()),
                (theirs.dev(), theirs.ino()),
                "{file}"
            );
        }
        // The dev's saves stay theirs: the oracle's hdd0 starts empty.
        assert_eq!(fs::read_dir(config.join("dev_hdd0")).unwrap().count(), 0);

        // A home whose hdd0 is a link into the dev's RPCS3 dir is refused.
        let trapped = fx.tmp.path().join("trapped");
        let trapped_config = rpcs3_config_dir(&trapped);
        fs::create_dir_all(&trapped_config).unwrap();
        std::os::unix::fs::symlink(
            fx.real_config().join("dev_hdd0"),
            trapped_config.join("dev_hdd0"),
        )
        .unwrap();
        let err = init(&trapped, &fx.opts()).unwrap_err().to_string();
        assert!(err.contains("is a link"), "{err}");
        // A linked Library above the config dir is refused too.
        let linked = fx.tmp.path().join("linked");
        fs::create_dir_all(&linked).unwrap();
        std::os::unix::fs::symlink(
            fx.tmp.path().join("real-home/Library"),
            linked.join("Library"),
        )
        .unwrap();
        let err = init(&linked, &fx.opts()).unwrap_err().to_string();
        assert!(err.contains("link") || err.contains("overlaps"), "{err}");
        // And the dev's file is untouched by all of it.
        assert_eq!(
            fs::read_to_string(fx.real_config().join("dev_hdd0/game/save")).unwrap(),
            "the dev's save"
        );
    }

    #[test]
    fn init_refuses_the_devs_own_rpcs3_dirs() {
        let fx = Fixture::new();
        // HOME itself: its config dir is the real one.
        let err = init(&fx.tmp.path().join("real-home"), &fx.opts())
            .unwrap_err()
            .to_string();
        assert!(err.contains("overlaps the dev's own RPCS3 dir"), "{err}");
        // A home inside the real config dir, and one inside the real cache dir.
        for inside in [
            fx.real_config().join("oracle"),
            rpcs3_cache_dir(&fx.tmp.path().join("real-home")).join("oracle"),
        ] {
            let err = init(&inside, &fx.opts()).unwrap_err().to_string();
            assert!(err.contains("overlaps"), "{err}");
        }
        assert!(!fx.real_config().join("oracle").exists());
    }

    #[test]
    fn init_refuses_a_home_inside_a_work_tree_of_this_repo() {
        let fx = Fixture::new();
        let inside = Path::new(env!("CARGO_MANIFEST_DIR")).join("no-such-dir/oracle-home");
        let err = init(&inside, &fx.opts()).unwrap_err().to_string();
        assert!(
            err.contains("refusing to write capture data inside"),
            "{err}"
        );
        assert!(!inside.parent().unwrap().exists());
        let err = OracleHome::open(&inside).unwrap_err().to_string();
        assert!(err.contains("refusing"), "{err}");
    }

    #[test]
    fn open_refuses_a_dir_init_never_built() {
        let tmp = tempfile::tempdir().unwrap();
        let err = OracleHome::open(tmp.path()).unwrap_err().to_string();
        assert!(err.contains("isn't an oracle home"), "{err}");
    }
}
