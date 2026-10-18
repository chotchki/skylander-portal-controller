# Spec Delta

## MODIFIED Requirements

### Requirement: Each figure on the portal shows as a card
For every occupied portal slot, guacamole's TV SHALL show one card, in slot order, carrying the figure's turning model when it has one (the figure-turntable capability) and its portrait otherwise or while the model loads, its name and element as the phone's collection shows them and, when a profile placed it, that profile's colour and initial. A card SHALL appear within 1 second of the phone showing the figure as on the portal. A card SHALL NOT show a level, experience or gold.

#### Scenario: Placing a figure
- **WHEN** a phone with a profile unlocked places Eruptor in slot 1 after picking Giants
- **THEN** within 1 second the TV shows a card for Eruptor in the first position with his portrait or his turning model, the Fire element and the placing profile's colour and initial, and no level, experience or gold

#### Scenario: The same figure on two profiles
- **WHEN** two phones with different profiles each place the same figure
- **THEN** the TV shows two cards for that figure, each with the colour and initial of the profile that placed it

#### Scenario: No model
- **WHEN** the placed figure has no character model on the disc
- **THEN** its card shows its portrait, name, element and the placing profile's colour and initial
