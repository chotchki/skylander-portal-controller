# Spec Delta

## ADDED Requirements

### Requirement: The gloss map
A material with a gloss map SHALL take a gloss amount from the red channel of its third-layer texture, used as stored, at the third layer's texture coordinates, times that layer's strength, and SHALL draw no third layer. With the rim sample scaled by the gloss and the rim strength, and k the rim strength times one less the gloss, the rim's over SHALL be the colour times k plus that sample, its add the colour plus that sample and its multiply the colour times the sum of half k and that sample.

#### Scenario: Full gloss
- **WHEN** a part with a gloss map at strength 1 is drawn in the rim's over mode at full rim strength where its gloss texture's red is full
- **THEN** it shows the rim layer alone

#### Scenario: No gloss
- **WHEN** the same part is drawn where its gloss texture's red is zero
- **THEN** it shows its colour, untouched by the rim

### Requirement: A level's rim that multiplies
Where a level material's rim multiplies the colour, the multiply SHALL scale the decoded rim colour by the rim strength raised to 2.2, as the original's multiply of stored values comes out in linear light, the rim's alpha keeping the strength: a grey of one over the strength leaves the colour as it is.

#### Scenario: A coin's metal
- **WHEN** a coin whose rim multiplies at strength 2 is drawn where its rim texture is half grey
- **THEN** that part of the coin keeps its lit colour, neither darkened nor brightened

### Requirement: Layers a level material leaves unset
Where a level material with a second layer sets no exponent and detail weight for it, that layer SHALL take an exponent of 1 and a detail weight of 0.25; where it sets no blend for it, the layer SHALL be blended over the first. Where a level material with a third layer sets no blend for it, the third layer SHALL multiply the colour. This is how the original draws chapter 1, whose third layers set no blend and whose second layers mostly set none.

#### Scenario: A third layer with no blend set
- **WHEN** a part of chapter 1 with a third layer at strength 1 and no blend of its own is drawn
- **THEN** its colour is multiplied by the third layer's colour map
