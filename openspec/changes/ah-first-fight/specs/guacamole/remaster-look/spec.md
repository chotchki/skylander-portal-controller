# Spec Delta

## ADDED Requirements

### Requirement: A placement tint the scripts move
A placement tint other than neutral SHALL multiply a part's colour in linear light by each channel's ratio to neutral raised to 2.2, as the original's multiply of stored values comes out decoded, its alpha used as stored; a neutral placement tint SHALL scale the part as the decoding requirement says.

#### Scenario: Lava going black
- **WHEN** the eruption's lava reaches its dark tint of (34, 20, 20)
- **THEN** it draws at about 5 % of its neutral red and under 2 % of its green and blue in linear light, not at the 27 % and 16 % those values give as stored
