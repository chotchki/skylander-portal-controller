# Spec Delta

## Purpose

The experience orbs and coins a hero collects in a level, how they reach him and what each adds to his figure, levelling up included, as the RPCS3 oracle shows the original Skylanders Giants, in our own words.

## ADDED Requirements

### Requirement: Experience comes out as orbs
A beaten creature's experience, times the save's experience multiplier (1 on a fresh save and in guacamole until story saves), SHALL come out as orbs worth 50, 25, 10, 5 and 1, the largest that fit first, a remainder under 1 making one more 1-point orb, each 50, 25 or 10 one size smaller when a whole-number roll from 0 to 100 is over 75. They SHALL pop out two a frame and come down within 1.2 s. A hero in an area of his own element SHALL get orbs worth twice as much.

#### Scenario: A Chompy beaten
- **WHEN** Eruptor beats a chapter 1 Chompy outside a Fire area
- **THEN** four orbs worth 1 each come out where it fell, or five where the save's multiplier is 1.1

#### Scenario: A big orb
- **WHEN** creatures worth 50 experience are beaten 1,000 times
- **THEN** the first orb comes out worth 25 between 207 and 288 times, and worth 50 otherwise

### Requirement: A hero collects orbs
An orb come down within 20 ft (6.1 m) of a hero, measured across the ground (its height over him not counted), SHALL fly to him at 55 ft/s (16.8 m/s) from that frame on, and its worth SHALL be added to his figure when it arrives, times the level's difficulty multiplier, which is 1 throughout chapter 1. An orb still in the air SHALL NOT fly, however near. One arriving while he's pushed or off the ground SHALL count as any other.

#### Scenario: Orbs by his feet
- **WHEN** a Chompy is beaten 10 ft from Eruptor
- **THEN** each of its orbs flies to him as it comes down and his figure has 4 more experience once the last arrives

#### Scenario: Orbs out of reach
- **WHEN** a Chompy is beaten 30 ft from Eruptor and he doesn't move
- **THEN** its orbs stay where they came down, and fly to him once he walks within 20 ft of them

### Requirement: Levelling up
When collected experience reaches the next level's threshold (level 2 at exactly 1,000) the hero SHALL level up that frame: a 2 s flourish, nothing hurting him for 142 frames (2.4 s), at whose end his health becomes the new level's full health, and 24 to 26 frames (0.4 s) in a shockwave doing 99 to each creature within 30 ft (9.1 m), which armour doesn't turn away. Past level 15's 96,500, experience SHALL change no level.

#### Scenario: The thousandth point
- **WHEN** Eruptor, bitten to 278 health and with 999 experience, collects a 1-point orb
- **THEN** he is level 2 that frame, a bite in the next 2.4 s does nothing, and when that ends he has 323 health (his full health there)

#### Scenario: The shockwave
- **WHEN** Eruptor levels up with Chompies of 1,000 health 16, 23 and 34 ft from him, the nearest with armour 200
- **THEN** the two within 30 ft lose 99 each and the one 34 ft off nothing

### Requirement: Coins
A coin SHALL fly to a hero once a check, every 0.25 s, finds it within 12 ft (3.7 m) of him in space and at least 0.7 s old; a dropped coin SHALL slide in over 0.48 s, a spinning one in 0.3 to 0.43 s, and its worth join his figure's gold the frame it reaches him, one taken while another still slides starting both slides again. A dropped coin SHALL pop up 2.8 to 5.6 m and fall back. Each spinning coin on chapter 1's walkway and above the pit SHALL be worth 1.

#### Scenario: A spinning coin on the walkway
- **WHEN** Eruptor walks within 12 ft of a spinning coin on the walkway
- **THEN** it flies to him over 0.48 s and his figure has 1 more gold

#### Scenario: Just dropped
- **WHEN** a barrel 3 m from Eruptor breaks and drops a coin
- **THEN** it pops up, falls back and flies to him 0.9 to 1.5 s after it appeared, his gold rising as it reaches him

#### Scenario: Two at once
- **WHEN** two coins are taken 0.25 s apart
- **THEN** both reach him together, 0.48 s after the second was taken

#### Scenario: Down the line into the pit
- **WHEN** Eruptor goes down the five spinning coins' line above the pit
- **THEN** each flies to him from under 12 ft, three of them reaching him together, and his gold rises by 5

#### Scenario: Under him as it drops
- **WHEN** a barrel breaks a metre from Eruptor and its coin starts within 12 ft of him
- **THEN** a check in its first frames leaves it, and it flies to him once it has come back down
