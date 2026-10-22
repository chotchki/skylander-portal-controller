# Spec Delta

## Purpose

What a hero's attacks break in a level, what a broken thing leaves and drops, the ways breaking opens, and what waits for a Giant's feats, starting with chapter 1's barrels, pots and gourds, as the RPCS3 oracle shows the original Skylanders Giants.

## ADDED Requirements

### Requirement: Attacks break barrels, pots and gourds
A barrel, pot or gourd SHALL break at the first hit of either of Eruptor's attacks, its solid going with it, and stay broken for the rest of the level, a character taken off the portal and put back included; its three pieces of debris SHALL fly off and be gone 1.5 s later. No damage number SHALL show on it.

#### Scenario: A barrel by the ledge route
- **WHEN** Eruptor's Lava Lob comes down on the barrel at the foot of the ledge route
- **THEN** it and the barrel 2.2 m behind it break at once and he can walk where they stood

#### Scenario: Swapped mid-smash
- **WHEN** Eruptor breaks the ledge route's two barrels, comes off the portal and is put back
- **THEN** both are still broken, and the next thing he breaks drops by its kind's count as it stood

### Requirement: What a broken thing drops
A broken barrel, pot or gourd SHALL drop a coin by its kind's running count, not a fresh chance each time: each break adds the kind's rate (0.6 for chapter 1's common ones, 1 for its two uncommon pots) to a count that wraps past 1, dropping when the count is at or under the rate. Where each count starts SHALL be drawn when the level starts, and a character swap SHALL leave it where it stood. A common coin SHALL be worth 1 or 2, two in three worth 1; an uncommon one 3 or 7, as likely.

#### Scenario: Five in a row
- **WHEN** Eruptor breaks five of the clearing's common barrels, pots and gourds in a row
- **THEN** exactly three of them drop a coin

#### Scenario: An uncommon pot
- **WHEN** Eruptor breaks either of the clearing's two uncommon pots
- **THEN** each drops a coin worth 3 or 7

#### Scenario: The coins' worth
- **WHEN** 300 coins have dropped from broken common barrels, pots and gourds
- **THEN** between 176 and 224 of them are worth 1 and the rest 2

### Requirement: Breaking opens the ledge route
With the two barrels and the pot at the foot of the ledge route broken, the hero SHALL climb it to the walkway above the gate, as in the original, and reach the spinning coins along it.

#### Scenario: Up to the walkway
- **WHEN** Eruptor breaks the two barrels and the pot and walks up the ledge route
- **THEN** he reaches the walkway above the gate

### Requirement: What only a Giant moves stays put
The boulders only a Giant lifts, and the stone wall by the spawn that only a Giant's attack or a bomb opens, SHALL stay whole where they stand and block every hero, Giants included, until guacamole runs a Giant's feats.

#### Scenario: Tree Rex at a boulder
- **WHEN** Tree Rex walks into one of the boulders over the clearing's emeralds
- **THEN** it stays where it is and stops him

#### Scenario: Eruptor at the stone wall
- **WHEN** two of Eruptor's Lava Lobs and an Eruption reach the stone wall by the spawn
- **THEN** it stays whole
