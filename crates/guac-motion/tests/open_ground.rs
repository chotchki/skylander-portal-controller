//! The hero controller on open level ground (openspec AG.16): hero-movement's
//! The stick walks the hero as seen from the camera, Top speed, Speeding up
//! and slowing down, Turning and hard reversals, No jumping and The same walk
//! at any frame rate, on parry and a synthetic floor of 2 m cells. The cases
//! live in `guac_motion::scenarios` so guacamole runs them on Avian too;
//! numbers are AG.8's, at its own 1/60 s predictions where its recordings'
//! steps vary.

mod common;

use common::on_parry;
use guac_motion::scenarios;

#[test]
fn settled_he_rides_5_mm_up() {
    on_parry(scenarios::settled_he_rides_5_mm_up());
}

#[test]
fn top_speed_is_21_5_ft_s_times_the_strength() {
    on_parry(scenarios::top_speed_is_21_5_ft_s_times_the_strength());
}

#[test]
fn from_rest_and_letting_go() {
    on_parry(scenarios::from_rest_and_letting_go());
}

#[test]
fn the_stick_walks_him_against_the_view() {
    on_parry(scenarios::the_stick_walks_him_against_the_view());
}

#[test]
fn no_gate_either_side_of_15_degrees() {
    on_parry(scenarios::no_gate_either_side_of_15_degrees());
}

#[test]
fn a_full_reversal_goes_straight_back() {
    on_parry(scenarios::a_full_reversal_goes_straight_back());
}

#[test]
fn half_stick_turns_at_720() {
    on_parry(scenarios::half_stick_turns_at_720());
}

#[test]
fn let_go_mid_turn_freezes_his_facing() {
    on_parry(scenarios::let_go_mid_turn_freezes_his_facing());
}

#[test]
fn the_push_and_the_slowing_take_the_steps_before_their_own() {
    on_parry(scenarios::the_push_and_the_slowing_take_the_steps_before_their_own());
}

#[test]
fn no_button_lifts_him() {
    on_parry(scenarios::no_button_lifts_him());
}

#[test]
fn the_same_path_at_30_60_and_144_hz() {
    on_parry(scenarios::the_same_path_at_30_60_and_144_hz());
}
