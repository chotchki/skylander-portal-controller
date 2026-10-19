# Spec Delta

## Purpose

How guacamole shades a character: the original Skylanders Giants' shading model, in our own words from what the disc, its decoded shader programs and the RPCS3 oracle show. Every term here is the original's; the colour space they're worked in is the remaster-look capability's (linear light, where the original works on stored, display-encoded values and clips each channel at 8 bits).

## ADDED Requirements

### Requirement: The material comes off the disc
Each part of a character SHALL be drawn with the material the disc gives it: its textures, each texture's role (its position in the material, read against the sampler order of the material's feature set), its feature switches and its parameters. Where a material sets no value, the original's default SHALL apply: a spec exponent of 1 and a detail weight of 0.25, the rim added at a lookup scale of 1 with all of it going into the colour, tints at neutral grey (128 of 255).

#### Scenario: Eruptor's body
- **WHEN** Eruptor's body is drawn
- **THEN** it uses his colour, normal, spec and rim maps in those roles, with the normal-map, spec-map and rim features on and directional lighting off, a spec exponent of 1, a detail weight of 1 and a glow amount of 64 of 255

### Requirement: Features no character has
A character's material SHALL NOT receive reflections, look its rim up by position or carry a gloss map (none on the disc does), and extraction SHALL refuse one that does, naming the feature, so this spec leaves those three out.

#### Scenario: A material this spec leaves out
- **WHEN** a character's material on the disc receives reflections
- **THEN** extraction refuses its model with an error that names reflections; the figure keeps its portrait

### Requirement: The scene sets its own switches
The material SHALL fix the feature switches it owns, and the scene SHALL set the rest: whether the part receives shadows, whether fog is on and how many point lights (up to three) reach it. A material can own one of those scene switches too, and then it SHALL stay as the material sets it whatever the scene has on.

#### Scenario: Eruptor in a fogged scene
- **WHEN** Eruptor's body is drawn in a scene with fog on and no shadows or point lights
- **THEN** it draws with exactly the normal map, spec map, rim and fog

#### Scenario: A material that keeps point lights off
- **WHEN** a part whose material fixes point lights off is drawn in a scene with three point lights
- **THEN** no point light reaches it

### Requirement: Normal maps
A normal map SHALL give X from its alpha channel and Y from its green, each mapped from 0..1 to -1..1, and Z as the square root of the absolute value of 1 − X² − Y². The mapped normal SHALL be Y times the interpolated bitangent, less X times the interpolated tangent (X runs against the tangent), plus Z times the interpolated normal, normalized, none of the three renormalized first.

#### Scenario: A flat normal-map texel
- **WHEN** a normal-map texel holds X and Y at zero
- **THEN** the mapped normal is the interpolated geometric normal, normalized

#### Scenario: A texel past the unit circle
- **WHEN** a normal-map texel holds X and Y at 0.9
- **THEN** its Z is the square root of 0.62, not zero

### Requirement: The geometric normal as interpolated
Everywhere else this spec uses the interpolated (geometric) normal (difference and directional lighting, a light's reflection, the rim lookup and point lights), it SHALL take the normal as interpolated, not renormalized.

#### Scenario: A normal shortened by interpolation
- **WHEN** a directionally lit part's interpolated normal is 0.9 of unit length and points straight at a light
- **THEN** that light's diffuse colour lights it at 0.9

### Requirement: Facing a light
A normal's facing to a directional light SHALL be its dot with the direction toward the light, the reverse of the direction the light travels.

#### Scenario: A light from straight ahead
- **WHEN** a light travels straight into a surface along its normal
- **THEN** that unit normal's facing to the light is 1

### Requirement: Parts without directional lighting
A part without directional lighting SHALL show its colour map at full strength plus, when it has a normal map, the detail weight times the sum over the three lights of the light's diffuse colour times the mapped normal's facing to the light less the geometric normal's, each facing clamped at zero before the difference. The sum SHALL be multiplied by twice the vertex colour, which is half grey (0.5, with an alpha of 1) when the part has none.

#### Scenario: A flat normal map
- **WHEN** a part without directional lighting or vertex colours, its interpolated normal of unit length, has a flat normal map
- **THEN** before specular, tints and rim it shows exactly its colour map

### Requirement: Directionally lit parts
A directionally lit part SHALL multiply four times its colour map and the vertex colour by the sum, over the three lights, of each light's ambient colour plus its diffuse colour times the normal's facing to the light, the facing clamped at zero. The normal SHALL be the mapped one when the part has a normal map, else the geometric normal. A directionally lit part SHALL NOT add the difference lighting of a part without.

#### Scenario: Facing away from every light
- **WHEN** a directionally lit part without vertex colours faces away from all three lights
- **THEN** before specular, tints and rim it shows twice its colour map times the sum of the three ambient colours

### Requirement: Specular
Specular SHALL be the spec map's colour times the sum, over the three lights, of the light's specular colour times the dot of the eye direction with the light's reflection, floored at 0.00001 and raised to the material's exponent, which can be under 1. Specular SHALL be added after the vertex colour and before the tints, so the vertex colour doesn't scale it and the tints do.

#### Scenario: Eruptor's sheen
- **WHEN** Eruptor's body is drawn with fog on
- **THEN** his spec map adds a broad sheen, his exponent being 1

#### Scenario: An exponent under 1
- **WHEN** a part's exponent is 0.34 and a light's reflection meets the eye direction at a dot of 0.5
- **THEN** that light adds 0.5 raised to 0.34 (about 0.79) of its specular colour times the spec map

### Requirement: The eye direction
The eye direction SHALL be the direction from the surface to the eye, of unit length at each vertex and interpolated between them, with each of its components clamped at zero.

#### Scenario: An eye direction pointing left
- **WHEN** the interpolated eye direction is (-0.6, 0.8, 0)
- **THEN** specular takes it as (0, 0.8, 0)

### Requirement: The reflection
A light's reflection SHALL be the direction the light travels less twice that direction's dot with the normal times the normal, normalized. The normal SHALL be the mapped one when the part has a normal map, else the geometric normal when the part has one, which it has with a rim layer, directional lighting or a point light reaching it. A part with neither SHALL take the direction the light travels in place of its reflection.

#### Scenario: A spec map without a normal map or lighting
- **WHEN** a part with a spec map and no normal map, rim layer, directional lighting or point light is drawn with fog on
- **THEN** its specular takes the direction each light travels in place of a reflection

### Requirement: No eye direction, no specular
The eye direction SHALL exist only when the part has a spec map and the scene has fog or a point light on. Otherwise it SHALL be zero, which leaves each light's specular at the floor raised to the exponent: 0.00001 of the light's specular colour times the spec map at an exponent of 1, too little to show.

#### Scenario: A scene with nothing on
- **WHEN** Eruptor's body is drawn with fog, shadows and point lights all off
- **THEN** his specular is only the floor, 0.00001 of his spec map times the lights' specular colours

### Requirement: Tints
The lit colour SHALL be multiplied by four times the placement tint and the material tint, each neutral at 128 of 255 (so neutral tints scale by 1.0079), and a packed material tint SHALL read red from its lowest byte.

#### Scenario: Neutral tints
- **WHEN** a part with neutral tints is drawn
- **THEN** its lit colour is scaled by 1.0079

#### Scenario: A packed tint
- **WHEN** a material tint is stored as the packed value 0xFF00F0F0
- **THEN** it tints the part yellow

### Requirement: Alpha
Alpha SHALL be the first layer's alpha times the vertex alpha and both tints' alphas, with no factor of four. A layer's alpha SHALL be its colour map's alpha mixed toward the material's fixed alpha for that layer by (fixed alpha + 0.001) × 10,000, clamped to 0..1: a fixed alpha of -0.001 or less leaves the colour map's alpha, and one from -0.0009 up replaces it.

#### Scenario: A fixed alpha
- **WHEN** a part's fixed alpha for its first layer is 0.5
- **THEN** its alpha is 0.5 times the vertex and tint alphas, whatever its colour map's alpha

### Requirement: The rim lookup
The rim layer SHALL be looked up at (0.5 + 0.5·Nx, 0.5 − 0.5·Ny) times the material's rim scale, N being the view-space mapped normal when the part has a normal map, else the geometric normal.

#### Scenario: A normal facing the camera
- **WHEN** a part's normal points straight at the camera and its rim scale is 1
- **THEN** its rim is looked up at the middle of the rim map

### Requirement: The rim blend
All four channels of the rim sample SHALL be multiplied by the rim strength (the material's fixed alpha for the rim) and combined with the tinted colour and alpha by the material's weights for three blends: over (the scaled sample plus the colour times one less the strength), add (the colour plus the scaled sample) and multiply (the colour times the scaled sample). The material's split SHALL then move the colour toward that result by one less the split and the alpha toward it by the split.

#### Scenario: Eruptor's rim
- **WHEN** Eruptor's body is drawn
- **THEN** his rim map is added on top of the lit colour at full strength

#### Scenario: Eruptor's spikes
- **WHEN** a part's rim multiplies at a strength of 2 with no split
- **THEN** its colour is multiplied by twice the rim's colour and its alpha is left alone

### Requirement: A second layer
A second layer SHALL be lit as the first is, with its own colour map, its own normal and spec maps when the material has those, its own exponent and detail weight, and its alpha from its own colour map and the material's fixed alpha for it. Before the tints, its lit colour times its coverage (the vertex alpha times its alpha times the material tint's alpha) SHALL be blended into the first layer's by the material's weights for three blends.

#### Scenario: A second layer over the first
- **WHEN** a second layer at a coverage of 0.25 is blended over the first
- **THEN** the lit colour is a quarter the second layer's plus three quarters the first's

### Requirement: The second layer's blends
The second layer's three blends SHALL be over (the covered second layer plus the first times one less the coverage), add (the first plus the covered second) and multiply (the first times the covered second, plus the first times one less the coverage).

#### Scenario: A second layer multiplied
- **WHEN** a second layer at full coverage multiplies the first
- **THEN** the lit colour is the two layers' lit colours multiplied

### Requirement: A third layer
After the tints, a third layer's colour map times its strength (the material's fixed alpha for it) SHALL be blended into the colour by the material's weights for over (the scaled third layer plus the colour times one less the strength), add and multiply, leaving alpha alone.

#### Scenario: A third layer added
- **WHEN** a third layer at strength 1 is added
- **THEN** its colour map adds to the tinted colour and the alpha stays as it was

### Requirement: Texture-coordinate sets
Texture-coordinate sets SHALL be handed out in order of use (the first layer the first set, then a second layer, then a third), so a third layer without a second reads the second set. Each set SHALL go through its own 2×3 transform (u' = a·u + b·v + c, and v' alike), numbered as the set.

#### Scenario: A third layer alone
- **WHEN** a material has a third layer and no second
- **THEN** the third layer reads the part's second texture-coordinate set through the second transform

### Requirement: The order of the terms
The terms SHALL apply in this order: each layer's lighting and specular, the second layer's blend, tints, third layer, shadow, rim, point lights, replace colour, fog.

#### Scenario: A rim over a shadow
- **WHEN** a part in full shadow has a rim added
- **THEN** the rim still shows, since the shadow scales the colour before the rim adds to it

### Requirement: Shadows
A received shadow SHALL scale the colour by 2·(a + (2 − k)·(rgb − a)) of the shadow sample, k being the scene's shadow mode.

#### Scenario: An unshadowed sample in mode 2
- **WHEN** a part receives a shadow sample whose alpha is 0.5 in shadow mode 2
- **THEN** its colour is unchanged

### Requirement: Point lights
Each point light SHALL add its colour times the normal's facing to it (the mapped normal with a normal map, else the geometric normal, dotted with the unit direction from the surface to the light and clamped at zero) times its falloff, its linear term over (d + 0.001) plus its quadratic term over (d + 0.001)², d being the distance to the light. A point light SHALL NOT be scaled by the colour map.

#### Scenario: A point light on a black part
- **WHEN** a point light reaches a part whose colour map is black
- **THEN** the part still shows the light's colour

### Requirement: Replace colour and fog
The replace colour SHALL be mixed over the result by its amount (how the original flashes a character), and then, with fog on, fog SHALL mix toward the fog colour by (depth − near) / (far − near), clamped to 0..1, depth being the distance along the view direction and near and far the fog's distances.

#### Scenario: Nearer than the fog
- **WHEN** a character stands nearer than the fog's near distance
- **THEN** fog leaves its colour unchanged

#### Scenario: Past the fog
- **WHEN** a character stands past the fog's far distance
- **THEN** it shows the fog colour alone

### Requirement: Render states
Parts the disc marks as blended, additive, alpha-tested, two-sided or not writing depth SHALL draw that way: blended by source alpha, additive as source alpha plus what's already there, alpha tests passing only above the material's reference, two-sided parts with no face culled (other parts' back faces are) and parts not writing depth leaving it as it was.

#### Scenario: Two additive parts
- **WHEN** an additive part overlaps another part
- **THEN** the two colours add

### Requirement: The glow mask
Once every part is drawn, each part SHALL be drawn again into a glow mask that starts at zero, writing its glow amount when its glow attribute is on, its glow alpha when the attribute is off, and zero when it has none, except a part with none that writes no depth, which SHALL NOT be drawn into it. Its glow alpha SHALL be its first layer's alpha switched hard (the fixed alpha from 0 up, else the colour map's alpha) times the vertex and both tints' alphas.

#### Scenario: Eruptor's glow
- **WHEN** Eruptor is drawn
- **THEN** his body's mask is 64 of 255 and his eyes', which carry no glow attribute, zero

#### Scenario: A glow attribute turned off
- **WHEN** a part's glow attribute is off and its first layer's fixed alpha is 0.3
- **THEN** its mask is 0.3 times its vertex and tints' alphas, whatever its colour map holds

### Requirement: Drawing the glow mask
The glow mask SHALL be drawn without blending, each part keeping its own culling and its alpha test (run on its glow alpha), and only where the part is the nearest surface, so a part in front writes its own mask over what's behind it.

#### Scenario: A part without glow in front of one with it
- **WHEN** a part with no glow attribute that writes depth covers part of a glowing one
- **THEN** the mask is zero where it covers

#### Scenario: A see-through part without glow in front of one with it
- **WHEN** a blended part with no glow attribute that writes no depth covers part of a glowing one
- **THEN** the glowing part's mask shows through it

### Requirement: Extracting the glow
The glow SHALL be extracted into a buffer 512/960 of the frame's width and half its height, rounded (512 by 270 at the original's 960 by 540), each buffer texel sampling the frame's colour and the mask with linear filtering at its own centre and multiplying the two. A sample past an edge SHALL take the edge texel, here and through the blur.

#### Scenario: The original's frame
- **WHEN** the frame is 960 by 540
- **THEN** the glow buffer is 512 by 270

### Requirement: Blurring the glow
The glow buffer SHALL be blurred across and then down, each pass summing 15 linear-filtered taps: tap k, from -7 to 7, sits 2|k| - 0.5 texels to the side (the centre for k = 0) and weighs exp(-k²/32) over the 15 weights' sum. Each off-centre tap falls halfway between two texels, so a texel reaches 14 to either side.

#### Scenario: One bright texel
- **WHEN** the glow buffer holds one bright texel
- **THEN** after both passes the texel d across and e down from it holds its value times w(d)·w(e), where w(0) is the centre tap's weight and w(2j - 1) and w(2j) half tap j's, and nothing reaches past 14 either way

### Requirement: Adding the glow back
After every part is drawn and before tonemapping, the blurred glow, sampled with linear filtering at each pixel's centre, SHALL be added to the frame's colour twice over, alpha left alone. Nothing in the chain SHALL be clamped.

#### Scenario: Nothing glows
- **WHEN** no part writes a mask above zero
- **THEN** the frame is what the parts drew
