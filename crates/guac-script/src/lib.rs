//! The tfbScript interpreter (openspec AH.14, design decisions 3 and 21):
//! the original game's own scripts, read from the user's disc by
//! `guac-formats` (AH.13's op graphs), run as the original runs them. Pure:
//! no Bevy, no I/O. Engine state is reached through a [`Host`]; guacamole's
//! host is a `SystemParam` over its ECS queries, the tests' host plain
//! structs.
//!
//! This is the semantics, in our own words, written before the code and
//! from aj-vm's and ah-progression's reading of Giants (BLUS30968 01.00);
//! the decompile only explained what those show. Where a rule is a reading
//! the oracle hasn't confirmed, it says so.
//!
//! # Programs and placements
//!
//! A *program* ([`Program`]) is the op lists of the files loaded together
//! (`bootstrap.bld`, `global.bld` and a level or character file), linked: every
//! op, operand and object a list reaches gets an id, `global.bld` imports
//! resolve through the export ids (bootstrap's roots first), and a macro or
//! structure *stub* (a definition in no list) resolves by name to the
//! definition of that name that sits in a list; read as a value, a stub is
//! that definition, so two files naming one macro name one object (AH.19:
//! Eruptor's file names `State - On Fire` through a stub, and a Chompy's
//! list of states holds `global.bld`'s definition; the original's lookup
//! of the state finds it, AH.10's lob stunning a Chompy). A script's `_masterVarList`
//! is linked too: the variables a placement's locals are built from. A
//! *placement* is one thing in the world running one script: the VM keeps,
//! per placement, whether its STARTUP has run, its current behaviour, its
//! local variables, whether a shutdown is pending, whether a `remove` took it
//! off and its seconds of ticks. The script's op objects are shared by every
//! placement running it.
//!
//! A placement is one value however a script reaches it: the embedder names
//! each placement's object in its file with the handle it runs under
//! ([`Vm::add_at`], [`Vm::name_object`], for placements with no script too),
//! and that object reads as the handle, as `my` does.
//!
//! # The tick
//!
//! Once a logic tick each placement's script runs from position 0 with no
//! yields: take the op at the program counter, move the counter past it, run
//! the op; until the counter passes the end of the list it's in or the
//! current behaviour's block ends. Nothing waits inside a tick. What takes
//! time (an animation, a slide, a move) is started by an op and polled by
//! the script on later ticks through members. After each tick the
//! placement's `system.script timer` grows by the tick's seconds
//! ([`Config::tick_seconds`], 1/60 s), so it reads 0 through the first.
//!
//! Before the frame's scripts the sliders step, once ([`Vm::slide`]; Slides
//! below).
//!
//! The order is the original scheduler's, as AH.59 recorded it a frame at a
//! time on the oracle (the scheduler's lists at every frame's pad read; the
//! decompile explaining them): placements tick group by group, each group a
//! list walked from its head. The *activators* ([`Vm::set_activator`]: the
//! hero's own minion, the one placement the others' activation ranges are
//! measured from) tick first, then every other awake placement in its list's
//! order. A placement joins its group's list at the end: added, woken or
//! spawned. The walk takes each place as the list stands when it gets there,
//! so one joining the end during the walk ticks that step too, and one
//! leaving a place the walk has passed doesn't make it skip the next.
//!
//! A placement whose `placement.script suspended` is set isn't ticked: the
//! walk passes it, its place on its list, its state, its locals and its
//! `system.script timer` kept (the tick is what adds a frame's seconds to
//! it), and once a script clears it, it ticks again at that place the next
//! time the walk reaches it: that step when a placement ahead of it on the
//! list cleared it, the next when one behind it did. Set in its own tick,
//! the rest of that tick runs. The member is the VM's: on a placement it
//! runs, the flag (0 or 1, a write clamped there, a reset clearing it); on
//! anything else a path through nothing. Read off the EBOOT, not the oracle
//! (openspec AS.29: no route reaches a level that sets it): the frame's
//! scripts updater takes each placement's activity, then runs its tick
//! unless its flag 4 is set, reading the flag as it comes to the placement
//! (aj-vm's and AH.59's reading), so its activity still follows the
//! embedder's rule while it's suspended; the member reads and writes that
//! flag, a reset putting back its reset template's, clear in every file on
//! the disc (all 51,686 placements' flag words are 0). PvP's bricks suspend
//! themselves until the brick under them goes, a challenge's replay
//! suspends a troll. A reading: a suspended placement taken off still runs
//! its SHUTDOWN (whether the shutdown queue passes one isn't read).
//!
//! One sent round its states by a `reset` ([`Vm::requeue`]: the mover
//! update, which the music controller's tick resets every frame) leaves its
//! list, its `system.script timer` back to 0, and goes back on the list's end
//! once the walk has passed every group, ticking there: after every other
//! placement that step, those that joined the list behind it during the walk
//! included. AH.59's recordings show both halves: the mover update's timer
//! reads that frame's own time at every record, and the eruption's lava,
//! spawned by its tick after the music controller's, lists ahead of the
//! mover update at its first record (`fight`'s three waves). Behind it are
//! only placements reset after it (two push blocks' water ends, Norticus'
//! encounter) and what Norticus' tick then spawned. A script's own `reset`
//! of another placement doesn't send it round yet: the host resets its
//! buffer alone (openspec AH.80).
//!
//! A placement's `reset` of itself does, from its STARTUP (openspec AS.67):
//! once the host has answered it, its SHUTDOWN, if it has one and has
//! started, is pending and runs at the tick's end as a removal's would;
//! after it the placement has its started flag and behaviour cleared, its
//! locals gone (each made again at its declared value as it's next
//! reached), its timer back to 0, and rejoins the list's end as a requeue
//! does, running STARTUP there; where it stands is the host's. A reading,
//! no recording: level 214's bomb resets itself after its burst, and its
//! script needs each half: a bounce count it only ever adds to, which
//! would burst the next bomb at its first landing, and a lit fuse, glow
//! and gesture manager its SHUTDOWN alone takes off. The dead automaton
//! parts of levels 003, 202 and 209 say the same: one explodes and resets
//! itself, and only a STARTUP gives it a fresh fuse; kept where it was,
//! its behaviour stays `explode` and it explodes every tick. One that
//! resets itself again on the way (in that STARTUP) waits for the next
//! step, as a requeue sent again does: at most two rounds in its first
//! step, one a step after.
//!
//! The embedder puts placements to sleep and wakes them by its activity
//! rule ([`Vm::set_activity`], a pass at a time, or [`Vm::set_asleep`]);
//! one asleep isn't ticked and keeps its state, and waking puts it at its
//! group's end: placements tick in the order they woke. AH.10's encounter
//! end shows it too (Norticus, woken last, takes his scene the frame the
//! last Chompy goes, and `Setup_001_Controller`, woken first, sees it a
//! frame later). The asleep wait on a list of their own, each joining its
//! end as it goes to sleep, and those waking in one pass wake in that
//! list's order, whatever order the pass names them in (openspec AH.195:
//! AF.8's fresh state holds level 213's 468 asleep placements in its file's
//! root-list order, the order its load put them to sleep in, and 54 of
//! the 56 that woke in one frame 13.18 s before it in that order too
//! (the other two at their end, an earlier frame's doing), behind the 29
//! awake from the load; the embedder adds a file's placements in that
//! order). One that has started goes to sleep through the shutdown queue
//! (openspec AR.22): the step's walk still ticks it once at its place, its
//! seconds counted, then the queue, once the walk is done, clears its
//! started flag and runs its SHUTDOWN as a taken-off placement's (below),
//! if its script has one; woken, its `system.script timer` starts from 0
//! and it runs STARTUP again. One never started, held, inert, taken off or
//! stopped gets none of that, and one woken before the step's walk never
//! slept: it ticks on started, its timer kept. The recordings settle it:
//! the oracle's census has the first combat watcher, whose own write of an
//! activation range of 0 takes it to sleep, started and `Watching` at one
//! read and neither 90 flips on, its SHUTDOWN's stinger the music by the
//! read after (AR.16); in AH.59's and AH.195's scheduler reads no asleep
//! placement is ever started, the 17 that went to sleep started each had
//! that frame's time added and the 9 that went unstarted none, and the 6
//! woken again each restarted its timer at one frame's. AH.59's decompile
//! reads the same way (an aid): the state-change handler queues a started
//! sleeper, and the tick runs a started placement whatever its state.
//!
//! The asleep are checked again after the walk and those sent round
//! (openspec AS.83, [`Host::wakes`]): the host's rule is asked which of
//! them it finds awake now, in the asleep list's order, and each it names
//! wakes onto its group's end and ticks there, that step, before the
//! shutdown queue. AH.59's recordings put the original's asleep group
//! after the awake one in its updater's walk, and its decompile (an aid)
//! has that walk take each placement's activity, then tick it if it's
//! awake now, so a placement whose range a script writes wakes and runs
//! its STARTUP the step of the write. A reading, no recording: level 214's
//! elf helper writes its two elves' range, then the next tick asks combat
//! for each, which knows a creature only once its STARTUP has run; woken
//! the step after, as the pass between steps alone wakes one, both read
//! nothing and its writes on them dropped. A placement put to sleep this
//! step isn't asked; the pass between steps still wakes what a hero's walk
//! brings into range, ahead of the walk's requeued (where the original's
//! asleep group ticks it after them).
//!
//! A placement a tick spawns with a script
//! of its own ([`Host::adopt`]) joins the end and ticks that same tick:
//! AH.54's recordings list the eruption's first wave of lava at the
//! eruption's own first record, which its first tick spawns. It joins at
//! its spawner's turn's end, or sooner: a `find variable` on anything not
//! yet in the VM adopts what's been spawned so far, a native's included,
//! the order the same: the portal manager finds its new death actor's
//! `dying actor` the op after the spawn and sets it, which reads as the
//! original's spawn making the placement, variables and all, at once
//! (AH.39's Mac step). Outside a step's walk the embedder adds what's
//! spawned. A held
//! placement (below) runs what the embedder does in its place natively
//! ([`Vm::step_with`]), where the walk reaches it: design decision 2's
//! player loop runs the portal manager's and the game systems' ticks so.
//!
//! `remove` takes a placement off: the rest of the tick it's in runs (a
//! script taking itself off finishes its tick), then it's never ticked again;
//! a tell of it skips its block. One that has started runs its SHUTDOWN once
//! on the way out (openspec AH.200, on the oracle: Eruptor's figure taken off
//! runs his minion's, his `Minion - Shutdown` taking him out of
//! `Minions_Loaded` and his description out of `combat control data`'s
//! `creature descriptions` between 5 and 20 frames after the figure left, and
//! the next Eruptor's join putting one back, so the set holds at most one
//! minion's entry at every read). Taking itself off, it runs it at its own
//! tick's end (a shutdown pending, below); taken off by another placement or
//! the embedder, in a tick of its own once the step's walk is done
//! ([`Vm::shut_down`]: the original's queue of shutdowns after its groups,
//! aj-vm's reading): PRESCRIPT from its start, STARTUP skipped, to its
//! SHUTDOWN, whose end ends the tick before PRESCRIPT's own ops past its
//! sections, its `system.script timer` not growing. Either way its started
//! flag is cleared after (AH.4's census shows level 213's definition so:
//! removed, not started, one tick's seconds). One never started, held, inert
//! or stopped runs none, nor one whose script has no SHUTDOWN. A whitelist
//! listing a script for its first tick alone ticks its placements until
//! their STARTUP has run, then holds them, listed ([`Vm::held`]). A
//! placement the embedder adds under the handle of one taken off (a
//! character's file loaded again after his swap, openspec AH.105) runs under
//! that handle from its own first tick; the one taken off stays off.
//!
//! An op's *block* is the `_branchPC` ops after it. An op either skips its
//! block (counter += block length), runs into it (and may *push* an entry on
//! the block stack: the list, the counter at push, the block length and the
//! op), or is a *flow* that closes entries. Closing pops, newest first, every
//! entry of the current list whose block the counter has reached the end of,
//! running each popped op's end rule. Blocks don't nest strictly: an
//! else-if's skip runs to the end of its whole chain, past the end of the
//! block it sits in, and the pop cleans up whatever it left behind. That is
//! why the VM steps a counter over the list rather than walking a tree.
//!
//! # A stretch of a tick
//!
//! An embedder that schedules part of a placement's tick natively runs the
//! ops between two positions of the placement's own script, as its tick
//! would reach them ([`Vm::run_span`]): the placement's locals, `my` the
//! placement, its `system.script timer` as it stands (no tick counted).
//! The stretch starts and ends between statements of one list, so every
//! block it opens closes inside it; a `flow end` in it ends it there, as it
//! would the tick. Design decision 2's player loop runs the portal
//! manager's own ops this way, its orbs, award and loot check among them,
//! only which stretches run and when being guacamole's.
//!
//! A stretch run *apart* ([`Vm::run_span_apart`]) stops alone: in play mode
//! what would stop the placement's script stops that stretch for good,
//! logged and kept ([`Vm::span_stopped`]), and the placement's other
//! stretches and ticks run on. The portal manager's tick opens on the
//! gameplay camera's frame (openspec AG.28), which runs so: the camera
//! stopping hands the view to the TV and leaves the player loop running.
//!
//! # A script's frame
//!
//! Every script opens with PRESCRIPT, whose block holds STARTUP, SHUTDOWN,
//! a MACROS section and any STRUCTURES sections; behaviours (named states)
//! follow it.
//!
//! - PRESCRIPT pushes and runs its ops every tick. Its end rule, when the
//!   counter falls through to its block's end exactly: with a shutdown
//!   pending, jump to SHUTDOWN; else, with a current behaviour, jump to it;
//!   else carry on past the block.
//! - STARTUP runs its block on the placement's first tick only, and sets the
//!   current behaviour to the op right after PRESCRIPT's block when that op
//!   is a behaviour (a script with no behaviours has none). Later ticks skip
//!   it.
//! - SHUTDOWN is skipped unless a shutdown is pending; then it pushes, and
//!   its end rule clears the request and the started flag (so a placement
//!   started again runs STARTUP again) and ends the tick.
//! - MACROS, STRUCTURES and a definition at a position skip their blocks:
//!   definitions run only when called.
//! - A behaviour pushes; its end rule ends the tick, or jumps to SHUTDOWN
//!   when one is pending. So a tick is PRESCRIPT, then the current behaviour.
//! - `set behavior` names a behaviour of the placement's own script, matched
//!   by name ignoring case, and makes it current. Set inside PRESCRIPT it
//!   runs this tick (PRESCRIPT's end rule reads it); set anywhere else, the
//!   running behaviour finishes and the new one runs next tick. A name the
//!   script doesn't have changes nothing and is logged.
//!
//! # Checks and flows
//!
//! - A check (`if value`, `if reference`, `if member`) that holds runs into
//!   its block without pushing; one that fails, or whose left side can't be
//!   read (a path through nothing), skips it. Read as a path element, a
//!   check is what its left side read, whether it held or not: a value, a
//!   reference, or `if member`'s set (openspec AS.57, a reading: level 214's
//!   tool spawner checks its bomb's clones through the check before).
//! - An `if value` whose left side starts at `undefined` skips whatever it
//!   compares: the walk reads 0 there but leaves no object to compare it
//!   on, as a path through nothing does (openspec AS.96, a reading of the
//!   check and its walk in code; AH.10's oracle run walked into chapter 1's
//!   cave interior, whose entry check ends the tick when `undefined == 0`
//!   holds, and a deleted variable's checks across the disc read as
//!   switched off, `== 0` and `== 1` alike). On the right side, in an
//!   expression or as a reference it still reads 0 and none.
//! - `if member` asks of its left side's set and its right side (a set, or
//!   one object): code 0 holds when every member of the right side is in
//!   the left, 1 when none is, 2 when any is, 3 when the left holds each at
//!   least as often as the right does; one object is in the set or not (1
//!   not). An empty right side fails every code (codes 0 and 3 since
//!   openspec AS.57, a reading: level 214's tool spawner ends its tick when
//!   such a check holds, and with nothing carried an empty set holding it
//!   would keep its bomb from ever being cloned).
//! - `else` (an `OpAbstractFlow` skip) jumps forward its block length, then
//!   closes. As the last op of a check's block it jumps over the else block;
//!   with length 0 it only closes; in an else-if chain it lands at the
//!   chain's end, past enclosing blocks, and `break` out of a loop is the
//!   same jump.
//! - `flow continue` only closes.
//! - `flow` steps a loop: the counter goes back to the newest entry's start,
//!   that op's loop step runs (which, on the last pass, moves the counter
//!   past the block), then it closes. On a block that isn't a loop it simply
//!   runs the block again.
//! - `flow end` stores 0xFFFF as its length: the jump carries the counter
//!   past every block of the current list and the close pops them all. Inside
//!   a macro body that pops the definition's entry, whose end rule returns to
//!   the caller; at the top of a script it ends the tick.
//!
//! # Loops
//!
//! - `loop value` reads its count once, as an integer; 0 or less skips. Its
//!   index, read as a path element, runs 0 to count - 1.
//! - `for each` snapshots the set it names, rotated left by its offset
//!   (modulo the count, negatives wrapping); direction 0 runs it forward, 1
//!   backward, 2 in a shuffled order. An empty set skips, as `undefined`
//!   does, which holds no set (Members; openspec AS.28: the spike floors'
//!   controller, chapter 1's among them, walks `undefined` in its
//!   STARTUP). Its element, read
//!   as a path element, is the current one.
//! - `find subset` reads a path with one empty slot: the part before the
//!   slot names a set; each member but the running placement goes in the
//!   slot and the rest of the path is compared with the right side. The
//!   matches, sorted ascending by the compared value with ties kept in set
//!   order, become a set the op holds (read as a path element); with none
//!   it skips, else it pushes and runs its block once. `its`, while the
//!   right side is read, is the member under test.
//!
//! # Macros
//!
//! A macro definition owns one *frame*: its variables, by name. Frames are
//! static, not a stack, and a definition that is running is *busy*.
//!
//! - `use macro` reads which definition it names (a path, maybe indexed into
//!   a set of them), following a stub by name. When it names nothing, or the
//!   definition is busy (no recursion, direct or not), the whole use block
//!   is skipped. Otherwise the definition becomes busy and the use op pushes.
//! - The use block opens with the call's INTERFACE block: each `parameter`
//!   reads its right side in the caller's context and writes it into the
//!   callee's frame, matched by name (ignoring case), else by parameter id. A
//!   value is written whole, a set combined in by the parameter's combine
//!   code, a reference read at its index. While a parameter's right side is
//!   read, `its` is that parameter, its variable as the frame holds it, so
//!   a parameter bound to `its` keeps what it holds, whatever a `find
//!   subset` left `its` at (the original's parameter op makes itself `its`
//!   before it reads its right side; AG.171: read as a `find subset`'s
//!   last candidate, Giants' `Movement - Add Mover` lost two heroes'
//!   controllers). Then `flow macro` enters the body.
//! - Entering pushes the definition and skips the definition's own INTERFACE
//!   block: a parameter's default there is never read. A parameter the call
//!   leaves out reads 0, nothing or an empty set.
//! - A parameter op outside any call that binds into no definition (its
//!   `_macroDef` empty) binds nothing and reads nothing: level 212's
//!   definition holds one between two camera calls, an editor's leftover,
//!   and its level plays on past it (openspec AS.34; a reading: the next
//!   call binds the same parameter, so nothing observable rides on it).
//!   One outside a call that names a definition stops as unsupported.
//! - The body's end (its block closing, or `flow end`) returns: the counter
//!   resumes right after the caller's INTERFACE block, in the caller's
//!   context. The rest of the use block is the caller's after-return code:
//!   it still reads the callee's frame (that's how a macro returns values),
//!   through the callee's parameters. A family (a base and its
//!   specializations) shares its parameter ops, so there a parameter naming
//!   the callee's definition, or the base it specializes, reads the
//!   callee's frame even where the caller declares it too: Giants' `Calc -
//!   Tangent` reads the `Calc - Sin` it called that way (AH.60: read as its
//!   own, every lob's speed came out wrong; read so, AH.10's 20 lobs are the
//!   oracle's within 1 %).
//! - When the use block itself closes, every variable in the callee's frame
//!   goes back to its default and the definition is no longer busy.
//! - A specialization is a definition with an INTERFACE and body of its own;
//!   calling it runs it. A parameter that names its base's definition reads
//!   the specialization's frame while the specialization runs. A reading:
//!   the disc shows only that every specialization's parameters name the
//!   base, and a call binds its arguments into the callee's frame, which the
//!   base's never is; no oracle read has settled it yet.
//!
//! # Variables, structures and sets
//!
//! - A variable named at the head of a path lives in the *current context*:
//!   the running macro's frame inside a body (or in a call's after-return
//!   code, the caller's), else the placement's locals. A definition's own
//!   parameter (one its INTERFACE declares) reads that definition's frame
//!   wherever its body runs, binding a nested call's arguments and in a
//!   nested call's after-return code included (but for the family's read
//!   above); a specialization's own parameters name the base and read the
//!   specialization's frame. Any other parameter on a path (an after-return
//!   read of what a call returned) reads the frame of the definition it
//!   binds into (its `_macroDef`, a stub followed), or of the open call's
//!   specialization of that definition.
//! - A variable holds a number (dynamically an integer or a float: a write
//!   takes the type it's given), a reference, a set, a reference to a set or
//!   a structure, as its declaration says. Writing anything but a number or a
//!   packed colour or screen position into a number variable drops the write.
//! - A structure has the fields its definition's block declares, built on
//!   first use and found by name ignoring case. Writing a structure into a
//!   structure variable copies it field by field.
//! - A `set reference` whose right side's edit type names a structure takes
//!   nothing for a structure of another definition (a copy keeps its
//!   original's), as the original's checks the object it read against the
//!   side's edit type (aj-vm's reading of the op, an aid), comparing the two
//!   definitions' names, ignoring case, when they aren't the same one: a
//!   structure of a definition of the same name placed in another file is
//!   taken (a hero's file and a level's copy of his scripts both place
//!   theirs, as do a hero's and his alt's). A cutscene's step finds its own data among its actor's by looping over them until a
//!   typed read takes one (openspec AS.52: read untyped, chapter 1's outro
//!   read its dialogue's data as Norticus' animation's and never started
//!   it). An untyped side, or one naming a class, takes what it read; an
//!   object against a class isn't checked here.
//! - A set is an ordered list. Combine codes: 0 adds if absent (a set's
//!   members in order, or copied whole into an empty set), 1 removes one
//!   occurrence of each, 2 intersects, 3 replaces, 4 appends with duplicates,
//!   5 removes every occurrence. `count` reads the length; written, it
//!   resizes, new slots holding fresh default elements of the set's element
//!   type (a new structure, a new number variable, a new reference, or for
//!   a set of engine objects a fresh one the host makes, [`Host::make`]:
//!   an orb's landing point is a new `ActorWaypoint`), so `inc count`
//!   appends one. `first` and `last` read the ends, nothing when
//!   empty; `random` a member by a draw modulo the count.
//! - A set reference given what isn't a set: a lone object, a file's or the
//!   host's, reads as a set of that one object (`count` 1, a `for each`
//!   walking it once, a set combined with it gaining it), the same set
//!   again while it holds that object alone; anything else (a number, a
//!   structure) drops the write. Read off the disc (openspec AT.44): the
//!   cutscenes hand `Add Dialogue` one string and one sound for its two
//!   set-reference parameters, each right side's edit type the element's
//!   class, which the original's reference read checks the object against
//!   (aj-vm's reading: a mismatch gives nothing), and the music
//!   controller's `Environmental Sound - Define Transition Plane` sets its
//!   trigger set to the one actor it was given, then walks it. Dropped,
//!   every line ran with no words and no voice. A reading: whether the
//!   original makes a set of the object or keeps it for each reader to
//!   take as one isn't read; a set of one is what every reader needs, and
//!   aj-vm's reads agree with it (`random` draws whatever the count, so a
//!   set of one costs a draw: a text box's four corners take four, AT.44's
//!   review; `for each` walks the object its path reads with no check that
//!   it's a set). The same set for the same object is ours: nothing on the
//!   disc changes a set made so (AT.44's review: 411 places over every
//!   level, none written into), so whose it is can't show.
//! - A number variable handed to a set reference by reference (a run-time
//!   one, a file's, a structure's field, a number a member holds) reads as
//!   a set of that variable: a reference read off it at an index is the
//!   variable, so a read through that reference reads its number and a
//!   write through it lands in the variable (openspec AT.47). Read off the
//!   disc: the bit-field macros (`Save - Bit Field - Get Bit Field` and
//!   `Set Bit Field`, `UI - Get/Set Sprite Bits`) take `values in` by set
//!   reference and read or write `values in[set index]` through a
//!   reference, and their callers hand one number variable, the right
//!   side's edit type `AbstractNumberMeasurement`, the shape AT.44 read for
//!   objects. Dropped, every Get read 0 and every Set wrote nothing. The
//!   same set again while it holds that variable alone, as an object's; a
//!   member's number is a fresh set each time, as its reference read is a
//!   fresh place. A literal number still drops. A reading, as AT.44's:
//!   whether the original makes a set of the variable or each reader takes
//!   it as one isn't read, and the same set again is ours. An index past 0
//!   wraps modulo the count, as every set's reference read at an index does
//!   here, so it reads the one variable again, where the original's would
//!   reach past the set (unread): the bit-field macros step `set index` on
//!   by `bit index` / 32, so a `bit index` past 31 reads past the first.
//!   None of the disc's callers does so on one number (AT.47's review: no
//!   index past 0 on such a set over every level's `giants_levels` run and
//!   chapter 1's route).
//! - `global.bld`'s numbers, screen positions, colours, sets, structures and
//!   references are cells made from the file on first use and shared by
//!   every placement.
//! - `find variable` finds a variable by name and declaration (container and
//!   contents type, a structure named through a stub being the structure) on
//!   what its owner path names. On a placement the VM runs, that's its
//!   locals: a declared one is made as it's first reached, as the original
//!   builds every declared local with the list. Filter 0 finds (the block
//!   runs only with one found), asking the owner for its list made if it has
//!   none yet, so a placement whose STARTUP hasn't run yet has its declared
//!   locals already (a reading of the original's search, which asks for the
//!   list with that flag set for filter 0 alone: the portal manager's
//!   STARTUP finds the portal control's `portal data` so, ticking before
//!   it). Filter 1 finds in a list the owner already has: its tags from
//!   the start, its declared locals once its first tick (its STARTUP) is
//!   done or a find made the list (the same reading, and AH.18's: a Chompy's
//!   own STARTUP finds `communicate range` with filter 1 and gets its
//!   script's default, 10, as the live game holds it, where its script
//!   declares the local; the original's other branch for filter 1 is for a
//!   name holding a pattern, which no filter-1 name on the disc does). 2
//!   finds or makes, 3
//!   finds and takes it off; the variable is the op's result as a path
//!   element. On anything else it's a table per owner. Filter 4 and the
//!   skip operand stop as unsupported: what they select is a reading not
//!   yet settled.
//! - A placement's *tags* (its `_tagList`'s variables, each with a value
//!   its file gives: a Chompy's `notice player radius` 30) are among its
//!   variables from the start: `find variable` finds one by name and
//!   declaration's container, holding the tag's value ([`Program::tags`]).
//!   A value of literals alone (a roll, an expression of literals) reads as
//!   it does on a right side, its roll drawn from the session's generator
//!   when the tag is first found (openspec AS.33: challenge 49's clocks'
//!   `time` is `random(30)`, 205's elemental wall's `dirt tint` a random
//!   colour, the disc's only two; a reading: whether the original builds a
//!   tag's variable, roll and all, at the placement's load or at its first
//!   find isn't read, and a draw at another moment moves every roll after
//!   it, AS.41). A
//!   value reading a path that names no single object, file variable or
//!   nothing stops as unsupported.
//!   A placement with no script has them too, found from any script as
//!   its table's first entries (AH.21: the loot system's init reads each
//!   breakable's `archetype ref`, and a breakable runs no script).
//! - A reference names a set, a structure, a number variable, a part of a
//!   packed value or a value cell's number (`setref r = s.first.screen
//!   measurement.x`), or a host member's number (`setref r = my.actorinfo.
//!   movement.actorphysics.ground turn rate`, as the hero's movement data
//!   keeps his): written through `valuedereferencer`, that part or member
//!   changes; read, a host member's is asked anew. Two references to one
//!   host member compare equal. `undefined` reads 0 as a number and as an
//!   object none.
//! - An object named by namespace and name (EXNM) holds nothing to write
//!   into: a write through it drops, as one through a path through nothing
//!   does. An alternate's file names its base's file variables that way,
//!   renamed (`105_FireFeet_Alt`'s `105_FireFeet.ai` #137 sets
//!   `*^105 fire wave par to use`, which its base declares as `105 fire wave
//!   par to use`), so the two load side by side; no file defines the
//!   renamed one, so the original's load resolves it to nothing and the
//!   write lands nowhere (openspec AG.160, a reading of the link; no oracle
//!   run plays the alternate). Read, it is its name.
//! - A placement reference keeps a point of its own
//!   (`placementreference.remembered`): each reference its own, two naming
//!   one actor keeping two, so the host is told the reference's cell
//!   ([`Access::via`]) beside the actor it names (openspec AG.28: the
//!   camera's look-from and look-at points name one follower clone and sit
//!   55 ft apart, and kept on the actor the view looked nowhere). One
//!   naming nothing still has it, its receiver the reference's cell. A
//!   reading: the orbs' delayed spawn remembers where an orb comes out
//!   through its `orb` reference before the orb exists, then moves the
//!   spawn point there (AH.20). A script setting a placement reference to
//!   an actor tells the host ([`Host::referenced`]), the actor it names
//!   already too: the reference's point is where that actor stands then,
//!   until a script writes it (a reading, openspec AH.250: the hero's death
//!   actor lifts him that many feet over its `dying actor`'s point, set as
//!   it spawns, and the stat-buff icon's measurement actor rises over its
//!   `pos actor`'s; other discs' scripts name such references
//!   `$MyStartPosition` and `$StartPos`). A set to the same actor takes his
//!   place again, as the scripts use it: the Titan boulder sets `actor
//!   carrying me` to itself before lifting its point to the carry height,
//!   the chest sets `chest reference` to itself again once it has risen and
//!   then shakes about that point, `Calc - Get Center Point` sets one to
//!   the same actor twice running. A copied structure's references carry
//!   none.
//! - Every member the host answers comes with the op that reads or writes
//!   it ([`Access::op`]), so the host can answer one asker apart from
//!   another (openspec AH.150: a point against the gameplay camera, on
//!   screen for every point but the asks it reads off the camera).
//!
//! # Values and arithmetic
//!
//! - Integers are 32-bit and wrap. An integer op an integer stays an
//!   integer; anything with a float converts both sides to single precision.
//!   Nothing is ever computed in double precision, bar the perturbation the
//!   exact layer's tests run ([`Precision::Double`]): there a fraction is
//!   carried in double precision, in variables too, and rounded only where a
//!   member takes it.
//! - A screen position's halves are 12.4 fixed point: `+` and `-` work on
//!   each half, `*` multiplies each and shifts the four fraction bits back,
//!   every half held to 16 bits signed. A colour's `+` and `-` work on
//!   each byte, held to 0-255 (AH.21: the particle tinter's white less a
//!   pot's tint). Screen division and a colour's `*` and `/` stop as
//!   unsupported until a script reaches them.
//! - An expression is `a <op> b` (+ - * /) or `a` alone, read left side
//!   first, nested as the data nests it: the order written is the order
//!   computed, which single precision makes observable.
//! - A division by 0 fails the expression, as a path through nothing does:
//!   the write it feeds drops and the check it feeds skips. That's an
//!   integer 0, a float 0 of either sign, or nothing (which counts as the
//!   integer 0 in arithmetic). The original's evaluator tests the divisor
//!   against 0.0 before it divides (aj-vm's 0x7a028, read as an aid), and
//!   AG.11's recordings show it: at rest the locomotion sets its idle's rate
//!   from his walk over the idle's speed of 0, and the idle plays at 100 %,
//!   the rate the failed write leaves (AH.49). `i32::MIN / -1` gives 0, as
//!   the PPU's `divw` does; any other float division follows IEEE.
//! - A path read for its value through a reference to a number reads the
//!   number.
//!
//! # Comparisons
//!
//! Codes 0 `<=`, 1 `==`, 2 `>=`, 3 `<`, 4 `>`, 5 `!=` (negations pair 0 and
//! 4, 1 and 5, 2 and 3), read in code by aj-vm and measured by
//! ah-progression; AH.11's level-up at exactly 1,000 confirms it or breaks
//! it. Two integers compare exactly. Otherwise both go to floats, d = left -
//! right, and with e = 1e-4: `==` is |d| <= e, `!=` |d| > e, `<` d < -e, `<=`
//! d <= e, `>` d > e, `>=` d >= -e. Anything else (references, packed
//! colours) supports only `==` and `!=` (the same thing, or not); any other
//! code fails the check. Not modelled yet: a number against nothing or a
//! reference, where only `!=` holds here; the original's routine is read to
//! compare two different kinds as numbers, the other side's word as one, so
//! a number against nothing would compare with 0 there. No oracle read has
//! shown whether the disc's scripts ever compare the two.
//!
//! # Literals and rolls
//!
//! A literal's class says how its bits read: a float, an integer, or a
//! packed colour or screen position. A colour the file holds (a literal, a
//! `global.bld` colour) keeps alpha in its high byte and red in its low, so
//! it reads byte-swapped into the VM's order, red high (AH.67: the
//! eruption's dark colour `0xFF141422` is the oracle's (34, 20, 20, 255);
//! of the 1,102 distinct nonzero colour literals in the 01.00 disc's
//! archives the high byte is 0 or 255 in 981, as an alpha mostly is, the low
//! byte in 155). A random literal draws a 32-bit number
//! from the session's generator ([`Generator`], through [`Host::draw`]): one
//! MT19937 per session, seeded the original's way from the clock, as the
//! original's every roll draws from its one MT. An integer literal n reads
//! (number & 0x7fffffff) mod (n + 1) for n >= 0 and mod (n - 1) for n < 0,
//! so `random(3)` is 0 to 3 with both ends. A float literal r reads r x
//! number x 2^-32 with the number rounded to single precision first, as the
//! original's routine is read to do: [0, r), except that the 128 highest
//! numbers round to 2^32 and read r itself, one draw in 2^25. That corner
//! is the reading, kept for parity as script-runtime's Random rolls says;
//! no measurement sees it. A colour literal rolls each byte on its own,
//! four draws: byte b reads (number & 0x7fffffff) mod (b + 1), so 0 to b
//! with both ends, the file's word from its high byte down, alpha first,
//! then blue, green and red (openspec AS.34: 003's definition rolls a
//! zone's fog colour, the dirt mounds their tint; aj-vm's read of the
//! routine, an aid, no measurement yet). A screen position's roll stops as
//! unsupported until a script reaches one.
//!
//! # Members
//!
//! An engine member is a path step imported by name (FNV-1a of the
//! lowercased member name and of the lowercased class namespace). A few are
//! the VM's own:
//!
//! - `optoplevelbehavior.my`, the running placement; `abstractscriptgroup.null`
//!   (nothing) and `.undefined` (reads 0, writes go nowhere, a set change
//!   into it and a write to its count too, and a value check through it, a
//!   membership check, a `for each` or a `find subset` of it skips;
//!   its `setvariant` members read as an empty set's: a count of 0, no
//!   element; openspec AS.34: challenge 49's trees raise its count, then
//!   read its last);
//!   `.its`, the
//!   member under test inside `find subset`; `.valuedereferencer`, the
//!   object a reference names, as the place to write;
//! - `setvariant.count`, `.first`, `.last` and `.random` on a set, and
//!   `.next`, its elements in turn from the first, round again after the
//!   last, a cursor per set (a reading: the original's sound sets read it
//!   so; whether its order is shuffled isn't read);
//! - `optoplevelbehavior.behavior`, the running placement's behaviour as the
//!   object `set behavior` names (`if reference behavior == <Idle>`);
//! - `system.cast to integer`, an integer cell: written, it truncates toward
//!   0 (no rounding); read, it gives what was written;
//! - `system.script timer`, the running placement's seconds of ticks (a
//!   tick the embedder runs natively for one counts too, [`Vm::count_tick`]),
//!   and `system.frame rate scale`, the tick's seconds times 60;
//! - a packed value's parts, wherever the value lives (a cell, a value cell
//!   or a host member, read and written back whole):
//!   `screenmeasurement.x` and `.y`, the high and low halves in sixteenths,
//!   a write truncated to them; `colormeasurement.red`, `.green`, `.blue`
//!   and `.alpha`, the bytes from the high one down, a write clamped to
//!   0-255.
//!
//! Every other member is in a [`Members`] table or unmodelled. Each entry has
//! the member's registered type, minimum and maximum, and lives either in
//! the host (its get, set and reset go to [`Host`]) or as a *value cell*,
//! which reads the registered default and keeps what's written, per object.
//! Every write is converted and clamped first: an integer or enum member
//! truncates a float toward 0 and clamps to [min, max], but only when min or
//! max is nonzero; a float member converts an integer and clamps the same
//! way; a packed member takes only packed values and a reference member only
//! references. A value that can't convert drops the write. An engine set
//! member (an actor's waypoints) is read whole through [`Host::elements`],
//! anew each read, and never written. A *set reference the host copies*
//! (openspec AT.9: the string converter's `value set`, the text colour
//! converter's `color set`) is a value cell holding the reference a script
//! writes, as a reference member's does, so the script reads back its own
//! set; each write also hands the host that set's elements as they read
//! then (none for a null or anything not a set), and each read first asks
//! the host whether it changed its copy since: a changed one reads from
//! then as a fresh set of `ValueInfo`, each number in a cell of its own, so
//! a script reads its elements through references, writes them and grows
//! it as it would its own; a reset puts the cell back and resets the
//! host's copy too. A path through nothing (a null
//! receiver, a member the host answers with nothing) fails quietly as the
//! original's does: a write drops, a check skips. [`members`] lists the
//! members as the original registers them and who keeps each here.
//!
//! # Slides
//!
//! `slide value` eases its place from what it holds to its target over its
//! seconds, by a *slider* of a pool of [`SLIDERS`] (aj-vm's 400). The
//! slider is the op's result, read in its block (`setref ... = <slide>`),
//! and its block runs. Once a frame before the scripts each running slider
//! moves a tick's seconds times its playback rate over 100, and writes its
//! place `from + (to - from) * e(x)`, `x` the share of its time gone and
//! `e` the ease ([`eased`]): the rate of change rising linearly over the
//! first `out × 0.02` of the time, falling over the last `in × 0.02`, both
//! scaled down to fit when they'd overlap. A slide begun this frame first
//! moves in the next; at its time's end it lands on its target and ends.
//! Numbers slide in single precision, a packed colour channel by channel, a
//! screen position half by half.
//!
//! - With every slider running, a time that isn't over 0, or values that
//!   don't slide, it lands on its target at once and skips its block, as
//!   the original does with its pool full.
//! - A script's own write to a place a slide is easing ends the slide, and
//!   a new slide on the place takes over from the old (a reading of AH.10's
//!   pushes: a hero's hurt reaction puts his deceleration back to 80 ft/s²
//!   while a push's slide eases it in from nothing, and 80 holds; on a
//!   Chompy, whose push slides the engine's deceleration and whose reaction
//!   puts back the script's own, the slide runs on).
//! - A slider's `sequence.time remaining` and `.percent remaining` read
//!   what's left of this pass, and written set it (0 lands it next frame);
//!   `.playback rate` (percent) speeds it; `.raw duration` reads its time;
//!   `slider.ignores time scales` is kept, no time scale being modelled for
//!   it to ignore. `stop` on a slider ends it where its place stands. Any
//!   other member of a slider is unmodelled.
//! - `.playback mode` (0 one way, 1 ping pong) and `.looping` (0 or 1): one
//!   way, a slide ends on its target, or looping starts again from where it
//!   began; ping pong, at its target it comes back retracing its way out,
//!   and back where it began it ends there, or looping goes out again. A
//!   looping slide never ends on its own. A reading (openspec AH.250): the
//!   hero's death actor slides his lift 0 to 12 ft over 0.35 s ping pong
//!   without looping, and AH.11's `restart1` and `restart2` lift him 3.44 m
//!   and set him back down within 2 cm 0.67 s on, his burst 0.7 s in;
//!   Eruptor's splash mesh widens 25 to 110 over 0.3 s while it rises 5 to
//!   300 over 0.15 s ping pong, ending flat as it ends wide.
//!
//! # Modes, the budget and the whitelist
//!
//! - *Strict* (tests): reaching an unmodelled member, an op the host leaves
//!   unmodelled, a shape the VM doesn't model or the budget stops the run and
//!   returns a [`Halt`] naming the placement, the script, the position and
//!   the member or op. *Play*: the same stops only that placement's script
//!   for good, with a log line naming it and the placement, and every other
//!   placement runs on. Never a panic. An embedder's call in a placement's
//!   name for work of its own ([`Vm::call_for`]: the join's, on the portal
//!   manager's behalf) stops nothing in either mode: the stop is returned,
//!   the placement runs on.
//! - The budget: no placement may execute more than [`Config::budget`] ops in
//!   one tick (a call counts the body's ops). It is set above the largest tick
//!   the disc tests measure ([`DEFAULT_BUDGET`]), so only a script that loops
//!   inside a tick reaches it.
//! - The whitelist ([`Whitelist`]): only placements whose name or script it
//!   lists run; any other placement added stays inert, never ticked, and is
//!   listed ([`Vm::inert`]).
//! - The stall detector ([`StallDetector`]), for the headless runs: a
//!   placement whose behaviour the oracle's trace shows advancing must leave
//!   it within that behaviour's longest recorded stay.
//!
//! # Left to the host, and not modelled yet
//!
//! The world ops (spawning a placement, tell, remove, a placement's reset,
//! displace, teleport, turn and move steering, sequences, field-of-view
//! checks) go to [`Host::op`] with their operands read; the host answers
//! whether the op's block runs. `remove` and `stop sequence` go once per
//! object a set names, and a removal once per placement: one already off
//! isn't removed again. A path through nothing answers for it, as in the
//! original: a branch op skips, any other does nothing. A `start sequence`
//! the host answers as starting nothing (no template, or a play it refuses)
//! reads as nothing on a path after, not as the sequence it last started:
//! the original writes what each start handed back, nothing included, into
//! the op (openspec AH.184).
//!
//! What the VM does itself: a spawn of a structure or a set copies it (its
//! structures and sets copied too) and runs the block with the copy as the
//! receiver, the copy the op's result; a reset of a structure, or of what a
//! reference names, puts it back to its defaults, the reference kept (a
//! reading: `EnemySystem_` resets its action references, then nulls them);
//! a reset of a set, or of a set reference, resets each thing it holds once
//! as a reset of that thing would (a number variable to 0, a structure's
//! fields, a placement by the host), the set and the reference kept, where a
//! structure's own reset still empties a set among its fields (openspec
//! AS.77, a reading off the disc's own uses: Combat gathers six of a
//! creature's per-state flags into `reset on new state` once and resets the
//! set at each new state; `PuzzleResetter.ai` and level 046's dragon-fire
//! controllers fill a set with a puzzle's parts, reset it and go; a
//! cutscene's SHUTDOWN resets the set of placements its STARTUP took off;
//! emptied, each of those would do nothing); and slides, with their
//! sliders (Slides).
//!
//! A static member (one heading its path) is read under what the block
//! runs on: the running placement, or inside a tell's or a clone's block the
//! object told. The VM tells the host before each such host read
//! ([`Host::told`]); the host decides what that member makes of it
//! (openspec AS.52: a cutscene's game systems' placement reads each told
//! actor's animation palette).
//!
//! Not modelled yet: a colour's multiplication and division and screen
//! division (they stop as unsupported), the set properties (unmodelled members); and, with no
//! stop, a reference's type check of an object against a class edit type
//! (a structure's is, above), a `check reference`'s or a parameter's,
//! `for each`'s shuffled order (any order that visits each member once)
//! and the scheduler's placement groups past its awake and asleep lists
//! (activity by distance is the embedder's, a pass at a time through
//! [`Vm::set_activity`]), so SHUTDOWN's
//! restart isn't either: the original runs STARTUP again at once when an
//! active placement's SHUTDOWN block falls through, where the VM ends the
//! tick and starts it on the next.

#![forbid(unsafe_code)]

mod generator;
mod host;
mod ids;
pub mod members;
mod program;
mod slide;
mod stall;
mod value;
mod vm;
mod whitelist;

#[cfg(test)]
mod tests;

pub use generator::{Generator, StateError, session_seed};
pub use host::{
    Access, Answer, Builtin, Host, HostFault, Kind, MemberId, MemberSpec, Members, OpCall, Place,
};
pub use program::{
    Arith, Container, Contents, Decl, Def, DefId, DefKind, Elem, Global, GlobalId, Ins, LinkError,
    Literal, LiteralClass, Obj, OpId, OpInfo, Operand, OperandId, Param, Program, Role, ScriptId,
    ScriptInfo, Sym, Tag, Unit,
};
pub use slide::{between, eased};
pub use stall::{Stall, StallDetector, StallLimit};
pub use value::{
    ArithFault, Value, arith, arith_in, compare, convert, random_colour, random_literal,
};
pub use vm::{
    Config, DEFAULT_BUDGET, Halt, Handle, LogLine, Mode, Native, PlacementId, Precision, Reason,
    SLIDERS, StepReport, TICK_SECONDS, Vm,
};
pub use whitelist::{Listed, Whitelist};
