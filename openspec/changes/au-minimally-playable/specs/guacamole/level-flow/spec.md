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

### Requirement: Chapter 1 played to its end on foot
A new game SHALL play chapter 1 from its spawn to its outro with nothing but the pads: the cave and its Giant, the cracked floor he breaks, the second cave's Chompies and Arkeyan, its jump pads to the quarry, the three posts whose chains hold the robot, each breaking under the hero's attacks, and the fights between them.

#### Scenario: The way past the pit
- **WHEN** a hero beats the pit's four Chompies and walks into the cave
- **THEN** its cutscene frees the Giant, the Giant breaks the floor as the hero nears him, and falling through it takes the hero to the second cave

#### Scenario: The last chain
- **WHEN** the third post gives
- **THEN** its cutscene plays, the screen fades, the outro plays and the level end names level 214

### Requirement: A hero over nothing comes back
A hero with no ground under him who has fallen far past the last ground, or who hardly moves for a second, SHALL come back to where he stood on the ground a moment before, his walk gone, so no fall or edge leaves a kid stuck under or beside a level.

#### Scenario: Held on a chasm's rim
- **WHEN** a knock leaves the hero over a chasm, held on its rim
- **THEN** within a second or so he stands on the ground he left, the stick moving him again

