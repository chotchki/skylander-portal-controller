# Spec Delta

## Purpose

guacamole draws characters in one of two looks, PARITY (the original's own arithmetic) or REMASTER (the same material in linear light without the original's clipped colours), and a grown-up picks which.

## ADDED Requirements

### Requirement: Two looks
guacamole's TV SHALL draw characters in one of two looks. PARITY SHALL shade them as the original does (the character-shading capability). REMASTER SHALL draw the same models, poses, materials, textures and framing with the arithmetic in linear light, and SHALL NOT let a colour change hue by running past full in one channel.

#### Scenario: Switching with Eruptor on the portal
- **WHEN** Eruptor is turning on his card and the look switches from PARITY to REMASTER
- **THEN** the same model keeps turning in the same pose at the same size, and only its colours change

### Requirement: REMASTER keeps the lava orange
Drawn under the light rig of the oracle's chapter-one frame, Eruptor in REMASTER SHALL show his lava orange, its chroma-weighted hue below 95° where PARITY's is about 107°, and fewer than 1% of his pixels SHALL have red and green both at full.

#### Scenario: Eruptor in both looks
- **WHEN** Eruptor is drawn in each look at the camera, idle time and light rig of the oracle's chapter-one frame
- **THEN** PARITY's lava reads yellow and REMASTER's reads orange, with fewer than 1% of his REMASTER pixels at full red and green

### Requirement: REMASTER decodes the disc's colours
In REMASTER every colour the disc stores encoded for display SHALL be decoded into linear light before it's used: the colour, spec and rim textures plus the light, fog and replace colours. Normal maps, tints and vertex colours (multipliers centred on half grey, not colours) and plain numbers (weights, exponents, alpha references, fog distances) SHALL be used as stored, as in PARITY.

#### Scenario: The key light
- **WHEN** a light's diffuse colour is stored as (172, 134, 18)
- **THEN** REMASTER lights with its linear value, about (0.41, 0.24, 0.006), and PARITY with 172/255, 134/255 and 18/255

#### Scenario: A neutral tint
- **WHEN** a part with neutral tints is drawn in each look
- **THEN** its tints scale it by 1.0079 in both

### Requirement: A grown-up picks the look
A grown-up SHALL be able to switch the look from the guacamole PC without restarting it, with a key while the TV shows the portal or by starting guacamole with the look named. guacamole SHALL keep the choice across restarts.

#### Scenario: The key
- **WHEN** the look key is pressed on the guacamole PC while the portal shows
- **THEN** the TV switches to the other look

#### Scenario: A restart
- **WHEN** the look was switched to PARITY and guacamole is restarted
- **THEN** the TV draws PARITY

### Requirement: A first start shows REMASTER
Until a grown-up picks a look, guacamole SHALL draw REMASTER.

#### Scenario: First start
- **WHEN** guacamole starts with no look ever picked and a figure is placed
- **THEN** its model is drawn in REMASTER
