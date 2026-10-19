# Spec Delta

## ADDED Requirements

### Requirement: The face buttons attack
In a level, the face buttons SHALL fire the hero's first, second and third attacks from the positions the original reads them at, the same position on every pad whatever its label, and holding one SHALL repeat its attack only as the original's does.

#### Scenario: Two 8BitDo variants
- **WHEN** the button in the first attack's position is pressed on the PC variant, then on the Switch variant
- **THEN** Eruptor throws a lava ball both times

#### Scenario: The second attack's position
- **WHEN** the button in the second attack's position is pressed while Eruptor stands on the ground
- **THEN** he erupts
