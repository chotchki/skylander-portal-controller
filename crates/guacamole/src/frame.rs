//! The game step's order (openspec AH.59): the original's frame as the
//! oracle recorded it, and the one `FixedUpdate` schedule guacamole runs it
//! by, every fixed-step system in one of [`Frame`]'s sets.
//!
//! What the original does each frame (AH.59's three drives on the O6 build,
//! the scheduler's lists read at every frame's pad read; the decompile only
//! explaining them):
//!
//! 1. Its frame time, then its input context's update: the pads are read
//!    before anything else runs, and the stick moves the hero the frame its
//!    read lands (the walk from the spawn: still at the record that reads the
//!    stick, 0.80 m/s and 1.3 cm on at the next).
//! 2. Its updaters, in the order [`ORIGINAL_UPDATERS`] lists at anchor B, at
//!    the spawn and in the June state alike: the portal's USB, the physics
//!    world's step, the scripts, every actor moved by its velocity, the
//!    camera, then particles, lights, sprites, sound and the screen.
//! 3. In the scripts' updater: the slides, then the scheduler's groups. The
//!    first holds the activators, which every frame of all three drives
//!    held as Eruptor's own minion alone, so his script (his walk off the
//!    stick, his hurt reaction) runs first. Then the awake placements in
//!    the order they woke: [`AWAKE_HEAD`] in that order in every record,
//!    the level's placements after them as they woke, a spawn at the end.
//!    The music controller's tick resets the mover update, which then rejoins
//!    the end, so it runs after every other awake placement (last in 891 of
//!    900, 699 of 706 and 176 of 177 records, the rest behind a push block's
//!    water end or a respawned actor that joined after the reset). Then the
//!    lights and particles, then the asleep placements' activity checks.
//!
//! guacamole's step follows it: [`ORDER`] chains the sets, the scripts' set
//! runs the VM's walk in the scheduler's order (`guac_script`'s crate docs,
//! The tick: the activators' group first, then the rest, each joining its
//! group's end), with the player loop's slices where their placements sit in
//! it ([`crate::player_loop::PlayerLoop::walk`]), and Avian's step in
//! `FixedPostUpdate` is the next step's physics.

use bevy::prelude::*;

/// The fixed step's sets, in [`ORDER`].
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Frame {
    /// The pads as the step reads them (the original's input context, ahead
    /// of every updater): what a test drives the walking pad with, the pads'
    /// count.
    Pads,
    /// The scripts' updater: the slides, the activity rule, the join, then
    /// every placement's tick in the scheduler's order.
    Scripts,
    /// What the scripts took off the world leaves the physics: a broken
    /// breakable's solid, before anything moves.
    Solids,
    /// The actors' updater, the hero first: his move by what his scripts or
    /// the stick left.
    Hero,
    /// The rest of the actors' frame: the sequences on, what the scripts
    /// threw flying, the creatures walked, anything else moving after him.
    Actors,
    /// His locomotion's clips on what his step left (AG.30's stand-in).
    Locomotion,
    /// His attach nodes posed on his clips, and what hangs from them (AH.50).
    Hands,
    /// The viewport's updater: the camera after the actors.
    Camera,
    /// The test hooks' logs of what the step left.
    Record,
}

/// The step's order, chained, so each set runs once the one before has
/// finished.
pub const ORDER: [Frame; 9] = [
    Frame::Pads,
    Frame::Scripts,
    Frame::Solids,
    Frame::Hero,
    Frame::Actors,
    Frame::Locomotion,
    Frame::Hands,
    Frame::Camera,
    Frame::Record,
];

/// The original's updaters as its serial scheduler runs them, by class
/// (AH.59: the same twelve in the same order at anchor B, at the spawn and
/// in the June state), after its input context's read.
pub const ORIGINAL_UPDATERS: [&str; 12] = [
    "tfbPS3HardwareUpdate",
    "tfbPhysicsLinkUpdate",
    "tfbScriptUpdateGame",
    "tfbActorUpdateGame",
    "tfbViewportUpdateGame",
    "tfbParticleUpdateGame",
    "tfbLightUpdateGame",
    "tfbSpriteUpdateGame",
    "tfbSoundUpdate",
    "tfbLightUpdater",
    "ScreenFXupdater",
    "tfbGraphicsUpdater",
];

/// Where each of the original's phases runs in guacamole's step, if it does:
/// the input context's read (`"input"`) and each updater in
/// [`ORIGINAL_UPDATERS`]. The portal's USB has no counterpart (the phone is
/// the portal), the physics world's step is Avian's in `FixedPostUpdate`
/// (the next step's), and what draws is the renderer's.
pub fn set_of(phase: &str) -> &'static [Frame] {
    match phase {
        "input" => &[Frame::Pads],
        "tfbScriptUpdateGame" => &[Frame::Scripts, Frame::Solids],
        "tfbActorUpdateGame" => &[Frame::Hero, Frame::Actors, Frame::Locomotion, Frame::Hands],
        "tfbViewportUpdateGame" => &[Frame::Camera],
        _ => &[],
    }
}

/// The awake placements at the head of the scheduler's list, in its order,
/// in every record of AH.59's three drives: the globals as the original
/// woke them, ahead of every placement of the level. `Bootstrap_Halt_LoadLevel`
/// and `zPortalManager` are on it too; neither runs here.
pub const AWAKE_HEAD: [&str; 13] = [
    "Bootstrap_Halt_LoadLevel",
    "Macros_Animation",
    "Macros_GameSystems",
    "Macros_Music",
    "Macros_Sound",
    "Macros_UI",
    "Spyro_PortalManager",
    "Pause Controller",
    "TRG_Checker",
    "Macros_Achievements",
    "PointLightManager",
    "Spyro_PortalControl",
    "zPortalManager",
];

/// The schedule every plugin with a fixed-step system adds its systems
/// into: without it a set's systems run in no order against another's.
pub struct FramePlugin;

/// [`FramePlugin`] added unless it is: what every plugin with a fixed-step
/// system calls, so the order holds whichever of them an app has.
pub fn ensure(app: &mut App) {
    if !app.is_plugin_added::<FramePlugin>() {
        app.add_plugins(FramePlugin);
    }
}

impl Plugin for FramePlugin {
    fn build(&self, app: &mut App) {
        // [`ORDER`] as a tuple, which is what chains; its test holds the two
        // the same.
        app.configure_sets(
            FixedUpdate,
            (
                Frame::Pads,
                Frame::Scripts,
                Frame::Solids,
                Frame::Hero,
                Frame::Actors,
                Frame::Locomotion,
                Frame::Hands,
                Frame::Camera,
                Frame::Record,
            )
                .chain(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sets the original's phases land in, taken in its order, are
    /// [`ORDER`] less its test hooks' log: the step's order is the measured
    /// one.
    #[test]
    fn the_sets_follow_the_originals_phases() {
        let mut from_original: Vec<Frame> = Vec::new();
        for phase in std::iter::once("input").chain(ORIGINAL_UPDATERS) {
            for &set in set_of(phase) {
                assert!(!from_original.contains(&set), "{set:?} twice");
                from_original.push(set);
            }
        }
        assert_eq!(from_original, ORDER[..ORDER.len() - 1]);
        assert_eq!(ORDER.last(), Some(&Frame::Record));
    }

    /// A system in each set, added in reverse, runs in [`ORDER`] once the
    /// plugin chains them.
    #[test]
    fn a_step_runs_the_sets_in_order() {
        #[derive(Resource, Default)]
        struct Ran(Vec<Frame>);
        let mut app = App::new();
        app.add_plugins(FramePlugin).init_resource::<Ran>();
        for set in ORDER.into_iter().rev() {
            app.add_systems(
                FixedUpdate,
                (move |mut ran: ResMut<Ran>| ran.0.push(set)).in_set(set),
            );
        }
        app.world_mut().run_schedule(FixedUpdate);
        assert_eq!(app.world().resource::<Ran>().0, ORDER);
    }
}
