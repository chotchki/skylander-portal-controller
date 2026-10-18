//! The cards the TV shows, fed from the native driver's channel: a `PreUpdate`
//! system drains it into [`PortalCards`] (design decision 4).

use bevy::prelude::*;
use crossbeam_channel::Receiver;
use skylander_core::SLOT_COUNT;
use skylander_rpcs3_control::Placement;

use crate::driver::PortalChange;

/// The driver's channel, read only by [`drain_portal`].
#[derive(Resource)]
pub struct PortalFeed(pub Receiver<PortalChange>);

/// What's on each slot, as the TV last heard it.
#[derive(Resource, Default, Debug, Clone, PartialEq, Eq)]
pub struct PortalCards {
    pub slots: [Option<Placement>; SLOT_COUNT],
}

impl PortalCards {
    pub fn apply(&mut self, change: PortalChange) {
        match change {
            PortalChange::Placed { slot, placement } => {
                self.slots[slot.as_usize()] = Some(placement);
            }
            PortalChange::Cleared { slot } => self.slots[slot.as_usize()] = None,
        }
    }
}

pub fn drain_portal(feed: Res<PortalFeed>, mut cards: ResMut<PortalCards>) {
    for change in feed.0.try_iter() {
        cards.apply(change);
    }
}
