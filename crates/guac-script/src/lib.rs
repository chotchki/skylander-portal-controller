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
//! of a placement doesn't send it round yet: the host resets its buffer
//! alone (openspec AH.80).
//!
//! The embedder puts placements to sleep and wakes them by its activity
//! rule ([`Vm::set_asleep`]); one asleep isn't ticked and keeps its state,
//! and waking puts it at its group's end: placements tick in the order they
//! woke. AH.10's encounter end shows it too (Norticus, woken last, takes his
//! scene the frame the last Chompy goes, and `Setup_001_Controller`, woken
//! first, sees it a frame later). A placement a tick spawns with a script
//! of its own ([`Host::adopt`]) joins the end and ticks that same tick:
//! AH.54's recordings list the eruption's first wave of lava at the
//! eruption's own first record, which its first tick spawns. A held
//! placement (below) runs what the embedder does in its place natively
//! ([`Vm::step_with`]), where the walk reaches it: design decision 2's
//! player loop runs the portal manager's and the game systems' ticks so.
//!
//! `remove` takes a placement off: the rest of the tick it's in runs (a
//! script taking itself off finishes its tick), then it's never ticked again
//! and its started flag is cleared (AH.4's census shows level 213's
//! definition so: removed, not started, one tick's seconds); a tell of it
//! skips its block. Its SHUTDOWN doesn't run: that's the level's teardown,
//! which no box models yet (a reading: no oracle read has seen a removed
//! placement's SHUTDOWN run). A whitelist listing a script for its first
//! tick alone ticks its placements until their STARTUP has run, then holds
//! them, listed ([`Vm::held`]).
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
//!   read (a path through nothing), skips it.
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
//!   backward, 2 in a shuffled order. An empty set skips. Its element, read
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
//!   code, a reference read at its index. Then `flow macro` enters the body.
//! - Entering pushes the definition and skips the definition's own INTERFACE
//!   block: a parameter's default there is never read. A parameter the call
//!   leaves out reads 0, nothing or an empty set.
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
//! - A placement reference keeps a point of its own
//!   (`placementreference.remembered`), which the host keeps on the actor
//!   it names; one naming nothing still has it, the host keeping it on the
//!   reference itself (its receiver is the reference's cell). A reading:
//!   the orbs' delayed spawn remembers where an orb comes out through its
//!   `orb` reference before the orb exists, then moves the spawn point
//!   there (AH.20).
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
//! - Integer division by 0 (and `i32::MIN / -1`) gives 0, as the oracle's
//!   PPU does; float division follows IEEE. Nothing counts as the integer 0
//!   in arithmetic.
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
//! no measurement sees it.
//!
//! # Members
//!
//! An engine member is a path step imported by name (FNV-1a of the
//! lowercased member name and of the lowercased class namespace). A few are
//! the VM's own:
//!
//! - `optoplevelbehavior.my`, the running placement; `abstractscriptgroup.null`
//!   (nothing) and `.undefined` (reads 0, writes go nowhere, a set change
//!   into it too, and a membership check or a `find subset` of it skips);
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
//! anew each read, and never written. A path through nothing (a null
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
//!   `.playback rate` (percent) speeds it; `.looping` and `.playback mode`
//!   (one way, ping pong) make it start again at its end, or come back the
//!   way it went, and never end on its own; `.raw duration` reads its
//!   time; `slider.ignores time scales` is kept, no time scale being
//!   modelled for it to ignore. `stop` on a slider ends it where its place
//!   stands. Any other member of a slider is unmodelled.
//!
//! # Modes, the budget and the whitelist
//!
//! - *Strict* (tests): reaching an unmodelled member, an op the host leaves
//!   unmodelled, a shape the VM doesn't model or the budget stops the run and
//!   returns a [`Halt`] naming the placement, the script, the position and
//!   the member or op. *Play*: the same stops only that placement's script
//!   for good, with a log line naming it and the placement, and every other
//!   placement runs on. Never a panic.
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
//! original: a branch op skips, any other does nothing.
//!
//! What the VM does itself: a spawn of a structure or a set copies it (its
//! structures and sets copied too) and runs the block with the copy as the
//! receiver, the copy the op's result; a reset of a structure, a set or what
//! a reference names puts it back to its defaults, the reference kept (a
//! reading: `EnemySystem_` resets its action references, then nulls them);
//! and slides, with their sliders (Slides).
//!
//! Not modelled yet: a colour's multiplication and division and screen
//! division (they stop as unsupported), the set properties (unmodelled members); and, with no
//! stop, a reference's type check against the stack's edit type,
//! `for each`'s shuffled order (any order that visits each member once)
//! and the scheduler's placement groups (activity by distance is the
//! embedder's, through [`Vm::set_asleep`]), so SHUTDOWN's
//! restart isn't either: the original runs STARTUP again at once when an
//! active placement's SHUTDOWN block falls through, where the VM ends the
//! tick and starts it on the next.

#![forbid(unsafe_code)]

mod generator;
mod host;
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
pub use value::{Value, arith, arith_in, compare, convert, random_literal};
pub use vm::{
    Config, DEFAULT_BUDGET, Halt, Handle, LogLine, Mode, Native, PlacementId, Precision, Reason,
    SLIDERS, StepReport, TICK_SECONDS, Vm,
};
pub use whitelist::{Listed, Whitelist};
