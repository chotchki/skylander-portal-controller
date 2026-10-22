# Spec Delta

## Purpose

The creatures a level's scripts move and fight with, starting with the four Chompies of chapter 1's first fight: where they wait, how they wake, warn each other, close in, bite and fall, as the RPCS3 oracle shows the original Skylanders Giants, in our own words.

## ADDED Requirements

### Requirement: Chapter 1's first fight
Chapter 1 SHALL hold the original's first fight: four Chompies around Norticus on the floor of the pit below the walkway. Until a hero comes down to within 7 ft (2.1 m) of their height they SHALL stay as they are and nothing SHALL hurt them or knock them back; coming down plays their scene with Norticus, after which they stand idle until a hero comes within a notice range. Norticus SHALL hold his ground through the fight.

#### Scenario: Down into the pit
- **WHEN** Eruptor, down in the pit after the Chompies' scene, comes within 30 ft of one of them
- **THEN** it and every other Chompy within 20 ft of it wake together and come at him

#### Scenario: Watching from above
- **WHEN** Eruptor stands at the pit's rim above them
- **THEN** the Chompies stay as they are, and a Lava Lob that reaches one does nothing to it

### Requirement: A chapter 1 Chompy's numbers
At the original's medium difficulty a chapter 1 Chompy SHALL have 1 health, and beating it SHALL give 4 experience and no gold.

#### Scenario: One Lava Lob
- **WHEN** an awake Chompy takes a Lava Lob
- **THEN** it's beaten and leaves 4 experience and no coin

### Requirement: A Chompy closes in
An alerted Chompy SHALL wait a time drawn evenly from 0 up to 0.5 s, play its notice (0.75 s) turning to the hero, then close in at up to 20 ft/s (6.1 m/s), turning at up to 720° a second (12° a frame), until about 15 ft (4.5 m) separates their middles, its body within 10 ft (3 m) of his.

#### Scenario: Alerted across the pit
- **WHEN** a Chompy 30 ft from Eruptor is alerted
- **THEN** it starts for him between 0.75 and 1.3 s later and never runs faster than 20 ft/s

### Requirement: A Chompy bites
A Chompy whose body is within 10 ft (3 m) of a hero's, 2 s or more after its last bite ended, SHALL wind up (0.3 s), hold (0.3 s) and lunge along its facing at 34 ft/s (10.3 m/s) for up to 0.4 s, biting the first hero or ally it touches once for 11, 12, 13 or 14 at medium, each as likely, stopping there, then recover (0.53 s); its 2 s start when it's done.

#### Scenario: Standing still beside a Chompy
- **WHEN** Eruptor stands still within 15 ft of an awake Chompy for 10 s
- **THEN** each of its bites that hurts him does 11 to 14, and no two of its windups start less than 192 frames (3.2 s) apart

#### Scenario: Many bites
- **WHEN** 1,000 Chompy bites do damage to Eruptor at medium
- **THEN** each of 11, 12, 13 and 14 comes up between 209 and 291 times, and nothing else does

### Requirement: Creatures warn each other
Every 0.25 s each creature within its notice range of a hero SHALL be alerted: 30 ft (9.1 m) for chapter 1's first three Chompies, 35 ft (10.7 m) for the fourth. Each one alerted so SHALL alert every other creature within its own call range plus the other's listening range: 60 ft (18.3 m) from the fourth, 20 ft (6.1 m) from the others. One alerted by a call SHALL pass nothing on. A creature a hero hurts SHALL be alerted at once.

#### Scenario: One Chompy sees him
- **WHEN** Eruptor comes within the first Chompy's notice range while the others are out of theirs
- **THEN** within 0.25 s it's alerted, and of the others only those within 20 ft of it are too

#### Scenario: The fourth Chompy sees him
- **WHEN** the fourth Chompy is the first alerted
- **THEN** the others within 60 ft of it are alerted with it

### Requirement: A beaten creature
A creature with no health left SHALL stop blocking at once, play its fall and fade out, gone 0.8 s later, and its experience SHALL start coming out where it fell within two frames, two orbs a frame, one beaten with others waiting its turn.

#### Scenario: A Chompy beaten
- **WHEN** a Chompy's health runs out
- **THEN** the hero can walk through where it stood, its experience orbs come out there within 0.1 s, and it fades and is gone 0.8 s after

### Requirement: What the fight wakes past its own
Once chapter 1's four Chompies are beaten, Norticus' scene SHALL play the moment the last is gone (5 s, the hero set before him), then the spike gate at the cave SHALL lower within 0.5 s and Norticus walk off into the cave, the way on open; nothing else in the pit SHALL come at the hero. Anything the fight's scripts wake that guacamole doesn't run yet SHALL stay as it was while the level goes on.

#### Scenario: The pit after the fight
- **WHEN** Eruptor beats the four Chompies
- **THEN** Norticus thanks him, the spike gate lowers within 0.5 s of the scene's end, nothing else in the pit comes at him, and he can walk on into the cave
