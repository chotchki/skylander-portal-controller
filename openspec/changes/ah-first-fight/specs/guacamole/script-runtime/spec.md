# Spec Delta

## Purpose

What running the original game's own scripts from the user's disc makes observable: the numbers the game computes and the random rolls it makes come out as the original's, at the difficulty guacamole plays.

## ADDED Requirements

### Requirement: The game's own numbers
The values the original's scripts compute SHALL come out the same in guacamole from the same disc at the same difficulty, to the bit as the original stores them: a creature's health, damage and experience, each level's experience awards, the experience table and each hero's full health per level.

#### Scenario: Chapter 1 at medium
- **WHEN** guacamole starts chapter 1 at medium
- **THEN** a Chompy has 1 health, bites from 11 and gives 4 experience, and a near enemy's share of the level's experience is 128.333, as the original's running game holds them

### Requirement: Random rolls
Every roll the game makes SHALL cover the original's range the way the original does: a whole number from 0 to N with both ends, uniform, or a fraction from 0 to N with the draw rounded to single precision before it scales, as the original rounds it, so N itself comes up once in 2^25 draws. Two sessions SHALL NOT roll the same sequence.

#### Scenario: Chompy bites
- **WHEN** Chompies' bites do damage 1,000 times at medium
- **THEN** each of 11, 12, 13 and 14 comes up between 209 and 291 times, and nothing else does

#### Scenario: A fraction's top
- **WHEN** a fraction roll of 0.5 draws one of the generator's 128 highest numbers
- **THEN** it reads 0.5, and the number just below them reads under 0.5

#### Scenario: Two sessions
- **WHEN** chapter 1 is played twice from its start with the same input
- **THEN** the rolls differ between the two

### Requirement: Played at medium
Until a story save chooses it, guacamole SHALL play every level at the original's default difficulty, medium.

#### Scenario: A fresh profile
- **WHEN** a profile created a moment ago plays chapter 1
- **THEN** its Chompies have the medium numbers: 1 health and bites of 11 to 14
