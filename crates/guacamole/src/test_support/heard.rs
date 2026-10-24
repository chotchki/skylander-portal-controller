//! What the screen layer heard, for the headless tests.

use bevy::prelude::*;

use crate::input::*;

#[derive(Resource, Default)]
pub(crate) struct Heard(pub(crate) Vec<TvInput>);

pub(crate) fn listen(app: &mut App) {
    app.init_resource::<Heard>().add_systems(Update, record);
}

fn record(mut inputs: MessageReader<TvInput>, mut heard: ResMut<Heard>) {
    heard.0.extend(inputs.read().copied());
}

/// Everything heard since the last call.
pub(crate) fn take(app: &mut App) -> Vec<TvInput> {
    std::mem::take(&mut app.world_mut().resource_mut::<Heard>().0)
}
