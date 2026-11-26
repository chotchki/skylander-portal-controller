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
//! - **What it leaves.** A build without `played-card-games` still answers
//!   `Card Game - Start Card Game` with nothing shown and tells the pirate it
//!   was quit ([`crate::prompt::quit_card_game`]); one with it (off by
//!   default, with `test-hooks`: the landing script's playtest build) plays
//!   the game, its hand select, board and loot drawn by the binary's
//!   `card_screens` (AS.365 to AS.368).

use guac_script::{Halt, Handle, PlacementId, Program, Value, Vm};
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
    /// Whether the board stands, its controller (`CardGame_Setup.ai`) and
    /// its manager in play, read after each step of a played run
    /// ([`note_board`]) and as a quit removes the controller (openspec
    /// AS.876).
    board: bool,
    /// Whether the TV's hand select answers the hand select's page
    /// ([`CardStreams::answer_hands_on_screen`]); headless it's answered by
    /// Play.
    hand_by_screen: bool,
    /// Whether a pirate's loot reports are left to the player's own
    /// `accept button` alone ([`CardStreams::leave_loot_to_accept`]), the
    /// TV's pass (`card-loot-accept`) off: a test's.
    loot_unpassed: bool,
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
            hand_by_screen: linked.hand_by_screen,
            ..Self::default()
        }
    }

    /// From here on the hand select's page waits for the TV's own hand
    /// select (openspec AS.555) in place of Play run at once. Changes
    /// nothing on a run that plays no game: its page never comes up.
    pub fn answer_hands_on_screen(&mut self) {
        self.hand_by_screen = true;
    }

    /// The pirate's loot reports left to the scripts' own `accept button`
    /// (openspec AS.670), [`pass_loot_reports`] off: what a test shows a
    /// press alone passes them by. Behind `card-game-seams`.
    #[cfg(feature = "card-game-seams")]
    pub fn leave_loot_to_accept(&mut self) {
        self.loot_unpassed = true;
    }

    /// From here on a yes at a pirate plays the game (openspec AS.366):
    /// `Card Game - Start Card Game` runs as the original's script, the hand
    /// select and board join as asked and the host answers what they wait on
    /// ([`step`]). Behind `card-game-seams`, which the tests turn on; the
    /// only caller outside a test is the TV's, behind `played-card-games`
    /// (AS.367's first rung: off by default, with `test-hooks`).
    #[cfg(feature = "card-game-seams")]
    pub fn play_games(&mut self) {
        self.played = true;
    }

    /// The played game quit through the board's own confirm at the next step
    /// ([`step`], [`quit`]): the played build's test hook (openspec AS.367's
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

    /// The replacement's state set by hand: a unit test's.
    #[cfg(test)]
    pub(crate) fn set_restore(&mut self, r: Restore) {
        self.restore = r;
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

    /// Whether the board stands ([`CardStreams::board`]): its place drawn
    /// with it.
    pub fn board_up(&self) -> bool {
        self.board
    }

    /// The board standing or not, set by hand: the TV's camera tests'
    /// stand-in for the controller in play (openspec AS.876).
    /// Feature-gated as [`CardStreams::play_games`] is.
    #[cfg(feature = "card-game-seams")]
    pub fn set_board_up(&mut self, up: bool) {
        self.board = up;
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

/// Whether `actor` is a clone of one of the card game's streams' templates
/// (a stone's stock, its face, the board's pieces): [`crate::hands::posing`]
/// holds its last clip's end (openspec AS.672).
pub fn card_clone(world: &ScriptWorld, actor: Handle) -> bool {
    let units = &world.card_streams.units;
    !units.is_empty()
        && world
            .actor(actor)
            .and_then(|a| a.template)
            .is_some_and(|t| units.iter().any(|(_, u)| *u == t.unit))
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

/// The hand select's data, the global its menu macros keep the hands in
/// (openspec AS.555): `recommended hand card set` (stone indices),
/// `custom hand card set` (each a `card index`) and `collection card set`
/// (the save's collected stones, each a `card index`).
pub const HAND_SELECT_INFO: &str = "CardGame_MenuHandSelectInfo";
/// The stones a hand holds: the custom hand set has five slots and the board deals five.
pub const HAND_SIZE: usize = 5;
/// The game's start off a hand (`hand index` 0 the recommended hand, any
/// other the custom one, saved as the last hand).
const START_GAME: &str = "CardGame Menu - Start Game";
/// What the build-hand page's Begin runs after the start: the menus from
/// the hand select's page down, their shutdowns run.
const CLOSE_FROM: &str = "Menu - Close All Menus Starting With Page ";
/// The player flag the build-hand page's Begin sets before its start, and
/// what the hand select's back-out runs.
const PLAYER_FLAG: &str = "Save - Get/Set Player Flag";
const BUILT_A_HAND: i32 = 6;
const HAND_SELECT_BACK_OUT: &str = "CardGame Menu - Menu Hand Select - Back Out";

/// What the hand select offers, read off its data while its page is up and
/// its opening has played (openspec AS.555).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandOffer {
    /// The hand the scripts recommend, by stone index: Play's hand.
    pub recommended: Vec<u16>,
    /// The save's collected stones, by index, in the collection's order:
    /// what a custom hand is built from.
    pub collection: Vec<u16>,
    /// The page's tutorial mode (a first game's).
    pub tutorial: bool,
}

/// How the hand select is answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandChoice {
    /// Play: the recommended hand, as its button runs it.
    Recommended,
    /// A hand of the player's own, stone indices, each once (the build-hand
    /// page refuses a stone the hand holds).
    Custom(Vec<u16>),
}

/// Whether `hand` is a custom hand the build-hand page would take: five
/// stones, each from `collection`, none twice.
pub fn hand_takes(hand: &[u16], collection: &[u16]) -> bool {
    hand.len() == HAND_SIZE
        && hand.iter().all(|s| collection.contains(s))
        && hand.iter().enumerate().all(|(i, s)| !hand[..i].contains(s))
}

fn stone_index(vm: &Vm, v: Value) -> Option<u16> {
    let v = vm.value(v);
    let n = match v {
        Value::Int(_) | Value::Float(_) => v.as_i32(),
        _ => field(vm, v, "card index").and_then(|c| c.as_i32()),
    }?;
    u16::try_from(n).ok()
}

fn stones_of(vm: &Vm, info: Value, set: &str) -> Vec<u16> {
    field(vm, info, set)
        .and_then(|s| vm.set(s).map(<[Value]>::to_vec))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| stone_index(vm, v))
        .collect()
}

/// The hand select's offer, while its page is up and ready for a press.
pub fn hand_offer(vm: &Vm) -> Option<HandOffer> {
    let (_, page, title) = page_up(vm)?;
    if title != HAND_SELECT_TITLE || !page_ready(vm, page) {
        return None;
    }
    let info = global(vm, HAND_SELECT_INFO)?;
    Some(HandOffer {
        recommended: stones_of(vm, info, "recommended hand card set"),
        collection: stones_of(vm, info, "collection card set"),
        tutorial: field(vm, info, "tutorial mode")
            .and_then(|t| t.as_i32())
            .is_some_and(|t| t > 0),
    })
}

/// The hand select answered with `choice` as its buttons answer it
/// (openspec AS.555): Play's action for the recommended hand; for a custom
/// one, its stones written into the custom hand's slots as the build-hand
/// page writes them, then what that page's Begin runs (the player flag, the
/// start off the custom hand, the menus closed from the hand select down).
/// False when no hand select is up and ready, or a custom hand the page
/// wouldn't take.
pub fn play_hand(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    choice: &HandChoice,
) -> Result<bool, Halt> {
    let Some(offer) = hand_offer(vm) else {
        return Ok(false);
    };
    let Some((control, page, _)) = page_up(vm) else {
        return Ok(false);
    };
    let Some(gs) = running(vm, host.world, GAME_SYSTEMS) else {
        return Ok(false);
    };
    let hand = match choice {
        HandChoice::Recommended => {
            info!(
                "the card game's hand select: the recommended hand played, as its Play button plays it"
            );
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
            return Ok(true);
        }
        HandChoice::Custom(hand) => hand,
    };
    if !hand_takes(hand, &offer.collection) {
        return Ok(false);
    }
    let Some(info) = global(vm, HAND_SELECT_INFO) else {
        return Ok(false);
    };
    let slots: Vec<Value> = field(vm, info, "custom hand card set")
        .and_then(|s| vm.set(s).map(<[Value]>::to_vec))
        .unwrap_or_default();
    if slots.len() != HAND_SIZE {
        return Ok(false);
    }
    for (slot, stone) in slots.into_iter().zip(hand) {
        let slot = vm.value(slot);
        if !vm.set_field(slot, "card index", Value::Int(i32::from(*stone))) {
            return Ok(false);
        }
    }
    let title = field(vm, page, "menu def")
        .and_then(|d| field(vm, d, "title"))
        .unwrap_or(Value::Null);
    info!(
        ?hand,
        "the card game's hand select: a hand of the player's own played, as the build-hand page's Begin plays it"
    );
    vm.call_for(
        gs,
        PLAYER_FLAG,
        &[
            ("setting id", Value::Int(BUILT_A_HAND)),
            ("value", Value::Int(1)),
            ("do save", Value::Int(1)),
        ],
        &[],
        host,
    )?;
    vm.call_for(gs, START_GAME, &[("hand index", Value::Int(1))], &[], host)?;
    vm.call_for(
        gs,
        CLOSE_FROM,
        &[("run shutdown", Value::Int(1)), ("page title", title)],
        &[],
        host,
    )?;
    Ok(true)
}

/// The hand select backed out of, as its back button does: its own
/// back-out, which cleans the card data up, then its page closed with its
/// shutdowns run. False when no hand select is up.
pub fn back_out_of_hand_select(vm: &mut Vm, host: &mut WorldHost<'_, '_>) -> Result<bool, Halt> {
    let Some((control, page, title)) = page_up(vm) else {
        return Ok(false);
    };
    if title != HAND_SELECT_TITLE {
        return Ok(false);
    }
    let Some(gs) = running(vm, host.world, GAME_SYSTEMS) else {
        return Ok(false);
    };
    let title = field(vm, page, "menu def")
        .and_then(|d| field(vm, d, "title"))
        .unwrap_or(Value::Null);
    info!("the card game's hand select backed out of, as its back button does");
    vm.call_for(
        gs,
        HAND_SELECT_BACK_OUT,
        &[("page info", page), ("menu control data", control)],
        &[],
        host,
    )?;
    vm.call_for(
        gs,
        CLOSE_FROM,
        &[("run shutdown", Value::Int(1)), ("page title", title)],
        &[],
        host,
    )?;
    Ok(true)
}

/// The board's manager, by its clone's name.
const MANAGER: &str = "CardGameManager";

/// The running board manager, if a game's board is up.
pub fn manager(vm: &Vm, world: &ScriptWorld) -> Option<PlacementId> {
    let mut found = None;
    let mut pid: PlacementId = 0;
    while let Some(h) = vm.handle_of(pid) {
        if !vm.removed(pid) && world.name(h) == MANAGER {
            found = Some(pid);
        }
        pid += 1;
    }
    found
}

/// Whether a board manager's behaviour is one of the tutorial's stops
/// (openspec AS.877, AS.879): the first game's `wait for tutorial - ...`
/// and the later lessons' `tutorial N - ...`, each of which puts its line
/// in a message box.
pub fn is_tutorial_stop(behaviour: &str) -> bool {
    behaviour.starts_with("wait for tutorial")
        || behaviour
            .strip_prefix("tutorial ")
            .and_then(|r| r.chars().next())
            .is_some_and(|c| c.is_ascii_digit())
}

/// The tutorial's stops that go on by themselves (as-cards-first, off the
/// board's script): the turn's intro once its voice line has played, the
/// hand's explanation once its footer is up. Every other stop waits on the
/// player's accept.
pub const TIMED_TUTORIAL_STOPS: [&str; 2] = [
    "wait for tutorial - wait for turn intro",
    "wait for tutorial - wait for hand explanation",
];

/// Whether a tutorial stop waits on the player's accept (openspec AS.879).
pub fn tutorial_waits_on_accept(behaviour: &str) -> bool {
    is_tutorial_stop(behaviour) && !TIMED_TUTORIAL_STOPS.contains(&behaviour)
}

/// The line the tutorial's message box shows (openspec AS.877): the text
/// sprites in the board manager's `tutorial container` (`MainUI - Message
/// Box Text Create`), their strings the disc's, read off the interface
/// model at run time and joined. Up as long as the scripts keep the box
/// (the hand's explanation stays into the card's pick); none with no box,
/// or before its line is in.
pub fn tutorial_line(vm: &Vm, world: &ScriptWorld) -> Option<String> {
    manager_box_text(vm, world, "tutorial container")
}

/// The words of the box the board manager keeps in its local `local`: its
/// text sprites' strings off the interface model, joined; none with no box
/// or no words in it yet.
fn manager_box_text(vm: &Vm, world: &ScriptWorld, local: &str) -> Option<String> {
    let m = manager(vm, world)?;
    let container = vm.local(m, local).map(|v| vm.value(v))?;
    if container == Value::Null {
        return None;
    }
    let lines = world.texts_under(container);
    (!lines.is_empty()).then(|| lines.join(" "))
}

/// The name of the stone landing on the board (openspec AS.911): as a
/// stone is placed (either player's, `general turn - wait for card
/// placement`) the board manager puts a title-bar box up with the stone's
/// `card name`, keeps it in its local `card name intro` while the name's
/// voice plays, then fades it and takes it down. Its words are the disc's,
/// read off the interface model at run time; none while no box is up.
pub fn placed_stone_name(vm: &Vm, world: &ScriptWorld) -> Option<String> {
    manager_box_text(vm, world, "card name intro")
}

/// A stone's name as the interface reads it (openspec AS.911): its
/// `Card_Name_(NNN)` string, the one the hand select's name sprite and the
/// loot's report show. None where the program or the model doesn't hold it.
pub fn stone_name(vm: &Vm, world: &ScriptWorld, stone: u16) -> Option<String> {
    let o = vm
        .program()
        .object_named(&format!("Card_Name_({stone:03})"))?;
    world.text_of(Value::Object(o))
}

/// Both scores as the board's script holds them (openspec AS.556): the
/// manager's game data's `remembered player score` and `remembered
/// opponent score`, which `CardGame - Update Score Header` sets from the
/// stones each side holds on the board at each of its updates. The
/// player's first. None with no board up.
pub fn score(vm: &Vm, world: &ScriptWorld) -> Option<(i32, i32)> {
    let m = manager(vm, world)?;
    let data = vm.local(m, "game data ref").map(|v| vm.value(v))?;
    let n = |name| field(vm, data, name).and_then(|v| v.as_i32());
    Some((
        n("remembered player score")?,
        n("remembered opponent score")?,
    ))
}

/// What the board's manager holds of a player's pick, for a driver's and a
/// test's read (`/api/_test/tv`): its behaviour, the stone the hand's
/// cursor is on, the square the board's cursor is on, how many stones the
/// board's own preview flips there (`cards to flip this move`), the squares
/// taken and each hand stone's pips.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct BoardRead {
    pub behaviour: Option<String>,
    pub card_at: Option<i32>,
    pub cursor: Option<(i32, i32)>,
    pub flips: usize,
    pub taken: Vec<(i32, i32)>,
    pub hand_pips: Vec<f32>,
}

fn square(v: Value) -> Option<(i32, i32)> {
    match v {
        Value::Screen(b) => {
            let (x, y) = crate::viewport::unpack_screen(b);
            Some((x as i32, y as i32))
        }
        _ => None,
    }
}

/// The board's read ([`BoardRead`]), while a board is up.
pub fn board_read(vm: &Vm, world: &ScriptWorld) -> Option<BoardRead> {
    let m = manager(vm, world)?;
    let local = |n: &str| vm.local(m, n).map(|v| vm.value(v));
    let set = |v: Value| vm.set(v).map(<[Value]>::to_vec).unwrap_or_default();
    let data = local("game data ref");
    let pips = |c: Value| {
        [
            "north strength",
            "east strength",
            "south strength",
            "west strength",
        ]
        .iter()
        .filter_map(|f| field(vm, vm.value(c), f).and_then(|x| x.as_f32()))
        .sum::<f32>()
    };
    Some(BoardRead {
        behaviour: vm.behaviour(m).map(str::to_owned),
        card_at: local("card selection index").and_then(|v| v.as_i32()),
        cursor: local("board selection location").and_then(square),
        flips: local("cards to flip this move").map_or(0, |v| set(v).len()),
        taken: data
            .and_then(|d| field(vm, d, "all objects on board"))
            .map(set)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|o| field(vm, vm.value(o), "board location").and_then(square))
            .collect(),
        hand_pips: data
            .and_then(|d| field(vm, d, "my hand"))
            .map(set)
            .unwrap_or_default()
            .into_iter()
            .map(pips)
            .collect(),
    })
}

/// The card loot's reports (the reward card's and the coins'), each by its
/// update macro, the field its stage sits under and the stage at which it
/// waits on a player's accept press (openspec AS.368). The reporter's
/// pre-script removes it once its update sets `completed`; the pirate's
/// `wait for loot theatrics` holds while it lives.
const LOOT_REPORTS: [(&str, LootStage, i32); 2] = [
    (
        "UI - TR - Reward Card - Update",
        LootStage::Def("anim info"),
        2,
    ),
    (
        "UI - TR - Reward - Coins - Update",
        LootStage::Context("anim data"),
        9,
    ),
];

/// Where a report's stage is kept: under its def, or under each of its
/// context data.
#[derive(Debug, Clone, Copy)]
enum LootStage {
    Def(&'static str),
    Context(&'static str),
}

/// The reporters' script.
pub(crate) const REPORTER: &str = "UI_TheatricalReporter";

/// The pirate behaviours in which his loot's reports are up.
const LOOT_WAITS: [&str; 3] = [
    "wait for loot to be awarded",
    "wait for 2d sprites to be ready",
    "wait for loot theatrics",
];

/// Whether a player's accept went down this step: either pad's bottom face
/// button (action 1), pressed now and not the step before. The reports ask
/// `accept button` of `controller 1` and `controller 2`, which the host's
/// pads don't serve (only `action 1` to `action 4`; filed AS.670), and the
/// TV's South is both, read as the player holds it
/// ([`ScriptWorld::hold_accept`]): after a game the hero comes back under
/// his pirate's hold, and the TV gives a hero not joined no actions.
fn accept_pressed(world: &ScriptWorld) -> bool {
    world
        .accept
        .iter()
        .zip(world.accept_before.iter())
        .any(|(now, was)| *now && !*was)
}

/// The stage `report` (a reporter's `tr def ref`) waits a press at, where
/// that stage sits, if it waits on one now.
fn loot_report_waiting(vm: &Vm, def: Value) -> Option<Value> {
    let update = field(vm, def, "update macro")?;
    let Value::Object(o) = update else {
        return None;
    };
    let name = vm.program().name_of(o)?;
    let (_, at, waits) = LOOT_REPORTS.iter().find(|(n, _, _)| *n == name)?;
    let holders: Vec<Value> = match at {
        LootStage::Def(f) => field(vm, def, f).into_iter().collect(),
        LootStage::Context(f) => field(vm, def, "context data")
            .and_then(|s| vm.set(s).map(<[Value]>::to_vec))
            .unwrap_or_default()
            .into_iter()
            .filter_map(|c| field(vm, vm.value(c), f))
            .collect(),
    };
    holders
        .into_iter()
        .find(|h| field(vm, *h, "stage").and_then(|s| s.as_i32()) == Some(*waits))
}

/// Whether a person's behaviour (the level's people, `behaviours`) is one
/// of a pirate's loot waits ([`LOOT_WAITS`]): the only time
/// [`pass_loot_reports`] passes anything. Another level's reward card or
/// coins report (the same update macros) stays the scripts' own.
fn in_pirates_loot(behaviours: &[Option<&str>]) -> bool {
    behaviours
        .iter()
        .any(|b| b.is_some_and(|b| LOOT_WAITS.contains(&b)))
}

/// What a pirate's loot puts up while its reports are up (openspec AS.673),
/// read off each report's definition as its update macro names it: the
/// reward card's title string and the stone it gives (its name string and
/// its number off that string's `Card_Name_(NNN)`), the coins' title.
/// Strings are the interface's objects, their words read at run time.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LootUp {
    pub card_title: Option<Value>,
    pub stone: Option<u16>,
    pub stone_name: Option<Value>,
    pub coins_title: Option<Value>,
    /// The coins' report is up.
    pub coins: bool,
    /// A report among them waits on the player's accept now.
    pub waiting: bool,
}

/// The stone a `Card_Name_(NNN)` string names.
pub fn stone_of_name(name: &str) -> Option<u16> {
    name.strip_prefix("Card_Name_(")?
        .strip_suffix(')')?
        .parse()
        .ok()
}

/// [`LootUp`] while a pirate is in his loot ([`LOOT_WAITS`]) and a report of
/// its is up; none otherwise.
pub fn loot_up(vm: &Vm, world: &ScriptWorld) -> Option<LootUp> {
    let behaviours: Vec<Option<&str>> = world
        .people()
        .into_iter()
        .map(|h| vm.placement_of(h).and_then(|p| vm.behaviour(p)))
        .collect();
    if !in_pirates_loot(&behaviours) {
        return None;
    }
    let mut up = LootUp::default();
    let mut any = false;
    let mut pid: PlacementId = 0;
    while let Some(h) = vm.handle_of(pid) {
        pid += 1;
        if vm.removed(pid - 1) || world.name(h) != REPORTER {
            continue;
        }
        let Some(def) = vm.local(pid - 1, "tr def ref").map(|v| vm.value(v)) else {
            continue;
        };
        let Some(Value::Object(update)) = field(vm, def, "update macro") else {
            continue;
        };
        let title = field(vm, def, "title ref").filter(|t| *t != Value::Null);
        up.waiting |= loot_report_waiting(vm, def).is_some();
        match vm.program().name_of(update) {
            Some(n) if n == LOOT_REPORTS[0].0 => {
                any = true;
                up.card_title = title;
                let first = field(vm, def, "context data")
                    .and_then(|s| vm.set(s).and_then(|s| s.first().copied()))
                    .map(|c| vm.value(c));
                let name = first.and_then(|c| field(vm, c, "card name ref"));
                up.stone = match name {
                    Some(Value::Object(o)) => vm.program().name_of(o).and_then(stone_of_name),
                    _ => None,
                };
                up.stone_name = name;
            }
            Some(n) if n == LOOT_REPORTS[1].0 => {
                any = true;
                up.coins = true;
                up.coins_title = title;
            }
            _ => {}
        }
    }
    any.then_some(up)
}

/// A card loot's report waiting on the accept passed on a player's own
/// accept press, as the press would pass it (the report's stage moved on
/// one, its own next stages slide it out and end it); stand-in ledger:
/// `card-loot-accept`. Only while a pirate's loot is up. How many it passed.
fn pass_loot_reports(vm: &mut Vm, world: &ScriptWorld) -> usize {
    if world.card_streams.loot_unpassed || !accept_pressed(world) {
        return 0;
    }
    let behaviours: Vec<Option<&str>> = world
        .people()
        .into_iter()
        .map(|h| vm.placement_of(h).and_then(|p| vm.behaviour(p)))
        .collect();
    if !in_pirates_loot(&behaviours) {
        return 0;
    }
    let mut waiting = Vec::new();
    let mut pid: PlacementId = 0;
    while let Some(h) = vm.handle_of(pid) {
        if !vm.removed(pid)
            && world.name(h) == REPORTER
            && let Some(def) = vm.local(pid, "tr def ref").map(|v| vm.value(v))
            && let Some(stage) = loot_report_waiting(vm, def)
        {
            waiting.push(stage);
        }
        pid += 1;
    }
    let mut passed = 0;
    for stage in waiting {
        let next = field(vm, stage, "stage")
            .and_then(|s| s.as_i32())
            .unwrap_or(0)
            + 1;
        if vm.set_field(stage, "stage", Value::Int(next)) {
            info!(
                stage = next,
                "a card loot's report passed on the player's accept"
            );
            passed += 1;
        }
    }
    passed
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
    let clones = take_off_clones_of(vm, host.world, unit);
    if !off.is_empty() || clones > 0 {
        info!(
            stream = last,
            taken_off = off.len(),
            clones,
            "a misc load replaced the last card game stream"
        );
        vm.shut_down(host)?;
    }
    Ok(off.len() + clones)
}

/// Takes off every live clone of a template of `unit`, a card stream leaving
/// (openspec AS.880): a stone's asset holds the creature it presents and its
/// weapon (`Card_Model_Attach_NNN`, `CardMdlAttach_NN`), whose clones the board
/// hangs on the presented creature and never removes, and with the archive gone
/// nothing of them can stay. A clone with a script is taken off as a remove of it
/// does. How many.
fn take_off_clones_of(vm: &mut Vm, world: &mut ScriptWorld, unit: u16) -> usize {
    let mut gone: Vec<Handle> = world
        .actors
        .iter()
        .filter(|(_, a)| a.obj.is_none() && !a.removed)
        .filter(|(_, a)| a.template.is_some_and(|t| t.unit == unit))
        .map(|(h, _)| *h)
        .collect();
    gone.sort_unstable();
    for &h in &gone {
        if let Some(a) = world.actors.get_mut(&h) {
            a.removed = true;
        }
        vm.remove(h);
    }
    gone.len()
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
    pass_loot_reports(vm, host.world);
    // The TV's hand select answers the page itself ([`hand_offer`],
    // [`play_hand`]); headless the recommended hand is played as Play plays it.
    if host.world.card_streams.hand_by_screen {
        return Ok(false);
    }
    play_hand(vm, host, &HandChoice::Recommended)
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
    // The TV answers the quit after the step that read the board (openspec
    // AS.876): `Quit - Yes` removes the controller there and then, so the
    // board is read again now, as the next step would, and the camera this
    // frame knows its place is empty.
    host.world.card_streams.board = board_standing(vm, host.world);
    Ok(true)
}

/// Whether the board stands (openspec AS.876): its controller in play
/// (a quit removes it first) and its manager too (a won or lost game's end
/// takes the manager, the stones with it, a step before the controller).
fn board_standing(vm: &Vm, world: &ScriptWorld) -> bool {
    running(vm, world, BOARD_CONTROLLER).is_some_and(|c| !vm.removed(c))
        && manager(vm, world).is_some()
}

/// The board read once the step's requests are taken (openspec AS.876):
/// what the step's own ticks took down (a won or lost game's end takes the
/// manager, then the controller) reads gone before the TV's camera draws
/// that frame. [`step`] runs before the host takes them, so it doesn't read
/// it. Nothing on a run that doesn't play the game.
pub fn note_board(vm: &Vm, world: &mut ScriptWorld) {
    if world.card_streams.played {
        world.card_streams.board = board_standing(vm, world);
    }
}

/// The stones a fresh save's first win at Fangs (205's tutorial) collects,
/// as `giants_level_205`'s first-game test reads them off the save after it
/// (openspec AS.367, AS.675).
pub const STARTER_STONES: [u16; 6] = [0, 12, 15, 29, 46, 53];

/// The game systems' collection write a won game's stones go through.
pub const COLLECT_CARD: &str = "Save - Collection Get/Set - Card";

/// The starter stones collected into the run's save through the scripts'
/// own collection write (openspec AS.675: a lane's route to a save holding
/// them, as Fangs' won tutorial leaves it, so the hand select offers a hand
/// off the collection and Custom shows). It gives stones only: no game is
/// counted won, no pirate's state touched. False when the game systems
/// don't run.
pub fn give_starter_stones(vm: &mut Vm, host: &mut WorldHost<'_, '_>) -> Result<bool, Halt> {
    let Some(gs) = running(vm, host.world, GAME_SYSTEMS) else {
        return Ok(false);
    };
    for stone in STARTER_STONES {
        vm.call_for(
            gs,
            COLLECT_CARD,
            &[
                ("card id", Value::Int(i32::from(stone))),
                ("collected", Value::Int(1)),
            ],
            &[],
            host,
        )?;
    }
    info!(stones = ?STARTER_STONES, "the starter stones collected through the scripts' own write");
    Ok(true)
}

#[cfg(test)]
mod tests;
