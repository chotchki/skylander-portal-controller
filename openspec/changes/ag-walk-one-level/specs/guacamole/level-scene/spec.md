# Spec Delta

## Purpose

The level guacamole plays and how a placed figure gets into it: chapter 1 of Skylanders Giants built from the user's own disc, what's in it and solid at the start, its look, how it loads, and how the portal takes the TV into the level and keeps it there.

## ADDED Requirements

### Requirement: The level is chapter 1, from its start
With Giants picked and its extraction finished, the level a character enters SHALL be chapter 1, Time of the Giants, built from the user's own disc: its scenery, sky, lights and fog. The hero SHALL appear where the original puts its first player at the chapter's start, facing the same way.

#### Scenario: Eruptor enters
- **WHEN** Eruptor enters the level
- **THEN** he stands on the flagstone clearing within 1 mm of where the original's first player stands at rest there and facing the same way, the wooden gate beyond him, the waterfall to the right and the rope fence and gourds nearer the camera, as the original shows them

### Requirement: What's there at the start is what the original has
The level SHALL hold the props the original's level file places, where it puts them, but not those it keeps only as patterns its scripts copy. A prop the original wakes by distance SHALL be there while the hero is within that distance, and props its start-up scripts add or remove SHALL stand as they do once its chapter runs. Creatures the original moves by script (enemies, characters and animals) SHALL be left out, solid shapes and all, until guacamole runs their scripts.

#### Scenario: The opening clearing
- **WHEN** the level starts
- **THEN** the gate's door, the gourds and the boulder stand where the original has them at the start, and the hero walks through the spots where the sheep and Norticus stand in the original

#### Scenario: A coin beyond its waking distance
- **WHEN** the hero stands at the start, 49.3 m from the spinning coin the original wakes within 160 ft (48.8 m)
- **THEN** that coin isn't there, and it is once he has walked within 48.8 m of it, as on the original's walk to the gate's door

### Requirement: The level is solid where the original's is
The hero SHALL stand on, slide along and be stopped by the level's own collision and the solid shapes of the props present, as in the original, and SHALL pass through scenery the original lets him pass through. A prop whose solid has no shape SHALL stop nothing.

#### Scenario: The gate's door
- **WHEN** the hero walks straight into the closed door in the gate beyond the start
- **THEN** it stops him with his feet 1.478 m from the door's middle, within 0.01 m, where the original's door stops Eruptor

#### Scenario: A gourd by the start
- **WHEN** the hero walks into the gourd beside the rope fence
- **THEN** it stops him just short of touching it, as the original's does

#### Scenario: Grass
- **WHEN** the hero walks through the grass by the clearing
- **THEN** the grass doesn't stop him

### Requirement: The level is drawn in the remaster look
The level's surfaces SHALL be drawn with the original's shading model in the remaster look (the character-shading and remaster-look capabilities), lit and fogged as the original's opening area is (by its atmosphere's three lights, not the rig the level file stores) and shadowed from its main light. Grass and the water's warp SHALL show, drawn simply until a later change specs their own shading.

#### Scenario: The start under its own lights
- **WHEN** the level shows at the start
- **THEN** its surfaces are lit by the level's lights and fog, Eruptor's shadow falls on the flagstones and the grass shows

#### Scenario: The start's fog and lights
- **WHEN** the level shows at the start
- **THEN** its surfaces fade linearly toward (115, 83, 49) from 30.48 m (100 ft) to 335.28 m (1,100 ft) of depth along the view, wholly past it, under a key light of (172, 134, 18) heading 73° and 30° down, fills of (56, 50, 71) heading 279° and 30° up and (79, 75, 35) heading 215° and 45° down, and an ambient of (39, 42, 67)

### Requirement: The level loads behind the intro
Guacamole SHALL start loading the level when Giants is picked with its extract ready, with no TV frame over 50 ms on the M3 Max while it loads. When the hero's intro ends, or when he's placed if none plays, the level SHALL show within 1 second once it has loaded, and the portal view SHALL hold until it has.

#### Scenario: Placed right after the pick
- **WHEN** a character is placed and its intro skipped before the level has finished loading
- **THEN** the portal view holds until it has loaded, then the level shows with that character as the hero

### Requirement: The level never stalls the TV
On the M3 Max, once the level shows, no TV frame SHALL take longer than 50 ms, including the first time any part of the level or a character comes into view.

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

#### Scenario: Two characters before the start
- **WHEN** Eruptor and then Tree Rex are placed before the level has started
- **THEN** Eruptor's intro plays, then Tree Rex's, then the level shows with Eruptor as the hero and Tree Rex waiting

### Requirement: One hero at a time
While a hero is in the level, a character placed after it SHALL wait on the portal without joining the level. When the hero is removed, the character waiting longest SHALL take its place as the hero.

#### Scenario: A second kid places a character
- **WHEN** a second phone places Tree Rex while Eruptor walks the level
- **THEN** Eruptor stays the only hero, and Tree Rex shows on the phones' portal but not in the level

#### Scenario: The hero comes off with another waiting
- **WHEN** Eruptor is removed while Tree Rex waits on the portal
- **THEN** Tree Rex becomes the hero, coming in where the original brings in a replacement hero

### Requirement: Other figures leave the level as it is
A magic item, an adventure-pack piece or a sidekick placed while the level shows SHALL change nothing in the level; it SHALL show only on the phones' portal.

#### Scenario: An item mid-level
- **WHEN** Sky-Iron Shield is placed while Eruptor walks the level
- **THEN** the level carries on unchanged and the phones show the shield on the portal

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
