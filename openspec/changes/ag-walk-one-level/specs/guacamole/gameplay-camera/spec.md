# Spec Delta

## Purpose

Where the gameplay camera sits and how it follows a hero through a level: what the original Skylanders Giants' camera does, as the RPCS3 oracle shows it, written down in our own words.

## ADDED Requirements

### Requirement: The camera starts behind the hero
With a hero at the level's start, the camera SHALL keep the level's first camera's place: its eye 55 ft (16.764 m) from a point at his feet, 32° above level, behind him along the 15° the view looks across the ground, aimed at 9 ft (2.743 m) above that point, so it looks 23.36° down. The picture SHALL span 53.96° top to bottom at 16:9 (84.29° across), from 0.1 m out to that camera's far clip, 800 ft (243.84 m).

#### Scenario: Eruptor at the start
- **WHEN** Eruptor stands still at the level's start
- **THEN** the eye is 14.2167 m behind his feet along 15° and 8.8836 m above them, within 0.1 mm, looking along 15° and 23.36° down, the point 9 ft above his feet at the picture's centre

### Requirement: The camera follows
While the hero moves, the eye SHALL keep its offset from a follow point that closes 8 % of its distance to an aim point every game frame, however long the frame, within 5 ft; a frame beyond 5 ft SHALL raise that share until it rests on its aim (9 % after one at 5.004 ft). The aim point SHALL sit 35 % of the way from his feet to a look-ahead point that trails him as he sets off, draws ahead as he walks (1.04 m after 1.2 s at top speed, 4.79 m after 4.3 s up the corridor) and stops when he does.

#### Scenario: A walk and a stop
- **WHEN** Eruptor walks at full stick on open level ground about the start and the stick is let go
- **THEN** he drifts at most 8.2° off the view's centre line, the eye stays 15.16-17.97 m from his feet, and the camera stops moving 73-83 game frames after he does, resting within 1.5° of dead ahead and 16.34-16.77 m from his feet

#### Scenario: Half stick
- **WHEN** Eruptor walks at half stick on level ground about the start
- **THEN** he drifts at most 5.9° off the view's centre line and the eye stays 15.84-17.57 m from his feet

#### Scenario: A long frame
- **WHEN** a game frame takes 50 ms while the camera settles
- **THEN** the follow point closes 8 % of its distance in it, as in a 16.7 ms frame

### Requirement: Camera switches move the view
When a script switches cameras, the view SHALL move to the new camera over the switch's time in game seconds, speeding up evenly for its first 40 %, steady for 20 % and slowing evenly for its last 40 %, its far clip moving with it; a camera volume entered SHALL switch the same way (as read). Each frame the stick SHALL walk the hero against the view as the frame before showed it. A shake SHALL move the eye, not the view's angles.

#### Scenario: The rock slide up the corridor
- **WHEN** Eruptor comes within 32 ft (9.75 m) of the sliding rocks' trigger at the top of the corridor east of the clearing
- **THEN** the eye shakes to and fro, mostly across the view, up to 0.682 m off its place, a wave for each of three rocks that falls: the trigger sets two loose, each falling a roll of up to a second later, and a goat on the ledge above, startled once he is within 30 ft of it, sets the third loose as it lands its jump, that wave starting 1.45-2.0 s after he comes within range in four walks; a rock that falls while a wave stands at the new one's peak leaves that wave running

#### Scenario: The switch at the corridor's top
- **WHEN** Eruptor comes within 32 ft (9.75 m) of the sliding rocks' trigger
- **THEN** 2 s later the view moves over 2.00 s to a camera 57 ft (17.37 m) from the follow point and 28° up, aimed 6 ft above it and always facing the same point 54 m north of the start and 10.5 m above it, its far clip from 800 to 1,000 ft, the rock slide's third wave running on into it

#### Scenario: The stick held through a switch
- **WHEN** the stick is held while the view moves to another camera
- **THEN** in each frame he walks the stick's way against the view of the frame before, so his walk turns with the view

#### Scenario: Forward after a switch
- **WHEN** the view has switched to another camera and the stick is held forward
- **THEN** the hero walks away from the camera along the direction it now looks across the ground

### Requirement: A transition plane blends by where he stands
Inside a transition plane's box the view SHALL be its two cameras blended by where the hero's feet were the frame before, not by time: on the camera behind the plane a weight of 3G² − 2G³, G = 0.5 − a / L clamped to [0, 1], a his feet's distance in front of the box's centre along the plane's facing and L the box's length; the eye, the aim point and the far clip each that weighted sum of the two cameras' own. Standing in it, the view SHALL hold where it is.

#### Scenario: Across the transition by the gate and back
- **WHEN** Eruptor, his eruptions having broken the ledge route's barrels and pot, walks west at full stick from the ledge route across `Transition 7-1` (35 ft long, facing 92°, camera 7 in front of it and camera 1 behind) onto the upper walkway, and back east
- **THEN** the view starts toward camera 1 the frame after his feet come within 17.5 ft (5.334 m) of the plane's centre along its facing and is wholly camera 1's the frame after they are 17.5 ft past it, 1.72-1.73 s later at his 6.49-6.55 m/s, its far clip from 1,000 ft to 800 ft, and back the same way, every frame within 4.5e-7 of the blend

#### Scenario: Let go in the plane
- **WHEN** the stick is let go half way across and he stands 0.31-0.58 m in front of the plane's centre
- **THEN** the view holds 0.42-0.46 of the way to camera 1, its far clip unchanged at 908.7-916.2 ft, for the 3.7-3.8 s he stands, and moves on with him when he walks on

#### Scenario: Half stick across the plane
- **WHEN** Eruptor crosses `Transition 7-1` at half stick
- **THEN** the view moves as his feet do, over 2.98-3.00 s going west and 1.73-2.20 s coming back, the walkway's crests lifting a half-stick walk past its 3.28 m/s top (to 4.28 m/s going west, 6.55 m/s coming back) for part of it

### Requirement: A camera coming into use starts on its aim
When a camera comes into use, its follow point SHALL start on its aim point: one new to the level SHALL stay on it until 0.5 s of game time have passed from the frame after, and one coming back into use SHALL close on it from the next frame. A camera out of use SHALL keep its follow point where it was. A transition plane's cameras SHALL be in use while his feet are inside its box.

#### Scenario: Into the plane the first time
- **WHEN** Eruptor walks west into `Transition 7-1`'s box from the ledge route for the first time, camera 1 new to the level
- **THEN** camera 1's part of the blended eye sits on its aim for its first 31-32 frames (0.517-0.534 s of game time), then closes 8 % a frame

#### Scenario: Back into the plane
- **WHEN** he walks back east into the box from the upper walkway, camera 7's follow point having stood still while the view was wholly camera 1's
- **THEN** camera 7's follow point jumps onto its aim the frame his feet come back inside the box (1.33-1.46 m at once) and closes 8 % a frame from the next

### Requirement: Nothing pushes the camera in
Scenery or props between the camera and the hero SHALL NOT move the camera: its eye SHALL keep its camera's offset from the follow point whatever stands between them and the hero, as in the original, whose camera never tests itself against the level.

#### Scenario: Scenery in the way
- **WHEN** Eruptor walks right from the start along the clearing's near rim, the rim and the rope fence between the camera and his middle
- **THEN** the eye stays 55 ft at 32° from the follow point, within 0.1 mm, and rests 16.76 m from his feet once he stops there
