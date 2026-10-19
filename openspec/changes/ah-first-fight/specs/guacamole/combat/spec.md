# Spec Delta

## Purpose

How a hit lands in a level, for heroes and creatures alike: damage and criticals, armour, stun and knockback, health and defeat, as the original Skylanders Giants does it through the RPCS3 oracle, in our own words.

## ADDED Requirements

### Requirement: A hit does its attack's damage
A hit SHALL do its attack's damage to a target that still has health, the same at every hero level. A player's hero's hit SHALL be critical when that hero's critical chance (6 for Eruptor) is more than a roll from 0 to 100 (script-runtime: Random rolls), adding half its damage rounded down; a creature's hit SHALL never be critical. A target with no health left SHALL ignore hits.

#### Scenario: Lava Lob
- **WHEN** Eruptor's Lava Lob hits a target that has health
- **THEN** it takes 8, or 12 when the hit is critical

#### Scenario: Many throws
- **WHEN** 1,000 of Eruptor's Lava Lobs land with the game's generator seeded
- **THEN** a throw is critical exactly when its roll is under 6, and between 37 and 83 of them are

### Requirement: Armour can turn a hit away
A hit on a hero SHALL be turned away whole, doing nothing, when half his armour is more than a roll from 0 to 100 (script-runtime: Random rolls); Eruptor's armour at level 1 is 6. Nothing SHALL hurt a hero while he can't be hurt, as while he erupts.

#### Scenario: Chompy bites on Eruptor
- **WHEN** 1,000 Chompy bites land on Eruptor at level 1 with the game's generator seeded
- **THEN** a bite is turned away exactly when its roll is under 3, and between 14 and 46 of them are

### Requirement: No grace after a hit
A hit SHALL land whenever it reaches a target that can be hurt: after a hit there SHALL be no time the target can't be hurt, only the attacker's own wait before hitting that target again. A hurt creature SHALL be unable to act for 0.6 s, a second hit in that time starting it again only if the original's does. A hero hit in the adventure SHALL flash for 0.2 s and, unless he's a Giant, lose control for 0.6 s.

#### Scenario: Two bites at once
- **WHEN** two Chompies' bites reach Eruptor in the same moment
- **THEN** he takes both

#### Scenario: Eruptor bitten
- **WHEN** a Chompy's bite lands on Eruptor
- **THEN** he flashes for 0.2 s and the stick does nothing for 0.6 s

#### Scenario: A Giant bitten
- **WHEN** a Chompy's bite lands on Tree Rex
- **THEN** he flashes for 0.2 s and the stick still walks him

### Requirement: Knockback
A hit that knocks back SHALL push its target at 80 ft/s (24.4 m/s) for (its strength less the target's resistance) / 100 × 0.6 s for a creature, or × 0.525 s for a hero, never under 0.125 s. Each push SHALL raise the target's resistance by half its strength, to at most 100, falling back over time as the original's does, and SHALL be refused while a stronger push still acts. A target whose resistance outweighs a hit's strength SHALL NOT move.

#### Scenario: A Chompy's bite
- **WHEN** a Chompy's bite lands on Eruptor after a while unhurt
- **THEN** he's pushed away from it for 0.13 s

#### Scenario: A second bite soon after
- **WHEN** a second Chompy's bite lands on Eruptor 0.2 s after the first
- **THEN** he's pushed for less than 0.13 s, by what the first left of his resistance as the original's leaves it

#### Scenario: A sleeping Chompy
- **WHEN** Eruptor's Lava Lob bursts beside a Chompy still asleep in the pit
- **THEN** the Chompy neither moves nor loses health

### Requirement: A hero's health
A hero SHALL come into a level at the full health of his figure's level: base × (1 + (level − 1) × 0.111), plus 0.99, the fraction then dropped: Eruptor 290 at level 1, 323 at 2, 451 at 6, 580 at 10 and 741 at 15. Health SHALL NOT come back on its own. A figure taken off and put back in the same level SHALL come back with the health it left with, and start the next level full.

#### Scenario: Eruptor at level 1 and at level 10
- **WHEN** Eruptor comes into the level at level 1, and again after the phone set him to level 10
- **THEN** he has 290 health, then 580

#### Scenario: Standing after a bite
- **WHEN** Eruptor stands still for a minute after a Chompy's bite with no creature near
- **THEN** his health is what the bite left

#### Scenario: Put back in the same level
- **WHEN** Eruptor, bitten down to 200 health, is taken off the portal and put back in the same level
- **THEN** he comes back in with 200 health

### Requirement: A defeated hero
At no health a hero SHALL play his defeat, knocking back creatures within 30 ft (9.1 m), and leave the level, his figure out for the rest of the level with its experience and gold kept. Placed again, the TV SHALL ask for a different character and he SHALL NOT come back in. Any other character placed SHALL come in as the hero.

#### Scenario: Tree Rex waiting
- **WHEN** Eruptor's health runs out with Tree Rex waiting on the portal
- **THEN** Eruptor plays his defeat and leaves, Tree Rex comes in as the hero, and Eruptor's experience and gold on his profile are as they were

#### Scenario: Placed again
- **WHEN** Eruptor, defeated, is removed and placed again
- **THEN** the TV asks for a different character and Eruptor doesn't come back in
