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
