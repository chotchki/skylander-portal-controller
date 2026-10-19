# Spec Delta

## Purpose

What a hero's attacks do under the buttons, starting with Eruptor's first two with no upgrades bought, and which characters attack at all: the original Skylanders Giants as the RPCS3 oracle shows it, in our own words.

## ADDED Requirements

### Requirement: Eruptor's Lava Lob
Eruptor's first attack SHALL lob a lava ball that bursts on the first creature, wall or floor it meets, doing 8 to each creature within 5 ft (1.5 m) of the burst that cover doesn't shield. It SHALL aim at the nearest creature within 60 ft (18.3 m) and 37° either side of his facing, else straight ahead at its longest throw, and SHALL throw again no sooner than 0.25 s after the last, his hands taking turns.

#### Scenario: A Chompy ahead
- **WHEN** Eruptor throws with an awake Chompy 20 ft ahead of him
- **THEN** the ball comes down on it and the Chompy is beaten

#### Scenario: Nothing in range
- **WHEN** Eruptor throws with no creature within 60 ft
- **THEN** the ball flies straight ahead and bursts where it comes down

#### Scenario: The button held
- **WHEN** the button for his first attack is held for 2 s
- **THEN** he throws as often as the original's Eruptor does over 2 s, never twice within 0.25 s

### Requirement: Eruptor's Eruption
Eruptor's second attack SHALL erupt around him, its reach growing from 5 to 20 ft (1.5 to 6.1 m) over 0.5 s, held until 1.1 s, then shrinking back over 0.5 s, doing 8 to each creature inside every 0.333 s. Nothing SHALL hurt him from the eruption's start to its end. He SHALL NOT erupt in the air, nor again until 1 s after an eruption ends.

#### Scenario: Chompies all round
- **WHEN** Eruptor erupts with four awake Chompies within 15 ft of him
- **THEN** each takes 8 and is beaten

#### Scenario: Bitten while erupting
- **WHEN** a Chompy's bite lands on Eruptor during his eruption
- **THEN** he loses no health

### Requirement: An attack lands when the original's does
Each attack SHALL take effect at the moment in the hero's attack animation the original's does, the animation playing at the original's speed: Eruptor's lava ball leaving his hand once 16 % of his throw has played at 1.5 times its authored speed, and his eruption starting halfway through his eruption animation played at 1.4 times its speed.

#### Scenario: From the press to the ball
- **WHEN** Eruptor stands still and his first attack is pressed
- **THEN** the ball leaves his hand the same game time after the press as the original's Eruptor's does

### Requirement: Characters that don't attack yet
A character whose own attacks guacamole runs SHALL attack with them; any other character SHALL be the hero all the same, walking the level with its attack buttons doing nothing. Eruptor SHALL attack.

#### Scenario: A character whose attacks don't run yet
- **WHEN** such a character is the hero and an attack button is pressed
- **THEN** nothing happens and the stick still walks it

#### Scenario: Eruptor
- **WHEN** Eruptor is the hero and his first attack is pressed
- **THEN** he throws a lava ball
