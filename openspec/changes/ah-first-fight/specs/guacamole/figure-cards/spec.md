# Spec Delta

## MODIFIED Requirements

### Requirement: Each figure on the portal shows as a card
On the portal view, guacamole's TV SHALL show one card per occupied portal slot, in slot order, carrying the figure's turning model when it has one (the figure-turntable capability) and its portrait otherwise or while the model loads, its name and element as the phone's collection shows them and, when a profile placed it, that profile's colour and initial. A card SHALL appear within 1 second of the phone showing the figure as on the portal.

#### Scenario: Placing a figure
- **WHEN** a phone with a profile unlocked places Eruptor in slot 1 after picking Giants
- **THEN** within 1 second the TV shows a card for Eruptor in the first position with his portrait or his turning model, the Fire element and the placing profile's colour and initial

#### Scenario: The same figure on two profiles
- **WHEN** two phones with different profiles each place the same figure
- **THEN** the TV shows two cards for that figure, each with the colour and initial of the profile that placed it

#### Scenario: No model
- **WHEN** the placed figure has no character model on the disc
- **THEN** its card shows its portrait, name, element and the placing profile's colour and initial

#### Scenario: A level showing
- **WHEN** a figure is placed while a hero walks the level
- **THEN** the TV shows no card over the level, and the phones show the figure on the portal

## ADDED Requirements

### Requirement: A card shows the figure's level and gold
A card for a figure a profile placed SHALL show the level and gold that profile's figure has, following a change within 1 second. A card for a figure no profile placed SHALL show no level or gold.

#### Scenario: Alice's Eruptor
- **WHEN** Alice places her Eruptor, level 3 with 120 gold, on the portal view
- **THEN** his card shows level 3 and 120 gold

#### Scenario: The same figure on two profiles
- **WHEN** Alice's level 3 Eruptor and Bob's fresh Eruptor are both on the portal view
- **THEN** Alice's card shows level 3 and Bob's level 1 with no gold

#### Scenario: No profile unlocked
- **WHEN** a figure is placed with no profile unlocked
- **THEN** its card shows no level or gold

#### Scenario: A slot reset
- **WHEN** the slot holding Alice's level 10 Eruptor is reset to fresh while the portal view shows
- **THEN** within 1 second his card shows level 1 and no gold

## REMOVED Requirements

### Requirement: Figure progress is not shown or edited yet
**Reason**: guacamole keeps figure progress of its own from AH, one record per profile per figure, so the phone's figure details show it and its edit and reset-to-fresh actions work on it.
**Migration**: The phone's figure details, editing and reset to fresh in guacamole follow the figure-progress capability's requirements ("The phone shows a figure's progress", "The phone edits its own profile's figure", "Reset to fresh").
