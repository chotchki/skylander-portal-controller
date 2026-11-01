# Spec Delta

## ADDED Requirements

### Requirement: Sounds are extracted after the level
Extraction SHALL include every sound chapter 1 can play (the shared sounds, each modelled character's and the level's, its music included, English voices only: 8,216 sounds in 5,806 recordings, 505 MiB), after the level and without the video converter, so a missing converter costs only the intros. An extraction made before sounds were extracted SHALL add them on the next start without redoing the rest.

#### Scenario: No video converter
- **WHEN** Giants is picked for the first time on a machine without the video converter
- **THEN** the sounds still extract, with the missing-converter message standing for the intros alone

#### Scenario: An extraction from before sounds
- **WHEN** guacamole starts over a completed extraction that holds no sounds
- **THEN** the sounds extract and nothing else is redone
