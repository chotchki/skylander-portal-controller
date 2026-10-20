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
An alternate deco and a figure the disc defines as a toy of its own (Dark Spyro and the Spyro's Adventure legendaries) SHALL show the model the disc loads for that figure. A Series 2 or LightCore figure SHALL show its base character's model, the one the disc loads for it.

#### Scenario: Crusher and Granite Crusher
- **WHEN** Crusher and Granite Crusher are both on the portal
- **THEN** the two cards show two different models, each the one the disc loads for that figure

#### Scenario: Series 2 Whirlwind
- **WHEN** Series 2 Whirlwind is placed
- **THEN** its card shows Whirlwind's model

### Requirement: Models show no upgrades until progress is kept
Every figure is fresh until guacamole keeps figure progress, so a model SHALL show only the parts the original game draws for that character with no upgrades bought.

#### Scenario: Eruptor without upgrades
- **WHEN** Eruptor is placed
- **THEN** his card shows him without the spikes his upgrades add

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
