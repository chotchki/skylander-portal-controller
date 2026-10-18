//! The cards the TV shows, fed from the native driver's channel: a `PreUpdate`
//! system drains it into [`PortalCards`] (design decision 4), and the portal
//! screen draws one card per occupied slot from it.

use std::fmt::Write as _;

use bevy::prelude::*;
use crossbeam_channel::Receiver;
use skylander_core::{SLOT_COUNT, SlotIndex};
use skylander_rpcs3_control::Placement;
use tracing::info;

use crate::driver::PortalChange;

/// The driver's channel, read only by [`drain_portal`].
#[derive(Resource)]
pub struct PortalFeed(pub Receiver<PortalChange>);

/// What's on each slot, as the TV last heard it. A slot keeps its position:
/// a card's place on screen is its slot, never its rank among the occupied.
#[derive(Resource, Default, Debug, Clone, PartialEq, Eq)]
pub struct PortalCards {
    pub slots: [Option<Placement>; SLOT_COUNT],
}

impl PortalCards {
    /// A load over an occupied slot arrives as one `Placed`, no `Cleared` first.
    fn apply(&mut self, change: PortalChange) {
        match change {
            PortalChange::Placed { slot, placement } => {
                self.slots[slot.as_usize()] = Some(placement);
            }
            PortalChange::Cleared { slot } => self.slots[slot.as_usize()] = None,
        }
    }

    /// The reducer: the card set after `changes`, applied in order.
    pub fn reduce(mut self, changes: impl IntoIterator<Item = PortalChange>) -> Self {
        for change in changes {
            self.apply(change);
        }
        self
    }

    /// One card per occupied slot, in slot order.
    pub fn cards(&self) -> impl Iterator<Item = (SlotIndex, &Placement)> {
        self.slots.iter().enumerate().filter_map(|(i, p)| {
            let slot = SlotIndex::new(i as u8).expect("SLOT_COUNT fits a SlotIndex");
            p.as_ref().map(|p| (slot, p))
        })
    }

    /// One line for the log: `1 Eruptor fire A#ff0000 | 3 Spyro magic -`.
    pub fn summary(&self) -> String {
        let mut line = String::new();
        for (slot, p) in self.cards() {
            if !line.is_empty() {
                line.push_str(" | ");
            }
            let element = p
                .element
                .map(|e| format!("{e:?}").to_lowercase())
                .unwrap_or_else(|| "-".into());
            let owner = p
                .owner
                .as_ref()
                .map(|o| format!("{}{}", o.initial, o.color))
                .unwrap_or_else(|| "-".into());
            let portrait = if p.portrait.is_some() {
                ""
            } else {
                " noportrait"
            };
            let _ = write!(
                line,
                "{} {} {element} {owner}{portrait}",
                slot.display(),
                p.name
            );
        }
        if line.is_empty() {
            line.push_str("empty");
        }
        line
    }
}

/// Touches `PortalCards` only when something arrived, so the portal screen
/// rebuilds on a change and not every frame.
pub fn drain_portal(feed: Res<PortalFeed>, mut cards: ResMut<PortalCards>) {
    let changes: Vec<PortalChange> = feed.0.try_iter().collect();
    if changes.is_empty() {
        return;
    }
    *cards = cards.clone().reduce(changes);
    info!(count = cards.cards().count(), cards = %cards.summary(), "tv cards");
}

#[cfg(test)]
mod tests {
    use skylander_core::{Element, FigureId};
    use skylander_rpcs3_control::PlacementOwner;

    use super::*;

    fn slot(n: u8) -> SlotIndex {
        SlotIndex::new(n).unwrap()
    }

    fn figure(id: &str, name: &str, owner: Option<(&str, &str)>) -> Placement {
        let figure_id = FigureId::new(id);
        let (toy_type, variant) = figure_id.tag_parts().unwrap();
        Placement {
            figure_id,
            toy_type,
            variant,
            name: name.into(),
            element: Some(Element::Fire),
            portrait: None,
            owner: owner.map(|(id, color)| PlacementOwner {
                profile_id: id.into(),
                initial: id[..1].to_uppercase(),
                color: color.into(),
            }),
        }
    }

    fn placed(n: u8, p: Placement) -> PortalChange {
        PortalChange::Placed {
            slot: slot(n),
            placement: p,
        }
    }

    fn cleared(n: u8) -> PortalChange {
        PortalChange::Cleared { slot: slot(n) }
    }

    fn names(cards: &PortalCards) -> Vec<(u8, String)> {
        cards
            .cards()
            .map(|(s, p)| (s.as_u8(), p.name.clone()))
            .collect()
    }

    fn eruptor() -> Placement {
        figure("000009-0000", "Eruptor", Some(("ann", "#ff0000")))
    }

    fn spyro() -> Placement {
        figure("00001c-0000", "Spyro", Some(("ann", "#ff0000")))
    }

    #[test]
    fn a_placement_shows_one_card_in_its_slot() {
        let cards = PortalCards::default().reduce([placed(0, eruptor())]);
        assert_eq!(names(&cards), [(0, "Eruptor".into())]);
        assert_eq!(cards.summary(), "1 Eruptor fire A#ff0000 noportrait");
    }

    #[test]
    fn a_placement_on_an_occupied_slot_replaces_in_place() {
        let cards = PortalCards::default().reduce([
            placed(0, eruptor()),
            placed(1, eruptor()),
            placed(2, eruptor()),
            placed(1, spyro()),
        ]);
        assert_eq!(
            names(&cards),
            [
                (0, "Eruptor".into()),
                (1, "Spyro".into()),
                (2, "Eruptor".into())
            ]
        );
    }

    #[test]
    fn out_of_order_slots_come_out_in_slot_order() {
        let cards = PortalCards::default().reduce([
            placed(5, spyro()),
            placed(0, eruptor()),
            placed(3, spyro()),
        ]);
        let slots: Vec<u8> = cards.cards().map(|(s, _)| s.as_u8()).collect();
        assert_eq!(slots, [0, 3, 5]);
    }

    #[test]
    fn eight_placements_fill_every_slot() {
        let cards = PortalCards::default().reduce((0..8).rev().map(|n| placed(n, eruptor())));
        let slots: Vec<u8> = cards.cards().map(|(s, _)| s.as_u8()).collect();
        assert_eq!(slots, [0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn a_clear_removes_one_card_and_the_rest_keep_their_slots() {
        let full = PortalCards::default().reduce((0..8).map(|n| placed(n, eruptor())));
        let cards = full.clone().reduce([cleared(2)]);
        let slots: Vec<u8> = cards.cards().map(|(s, _)| s.as_u8()).collect();
        assert_eq!(slots, [0, 1, 3, 4, 5, 6, 7]);
        // Clearing an empty slot changes nothing.
        assert_eq!(cards.clone().reduce([cleared(2)]), cards);
        assert_eq!(
            PortalCards::default().reduce([cleared(4)]),
            PortalCards::default()
        );
        assert_eq!(cards.summary().matches(" | ").count(), 6);
        assert_eq!(full.reduce((0..8).map(cleared)).summary(), "empty");
    }

    #[test]
    fn the_same_figure_by_two_profiles_is_two_cards_each_with_its_owner() {
        let cards = PortalCards::default().reduce([
            placed(
                0,
                figure("000009-0000", "Eruptor", Some(("ann", "#ff0000"))),
            ),
            placed(
                1,
                figure("000009-0000", "Eruptor", Some(("bob", "#0000ff"))),
            ),
        ]);
        let owners: Vec<(u8, String, String)> = cards
            .cards()
            .map(|(s, p)| {
                let o = p.owner.as_ref().unwrap();
                (s.as_u8(), o.initial.clone(), o.color.clone())
            })
            .collect();
        assert_eq!(
            owners,
            [
                (0, "A".into(), "#ff0000".into()),
                (1, "B".into(), "#0000ff".into())
            ]
        );
    }

    #[test]
    fn a_kaos_swap_is_a_clear_then_a_placement_in_the_same_slot() {
        let cards = PortalCards::default()
            .reduce([placed(0, eruptor()), placed(1, eruptor())])
            .reduce([cleared(1), placed(1, spyro())]);
        assert_eq!(names(&cards), [(0, "Eruptor".into()), (1, "Spyro".into())]);
    }

    #[test]
    fn no_owner_and_no_portrait_still_make_a_card() {
        let cards =
            PortalCards::default().reduce([placed(3, figure("000009-0000", "Eruptor", None))]);
        assert_eq!(cards.summary(), "4 Eruptor fire - noportrait");
    }
}
