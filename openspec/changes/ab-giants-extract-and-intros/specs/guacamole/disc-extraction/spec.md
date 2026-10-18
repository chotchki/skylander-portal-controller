# Spec Delta

## Purpose

How guacamole turns the user's own Skylanders Giants disc dump, found through guacamole's own setup, into the files it plays, without ever changing the dump and without ever putting game content where it could be committed to a source repository.

## ADDED Requirements

### Requirement: Only a recognized dump is extracted
guacamole SHALL extract from a Skylanders Giants dump only when it recognizes the dump. Recognition covers its title (BLUS30968), its disc version and the contents of every game data file except the encrypted executable. For a dump it doesn't recognize, guacamole SHALL extract nothing. Its TV SHALL show a message with a distinct error code, and it SHALL write a report beside the extraction location listing the dump's title, versions and each file's name, size and content hash, with no file content. Figure cards SHALL keep working either way.

#### Scenario: The known Giants dump
- **WHEN** Giants is picked and the folder guacamole was set up with holds an unmodified BLUS30968 version 01.00 dump
- **THEN** guacamole extracts it

#### Scenario: A dump with one changed file
- **WHEN** the dump is otherwise identical but one game data file differs
- **THEN** nothing is extracted, the TV shows the unknown-dump message with its error code, the report lists every file's name, size and hash and placing figures still shows their cards

#### Scenario: The Giants dump can't be read
- **WHEN** Giants is picked and the Giants dump setup found has no readable game data any more
- **THEN** the TV says the Giants dump couldn't be read and where it looked, and placing figures still shows their cards

### Requirement: The dump is never changed
Extraction SHALL NOT create, modify or delete anything inside the dump's folder.

#### Scenario: A full extraction leaves the dump as it was
- **WHEN** an extraction of the known dump completes
- **THEN** every file in the dump has the same size, content and modification time as before, and no file was added to or removed from the dump's folder

### Requirement: Extracted files never land in a source checkout
guacamole and the extraction tool SHALL refuse an extraction location inside a git working tree. The refusal SHALL name the location and say why.

#### Scenario: A development config points inside the repository
- **WHEN** the configured extraction location is a folder inside a git working tree
- **THEN** extraction does not start and the message names that folder and says extracted game files may not live inside a source checkout

### Requirement: Extraction runs once and survives interruption
The first time Giants is picked with a recognized dump and no current extraction, guacamole's TV SHALL show extraction progress while it runs, and phones SHALL still be able to place figures. Later starts SHALL reuse a current extraction without redoing it. An interrupted extraction SHALL resume on the next start, redoing only unfinished files. An extraction made by a guacamole whose extraction format differs SHALL be redone automatically, with progress shown again.

#### Scenario: First pick
- **WHEN** Giants is picked for the first time with the known dump
- **THEN** the TV shows extraction progress until it finishes, and a figure placed meanwhile shows its card

#### Scenario: Second start
- **WHEN** guacamole is started again after a completed extraction and Giants is picked
- **THEN** no extraction progress is shown

#### Scenario: Interrupted
- **WHEN** guacamole is closed halfway through an extraction and started again, and Giants is picked
- **THEN** the extraction continues from the files it hadn't finished, and the result matches an uninterrupted extraction

#### Scenario: Format changed by an update
- **WHEN** a newer guacamole with a different extraction format starts over an older extraction
- **THEN** the TV shows extraction progress again and the old extraction is replaced

### Requirement: A missing video converter is named
When the video converter guacamole uses for extraction (ffmpeg) can't be found, the TV SHALL say that ffmpeg is needed and give the one-line install command for that operating system. Figure cards SHALL keep working, and intros SHALL stay unavailable until guacamole is started with ffmpeg present.

#### Scenario: ffmpeg absent on Windows
- **WHEN** Giants is picked on Windows with no ffmpeg configured, beside guacamole or on the PATH
- **THEN** the TV says ffmpeg is needed and shows the winget command that installs it, and placing a figure shows its card without an intro
