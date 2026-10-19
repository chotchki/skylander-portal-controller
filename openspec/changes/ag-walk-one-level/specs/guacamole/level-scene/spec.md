# Spec Delta

## Purpose

The level guacamole plays and how a placed figure gets into it: chapter 1 of Skylanders Giants built from the user's own disc, what's in it and solid at the start, its look, and how the portal takes the TV into the level and keeps it there.

## ADDED Requirements

### Requirement: The level is chapter 1, from its start
With Giants picked and its extraction finished, the level a character enters SHALL be chapter 1, Time of the Giants, built from the user's own disc: its scenery, sky, lights and fog. The hero SHALL appear where the original puts its first player at the chapter's start, facing the same way.

#### Scenario: Eruptor enters
- **WHEN** Eruptor enters the level
- **THEN** he stands on the flagstone clearing within 1 cm of where the original's first player starts and facing the same way, the wooden gate beyond him, the waterfall to the right and the rope fence and gourds nearer the camera, as the original shows them

### Requirement: What's there at the start is what the original has
The level SHALL hold, where the original places them, the props it has in place when the chapter starts and none it brings in only later. Creatures the original moves by script (enemies, characters and animals) SHALL be left out, solid shapes and all, until guacamole runs their scripts.

#### Scenario: The opening clearing
- **WHEN** the level starts
- **THEN** the gate's door, the gourds and the boulder stand where the original has them at the start, and the hero walks through the spots where the sheep and Norticus stand in the original

### Requirement: The level is solid where the original's is
The hero SHALL stand on, slide along and be stopped by the level's own collision and the solid shapes of the props present, as in the original, and SHALL pass through scenery the original lets him pass through.

#### Scenario: The gate's door
- **WHEN** the hero walks into the closed door in the gate beyond the start
- **THEN** it stops him where it stops the original's hero

#### Scenario: Grass
- **WHEN** the hero walks through the grass by the clearing
- **THEN** the grass doesn't stop him

### Requirement: The level is drawn in the remaster look
The level's surfaces SHALL be drawn with the original's shading model in the remaster look (the character-shading and remaster-look capabilities), lit by the level's own lights, fogged by its fog and shadowed from its main light. Grass and the water's warp SHALL show, drawn simply until a later change specs their own shading.

#### Scenario: The start under its own lights
- **WHEN** the level shows at the start
- **THEN** its surfaces are lit by the level's lights and fog, Eruptor's shadow falls on the flagstones and the grass shows

### Requirement: The level never stalls the TV
Once the level shows, no TV frame SHALL take longer than 50 ms, including the first time any part of the level or a character comes into view.

#### Scenario: New scenery comes into view
- **WHEN** the hero walks to where scenery he hasn't seen yet comes into view
- **THEN** no frame takes longer than 50 ms

### Requirement: The first character placed walks into the level
With Giants picked and its level extracted, placing a character on the portal SHALL take the TV into the level with that character as the hero, after its intro when one plays. A figure that isn't a character (a magic item, an adventure-pack piece or a sidekick) SHALL NOT start the level or become the hero.

#### Scenario: Eruptor first
- **WHEN** Eruptor is the first figure placed after Giants is picked
- **THEN** his intro plays and then the TV shows the level with Eruptor at its start

#### Scenario: An item first
- **WHEN** Sky-Iron Shield is placed before any character
- **THEN** the TV stays on the portal view with its card, and the first character placed after it starts the level

### Requirement: One hero at a time
While a hero is in the level, a character placed after it SHALL wait on the portal without joining the level. When the hero is removed, the character waiting longest SHALL take its place as the hero.

#### Scenario: A second kid places a character
- **WHEN** a second phone places Tree Rex while Eruptor walks the level
- **THEN** Eruptor stays the only hero, and Tree Rex shows on the phones' portal but not in the level

#### Scenario: The hero comes off with another waiting
- **WHEN** Eruptor is removed while Tree Rex waits on the portal
- **THEN** Tree Rex becomes the hero, coming in where the original brings in a replacement hero

### Requirement: Removing the hero pauses the level
When the hero is removed and no character waits, the level SHALL pause under a message asking for a Skylander on the portal. The next character placed SHALL become the hero where the original brings in a replacement, and the level SHALL carry on from where it paused rather than restart.

#### Scenario: Swapping heroes
- **WHEN** Eruptor is removed partway across the clearing and then Spyro is placed
- **THEN** the level pauses under the message, then carries on with Spyro as the hero where the original brings a replacement in, the rest of the level as it was

### Requirement: The level takes the TV
While a level is showing, the TV SHALL show it full screen with the small join code in a corner and no figure cards.

#### Scenario: Three figures on the portal
- **WHEN** Eruptor walks the level with two more figures on the portal
- **THEN** the TV shows the level and the small join code, and no cards
