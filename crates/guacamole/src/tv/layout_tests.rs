use skylander_core::SlotIndex;

use super::*;
use crate::driver::PortalChange;
use crate::test_support::tv::{outer_nodes, placement, portal_app, scale_of, tv_camera};

struct Laid {
    cards: Vec<(u8, Rect)>,
    empties: Vec<(u8, Rect)>,
    pips: Vec<(u8, Rect)>,
    texts: Vec<Rect>,
}

fn lay_out(width: u32, height: u32, mode: WindowMode, cards: PortalCards) -> Laid {
    let mut app = portal_app(width, height, mode, cards);
    measure(&mut app)
}

/// The layout at rest, as a shot sees it: the cards' motion at its end
/// (`ReducedMotion`), so a card mid-rise doesn't carry its lines across
/// its box. The motion's own tests measure it moving.
fn measure(app: &mut App) -> Laid {
    app.insert_resource(ReducedMotion(true));
    for _ in 0..3 {
        app.update();
    }
    let world = app.world_mut();
    let mut q = world.query::<(&CardSlot, &ComputedNode, &UiGlobalTransform)>();
    let mut cards: Vec<_> = q
        .iter(world)
        .map(|(s, n, t)| (s.0, node_rect(n, t)))
        .collect();
    cards.sort_by_key(|c| c.0);
    let mut q = world.query::<(&EmptySlot, &ComputedNode, &UiGlobalTransform)>();
    let mut empties: Vec<_> = q
        .iter(world)
        .map(|(s, n, t)| (s.0, node_rect(n, t)))
        .collect();
    empties.sort_by_key(|c| c.0);
    let mut q = world.query::<(&Pip, &ComputedNode, &UiGlobalTransform)>();
    let pips = q
        .iter(world)
        .map(|(s, n, t)| (s.0, node_rect(n, t)))
        .collect();
    // The TV's text. A title's ink lays out on the title's own camera,
    // and nothing else may: a line that lost the TV's camera would slip
    // past every check that reads these.
    let tv = tv_camera(world);
    let mut q = world.query_filtered::<(
        &ComputedNode,
        &UiGlobalTransform,
        &bevy::ui::ComputedUiTargetCamera,
        Option<&ChildOf>,
    ), With<Text>>();
    let mut texts = Vec::new();
    let mut off_tv = Vec::new();
    for (node, at, camera, parent) in q.iter(world) {
        if camera.get() == Some(tv) {
            texts.push(node_rect(node, at));
        } else {
            off_tv.push(parent.map(ChildOf::parent));
        }
    }
    for parent in off_tv {
        assert!(
            parent.is_some_and(|p| world.get::<title::TitleInk>(p).is_some()),
            "a text off the TV's camera that isn't a title's ink"
        );
    }
    Laid {
        cards,
        empties,
        pips,
        texts,
    }
}

fn full(name: &str) -> PortalCards {
    PortalCards::default().reduce((0..8).map(|n| PortalChange::Placed {
        slot: SlotIndex::new(n).unwrap(),
        placement: placement(name, Some("#33cc66")),
    }))
}

fn assert_eight_visible(width: u32, height: u32, mode: WindowMode, name: &str) {
    assert_eight_visible_under(width, height, mode, name, None);
}

fn assert_eight_visible_under(
    width: u32,
    height: u32,
    mode: WindowMode,
    name: &str,
    band: Option<Band>,
) {
    let mut app = portal_app(width, height, mode, full(name));
    app.insert_resource(ExtractBand(band));
    let laid = measure(&mut app);
    let safe = safe_area(width, height, mode);
    assert_eq!(laid.cards.len(), 8, "{width}x{height} {mode:?}");
    assert!(laid.empties.is_empty());
    for (slot, r) in &laid.cards {
        assert!(
            r.width() > 0.0 && r.height() > 0.0,
            "slot {slot} has no size"
        );
        assert!(contains(safe, *r), "slot {slot} {r:?} leaves {safe:?}");
    }
    for (i, (_, a)) in laid.cards.iter().enumerate() {
        for (_, b) in &laid.cards[i + 1..] {
            assert!(a.intersect(*b).is_empty(), "{a:?} overlaps {b:?}");
        }
    }
    // Slot order: two rows of four, left to right, top to bottom.
    for (slot, r) in &laid.cards {
        let (row, col) = (
            *slot as usize / CARDS_PER_ROW,
            *slot as usize % CARDS_PER_ROW,
        );
        if col > 0 {
            let left = laid.cards[*slot as usize - 1].1;
            assert!(left.max.x <= r.min.x && (left.center().y - r.center().y).abs() < 1.0);
        }
        if row > 0 {
            let above = laid.cards[*slot as usize - CARDS_PER_ROW].1;
            assert!(above.max.y <= r.min.y);
        }
    }
    // Every pip sits on its own card; all text stays inside the safe area.
    assert_eq!(laid.pips.len(), 8);
    for (slot, pip) in &laid.pips {
        assert!(contains(laid.cards[*slot as usize].1, *pip), "pip {slot}");
    }
    // No two lines of text overlap (the header's title and join address).
    for (i, a) in laid.texts.iter().enumerate() {
        for b in &laid.texts[i + 1..] {
            assert!(a.intersect(*b).is_empty(), "text {a:?} overlaps {b:?}");
        }
    }
    // Text is inside the safe area, and a card's text is inside its card
    // (cards clip, so text crossing an edge would be cut off).
    for t in &laid.texts {
        assert!(contains(safe, *t), "text {t:?} leaves {safe:?}");
        for (slot, card) in &laid.cards {
            let crosses = !card.intersect(*t).is_empty() && !contains(*card, *t);
            assert!(!crosses, "text {t:?} crosses card {slot}'s edge {card:?}");
        }
    }
}

#[test]
fn eight_cards_fit_inside_the_tv_inset_at_1080p_and_4k() {
    assert_eight_visible(1920, 1080, WindowMode::Tv, "Eruptor");
    assert_eight_visible(3840, 2160, WindowMode::Tv, "Eruptor");
}

#[test]
fn eight_cards_fit_the_desktop_window() {
    assert_eight_visible(1280, 720, WindowMode::Desktop, "Eruptor");
}

/// The longest band the TV draws: a failure naming two deep folders.
fn long_failure() -> Band {
    use crate::extraction::{ExtractStatus, Failure, band};
    let deep = "/Users/someone/Library/Application Support/guacamole/extract";
    band(&ExtractStatus::Failed(Failure::InCheckout {
        root: format!("{deep}/and/a/much/deeper/folder/than/anyone/would/use").into(),
        repo: format!("{deep}/and/a/much").into(),
    }))
    .unwrap()
}

#[test]
fn eight_cards_fit_under_the_extraction_band() {
    use crate::extraction::{ExtractStatus, band};
    let progress = band(&ExtractStatus::Converting {
        done: 47,
        total: 48,
        movie: "wreckingball".into(),
    });
    for b in [progress, Some(long_failure())] {
        assert_eight_visible_under(1920, 1080, WindowMode::Tv, "Eruptor", b.clone());
        assert_eight_visible_under(3840, 2160, WindowMode::Tv, "Eruptor", b.clone());
        assert_eight_visible_under(1280, 720, WindowMode::Desktop, "Eruptor", b);
    }
}

#[test]
fn the_band_replaces_the_empty_portal_hint_and_draws_its_bar() {
    use crate::extraction::{ExtractStatus, band};
    let lines = |app: &mut App| {
        let world = app.world_mut();
        let mut q = world.query::<&Text>();
        q.iter(world).map(|t| t.0.clone()).collect::<Vec<_>>()
    };
    let hint = "Put a figure on the portal from your phone.".to_string();
    let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
    measure(&mut app);
    assert!(lines(&mut app).contains(&hint));

    let checking = band(&ExtractStatus::Checking {
        bytes_done: 1,
        bytes_total: 2,
    });
    app.insert_resource(ExtractBand(checking));
    measure(&mut app);
    let drawn = lines(&mut app);
    assert!(!drawn.contains(&hint), "{drawn:?}");
    assert!(drawn.contains(&"Checking your Giants dump… 50%".to_string()));
    let world = app.world_mut();
    let mut bars = world.query::<(&ExtractBar, &ComputedNode)>();
    let (bar, node) = bars.single(world).unwrap();
    assert_eq!(bar.0, 50);
    assert!(node.size().x > 0.0);

    // Ready draws nothing: the hint is back.
    app.insert_resource(ExtractBand(None));
    measure(&mut app);
    assert!(lines(&mut app).contains(&hint));
}

#[test]
fn long_names_wrap_inside_their_cards() {
    assert_eight_visible(1920, 1080, WindowMode::Tv, "Legendary Chop Chop the Undead");
}

#[test]
fn a_removed_card_leaves_the_others_where_they_were() {
    let before = lay_out(1920, 1080, WindowMode::Tv, full("Eruptor"));
    let cleared = full("Eruptor").reduce([PortalChange::Cleared {
        slot: SlotIndex::new(2).unwrap(),
    }]);
    let after = lay_out(1920, 1080, WindowMode::Tv, cleared);
    assert_eq!(after.cards.len(), 7);
    assert_eq!(after.empties.len(), 1);
    assert_eq!(after.empties[0].0, 2);
    for (slot, r) in &after.cards {
        assert_eq!(before.cards[*slot as usize].1, *r, "slot {slot} moved");
    }
    assert_eq!(before.cards[2].1, after.empties[0].1);
}

#[test]
fn the_screen_follows_portal_cards() {
    let one = PortalCards::default().reduce([PortalChange::Placed {
        slot: SlotIndex::new(0).unwrap(),
        placement: placement("Eruptor", Some("#33cc66")),
    }]);
    let mut app = portal_app(1920, 1080, WindowMode::Tv, one.clone());
    assert_eq!(measure(&mut app).cards.len(), 1);
    // A Kaos swap in slot 1 and a placement in slot 6.
    let next = one.reduce([
        PortalChange::Cleared {
            slot: SlotIndex::new(0).unwrap(),
        },
        PortalChange::Placed {
            slot: SlotIndex::new(0).unwrap(),
            placement: placement("Spyro", Some("#33cc66")),
        },
        PortalChange::Placed {
            slot: SlotIndex::new(5).unwrap(),
            placement: placement("Tree Rex", None),
        },
    ]);
    *app.world_mut().resource_mut::<PortalCards>() = next;
    let laid = measure(&mut app);
    let slots: Vec<u8> = laid.cards.iter().map(|c| c.0).collect();
    assert_eq!(slots, [0, 5]);
    assert_eq!(laid.pips.len(), 1);
    let world = app.world_mut();
    let mut names = world.query::<&Text>();
    let names: Vec<String> = names.iter(world).map(|t| t.0.clone()).collect();
    assert!(names.contains(&"Spyro".to_string()) && names.contains(&"Tree Rex".to_string()));
    assert!(!names.contains(&"Eruptor".to_string()));
    // A quit clears the game: back to the join code, no cards.
    app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
    let laid = measure(&mut app);
    assert!(laid.cards.is_empty() && laid.empties.is_empty());
    assert_eq!(app.world().resource::<LayoutReport>().0, 0);
}

#[test]
fn the_join_screen_stays_inside_the_tv_inset() {
    for (w, h) in [(1920, 1080), (3840, 2160)] {
        let mut app = portal_app(w, h, WindowMode::Tv, PortalCards::default());
        {
            let mut current = app.world_mut().resource_mut::<Current>();
            current.screen = TvScreen::Join;
            current.clients = 1;
        }
        let laid = measure(&mut app);
        let safe = safe_area(w, h, WindowMode::Tv);
        assert!(laid.cards.is_empty());
        for t in &laid.texts {
            assert!(contains(safe, *t), "text {t:?} leaves {safe:?}");
        }
        let world = app.world_mut();
        let mut q = world.query::<&Text>();
        let lines: Vec<String> = q.iter(world).map(|t| t.0.clone()).collect();
        assert!(
            lines.contains(&"1 phone connected".to_string()),
            "{lines:?}"
        );
        assert!(lines.contains(&"http://christophers-macbook-pro.local:8765".to_string()));
    }
}

/// AA.21: a nested startup error reaches the screen whole, wrapped
/// inside the inset, including a deep path and a SHA-384 checksum
/// (96 hex digits, nowhere to break between words).
#[test]
fn a_server_error_draws_every_cause_wrapped_inside_the_inset() {
    let deep = "/Users/someone/Library/Application Support/guacamole/and/a/much/deeper/folder/than/anyone/would/use/db.sqlite";
    let causes = [
        "open profile store".to_string(),
        format!("open sqlite at {deep}"),
        format!(
            "migration 20261017000000 checksum {} does not match",
            "0123456789abcdef".repeat(6)
        ),
    ];
    // Formatted as boot formats a fatal error: the scope, then `{:#}`.
    let chain = anyhow::anyhow!("{}", causes[2]).context(causes[1].clone());
    let message = format!("{}: {chain:#}", causes[0]);
    for (w, h, mode) in [
        (1920, 1080, WindowMode::Tv),
        (3840, 2160, WindowMode::Tv),
        (1280, 720, WindowMode::Desktop),
    ] {
        let mut app = portal_app(w, h, mode, PortalCards::default());
        let status = LauncherStatus {
            screen: guac_server::state::LauncherScreen::ServerError {
                message: message.clone(),
            },
            ..Default::default()
        };
        app.world_mut().resource_mut::<Current>().screen =
            screen_for(&status, &SetupView::default());
        let laid = measure(&mut app);
        let safe = safe_area(w, h, mode);
        let world = app.world_mut();
        let mut q = world
            .query_filtered::<(&Text, &ComputedNode, &UiGlobalTransform), With<ErrorMessage>>();
        let (drawn, node, t) = q.single(world).unwrap();
        for cause in &causes {
            assert!(
                drawn.0.contains(cause.as_str()),
                "{cause} not in {:?}",
                drawn.0
            );
        }
        let rect = node_rect(node, t);
        let line = TvTheme::default().size(Step::Body) * h as f32 / DESIGN_HEIGHT;
        assert!(rect.height() > 2.0 * line, "{w}x{h}: one line, {rect:?}");
        assert!(contains(safe, rect), "{w}x{h}: {rect:?} leaves {safe:?}");
        // The title, the message, the Close button's label and the hint.
        assert_eq!(laid.texts.len(), 4);
        for (i, a) in laid.texts.iter().enumerate() {
            assert!(contains(safe, *a), "{w}x{h}: {a:?} leaves {safe:?}");
            for b in &laid.texts[i + 1..] {
                assert!(a.intersect(*b).is_empty(), "{a:?} overlaps {b:?}");
            }
        }
    }
}

/// Every drawn `Text` under the screen's root: its size, colour and rect.
fn drawn_text(app: &mut App) -> Vec<(String, FontSize, Color, Rect)> {
    let world = app.world_mut();
    let mut q = world.query::<(
        Entity,
        &Text,
        &TextFont,
        &TextColor,
        &ComputedNode,
        &UiGlobalTransform,
    )>();
    let mut parents = world.query::<&ChildOf>();
    let mut drawn = Vec::new();
    for (entity, text, font, color, node, at) in q.iter(world) {
        let mut top = entity;
        while let Ok(parent) = parents.get(world, top) {
            top = parent.parent();
        }
        if world.get::<ScreenRoot>(top).is_some() {
            drawn.push((text.0.clone(), font.font_size, color.0, node_rect(node, at)));
        }
    }
    drawn
}

/// AP.9: every screen, with the most lines each can draw, and the portal
/// full under a band and empty with its hint, draws its text on the TV
/// scale (nothing under 24 design px, nothing between steps) in a role's
/// colour or a pip's ink, inside the inset with no two lines overlapping,
/// at 1080p and 4K on the TV and in the desktop window.
#[test]
fn every_screen_draws_on_the_type_scale_inside_the_inset() {
    use crate::extraction::{ExtractStatus, band};
    let theme = TvTheme::default();
    let folders = vec![
        "/Volumes/dumps".to_string(),
        "/Users/someone/ps3".to_string(),
    ];
    let titles = vec![
        "Skylanders: SWAP Force (BLUS31076)".to_string(),
        "Skylanders: Trap Team (BLUS31442)".to_string(),
        "Skylanders: SuperChargers (BLUS31545)".to_string(),
    ];
    let error = Some("/Volumes/gone isn't a folder".to_string());
    let setup = |f: fn(&mut SetupView)| {
        let mut view = SetupView::default();
        f(&mut view);
        view
    };
    let no_giants = SetupView {
        folders: folders.clone(),
        found_titles: titles.clone(),
        problems: vec!["Couldn't read /Volumes/dumps/BLUS31442: permission denied".into()],
        error: error.clone(),
        ..Default::default()
    };
    let giants = || TvScreen::Portal {
        game: "Skylanders: Giants".into(),
    };
    let progress = band(&ExtractStatus::Converting {
        done: 12,
        total: 48,
        movie: "eruptor".into(),
    });
    let server_error = TvScreen::ServerError {
        message: "open profile store: open sqlite at /Users/someone/Library/Application Support/guacamole/db.sqlite: unable to open database file".into(),
    };
    let cases: Vec<(&str, TvScreen, SetupView, PortalCards, Option<Band>)> = vec![
        (
            "starting",
            TvScreen::Starting,
            SetupView::default(),
            PortalCards::default(),
            None,
        ),
        (
            "setup",
            TvScreen::Setup,
            setup(|v| v.needs_setup = true),
            PortalCards::default(),
            None,
        ),
        (
            "setup, a failed pick",
            TvScreen::Setup,
            SetupView {
                error: error.clone(),
                ..Default::default()
            },
            PortalCards::default(),
            None,
        ),
        (
            "setup, picking",
            TvScreen::Setup,
            setup(|v| v.picking = true),
            PortalCards::default(),
            None,
        ),
        (
            "no giants",
            TvScreen::NoGiants,
            no_giants.clone(),
            PortalCards::default(),
            None,
        ),
        (
            "no giants, nothing there",
            TvScreen::NoGiants,
            SetupView {
                folders: folders.clone(),
                ..Default::default()
            },
            PortalCards::default(),
            None,
        ),
        (
            "found",
            TvScreen::Found,
            SetupView {
                found_titles: titles.clone(),
                show_found: true,
                giants_found: true,
                ..Default::default()
            },
            PortalCards::default(),
            None,
        ),
        (
            "join",
            TvScreen::Join,
            SetupView::default(),
            PortalCards::default(),
            None,
        ),
        (
            "portal, empty",
            giants(),
            SetupView::default(),
            PortalCards::default(),
            None,
        ),
        (
            "portal, eight under a band",
            giants(),
            SetupView::default(),
            full("Legendary Chop Chop the Undead"),
            progress,
        ),
        (
            "portal, eight under a failure",
            giants(),
            SetupView::default(),
            full("Eruptor"),
            Some(long_failure()),
        ),
        (
            "already running",
            TvScreen::AlreadyRunning,
            SetupView::default(),
            PortalCards::default(),
            None,
        ),
        (
            "server error",
            server_error,
            SetupView::default(),
            PortalCards::default(),
            None,
        ),
        (
            "farewell",
            TvScreen::Farewell,
            SetupView::default(),
            PortalCards::default(),
            None,
        ),
    ];
    let roles = theme.text_colors();
    for (w, h, mode) in [
        (1920, 1080, WindowMode::Tv),
        (3840, 2160, WindowMode::Tv),
        (1280, 720, WindowMode::Desktop),
    ] {
        let safe = safe_area(w, h, mode);
        for (what, screen, view, cards, band) in &cases {
            let mut app = portal_app(w, h, mode, cards.clone());
            app.insert_resource(Setup(view.clone()))
                .insert_resource(ExtractBand(band.clone()));
            app.world_mut().resource_mut::<Current>().screen = screen.clone();
            measure(&mut app);
            let drawn = drawn_text(&mut app);
            let at = format!("{what} at {w}x{h}");
            assert!(!drawn.is_empty(), "{at}: no text");
            for (text, size, color, rect) in &drawn {
                let FontSize::Px(px) = *size else {
                    panic!("{at}: {text:?} is {size:?}, not a step");
                };
                assert!(px >= 24.0 && theme.on_scale(px), "{at}: {text:?} at {px}");
                assert!(roles.contains(color), "{at}: {text:?} in {color:?}");
                assert!(
                    contains(safe, *rect),
                    "{at}: {text:?} {rect:?} leaves {safe:?}"
                );
            }
            for (i, (a, _, _, ra)) in drawn.iter().enumerate() {
                for (b, _, _, rb) in &drawn[i + 1..] {
                    assert!(ra.intersect(*rb).is_empty(), "{at}: {a:?} overlaps {b:?}");
                }
            }
        }
    }
}

#[test]
fn a_card_without_an_owner_has_no_pip() {
    let cards = PortalCards::default().reduce([PortalChange::Placed {
        slot: SlotIndex::new(0).unwrap(),
        placement: placement("Eruptor", None),
    }]);
    let laid = lay_out(1920, 1080, WindowMode::Tv, cards);
    assert_eq!(laid.cards.len(), 1);
    assert_eq!(laid.empties.len(), 7);
    assert!(laid.pips.is_empty());
}

/// The screen's roots: (entity, the state it leaves with).
fn roots(app: &mut App) -> Vec<(Entity, TvState)> {
    let world = app.world_mut();
    let mut q = world.query_filtered::<(Entity, &DespawnOnExit<TvState>), With<ScreenRoot>>();
    q.iter(world).map(|(e, s)| (e, s.0)).collect()
}

fn texts(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    let mut q = world.query::<&Text>();
    q.iter(world).map(|t| t.0.clone()).collect()
}

/// AP.1: each step enters its screen and leaves the last one, tree and
/// all; a new game on the portal is no transition.
#[test]
fn each_screen_step_leaves_exactly_one_root() {
    let giants = || TvScreen::Portal {
        game: "Skylanders: Giants".into(),
    };
    let mut app = portal_app(1920, 1080, WindowMode::Tv, full("Eruptor"));
    // Titles in caps (AP.10); "Starting…" isn't one.
    let steps = [
        (TvScreen::Starting, TvState::Starting, "Starting…"),
        (TvScreen::Preparing, TvState::Preparing, "GETTING READY"),
        (
            TvScreen::Setup,
            TvState::Setup,
            "WHERE ARE YOUR DISC DUMPS?",
        ),
        (TvScreen::Found, TvState::Found, "FOUND YOUR GAMES"),
        (TvScreen::Join, TvState::Join, "SCAN TO PLAY"),
        (giants(), TvState::Portal, "SKYLANDERS: GIANTS"),
        (
            TvScreen::Farewell,
            TvState::Farewell,
            "GOODBYE, PORTAL MASTER!",
        ),
    ];
    for (screen, state, line) in steps {
        app.world_mut().resource_mut::<Current>().screen = screen;
        app.update();
        let found = roots(&mut app);
        assert_eq!(found.len(), 1, "{state:?}: {found:?}");
        assert_eq!(found[0].1, state);
        assert_eq!(*app.world().resource::<State<TvState>>().get(), state);
        let drawn = if state == TvState::Starting {
            texts(&mut app)
        } else {
            titles(&mut app)
        };
        assert!(drawn.contains(&line.to_string()), "{state:?}: {drawn:?}");
        let cards = {
            let world = app.world_mut();
            world.query::<&CardSlot>().iter(world).count()
        };
        assert_eq!(cards, if state == TvState::Portal { 8 } else { 0 });
    }

    // Portal to Portal keeps the root and draws the new game.
    app.world_mut().resource_mut::<Current>().screen = giants();
    app.update();
    let before = roots(&mut app);
    app.world_mut().resource_mut::<Current>().screen = TvScreen::Portal {
        game: "Skylanders: Swap Force".into(),
    };
    app.update();
    assert_eq!(roots(&mut app), before);
    let drawn = texts(&mut app);
    assert!(
        drawn.contains(&"SKYLANDERS: SWAP FORCE".to_string()),
        "{drawn:?}"
    );
    assert!(!drawn.contains(&"SKYLANDERS: GIANTS".to_string()));
    assert_eq!(titles(&mut app), ["SKYLANDERS: SWAP FORCE"]);
}

/// The titles' layout `Text`s: what each title draws.
fn titles(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    let mut q = world.query_filtered::<&Text, With<title::TitleText>>();
    q.iter(world).map(|t| t.0.clone()).collect()
}

fn every_screen() -> [TvScreen; 10] {
    [
        TvScreen::Starting,
        TvScreen::Preparing,
        TvScreen::Setup,
        TvScreen::NoGiants,
        TvScreen::Found,
        TvScreen::Join,
        TvScreen::Portal {
            game: "Skylanders: Giants".into(),
        },
        TvScreen::AlreadyRunning,
        TvScreen::ServerError {
            message: "open profile store".into(),
        },
        TvScreen::Farewell,
    ]
}

/// Every UI node, and the parentless ones: a screen root or a stray.
fn nodes(app: &mut App) -> (std::collections::BTreeSet<Entity>, Vec<Entity>) {
    let world = app.world_mut();
    let mut q = world.query_filtered::<(Entity, Has<ChildOf>), With<Node>>();
    let mut all = std::collections::BTreeSet::new();
    let mut parentless = Vec::new();
    for (entity, parented) in q.iter(world) {
        all.insert(entity);
        if !parented {
            parentless.push(entity);
        }
    }
    (all, parentless)
}

/// One root carrying `state` and no other parentless node but its
/// titles' inks, one per title with a camera each; the node count.
fn one_root(app: &mut App, state: TvState, step: &str) -> usize {
    let found = roots(app);
    assert_eq!(found.len(), 1, "{step}: {found:?}");
    assert_eq!(found[0].1, state, "{step}");
    assert_eq!(*app.world().resource::<State<TvState>>().get(), state);
    let (all, parentless) = nodes(app);
    let world = app.world_mut();
    let inks: Vec<Entity> = world
        .query_filtered::<Entity, With<title::TitleInk>>()
        .iter(world)
        .collect();
    let surfaces = world.query::<&title::TitleSurface>().iter(world).count();
    let cameras = world
        .query_filtered::<(), With<title::TitleCamera>>()
        .iter(world)
        .count();
    assert_eq!(
        (inks.len(), cameras),
        (surfaces, surfaces),
        "{step}: a title's ink or camera left behind"
    );
    let strays: Vec<Entity> = parentless
        .into_iter()
        .filter(|e| *e != found[0].0 && !inks.contains(e))
        .collect();
    assert!(strays.is_empty(), "{step}: a stray node {strays:?}");
    all.len()
}

/// AP.1 review: every ordered pair of the nine screens, three times over,
/// leaves exactly the new screen's root and nothing stray beside it, and a
/// screen comes back with the node count it had; `Current` going away and
/// back between two frames is no transition.
#[test]
fn every_pair_of_screens_leaves_one_root_and_no_strays() {
    let mut app = portal_app(1920, 1080, WindowMode::Tv, full("Eruptor"));
    app.world_mut().resource_mut::<Setup>().0.error = Some("gone".into());
    let mut counts: HashMap<TvState, usize> = HashMap::new();
    // Each round ends on the same screen with its title drawn: the
    // cameras, the titles' materials and the images there.
    let mut ends = Vec::new();
    for round in 0..3 {
        for from in every_screen() {
            for to in every_screen() {
                for screen in [from.clone(), to.clone()] {
                    let state = TvState::of(&screen);
                    app.world_mut().resource_mut::<Current>().screen = screen;
                    app.update();
                    let step = format!("round {round}, {from:?} to {to:?}");
                    let count = one_root(&mut app, state, &step);
                    assert_eq!(*counts.entry(state).or_insert(count), count, "{step}");
                }
            }
        }
        frames(&mut app, 3);
        let world = app.world_mut();
        let cameras = world.query::<&Camera>().iter(world).count();
        let materials = world.resource::<Assets<title::TitleMaterial>>().len();
        ends.push((cameras, materials, world.resource::<Assets<Image>>().len()));
    }
    // The TV's and the title's cameras, one material, and no image left
    // behind by a round's 162 steps (the first round grows the font
    // atlases, so it's the baseline's warm-up).
    assert_eq!((ends[1].0, ends[1].1), (2, 1), "{ends:?}");
    assert_eq!(ends[1], ends[2], "{ends:?}");

    let before = (roots(&mut app), nodes(&mut app).0);
    let farewell = app.world().resource::<Current>().screen.clone();
    app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
    app.world_mut().resource_mut::<Current>().screen = farewell;
    app.update();
    assert_eq!((roots(&mut app), nodes(&mut app).0), before);
}

/// AP.2 review: a source written with nothing new respawns nothing, on
/// any screen.
#[test]
fn a_touched_source_with_nothing_new_respawns_nothing() {
    use crate::extraction::{ExtractStatus, band};
    let mut app = portal_app(1920, 1080, WindowMode::Tv, full("Eruptor"));
    app.insert_resource(ExtractBand(band(&ExtractStatus::Converting {
        done: 3,
        total: 48,
        movie: "eruptor".into(),
    })));
    for screen in every_screen() {
        app.world_mut().resource_mut::<Current>().screen = screen.clone();
        frames(&mut app, 3);
        let (before, _) = nodes(&mut app);
        let world = app.world_mut();
        world.resource_mut::<Current>().set_changed();
        world.resource_mut::<Setup>().set_changed();
        world.resource_mut::<PortalCards>().set_changed();
        world.resource_mut::<ExtractBand>().set_changed();
        frames(&mut app, 3);
        assert_eq!(nodes(&mut app).0, before, "{screen:?}");
    }
}

/// The one entity carrying `M`, and its text.
fn marked<M: Component>(app: &mut App) -> (Entity, String) {
    let world = app.world_mut();
    let mut q = world.query_filtered::<(Entity, &Text), With<M>>();
    let (entity, text) = q.single(world).unwrap();
    (entity, text.0.clone())
}

fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

/// AP.2: a phone joining patches the drawn line, on Join and on the portal.
#[test]
fn a_new_phone_patches_the_players_line_in_place() {
    let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
    {
        let mut current = app.world_mut().resource_mut::<Current>();
        current.screen = TvScreen::Join;
        current.clients = 1;
    }
    frames(&mut app, 3);
    for screen in [
        TvScreen::Join,
        TvScreen::Portal {
            game: "Skylanders: Giants".into(),
        },
    ] {
        app.world_mut().resource_mut::<Current>().screen = screen.clone();
        app.world_mut().resource_mut::<Current>().clients = 1;
        frames(&mut app, 2);
        let (line, text) = marked::<PlayersLine>(&mut app);
        let (address, _) = marked::<JoinAddress>(&mut app);
        assert_eq!(text, "1 phone connected", "{screen:?}");
        app.world_mut().resource_mut::<Current>().clients = 2;
        app.update();
        assert_eq!(
            marked::<PlayersLine>(&mut app),
            (line, "2 phones connected".to_string()),
            "{screen:?}"
        );
        assert_eq!(marked::<JoinAddress>(&mut app).0, address);
    }
}

/// AP.2: a tick of the extraction patches the band; a failure, which
/// has no bar, respawns it.
#[test]
fn a_band_tick_keeps_its_lines_and_bar() {
    use crate::extraction::{ExtractStatus, Failure, band};
    let converting = |done| {
        band(&ExtractStatus::Converting {
            done,
            total: 48,
            movie: "eruptor".into(),
        })
    };
    let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
    app.insert_resource(ExtractBand(converting(12)));
    frames(&mut app, 3);
    let bar = |app: &mut App| {
        let world = app.world_mut();
        let mut q = world.query::<(Entity, &ExtractBar, &Node)>();
        let (entity, bar, node) = q.single(world).unwrap();
        (entity, bar.0, node.width)
    };
    let (title, _) = marked::<ExtractTitle>(&mut app);
    let (detail, text) = marked::<ExtractDetail>(&mut app);
    let (fill, pct, width) = bar(&mut app);
    assert_eq!(
        (text.as_str(), pct, width),
        ("Converting intro 13 of 48…", 25, percent(25))
    );

    app.insert_resource(ExtractBand(converting(13)));
    app.update();
    assert_eq!(marked::<ExtractTitle>(&mut app).0, title);
    assert_eq!(
        marked::<ExtractDetail>(&mut app),
        (detail, "Converting intro 14 of 48…".to_string())
    );
    assert_eq!(bar(&mut app), (fill, 27, percent(27)));

    app.insert_resource(ExtractBand(band(&ExtractStatus::Failed(
        Failure::NoFfmpeg {
            configured: None,
            install: "brew install ffmpeg".into(),
        },
    ))));
    app.update();
    let (failed, text) = marked::<ExtractTitle>(&mut app);
    assert_ne!(failed, title);
    assert_eq!(text, "No intros: ffmpeg is needed");
    let world = app.world_mut();
    assert_eq!(world.query::<&ExtractBar>().iter(world).count(), 0);
}

/// AP.2, with AP.6's button: another error's text is patched in place;
/// the sheet opening swaps the button for a line and a failed pick adds
/// the error, so the column respawns for both.
#[test]
fn the_setup_lines_follow_the_picker() {
    let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
    app.world_mut().resource_mut::<Current>().screen = TvScreen::Setup;
    frames(&mut app, 2);
    // Each line's entity and text; a button reads as `<its id>`.
    let lines = |app: &mut App| {
        let world = app.world_mut();
        let mut columns = world.query_filtered::<&Children, With<SetupLines>>();
        let kids: Vec<Entity> = columns.single(world).unwrap().iter().collect();
        let mut texts = world.query::<&Text>();
        let mut ids = world.query::<&FocusId>();
        kids.into_iter()
            .map(|kid| match ids.get(world, kid) {
                Ok(id) => (kid, format!("<{}>", id.0)),
                Err(_) => (kid, texts.get(world, kid).unwrap().0.clone()),
            })
            .collect::<Vec<_>>()
    };
    let drawn = |lines: &[(Entity, String)]| -> Vec<String> {
        lines.iter().map(|(_, t)| t.clone()).collect()
    };
    let or_run = "Or run: guacamole --setup <folder>";
    let before = lines(&mut app);
    assert_eq!(drawn(&before), ["<choose-folder>", or_run]);

    app.world_mut().resource_mut::<Setup>().0.picking = true;
    app.update();
    let picking = lines(&mut app);
    assert_eq!(
        drawn(&picking),
        ["Choose a folder in the window that opened.", or_run]
    );
    assert_ne!(picking[0].0, before[0].0);

    {
        let mut setup = app.world_mut().resource_mut::<Setup>();
        setup.0.picking = false;
        setup.0.error = Some("/Volumes/gone isn't a folder".into());
    }
    app.update();
    let failed = lines(&mut app);
    assert_eq!(
        drawn(&failed),
        [
            "Couldn't use that folder: /Volumes/gone isn't a folder",
            "<choose-folder>",
            or_run,
        ]
    );

    app.world_mut().resource_mut::<Setup>().0.error = Some("/Volumes/other isn't a folder".into());
    app.update();
    let again = lines(&mut app);
    let entities = |l: &[(Entity, String)]| l.iter().map(|(e, _)| *e).collect::<Vec<_>>();
    assert_eq!(
        entities(&again),
        entities(&failed),
        "patched, not respawned"
    );
    assert_eq!(
        again[0].1,
        "Couldn't use that folder: /Volumes/other isn't a folder"
    );
}

/// AP.2: another error's message lands on the drawn line.
#[test]
fn another_error_patches_the_drawn_message() {
    let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
    let error = |message: &str| TvScreen::ServerError {
        message: message.into(),
    };
    app.world_mut().resource_mut::<Current>().screen = error("open profile store");
    frames(&mut app, 2);
    let (line, text) = marked::<ErrorMessage>(&mut app);
    assert_eq!(text, "open profile store");
    app.world_mut().resource_mut::<Current>().screen = error("bind 0.0.0.0:8765");
    app.update();
    assert_eq!(
        marked::<ErrorMessage>(&mut app),
        (line, "bind 0.0.0.0:8765".to_string())
    );
}

/// Each cell's entity and its one child, the card or the empty frame.
fn cells(app: &mut App) -> std::collections::BTreeMap<u8, (Entity, Entity)> {
    let world = app.world_mut();
    let mut q = world.query::<(Entity, &Cell, &Children)>();
    q.iter(world)
        .map(|(cell, slot, kids)| {
            assert_eq!(kids.len(), 1, "cell {}", slot.0);
            (slot.0, (cell, kids[0]))
        })
        .collect()
}

/// AP.3: a placement in slot 3 respawns that cell's content and nothing
/// else: the other seven cells' content and the header stay.
#[test]
fn a_placement_respawns_only_its_own_cell() {
    let two = PortalCards::default().reduce([
        PortalChange::Placed {
            slot: SlotIndex::new(0).unwrap(),
            placement: placement("Eruptor", Some("#33cc66")),
        },
        PortalChange::Placed {
            slot: SlotIndex::new(5).unwrap(),
            placement: placement("Tree Rex", None),
        },
    ]);
    let mut app = portal_app(1920, 1080, WindowMode::Tv, two.clone());
    frames(&mut app, 3);
    let before = cells(&mut app);
    assert_eq!(before.len(), 8);
    let header = (
        marked::<GameName>(&mut app).0,
        marked::<PlayersLine>(&mut app).0,
    );

    let slot3 = SlotIndex::from_display(3).unwrap();
    *app.world_mut().resource_mut::<PortalCards>() = two.clone().reduce([PortalChange::Placed {
        slot: slot3,
        placement: placement("Spyro", Some("#33cc66")),
    }]);
    app.update();
    let after = cells(&mut app);
    for (slot, (cell, content)) in &after {
        assert_eq!(*cell, before[slot].0, "cell {slot} respawned");
        if *slot == slot3.as_u8() {
            assert_ne!(*content, before[slot].1);
        } else {
            assert_eq!(*content, before[slot].1, "slot {slot}'s content respawned");
        }
    }
    let world = app.world_mut();
    let card = world.get::<CardSlot>(after[&slot3.as_u8()].1).unwrap();
    assert_eq!(card.0, slot3.as_u8());
    assert_eq!(
        (
            marked::<GameName>(&mut app).0,
            marked::<PlayersLine>(&mut app).0
        ),
        header
    );

    // The same cards again (a resume replaying them) respawn nothing.
    let same = app.world().resource::<PortalCards>().clone();
    *app.world_mut().resource_mut::<PortalCards>() = same;
    app.update();
    assert_eq!(cells(&mut app), after);
}

/// Each frame's `Redrawn` messages, and the frames the layout report was
/// armed in (`report_layout` takes one off every `Last`, so it reads
/// `REPORT_AFTER_FRAMES` in `PostUpdate` only in a frame that armed it).
#[derive(Resource, Default)]
struct Seen {
    redrawn: Vec<Redrawn>,
    armed: usize,
}

fn see(mut seen: ResMut<Seen>, mut redrawn: MessageReader<Redrawn>, report: Res<LayoutReport>) {
    seen.redrawn.extend(redrawn.read().copied());
    if report.0 == REPORT_AFTER_FRAMES {
        seen.armed += 1;
    }
}

/// What was redrawn and how often the report was armed since last asked.
fn seen(app: &mut App) -> (Vec<Redrawn>, usize) {
    let mut seen = app.world_mut().resource_mut::<Seen>();
    (
        std::mem::take(&mut seen.redrawn),
        std::mem::take(&mut seen.armed),
    )
}

/// AP.4: one slot's change arms the layout report once, a band tick arms
/// it once, a patched line and an unchanged frame never do, and leaving
/// the portal disarms it.
#[test]
fn a_slot_or_a_band_tick_arms_the_layout_report_once() {
    use crate::extraction::{ExtractStatus, band};
    let converting = |done| {
        ExtractBand(band(&ExtractStatus::Converting {
            done,
            total: 48,
            movie: "eruptor".into(),
        }))
    };
    let mut app = portal_app(1920, 1080, WindowMode::Tv, full("Eruptor"));
    app.init_resource::<Seen>()
        .add_systems(PostUpdate, see)
        .add_systems(Last, report_layout);
    frames(&mut app, 6);
    let (entered, armed) = seen(&mut app);
    assert_eq!(
        entered,
        [
            Redrawn::Screen(TvState::Starting),
            Redrawn::Screen(TvState::Portal)
        ]
    );
    assert_eq!(armed, 1);

    let slot3 = SlotIndex::from_display(3).unwrap();
    let spyro = app
        .world()
        .resource::<PortalCards>()
        .clone()
        .reduce([PortalChange::Placed {
            slot: slot3,
            placement: placement("Spyro", Some("#33cc66")),
        }]);
    *app.world_mut().resource_mut::<PortalCards>() = spyro;
    frames(&mut app, 6);
    assert_eq!(seen(&mut app), (vec![Redrawn::Slot(slot3.as_u8())], 1));
    assert_eq!(app.world().resource::<LayoutReport>().0, 0, "never fired");

    // The band appearing, then a tick.
    for done in [12, 13] {
        app.insert_resource(converting(done));
        frames(&mut app, 6);
        assert_eq!(seen(&mut app), (vec![Redrawn::Band], 1), "intro {done}");
    }

    frames(&mut app, 6);
    assert_eq!(seen(&mut app), (vec![], 0));
    app.world_mut().resource_mut::<Current>().clients = 3;
    frames(&mut app, 6);
    assert_eq!(seen(&mut app), (vec![Redrawn::Text], 0));
    app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
    frames(&mut app, 6);
    assert_eq!(seen(&mut app), (vec![Redrawn::Screen(TvState::Join)], 0));
}

/// One star drawn on the sky: what it is, its node's translation in
/// design px, its centre in target px and what it draws.
type DrawnStar = (starfield::SkyStar, Val2, Vec2, ImageNode);

/// The one star layer up: its rect in target px and its stars, in the
/// order they draw.
fn star_layer(app: &mut App) -> (Entity, Rect, Vec<DrawnStar>) {
    let world = app.world_mut();
    let mut q = world.query_filtered::<(
        Entity,
        &ComputedNode,
        &UiGlobalTransform,
        &Children,
    ), With<starfield::StarLayer>>();
    let (layer, node, at, kids) = q.single(world).unwrap();
    let (rect, kids) = (node_rect(node, at), kids.to_vec());
    let mut q = world.query::<(
        &starfield::SkyStar,
        &UiTransform,
        &UiGlobalTransform,
        &ImageNode,
        Has<Children>,
    )>();
    let stars = kids
        .iter()
        .map(|kid| {
            let (star, moved, drawn, image, parent) = q.get(world, *kid).unwrap();
            assert!(!parent, "a star is one node");
            (*star, moved.translation, drawn.translation, image.clone())
        })
        .collect();
    (layer, rect, stars)
}

/// AP.11: every screen's root sits on the sky, at 1080p, 4K and in the
/// desktop window: §1's three gradients on the root and a layer of stars
/// as its first child, over the whole root from the window's corner (not
/// the inset's), with no text in it and no line moved by it. Since the
/// 2026-10-18 follow-up the layer holds a node a star, the small ones
/// first, each drawing its size's sprite in its tint at its alpha where
/// its copy in the window is with no drift.
#[test]
fn every_screen_sits_on_the_starfield() {
    let theme = TvTheme::default();
    let sky = starfield::stars(starfield::SEED);
    for (width, height, mode) in [
        (1920, 1080, WindowMode::Tv),
        (3840, 2160, WindowMode::Tv),
        (1280, 720, WindowMode::Desktop),
    ] {
        let mut app = portal_app(width, height, mode, full("Eruptor"));
        app.insert_resource(ReducedMotion(true));
        let scale = height as f32 / DESIGN_HEIGHT;
        let sprites = app.world().resource::<TvAssets>().stars.clone();
        for screen in every_screen() {
            let case = format!("{width}x{height} {screen:?}");
            app.world_mut().resource_mut::<Current>().screen = screen.clone();
            frames(&mut app, 3);
            let first = {
                let world = app.world_mut();
                let mut q =
                    world.query_filtered::<(&BackgroundGradient, &Children), With<ScreenRoot>>();
                let (sky, children) = q.single(world).unwrap();
                assert_eq!(*sky, starfield::sky(&theme), "{case}");
                assert_eq!(sky.0.len(), 3, "{case}");
                children[0]
            };
            let (layer, rect, stars) = star_layer(&mut app);
            assert_eq!(layer, first, "{case}: the stars draw under the screen");
            let window = Rect::new(0.0, 0.0, width as f32, height as f32);
            assert!(rect.min.abs_diff_eq(window.min, 0.01), "{case}: {rect:?}");
            assert!(rect.max.abs_diff_eq(window.max, 0.01), "{case}: {rect:?}");
            assert_eq!(stars.len(), sky.len(), "{case}");
            let big: Vec<bool> = stars.iter().map(|s| s.3.image == sprites.big).collect();
            assert!(big.is_sorted(), "{case}: the small stars first");
            for star in &sky {
                let (placed, moved, centre, image) =
                    stars.iter().find(|s| s.0.at == star.at).unwrap();
                assert_eq!(*moved, placed.placed(Vec2::ZERO).translation, "{case}");
                let home = starfield::wrap(star.at) * scale;
                assert!(centre.abs_diff_eq(home, 0.01), "{case}: {centre} {home}");
                let sprite = if star.big() {
                    &sprites.big
                } else {
                    &sprites.small
                };
                assert_eq!(image.image, *sprite, "{case}");
                let tint = theme.star_tints[star.tint as usize].with_alpha(star.alpha);
                assert_eq!(image.color, tint, "{case}");
            }
            assert!(app.world().get::<Text>(layer).is_none(), "{case}");

            // Without the layer every line lands where it did.
            let lines = |app: &mut App| {
                let mut drawn = drawn_text(app);
                drawn.sort_by(|a, b| a.0.cmp(&b.0));
                drawn
                    .into_iter()
                    .map(|(text, _, _, rect)| (text, rect))
                    .collect::<Vec<_>>()
            };
            let with = lines(&mut app);
            app.world_mut().entity_mut(layer).despawn();
            frames(&mut app, 2);
            assert_eq!(lines(&mut app), with, "{case}");
        }
    }
}

/// AP.11: the stars drift with the clock, a screen entered later picking
/// the drift up where the last one was, and hold at their start under
/// `ReducedMotion`.
#[test]
fn the_stars_drift_unless_motion_is_reduced() {
    use bevy::time::TimeUpdateStrategy;
    for reduced in [false, true] {
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        app.insert_resource(ReducedMotion(reduced))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(25)));
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_max_delta(Duration::from_secs(60));
        for screen in [TvScreen::Join, TvScreen::Farewell] {
            app.world_mut().resource_mut::<Current>().screen = screen.clone();
            frames(&mut app, 2);
            let elapsed = app.world().resource::<Time>().elapsed();
            assert!(elapsed >= Duration::from_secs(25), "{elapsed:?}");
            let at = if reduced {
                Vec2::ZERO
            } else {
                starfield::drift_at(elapsed)
            };
            assert_ne!(at == Vec2::ZERO, !reduced, "{screen:?} at {elapsed:?}");
            let (_, _, stars) = star_layer(&mut app);
            let mut moved = 0;
            for (placed, translation, centre, _) in &stars {
                let case = format!("{reduced} {screen:?} {placed:?}");
                assert_eq!(*translation, placed.placed(at).translation, "{case}");
                let drawn = placed.centre(at);
                assert!(centre.abs_diff_eq(drawn, 0.01), "{case}: {centre}");
                moved += usize::from(!drawn.abs_diff_eq(placed.centre(Vec2::ZERO), 0.01));
            }
            assert_eq!(moved == stars.len(), !reduced, "{reduced} {screen:?}");
        }
    }
}

/// The most pairs of `stars` one shift carries one onto the other, to
/// within about half a px: a pattern that repeats shows as a shift many
/// pairs share (AP.11's tile at 16:9: 960 px across, 32 pairs), where
/// scattered stars share one by chance at most twice.
fn most_pairs_one_shift(stars: &[Vec2]) -> usize {
    let mut shifts = std::collections::HashMap::<IVec2, usize>::new();
    for (i, a) in stars.iter().enumerate() {
        for b in &stars[i + 1..] {
            let d = *b - *a;
            let d = if (d.x, d.y) < (0.0, 0.0) { -d } else { d };
            *shifts.entry((d * 2.0).round().as_ivec2()).or_default() += 1;
        }
    }
    shifts
        .keys()
        .map(|at| {
            (-1..=1)
                .flat_map(|x| (-1..=1).map(move |y| *at + IVec2::new(x, y)))
                .filter_map(|near| shifts.get(&near))
                .sum()
        })
        .max()
        .unwrap_or(0)
}

#[test]
fn a_repeating_pattern_shares_a_shift_and_scattered_stars_dont() {
    let tile: Vec<Vec2> = starfield::stars(starfield::SEED)
        .iter()
        .map(|s| s.at)
        .filter(|at| at.cmplt(Vec2::new(960.0, 540.0)).all())
        .collect();
    assert_eq!(tile.len(), 16);
    let two_by_two: Vec<Vec2> = [0.0, 960.0]
        .iter()
        .flat_map(|x| [0.0, 540.0].map(|y| Vec2::new(*x, y)))
        .flat_map(|corner| tile.iter().map(move |at| *at + corner))
        .collect();
    assert_eq!(most_pairs_one_shift(&two_by_two), 32);
    let scattered: Vec<Vec2> = starfield::stars(starfield::SEED)
        .iter()
        .map(|s| s.at)
        .filter(|at| at.cmplt(Vec2::new(1920.0, 1080.0)).all())
        .collect();
    assert_eq!(scattered.len(), 64);
    assert!(most_pairs_one_shift(&scattered) <= 2);
}

/// The AP.11 follow-up (2026-10-18): AP.11's 960×540 tile showed 2×2 on
/// a 16:9 screen, the same 16 stars in each quarter. On a 16:9 screen at
/// 1080p and 4K and a 32:9 one at both, from the drift's start and then a
/// frame every 7 s through a loop and past it, every star on the screen,
/// a sliver at an edge included, is another star of the sky (no two at
/// the same offset within its period), each node draws its own star, no
/// shift carries more than two of them onto others (no pattern repeats,
/// whatever its period) and every 2×2 cells' patch of the screen holds
/// one.
#[test]
fn no_star_shows_twice_on_a_screen() {
    use crate::test_support::starfield::offset_in_period;
    use bevy::time::TimeUpdateStrategy;
    for (width, height) in [(1920, 1080), (3840, 2160), (3840, 1080), (7680, 2160)] {
        let mut app = portal_app(width, height, WindowMode::Tv, PortalCards::default());
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(7)));
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_max_delta(Duration::from_secs(60));
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
        let scale = height as f32 / DESIGN_HEIGHT;
        let screen = Vec2::new(width as f32, height as f32) / scale;
        let patch = 2.0 * starfield::CELL;
        let patches = (screen / patch).as_uvec2();
        let mut shown = 0;
        for frame in 0..22 {
            app.update();
            let elapsed = app.world().resource::<Time>().elapsed();
            let drift = starfield::drift_at(elapsed);
            let case = format!("{width}x{height} frame {frame} at {elapsed:?}");
            let (_, _, stars) = star_layer(&mut app);
            let mut seen: Vec<(Vec2, Vec2)> = Vec::new();
            let mut held = std::collections::BTreeSet::new();
            for (placed, _, centre, _) in &stars {
                let c = *centre / scale;
                let reach = Vec2::splat(placed.half);
                if !(c.cmpgt(-reach).all() && c.cmplt(screen + reach).all()) {
                    continue;
                }
                let offset = offset_in_period(c + drift);
                let own = offset_in_period(placed.at);
                assert!(offset.abs_diff_eq(own, 0.01), "{case}: {c} draws {own}");
                for (other, at) in &seen {
                    assert!(
                        !offset.abs_diff_eq(*other, 0.5),
                        "{case}: {c} and {at} are one star"
                    );
                }
                seen.push((offset, c));
                if c.cmpge(Vec2::ZERO).all() && c.cmplt(screen).all() {
                    held.insert((c / patch).as_uvec2().to_array());
                }
            }
            assert_eq!(held.len() as u32, patches.x * patches.y, "{case}");
            let on: Vec<Vec2> = seen.iter().map(|(_, at)| *at).collect();
            assert!(most_pairs_one_shift(&on) <= 2, "{case}");
            shown = shown.max(seen.len());
        }
        // About one star a cell: 64 cells on 16:9, 128 on 32:9.
        let cells = (screen / starfield::CELL).element_product() as usize;
        assert!((cells..cells + cells / 2).contains(&shown), "{shown}");
    }
}

/// AP.12: the owner pip stays in its card's top-right corner (design
/// decision 8), on eight cards at 1080p, 4K and in the desktop window,
/// with short names and with long ones under the longest band.
#[test]
fn each_pip_sits_in_its_cards_top_right_quadrant() {
    for (w, h, mode) in [
        (1920, 1080, WindowMode::Tv),
        (3840, 2160, WindowMode::Tv),
        (1280, 720, WindowMode::Desktop),
    ] {
        for (name, band) in [
            ("Eruptor", None),
            ("Legendary Chop Chop the Undead", Some(long_failure())),
        ] {
            let mut app = portal_app(w, h, mode, full(name));
            app.insert_resource(ExtractBand(band));
            let laid = measure(&mut app);
            assert_eq!(laid.pips.len(), 8, "{w}x{h} {name}");
            for (slot, pip) in &laid.pips {
                let card = laid.cards[usize::from(*slot)].1;
                let quadrant = Rect::from_corners(
                    Vec2::new(card.center().x, card.min.y),
                    Vec2::new(card.max.x, card.center().y),
                );
                assert!(
                    contains(quadrant, *pip),
                    "{w}x{h} {name}: pip {slot} {pip:?} leaves {quadrant:?}"
                );
            }
        }
    }
}

/// The card's box, its face, and the bezel, plate and pip `spawn_card`
/// drew for `slot`.
fn card_parts(app: &mut App, slot: u8) -> [Entity; 5] {
    let world = app.world_mut();
    let mut cards = world.query::<(Entity, &CardSlot)>();
    let card = cards
        .iter(world)
        .find(|(_, s)| s.0 == slot)
        .map(|(e, _)| e)
        .unwrap();
    let mut found = [
        card,
        Entity::PLACEHOLDER,
        Entity::PLACEHOLDER,
        Entity::PLACEHOLDER,
        Entity::PLACEHOLDER,
    ];
    let mut stack = vec![card];
    while let Some(e) = stack.pop() {
        if world.get::<CardFace>(e).is_some() {
            found[1] = e;
        }
        if world.get::<surfaces::Bezel>(e).is_some() {
            found[2] = e;
        }
        if world.get::<surfaces::Plate>(e).is_some() {
            found[3] = e;
        }
        if world.get::<Pip>(e).is_some() {
            found[4] = e;
        }
        if let Some(kids) = world.get::<Children>(e) {
            stack.extend(kids.iter());
        }
    }
    found
}

fn rect_of(app: &App, e: Entity) -> Rect {
    node_rect(
        app.world().get::<ComputedNode>(e).unwrap(),
        app.world().get::<UiGlobalTransform>(e).unwrap(),
    )
}

/// AP.12: a figure card is §3.3's framed panel edged in the gold run, its
/// portrait in §3.1's bezel ringed in the figure's element (the phone's
/// rule: the portrait covers the plate) or in gold for a figure with no
/// element, the plate concentric at 80% of the bezel holding the portrait,
/// and the pip a gold ring round the owner's colour. Since AP.14 the card
/// is the face over the `CardSlot` box, which wears nothing and keeps its
/// rect, the face filling it exactly at rest.
#[test]
fn a_card_wears_its_materials() {
    let theme = TvTheme::default();
    let dir = tempfile::tempdir().unwrap();
    let thumb = dir.path().join("thumb.png");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 60, 20, 255]))
        .save(&thumb)
        .unwrap();
    let mut eruptor = placement("Eruptor", Some("#e8423f"));
    eruptor.portrait = Some(thumb);
    let mut rex = placement("Tree Rex", None);
    rex.element = None;
    let cards = PortalCards::default().reduce([
        PortalChange::Placed {
            slot: SlotIndex::new(0).unwrap(),
            placement: eruptor,
        },
        PortalChange::Placed {
            slot: SlotIndex::new(1).unwrap(),
            placement: rex,
        },
    ]);
    for (w, h, mode) in [(1920, 1080, WindowMode::Tv), (3840, 2160, WindowMode::Tv)] {
        let mut app = portal_app(w, h, mode, cards.clone());
        measure(&mut app);
        let scale = h as f32 / DESIGN_HEIGHT;
        for (slot, element, portrait) in [(0, Some(Element::Fire), true), (1, None, false)] {
            let at = format!("{w}x{h} slot {slot}");
            let [card, face, bezel, plate, pip] = card_parts(&mut app, slot);
            let world = app.world();
            assert!(world.get::<BorderGradient>(card).is_none(), "{at}");
            assert!(world.get::<BackgroundGradient>(card).is_none(), "{at}");
            assert_eq!(rect_of(&app, face), rect_of(&app, card), "{at}");
            let (edge, fill) = (
                world.get::<BorderGradient>(face).unwrap(),
                world.get::<BackgroundGradient>(face).unwrap(),
            );
            let (expected_fill, expected_edge) = {
                let mut probe = World::new();
                let e = probe.spawn(surfaces::card(&theme)).id();
                (
                    probe.get::<BackgroundGradient>(e).unwrap().clone(),
                    probe.get::<BorderGradient>(e).unwrap().clone(),
                )
            };
            assert_eq!(*edge, expected_edge, "{at}: the gold edge");
            assert_eq!(*fill, expected_fill, "{at}: the panel");
            assert!(
                world
                    .get::<BorderColor>(face)
                    .unwrap()
                    .top
                    .is_fully_transparent(),
                "{at}: no flat edge over the gold"
            );
            let ring = &world.get::<BackgroundGradient>(bezel).unwrap().0[0];
            assert_eq!(*ring, surfaces::ring(&theme, element), "{at}");
            assert_eq!(
                *ring == surfaces::gold_ring(&theme),
                element.is_none(),
                "{at}: gold only for a figure with no element"
            );
            assert_eq!(
                world.get::<ImageNode>(plate).is_some(),
                portrait,
                "{at}: the portrait sits on the plate"
            );
            let (outer, inner) = (rect_of(&app, bezel), rect_of(&app, plate));
            assert!(
                (outer.width() - surfaces::BEZEL * scale).abs() < 1.0,
                "{at}"
            );
            assert!(
                inner.center().abs_diff_eq(outer.center(), 1.0),
                "{at}: {inner:?} in {outer:?}"
            );
            assert!(
                (inner.width() / outer.width() - 0.8).abs() < 0.01,
                "{at}: {inner:?} in {outer:?}"
            );
            if slot == 0 {
                let world = app.world();
                assert_eq!(
                    Srgba::from(world.get::<BackgroundColor>(pip).unwrap().0).to_hex(),
                    "#E8423F",
                    "{at}: the pip's plate is the owner's"
                );
                let ring = world.get::<BorderGradient>(pip).unwrap();
                assert_eq!(ring.0[0], surfaces::gold_ring(&theme), "{at}");
                let size = rect_of(&app, pip).width();
                assert!((size - surfaces::PIP * scale).abs() < 1.0, "{at}: {size}");
            } else {
                assert_eq!(pip, Entity::PLACEHOLDER, "{at}: no owner, no pip");
            }
        }
    }
}

/// AP.12, AP.13: every choice's face is §3.2's blue card: at rest (the
/// focus not showing) the three-stop fill, a solid `--gb` edge and the two
/// drops under it; focused, the brightened fill with §3.1's picking glow
/// between the drops and the face at 1.05, the button's own node unscaled.
#[test]
fn every_choice_wears_the_blue_card() {
    use bevy::input_focus::InputFocusVisible;
    let theme = TvTheme::default();
    let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
    app.insert_resource(ReducedMotion(true));
    let mut seen = std::collections::BTreeSet::new();
    for screen in every_screen() {
        app.world_mut().resource_mut::<Current>().screen = screen.clone();
        for showing in [false, true] {
            app.insert_resource(InputFocusVisible(showing));
            frames(&mut app, 2);
            let world = app.world_mut();
            let mut q = world.query::<(Entity, &FocusId, &Children)>();
            let faces: Vec<(Entity, &'static str, Entity)> = q
                .iter(world)
                .map(|(button, id, kids)| (button, id.0, kids[0]))
                .collect();
            for (button, id, face) in faces {
                let at = format!("{screen:?} {id} showing {showing}");
                assert!(world.get::<ChoiceFace>(face).is_some(), "{at}");
                let fill = world.get::<BackgroundGradient>(face).unwrap();
                let Gradient::Linear(base) = &fill.0[0] else {
                    panic!("{at}: {fill:?}");
                };
                let colours: Vec<Color> = base.stops.iter().map(|s| s.color).collect();
                let shadows = &world.get::<BoxShadow>(face).unwrap().0;
                let scale = world.get::<UiTransform>(face).unwrap().scale;
                if showing {
                    assert_eq!(colours, theme.blue_card_lit, "{at}");
                    assert_eq!(shadows.len(), 3, "{at}");
                    assert_eq!(shadows[1].color, theme.focus_glow, "{at}");
                    assert_eq!(scale, Vec2::splat(look::FOCUS_SCALE), "{at}");
                } else {
                    assert_eq!(colours, theme.blue_card, "{at}");
                    assert_eq!(shadows.len(), 2, "{at}");
                    assert_eq!(scale, Vec2::ONE, "{at}");
                }
                let edge = world.get::<BorderColor>(face).unwrap();
                assert_eq!(edge.top, theme.gold_bright, "{at}");
                let outer = world.get::<UiTransform>(button).unwrap();
                assert_eq!(*outer, UiTransform::IDENTITY, "{at}: the button's own node");
                seen.insert(id);
            }
        }
    }
    assert_eq!(
        seen,
        [CHOOSE_FOLDER, CONTINUE, CLOSE].into(),
        "every choice was drawn"
    );
}

/// AP.13: the focus moving from one choice to another eases one face down
/// and the other up through the spring's overshoot, and every other node,
/// the buttons and the marked lines included, stays exactly where it was
/// before, during and after; the change writes `Redrawn::Focus`, which
/// arms a shot.
#[test]
fn a_focus_change_moves_no_marked_rect() {
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy::time::TimeUpdateStrategy;
    let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
    app.init_resource::<Seen>()
        .add_systems(PostUpdate, see)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            20,
        )));
    app.world_mut().resource_mut::<Current>().screen = TvScreen::ServerError {
        message: "bind 0.0.0.0:8765: address in use".into(),
    };
    frames(&mut app, 2);
    // A second choice beside Close, as the Home menu will have.
    let root = {
        let world = app.world_mut();
        let mut q = world.query_filtered::<Entity, With<ScreenRoot>>();
        q.single(world).unwrap()
    };
    let pen = Pen::new(
        app.world().resource::<TvAssets>(),
        app.world().resource::<TvTheme>(),
    );
    let other = app
        .world_mut()
        .spawn(choice_bundle(&pen, "Try again", "try-again"))
        .id();
    app.world_mut().entity_mut(root).add_child(other);
    let close = crate::test_support::tv::choice(&mut app, CLOSE);
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(close, FocusCause::Navigated);
    frames(&mut app, 20);
    assert_eq!(scale_of(&app, close), look::FOCUS_SCALE);
    assert_eq!(scale_of(&app, other), 1.0);
    let before = outer_nodes(&mut app);
    assert!(before.len() > 4, "{before:?}");
    for marked in [close, other] {
        assert!(before.contains_key(&marked), "the buttons are measured");
    }
    seen(&mut app);

    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(other, FocusCause::Navigated);
    let mut ups = Vec::new();
    let mut downs = Vec::new();
    for frame in 0..20 {
        app.update();
        assert_eq!(outer_nodes(&mut app), before, "frame {frame}");
        ups.push(scale_of(&app, other));
        downs.push(scale_of(&app, close));
    }
    let top = ups.iter().copied().fold(f32::MIN, f32::max);
    assert!(top > look::FOCUS_SCALE, "the pop overshoots: {ups:?}");
    assert!(ups[1] > 1.0 && ups[1] < top, "mid-way: {ups:?}");
    assert!(
        downs.iter().any(|s| *s < 1.0),
        "and back past rest: {downs:?}"
    );
    assert_eq!(ups.last(), Some(&look::FOCUS_SCALE), "{ups:?}");
    assert_eq!(downs.last(), Some(&1.0), "{downs:?}");
    assert_eq!(outer_nodes(&mut app), before, "after");
    let (redrawn, armed) = seen(&mut app);
    assert_eq!(
        redrawn,
        [Redrawn::Focus],
        "a shot is armed, no layout report"
    );
    assert_eq!(armed, 0);
}

/// A node's rect as drawn, its transform's scale included (`node_rect`
/// takes only where its centre is).
fn drawn_rect(app: &App, e: Entity) -> Rect {
    let size = app.world().get::<ComputedNode>(e).unwrap().size();
    let at = app.world().get::<UiGlobalTransform>(e).unwrap();
    let [a, b] = [-0.5 * size, 0.5 * size].map(|p| at.transform_point2(p));
    Rect::from_corners(a, b)
}

/// Each card's face by slot: the entity, its rect as drawn, its
/// transform and whether it's shown.
fn faces(app: &mut App) -> std::collections::BTreeMap<u8, (Entity, Rect, UiTransform, bool)> {
    let world = app.world_mut();
    let mut q =
        world.query_filtered::<(Entity, &ChildOf, &UiTransform, &Visibility), With<CardFace>>();
    let found: Vec<_> = q
        .iter(world)
        .map(|(e, box_, t, v)| (e, box_.parent(), *t, *v != Visibility::Hidden))
        .collect();
    found
        .into_iter()
        .map(|(e, box_, t, shown)| {
            let slot = app.world().get::<CardSlot>(box_).unwrap().0;
            (slot, (e, drawn_rect(app, e), t, shown))
        })
        .collect()
}

/// The flash over `bezel`'s portrait, if its card was placed.
fn flash_on(app: &App, bezel: Entity) -> Option<Entity> {
    let kids = app.world().get::<Children>(bezel)?;
    kids.iter()
        .find(|k| app.world().get::<card_motion::ImpactFlash>(*k).is_some())
}

/// AP.14, the box's check: the portal's entrance rises its eight cards
/// in turn, and at every 20 ms frame along the way the mirror reads each
/// card's box where it rests, none outside the inset and no two
/// overlapping, every node off the faces keeps its rect, every pip (on
/// a face, so it moves) stays on its card's box, and the faces as drawn
/// never overlap each other or leave the window.
#[test]
fn the_entrance_rises_without_moving_a_marked_rect() {
    use bevy::time::TimeUpdateStrategy;

    use crate::mirror::{Mirror, TvMirror};
    let (w, h) = (1920, 1080);
    let window = Rect::new(0.0, 0.0, w as f32, h as f32);
    let mirrored = |reduced: bool| {
        let mut app = portal_app(w, h, WindowMode::Tv, full("Eruptor"));
        let mirror = TvMirror::default();
        app.insert_resource(mirror.clone())
            .insert_resource(ReducedMotion(reduced))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                20,
            )))
            .add_plugins(Mirror);
        (app, mirror)
    };
    let boxes = |mirror: &crate::mirror::TvMirror| {
        let shot = mirror.0.lock().unwrap().clone();
        let rects: Vec<[f32; 4]> = shot.cards.iter().map(|c| c.rect).collect();
        (shot, rects)
    };
    let rest = {
        let (mut app, mirror) = mirrored(true);
        frames(&mut app, 3);
        boxes(&mirror).1
    };
    assert_eq!(rest.len(), 8);

    let (mut app, mirror) = mirrored(false);
    app.update();
    let outer = outer_nodes(&mut app);
    let (mut waiting, mut rising) = (0, 0);
    for frame in 0..60 {
        let (shot, rects) = boxes(&mirror);
        assert!(shot.cards_outside.is_empty(), "frame {frame}: {shot:?}");
        assert_eq!(shot.overlapping, 0, "frame {frame}");
        assert_eq!(rects, rest, "frame {frame}: a card's box moved");
        assert_eq!(outer_nodes(&mut app), outer, "frame {frame}");
        // The pips ride the faces, and `report_layout` measures them
        // (`pips_off_card`): each stays on its card's box as drawn.
        let pips: Vec<(u8, Entity)> = {
            let world = app.world_mut();
            let mut q = world.query::<(Entity, &Pip)>();
            q.iter(world).map(|(e, p)| (p.0, e)).collect()
        };
        assert_eq!(pips.len(), 8, "frame {frame}");
        for (slot, pip) in pips {
            let [card, ..] = card_parts(&mut app, slot);
            let (card, pip) = (rect_of(&app, card), drawn_rect(&app, pip));
            assert!(
                contains(card, pip),
                "frame {frame}: pip {slot} {pip:?} off {card:?}"
            );
        }
        let faces = faces(&mut app);
        assert_eq!(faces.len(), 8);
        let shown: Vec<Rect> = faces.values().filter(|f| f.3).map(|f| f.1).collect();
        for (i, a) in shown.iter().enumerate() {
            assert!(
                contains(window, *a),
                "frame {frame}: {a:?} leaves the window"
            );
            for b in &shown[i + 1..] {
                assert!(a.intersect(*b).is_empty(), "frame {frame}: {a:?} on {b:?}");
            }
        }
        waiting += faces.values().filter(|f| !f.3).count();
        rising += faces
            .values()
            .filter(|f| {
                let Val::Px(drop) = f.2.translation.y else {
                    panic!("{:?}", f.2)
                };
                f.3 && drop > 0.0 && drop < card_motion::RISE_DROP
            })
            .count();
        app.update();
    }
    assert!(waiting > 0, "cards waited their turn, hidden");
    assert!(rising > 8, "and were caught mid-rise");
    for (slot, (_, rect, transform, shown)) in faces(&mut app) {
        assert_eq!(transform, UiTransform::IDENTITY, "slot {slot} at rest");
        assert!(shown, "slot {slot}");
        let r = rest[usize::from(slot)];
        assert!(
            rect.min.abs_diff_eq(Vec2::new(r[0], r[1]), 0.01),
            "slot {slot}: its face over its box at rest, {rect:?} on {r:?}"
        );
    }
}

/// AP.14: a placement while the portal is up rises alone, at once, under
/// its flash; the flash grows past its card's edge and the face clips it
/// there, so it never reaches a neighbour; its bezel holds still through
/// the impact while the others breathe, and breathes after.
#[test]
fn a_placement_rises_alone_and_its_flash_stays_on_its_card() {
    use bevy::time::TimeUpdateStrategy;
    use bevy::ui::{CalculatedClip, Val2};
    let two = PortalCards::default().reduce([
        PortalChange::Placed {
            slot: SlotIndex::new(0).unwrap(),
            placement: placement("Eruptor", Some("#33cc66")),
        },
        PortalChange::Placed {
            slot: SlotIndex::new(2).unwrap(),
            placement: placement("Spyro", None),
        },
    ]);
    let mut app = portal_app(1920, 1080, WindowMode::Tv, two.clone());
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        20,
    )));
    // The entrance is over by 1.1 s.
    frames(&mut app, 60);
    assert!(
        faces(&mut app)
            .values()
            .all(|f| f.2 == UiTransform::IDENTITY && f.3)
    );
    *app.world_mut().resource_mut::<PortalCards>() = two.reduce([PortalChange::Placed {
        slot: SlotIndex::new(1).unwrap(),
        placement: placement("Tree Rex", Some("#33cc66")),
    }]);
    app.update();
    let outer = outer_nodes(&mut app);
    let [_, face, bezel, _, _] = card_parts(&mut app, 1);
    let flash = flash_on(&app, bezel).expect("a placement flashes");
    let others = [0, 2].map(|slot| card_parts(&mut app, slot));
    for [_, _, bezel, _, _] in others {
        assert_eq!(flash_on(&app, bezel), None, "the entrance doesn't flash");
    }
    let (mut past_its_edge, mut others_breathe, mut floats_after) = (false, false, false);
    for frame in 0..45u32 {
        let age = frame * 20;
        let world = app.world();
        let lift = |e: Entity| world.get::<UiTransform>(e).unwrap().translation;
        let rise = *world.get::<UiTransform>(face).unwrap();
        assert_ne!(world.get::<Visibility>(face), Some(&Visibility::Hidden));
        if frame == 0 {
            assert_eq!(rise.translation, Val2::px(0.0, card_motion::RISE_DROP));
            assert_eq!(rise.scale, Vec2::splat(card_motion::RISE_SCALE));
        }
        if age >= 500 {
            assert_eq!(rise, UiTransform::IDENTITY, "risen at {age} ms");
        }
        for [_, face, bezel, _, _] in others {
            assert_eq!(
                *world.get::<UiTransform>(face).unwrap(),
                UiTransform::IDENTITY,
                "it rises alone"
            );
            others_breathe |= lift(bezel) != Val2::ZERO;
        }
        if age <= 600 {
            assert_eq!(lift(bezel), Val2::ZERO, "held through the impact, {age} ms");
        } else {
            floats_after |= lift(bezel) != Val2::ZERO;
        }
        let shown = world.get::<Visibility>(flash) != Some(&Visibility::Hidden);
        assert_eq!(shown, age > 0 && age < 600, "the flash at {age} ms");
        if shown {
            let drawn = drawn_rect(&app, flash);
            let card = drawn_rect(&app, face);
            let clip = app.world().get::<CalculatedClip>(flash).unwrap();
            assert!(clip.contains_point(drawn.center()), "{age} ms");
            let corners = [
                drawn.min,
                drawn.max,
                Vec2::new(drawn.min.x, drawn.max.y),
                Vec2::new(drawn.max.x, drawn.min.y),
                Vec2::new(drawn.center().x, drawn.min.y),
            ];
            for corner in corners.into_iter().filter(|c| !card.contains(*c)) {
                past_its_edge = true;
                assert!(!clip.contains_point(corner), "{age} ms: {corner} drawn");
            }
            for [neighbour, ..] in others {
                let r = rect_of(&app, neighbour);
                for p in [r.center(), r.min, r.max] {
                    assert!(!clip.contains_point(p), "{age} ms: on a neighbour");
                }
            }
        }
        assert_eq!(outer_nodes(&mut app), outer, "{age} ms");
        app.update();
    }
    assert!(
        past_its_edge,
        "at 2× the flash reaches past its card's edge"
    );
    assert!(others_breathe, "the cards at rest float");
    assert!(
        floats_after,
        "and the placed one does once its impact is over"
    );
}

/// AP.14: under `ReducedMotion` every card is at rest from the frame it's
/// drawn, through the entrance and a placement: risen, shown, its bezel
/// still and no flash.
#[test]
fn reduced_motion_holds_every_card_at_rest() {
    use bevy::time::TimeUpdateStrategy;
    let seven = full("Eruptor").reduce([PortalChange::Cleared {
        slot: SlotIndex::new(7).unwrap(),
    }]);
    let mut app = portal_app(1920, 1080, WindowMode::Tv, seven.clone());
    app.insert_resource(ReducedMotion(true))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            20,
        )));
    let at_rest = |app: &mut App, step: &str| {
        let faces = faces(app);
        for (slot, (_, _, transform, shown)) in &faces {
            assert_eq!(*transform, UiTransform::IDENTITY, "{step}: slot {slot}");
            assert!(shown, "{step}: slot {slot}");
        }
        let world = app.world_mut();
        let mut bezels = world.query_filtered::<&UiTransform, With<card_motion::Floats>>();
        assert!(
            bezels.iter(world).all(|t| *t == UiTransform::IDENTITY),
            "{step}"
        );
        let mut flashes = world.query_filtered::<&Visibility, With<card_motion::ImpactFlash>>();
        assert!(
            flashes.iter(world).all(|v| *v == Visibility::Hidden),
            "{step}"
        );
        (faces.len(), flashes.iter(world).count())
    };
    for frame in 0..10 {
        app.update();
        assert_eq!(at_rest(&mut app, &format!("entrance {frame}")), (7, 0));
    }
    *app.world_mut().resource_mut::<PortalCards>() = seven.reduce([PortalChange::Placed {
        slot: SlotIndex::new(7).unwrap(),
        placement: placement("Jet-Vac", None),
    }]);
    for frame in 0..10 {
        app.update();
        assert_eq!(at_rest(&mut app, &format!("placement {frame}")), (8, 1));
    }
}

/// The portal with the turntables (AE.13) over
/// `test_support::turntable::extract`'s shelf, `slots` placed with its modelled
/// figure 1; the extract's folder, which the loads read, beside it.
fn turntable_app(
    width: u32,
    height: u32,
    mode: WindowMode,
    slots: &[u8],
) -> (App, tempfile::TempDir) {
    use crate::test_support::turntable::{add_turntable, extract, placed};
    let tmp = tempfile::tempdir().unwrap();
    let shelf = extract(tmp.path());
    let cards = PortalCards::default().reduce(slots.iter().map(|s| placed(*s, 1, "One")));
    let mut app = portal_app(width, height, mode, cards);
    add_turntable(&mut app);
    let (tx, feed) = crate::turntable::shelf_channel();
    tx.send(std::sync::Arc::new(shelf)).unwrap();
    app.insert_resource(feed)
        .insert_resource(ReducedMotion(true));
    crate::test_support::turntable::settle(&mut app);
    (app, tmp)
}

/// Each plate's slot, rect, the image it shows and its card's rect.
fn plates(app: &mut App) -> Vec<(u8, Rect, Option<AssetId<Image>>)> {
    let world = app.world_mut();
    let mut q = world.query::<(
        &CardPlate,
        &ComputedNode,
        &UiGlobalTransform,
        Option<&ImageNode>,
    )>();
    let mut out: Vec<_> = q
        .iter(world)
        .map(|(p, n, t, i)| (p.slot, node_rect(n, t), i.map(|i| i.image.id())))
        .collect();
    out.sort_by_key(|p| p.0);
    out
}

/// AE.13: a full portal's turning models each draw inside their own card
/// and the TV's inset, at 1080p, 4K and the desktop window.
#[test]
fn eight_turntable_images_sit_inside_their_cards_and_the_inset() {
    for (w, h, mode) in [
        (1920, 1080, WindowMode::Tv),
        (3840, 2160, WindowMode::Tv),
        (1280, 720, WindowMode::Desktop),
    ] {
        let (mut app, _extract) = turntable_app(w, h, mode, &[0, 1, 2, 3, 4, 5, 6, 7]);
        let laid = measure(&mut app);
        let safe = safe_area(w, h, mode);
        let turntables = app.world().resource::<Turntables>();
        let images: Vec<_> = (0..8).map(|s| turntables.image(s).unwrap().id()).collect();
        let plates = plates(&mut app);
        assert_eq!(plates.len(), 8, "{w}x{h}");
        for (slot, rect, image) in &plates {
            assert_eq!(
                *image,
                Some(images[usize::from(*slot)]),
                "slot {slot} shows its model"
            );
            let card = laid.cards[usize::from(*slot)].1;
            assert!(
                rect.width() > 0.0 && contains(card, *rect),
                "slot {slot}: {rect:?} in {card:?}"
            );
            assert!(
                contains(safe, *rect),
                "slot {slot}: {rect:?} leaves {safe:?}"
            );
            for (other, r, _) in &plates {
                assert!(
                    other == slot || r.intersect(*rect).is_empty(),
                    "{slot} and {other}"
                );
            }
        }
        // Drawn at the largest plate's pixels, so the UI never scales an
        // image up; the plates lay out a pixel apart at most by where
        // they sit.
        let size = app.world().resource::<crate::turntable::PlateSize>().0;
        let sides = plates.iter().flat_map(|(_, r, _)| [r.width(), r.height()]);
        let largest = sides.clone().fold(0.0, f32::max);
        assert_eq!(size.x as f32, largest.ceil(), "{w}x{h}");
        assert!(
            sides.clone().all(|s| size.x as f32 - s <= 1.0),
            "{w}x{h}: {size} for {:?}",
            sides.collect::<Vec<_>>()
        );
        let assets = app.world().resource::<Assets<Image>>();
        for id in &images {
            assert_eq!(assets.get(*id).unwrap().size(), size);
        }
    }
}

/// AE.13: leaving the portal and coming back respawns every card, but no
/// turntable: the new card shows the scene that was already turning.
#[test]
fn a_ui_rebuild_keeps_the_turntable_and_shows_it_again() {
    let (mut app, _extract) = turntable_app(1920, 1080, WindowMode::Tv, &[0, 5]);
    let scenes = |app: &App| {
        let t = app.world().resource::<Turntables>();
        (t.stage_entities(0).unwrap(), t.stage_entities(5).unwrap())
    };
    let before = scenes(&app);
    let shown = plates(&mut app);
    let card = cells(&mut app)[&0].1;

    app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
    frames(&mut app, 3);
    assert!(plates(&mut app).is_empty(), "the portal's gone");
    app.world_mut().resource_mut::<Current>().screen = TvScreen::Portal {
        game: "Skylanders: Giants".into(),
    };
    frames(&mut app, 3);
    assert_ne!(cells(&mut app)[&0].1, card, "the card was respawned");
    assert_eq!(scenes(&app), before, "the scenes weren't");
    let again = plates(&mut app);
    let images = |p: &[(u8, Rect, Option<AssetId<Image>>)]| {
        p.iter().map(|(s, _, i)| (*s, *i)).collect::<Vec<_>>()
    };
    assert_eq!(images(&again), images(&shown));
    assert!(again.iter().all(|(_, _, i)| i.is_some()));

    // A placement in another slot respawns that cell, not slot 1's model.
    let cards = app
        .world()
        .resource::<PortalCards>()
        .clone()
        .reduce([crate::test_support::turntable::placed(2, 3, "Three")]);
    app.insert_resource(cards);
    crate::test_support::turntable::settle(&mut app);
    assert_eq!(scenes(&app), before);
    let t = app.world().resource::<Turntables>();
    assert_eq!(
        t.card_state(2),
        Some(crate::turntable::CardState::Portrait),
        "the damaged one keeps its portrait"
    );
    let third = plates(&mut app).into_iter().find(|p| p.0 == 2).unwrap();
    assert_eq!(third.2, None, "no portrait in the made-up roster, no model");
}

/// AE.25: the start's pass's screen, its title, lines and bar inside
/// the inset at 1080p and 4K on the TV and in the desktop window, the
/// line and the bar following the count of keys ready.
#[test]
fn the_preparing_screen_lays_out_inside_the_inset_and_counts_up() {
    for (width, height, mode) in [
        (1920, 1080, WindowMode::Tv),
        (3840, 2160, WindowMode::Tv),
        (1280, 720, WindowMode::Desktop),
    ] {
        let mut app = portal_app(width, height, mode, PortalCards::default());
        let mut prepare = Prepare::new(true);
        prepare.set_progress(44, 88);
        app.insert_resource(prepare);
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Preparing;
        let laid = measure(&mut app);
        let safe = safe_area(width, height, mode);
        assert!(laid.texts.len() >= 3, "{width}x{height}: {:?}", laid.texts);
        for t in &laid.texts {
            assert!(
                contains(safe, *t),
                "{width}x{height} {mode:?}: {t:?} outside {safe:?}"
            );
        }
        assert!(laid.cards.is_empty() && laid.empties.is_empty());
        let drawn = texts(&mut app);
        assert!(drawn.contains(&"44 of 88 ready".to_string()), "{drawn:?}");
        assert_eq!(titles(&mut app), ["GETTING READY"]);
        let world = app.world_mut();
        let bars: Vec<u8> = world
            .query::<&PrepareBar>()
            .iter(world)
            .map(|b| b.0)
            .collect();
        assert_eq!(bars, [50]);
        // The count moves on: patched in place, no new screen.
        let root = roots(&mut app);
        app.world_mut()
            .resource_mut::<Prepare>()
            .set_progress(88, 88);
        app.update();
        assert!(texts(&mut app).contains(&"88 of 88 ready".to_string()));
        let world = app.world_mut();
        let bars: Vec<u8> = world
            .query::<&PrepareBar>()
            .iter(world)
            .map(|b| b.0)
            .collect();
        assert_eq!(bars, [100]);
        assert_eq!(roots(&mut app), root);
    }
    assert_eq!(prepare_line(0, 0), "");
    assert_eq!(prepare_percent(3, 0), 0);
    assert_eq!(prepare_percent(1, 3), 33);
}

/// AE.25: the pass's card is drawn by its own camera and counted by
/// nothing that counts cards: no card, plate, pip or empty frame on the
/// TV's camera, the portal's own cards untouched beside it.
#[test]
fn the_sample_card_draws_offscreen_and_is_counted_nowhere() {
    let mut app = portal_app(1920, 1080, WindowMode::Tv, full("Eruptor"));
    let laid = measure(&mut app);
    let before = (laid.cards.len(), laid.pips.len());
    let camera = app.world_mut().spawn(Camera2d).id();
    let plate = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::default());
    let world = app.world_mut();
    let assets = world.remove_resource::<TvAssets>().unwrap();
    let theme = world.resource::<TvTheme>().clone();
    let root = {
        let mut commands = world.commands();
        spawn_sample_card(
            &mut commands,
            &assets,
            &theme,
            camera,
            UVec2::splat(448),
            plate.clone(),
            Duration::ZERO,
        )
    };
    world.flush();
    world.insert_resource(assets);
    frames(&mut app, 3);
    let world = app.world_mut();
    let counted = (
        world.query::<&CardSlot>().iter(world).count(),
        world.query::<&Pip>().iter(world).count(),
    );
    assert_eq!(counted, before);
    assert_eq!(world.query::<&EmptySlot>().iter(world).count(), 0);
    assert_eq!(
        world
            .get::<bevy::ui::UiTargetCamera>(root)
            .map(|c| c.entity()),
        Some(camera)
    );
    assert_eq!(world.query::<&CardPlate>().iter(world).count(), 8);
    // Its face, bezel and plate are there, on its camera alone.
    let mut faces = world.query_filtered::<&bevy::ui::ComputedUiTargetCamera, With<CardFace>>();
    let on: Vec<Option<Entity>> = faces.iter(world).map(|c| c.get()).collect();
    assert_eq!(
        on.iter().filter(|c| **c == Some(camera)).count(),
        1,
        "{on:?}"
    );
    let mut images = world.query::<&ImageNode>();
    assert!(images.iter(world).any(|i| i.image == plate));
}
