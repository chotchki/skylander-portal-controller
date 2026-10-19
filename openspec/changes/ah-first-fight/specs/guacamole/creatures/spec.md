# Spec Delta

## Purpose

The creatures a level's scripts move and fight with, starting with the four Chompies of chapter 1's first fight: where they wait, how they wake, warn each other, close in, bite and fall, as the RPCS3 oracle shows the original Skylanders Giants, in our own words.

## ADDED Requirements

### Requirement: Chapter 1's first fight
Chapter 1 SHALL hold the original's first fight: four Chompies around Norticus on the floor of the pit below the walkway, as the original places them. Until a hero comes down to within 7 ft (2.1 m) of their height they SHALL stay as they are, and nothing SHALL hurt them or knock them back; then they fight. Norticus SHALL do in the fight what the original's does.

#### Scenario: Down into the pit
- **WHEN** Eruptor comes down onto the pit floor
- **THEN** the four Chompies wake and come at him

#### Scenario: Watching from above
- **WHEN** Eruptor stands at the pit's rim above them
- **THEN** the Chompies stay as they are, and a Lava Lob that reaches one does nothing to it

### Requirement: A chapter 1 Chompy's numbers
At the original's medium difficulty a chapter 1 Chompy SHALL have 1 health, and beating it SHALL give 4 experience and no gold.

#### Scenario: One Lava Lob
- **WHEN** an awake Chompy takes a Lava Lob
- **THEN** it's beaten and leaves 4 experience and no coin

### Requirement: A Chompy closes in
An alerted Chompy SHALL wait up to 0.5 s, each wait as likely as the original's, turn to the hero and close in at up to 20 ft/s (6.1 m/s), turning at up to 720° a second, until it can bite or is within 5 ft (1.5 m) of him.

#### Scenario: Alerted across the pit
- **WHEN** a Chompy 40 ft from Eruptor is alerted
- **THEN** within 0.5 s it turns to him and runs at him no faster than 20 ft/s

### Requirement: A Chompy bites
A Chompy within 10 ft (3 m) of a hero, 2 s or more after its last bite ended, SHALL wind up and lunge along its facing, biting each hero or ally it overlaps during the lunge once, for 11, 12, 13 or 14 at medium, each as likely, stunning for 0.6 s, then recover before it can bite again.

#### Scenario: Standing still beside a Chompy
- **WHEN** Eruptor stands still within 10 ft of an awake Chompy for 10 s
- **THEN** each of its bites that lands does 11 to 14, and no two bites start less than 2 s plus its lunge and recovery apart

#### Scenario: Many bites
- **WHEN** 1,000 Chompy bites land on Eruptor at medium
- **THEN** 11, 12, 13 and 14 come up equally often

### Requirement: Creatures warn each other
Every 0.25 s each creature within its notice range of a hero SHALL be alerted (30 to 60 ft, 9.1 to 18.3 m, for chapter 1's first four Chompies). An alerted creature SHALL alert every other creature within its call range (50 ft, 15.2 m, for those four), and a creature a hero hurts SHALL be alerted at once.

#### Scenario: One Chompy sees him
- **WHEN** Eruptor comes within one awake Chompy's notice range while the others are out of theirs
- **THEN** within 0.25 s that one is alerted, and the others within 50 ft of it are too

### Requirement: A beaten creature
A creature with no health left SHALL stop blocking at once, play its fall and fade out, gone when that ends, and its experience SHALL appear where it fell the moment it's beaten.

#### Scenario: A Chompy beaten
- **WHEN** a Chompy's health runs out
- **THEN** the hero can walk through where it stood, its experience orbs appear there at once, and it fades and is gone
