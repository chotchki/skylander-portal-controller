# Spec Delta

## Purpose

What a hero's attacks break in a level, what a broken thing leaves and drops, the ways breaking opens, and what waits for a Giant's feats, starting with chapter 1's barrels, pots and gourds, as the RPCS3 oracle shows the original Skylanders Giants.

## ADDED Requirements

### Requirement: Attacks break barrels, pots and gourds
A barrel, pot or gourd SHALL break after as many hits from each of the hero's attacks as the original's takes, leave the debris the original's leaves and stop blocking the hero once broken, wherever the debris doesn't.

#### Scenario: A barrel by the ledge route
- **WHEN** Eruptor hits a barrel at the foot of the ledge route with Lava Lobs until it breaks
- **THEN** it breaks on the same hit as the original's and he can walk where it stood

### Requirement: What a broken thing drops
A broken barrel, pot or gourd SHALL drop a coin by a running count, not a fresh chance each time: each break adds the level's drop rate (0.6 in chapter 1) to a count that wraps past 1, dropping when the count is at or under the rate, so three of every five breaks in a row drop. Where the count starts SHALL be drawn when the level starts. Each coin SHALL be worth 1 or 2, two in three worth 1.

#### Scenario: Five in a row
- **WHEN** Eruptor breaks five of the clearing's barrels, pots and gourds in a row
- **THEN** exactly three of them drop a coin

#### Scenario: The coins' worth
- **WHEN** 300 coins have dropped from broken barrels, pots and gourds
- **THEN** between 176 and 224 of them are worth 1 and the rest 2

### Requirement: Breaking opens the ledge route
With the two barrels and the pot at the foot of the ledge route broken, the hero SHALL climb it to the walkway above the gate, as in the original, and reach the spinning coins along it.

#### Scenario: Up to the walkway
- **WHEN** Eruptor breaks the two barrels and the pot and walks up the ledge route
- **THEN** he reaches the walkway above the gate

### Requirement: What only a Giant moves stays put
The boulders only a Giant lifts in the original SHALL stay where they stand and block every hero, Giants included, until guacamole runs a Giant's feats.

#### Scenario: Tree Rex at a boulder
- **WHEN** Tree Rex walks into one of the boulders over the clearing's emeralds
- **THEN** it stays where it is and stops him
