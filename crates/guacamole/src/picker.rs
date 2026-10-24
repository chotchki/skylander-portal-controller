//! The setup screen's folder picker: rfd's async dialog opens as a sheet on
//! the window and a Bevy task awaits it, so frames keep running. The sync
//! call off the main thread stalls Bevy, so it's only used before the window
//! exists (`--setup`).

use std::path::PathBuf;
use std::time::Duration;

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::tasks::futures::check_ready;
use bevy::tasks::{IoTaskPool, Task};
use bevy::window::{PrimaryWindow, RawHandleWrapper};
use guac_server::dumps::DumpScan;
use tracing::{info, warn};

use crate::screen::{SetupView, TvScreen};
use crate::tv_state::{Current, ServerLink, Setup};

/// guacamole's data root, where a pick is stored.
#[derive(Resource, Clone)]
pub struct DataRoot(pub PathBuf);

/// The Setup screen opens the picker by itself once it has shown this long.
const AUTO_OPEN_AFTER: Duration = Duration::from_secs(1);

/// How long the found list shows before the join code.
pub const FOUND_BEAT: Duration = Duration::from_secs(8);

pub const PICKER_TITLE: &str = "Choose the folder that holds your disc dumps";

enum Picked {
    Cancelled,
    Stored {
        folders: Vec<PathBuf>,
        scan: DumpScan,
    },
    Failed(String),
}

#[derive(Resource, Default)]
pub(crate) struct Picking {
    task: Option<Task<Picked>>,
    auto_opened: bool,
    /// `should_open` said open; `open_picker` opens the sheet this frame.
    wanted: bool,
}

#[cfg(test)]
impl Picking {
    /// Setup's own opening done and its sheet closed again.
    pub(crate) fn closed_once() -> Self {
        Self {
            auto_opened: true,
            ..Default::default()
        }
    }

    /// Whether the picker was wanted since the last call.
    pub(crate) fn take_wanted(&mut self) -> bool {
        std::mem::take(&mut self.wanted)
    }
}

/// A press of a choose-folder button (`tv::on_choice`), not yet spent.
#[derive(Resource, Default)]
pub(crate) struct FolderAsked(pub(crate) bool);

/// The picker: when it opens and when Found gives way ([`PickerChoices`]),
/// and the sheet itself.
pub struct Picker;

impl Plugin for Picker {
    fn build(&self, app: &mut App) {
        app.add_plugins(PickerChoices)
            .add_systems(Update, (open_picker.after(want_picker), finish_pick));
    }
}

/// The picker's decisions without the sheet, so headless tests run them.
pub(crate) struct PickerChoices;

impl Plugin for PickerChoices {
    fn build(&self, app: &mut App) {
        app.init_resource::<Picking>()
            .init_resource::<FolderAsked>()
            .add_systems(Update, (want_picker, leave_found));
    }
}

/// Setup opens the picker once by itself; Setup and NoGiants open it on a
/// press of their choose-folder button. Never two at once.
pub fn should_open(
    screen: &TvScreen,
    shown_for: Duration,
    pressed: bool,
    auto_opened: bool,
    picking: bool,
) -> bool {
    if picking {
        return false;
    }
    match screen {
        TvScreen::Setup => pressed || (!auto_opened && shown_for >= AUTO_OPEN_AFTER),
        TvScreen::NoGiants => pressed,
        _ => false,
    }
}

/// [`should_open`] each frame. A button press is spent either way, so one
/// made while the sheet is up doesn't open another after it.
fn want_picker(
    current: Option<Res<Current>>,
    mut asked: ResMut<FolderAsked>,
    mut picking: ResMut<Picking>,
) {
    let pressed = asked.0;
    if pressed {
        asked.0 = false;
    }
    let Some(current) = current else {
        return;
    };
    let busy = picking.task.is_some() || picking.wanted;
    if should_open(
        &current.screen,
        current.since.elapsed(),
        pressed,
        picking.auto_opened,
        busy,
    ) {
        picking.auto_opened = true;
        picking.wanted = true;
    }
}

/// On the main thread: macOS builds the sheet where it's called.
fn open_picker(
    _main_thread: NonSendMarker,
    current: Option<Res<Current>>,
    window: Query<&RawHandleWrapper, With<PrimaryWindow>>,
    root: Res<DataRoot>,
    mut picking: ResMut<Picking>,
    mut setup: ResMut<Setup>,
) {
    if !picking.wanted {
        return;
    }
    picking.wanted = false;

    let mut dialog = rfd::AsyncFileDialog::new().set_title(PICKER_TITLE);
    match window.single() {
        Ok(wrapper) => {
            // SAFETY: a NonSendMarker system runs on the main thread, where
            // macOS requires the window to be used.
            let handle = unsafe { wrapper.get_handle() };
            dialog = dialog.set_parent(&handle);
        }
        Err(e) => warn!("no window handle, the picker opens on its own: {e}"),
    }
    let pick = dialog.pick_folder();
    let pick = async move { pick.await.map(|h| h.path().to_path_buf()) };
    let task = IoTaskPool::get().spawn(store_pick(pick, root.0.clone()));
    picking.task = Some(task);
    info!(screen = ?current.as_ref().map(|c| &c.screen), "folder picker open");
    setup.0.picking = true;
    setup.0.error = None;
}

/// Awaits the dialog, then stores what it picked (blocking, on the IO pool).
async fn store_pick(pick: impl Future<Output = Option<PathBuf>>, root: PathBuf) -> Picked {
    let Some(folder) = pick.await else {
        return Picked::Cancelled;
    };
    let folders = vec![folder];
    match crate::cli::store(&root, &folders) {
        Ok(scan) => Picked::Stored { folders, scan },
        Err(e) => Picked::Failed(format!("{e:#}")),
    }
}

fn finish_pick(mut picking: ResMut<Picking>, mut setup: ResMut<Setup>, link: Res<ServerLink>) {
    let Some(task) = picking.task.as_mut() else {
        return;
    };
    let Some(picked) = check_ready(task) else {
        return;
    };
    picking.task = None;
    match picked {
        Picked::Cancelled => {
            info!("folder picker cancelled");
            setup.0.picking = false;
        }
        Picked::Stored { folders, scan } => {
            info!(
                found = scan.found.len(),
                giants = scan.offered().is_some(),
                "stored the dumps folder"
            );
            // The phone's game list follows the pick; no restart.
            *link.games.write().unwrap_or_else(|e| e.into_inner()) = scan.games();
            setup.0 = SetupView::after_pick(&folders, &scan);
        }
        Picked::Failed(message) => {
            warn!("store the picked folder: {message}");
            setup.0.picking = false;
            setup.0.error = Some(message);
        }
    }
}

/// The found list gives way to the join code after the beat (its Continue
/// button does it at once, `tv::on_choice`), and is dropped once a game
/// starts over it (else a quit brings it back).
fn leave_found(current: Option<Res<Current>>, mut setup: ResMut<Setup>) {
    let Some(current) = current else { return };
    let leave = match current.screen {
        TvScreen::Found => current.since.elapsed() >= FOUND_BEAT,
        // Only Portal: `--setup` passes Starting on its way to Found.
        TvScreen::Portal { .. } => true,
        _ => false,
    };
    if setup.0.show_found && leave {
        setup.0.show_found = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::screen_for;
    use crate::test_support::cli::dump;
    use guac_server::dumps::GIANTS_TITLE_ID;
    use guac_server::setup;
    use guac_server::state::{GameList, LauncherStatus};

    const LATE: Duration = Duration::from_secs(5);

    #[test]
    fn setup_opens_the_picker_by_itself_once() {
        assert!(!should_open(
            &TvScreen::Setup,
            Duration::ZERO,
            false,
            false,
            false
        ));
        assert!(should_open(&TvScreen::Setup, LATE, false, false, false));
        // Cancelled: it waits for a press.
        assert!(!should_open(&TvScreen::Setup, LATE, false, true, false));
        assert!(should_open(&TvScreen::Setup, LATE, true, true, false));
    }

    #[test]
    fn no_giants_opens_only_on_a_press() {
        assert!(!should_open(&TvScreen::NoGiants, LATE, false, false, false));
        assert!(should_open(&TvScreen::NoGiants, LATE, true, true, false));
    }

    #[test]
    fn never_while_one_is_open_or_on_other_screens() {
        assert!(!should_open(&TvScreen::Setup, LATE, true, false, true));
        for screen in [
            TvScreen::Starting,
            TvScreen::Found,
            TvScreen::Join,
            TvScreen::AlreadyRunning,
            TvScreen::Farewell,
            TvScreen::ServerError {
                message: "x".into(),
            },
        ] {
            assert!(
                !should_open(&screen, LATE, true, false, false),
                "{screen:?}"
            );
        }
    }

    fn found_app(age: Duration) -> App {
        on_screen(TvScreen::Found, age)
    }

    fn on_screen(screen: TvScreen, age: Duration) -> App {
        let mut app = App::new();
        app.insert_resource(Current {
            screen,
            since: std::time::Instant::now().checked_sub(age).unwrap(),
            clients: 0,
        })
        .insert_resource(Setup(SetupView {
            giants_found: true,
            show_found: true,
            ..Default::default()
        }))
        .add_systems(Update, leave_found);
        app.update();
        app
    }

    /// Runs `finish_pick` until a pick of `folder` (None: cancelled) lands.
    fn land_pick(root: &std::path::Path, folder: Option<PathBuf>) -> (App, GameList) {
        use bevy::tasks::TaskPool;
        IoTaskPool::get_or_init(TaskPool::new);
        let pick = std::future::ready(folder);
        let task = IoTaskPool::get().spawn(store_pick(pick, root.to_path_buf()));
        let games = GameList::default();
        let mut app = App::new();
        app.insert_resource(ServerLink {
            status: Default::default(),
            clients: Default::default(),
            phone_url: String::new(),
            join_qr_png: Default::default(),
            games: games.clone(),
        })
        .insert_resource(Setup(SetupView {
            needs_setup: true,
            picking: true,
            ..Default::default()
        }))
        .insert_resource(Picking {
            task: Some(task),
            auto_opened: true,
            wanted: false,
        })
        .add_systems(Update, finish_pick);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while app.world().resource::<Picking>().task.is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "the pick never landed"
            );
            app.update();
            std::thread::sleep(Duration::from_millis(5));
        }
        (app, games)
    }

    fn ready() -> LauncherStatus {
        LauncherStatus {
            server_ready: true,
            ..Default::default()
        }
    }

    #[test]
    fn a_pick_with_giants_is_stored_lists_the_dumps_then_offers_giants() {
        let root = tempfile::tempdir().unwrap();
        let discs = tempfile::tempdir().unwrap();
        dump(&discs.path().join("Giants"), GIANTS_TITLE_ID);
        dump(&discs.path().join("Swap Force"), "BLUS31076");

        let (app, games) = land_pick(root.path(), Some(discs.path().to_path_buf()));
        let view = &app.world().resource::<Setup>().0;
        assert!(!view.picking);
        assert_eq!(view.found_titles.len(), 2, "{view:?}");
        assert_eq!(screen_for(&ready(), view), TvScreen::Found);
        let served = games.read().unwrap().clone();
        assert_eq!(served.len(), 1);
        assert_eq!(served[0].serial.as_str(), GIANTS_TITLE_ID);
        // A restart doesn't ask again.
        let loaded = setup::load_with(root.path(), &Default::default()).unwrap();
        assert!(!loaded.needs_setup());
    }

    #[test]
    fn a_pick_without_giants_says_so_and_offers_nothing() {
        let root = tempfile::tempdir().unwrap();
        let discs = tempfile::tempdir().unwrap();
        dump(&discs.path().join("Swap Force"), "BLUS31076");
        let (app, games) = land_pick(root.path(), Some(discs.path().to_path_buf()));
        let view = &app.world().resource::<Setup>().0;
        assert_eq!(screen_for(&ready(), view), TvScreen::NoGiants);
        assert!(games.read().unwrap().is_empty());
    }

    #[test]
    fn a_pick_with_a_broken_dump_says_why_on_no_giants() {
        let root = tempfile::tempdir().unwrap();
        let discs = tempfile::tempdir().unwrap();
        let sfo = discs.path().join("Giants/PS3_GAME/PARAM.SFO");
        std::fs::create_dir_all(sfo.parent().unwrap()).unwrap();
        std::fs::write(&sfo, b"not an sfo").unwrap();
        let (app, _) = land_pick(root.path(), Some(discs.path().to_path_buf()));
        let view = &app.world().resource::<Setup>().0;
        assert_eq!(screen_for(&ready(), view), TvScreen::NoGiants);
        assert_eq!(view.problems, [format!("Couldn't read {}", sfo.display())]);
    }

    #[cfg(unix)]
    #[test]
    fn a_pick_that_cant_be_listed_says_so() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let discs = tempfile::tempdir().unwrap();
        dump(&discs.path().join("Giants"), GIANTS_TITLE_ID);
        let set = |mode| {
            std::fs::set_permissions(discs.path(), std::fs::Permissions::from_mode(mode)).unwrap()
        };
        set(0o311);
        if std::fs::read_dir(discs.path()).is_ok() {
            set(0o755);
            return; // root lists anything
        }
        let (app, _) = land_pick(root.path(), Some(discs.path().to_path_buf()));
        set(0o755);
        let view = &app.world().resource::<Setup>().0;
        assert_eq!(screen_for(&ready(), view), TvScreen::NoGiants);
        assert_eq!(
            view.problems,
            [format!("Couldn't list {}", discs.path().display())]
        );
    }

    #[test]
    fn a_cancelled_pick_stores_nothing_and_stays_on_setup() {
        let root = tempfile::tempdir().unwrap();
        let (app, games) = land_pick(root.path(), None);
        let view = &app.world().resource::<Setup>().0;
        assert!(!view.picking && view.error.is_none());
        assert_eq!(screen_for(&ready(), view), TvScreen::Setup);
        assert!(games.read().unwrap().is_empty());
        assert!(!root.path().join("db.sqlite").exists());
    }

    #[test]
    fn a_pick_that_cant_be_stored_shows_why() {
        let root = tempfile::tempdir().unwrap();
        let gone = root.path().join("unplugged");
        let (app, _) = land_pick(root.path(), Some(gone));
        let view = &app.world().resource::<Setup>().0;
        assert!(
            view.error.as_deref().unwrap().contains("isn't a folder"),
            "{view:?}"
        );
        assert_eq!(screen_for(&ready(), view), TvScreen::Setup);
    }

    #[test]
    fn found_gives_way_to_join_after_the_beat_or_a_click() {
        use crate::test_support::tv::{choice, click_on, tv_on};
        use crate::tv::CONTINUE;
        let shown = |app: &App| app.world().resource::<Setup>().0.show_found;
        assert!(shown(&found_app(Duration::ZERO)));
        assert!(!shown(&found_app(FOUND_BEAT + LATE)));
        // AP.6: the click is on its Continue button.
        let found = SetupView {
            giants_found: true,
            show_found: true,
            ..Default::default()
        };
        let mut app = tv_on(TvScreen::Found, Duration::ZERO, found);
        assert!(shown(&app));
        let button = choice(&mut app, CONTINUE);
        click_on(&mut app, button);
        assert!(!shown(&app));
    }

    #[test]
    fn a_game_started_over_the_list_drops_it() {
        let app = on_screen(
            TvScreen::Portal {
                game: "Skylanders: Giants".into(),
            },
            Duration::ZERO,
        );
        let view = &app.world().resource::<Setup>().0;
        assert!(!view.show_found);
        // So the quit goes back to the join code, not the stale list.
        assert_eq!(screen_for(&ready(), view), TvScreen::Join);
        // `--setup` passes Starting before Found: the list survives it.
        let starting = on_screen(TvScreen::Starting, Duration::ZERO);
        assert!(starting.world().resource::<Setup>().0.show_found);
    }
}
