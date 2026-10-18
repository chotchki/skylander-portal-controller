//! Which screen the TV shows: a pure function of the server's status and
//! what setup found, so every case is testable without a window.

use guac_server::dumps::DumpScan;
use guac_server::state::{LauncherScreen, LauncherStatus};

/// What setup knows at startup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetupView {
    /// No dumps folder chosen yet.
    pub needs_setup: bool,
    /// A BLUS30968 dump was found.
    pub giants_found: bool,
    /// Every dump found, by title ("Skylanders: Giants (BLUS30968)").
    pub found_titles: Vec<String>,
    /// The chosen folders, for the no-Giants screen.
    pub folders: Vec<String>,
    /// The folder picker is open.
    pub picking: bool,
    /// The last pick couldn't be stored.
    pub error: Option<String>,
    /// A pick just found Giants: list what was found before the join code.
    pub show_found: bool,
    /// Folders that couldn't be listed and dumps that couldn't be read.
    pub problems: Vec<String>,
}

impl SetupView {
    pub fn new(needs_setup: bool, folders: &[std::path::PathBuf], scan: &DumpScan) -> Self {
        Self {
            needs_setup,
            giants_found: scan.offered().is_some(),
            found_titles: scan
                .found
                .iter()
                .map(|d| match d.display_name() {
                    Some(name) => format!("{name} ({})", d.title_id),
                    None => d.title_id.clone(),
                })
                .collect(),
            folders: folders.iter().map(|f| f.display().to_string()).collect(),
            problems: scan
                .unlisted
                .iter()
                .map(|f| format!("Couldn't list {}", f.display()))
                .chain(
                    scan.unreadable
                        .iter()
                        .map(|s| format!("Couldn't read {}", s.display())),
                )
                .collect(),
            ..Default::default()
        }
    }

    /// The view at startup: a `--setup` pick that stored lists what it found,
    /// as the in-window sheet does.
    pub fn at_start(
        picked: bool,
        needs_setup: bool,
        folders: &[std::path::PathBuf],
        scan: &DumpScan,
    ) -> Self {
        if picked && !needs_setup {
            Self::after_pick(folders, scan)
        } else {
            Self::new(needs_setup, folders, scan)
        }
    }

    /// Every found title on one wrapping line, so a big library stays on screen.
    pub fn titles_line(&self) -> String {
        self.found_titles.join("   ·   ")
    }

    /// After a pick was stored: Giants shows what was found before the join code.
    pub fn after_pick(folders: &[std::path::PathBuf], scan: &DumpScan) -> Self {
        let view = Self::new(false, folders, scan);
        Self {
            show_found: view.giants_found,
            ..view
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TvScreen {
    /// The server hasn't bound yet.
    Starting,
    /// No dumps folder: ask for one.
    Setup,
    /// Folders chosen, no Giants dump among them.
    NoGiants,
    /// A pick found Giants: what was found, then the join code.
    Found,
    /// Serving, no game picked: the join code.
    Join,
    /// A game is picked: a card per figure on the portal, the join code small.
    Portal { game: String },
    /// The port is taken: the controller or another guacamole is serving.
    AlreadyRunning,
    /// Startup failed, or (unreachable under Native) the emulator crashed.
    ServerError { message: String },
    /// SHUT DOWN from a phone: the farewell beat, then exit.
    Farewell,
}

impl TvScreen {
    /// Screens the keyboard's Escape may close: nothing is being served.
    pub fn escape_quits(&self) -> bool {
        matches!(self, Self::AlreadyRunning | Self::ServerError { .. })
    }
}

/// Farewell and errors beat setup, so a second copy never asks for a folder.
pub fn screen_for(status: &LauncherStatus, setup: &SetupView) -> TvScreen {
    match &status.screen {
        LauncherScreen::Farewell => TvScreen::Farewell,
        LauncherScreen::ServerError { .. } if status.port_in_use => TvScreen::AlreadyRunning,
        LauncherScreen::ServerError { message } | LauncherScreen::Crashed { message } => {
            TvScreen::ServerError {
                message: message.clone(),
            }
        }
        LauncherScreen::Main if !status.server_ready => TvScreen::Starting,
        LauncherScreen::Main if setup.needs_setup => TvScreen::Setup,
        LauncherScreen::Main if !setup.giants_found => TvScreen::NoGiants,
        // A pick's Found list never hides a game a phone already started.
        LauncherScreen::Main if status.current_game.is_some() => TvScreen::Portal {
            game: status.current_game.clone().unwrap_or_default(),
        },
        LauncherScreen::Main if setup.show_found => TvScreen::Found,
        LauncherScreen::Main => TvScreen::Join,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> LauncherStatus {
        LauncherStatus {
            server_ready: true,
            ..Default::default()
        }
    }

    fn set_up() -> SetupView {
        SetupView {
            giants_found: true,
            ..Default::default()
        }
    }

    fn pending() -> SetupView {
        SetupView {
            needs_setup: true,
            ..Default::default()
        }
    }

    fn no_giants() -> SetupView {
        SetupView {
            found_titles: vec!["Skylanders: Spyro's Adventure (BLUS30731)".into()],
            ..Default::default()
        }
    }

    fn error(port_in_use: bool) -> LauncherStatus {
        LauncherStatus {
            screen: LauncherScreen::ServerError {
                message: "bind 0.0.0.0:8765: Address already in use".into(),
            },
            port_in_use,
            ..Default::default()
        }
    }

    #[test]
    fn main_once_serving_shows_the_join_code() {
        assert_eq!(screen_for(&ready(), &set_up()), TvScreen::Join);
    }

    fn playing(game: &str) -> LauncherStatus {
        LauncherStatus {
            current_game: Some(game.into()),
            ..ready()
        }
    }

    #[test]
    fn a_picked_game_shows_the_portal() {
        let portal = TvScreen::Portal {
            game: "Skylanders: Giants".into(),
        };
        assert_eq!(
            screen_for(&playing("Skylanders: Giants"), &set_up()),
            portal
        );
        // Over a pick's Found list too.
        let found = SetupView {
            show_found: true,
            ..set_up()
        };
        assert_eq!(screen_for(&playing("Skylanders: Giants"), &found), portal);
        assert!(!portal.escape_quits());
        // A quit clears the game: back to the join code.
        assert_eq!(screen_for(&ready(), &set_up()), TvScreen::Join);
    }

    #[test]
    fn the_farewell_and_errors_beat_the_portal() {
        let farewell = LauncherStatus {
            screen: LauncherScreen::Farewell,
            ..playing("Skylanders: Giants")
        };
        assert_eq!(screen_for(&farewell, &set_up()), TvScreen::Farewell);
        let crashed = LauncherStatus {
            screen: LauncherScreen::Crashed {
                message: "gone".into(),
            },
            ..playing("Skylanders: Giants")
        };
        assert!(matches!(
            screen_for(&crashed, &set_up()),
            TvScreen::ServerError { .. }
        ));
    }

    #[test]
    fn main_before_the_bind_is_starting_whatever_setup_says() {
        let status = LauncherStatus::default();
        for setup in [set_up(), pending(), no_giants()] {
            assert_eq!(screen_for(&status, &setup), TvScreen::Starting);
        }
    }

    #[test]
    fn setup_pending_asks_for_the_folder() {
        assert_eq!(screen_for(&ready(), &pending()), TvScreen::Setup);
    }

    #[test]
    fn no_giants_dump_says_so() {
        assert_eq!(screen_for(&ready(), &no_giants()), TvScreen::NoGiants);
    }

    #[test]
    fn a_taken_port_is_already_running_even_with_setup_pending() {
        for setup in [set_up(), pending(), no_giants()] {
            assert_eq!(screen_for(&error(true), &setup), TvScreen::AlreadyRunning);
        }
        assert!(TvScreen::AlreadyRunning.escape_quits());
    }

    #[test]
    fn any_other_startup_failure_shows_its_message() {
        let screen = screen_for(&error(false), &pending());
        assert_eq!(
            screen,
            TvScreen::ServerError {
                message: "bind 0.0.0.0:8765: Address already in use".into()
            }
        );
        assert!(screen.escape_quits());
    }

    #[test]
    fn crashed_shows_the_error_screen() {
        let status = LauncherStatus {
            screen: LauncherScreen::Crashed {
                message: "gone".into(),
            },
            server_ready: true,
            ..Default::default()
        };
        assert_eq!(
            screen_for(&status, &set_up()),
            TvScreen::ServerError {
                message: "gone".into()
            }
        );
    }

    #[test]
    fn farewell_wins_over_everything() {
        for server_ready in [false, true] {
            let status = LauncherStatus {
                screen: LauncherScreen::Farewell,
                server_ready,
                ..Default::default()
            };
            for setup in [set_up(), pending(), no_giants()] {
                assert_eq!(screen_for(&status, &setup), TvScreen::Farewell);
            }
        }
        assert!(!TvScreen::Farewell.escape_quits());
        assert!(!TvScreen::Join.escape_quits());
    }

    #[test]
    fn a_pick_that_found_giants_shows_the_list_then_join() {
        let found = SetupView {
            show_found: true,
            ..set_up()
        };
        assert_eq!(screen_for(&ready(), &found), TvScreen::Found);
        assert!(!TvScreen::Found.escape_quits());
        // Errors and the farewell still win.
        assert_eq!(screen_for(&error(true), &found), TvScreen::AlreadyRunning);
        let unfound = SetupView {
            show_found: true,
            ..no_giants()
        };
        assert_eq!(screen_for(&ready(), &unfound), TvScreen::NoGiants);
    }

    #[test]
    fn after_pick_shows_found_only_with_giants() {
        use guac_server::dumps::FoundDump;
        let dump = |id: &str| FoundDump {
            title_id: id.into(),
            dir: "/d/x".into(),
        };
        let giants = DumpScan {
            found: vec![dump("BLUS31076"), dump("BLUS30968")],
            ..Default::default()
        };
        let view = SetupView::after_pick(&["/d".into()], &giants);
        assert!(!view.needs_setup && view.giants_found && view.show_found);
        assert_eq!(view.found_titles.len(), 2);
        let swap_force = DumpScan {
            found: vec![dump("BLUS31076")],
            ..Default::default()
        };
        let view = SetupView::after_pick(&["/d".into()], &swap_force);
        assert!(!view.needs_setup && !view.giants_found && !view.show_found);
        assert_eq!(screen_for(&ready(), &view), TvScreen::NoGiants);
    }

    fn giants_and_swap_force() -> DumpScan {
        use guac_server::dumps::FoundDump;
        let dump = |id: &str| FoundDump {
            title_id: id.into(),
            dir: "/d/x".into(),
        };
        DumpScan {
            found: vec![dump("BLUS31076"), dump("BLUS30968")],
            ..Default::default()
        }
    }

    #[test]
    fn a_setup_pick_lists_what_it_found_then_a_plain_start_doesnt() {
        let scan = giants_and_swap_force();
        let view = SetupView::at_start(true, false, &["/d".into()], &scan);
        assert_eq!(screen_for(&ready(), &view), TvScreen::Found);
        let view = SetupView::at_start(false, false, &["/d".into()], &scan);
        assert_eq!(screen_for(&ready(), &view), TvScreen::Join);
        // A cancelled pick with nothing stored still asks for a folder.
        let view = SetupView::at_start(false, true, &[], &DumpScan::default());
        assert_eq!(screen_for(&ready(), &view), TvScreen::Setup);
    }

    #[test]
    fn titles_share_one_line() {
        let view = SetupView::new(false, &["/d".into()], &giants_and_swap_force());
        assert_eq!(
            view.titles_line(),
            format!("{}   ·   {}", view.found_titles[0], view.found_titles[1])
        );
        assert_eq!(SetupView::default().titles_line(), "");
    }

    #[test]
    fn scan_problems_reach_the_view() {
        let scan = DumpScan {
            unlisted: vec!["/d".into()],
            unreadable: vec!["/e/PS3_GAME/PARAM.SFO".into()],
            ..Default::default()
        };
        let view = SetupView::new(false, &["/d".into(), "/e".into()], &scan);
        assert!(view.found_titles.is_empty());
        assert_eq!(
            view.problems,
            ["Couldn't list /d", "Couldn't read /e/PS3_GAME/PARAM.SFO"]
        );
        assert_eq!(screen_for(&ready(), &view), TvScreen::NoGiants);
        assert_eq!(
            SetupView::after_pick(&["/d".into()], &scan).problems.len(),
            2
        );
    }

    #[test]
    fn setup_view_lists_titles_and_offers_giants() {
        use guac_server::dumps::FoundDump;
        let scan = DumpScan {
            found: vec![
                FoundDump {
                    title_id: "BLUS30968".into(),
                    dir: "/d/giants".into(),
                },
                FoundDump {
                    title_id: "BLES99999".into(),
                    dir: "/d/other".into(),
                },
            ],
            ..Default::default()
        };
        let view = SetupView::new(false, &["/d".into()], &scan);
        assert!(view.giants_found);
        assert_eq!(view.found_titles.len(), 2);
        assert!(view.found_titles[0].ends_with("(BLUS30968)"));
        assert_eq!(view.found_titles[1], "BLES99999");
        assert_eq!(view.folders, vec!["/d".to_string()]);
    }
}
