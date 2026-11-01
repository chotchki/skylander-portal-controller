//! The original's interface (openspec AT.4, design decisions 2 and 3): the
//! sprites the original's scripts clone, hang in a tree and set member by
//! member, and what the frame's sprite updater makes of them. Pure: no Bevy,
//! no I/O. guacamole's host forwards the interface namespaces here and draws
//! the draw list; headless tests do the same with no app.
//!
//! This is the semantics, in our own words, written before the code. It is
//! read off Giants' EBOOT (BLUS30968 01.00) by aj-vm's method (Ghidra over the
//! decrypted EBOOT, headless and read-only, the decompiler's text kept in
//! `~/workspace/guacamole-research/at-lane/`), each member at the handlers
//! aj-vm's live member table names, each with its address (`0x…`); AT.1's
//! bitfields and AT.3's reading of AC.13's captures fill in the rest, and
//! AC.4's registry names the classes the handlers test against. A rule marked
//! **Hypothesis** is inferred, not read; the box that settles it is named.
//! Nothing here is checked against the oracle yet: AT.10's sprite reader is.
//! [`members::MEMBERS`] lists the 64 members with their handlers.
//!
//! # The sprite
//!
//! A sprite is a `tfbSpriteInfo` placement: drawn, never ticked (it carries no
//! script; the frame's sprite updater draws it). A clone copies its template's
//! fields whole (`OpSpawn`, the VM's), and from then the scripts move it by its
//! members. What a sprite holds, by what the handlers read and write:
//!
//! - **Place**: `location`, two signed 12.4 fixed-point halves, x then y, in
//!   interface units (16 to a unit), relative to its parent (Layout below).
//! - **Properties**, one word of AT.1's bitfields: visual type (bits 0-2),
//!   blend mode (3-5), justification (6-7), alignment (8-11), align to parent
//!   (12-15), visible (16), point size (17-28, twelve bits), *dirty* (29: its
//!   kind builds its geometry again) and *string width valid* (30: the text's
//!   measure stands).
//! - **Flags**, the template's other word (`flags` in the IR): bit 0 *sized*
//!   (a script wrote its width, height or depth, so its kind's own size no
//!   longer answers); bit 7 *wraps* and bit 8 *fits* (the text's layout
//!   wraps on the first and fits on the second, 0xB6F70, read for AT.6; no
//!   member writes either, so they come from the template: `global.bld`'s
//!   `Generic_TextBox` templates hold both, its `Generic_Text` ones
//!   neither); bit 9 `Take a Picture`;
//!   bits 10-13 its text's string flags, copied from the string on every text
//!   layout (bit 10 *a time*, bit 11 *has an `@`*, bit 13 *commas*; bit 12 is
//!   read by the number fill, 0x9C858, and set by nothing read); bit 17
//!   *point size set* (a positive point size written); bits 18-19 `texture
//!   flip`; bit 25 `clipping`. Bits 21, 24, 26 and 27-29 are the updater's
//!   and the art's (**Hypothesis**: what they do isn't read, AT.7).
//! - **Depths**: `Z Depth` and `priority`, floats the registry ranges at
//!   ±10,000.
//! - **Size**: `width`, `height` and `depth`, floats in interface units.
//! - **Scale**: three floats in percent (100 is 1), x, y, z; read-only to the
//!   scripts (no setter is registered).
//! - **Colour**: four bytes, red, green, blue, alpha, 0x80 a colour channel at
//!   full (AT.3: the art draws as stored at 0x80, alpha isn't doubled).
//! - **Replace colour**: three bytes, red, green, blue, and a fourth byte that
//!   is the replace colour percentage on a 0-255 scale.
//! - **Context**: what it draws (an image's texture data, a font, a model, an
//!   emitter; none for a container), as AT.2 reads it.
//! - **String**: the `StringInfo` (or `NameExtractor`, a `StringInfo` holding
//!   an object to show the name of) it shows, or none.
//! - **Numeric value**: one float, the number its text's `@` shows.
//! - **The bloom intensity**, seven bits of the placement's own flags word
//!   (bits 1-7, beside bit 0, *attached*: Tree below).
//! - **Its kind's state** (the original keeps a per-sprite transform object
//!   made at run time, with the kind's renderer behind it): the computed size
//!   and natural size, the place on screen, the computed scale, priority and
//!   depth, the colour with its parent's alpha in it. Here that is the
//!   layout's output (Layout below), never a member.
//!
//! The kind follows the visual type: 0 art (an image, or a movie on an image),
//! 1 text, 2 model, 3 container, 4 particles. Containers draw nothing; their
//! children draw. Art and text are drawn whole by the draw list; model,
//! emitter and movie sprites are entries the adapter draws (design decision
//! 6).
//!
//! # Members
//!
//! The scripts reach a sprite through 64 registered members in eight
//! namespaces. Each is listed by its registered class and its get, set and
//! reset handlers; a member with no setter is read-only (a write is the
//! registry's refusal, as `guac-script` handles one), and every write is
//! converted and clamped by the VM first: integers and enums truncated, and
//! clamped to the registered range only where either end of it is nonzero
//! (aj-vm's rule, `guac-script`'s crate docs). A reference member's get goes
//! through the shared reference getter (0x5E524), which calls the member's
//! own and remembers the object read for the path's next step.
//!
//! ## `spriteinfo` (34)
//!
//! - `type` (enum `art|text|model|container|particles`, 0-4; get 0xB32DC): the
//!   visual type.
//! - `visible` (enum `off|on|loading`, 0-2; get 0xB3BCC, set 0xB3C7C): reads 1
//!   with the visible bit set, else 0; a visible art sprite whose texture
//!   data reports itself still loading (0xF82F8) reads 2. A write turns it on
//!   for any nonzero value, off for 0; a change sets *dirty*.
//! - `color` (colour; get 0xB32F4, set 0xB3308): the four colour bytes; a write
//!   sets all four and nothing else (no *dirty*: the layout reads the colour
//!   every frame a sprite is drawn).
//! - `scale` (scale; get 0xB3430): the scale vector, read-only.
//! - `location` (screen; get 0xB3444, set 0xB3458): the two 12.4 halves; a
//!   write sets both.
//! - `screen location` (screen; get 0xB3480): read-only: the sprite's own
//!   alignment point on screen in 12.4: its alignment's column times half its
//!   width and row times half its height, at its computed scale, carried by
//!   its place on screen.
//! - `screen width`, `screen height` (float; get 0xB36C4, 0xB3734): its width
//!   (height), as `width` reads it, times its own scale's x (y) over 100 and
//!   every ancestor's in turn (0xBA018): what it measures on screen in
//!   interface units.
//! - `text` (reference; get 0xB36B0, set 0xB07F0): the string. A write of a
//!   `NameExtractor` keeps the object the extractor holds instead (so the
//!   sprite shows that object's name); a change clears flags bits 10-13, sets
//!   *dirty* and clears *string width valid*.
//! - `priority` (float, ±10,000; get 0xB3690, set 0xB36A4): stored as written.
//! - `width`, `height`, `depth` (float; get 0xB37A0, 0xB3854, 0xB390C; set
//!   0xB3814, 0xB38CC, 0xB3984): a sprite *sized* reads what was written; one
//!   not reads what its kind computes (0xB9ECC: art, its image's size; text,
//!   its string on one line at its point size; a container, its stored
//!   numbers). A write stores the value and sets *sized* and *dirty*, unless
//!   it is the value already stored on a sprite already *sized*.
//! - `natural size` (screen; get 0xB39C8): read-only, in 12.4: what its kind
//!   measures, written size or not (0xBA16C): art, what its size reads
//!   unwritten (below); text, its whole string at its point size, wrapped
//!   when it wraps (Text below, 0xB81E8); a container, its width and
//!   height. Before its first layout an art sprite whose image is bound
//!   reads the data info's desired size, any other sprite its stored width
//!   and height. The value is truncated to 1/16 of a unit, and the HUD's
//!   macros write it back as the width (and height) of the text they made,
//!   so a stretched text draws a hair narrower than its string: AC.13's
//!   `Level`, `1` and gold `0` at 0.9993, 0.9997 and 0.9954 of their point
//!   size's width (AT.6).
//!
//!   Art's own size (0xB6950, read in the disassembly for AT.6): its image's
//!   size in pixels when the image is in (its data info's `_data` in
//!   memory, or a streamed source resident); else its data info's desired
//!   size × 0.5 (the constant at 0x724360) when the texture attribute is
//!   bound; else its stored size × 0.5. The IR keeps no texture binding, so
//!   the model takes the extract's image size when it holds the image, else
//!   a desired size with both halves positive (a movie's are -1) as bound
//!   (**Hypothesis**, AT.9's census, which names any streamed image that
//!   isn't), else the stored size, each halved where the original halves.
//! - `origin` (matrix measurement; get 0xB3B34): the sprite's
//!   `spritematrixmeasurement` (below).
//! - `blend mode` (enum `opaque|crossfade|additive|cuts out|gets cut out|clears
//!   cut out|gets cut out (add)|cuts out (invert)`, 0-7; get 0xB3B48, set
//!   0xB3B60): bits 3-5; no *dirty*.
//! - `alignment`, `align to parent` (enum, the nine anchors from top-left
//!   row by row, 0-8; get 0xB3B74, 0xB3BA0; set 0xB3B8C, 0xB3BB8): bits 8-11
//!   and 12-15; no *dirty*. An anchor's column is its value mod 3 (left,
//!   middle, right), its row its value div 3 (top, centre, bottom).
//! - `texture flip` (enum `none|horizontal|vertical|both`, 0-3; get 0xB3CD0,
//!   set 0xB3CEC): flags bits 18-19; a change sets *dirty*.
//! - `clipping` (enum `off|enabled`, 0-1; get 0xB3D30, set 0xB3D48): flags bit
//!   25. A write that differs from what reads ORs its low bit into bit 25 and
//!   clears bit 0 (*sized*), not bit 25 (read in the disassembly at 0xB3D60):
//!   once on, clipping stays on, every write of 0 after that drops the
//!   sprite's written size, and each such write sets *dirty*. Kept as the
//!   original does it.
//! - `Take a Picture` (enum `false|true`, 0-1; get 0xB3D80, set 0xB3DA4): flags
//!   bit 9; a change sets *dirty*. An art sprite's layout clears it (0xB64BC),
//!   so it asks once (**Hypothesis**: a capture of the screen into the
//!   sprite; nothing here takes one).
//! - `numeric value` (sprite value; get 0xB3E40, set 0xB3FB8): the float. It
//!   reads as a float when the sprite's text is a string with decimal digits
//!   (one or more), else as an integer; where it reads as an integer a float
//!   written is truncated toward zero first (an integer written where it
//!   reads as a float is whole already). A change on a sprite with text sets
//!   *dirty* and clears *string width valid*.
//! - `justify` (enum `left|center|right`, 0-2; get 0xB3DE4, set 0xB3DFC): bits
//!   6-7; a change sets *dirty* only on a sprite with text.
//! - `point size` (int, 0 to 2^31-1; get 0xB4750, set 0xB4768): bits 17-28; a
//!   write keeps its low twelve bits, so 4,096 reads 0. A change sets *dirty*,
//!   and sets *point size set* when the written value is positive, clears it
//!   otherwise.
//! - `playing animations` (set; get 0x6AA68, reset 0x7BB04): the animations
//!   playing on it; empty for every interface kind but a model's.
//! - `model attach nodes` (set; get 0xB47C4, reset 0x7BB04): a model sprite's
//!   attach nodes (its `tfbModelInfo`'s), reached through a path; empty
//!   otherwise. Reading it remembers the sprite, so the next `attach node`
//!   write hangs its object on that model.
//! - `generators` (set; get 0xB3EB4, reset 0x7BB04): an emitter sprite's
//!   generators (its `tfbParticleInfo`'s); empty otherwise.
//! - `Z Depth` (float, ±10,000; get 0xB3670, set 0xB3684): stored as written.
//! - `replace color` (colour; get 0xB3358, set 0xB336C): a write sets the
//!   replace colour's red, green and blue and leaves the percentage byte. A
//!   read answers the sprite's colour, not its replace colour: the getter
//!   points at the colour's bytes (read in the disassembly). Kept as the
//!   original does it.
//! - `replace color percentage` (int, 0-100; get 0xB33AC, set 0xB33F8): the
//!   fourth byte as a percent: a read is (byte × 100 + 127) div 255, a write
//!   stores (percent × 255 + 50) div 100.
//! - `bloom intensity` (int, 0-100; get 0xB40DC, set 0xB4100): the seven bits
//!   of the placement's flags word, clamped to 100 both ways; every write sets
//!   *dirty*. Nothing here draws bloom.
//! - `active`, `removed` (sets; get 0xB32A4, 0xB32C0; reset 0x7BB04): the
//!   sprite class's two lists, every sprite awake and every sprite taken off.
//!   The updater walks `active` (Layout below).
//! - `Text Color Converter` (text colour converter; get 0x3A8DF0): the one
//!   text colour converter (below), reached through any sprite.
//!
//! ## `spritematrixmeasurement` (8)
//!
//! The placement half of a sprite: where it hangs and what hangs on it.
//!
//! - `attach node` (reference; get 0x6ABE8 through 0x5E524, set 0x6A198): the
//!   parent. A read answers the parent and remembers the model it hangs on,
//!   if any, for the path. A write first takes the sprite off its parent, if
//!   it has one: out of the parent's `attached objects` with the rest kept in
//!   order (0x158620), the list dropped when it empties, *attached* cleared
//!   and its offset reset. Then it hangs it on the new parent when the sprite
//!   is awake, the new parent is an attach node (a placement or a model's
//!   node: `AbstractAttachNode`) that is awake, and the new parent is neither
//!   the sprite nor one of its descendants: last in the new parent's
//!   `attached objects`, *attached* set. A write that fails any of those
//!   leaves it parentless. Writing the parent it already has moves it to the
//!   end of that list.
//! - `attached objects` (set; get 0x6AC60, reset 0x7BB04): the children, in the
//!   order they were hung; empty when none.
//! - `-orientation` (orientation; get 0xB01D4): the sprite itself, as an
//!   orientation, when reached through a path; nothing otherwise.
//! - `ID` (int; get 0x6A810, set 0x6A824, reset 0x6A830): an integer kept on
//!   the placement; the reset gives it an id object of its own
//!   (**Hypothesis**: unused by the interface's scripts, AT.9's census).
//! - `origin` (matrix; get 0x6A914): the placement's matrix, read-only.
//! - `clones` (set; get 0x6A928, reset 0x7BB04): the clones made of this
//!   template that are still awake.
//! - `parent` (reference; get 0x6AB0C through 0x5E524): the template the
//!   sprite was cloned from.
//! - `activator` (reference; get 0x6AB50 through 0x5E524): the placement that
//!   activated it.
//!
//! A set's reset (0x7BB04, every set above) takes each element out of the
//! set, last first.
//!
//! **States** (AT.5, read at the placement's state change, 0x69508, and its
//! deactivate hook, 0x6A620): a template in a file is a placement too,
//! asleep until a script wakes it, and a clone is awake from its spawn
//! (**Hypothesis** for which templates wake: the June savestate's 153 live
//! sprites hold the root containers and not the templates only cloned; AT.9
//! checks it against its run). A clone hangs on nothing and holds nothing
//! until a script hangs it (**Hypothesis**: the clone's internals, 0x69D7C,
//! are unread here as in aj-vm's notes; AT.9's HUD build, which writes every
//! clone's `attach node`, checks it).
//! Only an awake sprite is in `active`, hangs on or holds anything. `remove`
//! takes a sprite off: when it was awake, its playing animations stop, its
//! activator is cleared, it comes off its parent and takes each of its
//! children off itself, last first, an attach of nothing each (the
//! disassembly passes a zero, 0x6A6E4), so they stay awake and parentless
//! and draw on the root; it leaves the `clones` of what it was cloned from
//! and moves from `active` to `removed`. That's why the scripts take a
//! sprite's children off first (`UI - Remove Sprite and Attached` walks
//! `attached objects`).
//!
//! ## `spritepositionmeasurement` (5)
//!
//! Paired measurements, one sprite's place against another's (the pair's
//! second), each in the place on screen the layout gives (x, the computed
//! depth, y):
//!
//! - `screen distance` (paired float, 0 to 2^31-1 pixels; get 0xAE8D8, set
//!   0xAE9D8): the distance between the two in x and y; a write moves the
//!   first along the line from the second to that distance.
//! - `heading` (paired float, degrees; get 0xAECE8, set 0xAEED0): the angle
//!   of the first from the second, atan2 of the x difference over the y
//!   difference in degrees, 0 when both differences are within 0.001; a write
//!   moves the first round the second to that angle at the same distance.
//! - `X offset`, `Y offset` (paired float, pixels; get 0xAF45C, 0xAF15C; set
//!   0xAF618, 0xAF318): the first's x (y) less the second's; a write sets the
//!   first's x (y) to the second's less the value.
//! - `priority offset` (paired float; get 0xAF754, set 0xAF820): the first's
//!   computed depth less the second's; a write sets the first's to the
//!   second's less the value.
//!
//! None is read by the HUD's, dialogue's, pause menu's or upgrade screen's
//! closures (AT's census), so these wait for a screen that does.
//!
//! ## `stringconverter` (6) and `stringinfo` (1)
//!
//! There is one string converter, global, and the scripts build every string
//! through it (`Text - String Concatenate` grows its `value set`):
//!
//! - `value set` (set reference; get 0x9E650, set 0x9E770, reset 0x80E0C): the
//!   string as a set of character codes. A write keeps the set and makes a new
//!   `StringInfo` from it, which `text` then reads: each element read as a
//!   number (a float rounded half up), its low sixteen bits a character; an
//!   element of 64 (`@`) marks the place the number goes. The new string
//!   takes the converter's `integer digits` and `decimal digits`, *a time*
//!   when `'@' representation` is `time`, *commas* when `use comma` is on,
//!   and *has an `@`* when an element was 64. An empty set makes no string.
//! - `text` (reference; get 0x9EB3C through 0x5E524, set 0x9EB54): the string
//!   the converter last made or was given. A write of a string keeps it and
//!   turns it into the `value set`, one element per character (a
//!   `NameExtractor`'s, its object's name).
//! - `'@' representation` (enum `number|time`, 0-1; get 0x9EC04, set 0x9EC28),
//!   `integer digits`, `decimal digits` (int, 0 to 2^31-1; get 0x9EC44,
//!   0x9EC6C; set 0x9EC5C, 0x9EC84), `use comma` (enum `false|true`; get
//!   0x9EC94, set 0x9ECB8): the settings the next `value set` write copies
//!   into its string. `'@' representation` and `use comma` keep only whether
//!   the value is nonzero.
//!
//! `stringinfo`'s one member, `String Converter` (string converter; get
//! 0x3A8DF0), answers the string converter itself from any string.
//!
//! **The number fill** (0x9C858, its fraction 0x9C4F0 and its time 0x9C674,
//! read for AT.6), done each time a text whose string has an `@` is measured
//! or drawn: the sprite's `numeric value`, read as a float when the string
//! has decimal digits and as the integer it stores otherwise, written
//! digit by digit into a buffer and copied in:
//!
//! - *decimal digits* above 0 (or the string's flag bit 12, which nothing
//!   sets): its absolute value plus 0.49995 of a unit of the last place
//!   (0.049995 at one place, 0.0049995 at two, the table at 0x723A58 to 9
//!   places, 0 past that), the fraction part of that times 10 to the digits
//!   truncated and written as exactly that many digits after a `.`, the
//!   whole part going on as the integer;
//! - the integer as decimal digits: with *integer digits* above 0 exactly
//!   that many (at most 199), zero-padded and the higher digits dropped;
//!   else as many as it takes, at least one; with *commas* a `,` after every
//!   third digit from the right when another digit follows; a `-` before it
//!   when negative;
//! - or, for a time (*a time*), fields of 2 digits from the seconds up,
//!   each the integer's absolute value mod 60 and the next the quotient,
//!   joined by `:`, while *integer digits* (6 when 0, nothing below 0)
//!   allows: a field taking 2 of them, or 1 when only 1 is left, that last
//!   one a single digit taken mod 10 when above 10 (so 4 digits is `MM:SS`,
//!   6 `HH:MM:SS`, 3 `M:SS`);
//! - the buffer put where the string's first `@` stands, every later `@`
//!   dropped, the rest of the string kept; a string with neither *has an
//!   `@`* nor *a time* keeps its `@`s.
//!
//! ## `textcolorconverter` (2)
//!
//! One global converter colouring a text sprite letter by letter:
//!
//! - `text sprite` (reference; get 0xB0724 through 0x5E524, set 0xB073C): the
//!   sprite it colours; a write keeps the sprite and reads its letters'
//!   colours into `color set`.
//! - `color set` (set reference; get 0xB066C, set 0xB0698, reset 0x80E0C): one
//!   colour per letter, in order. A write colours the kept sprite's letters
//!   from it, each glyph's six vertices in its colour (0xBB5D8): as many
//!   letters as both have, the rest keeping theirs (read for AT.6; Text
//!   below).
//!
//! ## `nameextractor` (4)
//!
//! One global name extractor over an object:
//!
//! - `Object to name` (reference; get 0x9DBE8 through 0x5E524, set 0x9DC04):
//!   the object.
//! - `Name checksum` (int; get 0x9DC50): the 32-bit FNV-1a of the object's
//!   name (offset basis 0x811C9DC5), 0 when it has none or there's no object.
//! - `Instantiation size`, `type count` (int; get 0x9DCD0, 0x9DD40): numbers
//!   the object's class answers, 0 with no object; nothing in the interface's
//!   closures reads either.
//!
//! ## `viewporttitlesafeinfo` (4)
//!
//! `left margin`, `right margin`, `upper margin`, `lower margin` (int, pixels;
//! get 0x1DE74, 0x1DE88, 0x1DE9C, 0x1DEB0): four integers kept on the
//! viewport's title-safe info, read-only. **Hypothesis**: the title-safe
//! margins in frame pixels, AT.10's to read live.
//!
//! # Text
//!
//! AT.6, read in the EBOOT for this box (the text kind's handlers at its
//! vtable's slots 16-21, the bitmap font's at its slots 16-25, the number
//! fill) and checked against AC.13's 43 glyphs ([`text`] has the code).
//! Arithmetic is single precision, rounded after every operation as the
//! original's is, and a multiply-add where the original fuses one.
//!
//! A text sprite's font is its context's (`FontDataInfo`'s font), else the
//! default font, a registered static the June savestate holds null and
//! nothing read sets (0x10BBD4 registers it, 0x10C25C and 0x117618 clear
//! it): so a text with no font context measures nothing and draws nothing,
//! its size reads its stored numbers and its natural size 0 wide by its
//! stored height. The default font would take 4 for −baseline; the rules
//! below write the font's −baseline as *nb*. Its string is its `text`'s, the
//! number filled in when the string has an `@` (the string's *has an `@`*
//! or *a time*), or, when `text` holds an object that isn't a string (a
//! name extractor's), that object's name. A text with no string or an empty
//! one draws nothing.
//!
//! - **A glyph** is found by binary search over the font's list by
//!   character (0x1F5238), so the list is taken as sorted, as the disc's
//!   three are; a character it doesn't find has no glyph.
//! - **The scale** *s*: 1, or with *point size set* the point size over the
//!   font's height (`contemp` is 82 high, so 22 points is 22/82). AT.3: one
//!   unit is 1.5 frame pixels, so a glyph at *s* is its cell × *s* × 1.5.
//! - **A run's width** (0x3D7790): over its characters, each with a glyph
//!   adds its step and the font's character padding, less a kern against the
//!   glyph before when there is one and the kern amount is above 0: the
//!   smaller of (the one before's top-right corner plus this one's top-left)
//!   and (the one before's bottom-right plus this one's bottom-left), times
//!   the kern amount. A character with no glyph adds nothing, and the next
//!   one doesn't kern against anything. `contemp`'s padding and kern amount
//!   are 0, `simple`'s kern amount 2.
//! - **A string's width** (0x1F4BD0): its widest line, a line ending at a
//!   line feed or carriage return, the next starting after that one
//!   character (so CR LF leaves an empty line between).
//! - **Its lines** (0x1F4CF0): one more than its line feeds and carriage
//!   returns, less those at its end.
//! - **A height of *n* lines**: *n* × (height + *nb*) − *nb*, one rounding.
//! - **Wrapping at a width** (0x3D4EE8, its line count through 0x1F4C9C):
//!   each source line (up to a line feed or carriage return) is cut into
//!   lines; a word (a run of anything but space, tab and line ends) joins
//!   the line while the line so far (each earlier word with the blanks after
//!   it, each measured alone) plus the word stays within the width, or when
//!   it is the line's first, its trailing blanks joining with it; a width
//!   within 5e-7 of 0 doesn't wrap. The lines are written out joined by line
//!   feeds, each keeping its trailing blanks, except that an empty source
//!   line is counted and written as nothing.
//! - **Lines that fit a height** (0xB6E80): *k* = (height + *nb*) ÷ (font
//!   height + *nb*) truncated, one more when *k* + 1 lines overhang the
//!   height by 0.001 or less.
//!
//! **The size** (slot 20, 0xB7F08), what `width` and `height` read on a
//! sprite not *sized*: its string's width × *s* by the font's height × *s*
//! (one line, whatever the string holds).
//!
//! **The text's layout** (slot 16, 0xB6F70), when its string or anything it
//! measures by changed: with *sized* off, its computed size is its string's
//! width × *s* by its lines' height × *s*, the glyphs at *s* on both axes.
//! With *sized* on, the size is its written width *W* and height *H*, and:
//!
//! - neither *wraps* nor *fits*: the glyphs stretch, *W* over the string's
//!   width on x, *H* over its lines' height on y;
//! - *fits* without *wraps*: y at *s* × min(1, *H* ÷ (lines' height × *s*)),
//!   x at *s* × min(1, *W* ÷ (string's width × *s*)) and never above y; its
//!   height the smaller of *H* and the lines' height × *s*;
//! - *wraps* and *fits* (with *W* ÷ *s* and *H* ÷ *s* both above 0): wrapped
//!   at *W* ÷ *s*; when the lines that fit *H* ÷ *s* are fewer, shrunk in
//!   passes: *k* the lines that fit, at least the string's own, each pass
//!   trying wrap widths from the string's width ÷ *k* up in tenths of it,
//!   at most twelve, until the string wraps into *k* lines or fewer, then
//!   one more line for the next pass, while the widest line changes and
//!   (*W* ÷ *s* ÷ widest) ÷ min(1, *H* ÷ *s* ÷ *k* lines' height) stays under
//!   0.5. Then a widest line past *W* ÷ *s* wraps again at its own width; x at
//!   *s* × that width ratio, y at *s* × min(1, *H* ÷ *s* ÷ the last pass's
//!   lines' height), x never above y; its height its lines' at y;
//! - *wraps* without *fits*: at *s* on both axes. **Hypothesis**: the
//!   height counts the lines of a buffer this path never fills (0xB74C0);
//!   taken as the string's own lines: none of `global.bld`'s 409 templates
//!   has *wraps* without *fits* and no member sets either (AT.9's census
//!   checks the other archives').
//!
//! **`natural size` for text** (slot 21, 0xB81E8): without *wraps*, the
//! string's width × *s* by its lines' height × *s*. With *wraps*: wrapped at
//! its stored width ÷ *s*, the lines kept those that fit its stored height ÷
//! *s*; its height the kept lines less the last plus the source string's
//! lines from where the last kept line starts, at *s*; its width the last
//! kept line's, measured on the source string at the wrapped text's offset
//! (an offset the inserted line feeds move: kept as the original does it),
//! or the earlier lines' widest when that is wider than the last and
//! exceeds the stored width.
//!
//! **Its glyphs** (slot 18, 0xB79A8, through the font's 0x3D6EB0 and 0x3D5698):
//! laid out in font pixels from the text's top-left, then drawn at the
//! layout's x and y scales. A *sized* text's lines break at its computed
//! width over its x scale, plus 0.4999, truncated, and take its
//! justification; one not *sized* never breaks and sets left. Its lines are
//! the wrap above, each with its width (its words and blanks measured
//! alone). A line starts at x 0 left, at the width less its own right, at
//! floor((width − its own − its leading blanks) × 0.5 + 0.5) centred (the
//! leading blanks counted twice: kept as the original does it), the first
//! at y 0 and each next (height − baseline) lower. A line feed ending a
//! line is drawn with it, as a space. Each character with a glyph is
//! kerned as in a run's width, drawn as a quad from (x + its offset) over
//! its size with its atlas box, and moves x by its step and the padding; one
//! without moves x by the padding only when the line's last character is a
//! space (the character it reads, kept as the original does it).
//!
//! **The text colour converter** (0xBB5D8, 0xBAE20): its `color set`
//! colours the kept sprite's first letters, one colour per glyph drawn,
//! in order, as many as both have; the rest keep their colour. A letter's
//! colour is 0x808080FF until set (the font's colour at run time, read live
//! as 0.5, 0.5, 0.5, 1). Its `text sprite` write reads the kept sprite's
//! letters' colours back into `color set`, one per glyph drawn.
//!
//! # Layout
//!
//! **The frame** (AT.3, measured on AC.13's captures and the June savestate's
//! live tree): the root is the 1280x720 frame. `_Screen_Container` is resized
//! at run time to 853.333 by 480 units at 150 %, the UI and gameplay
//! containers to 768 by 432 units (90 % of its size, the title-safe area; the
//! scale stays 1.5) centred in it at frame (64, 36); so one
//! unit is 1.5 frame pixels on each axis, from the frame's top-left, offset
//! by (64, 36) for a sprite in the UI or gameplay container. A sprite with no
//! parent draws on the root, in frame pixels.
//!
//! **The updater** (`tfbSpriteUpdateGame`, 0xBCB0C, eighth of the frame's
//! twelve updaters): once a frame it walks `active` in order. A sprite's
//! parent is its `attach node` when that is a sprite, else the model it hangs
//! on when that is a sprite, else the root. A sprite whose parent hasn't been
//! laid out this frame goes to the end of `active` and the walk takes the
//! next one, so parents are laid out before their children and the list's
//! order drifts that way. A parent changed since the last frame sets
//! *dirty*. Then each sprite (0xBBD04):
//!
//! - is *drawn* when its visible bit is set and its parent is drawn (the root
//!   always is), and *dirty* when its own bit is set or its parent was dirty
//!   this frame; a sprite not drawn has its dirty bit cleared and its kind's
//!   geometry released;
//! - is laid out (0xBA240) when drawn or dirty;
//! - is drawn by its kind unless it is a container, which draws nothing; when
//!   dirty its kind builds its geometry again first (a text's glyphs), and a
//!   kind not ready (an image still loading) skips the sprite this frame.
//!
//! **Laying out one sprite** (0xBA240), in its parent's space:
//!
//! - its point is its `location` (12.4 to units);
//! - its computed scale is its own (percent over 100) times its parent's, per
//!   axis; the z scale counts only for a model, a container or particles;
//! - its computed priority is its `priority` plus its parent's computed
//!   priority, its computed depth its `Z Depth` plus its parent's;
//! - its colour is its colour with the alpha times its parent's computed
//!   alpha over 255; its replace colour is copied;
//! - its size is its kind's (0xB9ECC: written when *sized*, else computed),
//!   and a sprite not *sized* stores the computed size as its width and
//!   height; an art sprite's image stretches to it (0xB64BC);
//! - in a sprite parent (not hung on a model's node), its point is at the
//!   parent's scale and, unless the parent is a model or particles, moved to
//!   the parent's `align to parent` anchor (the parent's column × ½ its
//!   computed width and row × ½ its computed height, at the parent's scale);
//!   then carried by the parent's place on screen;
//! - unless it is a model or particles, its own `alignment` anchor is
//!   subtracted (column × ½ its computed width and row × ½ its computed
//!   height, at its scale), so that point of it sits on the point above;
//! - with no parent sprite its depth is 10,000 (20,000 × ½); with one, its
//!   `Z Depth` × 25 plus what it inherits.
//!
//! **Hypothesis** (AT.7 and AT.24 settle it): the y anchor subtracts as the x
//! one does (the decompile shows the x subtraction and folds the y into a
//! vector op), which AT.3's parentless quads agree with.
//!
//! # Colour and blends
//!
//! AT.3, off AC.13's fragment program: a sprite draws rgb = texture × tint × 2
//! and alpha = texture alpha × tint alpha, the tint its colour bytes over 255,
//! then mixes rgb toward its replace colour by the replace percentage. The
//! blends the captures draw: crossfade (1: alpha over), cuts out (3: the alpha
//! written into the destination's alpha), cuts out (invert) (7: the same, the
//! program's alpha inverted), gets cut out (4: colour by the destination's
//! alpha) and clears cut out (5: the destination's alpha reverse-subtracted).
//! Opaque (0), additive (2) and gets cut out (add) (6) no captured frame
//! draws (**Hypothesis**: by their names, AT.7).
//!
//! # Draw order
//!
//! Drawn sprites join the frame's sprite list in the updater's walk order
//! (parents before children). **Hypothesis** (AT.7 reads the renderer and
//! checks AC.13's 72 draws against it): the list is drawn by computed
//! priority, lower in front (a message box's text is written at priority -2
//! over its box), the walk order breaking ties.
#![forbid(unsafe_code)]

pub mod members;
pub mod model;
pub mod sprite;
pub mod text;

pub use members::{MEMBERS, Member, find};
pub use model::{Answer, Bases, LETTER, Model, NoWorld, Refusal, Str, World};
pub use sprite::{Sprite, SpriteId, State};
