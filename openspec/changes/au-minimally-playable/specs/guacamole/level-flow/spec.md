# Spec Delta

## Purpose
What follows a level's end on the TV, on the way to the next chapter: the completion screen, the movie the level end queues, the hub between chapters and the next chapter picked there.

## ADDED Requirements

### Requirement: A level's end shows its completion screen
When a level's own end-of-level sequencing runs, the TV SHALL show its completion screen over the held level. The screen SHALL be laid out on the TV's own screens with the disc's own art and fonts. It SHALL show what the original's shows for that level, as the original's sequencing computes it (its objectives and what was found). A press of the original's continue button on any pad SHALL go on to whatever the level end queued next, its movie or the next level.

#### Scenario: Chapter 1's end
- **WHEN** chapter 1's outro ends
- **THEN** its completion screen shows over the held level in the disc's own art, and a continue press goes on to FMV02, then level 214

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
