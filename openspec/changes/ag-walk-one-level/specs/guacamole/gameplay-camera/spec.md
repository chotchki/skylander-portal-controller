# Spec Delta

## Purpose

Where the gameplay camera sits and how it follows a hero through a level: what the original Skylanders Giants' camera does, as the RPCS3 oracle shows it, written down in our own words.

## ADDED Requirements

### Requirement: The camera starts behind the hero
With a hero at the level's start, the camera SHALL sit where the original's does: behind him along the view the level gives its first camera, 55 ft (16.76 m) from his feet and 32° above level, aimed at a point 9 ft above him.

#### Scenario: Eruptor at the start
- **WHEN** Eruptor stands still at the level's start
- **THEN** the camera is 14.2 m behind him along its view and 8.9 m above him, within a centimetre, with Eruptor on the screen's vertical centre line

### Requirement: The camera follows
While the hero walks, the camera SHALL follow him as the original's does, keeping its view's direction and distance and letting him drift off its centre by no more than the original lets him (under 5° at top speed), then easing back onto him once he stops.

#### Scenario: A walk and a stop
- **WHEN** Eruptor walks at top speed from the start and then stops
- **THEN** the camera trails him by no more than the original's trails (within the oracle's spread) and settles back within 0.6° of him

### Requirement: Camera zones switch the view
When the hero walks into one of the level's camera zones, the camera SHALL move to that zone's view over the time and with the easing the original uses, and walking back out SHALL bring the view back the same way. The stick SHALL then walk the hero as seen from the new view.

#### Scenario: Into a zone and back
- **WHEN** Eruptor walks into one of chapter 1's camera zones and back out
- **THEN** the view moves to that zone's camera and back, each over the original's time and easing

#### Scenario: Forward after a switch
- **WHEN** the view has switched to a zone's camera and the stick is held forward
- **THEN** the hero walks away from the camera along its new view

### Requirement: Nothing pushes the camera in
Scenery or props between the camera and the hero SHALL NOT move the camera, as in the original, whose camera never tests itself against the level.

#### Scenario: Scenery in the way
- **WHEN** Eruptor walks behind a tall piece of scenery as seen from the camera
- **THEN** the camera keeps its place and distance
