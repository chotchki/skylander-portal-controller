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
Pushed from rest, a hero SHALL speed up at 160 ft/s² (48.77 m/s²) × S to his top speed for that strength, about 0.13 s whatever S; a speed over a weaker push's top SHALL hold. Let go, his walk's speed across the ground SHALL fall by 80 ft/s² (24.38 m/s²) × the step two frames back, its vertical part held, his speed along the ground their root sum square, and once it's gone he SHALL stop dead and stay; over 21.5 ft/s plus 1e-4 ft/s it SHALL fall so with the stick held too.

#### Scenario: From rest
- **WHEN** the stick is pushed fully over with Eruptor at rest on level ground
- **THEN** he walks at 90 % of top speed or more from 0.115-0.119 s of game time on, and covers 2.88-2.90 m in the first half second

#### Scenario: Letting go
- **WHEN** the stick is let go with Eruptor at top speed on level ground
- **THEN** he stops 0.81-0.85 m further along the way he was walking, 0.26-0.27 s of game time later, and stays put

#### Scenario: Let go on a slope
- **WHEN** the stick is let go with Eruptor at top speed on the 5.3° rise ahead of the start or the corridor's 8.5° and 17.3° slopes, walking down, across or up them
- **THEN** he slides 0.81-1.05 m in 0.23-0.27 s of game time, his speed along the ground falling in shrinking steps to what his walk's vertical part leaves, and stops dead from it: 1.32-2.54 m/s walking down, 0.01-0.52 m/s across, 1.21-2.73 m/s up, each uphill slide leaving the ground for a frame

#### Scenario: Over top speed across the ground
- **WHEN** Eruptor, his walk crept past 21.5 ft/s plus 1e-4 ft/s and steering, walks at full stick off the gate's door onto the 9° slope at the corridor's foot (AF.11's walk)
- **THEN** for one frame he walks 0.41-0.45 m/s slower, 80 ft/s² × the step, and is back at 6.553 m/s the next

### Requirement: Turning and hard reversals
A hero SHALL turn to face the way he walks, the short way, at 1440°/s × S for Eruptor. While his walk's speed along the ground is at most 21.5 ft/s plus 1e-4 ft/s the walk SHALL take each new direction of the stick at once at that speed, so a hard reversal sends him straight back; over it, it SHALL steer, gaining 160 ft/s² × S along the stick over the step three frames back, cut to the larger of 21.5 ft/s × S and the speed it had. Let go mid-turn, his facing SHALL stay put while he slows.

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

#### Scenario: A swing at the gate
- **WHEN** AF.11's walk swings the stick 90° at full stick on the ground falling away ahead of the gate
- **THEN** in each frame his walk sits over 21.5 ft/s plus 1e-4 ft/s it steers, 6-8° a frame, and a walk that stays over meets the door's face; under it, or cut back to 21.5 ft/s by a frame off the ground, it turns at once and he walks free

### Requirement: Walls
A wall or a solid prop SHALL hold a hero 5 mm off its face, never lifting him, and slide him along it at his walking speed times 1 − 0.1235 × (1 − sin θ), θ the push's angle from head-on, from 2° on; let go, he slows along it as on open ground. Nearer head-on he SHALL creep along it at the push's share of his speed (0.12 m/s at 1.06°) and, within 0.15°, stop dead the frame he meets it and stay while the stick holds.

#### Scenario: Along a wall
- **WHEN** Eruptor walks at full stick into the wall behind the start at 2°, 5°, 10°, 20°, 45° and 70° from head-on
- **THEN** he slides along it at 5.75-5.79, 5.79-5.85, 5.88-5.90, 6.015-6.018, 6.316 and 6.503-6.504 m/s, never more than 1.1 mm higher than where he met it

#### Scenario: Half stick along a wall
- **WHEN** Eruptor walks at half stick into the wall behind the start at 45.7° from head-on
- **THEN** he slides along it at 3.183 m/s, 0.965 of his 3.297 m/s

#### Scenario: Head-on
- **WHEN** Eruptor walks at full stick straight into the wall behind the start, or into the gate's door
- **THEN** he stops dead the frame he reaches it, 5 mm off its face (at the door his pivot 1.4781 m from its middle), and stays there while the stick holds

#### Scenario: Into the corner by the gate
- **WHEN** the stick and frame steps of an oracle recording walk Eruptor into the corner by the gate
- **THEN** he leaves it the way that recording's Eruptor did, sliding along the door's face or turning free

### Requirement: Slopes
A hero SHALL walk the opening area's slopes (to 17.3°) at his walk's speed along the ground, each push tilting the walk toward the ground's slope along the stick. A frame off the ground within 0.4 ft (12.2 cm) above it SHALL add 10 ft/s downward to his walk, the whole cut to 21.5 ft/s if over; landed, he SHALL walk at its whole speed, √(v² + 10²) ft/s from v, held until let go. Ground steeper than 45° SHALL NOT hold a hero; up to 45° it can.

#### Scenario: Up and down the corridor
- **WHEN** Eruptor walks at full stick up the corridor east of the clearing, over slopes of 3.4° to 17.3°, and back down at half stick
- **THEN** his speed along the ground is his walking speed on every slope, 6.553 m/s up and 3.284 m/s down, within 1.1 % but for the three frames after the snap sets him down at the 13.3° crest (to 7 % under), until the first frame he leaves the ground

#### Scenario: Half stick over the dip ahead of the start
- **WHEN** Eruptor walks at half stick from the start over the edge of the 1.4° dip ahead of it
- **THEN** he leaves the ground for one frame there and walks on at 4.41-4.49 m/s, the root sum square of his 3.18-3.28 m/s and that frame's 10 ft/s (3.05 m/s), until the stick is let go

#### Scenario: The dip's edge at six strengths
- **WHEN** Eruptor walks from the start over the edge of the dip ahead of it at 0.35, 0.49, 0.60, 0.76, 0.89 and all of the stick
- **THEN** he leaves the ground for one frame there and walks on at 3.809, 4.415, 5.006-5.007, 5.826-5.827, 6.553 and 6.553 m/s from 2.269, 3.179, 3.957, 4.951, 5.861 and 6.553 m/s, the last two cut back to full stick's top

#### Scenario: A steep bank
- **WHEN** Eruptor walks onto ground sloping at 50°
- **THEN** he slides back to its foot, never higher up it than the original's Eruptor gets

#### Scenario: The cliff under the ledge route
- **WHEN** Eruptor goes over the ledge route's edge into the corridor and down the cliff below it
- **THEN** its faces hold him up to 44.92° and none from 45.04°, which slide him down

### Requirement: Steps and drops
A hero SHALL ride 5 mm above level ground, and 10 mm above a slope once the snap has set him there. Where the ground falls away he SHALL be held 10 mm above it, set back there in one frame from as high as 189 mm; a frame without support SHALL set him back 5-10 mm up. He SHALL climb no step the original's same character doesn't: the opening area's static collision has no riser between 4 cm and 2.36 m and no drop between 7 cm and 2.5 m bar the ledge route's edge (An edge with nothing below).

#### Scenario: The ground falling away
- **WHEN** Eruptor walks at full stick from the start toward the gate, the ground falling away 0.15°-1° under him
- **THEN** he rides up to 15 mm above it and is set back 10 mm above it, or 5 mm after a frame without support

#### Scenario: On the corridor's slopes
- **WHEN** Eruptor walks at full stick up the corridor east of the clearing and back down at half stick, over slopes of 3.4° to 17.3°
- **THEN** he rides 10 mm above them, up and down, but for the frames after a landing, which set him 5 mm up

#### Scenario: Snapped back down
- **WHEN** Eruptor walks at half stick down the corridor east of the clearing against its north wall, the wall nearer him than the ground
- **THEN** he drifts up to 171-189 mm above the ground and is put back 10 mm above it in one frame

### Requirement: An edge with nothing below
A hero SHALL stop dead the frame a point 1.2105 m (4 ft as read) ahead along his walk, with no wall between, would be over no ground within 225 ft (68.58 m) below: his walk across the ground drops to 0 there whatever the angle, held or let go, with no slide along the edge, and he stays while the stick pushes on. Slower than 2 ft/s (0.61 m/s) the point SHALL be half his width and 1 ft ahead along the stick (3 ft for Eruptor). Where ground lies within 225 ft below, he SHALL walk off and fall.

#### Scenario: Head-on at the bridge's foot
- **WHEN** Eruptor walks at full stick straight at the edge by the bridge's foot, or at the sheep's ground's rim or the ledge route's edges
- **THEN** he stops dead from top speed with 1.12-1.18 m of ground left ahead of him, supported, only the ground in his contacts, and stays there while the stick holds and once it's let go

#### Scenario: Along an edge
- **WHEN** Eruptor walks at full stick at an edge with nothing below 21°, 30°, 39°, 47-49°, 60° and 82-83° from head-on
- **THEN** he stops dead with 1.11-1.21 m left ahead along his walk, his pivot 1.05-1.06, 0.97-1.02, 0.90, 0.74-0.78, 0.56-0.60 and 0.14-0.17 m from the edge, and never slides along it

#### Scenario: Let go short of it
- **WHEN** Eruptor walking at full stick at the edge has the stick let go 1.4-1.85 m short of it
- **THEN** his slide stops dead from 4.6-5.8 m/s with 1.16-1.20 m left ahead of him

#### Scenario: Part stick
- **WHEN** Eruptor walks at the edge at half or 0.6 of the stick, or creeps at 0.088 of it (0.58 m/s)
- **THEN** at half and 0.6 he stops dead 1.13-1.21 m short, then creeps on a 5-8 mm step every third frame and stops for good 0.896-0.911 m short; creeping, he stops there at once; at 0.9 of the stick he stays at the first stop

#### Scenario: Off the ledge route
- **WHEN** Eruptor walks at full stick off the ledge route's edge over the corridor, its upper slope 2-3.5 m below, 17.5° and 33.8° from head-on
- **THEN** he walks over the edge at 6.553 m/s without slowing, falls 3.60 and 2.72 m onto the slope in 0.40 and 0.36 s, and walks on

#### Scenario: A wall at the level's edge
- **WHEN** Eruptor walks at full stick into the wall behind the start, past which the level holds nothing
- **THEN** he walks up to it and slides along it as he does along any wall, never stopped short of it

#### Scenario: Where the ledge route lets him off
- **WHEN** Eruptor walks at full stick at the ledge route's edge over the corridor, head-on, 17°-70° east of head-on or 34°-64° west of it, or creeps at it under 2 ft/s
- **THEN** head-on and east of head-on he walks off anywhere along it from its west end, and 42° west of it only from 0.65 m along it, where his pivot crosses the edge before the next frame's probe stops him dead over it, so he slips down the cliff for 0.9-1.0 s with his walk held at 0; creeping 5.5° east of head-on he walks off 0.78 m along it

### Requirement: He leans with the ground
A hero SHALL turn his up axis toward the normal of the ground under him at 120°/s of game time until he's within half a degree of it, the last frame taking what's left or stopping short by up to 0.48°, so a change under 2° at 1/60 s is done in one frame.

#### Scenario: Onto a slope
- **WHEN** Eruptor walks at full stick from the 3.5° ground east of the clearing onto its 11.3° slope
- **THEN** his tilt turns toward the new slope at 120°/s, 1.9-2.1° a frame, and stays 0.26-0.31° short of it after six frames

### Requirement: Falling and landing
Off the ground a hero SHALL fall faster by 136.21 ft/s² (41.52 m/s²) over the step two back, up to 120 ft/s as read. In the air his walk SHALL steer, gaining 160 ft/s² × S along the stick over the step three back, cut across the ground to the larger of 21.5 ft/s × S and what it had, and slow that part as on the ground but at 50 ft/s². He SHALL land without bouncing and, from a last frame in the air begun over 0.4 ft up, slide a frame down the fall line, then walk the stick's way at that speed.

#### Scenario: Falling off the ledge route
- **WHEN** Eruptor walks at full stick off the ledge route's edge into the corridor, head-on, up to 64° west of it or up to 70° east of it, and keeps the stick held
- **THEN** in every frame nothing touches him his downward speed grows by 136.21 ft/s² times the step two frames back, his walk staying at 21.5 ft/s times the stick's strength along it, and he lands 1.5-3.9 m down after 0.27-0.48 s, 10.0-17.6 m/s down unless a face slows him first (one of the two at 64° west, from 15.2 m/s to 0.7)

#### Scenario: Steering in the air
- **WHEN** the stick is swung 90° or reversed 0.1 s into a fall from the ledge route, at full or half strength
- **THEN** his walk turns toward it 6.0-7.7° a frame at full stick and 3.1-3.9° at half over the first three frames, less as it nears the stick's way, keeping 6.553 m/s but for a frame at 6.30-6.33 m/s each time the walk creeps past 21.5 ft/s + 1e-4 (3 in the left swing's two replays); reversed, he slows 0.7-0.9 m/s a frame along the way he was going

#### Scenario: Let go in the air
- **WHEN** the stick is let go 0.1 s into a fall from the ledge route
- **THEN** he slows along the way he was going by 50 ft/s² times the step two frames back, 0.23-0.28 m/s a frame, his downward speed unchanged

#### Scenario: Landing on the corridor's slope
- **WHEN** Eruptor lands on the corridor's 13°-21° slope from 10-19 m/s down with the stick held, not into the cliff
- **THEN** he never rises; the next frame he slides 1.1-4.7 m/s straight down the slope (in all but one of 32 landings, whose last frame in the air began 0.133 ft up and which walked on at 21.5 ft/s at once), and the frame after walks the stick's way at that speed, back at full stick's top 4-8 frames after landing or, landing faster than a weaker push allows, holding that speed until it's let go

#### Scenario: Off where the drop is short
- **WHEN** Eruptor walks at full stick off the ledge route's edge near its east end, where the corridor's slope is 0.6 m or less below it
- **THEN** he walks down onto the slope supported but for one to three frames, never under 6.49 m/s, with no slide down the slope

#### Scenario: Let go at the edge
- **WHEN** the stick is let go 0.3 or 0.6 m short of the ledge route's edge at its west end, Eruptor at full stick
- **THEN** he slides over it and down the cliff below, and in 3 of 4 runs stops dead on it 0.4-0.9 m below the edge on a 42°-45° face and stays there

#### Scenario: A long drop
- **WHEN** Eruptor falls further than 16.1 m
- **THEN** his downward speed stops at 120 ft/s (36.58 m/s) and he lands without bouncing

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
