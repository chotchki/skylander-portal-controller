# Spec Delta

## MODIFIED Requirements

### Requirement: Placing a figure plays its character's intro
When a figure whose character has an intro on the Giants disc is placed before the level has started, guacamole's TV SHALL play that intro full screen with its sound (the English voice track), starting within 2 seconds of the phone showing the figure on the portal. When the intro ends the TV SHALL go on to the level when the figure starts it, and to the portal view with that figure's card otherwise. A figure placed once the level has started, paused or not, SHALL play no intro.

#### Scenario: A Giants character
- **WHEN** a phone places Eruptor after Giants was picked and extraction has finished, before the level has started
- **THEN** within 2 seconds Eruptor's intro plays full screen with sound, and when it ends the TV shows the level with Eruptor at its start

#### Scenario: Placed while the level runs
- **WHEN** a phone places Tree Rex while Eruptor walks the level
- **THEN** no intro plays or queues and the level carries on

#### Scenario: Placed while the level is paused for a hero
- **WHEN** Eruptor has been removed and the level waits for a Skylander, and Spyro is placed
- **THEN** no intro plays and the level carries on with Spyro

### Requirement: Intros queue, skip and stop
A placement while an intro is playing SHALL queue its intro to play after the current one, in placement order. Removing a figure SHALL drop its intro if it hasn't started and stop it if it's playing. Any button on a connected game controller or any keyboard key SHALL skip the intro that's playing.

#### Scenario: Two placements back to back
- **WHEN** a second figure is placed while the first figure's intro is playing, before the level has started
- **THEN** the second intro plays right after the first ends, and then the TV goes where the first's intro alone would have taken it: the level with the first figure as the hero when the first starts it, otherwise the portal view showing both cards

#### Scenario: Removed before its turn
- **WHEN** a figure whose intro is queued is removed before its intro starts
- **THEN** its intro never plays

#### Scenario: Skipped with the controller
- **WHEN** a button on the game controller is pressed during an intro
- **THEN** that intro stops and the next queued intro, or what follows the intros, takes its place

### Requirement: Intros wait for extraction
While extraction is still running or unavailable, a placement SHALL show the figure's card without an intro. Placements made after extraction finishes SHALL play their intros as placing a figure does, so none once the level has started.

#### Scenario: Placed during extraction
- **WHEN** a figure is placed while the TV shows extraction progress
- **THEN** its card is on the portal view after the progress screen, no intro plays for it and the next figure placed after extraction finishes, before the level has started, plays its intro
