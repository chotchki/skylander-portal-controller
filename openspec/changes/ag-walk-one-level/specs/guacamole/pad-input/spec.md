# Spec Delta

## Purpose

How guacamole reads a game pad in a level and names its buttons on the TV: buttons by position, the original's stick dead zone, which pad walks the hero and prompts that show the label printed on the pad in the player's hands.

## ADDED Requirements

### Requirement: Buttons are read by position
In a level, guacamole SHALL read a pad's four face buttons by position (bottom, right, left, top), and its sticks, Start and Select, the same way on every pad whatever labels it carries.

#### Scenario: Two 8BitDo variants
- **WHEN** the level is paused and the bottom face button is pressed on the PC variant (A printed there), then it's paused again and the bottom face button is pressed on the Switch variant (B printed there)
- **THEN** both presses resume the level

### Requirement: The move stick's dead zone
The left stick SHALL read each axis as centred while it's a quarter of its travel or less off centre (32 of the 128 steps either side), and past that count from nothing to full at full travel, so a push between the axes walks the hero nearer the closer axis than the stick points. Its strength SHALL be the two axes' combined length, capped at full; the hero's top speed scales with it.

#### Scenario: A stick resting off centre
- **WHEN** the left stick rests a fifth of its travel off centre, or exactly a quarter on one axis
- **THEN** the hero doesn't move

#### Scenario: Just past the dead zone
- **WHEN** one axis is pushed one step past a quarter of its travel
- **THEN** the hero creeps that way at a 96th of his top speed

#### Scenario: Full on both axes
- **WHEN** the left stick reads full travel on both axes at once
- **THEN** the hero walks at the same top speed as with the stick fully forward

#### Scenario: Between the axes
- **WHEN** a round-gate pad's stick is pushed fully 22.5° off forward
- **THEN** the hero walks 11.2° off forward at 91.3 % of his top speed

### Requirement: One pad walks the hero
The hero SHALL follow the first pad connected; another pad's stick and face buttons SHALL NOT move him. When that pad disconnects, or none is connected when the level starts, the level SHALL wait under a message asking for a controller, and a press on any connected pad SHALL make it the hero's and carry on.

#### Scenario: A second pad
- **WHEN** a second pad's stick is pushed while Eruptor walks under the first's
- **THEN** he follows the first pad alone

#### Scenario: The pad goes away
- **WHEN** the pad walking Eruptor is switched off, then a button is pressed on the other
- **THEN** the level pauses under the message, then carries on with the other pad walking him

### Requirement: Prompts name the button printed on the pad
Where the TV names a pad button, it SHALL show the label printed at that position on the player's pad: Xbox-style on a PC-layout pad (A at the bottom, B on the right), Nintendo-style on a Switch-layout pad (B at the bottom, A on the right). For a pad it can't identify it SHALL show the button's position, never a PlayStation symbol or another pad's label.

#### Scenario: The pause overlay on each 8BitDo
- **WHEN** the level is paused from the PC variant, then from the Switch variant
- **THEN** the overlay names the resume button "A" for the first and "B" for the second, the bottom button on both

#### Scenario: A pad guacamole doesn't know
- **WHEN** the level is paused from a pad guacamole can't identify
- **THEN** the overlay shows the bottom position, not a letter or a PlayStation symbol

### Requirement: Start pauses the level
Start on any connected pad SHALL pause the level under an overlay naming the button that resumes it, and nothing in the level SHALL move while it's paused. The bottom face button or Start on any connected pad SHALL resume it.

#### Scenario: Pause and resume
- **WHEN** Start is pressed while Eruptor walks, then the bottom face button
- **THEN** everything stops under the overlay, then carries on from where it stopped
