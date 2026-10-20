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
While the hero moves, the eye SHALL keep its offset from a follow point that closes 8 % of its distance to an aim point every game frame, however long the frame, within 5 ft (faster beyond). The aim point SHALL sit 35 % of the way from his feet to a look-ahead point that trails him as he sets off, leads him by up to 1.44 m at top speed and holds still once he stops, so the camera comes to rest a little ahead of him.

#### Scenario: A walk and a stop
- **WHEN** Eruptor walks at full stick on level ground about the start and then stops
- **THEN** he drifts at most 8.2° off the view's centre line, the eye stays 15.16-17.97 m from his feet, and the camera stops moving 73-83 game frames after he does, resting within 1.5° of dead ahead and 16.34-16.77 m from his feet

#### Scenario: Half stick
- **WHEN** Eruptor walks at half stick on level ground about the start
- **THEN** he drifts at most 5.9° off the view's centre line and the eye stays 15.84-17.57 m from his feet

#### Scenario: A long frame
- **WHEN** a game frame takes 50 ms while the camera settles
- **THEN** the follow point closes 8 % of its distance in it, as in a 16.7 ms frame

### Requirement: Camera switches move the view
When the level switches cameras, the view SHALL move to the new camera over the switch's time in game seconds, speeding up evenly for its first 40 %, steady for 20 % and slowing evenly for its last 40 %, its far clip moving with it; a transition plane crossed or a camera volume entered SHALL switch the same way, and crossing back SHALL switch back. Each frame the stick SHALL walk the hero against the view as that frame shows it.

#### Scenario: The rock slide up the corridor
- **WHEN** Eruptor comes within 32 ft (9.75 m) of the sliding rocks' trigger at the top of the corridor east of the clearing
- **THEN** 2 s later the view moves over 2.00 s to a camera 57 ft (17.37 m) from the follow point and 28° up, aimed 6 ft above it and always facing the same point 58 m north of the start, its far clip from 800 to 1,000 ft

#### Scenario: The stick held through a switch
- **WHEN** the stick is held while the view moves to another camera
- **THEN** in each frame he walks the stick's way against that frame's view, so his walk turns with the view

#### Scenario: Forward after a switch
- **WHEN** the view has switched to another camera and the stick is held forward
- **THEN** the hero walks away from the camera along the direction it now looks across the ground

#### Scenario: Across the transition by the gate and back
- **WHEN** Eruptor walks from the start across the level's `Transition 7-1` plane, past the ledge route's barrels and pot, and back
- **THEN** the view moves to the plane's far camera and back, each along the switch's curve over the plane's own time

### Requirement: Nothing pushes the camera in
Scenery or props between the camera and the hero SHALL NOT move the camera: its eye SHALL keep its camera's offset from the follow point whatever stands between them and the hero, as in the original, whose camera never tests itself against the level.

#### Scenario: Scenery in the way
- **WHEN** Eruptor walks right from the start along the clearing's near rim, the rim and the rope fence between the camera and his middle
- **THEN** the eye stays 55 ft at 32° from the follow point, within 0.1 mm, and rests 16.76 m from his feet once he stops there
