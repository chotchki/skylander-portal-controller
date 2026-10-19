# Spec Delta

## Purpose

How a hero moves through a level under a player's stick: what the original Skylanders Giants does, as the RPCS3 oracle shows it, written down in our own words.

## ADDED Requirements

### Requirement: The stick walks the hero as seen from the camera
The left stick SHALL walk the hero in the direction it's pushed as seen from the camera: pushed forward he walks away from the camera along its view, pushed left he walks to the screen's left at right angles to it. He SHALL face the way he walks.

#### Scenario: Forward at the start
- **WHEN** the stick is held fully forward with Eruptor at the level's start
- **THEN** he walks away from the camera within 0.01° of its view direction, facing his path within 0.01°

#### Scenario: Left at the start
- **WHEN** the stick is held fully left with Eruptor at rest at the level's start
- **THEN** he walks to the screen's left, at 90° to the camera's view direction

### Requirement: Top speed
With the stick held fully over on level ground, a hero SHALL walk at the top speed the original gives that character in the chapter: 21.5 ft/s (6.553 m/s) for Eruptor.

#### Scenario: Eruptor at full stick
- **WHEN** Eruptor has been walking with the stick fully forward for a second of game time
- **THEN** he covers 6.553 m in each further second, within the oracle's spread

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

### Requirement: Walls, slopes, steps and ledges
A wall SHALL slide a hero along it without bouncing or lifting him, keeping him as far from it as the original keeps the same character. Ground steeper than 45° SHALL NOT hold him. He SHALL get up small steps the way the original's hero does, with no fixed step height, and nothing SHALL stop him at a ledge: he walks off it and falls.

#### Scenario: Along a wall
- **WHEN** Eruptor walks into the gate's stone wall at an angle
- **THEN** he slides along it at the speed the original's Eruptor does, without rising

#### Scenario: Off a ledge
- **WHEN** Eruptor walks at a drop at the clearing's edge
- **THEN** he walks straight off it and falls

### Requirement: Falling and landing
Off the ground a hero SHALL fall under the original's gravity, 128 ft/s² (39.0 m/s²), up to its terminal speed of 120 ft/s (36.6 m/s), steering at the air speed the original gives that character, and SHALL land without bouncing.

#### Scenario: A long drop
- **WHEN** Eruptor walks off a high ledge
- **THEN** he falls no faster than 36.6 m/s and lands without bouncing

### Requirement: No jumping
A hero SHALL NOT jump: no button lifts a hero off the ground, as in the original Giants, where no hero can jump.

#### Scenario: The bottom button
- **WHEN** the bottom face button is pressed while Eruptor walks
- **THEN** he stays on the ground

### Requirement: Falling out of the level
A hero who falls out of the level SHALL come back on solid ground at the place the original brings a hero back to after the same fall.

#### Scenario: Off the world's edge
- **WHEN** Eruptor falls past the bottom of the level
- **THEN** he reappears on solid ground where the original's hero reappears

### Requirement: Idle, walk and run
A hero SHALL stand in his own idle animation and move in his own walk or run animation, switching between them at the speeds the original does and stepping at a pace that matches how fast he moves.

#### Scenario: Speeding up
- **WHEN** Eruptor goes from rest to top speed
- **THEN** he goes from his idle into his walk and run as the original's Eruptor does at those speeds, his feet keeping pace with the ground

### Requirement: The same walk at any frame rate
A hero SHALL cover the same ground in the same game time whatever the TV's frame rate, the game stepping 60 times a second, as the original does at full speed.

#### Scenario: A faster display
- **WHEN** the same stick input is given on a 60 Hz display and on a 120 Hz one
- **THEN** the hero ends up in the same place after the same game time
