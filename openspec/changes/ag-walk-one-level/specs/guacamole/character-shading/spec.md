# Spec Delta

## ADDED Requirements

### Requirement: The gloss map
A material with a gloss map SHALL take a gloss amount from the red channel of its third-layer texture, used as stored, at the third layer's texture coordinates and scaled by that layer's strength. In the rim's over mode the gloss SHALL fade from the colour to the rim layer, both scaled by the rim strength; in its multiply mode the colour SHALL be scaled by the glossed rim plus half the strength the gloss leaves. No material SHALL carry both a gloss map and a third layer.

#### Scenario: Full gloss
- **WHEN** a part with a gloss map is drawn in the rim's over mode at full rim strength where its gloss texture's red is full
- **THEN** it shows the rim layer alone

#### Scenario: No gloss
- **WHEN** the same part is drawn where its gloss texture's red is zero
- **THEN** it shows its colour, untouched by the rim
