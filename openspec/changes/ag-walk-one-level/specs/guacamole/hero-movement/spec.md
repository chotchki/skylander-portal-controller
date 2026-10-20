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
- **THEN** once up to speed, 0.115-0.135 s of game time after a push from rest, he covers 6.553 m in each second of game time, within 1 mm

#### Scenario: Half stick
- **WHEN** Eruptor walks on level ground with the stick half over
- **THEN** he walks at 3.277 m/s

### Requirement: Speeding up and slowing down
Pushed from rest, a hero SHALL speed up at 160 ft/s² (48.77 m/s²) times the stick's strength until he walks at his top speed for that strength, about 0.13 s whatever the strength; a speed over a weaker push's top SHALL hold. Let go, he SHALL slow at 80 ft/s² (24.38 m/s²) along the way he last walked to a stop, and stay there.

#### Scenario: From rest
- **WHEN** the stick is pushed fully over with Eruptor at rest on level ground
- **THEN** he walks at 90 % of top speed or more from 0.115-0.119 s of game time on, and covers 2.88-2.90 m in the first half second

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
A wall or a solid prop SHALL hold a hero 5 mm off its face, never lifting him, and slide him along it at his walking speed times 1 − 0.1235 × (1 − sin θ), θ the push's angle from head-on, from 2° on; let go, he slows along it as on open ground. Nearer head-on he SHALL creep along it at the push's share of his speed (0.12 m/s at 1.06°) and, within 0.15°, stop dead the frame he meets it and stay while the stick holds.

#### Scenario: Along a wall
- **WHEN** Eruptor walks at full stick into the wall behind the start at 2°, 5°, 10°, 20°, 45° and 70° from head-on
- **THEN** he slides along it at 5.75-5.79, 5.79-5.85, 5.88-5.90, 6.015-6.018, 6.316 and 6.503-6.504 m/s, never more than 1.1 mm higher than where he met it

#### Scenario: Half stick along a wall
- **WHEN** Eruptor walks at half stick into the wall behind the start at 45.7° from head-on
- **THEN** he slides along it at 3.182 m/s, 0.965 of his 3.297 m/s

#### Scenario: Head-on
- **WHEN** Eruptor walks at full stick straight into the wall behind the start, or into the gate's door
- **THEN** he stops dead the frame he reaches it, 5 mm off its face (at the door his pivot 1.4781 m from its middle), and stays there while the stick holds

#### Scenario: Into the corner by the gate
- **WHEN** the stick and frame steps of an oracle recording walk Eruptor into the corner by the gate
- **THEN** he leaves it the way that recording's Eruptor did, sliding along the door's face or turning free

### Requirement: Slopes
A hero SHALL walk up and down the opening area's slopes (to 17.3°) at his walking speed measured along the ground. Where the original's leaves the ground for a frame crossing onto steeper downhill ground, he SHALL too and land 0.88-1.36 m/s faster, a speed over his push's top that holds until he lets go, never past full stick's 6.553 m/s. Ground too steep for the original's hero (45°-46.4° as read) SHALL NOT hold a hero.

#### Scenario: Up and down the corridor
- **WHEN** Eruptor walks at full stick up the corridor east of the clearing, over slopes of 3.4° to 17.3°, and back down at half stick
- **THEN** his speed along the ground is his walking speed on every slope, 6.553 m/s up and 3.284 m/s down until the first frame he leaves the ground

#### Scenario: Half stick over the dip ahead of the start
- **WHEN** Eruptor walks at half stick from the start over the edge of the 1.4° dip ahead of it
- **THEN** he leaves the ground for one frame there and walks on at 4.41-4.49 m/s until the stick is let go

#### Scenario: A steep bank
- **WHEN** Eruptor walks onto ground sloping at 50°
- **THEN** he slides back to its foot, never higher up it than the original's Eruptor gets

### Requirement: Steps and drops
A hero SHALL ride 5 mm above the ground he stands or walks on. Where it falls away under him he SHALL be held 10 mm above it, set back there in one frame from as high as 189 mm, and a frame without support SHALL set him back 5-10 mm above it. He SHALL get up no step the original's same character doesn't: the opening area's static collision has no riser between 4 cm and 2.36 m and no drop between 7 cm and 2.5 m.

#### Scenario: The ground falling away
- **WHEN** Eruptor walks at full stick from the start toward the gate, the ground falling away 0.15°-1° under him
- **THEN** he rides up to 15 mm above it and is set back 10 mm above it, or 5 mm after a frame without support

#### Scenario: Snapped back down
- **WHEN** Eruptor walks at half stick down the corridor east of the clearing against its north wall, the wall nearer him than the ground
- **THEN** he drifts up to 171-189 mm above the ground and is put back 10 mm above it in one frame

#### Scenario: The edge by the bridge's foot
- **WHEN** Eruptor walks at full stick toward the edge with nothing below it by the bridge's foot
- **THEN** he stops dead short of it with nothing but the ground in his contacts, as AG.36 records

### Requirement: He leans with the ground
A hero SHALL turn his up axis toward the normal of the ground under him at 120°/s of game time until he's within half a degree of it, the last frame taking what's left or stopping short by up to 0.48°, so a change under 2° at 1/60 s is done in one frame.

#### Scenario: Onto a slope
- **WHEN** Eruptor walks at full stick from the 3.5° ground east of the clearing onto its 11.3° slope
- **THEN** his tilt turns toward the new slope at 120°/s, 1.9-2.1° a frame, and stays 0.26-0.31° short of it after six frames

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
