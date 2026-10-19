# Spec Delta

## Purpose

Where the gameplay camera sits and how it follows a hero through a level: what the original Skylanders Giants' camera does, as the RPCS3 oracle shows it, written down in our own words.

## ADDED Requirements

### Requirement: The camera starts behind the hero
With a hero at the level's start, the camera SHALL sit where the original's does: behind him, looking across the ground the way the level's first camera looks (15° at chapter 1's start), 55 ft (16.764 m) from his feet and 32° above level, aimed at a point 9 ft above him.

#### Scenario: Eruptor at the start
- **WHEN** Eruptor stands still at the level's start
- **THEN** the camera is 14.217 m behind him along that direction and 8.883 m above him, within 1 mm, with Eruptor on the screen's vertical centre line

### Requirement: The camera follows
While the hero walks, the camera SHALL follow him as the original's does, keeping its direction and its distance from him as the original's keeps them, letting him drift off its centre by no more than the original lets him, and easing back onto him once he stops as the original's does.

#### Scenario: A walk and a stop
- **WHEN** Eruptor walks at top speed from the start and then stops
- **THEN** the camera's trail angle, its distance from him and its settle stay inside the original's spread over the same walk

### Requirement: Camera triggers switch the view
When the hero crosses one of the level's camera transition planes, the camera SHALL move to the camera on the side he crossed to, over the time and with the easing the original uses, and crossing back SHALL bring the other back the same way. Walking into one of its camera volumes SHALL switch the view as the original's does. The stick SHALL then walk the hero as seen from the new view.

#### Scenario: Across the transition by the gate and back
- **WHEN** Eruptor walks from the start across the level's `Transition 7-1` plane, about 18 m from it, and back
- **THEN** the view moves to the plane's far camera and back, each over the original's time and easing

#### Scenario: Forward after a switch
- **WHEN** the view has switched to a trigger's camera and the stick is held forward
- **THEN** the hero walks away from the camera along the direction it now looks across the ground

#### Scenario: The stick held through a switch
- **WHEN** the stick is held forward while the view switches
- **THEN** the hero's heading follows the stick the way the original's does through the same switch

### Requirement: Nothing pushes the camera in
Scenery or props between the camera and the hero SHALL NOT move the camera, as in the original, whose camera never tests itself against the level.

#### Scenario: Scenery in the way
- **WHEN** Eruptor walks behind a tall piece of scenery as seen from the camera
- **THEN** the camera keeps its place and distance
