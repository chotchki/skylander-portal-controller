# Spec Delta

## Purpose

What running the original game's own scripts from the user's disc makes observable: the numbers the game computes and the random rolls it makes come out as the original's, at the difficulty guacamole plays.

## ADDED Requirements

### Requirement: The game's own numbers
The values the original's scripts compute SHALL come out the same in guacamole from the same disc at the same difficulty, whole numbers equal and fractions within 0.0001: a creature's health, damage and experience, each level's experience awards, the experience table and each hero's health per level.

#### Scenario: Chapter 1 at medium
- **WHEN** guacamole starts chapter 1 at medium
- **THEN** a Chompy has 1 health, bites from 11 and gives 4 experience, a Pandoran Grunt has 33 health and hits for 36, and a near enemy's share of the level's experience is 128.333, as the original's running game holds them

### Requirement: Random rolls
Every roll the game makes SHALL be uniform over the original's range: a whole number from 0 to N with both ends included, or a fraction from 0 up to but not including N. Two sessions SHALL NOT roll the same sequence.

#### Scenario: Chompy bites
- **WHEN** Chompies bite 1,000 times at medium
- **THEN** 11, 12, 13 and 14 come up equally often and nothing else does

#### Scenario: Two sessions
- **WHEN** chapter 1 is played twice from its start with the same input
- **THEN** the rolls differ between the two

### Requirement: Played at medium
Until a story save chooses it, guacamole SHALL play every level at the original's default difficulty, medium.

#### Scenario: A fresh profile
- **WHEN** a profile created a moment ago plays chapter 1
- **THEN** its Chompies have the medium numbers: 1 health and bites of 11 to 14
