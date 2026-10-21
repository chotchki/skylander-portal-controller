# Spec Delta

## Purpose

Which figures guacamole offers, what it calls them and what the phone shows before it knows: the roster the user's own Giants disc defines.

## ADDED Requirements

### Requirement: The figure list is the disc's own roster
Once guacamole has read the roster of a recognized Giants dump, the phone's collection SHALL list every figure that disc defines and nothing else: each character, Giant, alternate deco, Series 2 figure, LightCore figure, magic item, adventure-pack piece and sidekick. Every profile SHALL get the same list.

#### Scenario: The 01.00 disc
- **WHEN** the roster of the recognized BLUS30968 01.00 dump has been read
- **THEN** the collection lists 123 figures: 45 character bases, 8 Giants, 15 alternate decos, 24 Series 2, 8 LightCore, 10 magic items, 5 adventure-pack pieces and 8 sidekicks

#### Scenario: Figures the interim list lacked
- **WHEN** a phone opens the collection with Giants picked
- **THEN** it lists Series 2 Whirlwind, LightCore Hex, Legendary Jet-Vac, Jade Flashwing, Golden Dragonfire Cannon, Dragon's Peak and Eye-Small among the rest

### Requirement: Names come from the disc
Each figure's name SHALL be the disc's own English text for it. A figure the disc marks as a variant of a character SHALL be named with the disc's variant word ahead of the character's name and SHALL sit with that character's other figures as one of its variants. A figure the disc defines as a toy of its own, such as Dark Spyro or Legendary Bash, SHALL keep the name the disc gives it.

#### Scenario: An alternate deco
- **WHEN** the collection lists Whirlwind's alternate deco
- **THEN** it's named "Polar Whirlwind" and shows as one of Whirlwind's variants

#### Scenario: Series 2 and LightCore
- **WHEN** the collection lists the Series 2 Whirlwind and LightCore Hex figures
- **THEN** they're named "Series 2 Whirlwind" and "LightCore Hex"

### Requirement: Elements and portraits come from the disc
Each figure SHALL show the element the disc gives it, except that a magic item or adventure-pack piece carrying the disc's default element value SHALL show none. Every figure SHALL have a portrait, the disc's own collection-screen portrait for that figure, the right way up as the game draws it.

#### Scenario: Eruptor
- **WHEN** the collection lists Eruptor
- **THEN** he shows the Fire element and the disc's portrait of him

#### Scenario: A magic item
- **WHEN** the collection lists Anvil Rain
- **THEN** it shows its own portrait and no element

#### Scenario: Portraits the right way up
- **WHEN** the phone's collection or a card on the TV shows Eruptor's portrait
- **THEN** his eyes are above his grin, as on the game's own collection screen

#### Scenario: The four characters the wiki data lacked
- **WHEN** the collection lists Dino-Rang, Wham-Shell, Drill Sergeant and Jet-Vac
- **THEN** they show Earth, Water, Tech and Air, each with its own portrait

### Requirement: Series 2 and LightCore figures are figures of their own
A Series 2 or LightCore figure SHALL be an entry of its own with its own portrait, apart from its base character's. A profile SHALL be able to have both on the portal at once, and anything guacamole keeps per figure for a profile SHALL be kept separately for the two.

#### Scenario: Hex and LightCore Hex
- **WHEN** one profile places Hex and then LightCore Hex
- **THEN** both are placed, each in its own slot with its own card

### Requirement: guacamole reads no wiki data
guacamole SHALL take its figures, names, elements and portraits from the user's disc alone, the names it matches intros by included, and the game picker's art from the dump's own icon. It SHALL start, offer Giants and list figures with the repository's scraped data (the figure list, its portrait images and the game box art) absent.

#### Scenario: No scraped data
- **WHEN** guacamole starts with the scraped figure list, portrait images and box art missing from where it used to read them
- **THEN** it starts normally, the game picker shows Giants with the disc's own icon, and once the roster has been read the collection lists all 123 figures with their portraits

### Requirement: No figures until the roster is read
Until guacamole has read the roster of a recognized Giants dump, the phone's collection SHALL list no figures and SHALL say why: the figures are being read off the disc, or the dump isn't one guacamole recognizes. When the roster lands, the collection SHALL fill in within 2 seconds without the phone reloading.

#### Scenario: First pick
- **WHEN** Giants is picked for the first time with the known dump and a phone opens the collection before the roster has been read
- **THEN** the collection says the figures are being read off the disc, then lists them within 2 seconds of the roster being read

#### Scenario: An unrecognized dump
- **WHEN** the Giants dump setup found isn't one guacamole recognizes
- **THEN** the collection lists no figures and says the disc wasn't recognized

### Requirement: Figures saved before keep their identity
A figure a profile used or left on the portal under guacamole's earlier figure list SHALL come back as the same figure under the disc roster, including the variants whose identity the disc defines differently.

#### Scenario: A saved Polar Whirlwind
- **WHEN** a profile whose last portal layout, saved before the disc roster, holds Polar Whirlwind unlocks and resumes its last setup
- **THEN** Polar Whirlwind is placed in its saved slot under that name
