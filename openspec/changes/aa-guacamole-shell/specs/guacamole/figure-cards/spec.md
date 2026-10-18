# Spec Delta

## Purpose

What the phone offers and the guacamole TV shows while phones drive the portal: the whole Giants figure list for every profile with no figure file anywhere, how to join, then one card per figure on the portal with who it is and who placed it. Every figure starts fresh in guacamole, so no figure progress is shown or edited until guacamole keeps its own.

## ADDED Requirements

### Requirement: Every profile sees the whole Giants figure list
With Skylanders Giants picked, the phone's collection SHALL list the same figures for every profile: each figure guacamole knows that Giants plays, with its name, its element and its portrait where it has one. The list SHALL NOT depend on figure files or figure dumps on the machine, or on which figures a profile has placed before. A figure guacamole can't name SHALL be left out rather than shown under another figure's name.

#### Scenario: A brand-new profile
- **WHEN** a profile created a moment ago unlocks and Giants is picked
- **THEN** the collection lists the same figures, names and elements as for any other profile

#### Scenario: Both games' characters
- **WHEN** a phone opens the collection with Giants picked
- **THEN** it lists Spyro's Adventure characters such as Spyro and Giants characters such as Tree Rex

### Requirement: The TV shows how to join
Until a game is picked, guacamole's TV SHALL show the join QR code, the join address as text and how many phones are connected. After a game is picked the join QR code SHALL stay on screen, smaller, so another player can still join.

#### Scenario: Fresh start
- **WHEN** guacamole has started and no phone has connected
- **THEN** the TV shows the join QR code, the join address and zero connected players

#### Scenario: A phone connects
- **WHEN** a phone opens the join address
- **THEN** the TV's connected-player count goes to one within 2 seconds

### Requirement: Each figure on the portal shows as a card
For every occupied portal slot, guacamole's TV SHALL show one card, in slot order, carrying the figure's portrait when it has one, its name and element as the phone's collection shows them and, when a profile placed it, that profile's colour and initial. A card SHALL appear within 1 second of the phone showing the figure as on the portal. A card SHALL NOT show a level, experience or gold.

#### Scenario: Placing a figure
- **WHEN** a phone with a profile unlocked places Eruptor in slot 1 after picking Giants
- **THEN** within 1 second the TV shows a card for Eruptor in the first position with its portrait, the Fire element and the placing profile's colour and initial, and no level, experience or gold

#### Scenario: The same figure on two profiles
- **WHEN** two phones with different profiles each place the same figure
- **THEN** the TV shows two cards for that figure, each with the colour and initial of the profile that placed it

#### Scenario: No portrait
- **WHEN** the placed figure has no portrait
- **THEN** its card shows the name, element and the placing profile's colour and initial with no portrait

### Requirement: Cards follow the portal
When a figure leaves the portal its card SHALL leave the TV within 1 second. When a slot's figure is replaced, by a phone or by a Kaos swap, the TV SHALL show the new figure's card in that position instead. With all eight slots occupied the TV SHALL show eight cards, none hidden or overlapping.

#### Scenario: Removing a figure
- **WHEN** a phone removes the figure in slot 3
- **THEN** within 1 second the TV no longer shows that card and the other cards keep their positions

#### Scenario: A Kaos swap
- **WHEN** a Kaos swap replaces the figure in slot 2 with another figure from the collection
- **THEN** the TV shows the new figure's card in the second position and the old card is gone

#### Scenario: A full portal
- **WHEN** eight figures are on the portal
- **THEN** the TV shows eight cards, each fully visible

### Requirement: A failed placement shows no card
When a placement fails, the phone SHALL show it the way any failed placement shows (an error message, the slot back to its previous state), and the TV SHALL show no card for it.

#### Scenario: A placement fails
- **WHEN** a phone places a figure and the placement fails
- **THEN** the phone shows an error and the slot returns to what it showed before, and the TV shows no new card

### Requirement: Figure progress is not shown or edited yet
Until guacamole keeps figure progress of its own, the phone's figure details in guacamole SHALL show no level, experience or gold, and the phone's actions for editing a figure's level and gold and for resetting a figure to fresh SHALL be unavailable and say so.

#### Scenario: Opening a figure's details
- **WHEN** a phone with a profile unlocked opens Eruptor's details in guacamole
- **THEN** no level, experience or gold is shown, and the edit and reset-to-fresh actions are disabled with a note that they aren't available yet
