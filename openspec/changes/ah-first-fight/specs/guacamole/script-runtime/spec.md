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
Every roll the game makes SHALL cover the original's range the way the original does: a whole number from 0 to N with both ends, uniform, or a fraction from 0 to N with the draw rounded to single precision before it scales, as the original rounds it, so N itself comes up once in 2^25 draws. Armour's roll is a whole number (armour 200: 2 of 711 bites landed in the oracle) and a critical hit's a fraction (a chance of 100: 846 of 846 hits critical). Two sessions SHALL NOT roll the same sequence.

#### Scenario: Chompy bites
- **WHEN** Chompies' bites do damage 1,000 times at medium
- **THEN** each of 11, 12, 13 and 14 comes up between 209 and 291 times, and nothing else does

#### Scenario: Armour at its top
- **WHEN** Eruptor's armour is 200 and Chompies bite him 1,000 times
- **THEN** a bite lands only when its whole-number roll is 100, between 1 and 20 times

#### Scenario: A critical chance of 100
- **WHEN** Eruptor's critical chance is 100 and his Lava Lobs hit 1,000 times
- **THEN** every hit is critical

#### Scenario: A fraction's top
- **WHEN** a fraction roll of 0.5 draws one of the generator's 128 highest numbers
- **THEN** it reads 0.5, and the number just below them reads under 0.5

#### Scenario: A roll up to a fraction the game keeps
- **WHEN** a coin in play rolls the wait to its next sparkle up to the 0.5 s the game keeps for it
- **THEN** the wait is a fraction from 0 to 0.5 s, so a coin sparkles about every quarter second and not every frame, as the original's three coins stood 0.013, 0.246 and 0.280 s from their next sparkle

#### Scenario: Two sessions
- **WHEN** chapter 1 is played twice from its start with the same input
- **THEN** the rolls differ between the two

### Requirement: An animation's end, as the scripts see it
A script reading how much of an animation is left SHALL read nothing once two frames (1/30 s) of its play or less remain, as the original's scripts do, so a wait on an animation's end ends two frames before the animation does.

#### Scenario: A Chompy winds up
- **WHEN** a Chompy plays its windup (1/3 s) and waits for it to end
- **THEN** it holds after 18 or 19 frames, not 20

### Requirement: The order within a frame
Within a frame the hero's own script SHALL act first, then the game's other scripts in the order they woke, the hand-off of every scripted mover's velocity after them all, and then everything SHALL move, as the original's frame runs: so a push begun that frame (a bite) moves him that frame and his hurt reaction slows it from the next, the stick walks him the frame it's read, and an orb landing within his reach is taken that frame.

#### Scenario: A Chompy's bite
- **WHEN** a Chompy's bite lands on Eruptor standing on open ground, no hurt reaction of his running
- **THEN** that frame he moves at 24.384 m/s, and from the next he slows by 80 ft/s² a frame, 23.978 m/s, 23.571 and on to 21.539 at the push's eighth frame

#### Scenario: A lob's barrels
- **WHEN** a Lava Lob breaks the two barrels at the foot of the ledge route at once
- **THEN** their coins come out the frame after they break, not the frame they break

### Requirement: Played at medium
Until a story save chooses it, guacamole SHALL play every level at the original's default difficulty, medium.

#### Scenario: A fresh profile
- **WHEN** a profile created a moment ago plays chapter 1
- **THEN** its Chompies have the medium numbers: 1 health and bites of 11 to 14
