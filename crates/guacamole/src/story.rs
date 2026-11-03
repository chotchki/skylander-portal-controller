//! The TV's half of the story saves (openspec AS.38, AU.3, AU.4): the host
//! profile's save read from the store at the launch, the game resumed where
//! the original's load of it goes, the save system's requests answered after
//! each step and its writes handed to the store ([`guacamole::story_save`]
//! has the reading and the host's rules).
//!
//! - **The launch**: once the server names a host ([`GameHost`], the profile
//!   unlocked on the phone that started the game), a session for it begins
//!   and its rows for Giants are fetched. Where they resume the game
//!   (chapter 1 with no save, level 214 once chapter 1 is done, the hub past
//!   214) is asked by name as AS.13 asks a level, unless a level or a dev
//!   entry is asked already: the pick loads chapter 1 behind the portal, and
//!   the level the save names replaces it. A launch with no profile unlocked
//!   saves for the session alone.
//! - **Each step**: after the scripts' step, the run's start (its save
//!   loaded, the level's save asked) and the requests the scripts made
//!   ([`story_save::serve`]), but while another level is on its way; each
//!   frame the session's write handed to the store's keeper and its answer
//!   read back.
//!
//! Stand-in ledger: `story-save`.

use bevy::prelude::*;
use guac_server::saves::{Done, GameHost, StorySink};
use guacamole::scripts::{ScriptClone, ScriptRuntime, ScriptWorld, WorldHost};
use guacamole::story_save::{self, Report, Resume, SaveState, Session};
use serde::Serialize;
use tracing::{info, warn};

use crate::dev_entry::{DevEntry, EntryAsks};
use crate::hero_scripts::ScriptSource;
use crate::level_choice::{LevelAsks, LevelChoice};

/// The server's ends of the story saves: the host it names and the keeper.
#[derive(Resource, Clone)]
pub(crate) struct StoryLink {
    pub(crate) host: GameHost,
    pub(crate) sink: StorySink,
}

/// The session's save and what's in flight to the store.
#[derive(Resource, Default)]
pub(crate) struct Story {
    session: Session,
    /// The host the session began for, as the server named it.
    began_for: Option<Option<String>>,
    fetching: Option<u64>,
    /// The ticket of the write the keeper has, while one is in flight.
    writing: Option<u64>,
    /// Whether the session's resume was asked (or settled on chapter 1).
    resumed: bool,
}

/// `/api/_test/tv`'s `story`: the session's report and the state the host
/// answers `system.save state` with now.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct StoryShot {
    #[serde(flatten)]
    pub(crate) report: Report,
    /// `system.save state` as the run's host reads it; none with no run.
    pub(crate) state: Option<i32>,
    /// A save the store hasn't written yet.
    pub(crate) writing: bool,
}

impl Story {
    pub(crate) fn shot(&self, world: Option<&ScriptWorld>) -> StoryShot {
        StoryShot {
            report: self.session.report.clone(),
            state: world.map(|w| w.save.get()),
            writing: self.session.writing(),
        }
    }
}

pub(crate) struct StoryPlugin;

impl Plugin for StoryPlugin {
    fn build(&self, app: &mut App) {
        guacamole::frame::ensure(app);
        app.init_resource::<Story>()
            .add_systems(Update, (follow_host, pump).chain())
            .add_systems(
                FixedUpdate,
                serve_saves
                    .in_set(guacamole::frame::Frame::Scripts)
                    .after(crate::hero_scripts::step_scripts)
                    .run_if(resource_exists::<ScriptRuntime>)
                    .run_if(resource_exists::<ScriptWorld>)
                    .run_if(resource_exists::<ScriptSource>),
            );
    }
}

/// A new host from the server begins a session for it and fetches its
/// rows; none (a quit, or a launch with no profile unlocked) begins one
/// that saves for itself alone.
fn follow_host(link: Option<Res<StoryLink>>, mut story: ResMut<Story>) {
    let Some(link) = link else {
        return;
    };
    let host = link.host.get().map(|h| h.profile_id);
    if story.began_for.as_ref() == Some(&host) {
        return;
    }
    info!(host = ?host, "a story save session begins");
    *story = Story {
        session: Session::new(host.clone()),
        began_for: Some(host.clone()),
        ..Story::default()
    };
    match host {
        Some(profile) => match link.sink.fetch(&profile, story_save::GAME) {
            Some(ticket) => story.fetching = Some(ticket),
            None => {
                warn!("no store keeper: the host's story save isn't read");
                story.resumed = true;
            }
        },
        None => story.resumed = true,
    }
}

/// The keeper's answers read back, the resume asked once the rows are in,
/// and the session's next write handed over.
fn pump(
    link: Option<Res<StoryLink>>,
    mut story: ResMut<Story>,
    world: Option<ResMut<ScriptWorld>>,
    asks: Option<Res<LevelAsks>>,
    choice: Option<Res<LevelChoice>>,
    entry: Option<Res<DevEntry>>,
    entry_asks: Option<Res<EntryAsks>>,
) {
    let Some(link) = link else {
        return;
    };
    let mut world = world;
    let mut spare = SaveState::default();
    for done in link.sink.take_done() {
        match done {
            Done::Fetched { ticket, result } if story.fetching == Some(ticket) => {
                story.fetching = None;
                let rows = result.unwrap_or_else(|e| {
                    warn!("the host's story saves didn't read: {e}; the profile starts fresh");
                    story.session.report.error = Some(e);
                    Vec::new()
                });
                let resume = story.session.fetched(rows);
                info!(?resume, "where the host's save resumes the game");
                let dev = entry.as_deref().is_some_and(|e| e.asked.is_some())
                    || entry_asks
                        .as_deref()
                        .is_some_and(|a| a.0.lock().unwrap_or_else(|e| e.into_inner()).is_some());
                let changing = choice.as_deref().is_some_and(LevelChoice::changing);
                match resume_ask(&resume, changing || dev) {
                    Some(name) => {
                        info!(%name, "the host's save resumes the game there: asked by name");
                        if let Some(asks) = asks.as_deref() {
                            asks.ask(name);
                        }
                    }
                    None if resume != Resume::ChapterOne => {
                        info!(
                            ?resume,
                            "a level or a dev entry is asked already: the save's level isn't"
                        );
                    }
                    None => {}
                }
                story.resumed = true;
            }
            Done::Written { ticket, result } if story.writing == Some(ticket) => {
                story.writing = None;
                let save = match world.as_deref_mut() {
                    Some(w) => &mut w.save,
                    None => &mut spare,
                };
                story.session.written(save, result);
            }
            // A ticket from a session since replaced.
            Done::Fetched { .. } | Done::Written { .. } => {}
        }
    }
    if story.writing.is_none()
        && let Some((host, layout, values)) = story.session.take_write()
    {
        match link.sink.write(&host, story_save::GAME, &layout, values) {
            Some(ticket) => story.writing = Some(ticket),
            None => {
                let save = match world.as_deref_mut() {
                    Some(w) => &mut w.save,
                    None => &mut spare,
                };
                story
                    .session
                    .written(save, Err("no store keeper runs".into()));
            }
        }
    }
}

/// The level to ask by name for where a save resumes the game: none for
/// chapter 1, which the pick loads, or while another level or a dev entry
/// is asked (`taken`), which decides the level then.
fn resume_ask(resume: &Resume, taken: bool) -> Option<&str> {
    match resume {
        Resume::Level(name) | Resume::Hub(name) if !taken => Some(name),
        _ => None,
    }
}

/// Whether the save system's step runs: once the session's resume is
/// settled, and while no other level is on its way ([`serve_saves`]).
fn serves(resumed: bool, changing: bool) -> bool {
    resumed && !changing
}

/// The step's save requests, after the scripts' step: the run's start and
/// each request the scripts made ([`story_save::serve`]), on no engine (the
/// save macros reach none).
pub(crate) fn serve_saves(
    mut commands: Commands,
    mut story: ResMut<Story>,
    mut runtime: ResMut<ScriptRuntime>,
    mut world: ResMut<ScriptWorld>,
    source: Res<ScriptSource>,
    choice: Option<Res<LevelChoice>>,
) {
    // Not before the session's resume is settled: a run started on a
    // profile's save the TV hasn't read yet would save over it fresh. Nor
    // while another level is on its way: the pick's chapter 1 behind the
    // portal, replaced by the level a save resumes at, would make a start
    // (and a save naming chapter 1 its slot's level) the original never
    // makes; a level end's own save is the next level's start.
    if !serves(
        story.resumed,
        choice.as_deref().is_some_and(LevelChoice::changing),
    ) {
        return;
    }
    let ScriptRuntime { vm, generator, .. } = &mut *runtime;
    let mut spawn = |template: guac_script::Value| -> guac_script::Handle {
        commands.spawn(ScriptClone(template)).id().to_bits()
    };
    let mut engine = guacamole::script_engine::NoEngine;
    let mut host = WorldHost {
        world: &mut world,
        engine: &mut engine,
        generator,
        spawn: &mut spawn,
    };
    if let Err(h) = story_save::serve(vm, &mut host, &mut story.session, source.level()) {
        warn!(%h, "the save system's step stopped");
        story.session.report.error = Some(h.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A save past chapter 1 asks its level by name; chapter 1 asks
    /// nothing (the pick loads it), and nothing is asked over a level or a
    /// dev entry asked already.
    #[test]
    fn a_save_past_chapter_1_asks_its_level_unless_one_is_asked() {
        let level = Resume::Level("level_214_tutotial_2".into());
        let hub = Resume::Hub("level_hub_2_stage_1".into());
        assert_eq!(resume_ask(&level, false), Some("level_214_tutotial_2"));
        assert_eq!(resume_ask(&hub, false), Some("level_hub_2_stage_1"));
        assert_eq!(resume_ask(&Resume::ChapterOne, false), None);
        assert_eq!(resume_ask(&level, true), None);
        assert_eq!(resume_ask(&hub, true), None);
    }

    /// The save system steps only once the resume is settled and while no
    /// other level is on its way.
    #[test]
    fn the_save_system_waits_for_the_resume_and_a_level_on_its_way() {
        assert!(serves(true, false));
        assert!(!serves(false, false), "before the host's save was read");
        assert!(!serves(true, true), "on a run about to be replaced");
        assert!(!serves(false, true));
    }

    /// A host from the server begins a session for it, once; a host gone
    /// begins one for nobody; with no keeper the resume is settled at
    /// once, so the save system runs, saving for the session.
    #[test]
    fn a_host_begins_a_session_once_and_its_rows_are_asked_for() {
        let mut app = App::new();
        let link = StoryLink {
            host: GameHost::default(),
            sink: StorySink::new(),
        };
        app.insert_resource(link.clone())
            .init_resource::<Story>()
            .add_systems(Update, follow_host);
        app.update();
        {
            let story = app.world().resource::<Story>();
            assert_eq!(story.began_for, Some(None));
            assert!(story.resumed, "no host: nothing to read");
            assert_eq!(story.session.host, None);
        }
        link.host.set(Some(guac_server::saves::Host {
            profile_id: "alice".into(),
            game: story_save::GAME.into(),
        }));
        app.update();
        {
            let story = app.world().resource::<Story>();
            assert_eq!(story.began_for, Some(Some("alice".into())));
            assert_eq!(story.session.host.as_deref(), Some("alice"));
            assert_eq!(story.fetching, None, "no keeper to fetch from");
            assert!(story.resumed, "with no keeper the session saves alone");
        }
        app.world_mut().resource_mut::<Story>().resumed = false;
        app.update();
        assert!(
            !app.world().resource::<Story>().resumed,
            "the same host began a second session"
        );
        link.host.set(None);
        app.update();
        let story = app.world().resource::<Story>();
        assert_eq!(story.began_for, Some(None));
        assert_eq!(story.session.host, None);
    }
}
