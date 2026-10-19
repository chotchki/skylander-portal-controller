# Spec Delta

## Purpose

How a hit lands in a level, for heroes and creatures alike: damage and criticals, armour, stun and knockback, health and defeat, as the original Skylanders Giants does it through the RPCS3 oracle, in our own words.

## ADDED Requirements

### Requirement: A hit does its attack's damage
A hit SHALL do its attack's damage to a target that still has health, the same at every hero level. A hit by a player's hero SHALL be critical with that hero's critical chance, 6 % for Eruptor, adding half its damage rounded down; a creature's hit SHALL never be critical. A target with no health left SHALL ignore hits.

#### Scenario: Lava Lob
- **WHEN** Eruptor's Lava Lob hits a target that has health
- **THEN** it takes 8, or 12 when the hit is critical

#### Scenario: Many throws
- **WHEN** 1,000 of Eruptor's Lava Lobs land
- **THEN** each was critical with a 6 % chance, as in the original

### Requirement: Armour can turn a hit away
A hit on a hero SHALL be turned away whole, doing nothing, with the chance his armour gives: 3 in 101 for Eruptor at level 1. Nothing SHALL hurt a hero while he can't be hurt, as during his eruption.

#### Scenario: Chompy bites on Eruptor
- **WHEN** 1,000 Chompy bites land on Eruptor at level 1
- **THEN** each was turned away with a 3 in 101 chance, and the rest did their damage

### Requirement: No grace after a hit
A hit SHALL land whenever it reaches a target that can be hurt: after a hit there SHALL be no time the target can't be hurt, only the attacker's own wait before hitting that target again. A hurt creature SHALL be unable to act for 0.6 s. A hero hit in the adventure SHALL flash for 0.2 s and, unless he's a Giant, lose control for 0.6 s.

#### Scenario: Two bites at once
- **WHEN** two Chompies' bites reach Eruptor in the same moment
- **THEN** he takes both

#### Scenario: Eruptor bitten
- **WHEN** a Chompy's bite lands on Eruptor
- **THEN** he flashes for 0.2 s and the stick does nothing for 0.6 s

### Requirement: Knockback
A hit that knocks back SHALL push its target away for (its strength less the target's resistance) / 100 × 0.6 s for a creature, or × 0.525 s for a hero, never under 0.125 s, at 80 ft/s (24.4 m/s). A target whose resistance outweighs the hit's strength SHALL NOT move.

#### Scenario: A Chompy's bite
- **WHEN** a Chompy's bite lands on Eruptor
- **THEN** he's pushed away from it for 0.13 s

#### Scenario: A sleeping Chompy
- **WHEN** Eruptor's Lava Lob bursts beside a Chompy still asleep in the pit
- **THEN** the Chompy neither moves nor loses health

### Requirement: A hero's health
A hero SHALL come into a level at the full health of his figure's level, base × (1 + (level − 1) × 0.111) rounded up: Eruptor 290 at level 1, 451 at 6, 580 at 10 and 741 at 15. A hero's health SHALL NOT come back on its own.

#### Scenario: Eruptor at level 1 and at level 10
- **WHEN** Eruptor comes into the level at level 1, and again after the phone set him to level 10
- **THEN** he has 290 health, then 580

#### Scenario: Standing after a bite
- **WHEN** Eruptor stands still for a minute after a Chompy's bite with no creature near
- **THEN** his health is what the bite left

### Requirement: A defeated hero
At no health a hero SHALL play his defeat, knocking back creatures within 30 ft (9.1 m), and leave the level, his figure out for the rest of the level with its experience and gold kept. Placed again, the TV SHALL ask for a different character and he SHALL NOT come back in. Any other character placed SHALL come in as the hero.

#### Scenario: Tree Rex waiting
- **WHEN** Eruptor's health runs out with Tree Rex waiting on the portal
- **THEN** Eruptor plays his defeat and leaves, Tree Rex comes in as the hero, and Eruptor's experience and gold on his profile are as they were

#### Scenario: Placed again
- **WHEN** Eruptor, defeated, is removed and placed again
- **THEN** the TV asks for a different character and Eruptor doesn't come back in
