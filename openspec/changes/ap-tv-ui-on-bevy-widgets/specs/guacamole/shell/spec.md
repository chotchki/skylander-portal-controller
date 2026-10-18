# Spec Delta

## Purpose

How the guacamole app gets set up and shares one machine with the Skylander Portal Controller: it needs nothing but the user's own disc dumps, keeps profiles of its own that the kids set up again, phones join both apps at the same address, only one of them runs at a time and nothing guacamole does stops the controller from working or changes its profiles, settings or figures.

## ADDED Requirements

### Requirement: The TV's choices answer a controller
Each choice on guacamole's TV setup screens (asking for the dump folder, no Skylanders Giants dump found, the dumps found) and startup-error screens SHALL be a button already selected when its screen appears. A connected game controller's bottom face button, Enter or Space SHALL press the selected button. On the setup screens a click SHALL press a button only when it lands on it and SHALL do nothing elsewhere. The join code SHALL NOT react to any controller button, key or click.

#### Scenario: Continuing from the found games with a controller
- **WHEN** a folder holding a Skylanders Giants dump was chosen, the TV lists the dumps it found and a game controller's bottom face button is pressed
- **THEN** the TV shows the join code

#### Scenario: A click beside the continue button
- **WHEN** the TV lists the dumps it found and a click lands outside its continue button
- **THEN** the TV still lists the dumps it found right after the click

#### Scenario: Asking for the folder again with a controller
- **WHEN** the TV asks for the folder that holds the disc dumps, the folder picker it opened by itself was closed without a choice and a game controller's bottom face button is pressed
- **THEN** the folder picker opens again

#### Scenario: A click beside the folder button
- **WHEN** the TV asks for the folder that holds the disc dumps, the folder picker it opened by itself was closed without a choice and a click lands outside its choose-the-folder button
- **THEN** no folder picker opens

#### Scenario: A click beside the folder button when no Giants dump was found
- **WHEN** the TV says no Skylanders Giants dump was found in the chosen folder and a click lands outside its choose-the-folder button
- **THEN** no folder picker opens and the TV still says no Skylanders Giants dump was found

#### Scenario: Nothing on the join code reacts
- **WHEN** the TV shows the join code and a game controller button or a key is pressed, or a click lands anywhere on the screen
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
