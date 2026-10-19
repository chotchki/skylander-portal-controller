# Spec Delta

## MODIFIED Requirements

### Requirement: A placed character turns on its card
On the portal view, for each occupied slot whose figure has a character model on the disc, the TV SHALL show that model on the figure's card: the disc's own geometry and textures, posed by the character's own idle animation playing in a loop, turning about its vertical axis at a steady rate, the whole model inside the card. The card's name, element and owner pip SHALL stay as they are.

#### Scenario: Eruptor
- **WHEN** a phone places Eruptor while Giants' level is still extracting
- **THEN** his card shows Eruptor's model idling and turning, with his name, the Fire element and the placing profile's colour and initial

#### Scenario: A Giant
- **WHEN** a phone places Tree Rex while the portal view shows
- **THEN** his whole model, head to feet, stays inside his card all the way around a turn

### Requirement: The turntable never stalls the TV
On the M3 Max, placing a figure and its model appearing SHALL NOT make any TV frame take longer than 50 ms, from the first placement after guacamole starts. When the portal view follows a figure's intro, the figure's model SHALL be on its card by the time the intro ends; with no intro, within 1 second of its placement.

#### Scenario: The first placement after a start
- **WHEN** guacamole has just started and a phone places the first figure since
- **THEN** no TV frame takes longer than 50 ms, and if the portal view follows the intro the figure's model is on its card when it ends

### Requirement: Eight turning figures fit
On the portal view, with all eight slots holding figures that have models, the TV SHALL show eight cards, each with its model turning, none hidden, cut off or overlapping.

#### Scenario: A full portal
- **WHEN** eight characters are on the portal while the portal view shows
- **THEN** the TV shows eight turning models, each whole inside its own card
