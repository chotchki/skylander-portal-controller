# Spec Delta

## Purpose

The first piece of Skylanders Giants itself that guacamole plays: placing a figure plays that character's original intro video, from the user's own disc, on the TV.

## ADDED Requirements

### Requirement: Placing a figure plays its character's intro
When a figure whose character has an intro on the Giants disc is placed on the portal, guacamole's TV SHALL play that intro full screen with its sound (the English voice track), starting within 2 seconds of the phone showing the figure on the portal. When the intro ends the TV SHALL return to the portal view with that figure's card. Variants and reposes of a character SHALL play that character's intro. Giants' own disc carries the intros for its Spyro's Adventure era characters, and those SHALL play too.

#### Scenario: A Giants character
- **WHEN** a phone places Eruptor after Giants was picked and extraction has finished
- **THEN** within 2 seconds Eruptor's intro plays full screen with sound, and when it ends the portal view shows Eruptor's card

#### Scenario: A variant
- **WHEN** a phone places a variant of a character, such as Legendary Ignitor
- **THEN** the base character's intro plays (Ignitor's)

#### Scenario: A Spyro's Adventure figure
- **WHEN** a phone places a Spyro's Adventure figure such as Spyro
- **THEN** Spyro's intro from the Giants disc plays

### Requirement: Figures without an intro keep their card
When the placed figure has no character intro on the Giants disc (magic items, location pieces, and any figure that reaches the portal without one), guacamole SHALL play nothing and SHALL show the figure's card as it does without intros.

#### Scenario: A magic item
- **WHEN** a phone places a magic item such as Sky-Iron Shield after Giants was picked
- **THEN** no video plays and the item's card appears on the portal view

### Requirement: Intros queue, skip and stop
A placement while an intro is playing SHALL queue its intro to play after the current one, in placement order. Removing a figure SHALL drop its intro if it hasn't started and stop it if it's playing. Any button on a connected game controller or any keyboard key SHALL skip the intro that's playing.

#### Scenario: Two placements back to back
- **WHEN** a second figure is placed while the first figure's intro is playing
- **THEN** the second intro plays right after the first ends, and the portal view shows both cards afterwards

#### Scenario: Removed before its turn
- **WHEN** a figure whose intro is queued is removed before its intro starts
- **THEN** its intro never plays

#### Scenario: Skipped with the controller
- **WHEN** a button on the game controller is pressed during an intro
- **THEN** that intro stops and the next queued intro, or the portal view, takes its place

### Requirement: Intros wait for extraction
While extraction is still running or unavailable, a placement SHALL show the figure's card without an intro. Placements made after extraction finishes SHALL play their intros.

#### Scenario: Placed during extraction
- **WHEN** a figure is placed while the TV shows extraction progress
- **THEN** its card is on the portal view after the progress screen, no intro plays for it and the next figure placed after extraction finishes plays its intro
