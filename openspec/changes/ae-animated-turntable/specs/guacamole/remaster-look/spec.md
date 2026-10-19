# Spec Delta

## Purpose

guacamole draws characters in one look, a remaster: the original's own models, materials, textures and shading model (the character-shading capability), worked in linear light at high dynamic range and tonemapped for the display, without the original's colours clipping to another hue. It offers no look closer to the original's.

## ADDED Requirements

### Requirement: One look, in linear light
guacamole's TV SHALL draw characters with the original's shading model worked in linear light at high dynamic range and tonemapped for the display, and SHALL NOT let a colour change hue by running past full in one channel. guacamole SHALL offer no other look: no key, setting or start option switches to one.

#### Scenario: Eruptor on his card
- **WHEN** Eruptor is placed and his model turns on his card
- **THEN** he's drawn in the remaster, and nothing on the guacamole PC or the phone switches him to another look

### Requirement: The lava stays orange
Drawn under the light rig of the oracle's chapter-one frame, Eruptor's lava SHALL read orange, its chroma-weighted hue below 95° where the oracle's is about 107°, and fewer than 1% of his pixels SHALL have red and green both at full.

#### Scenario: Eruptor at the oracle's frame
- **WHEN** Eruptor is drawn at the camera, idle time and light rig of the oracle's chapter-one frame
- **THEN** his lava reads orange where the oracle's reads yellow, with fewer than 1% of his pixels at full red and green

### Requirement: The disc's colours are decoded
Every colour the disc stores encoded for display SHALL be decoded into linear light before it's used: the colour, spec and rim textures plus the light, fog and replace colours. Normal maps, tints and vertex colours (multipliers centred on half grey, not colours) and plain numbers (weights, exponents, alpha references, fog distances) SHALL be used as stored.

#### Scenario: The key light
- **WHEN** a light's diffuse colour is stored as (172, 134, 18)
- **THEN** it lights with its linear value, about (0.41, 0.24, 0.006), not with 172/255, 134/255 and 18/255

#### Scenario: A neutral tint
- **WHEN** a part with neutral tints is drawn
- **THEN** its tints scale it by 1.0079
