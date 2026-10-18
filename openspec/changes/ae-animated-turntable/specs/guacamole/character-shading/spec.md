# Spec Delta

## Purpose

How the original Skylanders Giants shades a character, in our own words from what the disc, its decoded shader programs and the RPCS3 oracle show. guacamole's PARITY look is built from this, and checked against the oracle with it.

## ADDED Requirements

### Requirement: The material comes off the disc
Each part of a character SHALL be drawn with the material the disc gives it: its textures, each texture's role (its position in the material, read against the sampler order of the material's feature set), its feature switches and its parameters. Where a material sets no value, the original's default SHALL apply: a spec exponent of 1 and a detail weight of 0.25, the rim added at scale 1, tints at neutral grey (128 of 255).

#### Scenario: Eruptor's body
- **WHEN** Eruptor's body is drawn
- **THEN** it uses his colour, normal, spec and rim maps in those roles, with the normal-map, spec-map and rim features on and directional lighting off, a spec exponent of 1, a detail weight of 1 and a glow amount of 64 of 255

### Requirement: The scene sets its own switches
The material SHALL fix the feature switches it owns, and the scene SHALL set the rest: whether the part receives shadows, whether fog is on and how many point lights (up to three) reach it.

#### Scenario: Eruptor in a fogged scene
- **WHEN** Eruptor's body is drawn in a scene with fog on and no shadows or point lights
- **THEN** it draws with exactly the normal map, spec map, rim and fog

### Requirement: The arithmetic runs on stored values
Every texture and colour SHALL be used as stored, with no conversion from the display encoding into linear light, and the result SHALL be stored the same way, with no conversion back and no tonemapping. Each channel past full SHALL clip on its own, so a colour past full in one channel changes hue.

#### Scenario: Eruptor's lava
- **WHEN** Eruptor is drawn under the light rig of the oracle's chapter-one frame
- **THEN** his lava cracks read yellow, red and green both at full on most of their pixels, where the same colour unclipped is orange (a hue near 80° clipping to near 107°)

### Requirement: Normal maps
A normal map SHALL give X from its alpha channel and Y from its green, each mapped from 0..1 to -1..1, and Z as the square root of the absolute value of 1 − X² − Y². The mapped normal SHALL be built from the interpolated tangent frame with X running against the tangent, and the interpolated normal SHALL NOT be renormalized first.

#### Scenario: A flat normal-map texel
- **WHEN** a normal-map texel holds X and Y at zero
- **THEN** the mapped normal is the interpolated geometric normal, normalized

### Requirement: Parts without directional lighting
A part without directional lighting SHALL show its colour map at full strength plus, for each of three lights, the light's diffuse colour times the mapped normal's facing to the light less the geometric normal's (each clamped at zero), scaled by the detail weight. The sum SHALL be multiplied by twice the vertex colour, which is half grey when the part has none.

#### Scenario: A flat normal map
- **WHEN** a part without directional lighting and without vertex colours has a flat normal map
- **THEN** before tint, specular and rim it shows exactly its colour map

### Requirement: Directionally lit parts
A directionally lit part SHALL multiply four times its colour map and the vertex colour by the sum, over the three lights, of each light's ambient colour plus its diffuse colour times the normal's facing to the light, clamped at zero. The normal SHALL be the mapped one when the part has a normal map, else the interpolated geometric normal.

#### Scenario: Facing away from every light
- **WHEN** a directionally lit part without vertex colours faces away from all three lights
- **THEN** before tint, specular and rim it shows twice its colour map times the sum of the three ambient colours

### Requirement: Specular
Specular SHALL be the spec map's colour times the sum, over the three lights, of the light's specular colour times the dot of the eye direction with the light's direction reflected about the normal, floored at 0.00001 and raised to the material's exponent, which can be under 1. The eye direction SHALL have each of its components clamped at zero.

#### Scenario: Eruptor's sheen
- **WHEN** Eruptor's body is drawn with fog on
- **THEN** his spec map adds a broad sheen, his exponent being 1

### Requirement: The normal specular reflects about
Specular SHALL reflect each light about the mapped normal when the part has a normal map, else about the interpolated geometric normal when the part carries one. A part with neither SHALL use the light's own direction in place of the reflected one.

#### Scenario: A spec map without a normal map or lighting
- **WHEN** a part with a spec map and no normal map, rim layer, directional lighting or point light is drawn with fog on
- **THEN** its specular takes each light's own direction in place of a reflection

### Requirement: No eye direction, no specular
The eye direction SHALL exist only when the part has a spec map and the scene has fog, a point light or a rim looked up by position on. Otherwise it SHALL be zero, and a part with a spec map SHALL show no specular at all.

#### Scenario: A scene with nothing on
- **WHEN** Eruptor's body is drawn with fog, shadows and point lights all off
- **THEN** it shows no specular

### Requirement: Tints and alpha
The lit colour SHALL be multiplied by four times the placement tint and the material tint, each neutral at 128 of 255 (so neutral tints scale by 1.0079), and a packed material tint SHALL read red from its lowest byte. Alpha SHALL be the material's fixed alpha when it's set (from -0.001 up), else the colour map's alpha, times the vertex alpha and both tints' alphas.

#### Scenario: Neutral tints
- **WHEN** a part with neutral tints is drawn
- **THEN** its lit colour is scaled by 1.0079

#### Scenario: A packed tint
- **WHEN** a material tint is stored as the packed value 0xFF00F0F0
- **THEN** it tints the part yellow

### Requirement: The rim layer
The rim layer SHALL be looked up by the view-space normal (the mapped one when there's a normal map) at (0.5 + 0.5·Nx, 0.5 − 0.5·Ny) times the rim scale, multiplied by the rim strength and blended into the colour as the material's weighted sum of over, add and multiply. The material's split SHALL weight that result into the colour and the alpha in opposite directions.

#### Scenario: Eruptor's rim
- **WHEN** Eruptor's body is drawn
- **THEN** his rim map is added on top of the lit colour at full strength

### Requirement: Further layers and texture coordinates
A material SHALL apply its second layer (a full lit layer blended into the first before the tints), its third layer (a colour blended in after them), its vertex colours and its per-layer texture-coordinate transforms as the disc defines them. Texture-coordinate sets SHALL be handed out in order of use, so a third layer without a second reads the second set.

#### Scenario: A third layer alone
- **WHEN** a material has a third layer and no second
- **THEN** the third layer reads the part's second texture-coordinate set

### Requirement: The order of the terms
The terms SHALL apply in this order: lighting per layer, the second layer's blend, tints, third layer, reflection, shadow, rim, point lights, replace colour, fog. A received shadow SHALL scale the colour by 2·(a + (2 − k)·(rgb − a)) of the shadow sample, k being the scene's shadow mode, and each point light SHALL add its colour times the facing and its distance falloff, not scaled by the colour map.

#### Scenario: A point light on a black part
- **WHEN** a point light reaches a part whose colour map is black
- **THEN** the part still shows the light's colour

### Requirement: Replace colour and fog
The replace colour SHALL be mixed over the result by its amount (how the original flashes a character), and then fog SHALL mix toward the fog colour by a factor linear in depth between the fog's near and far distances, clamped to 0..1.

#### Scenario: Nearer than the fog
- **WHEN** a character stands nearer than the fog's near distance
- **THEN** fog leaves its colour unchanged

### Requirement: Render states
Parts the disc marks as blended, additive, alpha-tested, two-sided or not writing depth SHALL draw that way: blended by source alpha, additive as source alpha plus what's already there, alpha tests passing above the material's reference and two-sided parts with no face culled. All blending SHALL happen on stored values.

#### Scenario: Two additive parts
- **WHEN** an additive part overlaps another part
- **THEN** the stored values add, clipping per channel at full

### Requirement: Glow
A material's glow SHALL come from a pass of its own: the drawn colour masked by the material's glow amount (or, with none set, its colour map's alpha times the vertex and tint alphas), blurred and added back over the frame, the blur and its sizes being what the oracle's captured frames show.

#### Scenario: Eruptor's glow
- **WHEN** Eruptor is drawn
- **THEN** his body glows at its amount of 64 of 255 and his eyes by their colour map's alpha

### Requirement: The original look matches the oracle
Drawn at the camera, pose, lights and fog of a frame the original rendered in the oracle, a character in the original look SHALL match that frame within a recorded FLIP threshold. The threshold SHALL be no looser than the first such comparison's measured error and SHALL be recorded with the residuals that comparison names (the shadowed area, any glow not yet drawn), so a later change can't drift past it.

#### Scenario: Eruptor in chapter one
- **WHEN** Eruptor is drawn at the camera, idle time, light rig and fog of an oracle frame of chapter one
- **THEN** the result matches the oracle's screenshot of that frame within the recorded threshold
