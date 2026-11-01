# Spec Delta

## Purpose

What the TV shows over a level about the hero (health, level, experience and gold) and the screen when no character is left to play, from the original game's own interface once it runs.

## ADDED Requirements

### Requirement: The original's HUD on the TV
While a level shows, the TV SHALL show each playing hero's HUD as the original's scripts build it at his join and update it: his portrait, his health as a number with its bar, his level, his experience bar and his gold, bottom left, each within a frame of the change it shows, hidden while the original hides its own. It replaces the HUD in the TV's own look once both changes archive.

#### Scenario: A bite
- **WHEN** a Chompy's bite takes 12 of Eruptor's 290 health
- **THEN** his HUD's health reads 278, the number alone, within a frame

#### Scenario: A coin
- **WHEN** Eruptor collects a coin worth 1
- **THEN** the gold on his HUD counts up to the new total as the original's does

#### Scenario: A scene
- **WHEN** the intro in chapter 1's pit plays
- **THEN** his HUD doesn't show until it ends, and shows again within a frame of its end

#### Scenario: The level cap
- **WHEN** Eruptor's experience reaches the cap
- **THEN** his HUD shows the original's LEVEL MAX, its plate and bars shifted as the original's are

### Requirement: The HUD's banners and a bite's number
A level-up's, a rank-up's and a toy accolade's banner SHALL show over the hero's HUD as the original's scripts draw it, a level-up's gone about 2.4 s after the award (the original's 142 frames). The health a bite takes SHALL show as a number by the hero, moving with him while it shows, as the original's does.

#### Scenario: Level 2 by an orb
- **WHEN** Eruptor at 999 experience collects an orb
- **THEN** the original's level-up banner shows over his HUD and is gone about 2.4 s after the award

#### Scenario: A bite's number
- **WHEN** a Chompy's bite takes 12 of Eruptor's health and pushes him
- **THEN** 12 shows by him and moves with him as he's pushed
