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
- **THEN** its surfaces fade linearly toward (115, 83, 49) from 30.48 m (100 ft) to 335.28 m (1,100 ft) of depth along the view, wholly past it, the sky not at all, under a key light of (172, 134, 18) heading 73° and 30° down, fills of (56, 50, 71) heading 279° and 30° up and (79, 75, 35) heading 215° and 45° down, and an ambient of (39, 42, 67)

### Requirement: The sky draws behind the level
The level's sky SHALL draw where nothing else of the level is, after its solid surfaces and before anything else that blends, its own see-through layers (clouds, horizon rings, the cap) over one another in the order the level lists them, none of them hiding the sky or anything behind it where it's clear.

#### Scenario: Clouds under the sky's cap
- **WHEN** the hero stands on the walkway past the gate with the follow camera behind him
- **THEN** the sky across the top of the view shows its cap over the cloud layers, with no hard-edged band where a cloud layer's outline cuts it

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

#### Scenario: The first time a Mac draws the level
- **WHEN** chapter 1 loads and first shows on a Mac that has never drawn it, after the start got the graphics ready
- **THEN** no frame of the load or the first show waits on a pipeline, and none takes longer than 50 ms

### Requirement: The level draws at the display's rate
While the level shows, the TV SHALL draw every frame its display shows, up to 240 Hz, whether its window has the focus or not, each frame moving the hero, his pose and the camera on from the last by the time between them, never repeating the last frame's picture of him or jumping two steps. Figure cards the level hides draw nothing.

#### Scenario: Another app in front
- **WHEN** Eruptor walks in the 1280x720 window on the Mac's 120 Hz screen while another app has the focus
- **THEN** the TV draws 120 frames a second, and each one moves him, his pose and the camera on from the last

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
- **THEN** Eruptor's intro plays, then Tree Rex's, then the TV holds under the too-many alert instead of showing the level, and once Tree Rex comes off the level shows with Eruptor at its start

### Requirement: One hero at a time
While a hero is in the level, a second character placed SHALL hold the level, within 1.5 s, under a full-screen alert that there are too many Skylanders on the portal, the hero and the creatures still, until one comes off: the second taken off, the hero carries on from where he stopped; the hero taken off, the second arrives and comes in where the hero stood. Two characters on the portal when the level would start SHALL hold it the same way.

#### Scenario: A second kid places a character
- **WHEN** a second phone places Tree Rex while Eruptor walks the level
- **THEN** within 1.5 s the level holds under the too-many alert with Eruptor and the creatures still, and once Tree Rex comes off Eruptor carries on from where he stopped

#### Scenario: The hero comes off with another waiting
- **WHEN** Eruptor is removed while Tree Rex's placement holds the level
- **THEN** Tree Rex arrives and comes in within 0.15 m of where Eruptor stood, facing his way within 0.2°, and the level carries on

### Requirement: Other figures leave the level as it is
A magic item or a sidekick placed while the level shows SHALL play its arrival and then leave the hero and the level as they were, showing only on the phones' portal until guacamole runs the items' own scripts; an adventure-pack piece SHALL change nothing. A second magic item or sidekick on the portal (Giants counts a sidekick as a magic item) SHALL hold the level under an alert that there are too many magic items on the portal until one comes off.

#### Scenario: An item mid-level
- **WHEN** Sky-Iron Shield is placed while Eruptor walks the level
- **THEN** its arrival plays, then the level carries on with Eruptor where he was and the phones show the shield on the portal

#### Scenario: A sidekick beside an item
- **WHEN** Barkley is placed while Sky-Iron Shield is on the portal and Eruptor walks the level
- **THEN** within 1.5 s the level holds under the too-many-magic-items alert, and once either comes off the level carries on

### Requirement: Removing the hero pauses the level
When the hero is removed with no other character on the portal, the level SHALL hold within 1.5 s, hidden behind a screen asking player 1 to put a Skylander on the portal, nothing in it moving. The next character placed SHALL arrive and come in where the hero stood when he came off, facing his way, and the level SHALL carry on from where it held rather than restart.

#### Scenario: Swapping heroes
- **WHEN** Eruptor is removed partway across the clearing and then Spyro is placed
- **THEN** within 1.5 s the level holds behind the message with nothing moving, then Spyro arrives and comes in within 0.15 m of Eruptor's last place, facing his heading within 0.2°, the rest of the level as it was

### Requirement: A character coming in arrives
Each time a character becomes the hero after the level has started, and each time a magic item or a sidekick is placed while it shows, the TV SHALL first show that figure's arrival full screen, the level held meanwhile: the figure alone over its element's colours with its name for about 3 s, from within 1.5 s of what brought it in (its placement, or the hero coming off for one the too-many alert held), then the level again 5-8 s after that. No intro SHALL play.

#### Scenario: A replacement arrives
- **WHEN** Eruptor has been taken off mid-level and Tree Rex is placed
- **THEN** within 1.5 s Tree Rex's arrival shows him named over his element's colours, and 5-8 s after the placement the level shows again with Tree Rex in Eruptor's place

### Requirement: A character coming in drops onto his place
A character coming in after the level has started SHALL appear, as the level shows again, about 0.4 m above where the last hero stood and a few centimetres across, fall onto the ground in 7 or 8 frames, and play his spawn animation from 5 or 6 frames after he lands, over about a second after he appeared. He SHALL then stand where he landed: within 7 cm of the last hero's place on gentle ground, further down a steep slope.

#### Scenario: Eruptor put back at rest
- **WHEN** Eruptor is taken off standing on the gentle slope at the foot of the ledge route and his figure is put back
- **THEN** 5.4-5.5 s after the placement he appears 0.41 m above his last place and 4.8 cm across, falling, lands 7-8 frames later 4.9-6.8 cm from it, and his spawn animation plays from 5-6 frames after the landing, over within 60 frames of his appearing

#### Scenario: Eruptor put back after a run up the slope
- **WHEN** Eruptor is taken off running up the ledge route and his figure is put back
- **THEN** he appears 0.38 m above where he was taken off and 8.5 cm across, slides down the slope as he falls, and lands 8 frames later 0.29-0.30 m from that place

### Requirement: A figure Giants can't use holds the level
A figure Giants has no use for (a character from a later game, or a kind it doesn't know, such as a trap or a vehicle) placed at any time SHALL hold the TV within 1.5 s and, within 2.5 s, cover it with a full-screen alert that a toy on the portal can't be used with this game, asking for it to be removed, the level and the figure untouched and the pad's stick and bottom button doing nothing; within 1 s of it coming off the TV SHALL show what it showed before.

#### Scenario: A later game's character mid-level
- **WHEN** Fryno is placed while Eruptor walks the level
- **THEN** within 1.5 s the alert covers the TV with Eruptor and the creatures still, and within 1 s of Fryno coming off the level shows again with Eruptor where he stopped

#### Scenario: A trap at the start
- **WHEN** a trap is placed while the TV asks for a Skylander to start the level
- **THEN** the alert covers the TV, and once the trap comes off the TV asks for a Skylander again

### Requirement: The level takes the TV
While a level is showing, the TV SHALL show it full screen with the small join code in a corner and no figure cards.

#### Scenario: A figure beside the hero
- **WHEN** Eruptor walks the level with Sky-Iron Shield on the portal beside him
- **THEN** the TV shows the level and the small join code, and no cards
