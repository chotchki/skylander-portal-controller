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
