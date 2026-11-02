# Spec Delta

## ADDED Requirements

### Requirement: A level's end takes the hero into the next level
When a level's own scripts end it and name the next level, the TV SHALL hold the level within 1.1 s, read the next (extracted on its first entry: level 214's 432 MB in 4.6 s) and enter it with the hero in it, the portal's figures as they stood, the first placed that walks joining that level's own scripts with his experience and gold, within 14 s of any movie the end queued being over on a first entry and 4 s after. Chapter 1's end SHALL lead to level 214, on a first run too.

#### Scenario: Chapter 1's end
- **WHEN** the hero reaches chapter 1's end
- **THEN** the level holds, and level 214 shows with him in it, walking on his own scripts, his experience as it was

#### Scenario: Chapter 1's end on a first run
- **WHEN** the hero reaches chapter 1's end before the interface's part has landed
- **THEN** the end waits, named in the TV's report, and level 214 is asked for within 2.1 s of the part landing

#### Scenario: Another level asked for while he walks
- **WHEN** a level is asked for by name while the hero walks a level
- **THEN** the level holds until the asked one is read, then that level shows with him in it and the same figures on the portal

### Requirement: Chapter 1's outro plays to its end
When chapter 1's end wakes its outro, the TV SHALL play the outro's cutscene to its end with nothing skipped: Norticus and the three Mabu drawn at 140 % in their hats, the Mabu walking to their waypoints, its four lines voiced in order, its scenes in order, and its own level end naming level 214, nothing stopped in the run: shown within 1 s of the dev entry's end anchor, the level held for what follows within 13 s (12.52-12.57 s measured).

#### Scenario: The dev entry's end anchor
- **WHEN** the hero is set down on the outro's waypoint
- **THEN** Norticus and the Mabu are drawn and play their scene, then the level holds for level 214

### Requirement: A level end's movie plays on the TV
When a level's end queues one of the disc's movies (chapter 1's FMV02), the TV SHALL convert it into the extract on its first play with the level held (FMV02's 218 MB in 28 s measured), then play it full screen with its sound, the level and its sounds held, △ or Start on any pad ending it, and go on to the next level after it. A second play SHALL convert nothing and start within 1 s; one that won't convert or open SHALL end at once, why in the TV's report.

#### Scenario: FMV02 after chapter 1
- **WHEN** chapter 1's outro ends for the first time on a dump
- **THEN** FMV02 is converted with the level held, plays with its sound, and level 214 comes after it

#### Scenario: The movie skipped
- **WHEN** a player presses △ while the movie plays
- **THEN** it ends and the next level is asked for

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

### Requirement: Level 214's bomb comes back to its spawner
A level 214 bomb SHALL stand again on the spawner that cloned it within 1.3 s of its burst, waiting to be taken, nothing left at the blast and its burst sounding once, and taken again it SHALL open the next castle wall, so one spawner's bombs open the wall before the pods, the wall past them and the wall before Eon's elemental hint in turn.

#### Scenario: The wall past the pods
- **WHEN** the first bomb has opened the wall before the pods
- **THEN** the bomb is back on its spawner, and carried to the wall past the pods and thrown it opens that wall too

### Requirement: Level 214's second fight
On level 214 Setup_002's controller SHALL wait at its first trigger for Eon's elemental hint, which plays its line as the hero walks into it, holding him, and goes 8.7 s after the line starts (77 steps with no recording); at its second trigger its four undead Chompies SHALL run in and fight (beaten in 3.6 s on the Mac); the two Elf Nears SHALL spar unhurt by any attack (48 of 48 hp after 1,500 steps of mashing) until a bomb opens their wall, then play their scene and fight, nothing stopped.

#### Scenario: Past the wall beyond the pods
- **WHEN** the hero walks past the opened walls into Eon's hint and on to the second trigger
- **THEN** the hint's line plays, the four Chompies run in, and beaten they pay his figure experience

#### Scenario: Attacks among the sparring elves
- **WHEN** the hero, set down among the two Elf Nears behind their shut wall, attacks them
- **THEN** neither loses health, and once a bomb opens their wall and their scene is over both can be beaten

#### Scenario: A lob in flight as an elf dies
- **WHEN** Eruptor's lava lob is in flight as a beaten Elf Near's dead state takes its shape off
- **THEN** the lob doesn't burst on it, and the portal manager and the camera's scripts run on

### Requirement: Level 214's rise over its push blocks
On level 214 the three push blocks by the pods SHALL each move 15 ft along its track when the hero walks into it on the stick and drop into the channel below onto its end marker (within 0.4 m), inactive, level with the terrace; the three SHALL bridge the channel up onto the rise holding the second bomb spawner, whose south ridge (1.8-2.4 m) no walk crosses; its bombs SHALL open the walls beyond, thrown over the ridge or carried round off the rise's east edge.

#### Scenario: From the east terrace
- **WHEN** the hero pushes the first block, then the stacked pair, then the top one from the first's top
- **THEN** the three lie inactive on their end markers and he walks over them onto the rise

### Requirement: Level 214's jump pad to the first ship part
Once both elves are beaten, level 214's first jump pad SHALL launch a hero standing on it at its own pitch and speed (80° and 90 ft/s) along his facing, his ground speed in the air his stick's: let go, the launch's ground speed is taken off and he goes 9 m up and comes back down on the pad; held toward the first ship part, he lands on the ledge holding it, where a press takes it.

#### Scenario: On the stick toward the part
- **WHEN** the hero walks onto the pad holding the stick toward the first ship part
- **THEN** he flies onto its ledge, and a press takes the part and the sail sets him on the second island's ship

### Requirement: Level 214's second island's first fight
On level 214's second island, the hero walking off the ship to Setup_003's trigger SHALL have its controller wake its Tree Rex and four undead Chompies; the Tree Rex SHALL eat one Chompy, and the rest and the Tree Rex (89 hp) SHALL be beaten by his attacks (all five gone in 23.2 s on the Mac), nothing stopped.

#### Scenario: Off the second island's ship
- **WHEN** the hero walks off the ship to Setup_003's trigger and fights
- **THEN** the Tree Rex eats a Chompy, and the other three and the Tree Rex go down

### Requirement: Level 214's second island after the sail
Once level 214's sail is done or skipped, what its cutscene took off SHALL stand again as its file has it: the second island's ship the hero is set down on, the island's NPCs and the ship's doors and hatches, since a reset of a set of placements resets each one and keeps the set.

#### Scenario: The first ship part taken
- **WHEN** the hero takes the first ship part and the sail is skipped
- **THEN** he stands on the second island's ship at (-383.18, -12.58, 714.41), the ship, `Flynn_Island_2` and `Door_(1)` standing again

### Requirement: A level's own emitters draw where they stand
Every emitter a level's own file places SHALL draw its particles where the file puts it, as the original does: one with an activation range while the hero is within that range of it (from the level's start before he's in), its particles living out their lives once he leaves; one with neither range nor buffer always; a template never. A clone a script makes of one of the level's own templates SHALL draw that template's generators, and no pipeline SHALL compile for them after a level shows.

#### Scenario: Chapter 1's start
- **WHEN** chapter 1 starts with Eruptor on the portal
- **THEN** the dust motes 5.7 m from the start drift there, and the waterfalls' spray and the rest of its 31 stand nowhere until he comes within their range

#### Scenario: A breakable's dust
- **WHEN** a script clones one of the level's own templates, a breakable's dust
- **THEN** the dust draws off that template's generators
