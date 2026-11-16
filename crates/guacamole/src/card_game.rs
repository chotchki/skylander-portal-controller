//! The card game's streams (openspec AS.364): the three archives the
//! original streams in for a card pirate's game, linked beside a level that
//! places a pirate and joined to its run when a script asks for one.
//!
//! - **What the original does.** A level with a card pirate
//!   (`CardPirate_Template.ai` placed running) loads the game's archives as
//!   it needs them, each through `Game - Load Streaming Level`, which writes
//!   the archive's path to `system.load level`: the pirate asks for the
//!   sprite stream ([`SPRITES`]) as his win's loot begins, when `Game
//!   Systems`' `reward card sprite macro` is still unset; `Card Game - Start
//!   Card Game` (`CardGame_External.ai`) asks for the hand select
//!   ([`HAND_SELECT`]); the hand select asks for the board ([`BOARD`])
//!   through `Portal - Load Replacement Character Stream`, a load string
//!   built on the board's archive with the hand's stones after it. A loaded
//!   archive's running placements start: the sprite stream's
//!   `2dSpritesStream_Startup` sets the reward macro (`CardGame UI - Get
//!   Reward Sprite`), its `CardArt_Macros` holds the stones' art macros; the
//!   hand select's `StartupMenu` opens its menu and `Menu_MoreInfo` its
//!   help page; the board's `Controller` (`CardGame_Setup.ai`) lays the
//!   board and `Atmospherics` sets its lights.
//! - **What the TV does the same.** The level's load links the three
//!   streams' script entries after the level's own and its ship parts
//!   ([`crate::level_scripts`]) when the level places a pirate
//!   ([`places_a_pirate`]), their running placements listed without
//!   listing anything of the level's own ([`with_streams`]). A request whose text names one ([`asked`]) is taken at the step's
//!   end as every stream is ([`crate::scripts::ScriptWorld::end_step`]), and
//!   that stream's running placements join the run from the next step
//!   ([`join_asked`]), as the ship parts' join theirs at the run's start; one
//!   asked again after its placements went (the hand select's starter takes
//!   its stream down as its menu closes) joins again.
//! - **What it leaves.** The game itself: the TV still answers `Card Game -
//!   Start Card Game` with nothing shown and tells the pirate it was quit
//!   ([`crate::prompt::quit_card_game`]), so in play only a test reaches the
//!   pirate's win; nothing draws the streams' sprites or the board (AS.365 to
//!   AS.368).

use guac_script::{PlacementId, Program, Vm};
use tracing::info;

use crate::scripts::{Files, ScriptWorld};

/// The pirate's script: a level placing it running has a card game.
pub const PIRATE: &str = "CardPirate_Template.ai";

/// The three, in the order they link (the extractor's part,
/// [`guac_extract::screens::CARD_GAME`]): the sprites first, then the
/// board, then the hand select.
pub const STREAMS: [&str; 3] = guac_extract::screens::CARD_GAME;
/// The reward card's sprites: the macro the pirate's loot waits on.
pub const SPRITES: &str = STREAMS[0];
/// The board: the game's manager, set-up, lights and its reports.
pub const BOARD: &str = STREAMS[1];
/// The hand select: its menu and the game's start.
pub const HAND_SELECT: &str = STREAMS[2];

/// A running placement's activity state.
const RUNNING: u32 = 4;

/// Whether the level linked as `unit` can ask for the streams at all: a
/// pirate placed asleep or only as a template never starts a game.
pub fn places_a_pirate(files: &Files, unit: u16) -> bool {
    files.placed.iter().any(|p| {
        p.obj.unit == unit
            && p.activity_state == RUNNING
            && p.script
                .as_deref()
                .is_some_and(|s| ends_with_file(s, PIRATE))
    })
}

/// `config`'s listing with the streams' running placements in `units`
/// listed so that nothing outside them changes: each by its script where
/// no placement outside the streams has that script, else by its name where
/// none has that name, else left off (the hand select's `Menu_MoreInfo`
/// beside a hub stage's own, which holds only macros). The helpers asleep
/// in them stay off, as their twins in the level's file are.
pub fn with_streams(
    mut config: guac_script::Config,
    files: &Files,
    units: &[u16],
) -> guac_script::Config {
    let guac_script::Whitelist::Only {
        scripts,
        placements,
        ..
    } = &mut config.whitelist
    else {
        return config;
    };
    let outside = |f: &dyn Fn(&crate::scripts::Placed) -> bool| {
        files
            .placed
            .iter()
            .any(|p| !units.contains(&p.obj.unit) && f(p))
    };
    for p in files
        .placed
        .iter()
        .filter(|p| units.contains(&p.obj.unit) && p.activity_state == RUNNING)
    {
        let Some(script) = &p.script else {
            continue;
        };
        if !outside(&|o| o.script.as_ref() == Some(script)) {
            if !scripts.contains(script) {
                scripts.push(script.clone());
            }
        } else if !outside(&|o| o.name == p.name) {
            if !placements.contains(&p.name) {
                placements.push(p.name.clone());
            }
        } else {
            info!(placement = %p.name, %script, "a card game stream's placement shares its script and name with the level's: left off");
        }
    }
    config
}

/// [`places_a_pirate`] over a level's own file, read alone: the load
/// decides by it which streams to link before it links anything.
pub fn file_places_a_pirate(igz: &guac_formats::igz::Igz<'_>) -> Result<bool, String> {
    let files = Files::read(&[(0, igz)])?;
    Ok(places_a_pirate(&files, 0))
}

/// Whether source path `path` names the file `file` (either slash, any
/// case, as the disc's paths mix them).
fn ends_with_file(path: &str, file: &str) -> bool {
    path.rsplit(['/', '\\'])
        .next()
        .is_some_and(|f| f.eq_ignore_ascii_case(file))
}

/// The stream a request's `text` asks for, by archive: the request names its
/// file after the stream type's folder (`misc/` and the stem), the board's
/// with the hand's stones after its stem, so its last path segment begins
/// with the stem and nothing of a name follows it. None for any other text
/// (a stone's own archive, `cardgame_card_005`, among them).
pub fn asked(text: &str) -> Option<&'static str> {
    let last = text.rsplit(['/', '\\']).next()?.to_ascii_lowercase();
    STREAMS.into_iter().find(|archive| {
        let stem = guac_extract::scripts::stem(archive);
        last.strip_prefix(stem)
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
    })
}

/// The run's card game streams: the units they link as and what's been asked.
#[derive(Debug, Clone, Default)]
pub struct CardStreams {
    /// Each linked stream's archive and unit, in [`STREAMS`]' order; none on a
    /// level without a pirate.
    pub units: Vec<(&'static str, u16)>,
    /// Streams asked for and not yet joined, by archive, in order.
    pub(crate) pending: Vec<&'static str>,
    /// Every join taken, by archive, with how many placements it added.
    pub joined: Vec<(&'static str, usize)>,
}

impl CardStreams {
    /// The streams as linked: `units` in the order of `archives`.
    pub fn linked(archives: &[&'static str], units: &[u16]) -> Self {
        Self {
            units: archives
                .iter()
                .copied()
                .zip(units.iter().copied())
                .collect(),
            ..Self::default()
        }
    }

    /// `linked`'s streams as a new run starts them: nothing asked yet.
    pub fn fresh(linked: &Self) -> Self {
        Self {
            units: linked.units.clone(),
            ..Self::default()
        }
    }

    /// The units, for the listing.
    pub fn unit_list(&self) -> Vec<u16> {
        self.units.iter().map(|(_, u)| *u).collect()
    }

    /// A request's text taken (the step's end): one naming a linked stream is
    /// kept to join. Whether it named one.
    pub fn take_request(&mut self, text: Option<&str>) -> bool {
        let Some(archive) = text.and_then(asked) else {
            return false;
        };
        if !self.units.iter().any(|(a, _)| *a == archive) {
            return false;
        }
        info!(
            stream = archive,
            "a script asked for a card game stream: it joins the run"
        );
        self.pending.push(archive);
        true
    }
}

/// Each asked stream's running placements added to the run, in the order a
/// file's load adds them (awake first, then by its root list), each one not
/// already running under its handle; what was added.
pub fn join_asked(vm: &mut Vm, world: &mut ScriptWorld) -> Vec<PlacementId> {
    let pending = std::mem::take(&mut world.card_streams.pending);
    if pending.is_empty() {
        return Vec::new();
    }
    let program: std::sync::Arc<Program> = vm.program_arc();
    let mut added = Vec::new();
    for archive in pending {
        let Some(&(_, unit)) = world.card_streams.units.iter().find(|(a, _)| *a == archive) else {
            continue;
        };
        let placed: Vec<_> = crate::ship_parts::stream_placements(world, unit)
            .into_iter()
            .map(|(p, h)| (p.obj, p.name.clone(), h))
            .collect();
        let before = added.len();
        for (obj, name, h) in placed {
            if vm.placement_of(h).is_some_and(|id| !vm.removed(id)) {
                continue;
            }
            if let Some(sid) = world.script_of(obj, &program) {
                added.push(vm.add_at(h, &name, sid, obj));
            }
        }
        let n = added.len() - before;
        info!(
            stream = archive,
            added = n,
            "a card game stream joined the run"
        );
        world.card_streams.joined.push((archive, n));
    }
    added
}

#[cfg(test)]
mod tests;
