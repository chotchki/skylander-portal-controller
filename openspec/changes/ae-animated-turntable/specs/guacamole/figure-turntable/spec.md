# Spec Delta

## Purpose

What guacamole's TV shows for a figure on the portal once the figure has a model on the disc: the character itself, textured, idling and turning on its card, in the remaster look.

## ADDED Requirements

### Requirement: A placed character turns on its card
For each occupied slot whose figure has a character model on the disc, the TV SHALL show that model on the figure's card: the disc's own geometry and textures, posed by the character's own idle animation playing in a loop, turning about its vertical axis at a steady rate, the whole model inside the card. The card's name, element and owner pip SHALL stay as they are.

#### Scenario: Eruptor
- **WHEN** a phone places Eruptor and his intro ends
- **THEN** his card shows Eruptor's model idling and turning, with his name, the Fire element and the placing profile's colour and initial

#### Scenario: A Giant
- **WHEN** a phone places Tree Rex
- **THEN** his whole model, head to feet, stays inside his card all the way around a turn

### Requirement: Each figure shows its own model
Each figure SHALL show the model the original game draws for it in play: an alternate deco or a toy of its own (Dark Spyro, the Spyro's Adventure legendaries) its own model, a Series 2 or LightCore figure its base character's. Where the game's portal screen draws another model than the one the figure plays as, the card SHALL show the one it plays as.

#### Scenario: Crusher and Granite Crusher
- **WHEN** Crusher and Granite Crusher are both on the portal
- **THEN** the two cards show two different models, each the one the original game draws for that figure

#### Scenario: Series 2 Whirlwind
- **WHEN** Series 2 Whirlwind is placed
- **THEN** its card shows Whirlwind's model

#### Scenario: Punch Pop Fizz
- **WHEN** Punch Pop Fizz is placed
- **THEN** her card shows the model she plays as in the original game, not the one its portal screen draws for her

### Requirement: A figure carries what it carries fresh and no upgrades
Every figure is fresh until guacamole keeps figure progress, so a card SHALL show the character with exactly what the original game hangs on it as a fresh figure lands (weapons and gear, each on its bone or on the part it hangs from, following it through the idle) and nothing an upgrade adds. A Series 2 or LightCore figure SHALL carry its base character's set. Where guacamole can't tell what a figure carries, its card SHALL show the body alone.

#### Scenario: Eruptor without upgrades
- **WHEN** Eruptor is placed
- **THEN** his card shows him without the spikes his upgrades add, and carrying nothing

#### Scenario: Stealth Elf holds her knives
- **WHEN** Stealth Elf is placed
- **THEN** her card shows a knife in each hand, each following its hand through her idle, and not the knives her upgrades swap in

#### Scenario: A part on a part
- **WHEN** Gill Grunt is placed
- **THEN** his card shows his harpoon gun in his hand with its harpoon on the gun, and his backpack on his back

#### Scenario: LightCore Prism Break
- **WHEN** LightCore Prism Break is placed
- **THEN** its card shows Prism Break's crystals on the hands and shoulders, as Prism Break's does

### Requirement: A figure without a model keeps its portrait
A figure with no character model on the disc (magic items, adventure-pack pieces and sidekicks, for now) SHALL keep its portrait on its card, and so SHALL a figure whose model couldn't be read, which guacamole SHALL log with the reason. A card SHALL show the portrait while its model loads. A model that can't be shown SHALL NOT stop the TV or another card.

#### Scenario: A magic item
- **WHEN** Anvil Rain is placed
- **THEN** its card shows its portrait

#### Scenario: A model that won't read
- **WHEN** the extracted model of a placed figure is damaged
- **THEN** its card shows its portrait, the log names the figure and why, and the other cards keep turning

### Requirement: The turntable never stalls the TV
On the M3 Max, placing a figure and its model appearing SHALL NOT make any TV frame take longer than 50 ms, from the first placement after guacamole starts. A figure's model SHALL be on its card by the time its intro ends, or within 1 second of its placement when it has no intro.

#### Scenario: The first placement after a start
- **WHEN** guacamole has just started and a phone places the first figure since
- **THEN** no TV frame takes longer than 50 ms, and the figure's model is on its card when the intro ends

### Requirement: A start gets the graphics ready first
Before the TV shows a screen the setup or a phone acts on, and before any extraction, guacamole SHALL get ready every pipeline it draws the disc's characters, a card and chapter 1 with, under a screen saying so. On a Mac that has drawn them before, that screen SHALL show no longer than the Mac takes to find them, under a second on the M3 Max. Nothing after it SHALL get one of them ready in a TV frame.

#### Scenario: A Mac's first start
- **WHEN** guacamole starts on a Mac that has never drawn Giants
- **THEN** the TV says it's getting ready until it is, then shows the join code, and the first pick, its extraction, a placement and chapter 1 loading make no TV frame wait on a pipeline

#### Scenario: A later start
- **WHEN** guacamole starts again on the same Mac
- **THEN** the getting-ready screen is up for under a second before the join code

### Requirement: Eight turning figures fit
With all eight slots holding figures that have models, the TV SHALL show eight cards, each with its model turning, none hidden, cut off or overlapping.

#### Scenario: A full portal
- **WHEN** eight characters are on the portal
- **THEN** the TV shows eight turning models, each whole inside its own card
