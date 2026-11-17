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

use guac_script::{Halt, PlacementId, Program, Value, Vm};
use tracing::info;

use crate::scripts::{Files, ScriptWorld, WorldHost, running};

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

/// The stones' archives (openspec AS.366,
/// [`guac_extract::screens::card_stones`]), linked after the three streams:
/// each stone's deck, which the board's load string names for every stone
/// either hand holds and whose header defines the stone for the game, and
/// each stone's asset, asked as it's played, which defines what the board
/// shows of it.
pub fn stones() -> &'static [&'static str] {
    static STONES: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    STONES.get_or_init(|| {
        guac_extract::screens::card_stones()
            .into_iter()
            .map(|d| &*Box::leak(d.into_boxed_str()))
            .collect()
    })
}

/// Whether `archive` loads as a misc stream (its folder): one at a time.
fn is_misc(archive: &str) -> bool {
    archive.starts_with("misc/")
}

/// Whether a request's `text` is a misc load naming nothing (`misc/`): the
/// slot emptied.
fn empties_misc(text: &str) -> bool {
    text.trim_end_matches(['/', '\\'])
        .eq_ignore_ascii_case("misc")
}

/// Whether `archive` is one of [`stones`].
fn is_stone(archive: &str) -> bool {
    stones().contains(&archive)
}

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
/// beside a hub stage's own, which holds only macros). A placement asleep in
/// them is listed by its script where nothing outside them has it (the
/// board's `CardGameManager`, the reports' and the hand select's macros,
/// which the streams' own scripts wake: openspec AS.366); the helpers asleep
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
    for p in files.placed.iter().filter(|p| units.contains(&p.obj.unit)) {
        let Some(script) = &p.script else {
            continue;
        };
        let own_script = !outside(&|o| o.script.as_ref() == Some(script));
        if p.activity_state != RUNNING {
            // Asleep: the board's manager and the reports' and the hand
            // select's macros (openspec AS.366), woken by the streams' own
            // scripts, listed by a script of their own alone.
            if own_script && !scripts.contains(script) {
                scripts.push(script.clone());
            }
            continue;
        }
        if own_script {
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

/// Every archive a request's `text` asks for: the stones its load string
/// names, each a `+`-joined part of its last path segment by its stem (the
/// board's `CardGame_Streaming+cardgame_deck_006+...` loads each hand's
/// decks with it; a stone's asset is asked alone, `misc/cardgame_card_000`),
/// then the stream it names ([`asked`]), so a stone's header has defined it
/// before the board's set-up reads it. Empty for any other text.
pub fn asked_all(text: &str) -> Vec<&'static str> {
    let last = text
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut out: Vec<&'static str> = Vec::new();
    for part in last.split('+') {
        if let Some(stone) = stones()
            .iter()
            .copied()
            .find(|d| guac_extract::scripts::stem(d) == part)
            && !out.contains(&stone)
        {
            out.push(stone);
        }
    }
    out.extend(asked(text));
    out
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
    /// The misc stream joined last: the one a misc load replaces.
    pub(crate) misc: Option<&'static str>,
    /// Whether a yes at a pirate plays the original's own game on the VM
    /// (openspec AS.366, [`CardStreams::play_games`]) in place of the TV's
    /// quit ([`crate::prompt::quit_card_game`]). False on every TV run.
    played: bool,
    /// Where the portal's replacement of the hero by the board stands
    /// (openspec AS.481, [`Restore`]), read each step of a played run.
    restore: Restore,
    /// A quit asked of a played game ([`CardStreams::ask_quit`]), answered
    /// by the next step's [`quit`].
    quit_asked: bool,
}

/// The portal's replacement of the hero by the board (openspec AS.481):
/// `Portal - Load Replacement Character Stream` takes his minion off and
/// keeps his loaded data as `portal control data`'s `loaded object for
/// minigame`; the board's teardown runs `Portal - Restore Minigame Stream
/// from Character`, which asks for his character's stream again and clears
/// that field. A field set and then cleared is the restore asked; a hero
/// taken off the portal never sets it, so his figure's return stays the
/// swap's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Restore {
    /// No replacement since the last restore answered.
    #[default]
    Idle,
    /// The board holds his slot.
    Replaced,
    /// The board's teardown asked for him back, not yet answered.
    Asked,
}

impl Restore {
    /// The next state, `replaced` whether the minigame's loaded object is
    /// set this step.
    pub fn next(self, replaced: bool) -> Self {
        match (self, replaced) {
            (_, true) => Restore::Replaced,
            (Restore::Replaced, false) => Restore::Asked,
            (s, false) => s,
        }
    }
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
            played: linked.played,
            ..Self::default()
        }
    }

    /// From here on a yes at a pirate plays the game (openspec AS.366):
    /// `Card Game - Start Card Game` runs as the original's script, the hand
    /// select and board join as asked and the host answers what they wait on
    /// ([`step`]). Behind `card-game-seams`, which the tests turn on; the
    /// only caller outside a test is the TV's, behind `played-card-games`,
    /// which only a lane's build names (AS.367's first rung).
    #[cfg(feature = "card-game-seams")]
    pub fn play_games(&mut self) {
        self.played = true;
    }

    /// The played game quit through the board's own confirm at the next step
    /// ([`step`], [`quit`]): the lane build's test hook (openspec AS.367's
    /// first rung), as a player's pause menu would. Feature-gated as
    /// [`CardStreams::play_games`] is.
    #[cfg(feature = "card-game-seams")]
    pub fn ask_quit(&mut self) {
        self.quit_asked = true;
    }

    /// Whether a yes plays the game ([`CardStreams::play_games`]).
    pub fn plays_games(&self) -> bool {
        self.played
    }

    /// Where the board's replacement of the hero stands ([`Restore`]).
    pub fn restore(&self) -> Restore {
        self.restore
    }

    /// The board's teardown's ask standing as asked ([`Restore::Asked`]):
    /// how the TV's tests stand for a played game's replacement set and
    /// cleared on the VM (openspec AS.553). Feature-gated as
    /// [`CardStreams::play_games`] is.
    #[cfg(feature = "card-game-seams")]
    pub fn restore_asked(&mut self) {
        self.restore = Restore::Asked;
    }

    /// The board standing as holding the hero's slot ([`Restore::Replaced`]):
    /// the TV's camera tests' stand-in for a played game's replacement on
    /// the VM (openspec AS.552). Feature-gated as
    /// [`CardStreams::play_games`] is.
    #[cfg(feature = "card-game-seams")]
    pub fn restore_replaced(&mut self) {
        self.restore = Restore::Replaced;
    }

    /// The restore answered: his character joined again.
    pub fn hero_returned(&mut self) {
        self.restore = Restore::Idle;
    }

    /// The units, for the listing.
    pub fn unit_list(&self) -> Vec<u16> {
        self.units.iter().map(|(_, u)| *u).collect()
    }

    /// A request's text taken (the step's end): one naming a linked stream is
    /// kept to join. Whether it named one.
    pub fn take_request(&mut self, text: Option<&str>) -> bool {
        let Some(text) = text else {
            return false;
        };
        let mut took = false;
        for archive in asked_all(text) {
            if !self.units.iter().any(|(a, _)| *a == archive) {
                continue;
            }
            info!(
                stream = archive,
                "a script asked for a card game stream: it joins the run"
            );
            self.pending.push(archive);
            took = true;
        }
        took
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
                // A stone loaded again (a second game) starts its script
                // again, as a reloaded archive does: its STARTUP is what
                // defines the stone, and the game's start cleared them.
                if !is_stone(archive) {
                    continue;
                }
                vm.remove(h);
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

/// Whether the minigame's loaded object is set (`portal control data`'s
/// `loaded object for minigame`, [`Restore`]).
fn replaced(vm: &Vm) -> bool {
    global(vm, "portal control data")
        .and_then(|pcd| field(vm, pcd, "loaded object for minigame"))
        .is_some_and(|v| v != Value::Null)
}

/// Whether the hero is owed back now (openspec AS.481): the board's
/// teardown asked for his character again ([`Restore::Asked`]), his minion
/// is off (`minion_off`) and no scene holds the players (`Cutscene State`).
/// The original's restore reloads his character's stream and its portal
/// manager brings his minion in, delaying a character's entry while
/// `Cutscene State` holds (its `delay entry because of cutscene`, the game's
/// quit and win scenes); his minion's own `Portal - Minion Spawn Sequence`
/// then holds him under designer control while his spawn clip plays and
/// lets him go at its end. The TV answers it as a figure put back on the
/// portal (its swap: his file unloaded and joined again), so his minion
/// runs the same sequence; the stream's load time is ours, none (stand-in
/// ledger: `card-restore`). The caller answers it and then calls
/// [`CardStreams::hero_returned`].
pub fn hero_owed(vm: &Vm, world: &ScriptWorld, minion_off: bool) -> bool {
    world.card_streams.restore == Restore::Asked && minion_off && !crate::anchors::scene_holds(vm)
}

/// The hand select's page, by its title's name.
pub const HAND_SELECT_TITLE: &str = "UI_CardGame_Menu_Title_HandSelect";
/// What the hand select's Play button runs: the recommended hand's game.
pub const HAND_SELECT_PLAY: &str = "CardGame Menu - Menu Hand Select - Action Play";
/// The game systems' script, in whose name the host's presses run.
const GAME_SYSTEMS: &str = "GameSystems.ai";

fn global(vm: &Vm, name: &str) -> Option<Value> {
    let g = vm.program().global_named(name)?;
    vm.global_peek(g).map(|v| vm.value(v))
}

fn field(vm: &Vm, v: Value, name: &str) -> Option<Value> {
    vm.field(v, name).map(|f| vm.value(f))
}

/// The menu page up and its title's name, if a page is up.
pub fn page_up(vm: &Vm) -> Option<(Value, Value, String)> {
    let control = global(vm, "menu control data")?;
    let page = field(vm, control, "current menu").filter(|p| *p != Value::Null)?;
    let title = field(vm, page, "menu def").and_then(|d| field(vm, d, "title"))?;
    let Value::Object(o) = title else {
        return None;
    };
    Some((control, page, vm.program().name_of(o)?.to_owned()))
}

/// Whether `page`'s opening has played (its timer past its animation in): a
/// press before then the original's menu doesn't take.
fn page_ready(vm: &Vm, page: Value) -> bool {
    let timer = field(vm, page, "menu timer").and_then(|t| t.as_f32());
    let anim = field(vm, page, "total anim in time").and_then(|t| t.as_f32());
    matches!((timer, anim), (Some(t), Some(a)) if t >= a)
}

/// A card game particle's removal no earlier than its disable time (the
/// module docs; stand-in ledger: `card-particles`): `UI - Particle Remover`
/// sets a particle's remove time from its generators' lifetimes
/// (`particleinfo.generators`), which the TV's host doesn't read, so the
/// write never lands and the time the board's script adds to it puts it in
/// the past: the stone's attack particle went the step after its spawn and
/// the flip that waits on it waited for good. A generator with no life of
/// its own leaves the remove time at the disable time, which this sets for
/// every particle the streams' templates made. How many it set.
fn hold_card_particles(vm: &mut Vm, world: &ScriptWorld) -> usize {
    let Some(Value::Entity(owner)) = global(vm, "Creature Struct Owner") else {
        return 0;
    };
    let Some(infos) = vm
        .placement_of(owner)
        .and_then(|p| vm.local(p, "particle remover infos"))
        .map(|v| vm.value(v))
        .and_then(|v| vm.set(v).map(<[Value]>::to_vec))
    else {
        return 0;
    };
    let mut held = 0;
    for info in infos {
        let info = vm.value(info);
        let Some(Value::Entity(particle)) = field(vm, info, "particle") else {
            continue;
        };
        let ours = world
            .actor(particle)
            .and_then(|a| a.template)
            .is_some_and(|t| world.card_streams.units.iter().any(|(_, u)| *u == t.unit));
        let disable = field(vm, info, "disable time").and_then(|v| v.as_f32());
        let remove = field(vm, info, "remove time").and_then(|v| v.as_f32());
        if let (true, Some(d), Some(r)) = (ours, disable, remove)
            && r < d
            && vm.set_field(info, "remove time", Value::Float(d))
        {
            held += 1;
        }
    }
    held
}

/// The misc stream slot (the module docs): the engine holds one misc
/// stream at a time (the hand select, the reward sprites, a stone's asset),
/// a misc load replacing the last and one naming nothing (`misc/`, a misc
/// actor's shutdown) emptying it. A request this step for a linked misc
/// card stream, or for none, takes the last one's running placements off
/// and runs their SHUTDOWNs now, before the step's end joins the new one ([`join_asked`]):
/// the hand select's starter closes its menus and gives the pause menu
/// back so, and asked again (a second game) the hand select starts anew.
/// How many it took off.
fn replace_misc(vm: &mut Vm, host: &mut WorldHost<'_, '_>) -> Result<usize, Halt> {
    let load = host.world.load_level;
    if load == Value::Null {
        return Ok(0);
    }
    let Some(text) = host.world.text_of(load) else {
        return Ok(0);
    };
    let streams = &host.world.card_streams;
    let next = asked_all(&text)
        .into_iter()
        .find(|a| is_misc(a) && streams.units.iter().any(|(u, _)| u == a));
    // A misc load naming nothing (`misc/`, a misc actor's shutdown) empties
    // the slot.
    if next.is_none() && !empties_misc(&text) {
        return Ok(0);
    }
    let last = std::mem::replace(&mut host.world.card_streams.misc, next);
    let Some(unit) = last.and_then(|l| {
        host.world
            .card_streams
            .units
            .iter()
            .find(|(a, _)| *a == l)
            .map(|(_, u)| *u)
    }) else {
        return Ok(0);
    };
    let off: Vec<_> = crate::ship_parts::stream_placements(host.world, unit)
        .into_iter()
        .map(|(_, h)| h)
        .filter(|h| vm.placement_of(*h).is_some_and(|id| !vm.removed(id)))
        .collect();
    for &h in &off {
        vm.remove(h);
    }
    if !off.is_empty() {
        info!(
            stream = last,
            taken_off = off.len(),
            "a misc load replaced the last card game stream"
        );
        vm.shut_down(host)?;
    }
    Ok(off.len())
}

/// One step's answers for a played card game (openspec AS.366), after the
/// walk as the level end's are; nothing on a run that doesn't play them
/// ([`CardStreams::play_games`]). The hand select's page, once open, is
/// answered by its Play button's action, the recommended hand
/// (`CardGame Menu - Create Recommended Hand`) played as the menu would have
/// it played; the game's particles are held to their disable time
/// ([`hold_card_particles`]). Whether the hand select was answered.
pub fn step(vm: &mut Vm, host: &mut WorldHost<'_, '_>) -> Result<bool, Halt> {
    replace_misc(vm, host)?;
    if !host.world.card_streams.played {
        return Ok(false);
    }
    if std::mem::take(&mut host.world.card_streams.quit_asked) && !quit(vm, host)? {
        info!("a quit asked with no board up: nothing to quit");
    }
    let now = replaced(vm);
    let restore = &mut host.world.card_streams.restore;
    *restore = restore.next(now);
    hold_card_particles(vm, host.world);
    let Some((control, page, title)) = page_up(vm) else {
        return Ok(false);
    };
    if title != HAND_SELECT_TITLE || !page_ready(vm, page) {
        return Ok(false);
    }
    let Some(gs) = running(vm, host.world, GAME_SYSTEMS) else {
        return Ok(false);
    };
    info!("the card game's hand select: the recommended hand played, as its Play button plays it");
    vm.call_for(
        gs,
        HAND_SELECT_PLAY,
        &[
            ("current page info", page),
            ("cur menu control data", control),
            ("just pressed", Value::Int(1)),
        ],
        &[],
        host,
    )?;
    Ok(true)
}

/// The board's quit's confirm (`CardGame_Setup.ai`'s `Quit - Yes`).
pub const QUIT_YES: &str = "Quit - Yes";
/// The board's controller's script: the game's set-up and its teardown.
const BOARD_CONTROLLER: &str = "CardGame_Setup.ai";

/// A played game quit as a player quits it (openspec AS.366): the board's
/// own quit confirm (`Quit - Yes`) run in its controller's name, as the
/// card game's pause menu's yes button runs it, which tells the pirate the
/// game was quit and takes the board down. Headless only: the TV has no
/// pause menu to quit from until AS.367. False when no board runs.
pub fn quit(vm: &mut Vm, host: &mut WorldHost<'_, '_>) -> Result<bool, Halt> {
    let Some(controller) = running(vm, host.world, BOARD_CONTROLLER).filter(|c| !vm.removed(*c))
    else {
        return Ok(false);
    };
    info!("a played card game quit through the board's own confirm");
    vm.call_for(controller, QUIT_YES, &[], &[], host)?;
    Ok(true)
}

#[cfg(test)]
mod tests;
