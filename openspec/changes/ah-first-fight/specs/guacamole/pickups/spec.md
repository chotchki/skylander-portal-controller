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
An orb come down within 20 ft (6.1 m) of a hero, measured in space, SHALL fly to him at 55 ft/s (16.8 m/s) from that frame on, and its worth SHALL be added to his figure when it arrives, times the level's difficulty multiplier, which is 1 throughout chapter 1. An orb still in the air SHALL NOT fly, however near. One arriving while he's pushed or off the ground SHALL count as any other.

#### Scenario: Orbs by his feet
- **WHEN** a Chompy is beaten 10 ft from Eruptor
- **THEN** each of its orbs flies to him as it comes down and his figure has 4 more experience once the last arrives

#### Scenario: Orbs out of reach
- **WHEN** a Chompy is beaten 30 ft from Eruptor and he doesn't move
- **THEN** its orbs stay where they came down, and fly to him once he walks within 20 ft of them

### Requirement: Levelling up
When collected experience reaches the next level's threshold the hero SHALL level up at once: his health back to the full health of the new level, the original's level-up flourish, a moment nothing can hurt him as long as the original's, and a shockwave doing 99 to each creature as far as the original's reaches, which armour doesn't turn away.

#### Scenario: The thousandth point
- **WHEN** Eruptor, hurt and with 999 experience, collects a 1-point orb
- **THEN** he is level 2 with 323 health (his full health there) and a bite in the next instant does nothing

### Requirement: Coins
A coin SHALL fly to a hero once a check, every 0.25 s, finds it within 12 ft (3.7 m) of him in space and at least 0.7 s old; it SHALL slide in over 0.48 s and add its worth to his figure's gold as it reaches him, one taken while another still slides starting both slides again. A dropped coin SHALL pop up 3 to 5.5 m where it breaks out and fall back. Each spinning coin on chapter 1's walkway and above the pit SHALL be worth 1.

#### Scenario: A spinning coin on the walkway
- **WHEN** Eruptor walks within 12 ft of a spinning coin on the walkway
- **THEN** it flies to him over 0.48 s and his figure has 1 more gold

#### Scenario: Just dropped
- **WHEN** a barrel 3 m from Eruptor breaks and drops a coin
- **THEN** it pops up, falls back and flies to him 0.9 to 1.5 s after it appeared, his gold rising as it reaches him

#### Scenario: Two at once
- **WHEN** two coins are taken 0.25 s apart
- **THEN** both reach him together, 0.48 s after the second was taken
