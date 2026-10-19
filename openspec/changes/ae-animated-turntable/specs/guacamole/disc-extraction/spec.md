# Spec Delta

## MODIFIED Requirements

### Requirement: Only the original Giants disc is extracted
guacamole SHALL extract from a Skylanders Giants dump only when it recognizes the dump as the original version 01.00 disc. Recognition covers its title (BLUS30968), its disc version and the contents of every game data file except the encrypted executable. A dump that a title update changed SHALL count as one guacamole doesn't recognize. Without a recognized dump guacamole has no figure list (the figure-roster capability).

#### Scenario: The known Giants dump
- **WHEN** Giants is picked and the folder guacamole was set up with holds an unmodified BLUS30968 version 01.00 dump
- **THEN** guacamole extracts it

#### Scenario: A dump with a title update applied
- **WHEN** the Giants dump's version or game data files were changed by installing a title update into it
- **THEN** nothing is extracted and the TV shows the unknown-dump message with its error code

#### Scenario: The Giants dump can't be read
- **WHEN** Giants is picked and the Giants dump setup found has no readable game data any more
- **THEN** the TV says the Giants dump couldn't be read and where it looked, and the phone's collection lists no figures and says why

### Requirement: An unrecognized dump is reported, not guessed at
For a dump it doesn't recognize, guacamole SHALL extract nothing and its TV SHALL show a message with a distinct error code. It SHALL write a report beside the extraction location listing the dump's title, version and each game data file's name, size and content hash, with no file content.

#### Scenario: A dump with one changed file
- **WHEN** the dump is otherwise identical but one game data file differs
- **THEN** nothing is extracted, the TV shows the unknown-dump message with its error code, the report lists every file's name, size and hash, and the phone's collection lists no figures and says the disc wasn't recognized

### Requirement: Extraction runs once and survives interruption
The first time Giants is picked with a recognized dump and no current extraction, guacamole's TV SHALL show extraction progress while it runs. The figure roster SHALL come first, and phones SHALL be able to place figures once it lands while the rest runs. Later starts SHALL reuse a current extraction. An interrupted extraction SHALL resume on the next start, redoing only unfinished files. An extraction in an older or newer extraction format SHALL be redone, with progress shown again.

#### Scenario: First pick
- **WHEN** Giants is picked for the first time with the known dump
- **THEN** the TV shows extraction progress until it finishes, the phone's collection fills in while that progress is still showing and a figure placed then shows its card

#### Scenario: Second start
- **WHEN** guacamole is started again after a completed extraction and Giants is picked
- **THEN** no extraction progress is shown

#### Scenario: Interrupted
- **WHEN** guacamole is closed halfway through an extraction and started again, and Giants is picked
- **THEN** the extraction continues from the files it hadn't finished, and the result matches an uninterrupted extraction

#### Scenario: Format changed by an update
- **WHEN** a newer guacamole with a different extraction format starts over an older extraction
- **THEN** the TV shows extraction progress again and the old extraction is replaced
