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
    /// Serving: the join code (AA.8 adds the portal cards).
    Join,
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
