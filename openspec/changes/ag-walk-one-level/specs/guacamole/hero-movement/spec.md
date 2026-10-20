# Spec Delta

## Purpose

How a hero moves through a level under a player's stick: what the original Skylanders Giants does, as the RPCS3 oracle shows it, written down in our own words.

## ADDED Requirements

### Requirement: The stick walks the hero as seen from the camera
The left stick SHALL walk the hero in the direction it's pushed as seen from the camera, from his first frame and whichever way he faces: pushed forward he walks away from the camera along the direction it looks across the ground, pushed left at right angles to its left. He SHALL turn to face the way he walks (Turning and hard reversals).

#### Scenario: Forward at the start
- **WHEN** the stick is held fully forward with Eruptor at rest at the level's start
- **THEN** from his first frame he walks within 0.01° of the direction the camera looks across the ground (15° there), facing it

#### Scenario: Left at the start
- **WHEN** the stick is held fully left with Eruptor at rest at the level's start, facing 0°
- **THEN** from his first frame he walks at 105° within 0.01°, his facing reaching 105° in his fifth frame

### Requirement: Top speed
With the stick held over on level ground a hero SHALL walk at the top speed the original gives that character in the chapter, times the stick's strength: for Eruptor, with no hat and no speed from heroic challenges, 21.5 ft/s (6.553 m/s) at full strength, distance over game time.

#### Scenario: Eruptor at full stick
- **WHEN** Eruptor walks on level ground with the stick fully over
- **THEN** once up to speed, within 0.12 s of game time, he covers 6.553 m in each second of game time, within 1 mm

#### Scenario: Half stick
- **WHEN** Eruptor walks on level ground with the stick half over
- **THEN** he walks at 3.277 m/s

### Requirement: Speeding up and slowing down
Pushed from rest, a hero SHALL speed up at 160 ft/s² (48.77 m/s²) times the stick's strength until he walks at his top speed for that strength, about 0.13 s whatever the strength; a speed over a weaker push's top SHALL hold. Let go, he SHALL slow at 80 ft/s² (24.38 m/s²) along the way he last walked to a stop, and stay there.

#### Scenario: From rest
- **WHEN** the stick is pushed fully over with Eruptor at rest on level ground
- **THEN** he walks at 90 % of top speed or more from 0.115-0.119 s of game time on, and covers 2.88-2.91 m in the first half second

#### Scenario: Letting go
- **WHEN** the stick is let go with Eruptor at top speed on level ground
- **THEN** he stops 0.81-0.85 m further along the way he was walking, 0.26-0.27 s of game time later, and stays put

### Requirement: Turning and hard reversals
A hero SHALL turn to face the way he walks, the short way, at 1440°/s times the stick's strength for Eruptor, while his walk takes each new direction of the stick at once at the speed he has: no angle stops him, so a hard reversal sends him straight back at speed. Let go mid-turn, his facing SHALL stay where it was while he slows along the way he last walked.

#### Scenario: A full reversal
- **WHEN** Eruptor walks at top speed and the stick is swung fully the opposite way
- **THEN** from the next frame he walks back the way he came at top speed, without stopping, his facing swinging round in 0.13-0.15 s of game time

#### Scenario: From rest at the start
- **WHEN** the stick is pushed fully forward with Eruptor at rest at the level's start, which points it 15° from his facing
- **THEN** in his first frame he faces 15° and walks off speeding up as from any other angle, as at 13.8° and 16.2° from his facing

#### Scenario: Part stick
- **WHEN** Eruptor turns with the stick half over
- **THEN** his facing turns at 720°/s, half the full stick's rate, while he walks the stick's new way at once

#### Scenario: Let go mid-turn
- **WHEN** the stick is swung 90° at top speed and let go one frame later
- **THEN** he slows to a stop along that one frame's stick, his facing staying where that frame left it

### Requirement: Walls
A wall SHALL slide a hero along it without bouncing or lifting him, at the speed the original's same character slides at the same angle of approach, keeping him as far from it as the original keeps him.

#### Scenario: Along a wall
- **WHEN** Eruptor walks into the gate's stone wall at an angle
- **THEN** he slides along it at the original Eruptor's speed for that angle, without rising

#### Scenario: Into the corner by the gate
- **WHEN** the stick and frame steps of an oracle recording walk Eruptor into the corner by the gate
- **THEN** he leaves it the way that recording's Eruptor did, sliding along the wall or turning free

### Requirement: Slopes
Ground too steep for the original's hero (between 45° and 46.4° as read) SHALL NOT hold a hero: he slides off it, and walking at it SHALL get him no higher up it than the original's same character gets.

#### Scenario: A steep bank
- **WHEN** Eruptor walks onto ground sloping at 50°
- **THEN** he slides back to its foot, never higher up it than the original's Eruptor gets

### Requirement: Steps and drops
A hero SHALL get up the steps the original's same character gets up and no higher ones, with no fixed step height, and SHALL stay on the ground walking down a drop no deeper than the original's stays on. Nothing SHALL stop him at a ledge: past that depth he walks off it and falls.

#### Scenario: A step up
- **WHEN** Eruptor walks into a step the original's Eruptor gets up, then into one taller than any he gets up
- **THEN** he gets up the first and the second stops him

#### Scenario: Off a ledge
- **WHEN** Eruptor walks at a drop at the opening area's edge deeper than the original's Eruptor stays on the ground for
- **THEN** he walks straight off it and falls

### Requirement: He leans with the ground
A hero SHALL tilt to the ground under him as the original's does, turning onto a new slope no faster than the original's same character turns onto it (read as 120°/s).

#### Scenario: Onto a slope
- **WHEN** Eruptor walks from the flat clearing onto sloping ground
- **THEN** his tilt follows the slope at the original Eruptor's rate

### Requirement: Falling and landing
Off the ground a hero SHALL fall under the original's gravity, 128 ft/s² (39.0 m/s²), up to its terminal speed of 120 ft/s (36.6 m/s), steering at the air speed the original gives that character, and SHALL land without bouncing.

#### Scenario: A long drop
- **WHEN** Eruptor walks off the 23.8 m drop at the opening area's edge
- **THEN** he falls no faster than 36.6 m/s and lands on the ground below without bouncing

### Requirement: No jumping
A hero SHALL NOT jump: no button lifts a hero off the ground, as in the original Giants, where no hero can jump.

#### Scenario: The bottom button
- **WHEN** the bottom face button is pressed while Eruptor walks
- **THEN** he stays on the ground

### Requirement: Idle, walk and run
A hero SHALL stand in his own idle animations, drawn from the set the original plays for him standing (not necessarily the one it picks at that moment), and move in his own walk or run animation, switching between them at the speeds the original does, his steps keeping pace with how fast he moves.

#### Scenario: Speeding up
- **WHEN** Eruptor goes from rest to top speed
- **THEN** he goes from his idle into his walk and run as the original's Eruptor does at those speeds, his feet keeping pace with the ground

#### Scenario: Standing
- **WHEN** Eruptor stands at the level's start for a minute
- **THEN** every idle he plays is one the original's Eruptor plays standing there

### Requirement: The same walk at any frame rate
A hero SHALL cover the same ground in the same game time whatever the TV's frame rate, the game stepping 60 times a second, as the original does at full speed.

#### Scenario: A faster display
- **WHEN** the same stick input is given on a 60 Hz display and on a 120 Hz one
- **THEN** the hero ends up in the same place after the same game time
