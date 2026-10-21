# Spec Delta

## Purpose

What a hero's attacks do under the buttons, starting with Eruptor's first two with no upgrades bought, and which characters attack at all: the original Skylanders Giants as the RPCS3 oracle shows it, in our own words.

## ADDED Requirements

### Requirement: Eruptor's Lava Lob
Eruptor's first attack SHALL lob a lava ball from one hand, then the other, 3° to that side of his facing and 65° up, turning him to the nearest creature, barrel, pot or gourd within 60 ft (18.3 m) and 37° either side and landing on it, else straight ahead at 80 ft/s (24.4 m/s). It SHALL burst where it lands, doing 8 to each creature within 5 ft (1.5 m) that cover doesn't shield and breaking the barrels, pots and gourds around it, up to 3.4 m off. A throw SHALL start at most every 0.333 s.

#### Scenario: A Chompy ahead
- **WHEN** Eruptor throws with an awake Chompy 20 ft ahead of him
- **THEN** the ball comes down on it and the Chompy is beaten

#### Scenario: Nothing in range
- **WHEN** Eruptor throws with no creature, barrel, pot or gourd within 60 ft
- **THEN** the ball flies straight ahead at 80 ft/s and bursts where it comes down

#### Scenario: The button held
- **WHEN** the button for his first attack is held for 2 s
- **THEN** he throws once

#### Scenario: Pressed again and again
- **WHEN** the button for his first attack goes down every other frame for 2 s
- **THEN** he throws 6 times, his hands taking turns, each throw starting 0.333 s or more after the last

### Requirement: Eruptor's Eruption
Eruptor's second attack SHALL play his eruption, 2 s at 1.4 times its speed (1.42 s), and erupt around him once half has played: its reach growing from 5 to 20 ft (1.5 to 6.1 m) over 0.5 s, held until 1.1 s and gone at 1.6 s, doing 8 to each creature inside every 0.333 s and breaking each barrel, pot and gourd it reaches. Nothing SHALL hurt him from its appearing to his eruption's last frame; a hit before SHALL land. He SHALL NOT erupt in the air, nor again until 1 s after one ends.

#### Scenario: Chompies all round
- **WHEN** Eruptor erupts with four awake Chompies within 15 ft of him
- **THEN** each takes 8 and is beaten

#### Scenario: A barrel 5.9 m off
- **WHEN** Eruptor erupts with a barrel 5.9 m from him
- **THEN** it breaks 0.47 s after the eruption appears, as the reach passes it

#### Scenario: Bitten while erupting
- **WHEN** a Chompy's bite lands on Eruptor while the eruption is out
- **THEN** he loses no health

#### Scenario: Bitten as he winds up
- **WHEN** a Chompy's bite lands on Eruptor after the press but before the eruption appears
- **THEN** he takes its damage

#### Scenario: Pressed again and again
- **WHEN** the button for his second attack goes down every other frame for 5 s, or is held as long
- **THEN** he erupts three times, each 1 s after the last ended, or once when held

### Requirement: An attack lands when the original's does
Each attack SHALL start the frame after its press and take effect where the original's does in the hero's attack animation, played at the original's speed: Eruptor's lava ball leaving his hand in the first frame that starts with 84 % or less of his throw (0.5 s at 1.5 times its speed) left, its fourth, and his eruption in the first that starts with half of his eruption left, its 43rd.

#### Scenario: From the press to the ball
- **WHEN** Eruptor stands still and his first attack is pressed
- **THEN** his throw starts the next frame and the ball leaves his hand 5 frames (0.083 s) after the press

#### Scenario: From the press to the eruption
- **WHEN** Eruptor stands still and his second attack is pressed
- **THEN** the eruption appears 44 frames (0.73 s) after the press

### Requirement: Which characters attack
A character whose own attack scripts guacamole runs SHALL attack with them; any other SHALL be the hero all the same, walking the level with its attack buttons doing nothing. Eruptor's attacks SHALL match the original's by every check above. Tree Rex's first attack SHALL match the original's from the press to its damage and its wait. Any other character's attacks SHALL strike a target, unchecked against the original beyond that.

#### Scenario: A character whose attacks don't run yet
- **WHEN** such a character is the hero and an attack button is pressed
- **THEN** nothing happens and the stick still walks it

#### Scenario: Eruptor
- **WHEN** Eruptor is the hero and his first attack is pressed
- **THEN** he throws a lava ball

#### Scenario: Tree Rex
- **WHEN** Tree Rex is the hero and his first attack is pressed with a Chompy in reach
- **THEN** it lands the same game time after the press as the original's, for the original's damage
