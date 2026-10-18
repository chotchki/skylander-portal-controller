# Spec Delta

## Purpose

How the guacamole app gets set up and shares one machine with the Skylander Portal Controller: it needs nothing but the user's own disc dumps, keeps profiles of its own that the kids set up again, phones join both apps at the same address, only one of them runs at a time and nothing guacamole does stops the controller from working or changes its profiles, settings or figures.

## ADDED Requirements

### Requirement: The TV's choices answer a controller
Each choice guacamole's TV offers on its setup, found-games and startup-error screens SHALL be a button that is already selected when its screen appears. A connected game controller's bottom face button, Enter or Space SHALL press the selected button. On the setup and found-games screens a click SHALL press a button only when it lands on that button and SHALL do nothing anywhere else. The join code SHALL NOT react to any controller button, key or click.

#### Scenario: Continuing from the found games with a controller
- **WHEN** a folder holding a Skylanders Giants dump was chosen, the TV lists the dumps it found and a game controller's bottom face button is pressed
- **THEN** the TV shows the join code

#### Scenario: A click beside the continue button
- **WHEN** the TV lists the dumps it found and a click lands outside its continue button
- **THEN** the TV still lists the dumps it found right after the click

#### Scenario: Asking for the folder with a controller
- **WHEN** the TV asks for the folder that holds the disc dumps, no folder picker is open and a game controller's bottom face button is pressed
- **THEN** the folder picker opens

#### Scenario: A click beside the folder button
- **WHEN** the TV asks for the folder that holds the disc dumps, no folder picker is open and a click lands outside its choose-the-folder button
- **THEN** no folder picker opens

#### Scenario: Nothing on the join code reacts
- **WHEN** the TV shows the join code and a game controller button, a key or a click is pressed
- **THEN** the TV keeps showing the join code

### Requirement: The startup-error screens close from a controller
When guacamole's TV says another app is already running, or shows a startup error, the app SHALL close when a connected game controller's bottom or right face button is pressed, when Esc is pressed or when a click lands anywhere on the screen.

#### Scenario: Closing the already-running screen with a controller
- **WHEN** the Skylander Portal Controller is serving phones, guacamole is started, its TV says another app is already running and a game controller's bottom face button is pressed
- **THEN** guacamole exits

#### Scenario: Closing a startup error with the right face button
- **WHEN** guacamole's TV shows a startup error and a game controller's right face button is pressed
- **THEN** guacamole exits

#### Scenario: A click anywhere still closes a startup error
- **WHEN** guacamole's TV shows a startup error and a click lands away from its close button
- **THEN** guacamole exits
