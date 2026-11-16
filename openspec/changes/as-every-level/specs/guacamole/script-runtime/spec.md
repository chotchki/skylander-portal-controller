# Spec Delta

## ADDED Requirements

### Requirement: Scripts keep state under their own name
A script that files its state under its placement's name SHALL find that state again each time the placement starts, the name read as the original reads it (the 32-bit FNV-1a of the placement's name, level 201's first vine-bridge activator's 179831879), so what a kid opened or grew stays that way once he has walked off far enough for it to sleep and comes back.

#### Scenario: Level 214's first locked door
- **WHEN** the hero opens level 214's first locked door, goes on into the second island's house and comes back out
- **THEN** the door stands open, as in all seven runs on the TV that watched it past the house (the au-name-checksum lane's five, its review's two, one of them resumed from a save an older build wrote)

#### Scenario: Level 201's first vine bridge
- **WHEN** the hero has grown level 201's first vine bridge and goes off until its activator sleeps, then comes back
- **THEN** the finished bridge stands again with one platform and doesn't grow a second time

### Requirement: A card pirate's level loads the card game's streams when asked
On a level that places a card pirate, the card game's three streams (the reward card's sprites, the board, the hand select) SHALL link beside the level, and a script's request for one SHALL start that stream's running placements, so what the pirate waits on after a game is set up by the stream's own scripts. Nothing of the level's own placements SHALL change for it.

#### Scenario: Dreadbeard's won game on hub stage 2
- **WHEN** a game against Dreadbeard on hub stage 2 ends won and his loot asks for the reward card's sprites
- **THEN** the sprite stream's startup sets the reward macro and he passes his wait for the sprites within 60 steps, nothing stopped

### Requirement: A card game plays on the original's scripts
A yes at a card pirate in a run built to play card games SHALL run the original's own game: the hand the hand select's recommended-hand macro picks, the board's cursor and accept on the pad, the flips, the opponent and the end the scripts' own, each stone's deck and asset loaded as the board asks. The TV SHALL keep answering a yes as a quit until it draws the game.

#### Scenario: Dreadbeard's games quit, lost and won
- **WHEN** a game on hub stage 2 is quit at its first pick, the next is lost and its replay won
- **THEN** he goes on to wait for the player, the board starts its replay, and he goes on to wait for his loot's theatrics, nothing stopped

#### Scenario: A seeded game replays
- **WHEN** two runs play a game from the same seed and the same start
- **THEN** the board's every behaviour and the opponent's every pick come at the same steps
