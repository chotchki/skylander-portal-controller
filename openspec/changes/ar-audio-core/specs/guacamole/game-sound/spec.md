# Spec Delta

## Purpose

What the kids hear in a level: the original game's own sound effects, voices and music from the user's disc, played when and how the original's scripts and sound data say.

## ADDED Requirements

### Requirement: A sound the scripts play is the original's recording
When a level's or a character's scripts play a sound, guacamole SHALL play that sound's own recording from the user's disc, in full and at its own sample rate, and SHALL play nothing for a sound whose recording the disc lacks. Voices SHALL be the English recordings.

#### Scenario: A throw
- **WHEN** Eruptor throws a Lava Lob in chapter 1
- **THEN** the throw's sound plays from his archive's recording, start to end

#### Scenario: A voice line
- **WHEN** Eon's line plays in chapter 1
- **THEN** it is the English recording, never another language's

### Requirement: A sound plays at its own settings
A sound SHALL play at the volume its data gives, varied per play by its own volume randomness and pitch randomness, and one marked as falling off with distance SHALL play full inside its full distance, silent past its silent distance and quieter between, measured from where the actor playing it stands to the listener (the camera, a hypothesis until the original's listener is read). A sound marked looping SHALL loop until its scripts stop it or its actor leaves.

#### Scenario: Two plays of the same bite
- **WHEN** a Chompy's bite sound with a volume randomness plays twice
- **THEN** the two plays may differ in loudness within that randomness, and neither is louder than the randomness allows

#### Scenario: A sound far away
- **WHEN** a sound that falls off with distance plays farther from the listener than its silent distance
- **THEN** nothing of it is heard

### Requirement: The level's music is the original's
Chapter 1 SHALL play its own music tracks from the disc (ten: four 129 s beds, an 18.8 s intro, four stingers and a silence), switched, layered and faded as the level's scripts direct (exploring, fighting, the stingers), and SHALL play none while the scripts play silence.

#### Scenario: A fight starts
- **WHEN** the first fight's creatures wake in chapter 1
- **THEN** the music moves from the level's exploring track to its fighting track the way the level's scripts switch them

### Requirement: Voices hold a scene for their length
A cutscene or dialogue that waits on its voices SHALL last as long as those voices play, as the original's does.

#### Scenario: Norticus' scene
- **WHEN** Norticus' scene plays after the first fight
- **THEN** it ends after its lines have played, at the original's 300 game steps rather than 183
