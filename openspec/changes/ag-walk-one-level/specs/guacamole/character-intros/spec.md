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
