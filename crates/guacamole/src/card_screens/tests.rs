use super::*;

fn offer(recommended: &[u16], collection: &[u16]) -> HandOffer {
    HandOffer {
        recommended: recommended.to_vec(),
        collection: collection.to_vec(),
        tutorial: false,
    }
}

const STARTERS: [u16; 6] = [0, 12, 15, 29, 46, 53];

/// openspec AS.555: Play on the hand as offered plays the recommended hand,
/// and back on Play backs out of the page.
#[test]
fn play_on_the_offer_is_the_recommended_hand_and_back_backs_out() {
    let mut e = HandEdit::new(offer(&[0, 12, 15, 29, 46], &STARTERS));
    assert_eq!(e.focus, HandFocus::Begin);
    assert_eq!(e.clone().input(TvInput::Back), Some(HandOutcome::BackOut));
    assert_eq!(
        e.input(TvInput::Activate),
        Some(HandOutcome::Play(HandChoice::Recommended))
    );
}

/// openspec AS.555: a slot changed through Custom plays as the player's own
/// hand, its new stone one the hand didn't hold, never a stone twice.
#[test]
fn a_slot_changed_plays_as_a_custom_hand_never_holding_a_stone_twice() {
    let mut e = HandEdit::new(offer(&[0, 12, 15, 29, 46], &STARTERS));
    assert!(e.can_custom());
    assert_eq!(e.input(TvInput::Move(CompassOctant::East)), None);
    assert_eq!(e.focus, HandFocus::Custom);
    assert_eq!(e.input(TvInput::Activate), None);
    assert_eq!(e.focus, HandFocus::Slot(0));
    // Slot 0's stone 0 forward: 12, 15, 29 and 46 are held, so 53.
    e.input(TvInput::Move(CompassOctant::North));
    assert_eq!(e.hand, [53, 12, 15, 29, 46]);
    // And back: past the held ones to 0 again.
    e.input(TvInput::Move(CompassOctant::South));
    assert_eq!(e.hand, [0, 12, 15, 29, 46]);
    e.input(TvInput::Move(CompassOctant::North));
    e.input(TvInput::Move(CompassOctant::East));
    assert_eq!(e.focus, HandFocus::Slot(1));
    e.input(TvInput::Activate);
    assert_eq!(e.focus, HandFocus::Begin);
    let out = e.input(TvInput::Activate);
    assert_eq!(
        out,
        Some(HandOutcome::Play(HandChoice::Custom(vec![
            53, 12, 15, 29, 46
        ])))
    );
    if let Some(HandOutcome::Play(HandChoice::Custom(h))) = out {
        assert!(card_game::hand_takes(&h, &STARTERS));
    }
}

/// openspec AS.555: with fewer than five stones collected (a first game's
/// five of stone 0) Custom isn't offered and the focus stays on Play.
#[test]
fn custom_isnt_offered_under_five_stones() {
    let mut e = HandEdit::new(offer(&[0, 0, 0, 0, 0], &[]));
    assert!(!e.can_custom());
    e.input(TvInput::Move(CompassOctant::East));
    assert_eq!(e.focus, HandFocus::Begin);
    assert_eq!(
        e.input(TvInput::Activate),
        Some(HandOutcome::Play(HandChoice::Recommended))
    );
}

/// openspec AS.555: the build-hand page's own rule, a hand of five
/// collected stones, none twice.
#[test]
fn a_custom_hand_takes_five_collected_stones_none_twice() {
    assert!(card_game::hand_takes(&[0, 12, 15, 29, 46], &STARTERS));
    assert!(!card_game::hand_takes(&[0, 0, 15, 29, 46], &STARTERS));
    assert!(!card_game::hand_takes(&[0, 12, 15, 29, 47], &STARTERS));
    assert!(!card_game::hand_takes(&[0, 12, 15, 29], &STARTERS));
}

/// openspec AS.555: a kid who presses nothing gets the hand played after
/// [`IDLE_PLAY`], not before.
#[test]
fn an_unpressed_hand_select_plays_after_the_idle_wait() {
    let open = Duration::from_secs(100);
    assert!(!idle_due(open, open + IDLE_PLAY - Duration::from_millis(1)));
    assert!(idle_due(open, open + IDLE_PLAY));
    assert!(!idle_due(open, open));
}

/// openspec AS.557: Start over a board brings the confirm up on No; it
/// stays while both hold, and goes once the level carries on or the board
/// does; Start with no board brings nothing.
#[test]
fn the_quit_confirm_comes_with_start_over_a_board_on_no() {
    assert_eq!(quit_shown(None, true, true), Some(QuitFocus::No));
    assert_eq!(
        quit_shown(Some(QuitFocus::Yes), true, true),
        Some(QuitFocus::Yes)
    );
    assert_eq!(quit_shown(Some(QuitFocus::Yes), true, false), None);
    assert_eq!(quit_shown(Some(QuitFocus::No), false, true), None);
    assert_eq!(quit_shown(None, false, true), None);
}

/// openspec AS.930: the quit is asked by the board player's own Start (the
/// walking pad's) alone: a second pad's Start over the board pauses the
/// level and asks no quit of the first kid's game; no pause, or the walking
/// pad gone (the controller message), asks none.
#[test]
fn only_the_walking_pads_start_asks_the_quit() {
    use crate::pad::Pause;
    assert!(asks_quit(Some(Pause::Start { by: 1 }), Some(1)));
    assert!(
        !asks_quit(Some(Pause::Start { by: 2 }), Some(1)),
        "another pad's"
    );
    assert!(!asks_quit(Some(Pause::Start { by: 1 }), None), "no walker");
    assert!(!asks_quit(Some(Pause::<u8>::NoPad), Some(1)));
    assert!(!asks_quit(None, Some(1u8)));
}

/// openspec AS.581: a stone's picture, in the hand select's slots and on the
/// loot panel, draws its interface image the right way up (the interface's
/// images run bottom row first), at its template's size times the scale.
#[test]
fn a_stones_picture_draws_upright_at_its_size() {
    let p = crate::completion::Piece {
        image: Handle::default(),
        size: Vec2::new(40.0, 50.0),
    };
    for scale in [1.0, 2.0] {
        let (node, image) = stone_picture(&p, scale);
        assert!(image.flip_y, "upright at {scale}");
        assert_eq!(node.width, px(40.0 * UNIT * scale));
        assert_eq!(node.height, px(50.0 * UNIT * scale));
    }
}

/// openspec AS.581: the buttons' disc art and the score boxes' caps draw
/// upright, the right cap mirrored across as well.
#[test]
fn the_buttons_and_score_caps_draw_upright() {
    let p = crate::completion::Piece {
        image: Handle::default(),
        size: Vec2::new(120.0, 30.0),
    };
    let (node, image) = button_art(&p);
    assert!(image.flip_y, "a button upright");
    assert_eq!(node.width, px(120.0 * UNIT));
    for mirrored in [false, true] {
        let (_, cap) = score_cap(&p, 88.0, Color::WHITE, mirrored);
        assert!(cap.flip_y, "a cap upright");
        assert_eq!(cap.flip_x, mirrored);
    }
}

/// openspec AS.875, AS.879: the strip asks over a finished board and at a
/// tutorial stop that waits on the accept: after a loss its two choices (A
/// again, B leave), after a win and at each such stop A alone; nothing
/// mid-game, at a timed stop or with no board.
#[test]
fn the_end_of_game_strip_asks_only_over_a_finished_board() {
    use crate::pad::Face;
    assert_eq!(game_over(Some("lost")), Some(Strip::Lost));
    assert_eq!(game_over(Some("won")), Some(Strip::Won));
    // The tutorial's stops by the names its manager gives them.
    for b in [
        "wait for tutorial - stone intro",
        "wait for tutorial - board intro",
        "wait for tutorial - goal of game",
        "wait for tutorial - opponent plays first",
        "wait for tutorial - explain strengths",
        "tutorial 2 - explain board blockers",
        "tutorial 3 - explain elemental spaces",
    ] {
        assert_eq!(game_over(Some(b)), Some(Strip::Tutorial), "{b}");
    }
    let faces = |s| choices(s).iter().map(|(f, _)| *f).collect::<Vec<Face>>();
    assert_eq!(faces(Strip::Lost), [Face::South, Face::East]);
    assert_eq!(faces(Strip::Won), [Face::South]);
    assert_eq!(faces(Strip::Tutorial), [Face::South]);
    for b in [
        None,
        Some("player turn - wait for player card selection"),
        Some("check for winner"),
        Some("delay after updating score header"),
        Some("tutorial setup"),
        // The two stops that go on by themselves ask nothing (as-cards-first).
        Some("wait for tutorial - wait for turn intro"),
        Some("wait for tutorial - wait for hand explanation"),
    ] {
        assert_eq!(game_over(b), None, "{b:?}");
    }
    assert_ne!(PLAY_AGAIN, LEAVE);
}

/// An app over the screens' `show`, the TV's pen with no disc art.
fn show_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::image::ImagePlugin::default(),
    ));
    let stars =
        crate::starfield::Sprites::new(&mut app.world_mut().resource_mut::<Assets<Image>>());
    app.insert_resource(TvAssets {
        font: Handle::default(),
        qr: None,
        stars,
    })
    .insert_resource(TvTheme::default())
    .init_resource::<CardScreens>()
    .init_resource::<CardRead>()
    .add_message::<Redrawn>()
    .add_systems(Update, show);
    app
}

/// The board's read at `behaviour` with the tutorial's line `line` up.
fn at_stop(app: &mut App, behaviour: &str, line: Option<&str>) {
    let mut read = app.world_mut().resource_mut::<CardRead>();
    read.board = true;
    read.pick = Some(card_game::BoardRead {
        behaviour: Some(behaviour.to_owned()),
        ..Default::default()
    });
    read.tutorial = line.map(str::to_owned);
}

/// openspec AS.877: a tutorial stop's line is drawn on the screen's own
/// root at the left, clear of the board (as-cards-first's f4: a child of the
/// strip's root at the bottom put it under the screen); a timed stop draws
/// the line with no strip; no line, no box.
#[test]
fn a_tutorial_stops_line_draws_at_the_screens_left() {
    let mut app = show_app();
    at_stop(
        &mut app,
        "wait for tutorial - board intro",
        Some("Stones go on squares"),
    );
    app.update();
    let world = app.world_mut();
    let mut q = world.query::<(Entity, &CardScreenRoot, &Node, Option<&ChildOf>)>();
    let all: Vec<(Entity, &'static str, Node, Option<Entity>)> = q
        .iter(world)
        .map(|(e, r, n, p)| (e, r.0, n.clone(), p.map(ChildOf::parent)))
        .collect();
    let over = all
        .iter()
        .find(|r| r.1 == "over")
        .expect("the strip's root");
    assert_eq!(
        (over.2.top, over.2.height),
        (px(0), percent(100)),
        "the whole screen"
    );
    let line = all
        .iter()
        .find(|r| r.1 == "tutorial line")
        .expect("the line's box");
    assert_eq!(line.3, Some(over.0), "on the screen's root");
    assert_eq!(
        (line.2.left, line.2.top),
        (px(TUTORIAL_LEFT), px(TUTORIAL_TOP))
    );
    let mut texts = world.query::<&Text>();
    assert!(
        texts.iter(world).any(|t| t.0 == "Stones go on squares"),
        "its words"
    );
    // A timed stop: the line, no strip's choices.
    at_stop(
        &mut app,
        "wait for tutorial - wait for hand explanation",
        Some("Pick one"),
    );
    app.update();
    let world = app.world_mut();
    let mut texts = world.query::<&Text>();
    let words: Vec<String> = texts.iter(world).map(|t| t.0.clone()).collect();
    assert!(words.iter().any(|w| w == "Pick one"), "{words:?}");
    assert!(
        !words.iter().any(|w| w == crate::collected_box::CONTINUE),
        "{words:?}"
    );
    // Mid-turn with no box up: nothing.
    at_stop(
        &mut app,
        "player turn - wait for player card selection",
        None,
    );
    app.update();
    let world = app.world_mut();
    let mut roots = world.query::<&CardScreenRoot>();
    assert!(
        !roots
            .iter(world)
            .any(|r| r.0 == "tutorial line" || r.0 == "over"),
        "nothing drawn"
    );
}
