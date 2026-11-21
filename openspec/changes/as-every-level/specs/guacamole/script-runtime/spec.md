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

### Requirement: A report passes on the player's press
While a report the scripts put up after a find waits on its continue (a hat's, a treasure's, a soul gem's, a story scroll's, a charm's, a reward card's, the rewards'), the joined player's bottom face SHALL pass it as the original's accept does. No other script SHALL take that press as its accept, so a talk's yes-or-no stays the TV's to answer, and the d-pad, start, select and cancel SHALL pass nothing.

#### Scenario: Chapter 1's first hat box, the TV's own pass off
- **WHEN** its report is up and nothing is pressed for 300 steps, then the bottom face once
- **THEN** it holds unpressed and is gone 36 steps after the press

#### Scenario: A kid mashes attack through hub stage 1's talks
- **WHEN** the bottom face goes down two steps in every eight through Auric's and Hatterson's talks
- **THEN** each asks the TV its one question and ends on the TV's answer, at the steps it ends unmashed

### Requirement: A card game plays on the original's scripts
A yes at a card pirate in a run built to play card games SHALL run the original's own game: the hand the hand select's recommended-hand macro picks, the board's cursor and accept on the pad, the flips, the opponent and the end the scripts' own, each stone's deck and asset loaded as the board asks. The TV SHALL keep answering a yes as a quit until it draws the game.

#### Scenario: Dreadbeard's games quit, lost and won
- **WHEN** a game on hub stage 2 is quit at its first pick, the next is lost and its replay won
- **THEN** he goes on to wait for the player, the board starts its replay, and he goes on to wait for his loot's theatrics, nothing stopped

#### Scenario: A seeded game replays
- **WHEN** two runs play a game from the same seed and the same start
- **THEN** the board's every behaviour and the opponent's every pick come at the same steps

### Requirement: The hero returns after a card game
Once a card game's teardown asks for the hero's character again and no scene holds the players, he SHALL come back as a figure put back on the portal does, his own spawn sequence letting him go at its end.

#### Scenario: Dreadbeard's game quit
- **WHEN** a game on hub stage 2 is quit at its first pick and his quit scene ends
- **THEN** the hero is back, put back after the scene, and a full stick walks him over 3 units in 150 steps, nothing stopped

### Requirement: The card game's templates play their clips
The extract SHALL hold the card game's streams' and stones' templates' palettes, read by the levels' palettes rule, and a level that links the card game SHALL give those templates their clips' lengths, kinds, loops and names, so the board's flips time by them.

#### Scenario: A card stream's template on the TV's load
- **WHEN** the TV installs a level whose card stream places a template naming a model the card palettes hold
- **THEN** that template has the palette's lengths, loops and names, and the level's own placements none of them

### Requirement: The card game's opponent picks as its scripts read
The opponent's best-move roll writes the first move's stone into the move it chooses, which the typed set reference refuses, so the opponent SHALL never play the best move: the second at its call's second chance, else the last.

#### Scenario: A thousand games against Dreadbeard
- **WHEN** 1,000 seeded games are played against hub stage 2's Dreadbeard, his chances 70 and 70
- **THEN** of the full lists' picks the first is played none, the second within 5 points of 70% and the last within 5 points of 30%

### Requirement: A screen position off the disc reads x from its low half
A packed screen position the disc holds, a literal or a file's screen global, SHALL read x from its low half and y from its high, and keep one order inside the VM and the host, so the card board's neighbours, edges, cursor and camera agree on one axis and the HUD's places land on the original's: 15 of 51 of AC.13's HUD quads within 1 px of the oracle's, where the other reading fails its main box.

#### Scenario: A stone placed beside another on the card board
- **WHEN** a stone is placed with a stone one square across from it
- **THEN** its side facing that stone is the one compared, and the opponent's edge check for that side is on the column

#### Scenario: Fang's opponent's first roll
- **WHEN** 150 seeded weak games are played against Fang
- **THEN** his first roll lands near 2 draws in 101, 19 of 770 measured, its call's 1 rolled as `random 100` with both ends
