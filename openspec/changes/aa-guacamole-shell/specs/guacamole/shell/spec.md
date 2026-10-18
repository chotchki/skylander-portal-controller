# Spec Delta

## Purpose

How the guacamole app gets set up and shares one machine with the Skylander Portal Controller: it needs nothing but the user's own disc dumps, keeps profiles of its own that the kids set up again, phones join both apps at the same address, only one of them runs at a time and nothing guacamole does stops the controller from working or changes its profiles, settings or figures.

## ADDED Requirements

### Requirement: Setup asks only where the disc dumps are
The guacamole app SHALL need nothing to start except the folder or folders that hold the user's disc dumps. It SHALL NOT need the Skylander Portal Controller's setup, an RPCS3 install, PS3 firmware, a figure pack or figure dumps. On its first start guacamole's TV SHALL ask for the folder, and once one is chosen it SHALL list the game dumps it found there by title and remember the folder for later starts. guacamole SHALL find a dump in the chosen folder itself or up to two folder levels below it.

#### Scenario: A machine with nothing but disc dumps
- **WHEN** guacamole is started for the first time on a machine that has never run the Skylander Portal Controller and has no RPCS3 or figure files, and a folder holding a Skylanders Giants dump is chosen
- **THEN** the TV lists Skylanders Giants as found and then shows the join code, a phone can create a profile and the phone's game list offers Skylanders Giants

#### Scenario: A folder of several discs
- **WHEN** the chosen folder holds dumps of Skylanders Giants and other games, each in a subfolder of its own
- **THEN** the TV lists every dump it found by title, and Skylanders Giants is the game guacamole offers

#### Scenario: Started again
- **WHEN** guacamole is started again after setup was done
- **THEN** the TV doesn't ask for the folder and goes straight to the join code

### Requirement: Phones join at the same address with a pairing of guacamole's own
The guacamole app SHALL serve the phone app at the same join address (host name and port) as the Skylander Portal Controller on that machine, with a pairing key of its own that stays the same across guacamole's restarts. A phone holding a pairing that guacamole doesn't accept SHALL be asked to scan the join code on guacamole's TV, and once it has, its signed commands SHALL be accepted.

#### Scenario: A phone that last joined the controller opens guacamole
- **WHEN** a phone saved the join link while the Skylander Portal Controller was running, the controller was closed, guacamole was started and the saved link is opened
- **THEN** the phone app loads from guacamole and asks for the TV's join code, and after the phone scans guacamole's join code its portal commands are accepted

#### Scenario: Restarted
- **WHEN** a phone joined guacamole by scanning its join code and guacamole is closed and started again
- **THEN** the phone's portal commands are accepted without scanning again

### Requirement: guacamole keeps profiles and PINs of its own
The guacamole app SHALL keep its own profiles, PINs and profile colours, separate from the Skylander Portal Controller's. On its first start it SHALL have no profiles, and a phone SHALL be able to create one with a name, a colour and a 4-digit PIN. A profile SHALL unlock only with its own PIN. Profiles SHALL survive guacamole's restarts. A profile created, changed or deleted in either app SHALL NOT appear in or change the other.

#### Scenario: First start on a machine that has the controller's profiles
- **WHEN** guacamole is started for the first time on a machine where the Skylander Portal Controller has profiles
- **THEN** the phone lists no profiles in guacamole and offers to create one

#### Scenario: Creating a profile and unlocking it
- **WHEN** a phone creates a profile in guacamole with PIN 1234
- **THEN** the phone lists that profile, PIN 4321 does not unlock it and PIN 1234 does

#### Scenario: Profiles survive a restart
- **WHEN** a profile was created in guacamole and guacamole is closed and started again
- **THEN** the phone lists that profile and its PIN still unlocks it

#### Scenario: The two apps' profiles stay apart
- **WHEN** a profile is created on the phone while guacamole runs, guacamole is shut down and the controller is started
- **THEN** the controller's phone app doesn't list the new profile, and the controller's own profiles and PINs are as they were

### Requirement: Only one app serves phones at a time
When the Skylander Portal Controller or another copy of guacamole is already serving phones on the machine, the app started second SHALL NOT serve phones and SHALL NOT open its profile data. Its TV SHALL say that another app is already running. The app that was already running SHALL keep working as if nothing happened.

#### Scenario: guacamole starts while the controller runs
- **WHEN** the Skylander Portal Controller is serving a connected phone and guacamole is started
- **THEN** guacamole's TV says another app is already running, the phone stays connected to the controller and keeps working, and guacamole has created no profile data of its own

#### Scenario: The controller starts while guacamole runs
- **WHEN** guacamole is serving phones and the Skylander Portal Controller is started
- **THEN** the controller shows its startup-error screen naming the busy port, and guacamole's phones keep working

### Requirement: Skylanders Giants is the only game offered
The phone's game list SHALL contain Skylanders Giants and nothing else, and only when a folder guacamole was set up with holds a Giants dump (BLUS30968). Picking Giants SHALL take the phone to its portal view without starting any emulator.

#### Scenario: A Giants dump was found
- **WHEN** the folder guacamole was set up with holds a BLUS30968 dump and a phone opens the game list
- **THEN** the list shows only Skylanders Giants, picking it opens the phone's portal view and no RPCS3 process starts

#### Scenario: No Giants dump
- **WHEN** no folder guacamole was set up with holds a BLUS30968 dump
- **THEN** the phone shows its no-games state, and guacamole's TV says no Skylanders Giants dump was found in the chosen folder and offers to choose another

### Requirement: The controller's settings and figures are left alone
The guacamole app SHALL NOT read or change any profile's figure files from the Skylander Portal Controller, SHALL NOT change the controller's profiles, PINs or settings, and SHALL NOT change the controller's window mode, render scale, RPCS3 settings or per-game display mode. The phone's grown-ups toggles for the window mode and render scale SHALL report that they are unavailable in guacamole, the phone's menu SHALL NOT offer the action that opens the RPCS3 settings while guacamole runs, and picking a game in guacamole SHALL NOT change the display's resolution or refresh rate.

#### Scenario: Window-mode toggle under guacamole
- **WHEN** a grown-up uses the phone's window-mode toggle while guacamole runs
- **THEN** the phone reports the setting is unavailable, and the controller's next start uses the window mode it had before

#### Scenario: No RPCS3 settings in guacamole's menu
- **WHEN** a grown-up opens the phone's menu while guacamole runs and no game is picked
- **THEN** the menu has no action for the RPCS3 settings, and the controller's menu still offers it before a game is picked

#### Scenario: Giants played in guacamole leaves the controller's display mode alone
- **WHEN** Giants was picked and figures were placed in guacamole, and Giants is later launched in the controller
- **THEN** the controller applies the same display mode it applied for Giants before guacamole ever ran

#### Scenario: A figure played in guacamole keeps its progress in the controller
- **WHEN** a kid places Eruptor in guacamole and later places Eruptor with the same profile in the controller
- **THEN** the controller's Eruptor has the same level, experience and gold it had before guacamole ran

### Requirement: Shutting down from the phone closes guacamole
SHUT DOWN on the phone SHALL show a farewell on guacamole's TV and close the app within 5 seconds, returning the screen to whatever launched it.

#### Scenario: Shut down from the phone
- **WHEN** guacamole was launched full screen on the TV and a phone uses SHUT DOWN
- **THEN** the TV shows the farewell, guacamole exits within 5 seconds and what launched it is back on screen
