//! The card game's three screens a kid needs on the TV (openspec AS.555 to
//! AS.557, the design doc's The card game's screen): Bevy pieces of ours in
//! the disc's images over the VM's state, where the original draws menus
//! and interface sprites the TV doesn't run yet (AT.16). Each is on the
//! stand-in ledger (`card-hand-select`, `card-score`, `card-quit`).
//!
//! - **The hand select**, while the hand select's page is up and ready
//!   ([`card_game::hand_offer`]): the hand in the stones' own pictures
//!   (`Card_Sprite_NNN`), Begin and Custom on the record's buttons. Begin
//!   plays the hand shown, the recommended one unless it was changed;
//!   Custom (offered when the save holds five stones or more) moves the focus
//!   onto the hand, where left and right pick a slot and up and down change
//!   its stone through the collection, never to a stone the hand holds;
//!   accept or back go back to Begin. Back on Begin backs out of the page as
//!   its own back button does. A kid who presses nothing gets the
//!   recommended hand played after [`IDLE_PLAY`].
//! - **The score header**, while a board is up: both scores as the board's
//!   script holds them ([`card_game::score`]) on the board record's
//!   `ScoreBox` pieces, the player's left, at the top of the picture.
//! - **The quit**, Start while a board is up: the level waits under Start as
//!   it does anywhere, and in place of its overlay a confirm asks whether to
//!   quit the game, the focus on No; Yes runs the board's own `Quit - Yes`
//!   ([`card_game::quit`]), which tells the pirate and takes the board
//!   down, and the hero comes back through the TV's swap (AS.553); No goes
//!   back to the board. Either answer lets the level go on.
//!
//! None of it shows on a run that plays no game: the hand select's page and
//! the board come up only where a yes at a pirate plays the game, which only
//! a build with `played-card-games` does (off by default, with `test-hooks`).

use std::sync::OnceLock;
use std::time::Duration;

use bevy::image::CompressedImageFormats;
use bevy::math::CompassOctant;
use bevy::prelude::*;
use bevy::render::renderer::RenderDevice;
use guacamole::card_game::{self, HandChoice, HandOffer};
use guacamole::scripts::{ScriptClone, ScriptRuntime, ScriptWorld, WorldHost};
use serde::Serialize;
use tracing::{info, warn};

use crate::completion::{Art, FONT_ARCHIVE, bitmap_text, load_art_of, upright_image};
use crate::hero_scripts::ScriptSource;
use crate::input::TvInput;
use crate::pad::LevelPads;
use crate::theme::{Role, Step, TvTheme};
use crate::tv_state::{Pen, Redrawn, TvAssets};

/// Over the level and Start's overlay (900), with the dialogue's question
/// (955), under the corner join code (960).
const CARD_Z: i32 = 955;
/// The score header's, under every overlay.
const SCORE_Z: i32 = 880;

/// How long the hand select waits on a kid who presses nothing before the
/// hand shown is played (ours: the original waits).
pub(crate) const IDLE_PLAY: Duration = Duration::from_secs(30);

/// The original's screen units to the TV's design pixels (the completion
/// screen's).
const UNIT: f32 = 2.0;
const LABEL_SIZE: f32 = 40.0;
const SCORE_SIZE: f32 = 64.0;
const BOX_FILL: Color = Color::srgba(0.03, 0.05, 0.16, 0.86);
const LABEL_COLOR: Color = Color::WHITE;
const FOCUS_RING: Color = Color::srgb(1.0, 0.82, 0.25);
const PLAYER_TINT: Color = Color::srgb(0.35, 0.6, 1.0);
const OPPONENT_TINT: Color = Color::srgb(1.0, 0.45, 0.35);

/// The hand select's record and pieces, by template name.
const BEGIN: &str = "UI CardGame HandSelect Button Begin";
const CUSTOM: &str = "UI CardGame HandSelect Button Custom";
/// The board record's score box pieces.
const SCORE_CAP: &str = "ScoreBox_Cap";

/// The end-of-game strip's words (openspec AS.875), ours: the report the
/// original draws there isn't drawn, and its choice is the kid's to make.
pub(crate) const PLAY_AGAIN: &str = "Play again";
pub(crate) const LEAVE: &str = "Leave";

/// What a strip at the bottom of the board asks of the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Strip {
    /// A lost game (openspec AS.875): A plays again, B leaves (the report's
    /// first action and cancel).
    Lost,
    /// A won game (AS.875): A goes on.
    Won,
    /// A tutorial stop (openspec AS.879): the board's manager waits on the
    /// player's accept under a line the TV doesn't draw (AS.877); A goes on.
    Tutorial,
}

/// The strip the board asks for, by its behaviour (openspec AS.875,
/// AS.879): over a finished game, or at a stop of the tutorial's that waits
/// on the accept ([`card_game::tutorial_waits_on_accept`]). None mid-game.
pub(crate) fn game_over(board: Option<&str>) -> Option<Strip> {
    match board {
        Some("lost") => Some(Strip::Lost),
        Some("won") => Some(Strip::Won),
        Some(b) if card_game::tutorial_waits_on_accept(b) => Some(Strip::Tutorial),
        _ => None,
    }
}
const SCORE_TILE: &str = "ScoreBox_Tile";
/// A tutorial stop's line box (openspec AS.877), in design px: a column at
/// the left, under the score header's height, its width short of where the
/// board and the hand start (a fifth of the 1920), so it never covers the
/// stones a stop talks about (as-cards-first `look/f1-b-tutorial.jpg`).
const TUTORIAL_LEFT: f32 = 32.0;
const TUTORIAL_TOP: f32 = 220.0;
const TUTORIAL_WIDTH: f32 = 340.0;

/// A stone's picture in the hand select's record, by its index.
fn stone_sprite(stone: u16) -> String {
    format!("Card_Sprite_{stone:03}")
}

/// Every name the hand select draws, its 82 stones' pictures and its two
/// buttons, for the art's load.
fn hand_art_names() -> &'static [&'static str] {
    static NAMES: OnceLock<Vec<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names: Vec<&'static str> = card_game::stones()
            .iter()
            .filter(|s| s.contains("cardgame_deck_"))
            .filter_map(|s| s.rsplit('_').next()?.split('.').next()?.parse::<u16>().ok())
            .map(|n| &*Box::leak(stone_sprite(n).into_boxed_str()))
            .collect();
        names.sort_unstable();
        names.dedup();
        names.extend([BEGIN, CUSTOM]);
        names
    })
}

/// What a press on the hand select acts on: Play, Custom, or a slot of the
/// hand while it's being changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum HandFocus {
    Begin,
    Custom,
    /// A slot of the hand being changed.
    Slot(usize),
}

/// What a press on the hand select asks of the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HandOutcome {
    Play(HandChoice),
    BackOut,
}

/// The hand select as a kid moves through it: the offer it opened on, the
/// hand Play would deal now and where the next press lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HandEdit {
    pub(crate) offer: HandOffer,
    pub(crate) hand: Vec<u16>,
    pub(crate) focus: HandFocus,
}

impl HandEdit {
    pub(crate) fn new(offer: HandOffer) -> Self {
        Self {
            hand: offer.recommended.clone(),
            offer,
            focus: HandFocus::Begin,
        }
    }

    /// The collection's distinct stones, in its order.
    fn stones(&self) -> Vec<u16> {
        let mut out: Vec<u16> = Vec::new();
        for s in &self.offer.collection {
            if !out.contains(s) {
                out.push(*s);
            }
        }
        out
    }

    /// Whether a hand of the player's own can be built: five stones collected.
    pub(crate) fn can_custom(&self) -> bool {
        self.stones().len() >= card_game::HAND_SIZE
    }

    /// The choice Begin plays: the recommended hand, unless it was changed.
    pub(crate) fn choice(&self) -> HandChoice {
        if self.hand == self.offer.recommended {
            HandChoice::Recommended
        } else {
            HandChoice::Custom(self.hand.clone())
        }
    }

    /// The slot's stone moved through the collection, `up` forward, to the
    /// next stone the hand doesn't hold.
    fn cycle(&mut self, slot: usize, up: bool) {
        let stones = self.stones();
        let Some(&now) = self.hand.get(slot) else {
            return;
        };
        let n = stones.len();
        let at = stones.iter().position(|s| *s == now).unwrap_or(0);
        for k in 1..n {
            let i = if up { (at + k) % n } else { (at + n - k) % n };
            if !self.hand.contains(&stones[i]) {
                self.hand[slot] = stones[i];
                return;
            }
        }
    }

    /// One press: what it asks of the page, if anything.
    pub(crate) fn input(&mut self, input: TvInput) -> Option<HandOutcome> {
        use CompassOctant::{East, North, South, West};
        match (self.focus, input) {
            (HandFocus::Begin, TvInput::Activate) => return Some(HandOutcome::Play(self.choice())),
            (HandFocus::Begin, TvInput::Back) => return Some(HandOutcome::BackOut),
            (HandFocus::Begin, TvInput::Move(West | East)) if self.can_custom() => {
                self.focus = HandFocus::Custom;
            }
            (HandFocus::Custom, TvInput::Move(West | East) | TvInput::Back) => {
                self.focus = HandFocus::Begin;
            }
            (HandFocus::Custom, TvInput::Activate) => {
                if !card_game::hand_takes(&self.hand, &self.stones()) {
                    self.hand = self.stones()[..card_game::HAND_SIZE].to_vec();
                }
                self.focus = HandFocus::Slot(0);
            }
            (HandFocus::Slot(k), TvInput::Move(West)) => {
                self.focus = HandFocus::Slot(k.saturating_sub(1))
            }
            (HandFocus::Slot(k), TvInput::Move(East)) => {
                self.focus = HandFocus::Slot((k + 1).min(card_game::HAND_SIZE - 1));
            }
            (HandFocus::Slot(k), TvInput::Move(North)) => self.cycle(k, true),
            (HandFocus::Slot(k), TvInput::Move(South)) => self.cycle(k, false),
            (HandFocus::Slot(_), TvInput::Activate | TvInput::Back) => {
                self.focus = HandFocus::Begin
            }
            _ => {}
        }
        None
    }
}

/// The quit confirm's focus: Yes or No.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub(crate) enum QuitFocus {
    Yes,
    #[default]
    No,
}

/// What the scripts' step reads of the game for the screens.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub(crate) struct CardRead {
    pub(crate) offer: Option<HandOffer>,
    /// The board's pick state, for `/api/_test/tv`'s drivers.
    pub(crate) pick: Option<card_game::BoardRead>,
    pub(crate) score: Option<(i32, i32)>,
    pub(crate) board: bool,
    /// A pirate's loot reports up (openspec AS.673), their words read.
    pub(crate) loot: Option<LootShown>,
    /// A tutorial stop's line, the disc's words read (openspec AS.877).
    pub(crate) tutorial: Option<String>,
}

/// A pirate's loot as the TV draws it (openspec AS.673): the reward card's
/// title, the stone given and its name, the coins' title, each the
/// interface's own words read at run time; none for a string that didn't
/// read.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct LootShown {
    pub card_title: Option<String>,
    pub stone: Option<u16>,
    pub stone_name: Option<String>,
    pub coins_title: Option<String>,
    pub waiting: bool,
}

impl LootShown {
    pub(crate) fn of(up: &card_game::LootUp, world: &ScriptWorld) -> Self {
        let text = |v: Option<guac_script::Value>| v.and_then(|v| world.text_of(v));
        Self {
            card_title: text(up.card_title),
            stone: up.stone,
            stone_name: text(up.stone_name),
            coins_title: text(up.coins_title),
            waiting: up.waiting,
        }
    }
}

/// What the screens ask of the next step.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub(crate) struct CardAsk {
    pub(crate) hand: Option<HandOutcome>,
    pub(crate) quit: bool,
}

/// What the screens show between the scripts' steps: the hand select's edit,
/// the quit confirm's focus and the answers given, kept apart from what the
/// step reads ([`CardRead`]) so a press never waits on a step.
#[derive(Resource, Debug, Default)]
pub(crate) struct CardScreens {
    pub(crate) hand: Option<HandEdit>,
    /// When the hand select opened or last took a press (the real clock).
    hand_since: Duration,
    pub(crate) quit: Option<QuitFocus>,
    /// What's been answered this session, for `/api/_test/tv`.
    pub(crate) answered: Vec<String>,
}

/// The hand select or the quit confirm is up: the router hands them the
/// pads (`crate::input::Modals`).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CardScreensOpen(pub(crate) bool);

/// The quit confirm is up: Start's overlay steps aside for it.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CardQuitOpen(pub(crate) bool);

pub(crate) struct CardScreensPlugin;

impl Plugin for CardScreensPlugin {
    fn build(&self, app: &mut App) {
        guacamole::frame::ensure(app);
        app.init_resource::<CardRead>()
            .init_resource::<CardAsk>()
            .init_resource::<CardScreens>()
            .init_resource::<CardScreensOpen>()
            .init_resource::<CardQuitOpen>()
            .add_message::<TvInput>()
            .add_systems(
                FixedUpdate,
                answer_and_read
                    .in_set(guacamole::frame::Frame::Scripts)
                    .after(crate::hero_scripts::step_scripts)
                    .run_if(resource_exists::<ScriptRuntime>)
                    .run_if(resource_exists::<ScriptWorld>),
            )
            .add_systems(Update, (open, presses, show).chain());
    }
}

/// After the scripts' step: what the screens asked run on the game, then the
/// game read for them.
fn answer_and_read(
    mut commands: Commands,
    mut runtime: ResMut<ScriptRuntime>,
    mut world: ResMut<ScriptWorld>,
    mut ask: ResMut<CardAsk>,
    mut read: ResMut<CardRead>,
) {
    // A run that plays no game (every build without the feature) reads nothing: the
    // board's read walks every placement, three times a step.
    if !world.card_streams.plays_games() {
        return;
    }
    let hand = ask.hand.take();
    let quit = std::mem::take(&mut ask.quit);
    if hand.is_some() || quit {
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
        let done = match (&hand, quit) {
            (Some(HandOutcome::Play(choice)), _) => card_game::play_hand(vm, &mut host, choice),
            (Some(HandOutcome::BackOut), _) => card_game::back_out_of_hand_select(vm, &mut host),
            (None, _) => card_game::quit(vm, &mut host),
        };
        match done {
            Ok(true) => info!(?hand, quit, "the card game's screen answered"),
            Ok(false) => warn!(?hand, quit, "the card game's screen answered nothing up"),
            Err(h) => warn!(%h, "the card game's screen's answer stopped"),
        }
    }
    let vm = &runtime.vm;
    let now = CardRead {
        offer: card_game::hand_offer(vm),
        pick: card_game::board_read(vm, &world),
        score: card_game::score(vm, &world),
        board: card_game::manager(vm, &world).is_some(),
        loot: card_game::loot_up(vm, &world).map(|l| LootShown::of(&l, &world)),
        tutorial: card_game::tutorial_line(vm, &world),
    };
    if *read != now {
        *read = now;
    }
}

/// The hand select opens with the page and the quit confirm with Start over
/// a board; each closes as its page or board goes, or once answered. A kid
/// who presses nothing on the hand select gets its hand played.
#[allow(clippy::too_many_arguments)]
fn open(
    read: Res<CardRead>,
    mut screens: ResMut<CardScreens>,
    mut ask: ResMut<CardAsk>,
    pads: Res<LevelPads>,
    mut open: ResMut<CardScreensOpen>,
    mut quit_open: ResMut<CardQuitOpen>,
    time: Res<Time<Real>>,
) {
    match (&read.offer, &screens.hand) {
        (Some(o), None) if ask.hand.is_none() => {
            info!(?o, "the card game's hand select opens");
            screens.hand = Some(HandEdit::new(o.clone()));
            screens.hand_since = time.elapsed();
        }
        (None, Some(_)) => screens.hand = None,
        _ => {}
    }
    if let Some(edit) = &screens.hand
        && idle_due(screens.hand_since, time.elapsed())
        && ask.hand.is_none()
    {
        let choice = edit.choice();
        info!(
            ?choice,
            "nothing pressed on the hand select: its hand played"
        );
        screens.answered.push("hand: played unpressed".into());
        ask.hand = Some(HandOutcome::Play(choice));
        screens.hand = None;
    }
    let started = pads
        .seat()
        .and_then(|s| s.pause())
        .is_some_and(|p| matches!(p, crate::pad::Pause::Start { .. }));
    let was = screens.quit;
    screens.quit = quit_shown(was, read.board, started);
    if was.is_none() && screens.quit.is_some() {
        info!("Start over a card game's board: its quit asked");
    }
    // Answered (or the board gone): the level goes on.
    let up = screens.hand.is_some() || screens.quit.is_some();
    if open.0 != up {
        open.0 = up;
    }
    let q = screens.quit.is_some();
    if quit_open.0 != q {
        quit_open.0 = q;
    }
}

/// Whether a hand select open since `since` (the real clock) has waited
/// [`IDLE_PLAY`] on a kid who pressed nothing by `now`.
pub(crate) fn idle_due(since: Duration, now: Duration) -> bool {
    now.saturating_sub(since) >= IDLE_PLAY
}

/// The quit confirm as it stands next: up (focus on No) once Start waits
/// the level over a board, kept as it was while both hold, gone when
/// either ends.
pub(crate) fn quit_shown(was: Option<QuitFocus>, board: bool, started: bool) -> Option<QuitFocus> {
    match (board && started, was) {
        (false, _) => None,
        (true, None) => Some(QuitFocus::default()),
        (true, kept) => kept,
    }
}

/// The router's presses, to the screen up.
fn presses(
    mut inputs: MessageReader<TvInput>,
    open: Res<CardScreensOpen>,
    mut screens: ResMut<CardScreens>,
    mut ask: ResMut<CardAsk>,
    mut pads: ResMut<LevelPads>,
    time: Res<Time<Real>>,
) {
    let read: Vec<TvInput> = inputs.read().copied().collect();
    if !open.0 {
        return;
    }
    for input in read {
        if let Some(focus) = screens.quit {
            let answer = match input {
                TvInput::Move(CompassOctant::West | CompassOctant::East) => {
                    screens.quit = Some(match focus {
                        QuitFocus::Yes => QuitFocus::No,
                        QuitFocus::No => QuitFocus::Yes,
                    });
                    None
                }
                TvInput::Activate => Some(focus == QuitFocus::Yes),
                TvInput::Back => Some(false),
                _ => None,
            };
            if let Some(yes) = answer {
                info!(yes, "the card game's quit answered");
                screens
                    .answered
                    .push(format!("quit: {}", if yes { "yes" } else { "no" }));
                ask.quit = yes;
                screens.quit = None;
                if let Some(seat) = pads.seat_mut() {
                    seat.resume();
                }
            }
            continue;
        }
        if let Some(edit) = screens.hand.as_mut() {
            let outcome = edit.input(input);
            screens.hand_since = time.elapsed();
            if let Some(o) = outcome {
                info!(?o, "the card game's hand select answered");
                screens.answered.push(format!("hand: {o:?}"));
                ask.hand = Some(o);
                screens.hand = None;
            }
        }
    }
}

/// What the screens draw with, read once a session off the run's extract.
#[derive(Clone)]
struct CardArt {
    hand: Option<Art>,
    score: Option<Art>,
    prompt: Option<Art>,
}

fn load_card_art(
    source: Option<&ScriptSource>,
    images: &mut Assets<Image>,
    device: Option<&RenderDevice>,
) -> CardArt {
    let formats = device.map_or(CompressedImageFormats::BC, |d| {
        CompressedImageFormats::from_features(d.features())
    });
    let Some((dir, ir)) = source.and_then(ScriptSource::extract) else {
        return CardArt {
            hand: None,
            score: None,
            prompt: None,
        };
    };
    let mut load = |archive: &str, names: &[&'static str]| match load_art_of(
        dir, *ir, archive, names, images, formats,
    ) {
        Ok(a) => Some(a),
        Err(why) => {
            warn!(archive, reason = %why, "a card game screen's art didn't load: the TV's own style");
            None
        }
    };
    CardArt {
        hand: load(card_game::HAND_SELECT, hand_art_names()),
        score: load(card_game::BOARD, &[SCORE_CAP, SCORE_TILE]),
        prompt: load(
            FONT_ARCHIVE,
            &[
                crate::yes_no::YES_ON,
                crate::yes_no::NO_ON,
                crate::yes_no::YES_OFF,
                crate::yes_no::NO_OFF,
            ],
        ),
    }
}

/// What's drawn now, to redraw on a change.
#[derive(Debug, Clone, PartialEq)]
enum Drawn {
    Hand(Vec<u16>, HandFocus, bool),
    Quit(QuitFocus),
    Loot(LootShown, crate::pad::Labels),
    /// The strip at the bottom: a game's end (lost: A again, B leave; won:
    /// A on) or a tutorial stop (A on); and the tutorial's line while its
    /// box is up. One or both.
    Over(Option<Strip>, Option<String>, crate::pad::Labels),
}

/// A drawn card screen's root, for the shots.
#[derive(Component, Debug, Clone, PartialEq)]
pub(crate) struct CardScreenRoot(pub(crate) &'static str);

/// Spawns and despawns the screens as their state moves.
#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    screens: Res<CardScreens>,
    read: Res<CardRead>,
    (assets, theme): (Option<Res<TvAssets>>, Option<Res<TvTheme>>),
    (source, mut images, device): (
        Option<Res<ScriptSource>>,
        ResMut<Assets<Image>>,
        Option<Res<RenderDevice>>,
    ),
    mut redrawn: MessageWriter<Redrawn>,
    (pads, gamepads): (
        Option<Res<LevelPads>>,
        Query<(&Gamepad, Option<&crate::pad::FacesPrinted>)>,
    ),
    mut shown: Local<Option<(Drawn, Entity)>>,
    mut score_shown: Local<Option<((i32, i32), Entity)>>,
    mut art: Local<Option<CardArt>>,
) {
    let labels = pads.as_deref().map_or(crate::pad::Labels::Position, |p| {
        crate::button_prompt::walking_labels(p, &gamepads)
    });
    let want = match (&screens.quit, &screens.hand, &read.loot) {
        (Some(q), _, _) => Some(Drawn::Quit(*q)),
        (None, Some(e), _) => Some(Drawn::Hand(e.hand.clone(), e.focus, e.can_custom())),
        (None, None, Some(l)) => Some(Drawn::Loot(l.clone(), labels)),
        (None, None, None) => read.pick.as_ref().and_then(|p| {
            let strip = game_over(p.behaviour.as_deref());
            let line = read.tutorial.clone();
            (strip.is_some() || line.is_some()).then(|| Drawn::Over(strip, line, labels))
        }),
    };
    let score_want = read.score.filter(|_| read.board);
    let modal_same = shown.as_ref().map(|(d, _)| d) == want.as_ref();
    let score_same = score_shown.map(|(s, _)| s) == score_want;
    if modal_same && score_same {
        return;
    }
    let (Some(assets), Some(theme)) = (assets, theme) else {
        return;
    };
    let pen = Pen::new(&assets, &theme);
    let art =
        art.get_or_insert_with(|| load_card_art(source.as_deref(), &mut images, device.as_deref()));
    if !modal_same {
        if let Some((_, e)) = shown.take()
            && let Ok(mut root) = commands.get_entity(e)
        {
            root.despawn();
        }
        if let Some(w) = want {
            let e = match &w {
                Drawn::Hand(hand, focus, custom) => {
                    spawn_hand(&mut commands, &pen, art, hand, *focus, *custom)
                }
                Drawn::Quit(focus) => spawn_quit(&mut commands, &pen, art, *focus),
                Drawn::Loot(loot, labels) => spawn_loot(&mut commands, &pen, art, loot, *labels),
                Drawn::Over(strip, line, labels) => {
                    spawn_over(&mut commands, &pen, art, *strip, line.as_deref(), *labels)
                }
            };
            *shown = Some((w, e));
        }
        redrawn.write(Redrawn::Alert);
    }
    if !score_same {
        if let Some((_, e)) = score_shown.take()
            && let Ok(mut root) = commands.get_entity(e)
        {
            root.despawn();
        }
        if let Some(s) = score_want {
            *score_shown = Some((s, spawn_score(&mut commands, &pen, art, s)));
        }
        redrawn.write(Redrawn::Text);
    }
}

fn full_screen(name: &'static str, z: i32) -> impl Bundle {
    (
        CardScreenRoot(name),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        GlobalZIndex(z),
    )
}

/// A line of text in the disc's font where it loaded, else the TV's.
fn label(parent: &mut ChildSpawnerCommands, pen: &Pen, art: Option<&Art>, text: &str, size: f32) {
    match art {
        Some(a) => bitmap_text(parent, a, text, size, LABEL_COLOR),
        None => {
            parent.spawn(pen.text(text, Step::Md, Role::Accent));
        }
    }
}

/// A framed piece: a focus ring when focused.
fn ring(focused: bool) -> (BorderColor, Node) {
    (
        BorderColor::all(if focused { FOCUS_RING } else { Color::NONE }),
        Node {
            border: UiRect::all(px(6)),
            padding: UiRect::all(px(4)),
            border_radius: BorderRadius::all(px(12)),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            ..default()
        },
    )
}

fn spawn_hand(
    commands: &mut Commands,
    pen: &Pen,
    art: &CardArt,
    hand: &[u16],
    focus: HandFocus,
    custom: bool,
) -> Entity {
    let a = art.hand.as_ref();
    commands
        .spawn(full_screen("hand-select", CARD_Z))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(40),
                    padding: UiRect::axes(px(56), px(40)),
                    border_radius: BorderRadius::all(px(32)),
                    ..default()
                },
                BackgroundColor(BOX_FILL),
            ))
            .with_children(|panel| {
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(16),
                        ..default()
                    })
                    .with_children(|row| {
                        for (i, stone) in hand.iter().enumerate() {
                            let mut slot = row.spawn(ring(focus == HandFocus::Slot(i)));
                            slot.with_children(|s| {
                                let name = stone_sprite(*stone);
                                match a.and_then(|a| a.pieces.get(name.as_str())) {
                                    // Stored upside down, as the stones'
                                    // faces are (openspec AS.554).
                                    Some(p) => {
                                        s.spawn(stone_picture(p, 1.0));
                                    }
                                    None => {
                                        s.spawn(pen.text(
                                            &format!("#{stone:03}"),
                                            Step::Md,
                                            Role::Primary,
                                        ));
                                    }
                                }
                            });
                        }
                    });
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(48),
                        ..default()
                    })
                    .with_children(|buttons| {
                        let mut button = |name: &'static str, text: &str, focused: bool| {
                            buttons.spawn(ring(focused)).with_children(|b| {
                                match a.and_then(|a| a.pieces.get(name)) {
                                    Some(p) => {
                                        b.spawn(button_art(p))
                                            .with_children(|t| label(t, pen, a, text, LABEL_SIZE));
                                    }
                                    None => label(b, pen, a, text, LABEL_SIZE),
                                }
                            });
                        };
                        button(BEGIN, "Play", focus == HandFocus::Begin);
                        if custom {
                            button(CUSTOM, "Custom", focus == HandFocus::Custom);
                        }
                    });
            });
        })
        .id()
}

fn spawn_quit(commands: &mut Commands, pen: &Pen, art: &CardArt, focus: QuitFocus) -> Entity {
    let a = art.prompt.as_ref();
    commands
        .spawn(full_screen("quit", CARD_Z))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(32),
                    padding: UiRect::axes(px(64), px(40)),
                    border_radius: BorderRadius::all(px(32)),
                    ..default()
                },
                BackgroundColor(BOX_FILL),
            ))
            .with_children(|panel| {
                label(panel, pen, a, "Quit this game?", 56.0);
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(32),
                        ..default()
                    })
                    .with_children(|row| {
                        for (text, on, off, me) in [
                            (
                                "Yes",
                                crate::yes_no::YES_ON,
                                crate::yes_no::YES_OFF,
                                QuitFocus::Yes,
                            ),
                            (
                                "No",
                                crate::yes_no::NO_ON,
                                crate::yes_no::NO_OFF,
                                QuitFocus::No,
                            ),
                        ] {
                            let focused = focus == me;
                            let piece =
                                a.and_then(|a| a.pieces.get(if focused { on } else { off }));
                            match piece {
                                Some(p) => {
                                    row.spawn(button_art(p)).with_children(|t| {
                                        if let Some(a) = a {
                                            bitmap_text(
                                                t,
                                                a,
                                                text,
                                                LABEL_SIZE,
                                                Color::srgb(0.03, 0.05, 0.16),
                                            );
                                        }
                                    });
                                }
                                None => {
                                    row.spawn(ring(focused))
                                        .with_children(|b| label(b, pen, a, text, LABEL_SIZE));
                                }
                            }
                        }
                    });
            });
        })
        .id()
}

/// The loot's words where a string didn't read (no interface model).
const LOOT_UNREAD: &str = "You won!";

/// A pirate's loot over the whole picture (openspec AS.673; stand-in
/// ledger `card-loot`): a backdrop the board's camera left on nothing is
/// hidden under, the reward card's title, the stone given in its own
/// picture with its name, the coins' title, and the prompt to go on, the
/// walking pad's bottom face as every prompt of the TV's draws it.
fn spawn_loot(
    commands: &mut Commands,
    pen: &Pen,
    art: &CardArt,
    loot: &LootShown,
    labels: crate::pad::Labels,
) -> Entity {
    let theme = &pen.theme;
    let a = art.prompt.as_ref();
    let picture = loot.stone.and_then(|n| {
        art.hand
            .as_ref()
            .and_then(|h| h.pieces.get(stone_sprite(n).as_str()))
    });
    commands
        .spawn((
            full_screen("loot", CARD_Z),
            BackgroundColor(Color::srgba(0.01, 0.02, 0.08, 0.97)),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(28),
                    padding: UiRect::axes(px(72), px(44)),
                    border: UiRect::all(px(3)),
                    border_radius: BorderRadius::all(px(32)),
                    ..default()
                },
                BackgroundColor(BOX_FILL),
                BorderColor::all(theme.gold),
            ))
            .with_children(|panel| {
                let title = loot
                    .card_title
                    .as_deref()
                    .or(loot.coins_title.as_deref())
                    .unwrap_or(LOOT_UNREAD);
                label(panel, pen, a, title, 56.0);
                if let Some(p) = picture {
                    panel.spawn(stone_picture(p, 2.0));
                }
                if let Some(name) = &loot.stone_name {
                    label(panel, pen, a, name, LABEL_SIZE);
                }
                if let (Some(coins), true) = (&loot.coins_title, loot.card_title.is_some()) {
                    label(panel, pen, a, coins, LABEL_SIZE);
                }
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: px(16),
                        ..default()
                    })
                    .with_children(|line| {
                        crate::pause::glyph(
                            line,
                            pen,
                            crate::pad::prompt(labels, crate::pad::Face::South),
                        );
                        label(line, pen, a, crate::collected_box::CONTINUE, LABEL_SIZE);
                    });
            });
        })
        .id()
}

/// The strip's choices: each its pad face and our word (openspec AS.875,
/// AS.879).
pub(crate) fn choices(strip: Strip) -> &'static [(crate::pad::Face, &'static str)] {
    match strip {
        Strip::Lost => &[
            (crate::pad::Face::South, PLAY_AGAIN),
            (crate::pad::Face::East, LEAVE),
        ],
        Strip::Won | Strip::Tutorial => {
            &[(crate::pad::Face::South, crate::collected_box::CONTINUE)]
        }
    }
}

/// The strip at the bottom over the board (openspec AS.875, AS.879): each
/// choice its pad glyph and our word; at a tutorial stop its line (the
/// disc's words, read at run time) in a column at the left, in the TV's own
/// type, clear of the board and the stones the stop shows (openspec
/// AS.877).
fn spawn_over(
    commands: &mut Commands,
    pen: &Pen,
    art: &CardArt,
    strip: Option<Strip>,
    line: Option<&str>,
    labels: crate::pad::Labels,
) -> Entity {
    let a = art.prompt.as_ref();
    let choices = strip.map_or(&[][..], choices);
    let root = commands
        .spawn((
            CardScreenRoot("over"),
            // The whole screen, so the strip (at the bottom) and the line (at
            // the left) each sit where they're put.
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                ..default()
            },
            GlobalZIndex(SCORE_Z),
        ))
        .with_children(|screen| {
            if choices.is_empty() {
                return;
            }
            let mut row = screen.spawn(Node {
                position_type: PositionType::Absolute,
                left: px(0),
                bottom: px(48),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            });
            row.with_children(|root| {
                root.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: px(64),
                        padding: UiRect::axes(px(48), px(20)),
                        border: UiRect::all(px(3)),
                        border_radius: BorderRadius::all(px(28)),
                        ..default()
                    },
                    BackgroundColor(BOX_FILL),
                    BorderColor::all(pen.theme.gold),
                ))
                .with_children(|strip| {
                    for (face, word) in choices {
                        strip
                            .spawn(Node {
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                column_gap: px(16),
                                ..default()
                            })
                            .with_children(|line| {
                                crate::pause::glyph(line, pen, crate::pad::prompt(labels, *face));
                                label(line, pen, a, word, LABEL_SIZE);
                            });
                    }
                });
            });
        })
        .id();
    if let Some(line) = line {
        let boxed = commands
            .spawn((
                CardScreenRoot("tutorial line"),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(TUTORIAL_LEFT),
                    top: px(TUTORIAL_TOP),
                    ..default()
                },
            ))
            .with_children(|top| {
                top.spawn((
                    Node {
                        width: px(TUTORIAL_WIDTH),
                        padding: UiRect::axes(px(28), px(24)),
                        border: UiRect::all(px(3)),
                        border_radius: BorderRadius::all(px(28)),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(BOX_FILL),
                    BorderColor::all(pen.theme.gold),
                ))
                .with_children(|b| {
                    b.spawn(pen.text(line, Step::Md, Role::Primary));
                });
            })
            .id();
        commands.entity(root).add_child(boxed);
    }
    root
}

fn spawn_score(
    commands: &mut Commands,
    pen: &Pen,
    art: &CardArt,
    (me, them): (i32, i32),
) -> Entity {
    let a = art.score.as_ref();
    commands
        .spawn((
            CardScreenRoot("score"),
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(24),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            },
            GlobalZIndex(SCORE_Z),
        ))
        .with_children(|root| {
            root.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(240),
                ..default()
            })
            .with_children(|row| {
                // The scripts tint each side's box; ours: blue the player's,
                // red the opponent's, the stones' own frame colours.
                for (n, tint) in [(me, PLAYER_TINT), (them, OPPONENT_TINT)] {
                    let tile = a.and_then(|a| a.pieces.get(SCORE_TILE));
                    let cap = a.and_then(|a| a.pieces.get(SCORE_CAP));
                    row.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|bx| {
                        let h = cap.map_or(88.0, |c| c.size.y * UNIT);
                        if let Some(c) = cap {
                            bx.spawn(score_cap(c, h, tint, false));
                        }
                        let mut mid = bx.spawn(Node {
                            min_width: px(96),
                            height: px(h),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        });
                        match tile {
                            Some(t) => {
                                mid.insert(ImageNode {
                                    image_mode: NodeImageMode::Stretch,
                                    color: tint,
                                    ..upright_image(t.image.clone())
                                });
                            }
                            None => {
                                mid.insert(BackgroundColor(BOX_FILL));
                            }
                        }
                        mid.with_children(|m| {
                            label(m, pen, art.hand.as_ref().or(a), &n.to_string(), SCORE_SIZE)
                        });
                        if let Some(c) = cap {
                            bx.spawn(score_cap(c, h, tint, true));
                        }
                    });
                }
            });
        })
        .id()
}

/// The card screens as `/api/_test/tv` reports them.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CardScreensShot {
    /// The hand select up: the hand shown, the recommended, the collection
    /// and the focus.
    pub hand: Option<HandShot>,
    /// The score header's numbers, the player's first, while a board is up.
    pub score: Option<(i32, i32)>,
    pub board: bool,
    /// The board's pick state as the manager holds it.
    pub pick: Option<card_game::BoardRead>,
    /// The quit confirm up, and its focus.
    pub quit: Option<QuitFocus>,
    /// A pirate's loot up, as drawn (openspec AS.673).
    pub loot: Option<LootShown>,
    /// A tutorial stop's line as read off the disc (openspec AS.877).
    pub tutorial: Option<String>,
    /// What the screens answered this session, in order.
    pub answered: Vec<String>,
    /// Which screens are drawn now, by name.
    pub drawn: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HandShot {
    pub hand: Vec<u16>,
    pub recommended: Vec<u16>,
    pub collection: Vec<u16>,
    pub focus: HandFocus,
    pub custom: bool,
}

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct CardShots<'w, 's> {
    screens: Option<Res<'w, CardScreens>>,
    read: Option<Res<'w, CardRead>>,
    roots: Query<'w, 's, &'static CardScreenRoot>,
}

impl CardShots<'_, '_> {
    /// None until the run has played a card game.
    pub(crate) fn shot(&self) -> Option<CardScreensShot> {
        let (screens, read) = (self.screens.as_ref()?, self.read.as_ref()?);
        let shot = CardScreensShot {
            hand: screens.hand.as_ref().map(|e| HandShot {
                hand: e.hand.clone(),
                recommended: e.offer.recommended.clone(),
                collection: e.offer.collection.clone(),
                focus: e.focus,
                custom: e.can_custom(),
            }),
            score: read.score.filter(|_| read.board),
            board: read.board,
            pick: read.pick.clone(),
            quit: screens.quit,
            loot: read.loot.clone(),
            tutorial: read.tutorial.clone(),
            answered: screens.answered.clone(),
            drawn: self.roots.iter().map(|r| r.0).collect(),
        };
        (shot != CardScreensShot::default()).then_some(shot)
    }
}

/// A button's disc art at its template's size, its label laid over it, the
/// right way up (openspec AS.581): the hand select's Play and Custom, the
/// quit confirm's two.
fn button_art(p: &crate::completion::Piece) -> (Node, ImageNode) {
    (
        Node {
            width: px(p.size.x * UNIT),
            height: px(p.size.y * UNIT),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        upright_image(p.image.clone()),
    )
}

/// A score box's end cap `h` high in its owner's `tint`, upright (openspec
/// AS.581), the right one `mirrored` across.
fn score_cap(
    c: &crate::completion::Piece,
    h: f32,
    tint: Color,
    mirrored: bool,
) -> (Node, ImageNode) {
    (
        Node {
            width: px(c.size.x * UNIT),
            height: px(h),
            ..default()
        },
        ImageNode {
            flip_x: mirrored,
            color: tint,
            ..upright_image(c.image.clone())
        },
    )
}

/// A stone's hand-select picture at `scale` times its template's size, the
/// right way up (openspec AS.581: the interface's images run bottom row
/// first): the hand select's slots and the loot panel's stone.
fn stone_picture(p: &crate::completion::Piece, scale: f32) -> (Node, ImageNode) {
    (
        Node {
            width: px(p.size.x * UNIT * scale),
            height: px(p.size.y * UNIT * scale),
            ..default()
        },
        upright_image(p.image.clone()),
    )
}

#[cfg(test)]
mod tests;
