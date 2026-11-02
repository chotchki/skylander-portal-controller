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

### Requirement: A level's clones draw their templates' models
A clone a level's scripts make SHALL draw its template's model off the level's own props, static as a prop, skinned as a creature whether or not a script runs it, unless every material is alpha-blended at a tint alpha of 0 with no tint track raising it (10 models over the disc) or it is a Titan pebble. Chapter 1's start SHALL draw 8 of the 14 templates it clones, as before, and no pipeline SHALL compile after a level shows (none in 10,040 frames on level 214).

#### Scenario: Level 214's start
- **WHEN** level 214 starts with a hero joined
- **THEN** its six red push blocks, the spike gate's spikes, holes and lock, the Chompy pods and the undead Chompies draw, and the pebbles don't

#### Scenario: A marker's clone
- **WHEN** a script clones a template whose model shows nothing (`inviso.mdl`)
- **THEN** the clone draws nothing

### Requirement: Level 214's bombs open its walls
On level 214 a hero within 10 ft across and 8 ft up or down of a spawner's bomb SHALL carry it 12 ft over his pivot (22 for a Giant), a tap of the fourth action (△) SHALL throw it, and its burst SHALL open every breakable castle wall within 20 ft, the wall and its solid gone within 0.6 s of the throw, so the pods' fight is reached on foot. A spawner SHALL clone a bomb only while none of its own stands and no player carries one.

#### Scenario: The wall before the pods
- **WHEN** the hero carries a bomb to the castle wall before the pods and taps △
- **THEN** the bomb flies, the wall falls in pieces and he walks through to the fight

### Requirement: Level 214's intro hands the hero over on its ship
Level 214's intro SHALL hold until a hero's minion is in, be skipped a frame in, and hand him over on the ship's deck at (-9.14, -12.58, 18.10), 4.3 m from the level's start marker, with every placement it took off back as its file has it (248 on the dock and the ship) and the ship's engine, masthead and roof standing once each, nothing stopped.

#### Scenario: Chapter 1's end into level 214
- **WHEN** chapter 1's end takes the hero into level 214
- **THEN** he stands on the ship's drawn deck, one engine on it, the dock's props back around him
