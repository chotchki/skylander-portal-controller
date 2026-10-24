//! One portal card as drawn (openspec AQ.7): the frame, the portrait or the
//! turning model in its bezel, the name, the element and the owner's pip.
//! The portal screen ([`crate::tv`]) draws one per placement and the start's
//! pass ([`crate::prepare`]) one offscreen, so the card sits below both.

use std::time::Duration;

use bevy::prelude::*;
use skylander_core::Element;
use skylander_rpcs3_control::{Placement, PlacementOwner};

use crate::card_motion::{self, CardFace, Schedule};
use crate::surfaces;
use crate::theme::{Role, Step, TvTheme};
use crate::turntable::CardPlate;
use crate::tv_state::{CardSlot, Pen, Pip, TvAssets};

/// A card's figure name: the larger of 32 and 40 that still fits a long
/// name in two rows of four at 1080p, 4K and the desktop window.
const CARD_NAME: Step = Step::Md;

/// The element as a word ("Fire").
pub fn element_label(element: Element) -> &'static str {
    match element {
        Element::Air => "Air",
        Element::Dark => "Dark",
        Element::Earth => "Earth",
        Element::Fire => "Fire",
        Element::Life => "Life",
        Element::Light => "Light",
        Element::Magic => "Magic",
        Element::Tech => "Tech",
        Element::Undead => "Undead",
        Element::Water => "Water",
    }
}

/// The profile's colour as the store keeps it (`#rgb` or `#rrggbb`); gold if
/// it won't parse, so a pip never goes invisible.
pub fn owner_color(owner: &PlacementOwner, theme: &TvTheme) -> Srgba {
    Srgba::hex(&owner.color).unwrap_or(theme.gold.to_srgba())
}

/// What a card's plate can show: its portrait, and its turntable's image
/// once the model is up, which it shows instead.
pub(crate) struct CardImages {
    pub(crate) portrait: Option<Handle<Image>>,
    pub(crate) model: Option<Handle<Image>>,
}

/// A cell's content box: fills the cell.
fn slot_box() -> Node {
    Node {
        flex_grow: 1.0,
        flex_basis: px(0),
        min_width: px(0),
        min_height: px(0),
        flex_direction: FlexDirection::Column,
        ..default()
    }
}

/// What a slot draws in its box: the edge and corners, the padding and the
/// lines centred, clipping anything that doesn't fit.
pub(crate) fn slot_frame() -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        row_gap: px(10),
        padding: UiRect::all(px(16)),
        border: UiRect::all(px(4)),
        border_radius: BorderRadius::all(px(24)),
        overflow: Overflow::clip(),
        ..default()
    }
}

/// Portrait, name, element and owner pip; no level, XP or gold (AH). The card
/// is §3.3's framed panel edged in gold, the portrait (or the figure's
/// turning model, AE.13) sits in §3.1's bezel ringed in the figure's element,
/// and the pip stays top-right. `CardSlot` is
/// the box the mirror and the tests measure; the card itself is its face, laid
/// over the box, which rises, and the bezel on it floats and flashes
/// (`card_motion`), so neither moves the box.
pub(crate) fn spawn_card(
    cell: &mut ChildSpawnerCommands,
    pen: &Pen,
    slot: u8,
    placement: &Placement,
    images: CardImages,
    motion: Schedule,
) {
    spawn_card_as(cell, pen, Some(slot), placement, images, motion);
}

/// [`spawn_card`], marked as `slot`'s card (`CardSlot`, `CardPlate`, `Pip`)
/// or, with none, marked as nothing: the start's pass's offscreen card.
fn spawn_card_as(
    cell: &mut ChildSpawnerCommands,
    pen: &Pen,
    slot: Option<u8>,
    placement: &Placement,
    images: CardImages,
    motion: Schedule,
) {
    let theme = &pen.theme;
    let mut card = cell.spawn(slot_box());
    if let Some(slot) = slot {
        card.insert(CardSlot(slot));
    }
    card.with_children(|card| {
        card.spawn((
            CardFace(motion),
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(0),
                bottom: px(0),
                ..slot_frame()
            },
            surfaces::card(theme),
        ))
        .with_children(|face| {
            let shown = images.model.or_else(|| images.portrait.clone());
            let (bezel, plate) = surfaces::spawn_bezel(face, theme, placement.element, shown);
            if let Some(slot) = slot {
                face.commands().entity(plate).insert(CardPlate {
                    slot,
                    portrait: images.portrait,
                });
            }
            card_motion::on_bezel(face.commands().entity(bezel), motion, theme);
            face.spawn((
                pen.text(&placement.name, CARD_NAME, Role::Primary),
                Node {
                    max_width: percent(100),
                    ..default()
                },
            ));
            if let Some(element) = placement.element {
                face.spawn(pen.text(element_label(element), Step::Caption, Role::Muted));
            }
            if let Some(owner) = &placement.owner {
                let color = owner_color(owner, theme);
                let mut pip = face.spawn(surfaces::pip(theme, color.into()));
                if let Some(slot) = slot {
                    pip.insert(Pip(slot));
                }
                pip.with_children(|pip| {
                    let ink = theme.ink_on(color);
                    pip.spawn(pen.inked(&owner.initial, Step::Body, ink, Justify::Center));
                });
            }
        });
    });
}

/// A card for no figure on the portal, drawn by `camera` (offscreen, `size`
/// pixels) and nowhere else: the start's pass (AE.25) draws one so the UI
/// pipelines a card's face, bezel, shadows, flash, name and pip draw with
/// are compiled before the first placement. It carries none of the markers
/// the mirror, the layout report and the turntables find a card by.
pub(crate) fn spawn_sample_card(
    commands: &mut Commands,
    assets: &TvAssets,
    theme: &TvTheme,
    camera: Entity,
    size: UVec2,
    plate: Handle<Image>,
    now: Duration,
) -> Entity {
    let pen = Pen::new(assets, theme);
    let figure_id = skylander_core::FigureId::new("000009-0000");
    let (toy_type, variant) = figure_id.subtype_parts().expect("a well-formed figure id");
    let placement = Placement {
        figure_id,
        toy_type,
        variant,
        name: "Getting ready".into(),
        element: Some(Element::Fire),
        portrait: None,
        owner: Some(PlacementOwner {
            profile_id: String::new(),
            initial: "G".into(),
            color: "#da5ad6".into(),
            progress: Default::default(),
        }),
    };
    let images = CardImages {
        portrait: Some(plate),
        model: None,
    };
    commands
        .spawn((
            Node {
                width: px(size.x as f32),
                height: px(size.y as f32),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            bevy::ui::UiTargetCamera(camera),
            Name::new("prepare: a card"),
        ))
        .with_children(|cell| {
            spawn_card_as(
                cell,
                &pen,
                None,
                &placement,
                images,
                Schedule::placement(0, now),
            );
        })
        .id()
}

#[cfg(test)]
mod tests {
    use skylander_core::Element;
    use skylander_rpcs3_control::PlacementOwner;

    use super::*;

    #[test]
    fn owner_colours_parse_both_store_forms_and_fall_back_to_gold() {
        let owner = |c: &str| PlacementOwner {
            profile_id: "p".into(),
            initial: "A".into(),
            color: c.into(),
            progress: Default::default(),
        };
        let theme = TvTheme::default();
        assert_eq!(
            owner_color(&owner("#ff0000"), &theme),
            Srgba::rgb(1.0, 0.0, 0.0)
        );
        assert_eq!(
            owner_color(&owner("#00f"), &theme),
            Srgba::rgb(0.0, 0.0, 1.0)
        );
        assert_eq!(owner_color(&owner("teal"), &theme), theme.gold.to_srgba());
    }

    #[test]
    fn every_element_has_a_word() {
        use Element::*;
        let all = [
            Air, Dark, Earth, Fire, Life, Light, Magic, Tech, Undead, Water,
        ];
        let words: Vec<&str> = all.iter().map(|e| element_label(*e)).collect();
        assert_eq!(
            words,
            [
                "Air", "Dark", "Earth", "Fire", "Life", "Light", "Magic", "Tech", "Undead", "Water"
            ]
        );
        let theme = TvTheme::default();
        assert_ne!(theme.element(Some(Fire)), theme.element(Some(Water)));
    }
}
