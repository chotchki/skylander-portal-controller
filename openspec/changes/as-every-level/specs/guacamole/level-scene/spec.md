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

### Requirement: A level's end runs its own sequencing
When a level's own scripts record its end, the TV SHALL run the original's end-of-level sequencing from the next game step, its states the scripts' own: the gameplay paused, the HUD hidden, the music faded, the fanfare, the completion screen until a continue, the movie the end queued, then its level asked as the original's switch asks it (chapter 1's 214 three steps after FMV02), nothing stopped and no pad moving the hero or pausing meanwhile.

#### Scenario: Chapter 1's end
- **WHEN** chapter 1's outro records its level end
- **THEN** the fanfare starts in the next step, the completion screen holds until a continue, FMV02 plays, and level 214 is asked for

#### Scenario: The players held through it
- **WHEN** a player holds the stick and presses an attack or Start between chapter 1's end and level 214's ask
- **THEN** the hero stands where the outro left him, no attack reaches his scripts and the level doesn't pause, its clock running on

#### Scenario: A run that can't sequence it
- **WHEN** the scripts that run the sequencing have stopped in play, its completion screen up or not
- **THEN** the screen goes, and the TV still plays the movie the end queued and asks for the level it named

### Requirement: The TV says why it holds a level
While the TV holds a level for its end's movie to convert on its first play (FMV02's 26.9 s on the Mac), or for the next level to be read, it SHALL show over the held level what it's getting ready and, for a level's read, how far it has got.

#### Scenario: FMV02's first play
- **WHEN** chapter 1's end hands FMV02 over and the extract doesn't hold it yet
- **THEN** the held level shows that FMV02 is being got ready until it plays

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

### Requirement: Level 214's second island plays to its second ship part
Past level 214's second locked door its scripts SHALL run on foot to the second ship part: Setup_004B's lobber comes down off his ledge and its closets' eight waves come on; Setup_005's Chompies chase the Mabu into his house; the key in the room under it readies the helper's fight; the jump pads carry a hero up to the second key; the door of two locks opens to both; Auric's scene wakes the part, and taking it starts the sail to the third island.

#### Scenario: From the second door to the sail
- **WHEN** a hero walks on from the second locked door, beats what comes at him, takes both keys and turns them in the door of two locks
- **THEN** Auric's scene plays, the ship part wakes, a press by it takes it and the sail plays, no script stopped

### Requirement: Level 214's intro hands the hero over on its ship
Level 214's intro SHALL hold until a hero's minion is in, then play its ten scenes to their end (52.9 s, Flynn and Cali in them, its ten lines whole) or end on a player's △ past its first second, and hand him over on the ship's deck at (-9.14, -12.58, 18.10), 4.3 m from the level's start marker, every placement it took off back as its file has it and the ship's engine, masthead and roof standing once each, nothing stopped.

#### Scenario: Chapter 1's end into level 214
- **WHEN** chapter 1's end takes the hero into level 214
- **THEN** the intro plays through, then he stands on the ship's drawn deck, one engine on it, the dock's props back around him

#### Scenario: The intro skipped
- **WHEN** a player presses △ in the intro's second scene
- **THEN** it ends within two steps, Flynn's line with it, and he stands on the deck as it hands him over

### Requirement: A cutscene's objects are defined by their placements
A level placement whose script defines a cutscene object into a file structure (Flynn's and Cali's definitions, awake from the load: 15 on 9 levels) SHALL run on the TV, with the script of the template it names as the object's actor, so every cutscene that adds the object to a scene finds its actor.

#### Scenario: Level 214's sail
- **WHEN** the hero takes the first ship part
- **THEN** the sail plays its eleven scenes to their end, Flynn and Cali in them, and sets him down on the second island's ship

### Requirement: A cinematic's camera shot lasts its clip
An animator camera's shot SHALL last its clip's length as its palette holds it, a clip past one Havok block read block by block: chapter 1's outro's second scene 10.0 s, level 214's intro's camera scenes 3.00, 2.30, 1.97 and 3.97 s, its sail's 3.13, 2.93, 2.17 and 4.67 s.

#### Scenario: Chapter 1's outro
- **WHEN** the outro reaches its second scene
- **THEN** it lasts 10.0 s and Norticus' last line is over by the level change

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

### Requirement: A Tree Rex's homing charge
On level 214 a Tree Rex's ranged attack SHALL make a homing charge 10 ft in front of it, which its script steers at its target at 25 ft/s turning at most 50°/s with no body, so nothing it meets stops it, for 2.5 s at most, laying a trail of dirt mounds; within 4 ft of a creature it SHALL strike it (17-20 to Eruptor as measured, his armour turning some or all), a bramble standing 1 s, and go 1.2 s after its strike, or 0.2 s after its time runs out.

#### Scenario: The hero walking at the Tree Rex
- **WHEN** the Tree Rex throws its charge as the hero walks at it from Setup_003's trigger
- **THEN** the charge comes at him across the ground, strikes him and goes

### Requirement: A carried bomb counts down
A level 214 bomb carried SHALL clone a countdown timer at each count past its first, its id the count (11 to 1, 1.1 s apart), each following its bomb by `move to` at its own 17 ft/s with no body and taking itself and its sprite off 2 s after it's made, and the bomb SHALL burst at 0, 12.1 s after the take, nothing stopped.

#### Scenario: A bomb carried to its burst
- **WHEN** the hero takes the first spawner's bomb and walks about with it
- **THEN** eleven timers follow it in turn, each gone 2 s on, and it bursts in his hands

### Requirement: Level 214's locked door and its fourth fight
On level 214's second island the hero SHALL take the key in the lobe below the gangplank's yard by walking into it, and a tap of △ by the yard's locked door SHALL open it; past it Setup_004's second trigger SHALL send its Mabu along his waypoints into the house by the yard, and its three Elf Nears, its Tree Rex and its Chompy spawner SHALL be beaten by his attacks, nothing stopped.

#### Scenario: Through the locked door
- **WHEN** the hero takes the key, taps △ at the door and walks on to Setup_004's triggers
- **THEN** the door opens, the Mabu runs into the house, and the five are beaten

### Requirement: Level 214's second island after the sail
Once level 214's sail is done or skipped, what its cutscene took off SHALL stand again as its file has it: the second island's ship the hero is set down on, the island's NPCs and the ship's doors and hatches, since a reset of a set of placements resets each one and keeps the set.

#### Scenario: The first ship part taken
- **WHEN** the hero takes the first ship part and the sail plays to its end, or a player skips it with △
- **THEN** he stands on the second island's ship at (-383.18, -12.58, 714.41), the ship, `Flynn_Island_2` and `Door_(1)` standing again

### Requirement: A level's own emitters draw where they stand
Every emitter a level's own file places SHALL draw where the file puts it: one with an activation range while the hero is within it in feet (from the level's start before he's in), its particles living out their lives once he leaves; one with neither range nor buffer always; a template never (the disc places 15,383: 2,343 ranged, 25 always, 12,986 templates). A clone a script makes of a level's template SHALL draw its generators, no pipeline compiling for them once a level shows.

#### Scenario: Chapter 1's start
- **WHEN** chapter 1 starts with Eruptor on the portal
- **THEN** the dust motes 5.7 m from the start drift there, and the waterfalls' spray and the rest of its 31 stand nowhere until he comes within their range

#### Scenario: A breakable's dust
- **WHEN** a script clones one of the level's own templates, a breakable's dust
- **THEN** the dust draws off that template's generators

### Requirement: A burst refills while its emitter is on
A generator at a burst's rate (9999 a second) SHALL make its most alive at once and, while its emitter is on, fill each place again as its particle dies, making none past its most once the emitter is off, so an experience orb's one-particle glows (0.3 s a life) stay lit while the orb is out, as the original's still are 10 s after the fight.

#### Scenario: Orbs left on the ground
- **WHEN** Eruptor beats a Chompy at anchor B and its experience orbs lie on the ground
- **THEN** each orb's two glows are drawn 10 s later, two particles an orb

#### Scenario: An eruption's drops
- **WHEN** Eruptor erupts and the eruption's head stays on past its drops' 1.0 s
- **THEN** a second round of drops comes as the first dies, and none after the head is taken off

### Requirement: Into an interior and out
A hero walking into an interior's box on any level SHALL be taken to its room: the screen darkening to black over 0.5 s, the hero set down 0.2 s later 1 ft behind the room's arrival waypoint facing as it does, the screen lifting over 0.5 s as the interior's camera takes the view, the room's exit box made; walking into that box SHALL take him back out the same way, 1 ft past the door's waypoint facing out, his health and what he carries kept.

#### Scenario: Chapter 1's cave
- **WHEN** the hero, the first fight won, walks into the cave's mouth
- **THEN** the screen goes black, he stands in the cave 0.7 s after the darkness began, within 1 m of where the original stood him, and the cave waits for his exit 1.35 s after it began

#### Scenario: Level 214's house by the yard
- **WHEN** the hero walks into the house the Mabu ran into, takes its room's bomb and walks into its exit's box
- **THEN** he stands outside its door with the bomb still over him, which opens the wall before the second key
