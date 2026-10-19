# Spec Delta

## ADDED Requirements

### Requirement: Levels are extracted after the figures
Extraction SHALL include each level guacamole plays (chapter 1 in this version), everything of it guacamole draws and walks on, after the figure roster and the character models and without waiting on the video converter, so figures can be placed while it runs and a missing converter costs only the intros. A character placed before its level is ready SHALL keep its card and enter the level once it is.

#### Scenario: Placed while the level is still extracting
- **WHEN** Eruptor is placed after the figure list has filled in but before the level has finished extracting
- **THEN** his card shows on the portal view, and once the level is ready the TV takes him into it

#### Scenario: No video converter
- **WHEN** Giants is picked for the first time on a machine without the video converter
- **THEN** the level still extracts and a placed character walks it, with the missing-converter message standing for the intros alone
