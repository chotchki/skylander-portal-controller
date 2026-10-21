# Spec Delta

## ADDED Requirements

### Requirement: The face buttons attack
In a level, the bottom face position SHALL fire the hero's first attack, the left face position and both right shoulder buttons his second, and the right face position and both left shoulder buttons his third, the same position on every pad whatever its label; the top face position SHALL fire no attack, nor a third attack not yet bought (Eruptor's Magma Ball is upgrade 4). Each press SHALL fire one attack at most: holding a button attacks once.

#### Scenario: Two 8BitDo variants
- **WHEN** the button in the first attack's position is pressed on the PC variant, then on the Switch variant
- **THEN** Eruptor throws a lava ball both times

#### Scenario: The second attack's position
- **WHEN** the button in the second attack's position, or either right shoulder button, is pressed while Eruptor stands on the ground
- **THEN** he erupts

#### Scenario: The third attack's position
- **WHEN** the button in the third attack's position, or either left shoulder button, is pressed while Eruptor stands on the ground
- **THEN** with upgrade 4 bought he spits a Magma Ball, 25 or 26 frames (0.42 s) after the press, that rolls off ahead of him; with no upgrades he does nothing

#### Scenario: The top position
- **WHEN** the button in the top face position is pressed while Eruptor stands on the ground, upgrade 4 bought or not
- **THEN** he does nothing

#### Scenario: A button held
- **WHEN** the button for his first, second or third attack is held for 2 s
- **THEN** Eruptor attacks once
