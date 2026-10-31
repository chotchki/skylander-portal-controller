# Spec Delta

## ADDED Requirements

### Requirement: The interface is extracted with the rest
Extraction SHALL include the interface: every sprite, image and font and the English text of the shared archives, the character archives, the menu archives and each level it extracts (409 sprites, 310 images and 906 texts in the main shared archive alone). An extract made before the interface SHALL extract the interface alone at the next start, keeping everything it already holds.

#### Scenario: An extract made before the interface
- **WHEN** guacamole starts on a Giants extract made before the interface was extracted
- **THEN** it extracts the interface alone (nothing else again) and the HUD draws once that's done

#### Scenario: A first run
- **WHEN** Giants is picked for the first time
- **THEN** the interface extracts with the rest, before a placed character enters the level
