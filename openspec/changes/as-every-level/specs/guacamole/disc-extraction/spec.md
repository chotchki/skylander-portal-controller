# Spec Delta

## ADDED Requirements

### Requirement: A level past chapter 1 is extracted on its first entry
A first run SHALL extract chapter 1 alone of the disc's 143 levels. Any other level on the disc SHALL be extracted the first time guacamole enters it (a story level, a PvP arena and a hub stage took 5.7-10.5 s for 260-426 MB on the M3 Max), keeping every level already extracted, and the next entry SHALL reuse it. An interrupted level extraction SHALL resume on the next entry, redoing only unfinished files. A level extracted or not SHALL never make a later start redo the first run.

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
The disc's 11 cinematic movies (not the figures' intros) SHALL be converted the first time one plays, in English, its voice over the movie's music and effects (fmv11's 35.5 s took 2.6 s on the M3 Max), and a converted movie SHALL be reused. A missing video converter SHALL leave the movie unplayed and named as the cause, with everything else working.

#### Scenario: A cinematic plays twice
- **WHEN** a cinematic movie plays for the first time and later plays again
- **THEN** the first play converts it with English voices over its music and effects, and the second converts nothing

#### Scenario: No video converter
- **WHEN** a cinematic movie is due to play on a machine without the video converter
- **THEN** it doesn't play, the missing-converter message names the cause, and the level carries on

### Requirement: A level's camera rigs are extracted with its palettes
A level's palettes SHALL hold every slot of each placed cinematic camera rig (a model under `Models/Cameras/`, scripted or not), a clip past one Havok block kept block by block. An extract written under the palettes' rule before SHALL read the palettes again and nothing else: chapter 1's on its next start, another level's on its next entry.

#### Scenario: An extract from before the camera rigs
- **WHEN** guacamole starts on an extract whose chapter 1 palettes predate the camera rigs
- **THEN** it reads chapter 1's palettes again, extracts nothing else, and the outro's camera shot lasts its clip

### Requirement: A hero's hurts reach an extract made before them
An extract made before a hero's hurts were extracted SHALL read them on its next start alone, every model kept, once (8 s on the Mac), and a build from before them SHALL play every hero's attacks on an extract a newer build wrote or brought up to date, reading nothing again.

#### Scenario: chotchki's dev extract
- **WHEN** guacamole starts on an extract its models went into before the hurts
- **THEN** it reads the heroes' hurts alone, Eruptor's the same as a new extract's, and a second start reads nothing

#### Scenario: Back to the build before
- **WHEN** the build before starts on that extract and Eruptor attacks
- **THEN** it reads nothing again and his throw and eruption play
