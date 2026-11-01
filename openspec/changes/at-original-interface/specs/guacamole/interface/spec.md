# Spec Delta

## Purpose

What the TV draws of the original game's own interface while a level shows: the sprites and text the original's interface scripts make, where, how big, in what colour and order, in the disc's own art and fonts; and the original's dialogue, hints, pause menu and upgrade screen, each as its scripts lay it out.

## ADDED Requirements

### Requirement: The interface draws what the original's scripts set
While a level shows, the TV SHALL draw every interface sprite the original's scripts make visible, with the disc's own image or text, at the place, size, colour, transparency and order they give it, changing within a frame of their change, and SHALL draw nothing they hide or remove. A sprite of a kind the TV can't draw yet (a model, an emitter or a movie) SHALL be drawn as nothing and named once in the log.

#### Scenario: The HUD the join builds
- **WHEN** Eruptor joins chapter 1
- **THEN** the TV draws the HUD his join's scripts build, its box 237 by 96 in the interface's 640 by 480 space with its bottom-left corner 20 above the screen's bottom-left

#### Scenario: A sprite removed with what hangs from it
- **WHEN** the scripts remove a sprite that others hang from
- **THEN** none of them is drawn from the next frame on

### Requirement: Text in the disc's own fonts
Text SHALL be drawn with the disc's own bitmap fonts (three: 42, 82 and 84 px tall with 177, 178 and 175 characters, Latin only) in English, a character its font lacks drawn as nothing. Text the original's scripts fit to a box by measuring it SHALL measure as the original's does, so the box ends at the size and the text on the lines the original's does.

#### Scenario: The HUD's numbers
- **WHEN** Eruptor's HUD shows his level and gold
- **THEN** each digit is the font's own character from the disc, placed at the spacing the font gives it

#### Scenario: A box fitted to its text
- **WHEN** a dialogue's text is longer than its box's first size
- **THEN** the box grows to the width and number of lines the original's grows to for that text

### Requirement: The interface between the level and the TV's own screens
The original's interface SHALL draw over the level and under the TV's own screens: a portal alert, the join code in the corner, a modal and an intro SHALL each cover it. Its 640 by 480 space SHALL be mapped onto the picture as the original maps it onto its 16:9 frame, keeping that shape in a window of any other shape.

#### Scenario: An alert over the HUD
- **WHEN** the call for a Skylander shows while the HUD is up
- **THEN** none of the HUD shows through the alert

#### Scenario: A window of another shape
- **WHEN** the TV's window is taller than 16:9
- **THEN** the interface keeps the original's proportions, bands filling the rest

### Requirement: Blends and colours as the original's
Each sprite SHALL blend with what's under it as the original's do: over by its own transparency, added, subtracted or by the transparency of what's under it (a frame of play draws 72 interface sprites, 30, 18, 11 and 13 of them those ways), and SHALL take its colour and replacement colour as the original's do, a colour of 128 on every channel leaving the art as the disc holds it.

#### Scenario: The plain colour
- **WHEN** a sprite's colour is 128, 128, 128 at full opacity
- **THEN** its art draws exactly as the disc holds it

#### Scenario: Faded out
- **WHEN** a sprite's opacity falls to 0
- **THEN** nothing of it is seen

### Requirement: Dialogue and hints as the original's
A dialogue, a hint, a story tablet or a collectible's box in a level SHALL show as the original's scripts lay it out: its box, its speaker, its text and any yes-or-no choice, the choice taken from the pad as the original takes it. A line that waits on its voice SHALL wait as the original's does once the voice plays.

#### Scenario: The cannon's line
- **WHEN** Eruptor comes up to the cannon in chapter 1
- **THEN** the line the original shows there appears in its box, laid out as the original's

#### Scenario: A yes-or-no choice
- **WHEN** a box asks yes or no and the pad's bottom face button is pressed on yes
- **THEN** the box closes and what follows a yes in the original follows

### Requirement: Start opens the original's pause menu
In a level, Start SHALL hold the level and open the original's pause menu, its choices and the pages behind them as the original's, moved through with the pad; backing out of it or pressing Start again SHALL close it and let the level carry on where it stood.

#### Scenario: Pause and back
- **WHEN** Start is pressed while Eruptor walks chapter 1, then Back
- **THEN** the level holds under the original's pause menu, then carries on from where it stood

### Requirement: The original's upgrade screen
Where the original opens its upgrade screen, the TV SHALL show the original's for the figure on the portal: its pages, the figure's own upgrade art and each upgrade's state and price as the original's scripts lay them out from that figure's kept upgrades.

#### Scenario: Eruptor's upgrades
- **WHEN** the upgrade screen opens for an Eruptor with no upgrades bought
- **THEN** it shows his own upgrade art and every upgrade unbought at its original price
