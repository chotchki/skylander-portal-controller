# Spec Delta

## ADDED Requirements

### Requirement: A placement tint the scripts move
A placement tint other than neutral SHALL multiply a part's colour in linear light by each channel's ratio to neutral raised to 2.2, as the original's multiply of stored values comes out decoded, its alpha used as stored; a neutral placement tint SHALL scale the part as the decoding requirement says.

#### Scenario: Lava going black
- **WHEN** the eruption's lava reaches its dark tint of (34, 20, 20)
- **THEN** it draws at about 5 % of its neutral red and under 2 % of its green and blue in linear light, not at the 27 % and 16 % those values give as stored

### Requirement: A level draws the game's glow
In a level, each part with a glow attribute (the level's own, its props', what its scripts spawn, the hero's) SHALL lay its glow mask where it's the nearest surface drawn with depth, after everything else has drawn, and the game's glow chain SHALL run on the level's frame with that mask, as it runs on a card's.

#### Scenario: Eruptor's ball glows and a wall hides it
- **WHEN** Eruptor throws at the start
- **THEN** his body and the ball in flight show the glow's halo, as on his card
- **WHEN** a part without a glow attribute that writes depth stands in front of a glowing one
- **THEN** no glow shows through it

### Requirement: The point lights the scripts light
A level's parts and its hero SHALL be lit by up to three point lights a frame: the level's placed ones and those the scripts' point-light manager hands out, each where the manager hangs it, raised by its vertical offset, with the values it copies on, the three nearest the hero taken when more are lit.

#### Scenario: The ball and the eruption light the clearing
- **WHEN** Eruptor throws
- **THEN** from the step after the ball appears until it's gone an orange-brown light rides on it
- **WHEN** he erupts
- **THEN** an orange light 22 ft over the eruption lights him and the ground, fading out over 1.5 s

### Requirement: What the scripts spawn draws its particles
An emitter the scripts spawn SHALL make particles from the disc's generators as they say (rate, most alive, life, spawn volume, speed and spread, drag and gravity, colour, size and spin over life, texture), drawn unlit as quads facing the camera and ribbons along its path, added or blended, glowing where a generator blooms; taken off, it SHALL stop and its particles live out their lives. Their random numbers SHALL never be the scripts'.

#### Scenario: The throw's trail and the eruption's sparks
- **WHEN** Eruptor throws
- **THEN** a flash shows at his hand and flames trail the ball until it lands, then a burst and smoke where it lands
- **WHEN** he erupts
- **THEN** sparks, smoke and flames rise from the eruption, and the scripts' generator has drawn as many numbers as it would with no particle drawing
