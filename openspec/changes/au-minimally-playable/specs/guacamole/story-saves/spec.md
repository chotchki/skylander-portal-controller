# Spec Delta

## Purpose
A profile's story progress in a game, kept by guacamole and picked up on the next session: what the original's save system keeps, one save per profile per game, the host's alone advancing.

## ADDED Requirements

### Requirement: A profile keeps one story save per game
guacamole SHALL keep one story save per profile per game, with no slot picker. When the original's own scripts save, the save SHALL hold what they keep: the chapters done, the story's state, the collectibles found. Only the save of the profile that started the game (the host) SHALL advance. In co-op, the other profile's save is untouched, while its figures' experience and gold still go to it.

#### Scenario: The host's save advances
- **WHEN** a profile starts Giants, finishes chapter 1 and the game's scripts save
- **THEN** that profile's save holds chapter 1 done

#### Scenario: Another profile's figure in co-op
- **WHEN** a second profile's figure plays beside the host's and the game saves
- **THEN** the second profile's story save is unchanged and its figure's experience and gold are its own

### Requirement: A later session resumes from the save
When a profile with a story save starts the same game again, the TV SHALL take it where the original's load of that save does (the hub, with the chapters it holds done), not to chapter 1. A profile with no save SHALL start at chapter 1 as today.

#### Scenario: Resuming after chapter 1 and 214
- **WHEN** a profile whose save holds chapter 1 and level 214 done starts Giants
- **THEN** the TV goes where the original's load goes, with those chapters done, not into chapter 1

#### Scenario: A fresh profile
- **WHEN** a profile with no save starts Giants
- **THEN** chapter 1 starts as it does today

### Requirement: A save from an older build never stops a start
A save whose layout the running build doesn't read SHALL be kept unchanged and named in the log. That profile SHALL start as a fresh one, and a save it makes on that build SHALL be kept beside the old one, never over it. No build SHALL delete a save it can't read, and the start SHALL never fail on one.

#### Scenario: A layout the build doesn't read
- **WHEN** a profile's save was written by a build with a different save layout
- **THEN** the log names it, the profile starts at chapter 1, and the save is still there for a build that reads it
