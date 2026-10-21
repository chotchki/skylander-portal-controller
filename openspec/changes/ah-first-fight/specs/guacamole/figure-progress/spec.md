# Spec Delta

## Purpose

A figure's progress in guacamole: one record per profile per figure in guacamole's own store, fresh on the profile's first placement, earned in play, levelled by the original's experience table, and shown, edited and reset to fresh from the phone.

## ADDED Requirements

### Requirement: Each profile keeps its own progress for each figure
Guacamole SHALL keep each figure's experience, gold and playtime per profile, one record per profile per figure, shared across games and kept across restarts. A profile's first placement of a figure SHALL start it fresh: level 1, no experience, no gold, no playtime. Two profiles placing the same figure SHALL each have their own.

#### Scenario: A first placement
- **WHEN** Alice places Eruptor for the first time while Bob's Eruptor has 500 gold
- **THEN** Alice's Eruptor is level 1 with no experience and no gold, and Bob's still has 500 gold

#### Scenario: Kept across a restart
- **WHEN** Alice's Eruptor has collected 16 experience and guacamole is quit and started again
- **THEN** her Eruptor still has 16 experience

### Requirement: Progress earned in play lands on the placing profile at once
Experience and gold a hero collects SHALL be added to the figure of the profile that placed it the moment it's collected, without waiting for a save point, and SHALL be kept if guacamole closes or the figure leaves the portal straight after. It SHALL NOT reach any other profile. A figure placed with no profile unlocked SHALL earn in play but keep nothing once it leaves the portal.

#### Scenario: Bob's Eruptor beats a Chompy
- **WHEN** Bob's Eruptor collects a Chompy's four experience orbs and guacamole is closed straight after
- **THEN** Bob's Eruptor has 4 more experience the next time guacamole starts, and no other profile's figure changed

#### Scenario: Taken off straight after
- **WHEN** Bob's Eruptor collects a coin and is taken off the portal in the same moment
- **THEN** Bob's Eruptor has that coin's gold

#### Scenario: No profile unlocked
- **WHEN** a figure placed with no profile unlocked collects a coin and is then removed
- **THEN** no profile's figure has gained that gold

### Requirement: Levels follow the original's experience table
A figure's level SHALL be the original's for its total experience: level 2 from 1,000, then 2,200, 3,800, 6,000, 9,000, 13,000, 18,200, 24,800, 33,000, 42,700, 53,900, 66,600 and 80,800; level 15, the cap, from 96,500. Gold SHALL stop at 65,000, as the original stops it. Experience past 96,500 SHALL change nothing a kid sees: the original keeps counting it in the figure's data with the level held at 15, and guacamole keeps 96,500.

#### Scenario: One short of level 2
- **WHEN** a figure with 999 experience collects a 1-point orb
- **THEN** it is level 2 with 1,000 experience

#### Scenario: At the caps
- **WHEN** a figure with 96,499 experience and 64,999 gold collects five 1-point orbs and coins worth 2 and 1
- **THEN** it is level 15 with its full health there and has 65,000 gold

### Requirement: Playtime counts the time in play
A figure's playtime SHALL grow by the time it spends on the portal while a level shows, as the original counts it: in play, in the pause menu and on the screen of a beaten hero alike, each figure on the portal counting its own.

#### Scenario: A minute in the level
- **WHEN** Eruptor walks the level as the hero for a minute, then the level is paused for a minute
- **THEN** his playtime has grown by two minutes, as the original's would

#### Scenario: Waiting on the portal
- **WHEN** Tree Rex waits on the portal for a minute while Eruptor walks the level
- **THEN** each one's playtime has grown by a minute

#### Scenario: Beaten and still on the portal
- **WHEN** Eruptor is beaten and stays on the portal a minute before he's taken off
- **THEN** his playtime has grown by that minute

### Requirement: The phone shows a figure's progress
On a phone with a profile unlocked, a figure's details in guacamole SHALL show that profile's level, gold and playtime for it, and a figure the profile has never placed as never placed. A change to the level or gold, in play or from another phone, SHALL show on every phone showing it within 1 second. Playtime SHALL show at most a minute behind while the figure is in a level, and exactly once it leaves.

#### Scenario: After the first fight
- **WHEN** Alice opens Eruptor's details after he collected 16 experience and 3 gold for her
- **THEN** they show level 1, 3 gold and his playtime

#### Scenario: Never placed
- **WHEN** Alice opens the details of a figure she has never placed
- **THEN** they show it as never placed, with editing available

#### Scenario: A second phone watching
- **WHEN** Bob's phone shows Bob's Eruptor while Bob's Eruptor collects a coin in the level
- **THEN** Bob's phone shows the new gold within 1 second

#### Scenario: Playtime while he plays
- **WHEN** Bob's phone shows Bob's Eruptor while he walks the level for three minutes, and then he's taken off the portal
- **THEN** the playtime shown is never more than a minute behind his play, and matches it once he's off

### Requirement: The phone edits its own profile's figure
A phone SHALL set its unlocked profile's figure to any level the game's table holds (1 to 15 in Giants), its experience becoming that level's threshold, and any gold from 0 to the game's cap. It SHALL refuse while that profile's own placement of the figure is on the portal, not for another profile's, and SHALL NOT edit a figure of a profile it hasn't unlocked. Editing a never-placed figure SHALL start its record. Nothing earned before an edit SHALL land on top of it.

#### Scenario: Setting a level
- **WHEN** Alice sets her Eruptor, off the portal, to level 10 and 1,234 gold
- **THEN** her Eruptor has 33,000 experience, level 10 and 1,234 gold, on every phone and in the next level

#### Scenario: Her own Eruptor on the portal
- **WHEN** Alice tries to edit her Eruptor while her own placement of him is on the portal
- **THEN** the phone says he's on the portal and nothing changes

#### Scenario: Another profile's Eruptor on the portal
- **WHEN** Alice edits her Eruptor while only Bob's Eruptor is on the portal
- **THEN** the edit goes through and Bob's Eruptor is unchanged

#### Scenario: A sibling's figure
- **WHEN** a phone with Alice unlocked asks to edit Bob's Eruptor
- **THEN** it's refused and Bob's Eruptor is unchanged

#### Scenario: An edit straight after play
- **WHEN** Alice takes her Eruptor off the portal straight after he collects a coin and at once sets him to level 10 and 50 gold
- **THEN** he has level 10 and 50 gold, with nothing from before the edit added on top

### Requirement: Reset to fresh
Resetting a figure to fresh from the phone SHALL drop that profile's progress for it, so its next placement starts fresh. Resetting a portal slot SHALL put that slot's figure back to fresh where it stands, unless it's the hero in a level: then the phone SHALL say so and nothing SHALL change. Nothing earned before a reset SHALL come back after it.

#### Scenario: Reset, then placed
- **WHEN** Alice resets her level 10 Eruptor to fresh and places him
- **THEN** he is level 1 with no experience, no gold and no playtime

#### Scenario: A slot reset in the level
- **WHEN** the slot holding Alice's Eruptor, the hero in the level, is reset to fresh
- **THEN** the phone says he's in the level and his progress is unchanged

#### Scenario: A slot reset while waiting
- **WHEN** the slot holding Alice's level 10 Tree Rex, waiting on the portal while Eruptor walks the level, is reset to fresh
- **THEN** her phone shows Tree Rex at level 1 with no gold, and he comes into the level at level 1
