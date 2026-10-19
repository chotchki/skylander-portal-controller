# Spec Delta

## Purpose

How a hero moves through a level under a player's stick: what the original Skylanders Giants does, as the RPCS3 oracle shows it, written down in our own words.

## ADDED Requirements

### Requirement: The stick walks the hero as seen from the camera
The left stick SHALL walk the hero in the direction it's pushed as seen from the camera: pushed forward he walks away from the camera along the direction it looks across the ground, pushed left he walks to the screen's left at right angles to that. He SHALL face the way he walks.

#### Scenario: Forward at the start
- **WHEN** the stick is held fully forward with Eruptor at the level's start
- **THEN** he walks away from the camera within 0.01° of the direction it looks across the ground (15° there), facing his path within 0.01°

#### Scenario: Left at the start
- **WHEN** the stick is held fully left with Eruptor at rest at the level's start
- **THEN** he walks to the screen's left, at 90° to that direction (105°)

### Requirement: Top speed
With the stick held fully over on level ground, a hero wearing no hat and with no speed from heroic challenges SHALL walk at the top speed the original gives that character in the chapter: 21.5 ft/s (6.553 m/s) for Eruptor.

#### Scenario: Eruptor at full stick
- **WHEN** Eruptor has been walking with the stick fully forward for a second of game time
- **THEN** he covers 6.553 m in each further second of game time, within 1 mm

### Requirement: Speeding up and slowing down
From rest a hero SHALL speed up to his top speed, and with the stick let go slow to a stop, over the same distances and game time as the original's same character, and SHALL NOT drift once he has stopped.

#### Scenario: From rest
- **WHEN** the stick is pushed fully forward with Eruptor at rest
- **THEN** over the first half second of game time he covers the distance the original's Eruptor covers, within the oracle's spread

#### Scenario: Letting go
- **WHEN** the stick is let go with Eruptor at top speed
- **THEN** he stops in the distance the original's Eruptor stops in (within the oracle's spread) and stays put

### Requirement: Turning and hard reversals
A hero SHALL turn toward the stick at the original's rate for that character (1440°/s for Eruptor at full stick), slower with the stick part way over, slowing his walk while he turns. Pushed more than 135° away from the way he's walking, he SHALL stop dead and turn in place before walking off; from rest, pushed more than 15° away from his facing, he SHALL turn in place first.

#### Scenario: A full reversal
- **WHEN** Eruptor walks at top speed and the stick is swung fully the opposite way
- **THEN** he stops dead, turns around on the spot and walks back the way he came

#### Scenario: From rest at the start
- **WHEN** the stick is pushed fully forward with Eruptor at rest at the level's start, which points it exactly 15° from his facing
- **THEN** over his first three frames of game time his heading and speed are the original Eruptor's

#### Scenario: Part stick
- **WHEN** Eruptor turns with the stick pushed half way over
- **THEN** he turns at the original Eruptor's rate for half stick, slower than at full stick

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
