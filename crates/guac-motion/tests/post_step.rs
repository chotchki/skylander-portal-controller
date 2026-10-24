//! AG.70 and AG.71's scenarios on parry: the engine's post-step as read
//! (his lean in the air, with his facing and near the ground, the snap only
//! while his lean turns, a frame begun on the ground ending supported within
//! the snap's reach), a still solid letting him out slowly and sliding him
//! plain, and the edge probe under a solid hanging over an edge and its
//! search through a solid's radius. guacamole runs the same list through
//! its Avian side.

use guac_motion::parry::ParryWorld;
use guac_motion::scenarios::post_step_scenarios;

#[test]
fn the_post_step_scenarios_on_parry() {
    let mut failures = Vec::new();
    let mut ran = 0;
    for scenario in post_step_scenarios() {
        let world = ParryWorld::new(&scenario.bodies).unwrap();
        if let Err(e) = (scenario.check)(&world) {
            failures.push(format!("{}: {e}", scenario.name));
        }
        ran += 1;
    }
    assert_eq!(ran, 8);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
