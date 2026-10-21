# Spec Delta

## Purpose

What the TV shows over a level about the hero (health, level, experience and gold) and the screen when no character is left to play, in guacamole's own look until it runs the original game's interface.

## ADDED Requirements

### Requirement: The hero's numbers on the TV
While a level shows, the TV SHALL show the hero's health against his full health, his level, his experience toward the next level and his figure's gold in the TV's own look, each matching the hero within one frame of changing, and SHALL warn while his health is under 20 % of full.

#### Scenario: A coin
- **WHEN** Eruptor collects a coin worth 1
- **THEN** the gold the TV shows goes up by 1 within a frame

#### Scenario: A bite
- **WHEN** a Chompy's bite takes 12 of Eruptor's 290 health
- **THEN** the TV shows 278 of 290 within a frame

#### Scenario: Low health
- **WHEN** Eruptor's health falls to 50 of 290
- **THEN** the TV warns that his health is low

### Requirement: No character left
When the hero is beaten and no character waits on the portal, the level SHALL pause under a screen asking for the beaten figure to be taken off, then for another character, offering Restart chapter from a few seconds in behind a yes-or-no check starting on no. Restart SHALL start the level again from its start, its breakables and creatures back, the beaten figure in at full health and every figure's experience and gold kept. Placing a character not beaten in the level SHALL carry on instead.

#### Scenario: Restart chapter
- **WHEN** Eruptor is beaten with no character waiting and Restart chapter is chosen and confirmed
- **THEN** the level starts again from its start with Eruptor as the hero at full health, his experience and gold as they were

#### Scenario: Another character instead
- **WHEN** Eruptor is beaten with no character waiting, is taken off and Spyro is placed
- **THEN** the level carries on with Spyro as the hero
