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

#### Scenario: Playing before the sounds are in
- **WHEN** a character plays chapter 1 on a first run before its sounds have extracted
- **THEN** the level plays without them until they're in, and every sound the scripts start from then on plays (17.3 s after chapter 1 was ready on the M3 Max)

### Requirement: A level's sounds are extracted with it
A level past chapter 1 SHALL bring its own sounds the first time it's entered, extracted with its other parts before it loads (level 214's 444 sounds in 439 recordings, 57 MiB, about 3 s on the M3 Max), so its music and sounds play from its start; a level whose sounds don't extract SHALL play silent rather than wait for them.

#### Scenario: Level 214 entered from chapter 1's end
- **WHEN** chapter 1's end takes the hero into level 214 for the first time
- **THEN** level 214's sounds extract with it and its music plays from its start

