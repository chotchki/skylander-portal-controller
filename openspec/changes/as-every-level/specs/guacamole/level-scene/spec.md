# Spec Delta

## ADDED Requirements

### Requirement: A level's end takes the hero into the next level
When a level's own scripts end it and name the next level, the TV SHALL hold the level within 1.1 s, read the next (extracted on its first entry: level 214's 432 MB in 4.6 s) and enter it with the hero in it, the portal's figures as they stood, the first placed that walks joining that level's own scripts with his experience and gold, within 14 s on a first entry and 4 s after. Chapter 1's end SHALL lead to level 214, on a first run too.

#### Scenario: Chapter 1's end
- **WHEN** the hero reaches chapter 1's end
- **THEN** the level holds, and level 214 shows with him in it, walking on his own scripts, his experience as it was

#### Scenario: Chapter 1's end on a first run
- **WHEN** the hero reaches chapter 1's end before the interface's part has landed
- **THEN** the end waits, named in the TV's report, and level 214 is asked for within 2.1 s of the part landing

#### Scenario: Another level asked for while he walks
- **WHEN** a level is asked for by name while the hero walks a level
- **THEN** the level holds until the asked one is read, then that level shows with him in it and the same figures on the portal
