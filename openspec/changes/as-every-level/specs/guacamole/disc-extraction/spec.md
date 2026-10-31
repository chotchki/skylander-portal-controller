# Spec Delta

## ADDED Requirements

### Requirement: A level past chapter 1 is extracted on its first entry
A first run SHALL extract chapter 1 alone of the disc's levels. Any other level on the disc SHALL be extracted the first time guacamole enters it, keeping every level already extracted, and the next entry SHALL reuse it. An interrupted level extraction SHALL resume on the next entry, redoing only unfinished files. A level extracted or not SHALL never make a later start redo the first run.

#### Scenario: First run
- **WHEN** Giants is picked for the first time with the known dump
- **THEN** chapter 1 is extracted with the figures and no other level is

#### Scenario: Entering another level
- **WHEN** a level past chapter 1 that has never been entered is entered
- **THEN** that level is extracted before it shows, and entering it again later extracts nothing

#### Scenario: Interrupted during a level's extraction
- **WHEN** guacamole is closed while a level past chapter 1 is extracting and that level is entered again after a restart
- **THEN** its extraction continues from the files it hadn't finished, and a start before that entry shows no extraction progress

### Requirement: A cinematic movie is converted on its first play
The disc's cinematic movies (not the figures' intros) SHALL be converted the first time one plays, in English, its voice over the movie's music and effects, and a converted movie SHALL be reused. A missing video converter SHALL leave the movie unplayed and named as the cause, with everything else working.

#### Scenario: A cinematic plays twice
- **WHEN** a cinematic movie plays for the first time and later plays again
- **THEN** the first play converts it with English voices over its music and effects, and the second converts nothing

#### Scenario: No video converter
- **WHEN** a cinematic movie is due to play on a machine without the video converter
- **THEN** it doesn't play, the missing-converter message names the cause, and the level carries on
