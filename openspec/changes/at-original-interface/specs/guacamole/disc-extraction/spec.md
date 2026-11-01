# Spec Delta

## ADDED Requirements

### Requirement: The interface is extracted with the rest
Extraction SHALL include the interface: every sprite, image and font and the English text of the shared archives, the character archives, the menu archives and each level it extracts (409 sprites, 310 images and 906 texts in the main shared archive alone). An extract made before the interface SHALL extract the interface alone at the next start, keeping everything it already holds.

#### Scenario: An extract made before the interface
- **WHEN** guacamole starts on a Giants extract made before the interface was extracted
- **THEN** it extracts the interface alone (nothing else again) and the HUD draws once that's done

#### Scenario: A first run
- **WHEN** Giants is picked for the first time
- **THEN** the interface extracts with the rest, after chapter 1 is playable, so a character placed meanwhile plays while it extracts

## MODIFIED Requirements

### Requirement: Extraction runs once and survives interruption
The first time Giants is picked with a recognized dump and no current extraction, the TV SHALL show progress on the portal screen naming what it reads, and phones SHALL still place figures. Chapter 1 SHALL be playable once its own parts are in, the rest extracting meanwhile (35-38 s into a first run on the M3 Max, where it had waited 139 s). Later starts SHALL reuse a current extraction; an interrupted one SHALL resume, redoing only unfinished files; one in another format SHALL be redone.

#### Scenario: First pick
- **WHEN** Giants is picked for the first time with the known dump
- **THEN** the TV shows extraction progress until it finishes, naming each part as it reads it, and a figure placed meanwhile shows its card

#### Scenario: Playing before the rest is in
- **WHEN** a character is placed after chapter 1's own parts are in and before the extraction finishes
- **THEN** the character enters chapter 1 and plays while the rest extracts, the progress hidden while the level shows and back on the portal screen

#### Scenario: Second start
- **WHEN** guacamole is started again after a completed extraction and Giants is picked
- **THEN** no extraction progress is shown

#### Scenario: Interrupted
- **WHEN** guacamole is closed halfway through an extraction and started again, and Giants is picked
- **THEN** the extraction continues from the files it hadn't finished, and the result matches an uninterrupted extraction

#### Scenario: Format changed by an update
- **WHEN** a newer guacamole with a different extraction format starts over an older extraction
- **THEN** the TV shows extraction progress again and the old extraction is replaced
