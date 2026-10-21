# Spec Delta

## ADDED Requirements

### Requirement: The face buttons attack
In a level, the bottom face position SHALL fire the hero's first attack and the left face position and both right shoulder buttons his second, the same position on every pad whatever its label; the right and top face positions and the left shoulder buttons SHALL fire no attack of an Eruptor with no upgrades. Each press SHALL fire one attack at most: holding a button attacks once.

#### Scenario: Two 8BitDo variants
- **WHEN** the button in the first attack's position is pressed on the PC variant, then on the Switch variant
- **THEN** Eruptor throws a lava ball both times

#### Scenario: The second attack's position
- **WHEN** the button in the second attack's position, or either right shoulder button, is pressed while Eruptor stands on the ground
- **THEN** he erupts

#### Scenario: A button held
- **WHEN** the button for his first or second attack is held for 2 s
- **THEN** Eruptor attacks once
