# Spec Delta

## Purpose
What follows a level's end on the TV, on the way to the next chapter: the completion screen, the movie the level end queues, the hub between chapters and the next chapter picked there.

## ADDED Requirements

### Requirement: A level's end shows its completion screen
When a level's own end-of-level sequencing runs, the TV SHALL show its completion screen over the held level, laid out on the TV's own screens with the disc's own art and fonts, showing what the original's shows for that level as its sequencing computes it (its objectives and what was found: chapter 1's 13). A press of the original's continue button on any pad SHALL go on to whatever the level end queued next, its movie or the next level.

#### Scenario: Chapter 1's end
- **WHEN** chapter 1's outro ends
- **THEN** its completion screen shows over the held level in the disc's own art, and a continue press goes on to FMV02, then level 214

#### Scenario: Level 214's end
- **WHEN** level 214's outro ends
- **THEN** its completion screen shows its own objectives, and a continue press goes on to hub stage 1

#### Scenario: Moving between the tabs
- **WHEN** a player presses left or right on the completion screen
- **THEN** the next tab's objectives show (its story goals, its dares, its collections), and the continue press on any tab goes on

#### Scenario: The portal holds the level under the screen
- **WHEN** a figure placed during the completion screen raises the portal's alert
- **THEN** the screen steps aside under the alert and a continue press there goes nowhere; once the alert clears the screen is back on the tab it was on

#### Scenario: The time dare's clock
- **WHEN** a level's completion screen shows its time dare
- **THEN** its time is the level's play outside its cutscenes, and a level that took longer than the target leaves the dare open

#### Scenario: The screen's art isn't extracted yet
- **WHEN** a level ends on an extract that doesn't hold the screen's art
- **THEN** the screen still shows the level's results in the TV's own style, and the TV's report says the art is missing

### Requirement: The hub between chapters
When a level's end names the hub, the TV SHALL take the hero there and draw it whole (its sky, its deck, its people), starting him where the original's hub does. From the hub, a kid SHALL be able to start any chapter the original's hub offers at that point of the story, the way the original's hub offers it.

#### Scenario: After level 214
- **WHEN** the hero reaches level 214's end and its end names the hub
- **THEN** the hub shows with its sky, the hero standing where the original's hub starts him

#### Scenario: The next chapter
- **WHEN** the kid takes the way the original's hub offers into the next chapter
- **THEN** that chapter is read, extracted on its first entry, and shown with the hero in it

#### Scenario: A level's end names the hub later in the story
- **WHEN** a level's end names the hub after the kid has finished level 210
- **THEN** the hub's second stage shows, as the original's would, not its first

#### Scenario: The way on after level 214
- **WHEN** the kid has talked with Cali on the hub after level 214, then talks to Flynn at the helm and answers yes to his question
- **THEN** level 201 is read, extracted on its first entry, and shown with the hero in it

#### Scenario: A question on the hub
- **WHEN** a person on the hub asks the kid a yes-or-no question, and a figure goes onto the portal while it's up
- **THEN** the TV shows the question with Yes and No, the focus where the original's starts, steps aside for the portal's message until it's gone, and the answer takes the branch the original's answer takes

### Requirement: The hub's people
Every hub stage SHALL hold the people the original's holds at that point of the story, each drawn on its model on its own scripts, standing, walking or flying where they put it, and those that stand SHALL stop the hero. A talk that leads to a screen the TV doesn't have yet SHALL end with nothing shown, and every other talk SHALL still work after it.

#### Scenario: Walking into Flynn
- **WHEN** the hero walks straight at Flynn or Cali
- **THEN** he stops against them and doesn't come out the far side, Eruptor 2.2 m from Flynn's pivot and 1.8 m from Cali's, the giants at 120 % up to 4.4 m and 4.0 m, inside the 25 ft and 15 ft a talk starts from

#### Scenario: A store's yes
- **WHEN** the kid answers yes to Auric's or Hatterson's question, or Brock's or Cali's
- **THEN** the talk ends with nothing shown, his own talk and Flynn's still start after it, and the run holds what a no leaves

#### Scenario: Dreadbeard's yes
- **WHEN** the kid answers yes to Dreadbeard's question
- **THEN** the game reads as quit: he says his quit line, nothing is won or lost, and his talk comes back

#### Scenario: Persephone's upgrades
- **WHEN** the kid talks to Persephone
- **THEN** she says her line, her upgrade menu backs out to its no branch, the scene ends and she idles

### Requirement: A chest opens, pays and stays opened

A treasure chest on any level SHALL rise for a hero within 18 ft (a Giant 30) and 5 ft of its height, ask him to mash, open after three seconds of it (a Giant one) and fling out its level's value in treasure, 63 on chapter 1, every piece to walk up, one past a rim brought back, each paying the figure that takes it; opened, it SHALL stay gone on a restart and in later sessions.

#### Scenario: Opening chapter 1's first chest

- **WHEN** a hero stands by chapter 1's first chest and mashes the fourth action for three seconds
- **THEN** it opens, its treasure lands around it and pays him as he walks it up
- **AND** after Restart chapter, and after a quit and a new session, the chest isn't there

### Requirement: A hat box gives its hat

A hat box SHALL shake open the same way and its hat SHALL be kept in the story save, the box gone from then on. Until hats are worn, a taken hat SHALL be named on the TV and worn by no one, and nothing SHALL hold the hero after it.

#### Scenario: Taking a hat

- **WHEN** a hero mashes a hat box open
- **THEN** the hat's name shows, the box runs to its end and he walks on
- **AND** that box isn't there on a restart

### Requirement: Every hero walks elemental ground

A level's elemental ground SHALL carry any hero, whatever his element. A surface's type is the top six bits of its collision byte, elemental grounds 40 to 47, and only a type of 50 or over SHALL stop a walk.

#### Scenario: Eruptor on chapter 1's air ground

- **WHEN** a fire hero walks up to `Chest(2)` on its air ground
- **THEN** he walks, and the chest rises for him

### Requirement: The hub's doors open by the story

Each door on the hub SHALL draw and swing open for a hero near it, and a room whose door asks for more adventures than are done SHALL stay shut. Flynn and Cali SHALL stay reachable on every stage.

#### Scenario: Stage 1 after level 214

- **WHEN** a hero walks stage 1 with two adventures done
- **THEN** Cali's door opens for him and the crane deck's and gun deck's stay shut

### Requirement: Chapter 1 played to its end on foot
A new game SHALL play chapter 1 from its spawn to its outro with nothing but the pads: the cave and its Giant, the cracked floor he breaks, the second cave's Chompies and Arkeyan, its jump pads to the quarry, the three posts whose chains hold the robot, each breaking under the hero's attacks, and the fights between them.

#### Scenario: The way past the pit
- **WHEN** a hero beats the pit's four Chompies and walks into the cave
- **THEN** its cutscene frees the Giant, the Giant breaks the floor as the hero nears him, and falling through it takes the hero to the second cave

#### Scenario: The second cave's mouths
- **WHEN** Setup_002 wakes its Chompies, each 6 ft across in a mouth 5.8 ft wide
- **THEN** every one comes out into the arena, jumping and falling along the walls that squeeze it

#### Scenario: The last chain
- **WHEN** the third post gives
- **THEN** its cutscene plays, the screen fades, the outro plays and the level end names level 214

### Requirement: A hero over nothing comes back
A hero on his own feet with no ground under him SHALL fall at his gravity to his 120 ft/s with the stick still his, and past the 225 ft the minion's own check reads SHALL come back on his safe point trail, the stick moving him on, so no fall leaves a kid stuck under or beside a level.

#### Scenario: Knocked over a gap
- **WHEN** a hero stands on level 214's first island and is put over the gap beside it, no mesh under him
- **THEN** he falls 69 m and stands on the ground he left, the stick walking him

#### Scenario: Held there by a scene
- **WHEN** a scene or a hook holds the hero over nothing
- **THEN** he stays where it holds him, level 201's walk off its pod crossing its plank on no gravity, until it lets him go

### Requirement: A bomb run out in his hands is his no more
A bomb whose count runs out while a hero carries it SHALL burst and stand back on its spawner carried by no one, so every other spawner on the level clones its own and a Giant can lift again.

#### Scenario: The workshop after a bomb left to run out
- **WHEN** a hero carries a bomb until it bursts over him and walks to another spawner
- **THEN** that spawner's bomb stands there and he takes it

### Requirement: A legendary treasure pays out
A level's legendary treasure SHALL be collected as a hero comes within 15 ft of it, in a scene of its own: it spins up over him, its name shows, it pays the gold its definition holds and goes, the story save keeping it collected so it's gone at every start after. A hero's way on SHALL be his again as it ends.

#### Scenario: Chapter 1's Eagle Paint
- **WHEN** a hero walks up to the Eagle Paint on chapter 1
- **THEN** he's paid 100 gold and it's gone, on a restart and a session on

### Requirement: A collected ship part hangs on the ship
The hub's ship SHALL hang one part of each kind (an engine, a masthead, a paint, a roof): a new game's defaults, and from then a legendary treasure a kid collects in its kind's place, kept in the save so a later session's hub hangs it too, a save made before the TV hung them included. The Dragon Engine hung SHALL open the galley's stair, so a hero walks down to Hatterson. Picking among the parts held is the treasure menu's, AK's.

#### Scenario: Level 214's propeller roof
- **WHEN** a hero collects level 214's legendary treasure and reaches the hub
- **THEN** its roof hangs on the ship's cabin where the default sail stood

#### Scenario: The Dragon Engine
- **WHEN** the save holds the Dragon Engine collected
- **THEN** it hangs on the ship, the galley's blocker is gone and a hero walks down its stair to Hatterson

### Requirement: Level 201 played to its end on foot
A hero landing on level 201 SHALL play it to its end with nothing but the pads: each setup's fight woken by its own trigger, its doors opened by their keys, its push blocks, the practice pit, the eyeball doors and the collapsing bridge, its jump pads out of the pits below its way, its four enemy intros with their cards for their scripts' 3.0, 2.2, 2.5 and 2.5 s, and the arena's waves to Brock's battle, its outro and the hub.

#### Scenario: The life wall's triggers
- **WHEN** a hero walks through the cave's trigger box by Setup_002's door and later past the climb's trigger
- **THEN** the wall's controller sleeps (range 0) while he's in the cave's box, and past the climb it wakes at 200 ft for the life area with the landing's people taken off

#### Scenario: The arena's commander
- **WHEN** the castle's script starts its battle and the wave master sets up its commander
- **THEN** the commander stays to command its waves, through the half battle and Brock's to the heavy elf's reveal and his fight

#### Scenario: Into the hub
- **WHEN** the arena's outro ends
- **THEN** the level asks for the hub's first stage

#### Scenario: Off the arena's floor
- **WHEN** a hero drops off the arena mid-wave, below its floor
- **THEN** the wave is held where it stands until he's back up, and none follows him off

#### Scenario: Out of a pit below the way
- **WHEN** a hero walks off the landing's yard into the pit under it, 9 m down, or off the grass the bridge drops him on into the pit under that, once the bridge is down
- **THEN** stepping onto the jump pad in the pit with the stick held toward the way puts him back up on it
