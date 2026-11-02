# Spec Delta

## ADDED Requirements

### Requirement: A level's end takes the hero into the next level
When a level's own scripts end it and name the next level, the TV SHALL hold the level while the next is read (extracted on its first entry), then leave it with the hero in it and enter the next with the portal's figures as they stood, the first placed that walks coming in and joining that level's own scripts with his experience and gold. Chapter 1's end SHALL lead to level 214.

#### Scenario: Chapter 1's end
- **WHEN** the hero reaches chapter 1's end
- **THEN** the level holds, and level 214 shows with him in it, walking on his own scripts, his experience as it was

#### Scenario: Another level asked for while he walks
- **WHEN** a level is asked for by name while the hero walks a level
- **THEN** the level holds until the asked one is read, then that level shows with him in it and the same figures on the portal
