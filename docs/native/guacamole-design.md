# Guacamole design

Status: DESIGN, started 2026-10-17 and revised the same day for chotchki's second, third and fourth rounds of
decisions.
This file owns the Guacamole track (`openspec/config.yaml` points here). Open changes:
`openspec/changes/aa-guacamole-shell/`, `ab-giants-extract-and-intros/` and `ac-giants-decode-gate/`; the
later rungs of the ladder are lines in the Guacamole section of `openspec/backlog.md`. Until G1 the
product (CLAUDE.md, SPEC.md, `docs/dev/*`) is owned where it always was, and it's frozen: the controller
installed on the HTPC keeps playing the games and chotchki doesn't have to touch it until Guacamole is
ready.

## What Guacamole is

A from-scratch Rust/Bevy reimplementation of the PS3 Skylanders games WITH the original gameplay,
Skylanders Giants (BLUS30968) first, so the art can be upscaled and relit (on the Mac first, the HTPC
later). Users supply their own disc dumps, `guac-extract` pulls the assets out at install time and nothing
read from a disc ever ships or lands in this repo. The phone stays the portal, and a figure is just an
identity out of the disc's own roster, so nobody dumps a toy and nothing reads a `.sky`. It runs as a
second binary, `guacamole`, beside the product launcher until it can play a whole game.

The bar is parity: what the original game observably does, as seen through the patched RPCS3 (the
oracle), at the original logic tick, EXCEPT the deliberate departures listed below. A Faithful render mode
matches the original look first and an Enhanced toggle layers modern lighting and upscaled textures on
top. Faithful comes first because the oracle can check it and can't check Enhanced (and an upscaled game
that plays wrong isn't the goal).

The why is chotchki's: exploring what's even possible and bringing joy to his kids. There's no time or
cost pressure and the scale is not up for re-argument. What the motivation DOES set is ordering: every
rung of the ladder ends in something the kids can see on the TV, early and often, ahead of
plumbing-only phases.

Out of scope: the product's eframe launcher and patched-RPCS3 window management (left alone until G1), a
USB/HID portal (never), anything online and reading a `.sky` file in any form.

## Decisions

All chotchki's, all 2026-10-17. Quotes are his words (spelling fixed). The second, third and fourth
batches came later that day and a later batch wins wherever two touched. What the second superseded (a
one-way `.sky` import, sky-parser in the Guacamole path, stats read from `.sky`, finding dumps through
RPCS3's `games.yml`, the product's wizard as a prerequisite, importing RPCS3 saves, trophies cut, PvP
undecided), the open questions the third answered and the HTPC as the gate every early rung checked
against (the fourth) are gone from this doc, not kept beside it. A proposal that needs one reversed says so
and asks, it doesn't relitigate quietly.

First batch:

- Option C, a full native rewrite with original-gameplay parity, chosen knowingly over the smaller
  options. Declined as the main track: a native sandbox on extracted assets with non-faithful gameplay
  (A), original logic in a hidden RPCS3 driving a Bevy renderer (B), an RPCS3 texture-replacement/post-FX
  patch and static recompilation. Those survive only as supporting tools (texture dumps to check the
  extractor, the patched RPCS3 as the oracle).
- Codename Guacamole, in his words: Engine codename "Guacamole". Binary `guacamole`, crates `guac-*`
  (`guac-formats` pure fuzzed parsers, `guac-ir` versioned serde IR, `guac-extract` the CLI, `guac-server`
  the shared server lib). Never "Skylanders..." in a crate or binary name.
- Giants first: "I'm fine with doing Giants first especially since it will probably be easier to do the
  later games too." Scoring is in Why Giants first.
- Two binaries for the transition: "I'm fine with keeping the existing window management structure (or
  even delivering two different binaries if that makes it easier)." The product binary's behavior and
  window management stay as they are until G1; `guacamole` is a separate Bevy app embedding the same
  server. How much of the product's SOURCE moves for that is the third batch's server split.
- One app at the end: "there's no need for the multiple windows if I own the whole thing." G1 makes
  `guacamole` the only thing this repo builds and ships; G2 retires the oracle.
- Planning is pure OpenSpec in this repo: "I recommend we nuke the hooks for this repo and just go pure
  openspec." The plan-bridge hooks and state are gone, PLAN.md's open phases became
  `openspec/changes/<id>-<slug>/` with their ids kept, PLAN_ARCHIVE.md is frozen.
- Wiki data is temporary: "those were scraped out of a fandom wiki and I'd argue that once we are parsing
  the game assets directly they are not needed (and neither is the reconstructed lookup tables since
  we'll be owning all that data)." `guacamole` stops reading it once the disc roster lands; the files go
  at G1 with the product code that still reads them (Figures).
- The phone is the portal. No USB/HID portal driver, ever.
- Target hardware: "The HTPC has a 2080 Ti GPU." The HTPC (Windows, Steam Big Picture, 86" 4K TV, the
  kids' machine) is the eventual performance and feature floor. Until its bring-up the M3 Max Mac is the
  target as well as the dev box (fourth batch).
- Tooling: the rea MCP isn't used (measured, see RE toolchain). Ghidra plus RPCS3's own tools.
- Hard rule: no game bytes in the repo, ever (last section).
- Method: observe in the oracle, write the behavior down in our own words as an OpenSpec spec, implement
  from the spec, run the original script data instead of rewriting it.

Second batch:

- Figures get a FULL reset. Figure crypto was already moot ("once we own the engine none of that matters
  anymore"), and then: "On the figure crypto point, I'm fine with a full reset. The web UI already allows
  figure editing but it's buggy due to the complexity of the sky format. Bypassing it entirely saves us a
  lot of reverse engineering, also people don't have to dump all their figures (we would drop the ACR122
  support too)." Guacamole never reads a `.sky`, so there's no import, no figure crypto and no firmware
  pack or figure dumps to collect. A figure is an identity (toy type + variant) from the disc's own
  roster and its progression starts fresh in Guacamole's own store.
- Per-profile figure instances stay: "I still want to keep the separation by user of figure instances
  like we do today." One instance per profile per figure. No per-kid shelf though: "every character on
  the disc is basically what's already seen since my kids had such a vast collection", so the phone shows
  the whole disc roster.
- The ACR122 reader leaves the product "as soon as it's convenient", as its own product-track removal
  (`crates/nfc-reader`, `scanned/<uid>.sky`, `FigureScanned`), not tied to G1: change AD
  (`ad-remove-acr122`), which lands before AA and takes `GameOfOrigin::Unknown` with it (fourth batch).
  `crates/sky-parser` stays only as long as the product binary does.
- Persistence and setup: "what has to be persisted on disk for saves/progression should be way easier /
  simpler no files to copy around. the setup screens get way easier too." One SQLite database holds
  everything worth keeping, the extract dir is a regenerable cache and setup is pointing Guacamole at the
  disc dumps. Story saves start fresh too, no import of RPCS3 saves.
- Story saves belong to profiles: "Tying story progression to a kid's profile would be a great quality of
  life improvement, right now they fight over whose slot is whose. I would say the profile that starts
  the game uses its slot." One save per profile per game, no slot picker. Co-op rules are in Deliberate
  departures.
- Online out, trophies in as achievements: "anything online should be dropped, I think we could bring in
  the trophies / achievements to the save profile."
- Local PvP is in: "keep local PvP, the kids love couch battles it's part of the core game(s)". It's part
  of parity for every game, and the first couch battle is its own rung right after the first fight.
- Controllers: prompts match the physical pad in use, and on Steam Input: "we should be able to tell Steam
  to pass through the native game controls, other games do it". The hardware checks behind that: "I'd
  say this is all non-urgent testing" (Controllers).
- The product is frozen: "I have a fully functioning portal controller on the HTPC and do not have to do
  anything to it until this is ready." G1 makes `guacamole` the only thing this repo builds and ships, and
  the controller already installed on the HTPC keeps playing the other five games until Guacamole covers
  them.
- No time estimates: "I disregard any/all time estimates in the face of AI development ... any/all
  estimates are completely hallucinated" and "I will accept estimates when they are based on evidence ...
  take the agile approach and try/iterate/measure. For this project none of that matters I'll decide based
  on our progress what I want to do." This doc sequences by conditions and evidence and carries no hour,
  week or year figures.

Third batch:

- Guacamole gets its OWN database: one SQLite file under its own data root from AA on (Persistence and
  setup). Sharing the controller's `db.sqlite` would brick the frozen controller (its migrator refuses a
  migration it doesn't know), and the profiles don't come across: "set them up again, it's super simple."
  `guacamole` never opens the controller's `db.sqlite` or `config.json`, so its pairing key is its own too
  and a phone switching apps scans the TV's join code once.
- Distribution: "Eventually this will be MSI based, this is where dual binaries is a good starting point."
  The Windows MSI carries both binaries until G1, starting with the Windows/HTPC bring-up (fourth batch).
  A `v*` tag (only on chotchki's word, like every release) installs the product rebuilt from current
  source beside `guacamole`, so the controller the kids use for the other five games gets replaced on
  install and its behavior has to survive AA's split (Architecture says how that's proven).
- The server split: "I'm good with it." `crates/guac-server` takes only what `guacamole` runs. `.sky`
  editing and stats, working copies and the eframe UI stay in `crates/server` and leave with it at G1.
  NFC isn't there to split: AD (the ACR122 removal) lands before AA and takes it out first.
- Achievements in co-op, for a counter both kids feed: each profile earns it from its own share, "I'm good
  with that if we can detect it." Where Guacamole can't tell whose share it was, only the host earns it
  (the question's other option). The co-op rules in Deliberate departures carry the whole set.
- The parity target is Giants' 1.00 disc: "HTPC's Giants doesn't have the title updates I couldn't figure
  out how to install them." The extractor reads the disc alone, and a dump a title update changed is an
  unknown dump (The extractor).
- Git: "Commit freely on a branch, we'll use a 2.0 branch as our 'main' until we've gotten through some
  milestones." Guacamole and all OpenSpec work land on `2.0`; `main` holds the frozen product line
  (CLAUDE.md, Git workflow).
- Ladder ids float: `openspec/config.yaml` allocates each one at `/opsx:propose` (this doc's working
  assumption, which round three left standing; no quote).
- Physics: Avian replaces Havok, "probably avian if I had to guess." Avian 0.7.0 and its main branch were
  still on Bevy 0.19 on 2026-10-17, so until its 0.20 release Guacamole takes the branch the developer
  recommends, avianphysics/avian#1070 (Bevy 0.20 + Parry 0.31), as a git dependency pinned by rev
  (`34c9b008d2f4` today; it was written against 0.20.0-rc.1, so the first compile against 0.20.0 is the
  check). Physics sits behind a thin internal seam either way. Nothing needs it before AG, and a
  read-only census of what Giants uses from Havok (character proxy or its own controller, rigid bodies,
  constraints, ragdolls, collision shapes) comes first, since that decides what feel parity means.

Fourth batch:

- Mac first: "Meh doesn't matter, we're focused on mac for now, windows will come much later." That was
  his answer to whether AD needed a Windows build before a tag, and it sets the whole track's order:
  Guacamole is built, run and validated on the M3 Max Mac, which drives the TV, and no rung's exit depends
  on Windows or the HTPC. The HTPC stays the eventual target (first batch) and gets ONE later bring-up, a
  line beside the ladder in `openspec/backlog.md` that collects everything Windows the rungs used to gate
  on: the MSI carrying both binaries (and the installed-product check that comes with it), Windows CI, the
  firewall prompt, Steam Big Picture with Steam Input passthrough, DLSS and the 4K floor on the 2080 Ti,
  the pad checks on Windows, the HTPC's own Giants dump and `guacamole.exe` running there. It starts on his
  call, not on a rung. What Mac first costs the renderer is in Renderer (no temporal upscaler, synchronous
  pipeline compiles).
- The Mac's checks are by hand plus `.githooks/pre-push` ("Enable the hook": it's on in the dev clone,
  `core.hooksPath=.githooks`). Push CI fires only on `main` (`ci.yml:3-5`) and the macOS lane has been off
  since 2026-05-30 (`:150-155`), so on `2.0` the hook is the only automatic gate.
- AD's open calls: `GameOfOrigin::Unknown` goes with the scanner ("Remove it."), the wizard copy AD
  proposed stands ("Don't care, we're going to touch it again later.") and AD needs no Windows build
  before a tag (the Mac-first quote above).

## Deliberate departures from the original

Parity means matching the original EXCEPT this list. Each item is chotchki's call (Decisions); the oracle
checks everything else.

- No toy and no portal hardware. The phone is the portal and offers every character on the disc, where
  the original needs the physical figure on a USB portal.
- Figure progression lives in Guacamole's store, keyed by profile + figure, never on a toy or in a `.sky`.
  Two kids with the SAME figure each have their own instance (today's working-copy rule, minus the file).
  Nothing flows back to the original games or to RPCS3.
- The phone can edit a figure's progress (level and gold, plus reset to fresh), which the original never
  offered. It edits rows in the store, not a file.
- Story saves belong to profiles: one save per profile per game and no slot picker. The profile that
  starts the game plays on its own save.
- Trophies become per-profile achievements stored with the profile instead of PSN trophies. Names and
  icons come from the disc's `PS3_GAME/TROPDIR` if its container reads (unverified); if not, we define our
  own from the unlock conditions the oracle shows. Who earns one in co-op is in the co-op rules below.
- Online is gone: DemonWare, PSN and anything else networked beyond the LAN link to the phone. The
  DemonWare evidence comes from SSA's EBOOT (103 matching strings, `cellHttp` and `sceNp` imports);
  Giants' online surface isn't measured, and the cut covers whatever is there.
- Button prompts show the label printed on the pad in the player's hands, not the PS3's glyphs
  (Controllers).
- The Enhanced renderer is an opt-in toggle over the Faithful default (Renderer).
- Rendering runs above the original frame rate by interpolating between logic ticks; the logic stays at
  the original tick (Renderer).

Co-op, spelled out:

- The host is the profile whose phone started the game. Its story save loads and only that save advances;
  the second player plays along on the host's story and their own save for that game doesn't move.
- Figure progress (XP, gold, upgrades) goes to the profile that placed the figure, today's ownership rule
  (`placed_by` on each loaded slot), so both kids' characters advance.
- In PvP no story save is in play; whatever figure progress a battle awards (the oracle will say) follows
  the same placed-by rule.
- Achievements follow the same split. One tied to the story follows the host's save and one tied to a
  figure follows the profile that placed it. A counter both kids feed counts each kid's own share toward
  their own profile wherever Guacamole can tell whose share it was ("I'm good with that if we can detect
  it"); where it can't, only the host earns it. Which counters can be attributed gets settled per
  achievement when AM builds them.

Not a departure: local PvP (Battle Mode, `pvp_maincontrol.arc` in Giants) is part of parity for every
game.

## Architecture

### Two binaries until G1

Verdict: the product binary (`crates/server`'s `skylander-portal-controller`: eframe launcher, patched
RPCS3, P7/P8 window management) stays frozen in behavior and window management, and `guacamole` is a
separate Bevy app that embeds the SAME server in-process, with a data root and database of its own. On
the Mac `guacamole` runs from `cargo` beside the product. From the Windows/HTPC bring-up on, both reach
the HTPC in one Windows MSI: a `v*` tag, on chotchki's word, installs the product rebuilt from current
source beside `guacamole` (Decisions, third and fourth batches).

```mermaid
flowchart LR
  phone["Phone SPA (Leptos)"] -- "REST + WS" --> axum
  subgraph g["guacamole process"]
    subgraph bg["background OS thread: tokio runtime (guac-server)"]
      axum["Axum"] --> worker["driver worker"]
      worker --> native["native PortalDriver"]
    end
    subgraph mt["main thread"]
      bevy["Bevy app: ECS, renderer, audio, input"]
    end
    native -- "channel: placed / cleared (identity + owner)" --> bevy
  end
  subgraph root["guacamole's own data root"]
    db[("SQLite: profiles, PINs, figure instances, story saves, achievements, settings")]
    extract[("extract/, a regenerable cache: KTX2, glTF, audio, video, IR (release; dev keeps it outside the tree)")]
  end
  ctrl[("the controller's db.sqlite + config.json")]
  dumps[("disc dumps (read-only)")]
  axum --- db
  axum -.->|"never opens"| ctrl
  bevy -- "progress + saves" --- db
  bevy --- extract
  dumps --> gx["guac-extract (first run)"] --> extract
```

- Bevy owns the main thread (winit wants it, macOS insists). Axum, the phone SPA, SQLite and the driver
  worker run on a tokio multi-thread runtime on a background OS thread that `guac_server::boot` starts,
  the same split the product has with eframe on main
  ([`crates/guac-server/src/boot.rs`](../../crates/guac-server/src/boot.rs)).
- The native PortalDriver backs the existing driver worker and places by identity: the server's figure
  ids already are the tag form `<toy type>-<variant>` (`0007d6-0000`), and a placement carries that plus
  the profile that placed it (the placement seam, below). No file is involved. Of the trait's RPCS3-shaped
  methods ([`crates/rpcs3-control/src/lib.rs`](../../crates/rpcs3-control/src/lib.rs)), `open_dialog` is
  required and is a no-op, and the IPC-only ones (`ipc_socket_path`, `emu_state`, `game_window_handle`,
  `game_surface`, `window_set`, `reconnect`) keep the trait's "not applicable" defaults, as
  `MockPortalDriver` does. Trimming the trait waits for G1, when its RPCS3 implementations leave.
- The portal protocol doesn't change (REST commands return 202, WS carries the snapshot then
  `SlotChanged`). The visible differences: the figure list is the disc roster (AA's interim catalogue
  until it lands), the game list offers what `guacamole` can play from the dumps it was pointed at
  (Giants), a figure's details and edits read and write Guacamole's store (from AH; before that every
  figure is fresh, so STATS and RESET TO FRESH are disabled with a "not available yet" note) and the
  grown-ups toggles that rewrite the product's RPCS3 and window settings answer 409. The phone says the
  window-mode and 2× toggles aren't available in guacamole and its menu leaves out HOLD FOR GAME SETTINGS
  (AA.19). It knows which app it's talking to from `/api/status`'s `app` (`product` or `guacamole`, keyed
  on the driver kind), which also sets whose placements block a figure: the product refuses one already on
  the portal whoever placed it, guacamole only when the viewer's own profile did, so two kids can each put
  their own Eruptor on (AA.18).
- One binary at a time. Both bind the same HTTP port (8765, the default the controller's setup writes)
  and advertise the same mDNS name, so the join address is the same for either. The listener binds
  before anything with a side effect (the boot call, below), so the second one to start sees the port
  taken, stops there and says so on the TV instead of fighting (`guacamole` shows its already-running
  screen for 8 s and exits). The pairing key is each app's own, so a phone that last joined the other
  app gets the phone's existing pairing screen and scans the TV's join code once. On the HTPC (the
  bring-up) Steam Big Picture gets two non-Steam shortcuts, with Steam Input off on `guacamole`'s
  (Controllers).
- Data lives in `guacamole`'s own SQLite plus the extract dir, both under its own data root (Persistence
  and setup). No working copies, no scanned figures and no RPCS3 config, and the extract dir sits outside
  the tree in dev (the hard rule says why).

The crate cut (AA.1). Only what `guacamole` runs moved, into `crates/guac-server` (lib `guac_server`): the
router (`http`), the driver worker and watchdogs (`state`), the profile store, Kaos, mDNS, the round QR,
`paths`, `logging`, the embedded phone bundle, the config types both loaders fill and the `BUILD_TOKEN`
half of the build script. `firewall` went too, because `LauncherStatus` carries its result and the
connectivity watchdog calls it (leaving it would have cost a fifth hook). AA then added `boot`, `setup`,
`dumps` and `native_catalogue` there. `crates/server` kept `.sky` editing and stats (`sky_edit`,
`sky_stats`, still behind its `sky-stats` feature), `working_copies`, `display_mode`, `rpcs3_config`, its
`config` loader and first-launch wizard (never ported: Guacamole's setup is one question), its three
migrations, the eframe launcher (`ui/`, the badge and vortex rigs, `compositor`, `gl_fallback`) and the
Windows resource stamping, and re-exports the moved modules so its own code reads as before. `cargo tree
-p guac-server -e normal --no-default-features` has no eframe, egui, glow, winresource or sky-parser in it.

Where moved code needs something that stayed, it calls one of four hook traits
([`crates/guac-server/src/hooks.rs`](../../crates/guac-server/src/hooks.rs)), each implemented in
[`crates/server/src/product_hooks.rs`](../../crates/server/src/product_hooks.rs) with the code that used to
sit at the call site. The product passes all four to `boot`; `guacamole` passes none, and the missing hook
IS its Native arm at that site.

| Hook | Covers |
|---|---|
| `FigureFiles` | the figure stats, edit and reset routes; the working-copy fork on a load, Kaos swap or auto-restart restore; the rewrite on a slot reset |
| `DisplayModes` | pre-setting and capturing the per-game Windows display mode around a launch |
| `Rpcs3Config` | RPCS3's Net config at boot and the transient savestate settings |
| `SettingsWriter` | the product's settings file behind the window-mode and 2× toggles |

No behavior change for the product, and that matters more than it did: the next tag ships this source's
product, and the bring-up's MSI puts it on the HTPC. `cargo test --workspace` ran 392 tests (350 passed +
42 ignored) at the pre-AA base (46866ce) and after the cut, and the product's phone e2e suite on the Mac
passes every test that passed at the base (AA.2, again at AA.10; what still fails failed there too, and
the HTPC's check is the bring-up's).

The boot call (AA.2). `guac_server::boot(BootOptions) -> Result<ServerHandles>` is everything between
"config loaded" and the binary's UI; the product's entry point went from 825 lines at the base to 249 (GL
bootstrap, config, logging, boot, eframe). The options: the loaded config (each binary has its own loader),
the runtime root (`db.sqlite` sits directly under it), the migration set, a driver factory run ON the
tokio thread (UIA initializes COM as MTA there), the hooks, extra routes merged into the router
(`guacamole`'s `/api/_test/tv`, under `test-hooks` only) and the catalogue: `FirmwarePack { scan }` for the
product (it hands `skylander_indexer::scan` in, since the indexer links sky-parser) or `Native { games }`
for `guacamole`. Inside the thread the listener binds FIRST, then the RPCS3 Net config write (real
emulators only), the driver, the database and the rest. An `AddrInUse` sets
`LauncherStatus::port_in_use`, and the second app stops before it builds a driver, writes RPCS3's config
or opens the profile store (a boot test checks all three; `guacamole`'s own loader has already read an
existing `db.sqlite` by then, and writes nothing). With no `bind_ip` it listens on 0.0.0.0, so the lock
holds across a route change, and the QR carries `<hostname>.local` when the OS hostname reads, else the
first non-loopback IPv4 (the raw-IP URL always carries the IP). Back come the
launcher status, the client counter, the phone and raw-IP URLs, the join-QR PNG, the event sender, the
game list (shared, so `guacamole`'s setup replaces it after boot), the worker's job sender and the server
thread.

The Native table (AA.3). `DriverKind::Native` gets an explicit arm at every site that assumed RPCS3 or a
figure file, keyed on the driver kind or on the hook being absent, never on `cfg(windows)`:

| Site | Product | `guacamole` (Native) |
|---|---|---|
| catalogue at boot | indexes the firmware pack, decorates vehicle terrain | the interim catalogue (Figures); a missing [`figures.json`](../../data/figures.json) fails boot before the bind |
| RPCS3 Net config at boot | written for UIA and IPC | skipped |
| IPC STATE poller | IPC only | not spawned |
| tracked RPCS3 process | Mock installs a stand-in | none, ever |
| game list | RPCS3's `games.yml` (Mock: every game) | the dump scan: Giants alone when a BLUS30968 dump was found, else empty |
| load, Kaos swap, auto-restart restore | fork or rewrite the profile's `.sky` working copy, then `load(slot, path)` | no file: the job's path is `None` and the worker calls `place` |
| slot reset; figure stats, edit and reset routes | rewrite or read the working copy | 409 |
| launch | EBOOT lookup in `games.yml` (not for Mock); display mode pre-set, then captured after the first load | neither |
| `BootDirect` | IPC spawn, else a UIA spawn on Windows | returns before any spawn arm, on any OS (the job carries the driver kind) |
| window-mode and 2× toggles, RPCS3 settings | rewrite the settings file; open RPCS3's settings (409 for Mock) | 409 |
| quit | kills RPCS3 under a cover | `DriverJob::ResetPortal` empties the driver's slots (the TV gets `Cleared`) and keeps the saved layout for "resume last setup?" |

`BootDirect` was the dangerous one: its non-IPC arm was gated on `cfg(windows)`, so anything without an
IPC socket would have launched RPCS3 on the HTPC. The unit test hands it a driver that advertises an IPC
socket, so a missed arm would try `launch_no_gui`. A catalogue record has no file under Native (its path
stays empty); making the path optional in the shared figure type waits for G1 with the rest of the
file-shaped API.

The placement seam (AA.3, AA.7). `DriverJob::LoadFigure.path` is `Option<PathBuf>`: `Some` for the RPCS3
drivers and the mock exactly as before, `None` under Native. For `None` the worker builds one `Placement`
([`crates/rpcs3-control/src/lib.rs`](../../crates/rpcs3-control/src/lib.rs)) and calls
`PortalDriver::place(slot, &placement)`, which defaults to an error ("this driver loads figure files"), so
a driver that loads files never quietly accepts one. A `Placement` is the figure id, its toy type and
identity-masked variant parsed from the id (AB keys the intro on the toy type, not the id string), the
name and element, the portrait path (server-private) and the owner (profile id, initial and colour; none
when no profile is unlocked). `NativePortalDriver`
([`crates/guacamole/src/driver.rs`](../../crates/guacamole/src/driver.rs)) errors on `load`, keeps its own
8 slots for `read_slots` and sends `PortalChange::Placed` or `Cleared` over a bounded (64)
`crossbeam-channel` with `try_send`: a full or closed channel drops the change, counts it and logs a warn,
which costs a stale card, never a hung load. A `PreUpdate` system
([`crates/guacamole/src/cards.rs`](../../crates/guacamole/src/cards.rs)) drains it into `PortalCards`
through a pure reducer, and the TV rebuilds its cards from that. Failure injection works against either
app: the mock and the native driver both implement `LoadInjector` (`skylander-rpcs3-control`'s `inject`),
so `/api/_test/inject_load` drives both.

### Running guacamole on the Mac

Verdict: from `cargo`, never while the product runs (the port lock). The dmg stays product-only, and
the Windows MSI that carries `guacamole` is the bring-up's (AA.13).

Dev, from the repo root:

```sh
(cd phone && trunk build)    # same tree as the server, or the phone's stale-version overlay fires
cargo run -p guacamole       # add --features test-hooks for the /api/_test/* endpoints
```

- It reads the dev env file the product reads (the keys are in
  [`.env.dev.example`](../../.env.dev.example)), and only these matter: `GUAC_DUMP_DIRS` (folders in the
  OS's path-list form; set, it overrides the stored folders and the TV skips setup), `BIND_PORT` (default
  8765), `DATA_ROOT` (default `data`, the interim catalogue's source) and `WINDOW_MODE` (`desktop` is a
  resizable 1280x720 window, anything else is borderless full screen on the primary display). Never
  `RPCS3_EXE` or `FIRMWARE_PACK_ROOT`.
- State goes under `./dev-data/guacamole/` (`db.sqlite`); logs go to `./logs/`, the same daily
  `server.log` the product's dev build writes.
- `GUAC_SHOT_DIR=<dir>` (process env, dev builds only) renders the TV offscreen at `GUAC_SHOT_SIZE`
  (default 1920x1080) and saves a PNG each time it settles. AA.8 checked its screens that way, because
  window captures on the locked Mac came back black.

Release, the build AA.11 runs on the TV:

```sh
(cd phone && trunk build --release)    # rust-embed bakes phone/dist into the binary
cargo build --release -p guacamole --no-default-features
SKYLANDER_APP_DIR="$PWD" target/release/guacamole
```

- No dev env file and no dev shortcuts: the window is always borderless full screen on the primary
  display, the data root is `~/Library/Application Support/guacamole/` (`db.sqlite` and `logs/`) and a
  first start asks for the dumps folder (Persistence and setup). Deleting the data root goes back to
  first run.
- The catalogue's `data/` resolves next to the executable (`paths::app_data_root`) and `target/release/`
  has none, so `SKYLANDER_APP_DIR` points it at the checkout. Without it there's no
  [`figures.json`](../../data/figures.json) and the TV shows the server-error screen.
- Measured 2026-10-18 (AA.6): 1 min 44 s cold on the M3 Max, 111 MB unstripped. Its first run on the TV
  is AA.11.

### G1: one app

At G1 (`guacamole` plays one game end to end) `guacamole` becomes the only thing this repo builds and
ships. The product binary leaves, and with it everything RPCS3 in what ships: `rpcs3-patches/` P1-P8 as a
product series, `rpcs3-control`'s product drivers (IPC and UIA), the nested-app bundling and signing, the
Win32 `SetWindowPos` tiling, the macOS CALayerHost embed (`crates/server/src/compositor.rs`),
`crates/sky-parser` and the wiki data the product's phone UI still reads. (`crates/nfc-reader` is gone
before AA, with AD on the product track.) The TV launcher surface (the QR badge, `BadgeRig` in
`crates/server/src/badge.rs`) is rebuilt as Bevy meshes and materials before then.

It leaves what SHIPS, not the repo. The RPCS3 submodule plus a dev-only patch series stay as the oracle,
built on dev machines and never shipped, until G2 (parity) retires them.

Nothing is lost on the HTPC. G1 stops BUILDING the product, it doesn't uninstall it: the controller
already installed there is frozen and keeps playing Spyro's Adventure, Swap Force, Trap Team,
SuperChargers and Imaginators until Guacamole covers them. One catch the MSI brings: the installer
upgrades in place (`MajorUpgrade` under one `UpgradeCode` in `wix/main.wxs`), so a G1 MSI that carries
only `guacamole` under that code would remove the controller. AN's packaging has to keep it installed;
how is AN's call when it's proposed.

## Persistence and setup

Verdict: `guacamole`'s own SQLite database, under its own data root, holds everything worth keeping, and
nothing else on disk is precious. Backing up the family's progress is copying one file.

- In it: profiles and PINs, per-profile figure instances (Figures), per-profile story saves (one per
  profile per game), achievements and settings (the dumps folders, the port, the pairing key). It starts
  in AA with the profiles and settings and grows a table set per rung (the figure store at AH).
- Its own file, never the controller's (Decisions, third batch). The controller installed on the HTPC
  opens its `db.sqlite` through `sqlx::migrate!` at its defaults, which refuses a database carrying a
  migration the binary doesn't know, so one Guacamole table in that file would stop the controller the
  kids still use for the other five games from starting. `guacamole` never opens the controller's
  `db.sqlite` or `config.json`, and its migrations are a set of its own, so nothing it adds can reach the
  controller's file even through the product binary it ships beside.
- Two migration sets (AA.1, AA.5). `ProfileStore::open` runs whichever `Migrator` the caller passes to
  `boot`. The product passes its own three (`crates/server/migrations/`, frozen), `guacamole` passes
  `setup::MIGRATOR` (`crates/guac-server/migrations/`): the product's three folded into one file, same
  tables (`game_display_modes` included, because the store queries it), plus a `settings` table. A test
  runs the store's queries against a database migrated by each set, so the two can't drift apart unseen.
- Settings live in that table: the dumps folders, the port (8765; only the dev `BIND_PORT` changes it,
  since nothing writes the stored one yet) and the pairing key, 32 random bytes made on the first start
  and stored only once the server came up, so a second copy that lost the port never writes one. Its
  loader ([`crates/guac-server/src/setup.rs`](../../crates/guac-server/src/setup.rs)) writes nothing: it
  opens an existing `db.sqlite` read-write only so SQLite can roll back a crash's hot journal, and a
  database with no `settings` table reads as never set.
- Profiles aren't copied over. The kids set theirs up again in `guacamole` (name, colour, PIN, through the
  phone's existing create-profile flow), and each app keeps its own from then on.
- Where: `~/Library/Application Support/guacamole/` on macOS (the target until the bring-up) and
  `%LOCALAPPDATA%\guacamole\` on Windows (machine-local, because the gigabyte extract dir lives under it
  and doesn't belong in a roaming profile), a root separate from the controller's on either OS (its
  `%APPDATA%\skylander-portal-controller\` on Windows). Dev uses `./dev-data/guacamole/`, gitignored with
  the rest of `./dev-data/`. `paths::guac_data_root` picks it and `boot` takes it as the runtime root,
  so the product's root doesn't move.
- Not in it, and not needed anywhere: `.sky` working copies, `scanned/`, RPCS3 savedata, PS3 firmware or
  RPCS3's config. No files to copy around.
- The extract dir is a regenerable CACHE, `extract/` under the data root in release and a path outside
  the working tree in dev (the hard rule says why). Delete it and `guac-extract` rebuilds it from the
  dumps (its manifest already forces a rebuild on an extractor or IR version bump), so nothing in it gets
  backed up. The local Enhanced pack is the same kind of cache.
- Story saves start fresh, like figures. The kids' RPCS3 Giants saves aren't imported.
- Setup asks one thing: where the disc dumps are. No RPCS3 path, no PS3 firmware, no figure pack and no
  `games.yml` scan. Guacamole identifies each dump itself and offers the games it can play. AA reads
  TITLE_ID from `PARAM.SFO` and nothing else; the extractor's identity key (The extractor) is AB's.
- The scan (AA.5, `dumps::scan`) looks in each chosen folder and up to two levels below it for
  `PS3_GAME/PARAM.SFO`, reads at most 64 KiB of each and reports a chosen folder it couldn't list or an
  SFO it couldn't read instead of skipping it. A subfolder it can't list is skipped without a word (a
  volume root holds system folders nobody can list), so a dump behind one goes missing silently; choosing
  the folder that holds the dumps, not a drive above it, keeps every listing failure on screen. Every
  dump it finds is listed by title; only Giants is offered (the Native table).
- The setup flow (AA.15). With no folder stored, the TV's Setup screen opens a native folder picker by
  itself after 1 s (`rfd` 0.17.2, a sheet on the window awaited from a Bevy task, so frames keep running),
  and Enter, Space or a click opens it again. A pick stores the folder whether it holds Giants or not and
  replaces the server's game list in place (the phone sees Giants without a restart). With Giants the
  TV shows Found, every dump by title, for 8 s or until a click, then the join code. Without it,
  NoGiants lists what it found plus any `Couldn't list` / `Couldn't read` lines and offers the picker
  again. Later starts skip Setup and rescan the stored folders: the join code while they still hold
  Giants, NoGiants when they don't (a no-Giants pick, an unmounted drive).
- The fallbacks, for a picker that won't show (whether it opens above the fullscreen window is AA.11's
  question). `guacamole --setup` asks with a plain dialog on the main thread before the window exists,
  stores the pick and starts on Found, or NoGiants when the pick has no Giants (a cancel keeps the stored
  folder). `guacamole --setup <folder>...` stores without any picker, prints each dump by title, whether
  Giants is ready and whether the dev `GUAC_DUMP_DIRS` overrides it, then exits (1 when a folder can't be
  stored, nothing stored). That report goes to stdout only; the log gets the dump count and whether
  Giants is there. A Windows release build has no console, so there the per-dump lines go nowhere (the
  bring-up decides whether to attach the parent console).
  Nobody has looked at either dialog yet (the display was locked through AA.15); that's AA.17.

## The six games

Three engine families. The surprise is Spyro's Adventure: the PS3 build is XPEC's port on their own
engine, not Toys for Bob's Alchemy. Measured by reading the discs (archive and IGZ headers, magic bytes,
PARAM.SFO), not taken from the wiki.

| Game | Engine | Container | Logic | Meshes | Physics / SPU | Audio | Video |
|---|---|---|---|---|---|---|---|
| Spyro's Adventure BLUS30779 | XPEC "Lux" port | loose `.lxb/.str/.apk/.lsa/.fnt` | Lua 5.1 (138 plaintext files) + native + `.lxb` trigger graphs | `.str`, encoding unknown | no Havok; SPURS jobs of unknown purpose | Wwise | Bink 1 |
| Giants BLUS30968 | Alchemy + TfbTool | IGA v0x08 (LE header, LZMA), IGZ v6 | native + tfbScript op graphs | `igPS3EdgeGeometry` | Havok 2011.2, probably on PPU; 20 SPU ELF images, all FMOD, plus Sony Edge's SPURS jobs inside the EBOOT | FSB5 MPEG | Bink 1 `.mov` |
| Trap Team BLUS31442 | Alchemy + TfbTool | IGA 0x0B LE, IGZ v8 | native + tfbScript | Edge | Havok 2013.1; FMOD on SPU | FSB5 | Bink 2 |
| Swap Force BLUS31076 | Alchemy + VV Laboratory | IGA 0x0A BE `.pak` (zlib), IGZ v7 | VVL (VV's .NET-like IL) | Edge | Havok 2012.1 + `.hkb` behavior graphs; 33 SPU images incl. Havok | FSB5 | Bink 2 (+ a few Bink 1) |
| SuperChargers BLUS31545 | Alchemy + VV Laboratory | IGA 0x0B BE `.pak` (LZMA), IGZ v9 | VVL | Edge | Havok 2014.1; 31 SPU images | FSB5 | Bink 2 |
| Imaginators BLUS31600 | Alchemy + VV Laboratory | IGA 0x0B BE `.pak` (LZMA), IGZ v9 | VVL | Edge | Havok 2014.1; 31 SPU images | FSB5 | Bink 2 |

### Giants in detail

- Disc: 964 files, 18 GiB. 294 `.arc` (11.2 GiB) and 599 `.bld` (3.7 GiB), all IGA; 61 `.mov`.
- IGA v0x08: a 0x34-byte header that is LITTLE-endian even on PS3, 12-byte file records
  `{offset, size, blockWord}`. `blockWord` 0xFFFFFFFF means stored, otherwise the top nibble is the codec
  (2 = LZMA) and the rest is the first slot in one of three block tables, picked by entry size: u8 small up
  to 0x7F sectors, u16 medium up to 0x7FFF, u32 large past that. Data comes in 0x8000-byte chunks and an
  entry of n chunks uses n+1 slots: slot j is chunk j's start in sectors from the entry's offset, with the
  table's top bit set when the chunk is compressed, and slot j+1 is its end. A compressed chunk is a u16
  packed length + 5 bytes of LZMA props + raw LZMA1; a chunk LZMA would grow is stored raw (1,384 on
  Giants), so NOT every chunk of an LZMA entry is LZMA. Header flags are 0 on all 893 archives. `.arc` entries are stored, `.bld` entries are LZMA. A ~150-line reader opened every archive
  on all five Alchemy discs (16,310 of 16,310, 0 errors) and 522 random compressed entries decompressed to
  exactly their declared size. Cross-checked against [igArchiveLib](https://github.com/LG-RZ/igArchiveLib)
  and [igArchiveExtractor](https://github.com/NefariousTechSupport/igArchiveExtractor).
- Inner content: 85,310 entries, 17.56 GiB decompressed, of which 9.8 GiB is duplicate by name (every
  level and character bundle is self-contained). 79,509 FSB5 voice lines, 599 `level.bld` (one IGZ each),
  1,542 texture IGZ, 3,660 language packs.
- tfbScript: character behavior (attacks, upgrades, projectiles) is serialized as op graphs inside IGZ
  (`ScriptSet`, `OpCodeList`, `OpUseMacro`, `OpMacroInterface`, `OpMacroParameter`, `OpCheckValue`,
  `OpDefineMacro`, `OpFlowMacro`, `OpFlowBuiltInBehavior`, `RHSValueStack`). The structure is documented
  for the Wii build by
  [Skylanders Giants in Spyro's Adventure (Wii)](https://github.com/xirba13/Skylanders-Giants-in-SSA)
  (`xirba13`, one commit, 2026-10-06), a tools-only Python project that ports the
  eight Giants plus four variants into Wii SSA by rewriting Giants' IGZ v6 files into SSA's v5. Its
  `docs/FORMATS.md` ("Character scripts") is the write-up. What it documents: a script is an `OpCodeList`
  whose ops carry a control word counting the ops in their block; op objects are SHARED across positions
  (one `OpFlowBuiltInBehavior` serves 326 positions in Tree Rex's script); a macro call is `OpUseMacro` →
  `OpMacroInterface` → one `OpMacroParameter` per argument → `OpFlowMacro`; expressions are binary nodes
  (+ - * /); comparisons are codes 0-5; an else is an `OpAbstractFlow` jump; engine members are named by
  FNV-1a hashes. Every offset it gives is for the PAL Wii discs it pins by `main.dol` hash (SSA `SSPP52`
  revision 2, Giants `SKYZ52` and `SKYP52`; other regions and revisions untried, by its own account), so
  nothing about BLUS30968's layout is verified. Some of its helpers read a Wii SSA decompilation nobody
  could find (Open question 2). Other tfbScript reading aids, none checked against Giants:
  SkydraHeavy's C# class stubs (`OpUseMacro.cs` and kin) and two write-ups for Madagascar 2, an older Toys
  for Bob game ([mad2projects](https://github.com/unix-supremacist/mad2projects) `docs/SCRIPT_FORMAT.md`,
  [madagascar-game-asset-tools](https://github.com/MaxStache/madagascar-game-asset-tools)
  `packages/tfbscript/FORMAT.md`). The VM is the documented half; the native built-ins and engine members
  behind it aren't, and they're where the reverse engineering is.
  MEASURED on BLUS30968 (2026-10-18, a read-only census over all 5,801 IGZ through `guac-formats`, 0
  errors; tool and tables in the research dir, outside the repo): every script offset the Wii project
  gives holds on PS3, and the EBOOT's registry names the fields behind them (`_branchPC` is its control
  word). 14,207 scripts (one `tfbScriptInfo` → one `OpCodeList` each, named by its `.ai` source path),
  4,617,339 op positions over 2,596,478 shared op objects, 44 of the 61 registered op classes on disc
  (the debug ones, `OpPrint`/`OpComment`/`OpBlock`/`OpIfElse`/`OpFlow`, never appear). Every script runs
  on at least one actor through `Placement._script` (34,781 actors). `OpFlowBuiltInBehavior` turned out to
  be the `flow continue` block terminator, one object per scripted file, so counting it measures nothing
  native. The real built-in surface is the ENGINE MEMBERS: named get/set/reset properties the EBOOT
  registers (592 across 67 classes, each with a measurement type, units and a min/max), imported by
  (FNV-1a of the lowercased member name, FNV-1a of the namespace); the disc uses 414 of them in 66
  namespaces, 553,024 references, UI sprites alone 18%. Most game logic is script macros
  (`global.bld` carries 1,080 `OpDefineMacro`, the disc 212,525 `OpUseMacro` calls), not native code.
  Control flow is a program counter over the op array with relative forward skips, and 21,774 else-if
  skips cross the end of their enclosing block, so the VM steps a PC rather than walking a tree. The phone
  portal plugs in at the `redoctane*` members.
- Audio: FSB5 v1, one sample per bank, MPEG, 44.1 kHz for 97% of them. There are no FMOD event files on
  disc, so triggering lives in engine objects (`tfbSoundDataInfo`) and script macros ("Sound - VO
  Request").
- Video: 61 Bink 1 `.mov` at 1280×720: 48 per-character intros named by character (`eruptor.mov`), one
  per playable character; 11 FMV (`fmv01`-`fmv12`, no `fmv05`); 2 logos. Ten carry 13 audio tracks
  (per-language voice), the rest carry one. FFmpeg decodes Bink 1 (probed on a Giants file).
- EBOOT: an encrypted SELF, 5.65 MiB of `.text` (~1.48M PPU instructions), 277 imports from 22 libraries.
  Decrypted, Ghidra counts 20,511 functions (RE toolchain). Stripped, but RTTI typeinfo strings, 115
  `*MetaField` reflection names, engine API names (`igPlatformVisualContext::drawEdgeGeometry`) and the
  reflection registry's static tables (989 types, below) survive. Sony Edge's SPU jobs ride inside it as
  SPURS job binaries, not ELFs: `EdgeGeometry` (its compress and decompress sources named in its strings),
  then `ShaderPatching` and `IntraMeshSorting`.
- Figure → asset: character archives are keyed by DECIMAL toy-type id, `character/009_eruptor.arc` ↔
  toy type 9, with variant suffixes (`_legendary`, `_alt`, `_dark`, `_red`). A placed figure resolves to
  its archive by filename, except where a toy type has no archive of its own number (Dark Spyro, toy 28,
  rides `016_spyro_dark`). `character/` holds 227 files: 68 character `.arc`/`.bld` pairs, the card-game
  archives and a few UI and puzzle bundles.
- Roster: 48 playable characters by the intro count (one intro each), plus variants. Giants also plays
  SSA-era figures (origin-game-and-later rule, `crates/core/src/compat.rs:48`). The disc's roster is the
  catalogue the phone offers (Figures).

#### IGZ v6 (AC.5)

Verdict: every IGZ on the disc parses with nothing guessed. `guac_formats::igz`
([`crates/guac-formats/src/igz.rs`](../../crates/guac-formats/src/igz.rs)) reads Eruptor's 29 (`009_eruptor.bld`
+ `.arc`), all 158 character `level.bld` (512,595 objects) and `permanent/global.bld`'s 13 with 0 failures,
and the tfbScript census read all 5,801 through it.

- Big-endian, 32-bit pointers. `IGZ\x01` + version 6, then a section table from 0x10: 16-byte entries
  (offset, size, alignment, memory pool) ended by a zero offset, where a placeholder section (size
  0xFFFFFFFF) has no bytes. Section 0 holds the fixups: magic and version again, the first record's offset
  at +0xC, the record count at +0x10, then records of (kind, 0, 0, count, length, data start). v6 numbers
  its kinds where v7+ uses 4CC tags.
- A location is `(data section << 24) | offset`, data section 0 being the one after the fixups. Location
  lists are nibble varints, low nibble first (3 data bits plus a continue bit), each value v stepping
  v × 4 + 4 bytes.
- An object opens with its TMET index (the vtable word, in a file) and its reference count as written (1
  or 2). RVTB lists the objects and MTSZ is the step to the next one.
- A memory ref is 8 bytes, a size word then a pointer. Present: bit 31 set, the size word unmarked and the
  pointer marked kind 6, the byte count in bits 0-26 and the alignment 4 << bits 27-30 (4, 16 and 128
  seen). Null: bit 31 clear and kind 0xF on the size word, which isn't always 0 (`global.bld` has 3,022
  null refs at 0x7 and 324 at 0x10000007). Anything else is refused on read. A probe over every memory-ref
  and vector field the registry declares found 234,179 present refs, all with unmarked size words, and
  86,668 null ones, all marked 0xF. A vector is a count plus a memory ref (12 bytes).
- The runtime rewrites kind-0xF words (AC.14, from the June savestate): an untouched null ref's size word
  reads 0x11 in memory where the file has 0 (0x10000011 for 0x10000000), while present size words keep bit
  31. What the 0x11 means is open, and nothing we read depends on it.
- The loader works IN PLACE: the tutorial's `level.bld` data section (70,834 objects) sits in guest memory
  at the file's RVTB offsets, nothing copied or re-stepped.
- External refs by hash are FNV-1a32 (namespace `metaimages` = 0x843CD0C2, brute-forced).

Every fixup kind on the disc, with AC.5's census as entries in files (blank: not broken out for the
`level.bld` run):

| Kind | Name | What it is | Eruptor's 29 IGZ | 158 character `level.bld` |
|---|---|---|---|---|
| 0 | TMET | type names | 247 in 29 | |
| 1 | TSTR | the names EXNM indexes | 28 in 1 | |
| 2 | EXID | imports by hash, (object, namespace) FNV-1a32 pairs | 175 in 17 | |
| 3 | EXNM | imports by name, (namespace, name) TSTR indexes | 27 in 1 | |
| 5 | RVTB | the objects, a location list | 6,769 in 29 | 512,595 |
| 6 | | pointer words (object refs, memory-ref data) | 14,758 in 29 | 1,237,825 |
| 7 | | import words, `0x80000000 \| EXID index` | 676 in 17 | 54,493 |
| 8 | | unknown, below | 29 in 29 | 158 in 158 |
| 0xA | TMHN | memory handle blocks, (size word, location) | 60 in 17 | |
| 0xB | | memory handle words, a TMHN index | 60 in 17 | |
| 0xC | MTSZ | instance size per TMET entry | 247 in 29 | |
| 0xE | RSTR | string pointer words | 4,960 in 13 | 367,218 |
| 0xF | | size words of NULL memory refs | 922 in 13 | 47,681 |
| 0x10 | | named external refs, `0x80000000 \| EXNM index` | 3,103 in 1 (1,742 unflagged) | 262,747 (146,754 unflagged) |

- Kinds 4, 9 and 0xD never appear.
- Kind 8 stays OPEN. Giants writes it in every IGZ (5,801 of 5,801), always count 1 and one big-endian word,
  0x00000004, which equals the first object's location (always 4), so a location and a constant can't be
  told apart on this disc. It isn't a nibble list either: that reading needs a non-zero padding byte, and
  none of 8,865 real lists has one. Nothing reads it; the parser keeps it raw and counts it.
- Kind 0x10 is named external refs through EXNM: metaobjects (namespace `metaobject`) plus 208 named
  object imports in the character `level.bld` (148 SpriteInfo, 16 StringInfo, 12 ActorInfo, 12 LightInfo,
  10 ScriptStructure, 7 ValueInfo, 3 ParticleInfo). A word without the 0x80000000 flag (script types only)
  stays unresolved (`Ref::UnresolvedMeta`); no gate type uses kind 0x10, so it went no further.
- The parser refuses instead of guessing: a non-zero word no fixup marks, a word two kinds mark, any marker
  on an object's type word, a dangling pointer or one into a placeholder section, an out-of-range import,
  handle or EXNM index, and a null ref with a pointer.

#### Field layouts: the static registry (AC.4, AC.14)

Verdict: the files don't carry field layouts, and recovering them needed neither a memory dump nor
statistics. The EBOOT registers every type from STATIC tables, readable straight out of the decrypted ELF,
and the live registry agrees field for field.

- How a type registers: one call per class with (meta slot, the parent's registration descriptor,
  constructor, name, instance size, a field-registration descriptor). The field function appends a static
  array of field-kind descriptors, then sets names, per-field key slots and u16 offsets from three more
  static arrays. A constant-propagating scanner (RE toolchain) reads every such call in 0.8 s: 989 types,
  609 with fields of their own. The per-field details (an object ref's target type, a memory ref's element
  kind, array counts, bitfields, defaults) were read in Ghidra for the gate's types only.
- Checked against the disc (AC.4): all 163 types the 158 character `level.bld` use are registered, 158 with
  MTSZ equal to the registry size, and 1,346,793 reference fixups over 512,595 objects land on a reference
  field the registry declares, 0 misses.
- Confirmed live (AC.14), off the June savestate's memory read through the reflection system's own
  registered layouts: 980 of 989 types are live (9 register lazily, none a gate type) and all 980 agree on
  name, size, parent, field count and order. Own fields agree on 979 (the odd one,
  `tfbSoundInfo._categoryInfo`, is a static the scanner labelled by its storage kind). The gate's 35
  readers make 272 field reads and 272 of 272 match live offset, size and kind. No Rust constant disagrees.
- AC.4's open question 2 is CLOSED. Five types are 4 bytes bigger in every file's MTSZ than registered
  (`igAlphaStateAttr`, `igBlendStateAttr` and `igDepthWriteStateAttr` 0x14 vs 0x10, `igCullFaceAttr` 0x18 vs
  0x14, `RHSReferenceStack` 0x30 vs 0x2C). Since the loader works in place, the tail sits in memory exactly
  as on disc (3,768 of 3,768 objects), no fixup marks it and nothing reads it: step objects by the file's
  MTSZ, read fields by the registry. None is a gate type.
- In Rust: `guac_formats::igz::layouts`
  ([`crates/guac-formats/src/igz/layouts.rs`](../../crates/guac-formats/src/igz/layouts.rs)), tagged
  `IGZ v6 / BLUS30968 01.00`, for the gate's types only. A reader refuses another type, an MTSZ size off the
  registry and a fixup on a word its layout doesn't read as a reference.
- How a hero hangs together, which is where AE starts: `tfbPhysicsModel` → `_tfbBody` (a `tfbPhysicsBody`
  whose `_data` is the Havok skeleton packfile) → `_node` (the `igActor2` scene graph) →
  `igBlendMatrixSelect` palettes → `tfbRuntimeTechniqueInstance` → `igEdgeGeometryAttr` → the
  `igPS3EdgeGeometry` segments. Beside it `_combinerPrototype` → `igSkeleton2` (the Alchemy skeleton and the
  inverse joints the palette indexes), and `_animationSet` → `AnimationInfo` → `tfbPhysicsAnimation`, whose
  `_data` is a Havok clip or a `0xDEADBEEF` lip-sync block. Eruptor: 34 `igSkeleton2` bones, 33 inverse
  joints, 2 meshes over 3 segments, 44 `igImage2` in `level.bld` (the `.arc` beside it adds 16
  upgrade-screen textures, 60 in all), a 185-slot clip table with 32 filled.

#### Textures (AC.6)

Verdict: `_tile` is RSX's swizzle, and all 4,527 character `igImage2` convert to KTX2 with their original
blocks (`guac_formats::texture`, [`crates/guac-formats/src/texture.rs`](../../crates/guac-formats/src/texture.rs):
our own writer, BC1_RGBA_UNORM, BC3_UNORM and R8_UNORM with an `rrr1` swizzle).

- Five metaimages across the character archives: `dxt5_tile_big_ps3` 2,158, `dxt1_tile_big_ps3` 1,283,
  `dxt5_big_ps3` 1,056, `dxt1_big_ps3` 20 and `l8_tile_big_ps3` 10, all depth 1 with one image. DXT maps 1:1
  onto BC1 (RSX honours DXT1's one-bit alpha) and BC3.
- The bytes decided `_tile` (the EBOOT's format table only pairs each canonical format with a `_big_ps3`
  and a `_tile_big_ps3` name). Every tiled image is power-of-two (3,451 of 3,451) with its levels packed and
  no row padding, every linear one is single-level and not power-of-two (1,076 of 1,076, a tight pitch),
  and the pixel block is the mip chain rounded up to 128 bytes on all 4,527. That's RSX's swizzled layout
  vs its pitched one.
- RSX ignores the swizzle for DXT, so tiled DXT blocks still run in rows (smoother as rows than as Morton in
  2,913 of 3,431 images). L8 under the swizzle IS Morton-ordered: all 10 `l8_tile` come out as two
  identical rows after the unswizzle and not as stored.
- Blocks are already in BC byte order, so the endian fix is the identity: read little-endian, 18,101,440
  of 18,101,440 DXT5 colour blocks with distinct endpoints are four-colour.
- Confirmed in the June savestate's RSX state: 1,092 resident `igImage2` are byte-identical to their file
  blocks and equal level for level after our untile, and 22 texture-unit states over 18 `_tile` DXT images
  (seen across the ring's draws, on units 0 and 1 only) show the linear bit CLEAR (swizzled) with kind,
  size and mip count matching. Not exercised yet: a linear image's pitch, a bound L8 and the hero's own 60
  (AC.15).
- Which unit is which (AC.10): `tfbRuntimeTechniqueInstance._materialPass` lists `tfbAlphaTextureUnitIDAttr`
  binds and `igAttr._cachedUnitID` is the unit. All 2,243 character techniques bind unit 0 exactly once;
  colour sits at 0, `_n`/`_nrm` only at 4 and `_s`/`_sp` only at 6, and Eruptor's rim map at 3.

#### Edge meshes, and where skinning runs (AC.7)

Verdict: the SPU skins the heroes, a strong inference no skinned draw has confirmed yet. Edge
decompression and skinning run in the `EdgeGeometry` SPURS job inside the EBOOT (which the 20-image SPU
census missed, it only counted ELFs), whose scratch holds the live skin palettes (below).
`tfbmulti2.fx`, which draws 2,201 of the 2,243 character techniques, sets its skinning bit on none of its
5,120 compiled vertex-shader variants (per the AE materials research). The other 42 use `tfbwarp.fx`,
whose 2 skinning variants of 4 (like `tfbshadow.fx`'s) take an `ig_matrix_blending` constant, so RSX
skinning does exist in the shader set (whether any of those 42 draw skinned segments is unchecked; the SPU
config of the segments under each technique says). The savestate only caught unskinned draws, so the SPU's
skinned output stays unobserved until AC.13 records posed vs bind on Eruptor's draw. Our decoder replaces
that job; nothing in it needed decompiling. `guac_formats::edge`
([`crates/guac-formats/src/edge.rs`](../../crates/guac-formats/src/edge.rs)).

- Two SPU configs on the characters, unskinned (1,897 segments) and skinned (424); anything else is refused.
  The rest of the disc holds 4 segments on other configs, left for the level reader that needs them.
- Index compression, in our words: an 8-byte header (explicit count, bias, first-use bytes, a bit width
  0-12), one first-use bit per consumed index (the next unseen vertex or the next explicit one), a 2-bit
  code per triangle saying which edge it shares with the previous triangle (or three new indexes), and
  explicit indexes delta-coded across 8 interleaved lanes under the bias. Every explicit index on the
  characters is a back-reference (368,718 of 368,718).
- Streams: an 8-byte header plus 8-byte blocks, where the format code is RSX's own vertex type and the slot
  is the RSX input register, so each descriptor predicts a draw's vertex array. The disc uses F32, F16, U8N
  and X11Y11Z10N (RSX's CMP word, big-endian, two's complement, read as signed normalized).
- Skin: 4 × (u8 weight, u8 local matrix) per vertex, summing to 255 on all 262,899 skinned vertices. Locals
  land in two DMA ranges of 48-byte 3×4 matrices, then palette slot → `_blendMatrixIndices` → the bone with
  that `_blendMatrixIndex`.
- Disc: 2,321 of 2,321 character segments decode (624,950 vertices, 698,924 triangles, 0 degenerate) and
  every bound group's union equals its authored `igAABox` (2,005 of 2,005, gap 0). Eruptor is 3 skinned
  segments, 2,861 vertices and 4,812 triangles, bounds (-1.9696, -0.0017, -0.6243)..(1.9696, 2.1768,
  0.5144) exactly. The whole disc: 50,262 decode, the same 4 refused.
- The June savestate, on non-hero data: 708 indexed draws over 12 unskinned `global.bld` segments, and
  every drawn triangle is in our decode in the same order and vertex rotation (708 of 708). Indexes stay
  segment-local u16, a segment is one draw split into batches of up to 256 indexes, RSX reads unskinned F32
  positions in place from `_spuVertexes0` (difference exactly 0), F16 UVs and U8N colour are exact.
- Edge culling is ON: `igPS3EdgeManager._cullingState` 1 and both `_disableStaticCulling` and
  `_disableSkinnedCulling` 0 in the live state, and 10 of the 12 segments draw fewer triangles than they
  hold (96 → 54, 240 → 202). So a capture holds an ordered SUBSET of the decode, and AC's index tolerance
  ("each draw's triangle set is equal to ours", decision 5 in
  [`ac-giants-decode-gate/design.md`](../../openspec/changes/ac-giants-decode-gate/design.md)) needs that
  wording or the two flags poked to 1 in guest memory first (chotchki's call, AC.16).
- Palettes (the savestate's 43 skinned jobs over 8 NPC models): blend-index (`_inverseJointArray`) order,
  48-byte row-major 3×4, MODEL space with a separate local-to-world per job, the two DMA sources at
  `_skinMatricesByteOffsets[0]` and `[1]`, bones mapped by name.
- Still for the hero's own draw (AC.16): the skinned output config, CMP normals (our signed normalize and
  RPCS3's differ by up to 7.7e-4 rad, inside the 1e-3 tolerance with little margin) and Eruptor's 3
  segments.
- Reading aids, nothing copied: NoesisPlugins' `fmt_alchemy_igz.py`
  ([NoesisPlugins](https://github.com/NefariousTechSupport/NoesisPlugins)) for field order and the
  attribute tables (its unsigned CMP decode doesn't fit this data), the index layout as SkydraHeavy's port
  of chrrox's decompressor describes it ([SkydraHeavy](https://github.com/spyrosadventure/SkydraHeavy)) and
  RPCS3's own vertex-type tables. igCauldron's `EdgeStuff.cs` wasn't needed.

#### Havok skeletons and clips (AC.8)

Verdict: one compression on the whole disc, decoded. `guac_formats::havok`
([`crates/guac-formats/src/havok.rs`](../../crates/guac-formats/src/havok.rs)), written from our own notes and
[Kovah](https://github.com/NefariousTechSupport/Kovah)'s Apache-2.0 2011.2 offsets (dumped from Giants on
Wii); [HavokLib](https://github.com/PredatorCZ/HavokLib) is GPL-3.0 and was only read.

- Packfiles ride inside `level.bld` as `tfbBuildDataInfo._data` (271 in one arena file): big-endian,
  `hk_2011.2.0-r1`, file version 9, layout rules 4/0/1/1 (4-byte pointers, big-endian, reuse padding,
  empty base), three sections, local, global and virtual fixups, classes pinned by signature.
- Clips are `hkaSplineCompressedAnimation` only, mask byte 0x45 on all 108,527 tracks (16-bit positions and
  scales, THREECOMP40 rotations), one block per clip (nothing past 256 frames), B-splines of degree 1 or 3
  over u8 knots clamped at both ends.
- THREECOMP40 is a 40-bit LITTLE-endian word: three 12-bit components scaled (v - 2047) / (2047 √2), two
  bits naming the dropped component and one its sign, the dropped one rebuilt as the largest (it is, on all
  1,576,123 quaternions; read big-endian, 102 of the idle's 391 break). The spline data is little-endian
  inside the big-endian packfile (`endian` 0 on all 3,920 clips).
- Census, every character archive: 120 skeletons, 3,920 clips and 12 empty containers parse, 0 failures;
  30 fps on 3,822 clips, 24 fps on 96. 262 clips carry the ADDITIVE blend hint; how the game layers them
  waits for an oracle comparison on one (the hero's idle doesn't use it).
- Eruptor: `Eruptor.ma` is one 33-bone skeleton, named `J_root` after its root bone. `igSkeleton2` has the
  same 33 names and parents plus an `Unnamed bone` root (34), in a different order (so bones map by name;
  the hero has no `_boneMappingTable`). `Eruptor_Idle.ma` is 1.3333 s, 41 frames at 30 fps, 33 tracks. The
  loop closes to 1.12e-8 and the ankles stay within 0.0047 while the root bobs 0.14, which needs the
  hierarchy, the rotations and the root translation right at once.
- On live NPC palettes (the savestate), solving the palette against every clip with our decode lands 12 of
  18 instances on one sharp minimum within 9.4e-7 translation and 4.8e-7 rad (6 Bird_Idle, 2
  BoxTortoise_Idle at 1.1710 s and 1.0225 s, 2 FlyingFlocker_Flying, Butterfly_Fly, Cannon_Idle). The other
  6 are blends or layered poses (4 goats 0.034 off their idle, a tortoise 0.152 off look-right, Mabu 0.275
  off a run).
- None of the 20 SPU ELF images is Havok (all FMOD) and the SPURS jobs found so far are Edge's, so Havok
  PROBABLY runs on the PPU and physics parity needs no SPU decompilation. Not proven: the EBOOT imports
  `cellSpursJq` (12 functions), so other non-ELF jobs could exist; following those imports' callers in
  Ghidra settles it before AG leans on it.

Which bind the game skins with is OPEN, for AC.17. Eruptor's Havok reference pose and Alchemy inverse
joints agree on 28 of 33 bones within 1e-4, and `J_wrist_R` plus its 4 children differ by a 20.80° turn
about the wrist's local +Y (107 of the 120 skeletons agree on every bone, 13 don't). Two lines of evidence
point opposite ways:

- The mirror argument, for Havok's bind. The stored mesh mirrors itself in x to 1e-4 (median 0), Havok's
  joints mirror on all 10 L/R pairs (within 3e-6) and Alchemy's break only at the right wrist and palm.
  The 39 vertices `J_palm_R` dominates centre 0.148 from Havok's joint and 0.349 from Alchemy's (the left
  palm's 53 sit 0.034 from theirs), and skinned with Alchemy's bind at the reference pose the right hand moves
  up to 0.239 (5.1% of the 4.643 `igAABox` diagonal) and sits off the mirror by up to 0.138. Weakening
  it: the skin weights themselves aren't mirror-symmetric (the palm counts above, the wrists 138 vs 132).
- The savestate's statistical lean, for Alchemy's. On the live NPC palettes, where the two binds differ by
  more than 7e-7 the Alchemy bind fits 9 of 9 (23 of 29 above 3e-7). Not decisive: no resident skeleton's
  binds differ by more than 1e-6.

Eruptor's own palette settles it (AC.17). Until then the working assumption (palette = Havok model pose ×
Alchemy inverse joint) is `hero_view`'s default, and `--bind havok` draws the other one.

### What carries past Giants

- Into Trap Team, nearly everything: 147 of 175 sampled Giants IGZ type names also appear in Trap Team,
  79 of them script or `tfb*` types. IGA moves to 0x0B (a u64 name-table offset), IGZ to v8 (4CC
  fixups), Havok to 2013.1 and the tfbScript VM carries. A Trap Team reflection schema (metaobjects) is
  public in [ig-workshop-tests](https://github.com/spyrosadventure/ig-workshop-tests), dumped from TT 1.1.0
  (our disc is 1.00). Bink 2 is new.
- Into Swap Force, SuperChargers and Imaginators: the container, IGZ, `igImage2`, Edge and Havok-reader
  layers only (30, 25 and 18 shared type names). Their logic is VVL, a different VM (igCauldron's
  `VvlToDll` documents VV's non-standard opcodes). [igCauldron](https://github.com/NefariousTechSupport/igRewrite8)
  ships full SC 1.6.x and Imaginators 1.1.0 schemas (8,390 metaobjects for Imaginators), again later
  builds than our 1.00 discs.
- Spyro's Adventure (PS3): nothing at the format level, XPEC's Lux shares no formats with Alchemy. The
  Bevy runtime, the extractor framework, the oracle and the method carry. (TfB's own SSA is the Wii
  Alchemy build with IGZ v5 and tfbScript; it isn't on the shelf.)
- Bink 2, for every later game: FFmpeg can't decode it (probed: "Bink 2 video is not implemented");
  [NihAV](https://nihav.org) and [librempeg](https://github.com/librempeg/librempeg) have decoders, NihAV's
  author says with imperfect reconstruction (no in-loop deblock). That's every later game's M0 and its
  cutscenes.

## Why Giants first

Scored on chotchki's own criterion, risk and leverage, then confirmed by him (quote in Decisions).

| Lens | Spyro's Adventure (PS3, XPEC) | Giants | Winner |
|---|---|---|---|
| Oracle readiness | never registered in RPCS3 on the Mac (not in `games.yml`, no cache, no savedata); compat "Ingame" | in `games.yml`, PPU AOT cache, 155 FP / 62 VP shader cache, savedata; compat "Playable"; ~60 fps SPU-LLVM on the M3 Max on the v1.9.19 build (`09d602fd5`); the product drives it on the HTPC | Giants |
| Mesh and animation decode | `.str`: no public decoder, not plain float32 | public references for v6 Edge and Havok 2011.2 | Giants |
| Logic as data | over 95% native C++; Lua is per-actor glue (~7.6k code lines, 65% of it hub story) | attacks and upgrades are tfbScript data a VM can run | Giants |
| SPU code | SPURS jobs of unknown purpose (solver/dynamics strings) | every SPU ELF image is FMOD; the SPURS jobs inside the EBOOT found so far are Sony Edge's, whose geometry format we decode instead of its code (Giants in detail) | Giants |
| Carry-over | none | strong into Trap Team, readers into the VV games | Giants |
| Closed middleware | none in gameplay | Havok-tuned physics feel, FMOD | SSA |
| Field names | self-describing `.lxb` (CRC32 name tables) | reflection needed | SSA |
| Roster | 32 | 48 | SSA |

Giants wins the three lenses that can sink the project (oracle, decode, logic as data) plus carry-over.
SSA's wins are real, but none of them can sink it the way a missing oracle can. For the kids it's also the
better game one: it plays their SSA-era figures too, ships 48 intros (M0 needs only IGA and Bink 1, both
already read) and already runs here.

What flips it: AC's decoded hero doesn't match its RSX capture. Then AC proposes Trap Team (same TfbTool
island, reflection schema already public) at the price of Bink 2 for its M0, and chotchki decides. What
the kids play most is unknown, and it matters less now that the phone offers the whole roster: it would
reorder which figures and levels get attention first, not the game. The only signal is on the HTPC,
where `figure_usage` holds a last-used time per (profile, figure) with no counts (the Mac's table is
empty, it's a dev box) and the product's own figure stats carry playtime. Looking at those is one-off
research on the product's data; nothing from it reaches Guacamole.

## Method

The spec sits between what the oracle shows and what we build, and the original script data runs as
shipped instead of being rewritten.

1. Observe in the oracle: run the original in the patched RPCS3 and capture what it does (input → state
   traces, RSX frames, portal traffic, script events). The decompiler is a reading aid for a mechanism
   the oracle already showed, never a source of code.
2. Write the behavior down in our own words as an OpenSpec spec, through a change's delta into
   `openspec/specs/<capability>/spec.md`: SHALL/WHEN/THEN about what the game observably does, never
   internal names (`openspec/config.yaml`'s specs rule).
3. Implement from the spec. Tests come from the spec (unit, integration) and from the oracle
   (tolerance-based comparisons, generated locally, never committed).
4. Run the original script data instead of rewriting it: a tfbScript VM over the user's IGZ. Fixes ship
   as our own code in the VM or its built-ins, never as edits to the script data.

Why it's shaped this way:

- The spec is the arbiter when the oracle and Guacamole disagree. It says which side is wrong and it's
  reviewable without Ghidra open. A transliteration of PPU code would drag SPU, Edge and RSX
  assumptions into a Bevy engine and leave nothing to review.
- Running the script data means 48 characters' moves come from one VM plus its built-ins, so a bug fixed
  in a built-in is fixed for every character. Rewriting each moveset by hand is 48 chances to drift.
- Shaders follow the same path: the shading model goes into a spec as equations, gets written fresh in
  WESL and is checked with [FLIP](https://github.com/NVlabs/flip) against RSX captures. Ported microcode
  (with the PS3's shader patching baked in) can't grow an Enhanced mode.
- The nearest prior shape is [OpenMW](https://openmw.org) (the original data and scripts on a
  from-scratch engine), not [OpenGOAL](https://opengoal.dev) (decompiles and recompiles the original code)
  or [Unleashed Recompiled](https://github.com/hedge-dev/UnleashedRecomp) (static recompilation).

## RE toolchain

| Job | Tool | Notes |
|---|---|---|
| Decrypt the EBOOT | `rpcs3 --decrypt <copy>` (`vendor/rpcs3/rpcs3/rpcs3.cpp:392,835`) | Writes NEXT TO its input (`EBOOT.BIN` → `EBOOT.elf`), so only ever on a copy outside both the disc tree and the repo. RPCS3 has keys for all six EBOOTs. Giants' ran through the in-tree build under a throwaway HOME, with TMPDIR outside it (RPCS3 refuses a config dir under its temp dir), and came out bit-identical to an earlier decrypt. |
| Static analysis | [Ghidra](https://github.com/NationalSecurityAgency/ghidra) 12.1.4 + [clienthax/Ps3GhidraScripts](https://github.com/clienthax/Ps3GhidraScripts) (`d7cb6fd3`) | Language `PowerPC:BE:64:A2ALT-32addr`. The release zip ships NO macOS decompiler (natives for Windows and Linux x86-64 only), so on the Mac its natives get built from source with Ghidra's own `gradlew buildNatives`. The scripts go in as an installed extension (headless only finds `nids.txt` that way) with `AnalyzePs3Binary` as a pre-script, and r2 goes in `<unaffected>` in `ppc_64_32.cspec`, or the decompiler assumes every call clobbers the TOC. Giants loads globals with a single `lwz rX,d(r2)` (13,085 in `.text`, plus 2,493 `lfs` for float constants); only 2 `addis rX,r2` words exist, because the whole TOC sits inside r2's 16-bit displacement. Headless analysis takes 82 s: 20,511 functions, 277 of 277 imports named, `TOC_BASE` 0x00723C00 as one global r2 (all 14,502 `.opd` descriptors agree), 99.2% of TOC operands resolved to references. Known gap: 824 Cell-only vector ops (`lvlx`, `lvrx`, `stvlx`, `stvrx`) have no SLEIGH constructors, so a function holding one decompiles with a hole. The patched Ghidra, the project and every decompile stay in the research dir outside the repo. |
| Reflection registry | a constant-propagating PPC scanner (research dir, Python) | Walks `.text` once, tracking constants into the argument registers, and records the arguments at every call to the four registration entry points: 989 types with parents, sizes and fields in 0.8 s. It reads raw instruction words, so it walks straight through the vector-op holes that stop the decompiler. How AC.4 got the layouts (Giants in detail). |
| SPU code | [aerosoul94/GhidraSPU](https://github.com/aerosoul94/GhidraSPU) | Only if SPU code matters. Probably never for game one (Why Giants first, SPU code). |
| Live debugging | RPCS3's GDB stub | On by default at `127.0.0.1:2345` (`vendor/rpcs3/rpcs3/Emu/system_config.h:379`; the oracle's log shows it listening). PPU only, Z0 software breakpoints only and breakpoints need the PPU interpreter: under LLVM RPCS3 refuses them but the stub still answers OK, so the breakpoint silently never fires. A 0x03 byte pauses, `m` reads (about 600 bytes a packet), `vCont;c` resumes: good for targeted reads, too slow for bulk (PINE, next row). |
| Bulk live reads | RPCS3's PINE server | Off by default; `IPC Server enabled: true` in `ipc.yml` under RPCS3's config dir turns it on (`--config` doesn't cover that file, so an isolated HOME carries its own), and it listens on `$TMPDIR/rpcs3.sock`. 8- to 64-bit reads batched up to 450,000 bytes a reply. No pause of its own, so reads race the game unless a capture or the GDB stub has paused it. |
| Runtime state | Guest memory dump (`vendor/rpcs3/rpcs3/rpcs3qt/guest_memory_dumper.cpp`), or a savestate | The dumper writes a sparse 4 GB file where offset = guest address, plus SPU local stores and a JSON manifest. A savestate is a memory dump too: a parser outside the repo walks its memory section into regions (55 for the June Giants state) and fills the text segment from the decrypted EBOOT, which a state leaves out unless Inspection Mode saved it. AC.14's live-registry check ran on that. |
| Frame ground truth | RSX capture (`.rrc.gz`), read by [`tools/rrc-dump`](../../tools/rrc-dump/FORMAT.md) | Taken with the game window's RSX-capture shortcut into `<rpcs3 config>/captures/` (`vendor/rpcs3/rpcs3/Emu/RSX/RSXThread.cpp:3401`); `--rsx-capture <path>` REPLAYS one, it doesn't take one. Real vertex, index and texture bytes for one frame plus the register state. RPCS3's own serialization (magic `RRC`, version 6), no public spec, so `rrc-dump` carries our write-up of it, pins version 6 and refuses to write inside any work tree of this repo. The oracle section has the recipe. |
| SPU programs | SPU ELF dumper (`vendor/rpcs3/rpcs3/rpcs3qt/elf_memory_dumping_dialog.cpp`) | Carves SPU programs out of local store into ELF. |
| Which files load when | RPCS3 `sys_fs` logging | Opens are logged by default; reads need the trace level. |
| Glue | small Rust or Python scripts | Read inputs from a path, write outputs outside the tree. |

Why not the rea MCP: measured, it can't see the code. It rejects the encrypted SELF ("unsupported binary
format"), rejects the decrypted ELF (its ELF gate knows `EM_PPC` = 20, not `EM_PPC64` = 21) and forcing
ELF32 fails in every provider. Its arm64 path only sees RPCS3's PPU AOT cache objects, which are
block-granular LLVM translations bound to RPCS3's `ppu_thread` ABI (less readable than Ghidra on the
ELF), and the dev Mac had no rea provider installed for them anyway. Drive Ghidra directly (headless plus
scripts or a Ghidra MCP server).

## The oracle

What exists: the product's patched RPCS3 (P1 portal control over AF_UNIX, P4 portal-traffic events, P6
`BUTTON_PRESS`), the Rust IPC client in `crates/rpcs3-control/src/ipc/` and `tools/playthrough`'s
scripted beats. Giants has run on both machines on the `09d602fd5` build v1.9.19 shipped.

Which build (AC.2, 2026-10-18): `09d602fd5`, from a copy. The in-tree pin `accfecd2d` (+P1-P8) dies at
boot on the M3 Max (below). The RPCS3 nested in the installed v1.9.19 app doesn't launch here anymore
either: its bundled `libopencv_imgproc` finds `libopencv_core` only through an rpath into a Homebrew
cellar that's gone (a shipped Mac bug with its own backlog line). The copy adds two `@loader_path` rpaths
and an ad-hoc signature, lives in the research dir outside the repo and reaches playable no-GUI Giants at
54.9 fps over 30 s (`live_launch` passed).

Why `accfecd2d` dies, most likely (read from source, not confirmed by a run): Metal's
`attachmentIndex(220) must be < 8`, right after GPU detection, and the crash report shows only the RSX
thread creating a pipeline (`vk::glsl::program::link` into MoltenVK's `addFragmentOutputToPipeline`).
Upstream `7022a322a` (2026-09-19, inside our pin window) warms the shader interpreter with 2-4 colour
attachments against a render pass that has one, a Vulkan valid-usage violation. MoltenVK 1.4.2
(Homebrew's, not bundled) indexes its attachment table with no bounds check, reads garbage and Metal
asserts on it (which would explain 220, and why it can be intermittent). Upstream fixed it the day after
our pin (`c938d3e049`, `d08d568d5e`), and `Shader Mode: Async Recompiler (multi-threaded)` skips the
interpreter warm-up, so it should dodge the crash without a rebuild. None of the cheap confirmations has
run: one boot in that mode (past the 24 s mark settles cause and workaround together), the log the crash
probably left (`~/Library/Caches/rpcs3/RPCS3.log.gz`, mode 000) or a rebuild with the two fixes. Windows
drivers may tolerate it, untested.
It matters to the product only if the v1.10.0 prerelease ever gets promoted
([`openspec/backlog.md`](../../openspec/backlog.md)), and to AF's pin choice.

The capture recipe (AC.13 as prepared; nothing is captured yet). Everything sits ready in the research
dir, with the exact launch line in `~/workspace/guacamole-research/captures/BLUS30968/09d602fd5/SESSION.md`
(outside the repo):

1. Boot the oracle `--no-gui` with a per-run `--config` (RPCS3's own config file is then never read or
   written): RPCS3's config plus FIFO reordering OFF (so RPCS3 doesn't merge draws, AC.12), `sys_fs: Trace`
   (opens log at the default level anyway; trace adds reads and seeks inside an archive), capture hints off,
   and Compatible plus Inspection Mode savestates (so a savestate carries the executable-backed memory
   too). Add an input config that moves Circle and Left Stick Down off C and S: the keyboard pad sees the
   hotkeys too, so Option+C would also press Circle and Cmd+S push the stick, either of which knocks the
   hero out of idle.
2. Don't set `SKYLANDER_BORDERLESS`: P8 hides the game window on the Mac and publishes a surface
   instead, and the hotkeys live on that window.
3. From a cold boot, mash CROSS over P6 (`BUTTON_PRESS CROSS`) to the "put a Skylander on the Portal of
   Power" prompt, then P1 `LOAD` a COPY of the dev's Eruptor `.sky` (the game writes to it).
4. With Eruptor idle on screen, standing still and silent, upgrades recorded: Option+C on the focused,
   UNLOCKED game window. The next frame end arms the capture, the one after writes
   `<config>/captures/BLUS30968_<datetime>_capture.rrc.gz` and PAUSES the emulator, which is also the
   moment for a consistent memory read; Cmd+P resumes. Two more on visibly different idle poses, then
   Cmd+S for a savestate (the guest memory), then shut down. On a locked screen the keystrokes land in the
   lock screen's password field, which is why the overnight attempt sent none.
5. Restore RPCS3's Giants savedata and trophies from the pre-session backup (the cold-boot nav writes a
   save slot), then `rrc-dump --list` each capture (AC.12).

No unattended trigger exists at either pin: a capture starts only from that hotkey or the main window's
menu, and no config key, CLI flag or home-menu entry does it. AF's smallest fix is a dev-only `RSXCAP` +
`RESUME` (+ `SAVESTATE`) on P1's listener, about 20 lines, which is the scripted-capture verb below.

The savestate route, lighter than a capture for most of this. A savestate taken DURING GAMEPLAY carries
nearly everything a capture gives except render targets: RPCS3 serializes the RSX state and the 2 MiB
command ring lives in guest memory, so a replay is exact. On the June Giants state, replaying the ring
(148,387 commands, 4,833 draws, 59 flips) reproduces the serialized state with 0 mismatches (547
registers, 468 of 468 transform constants, 2,176 program words), and the last 3D frame's Edge job
records and per-job scratch survive (that's where the skin palettes came from). Render targets come back
zero, since RPCS3 doesn't write them back, so only AC.18's FLIP still needs a real capture. Taking one is
the same attended Cmd+S on the game window, or AF's `SAVESTATE`. The June state itself (parked on a pause
screen in `level_213_tutotial`, the game's own spelling, so no hero is drawn) passes `09d602fd5`'s
version check and fails `accfecd2d`'s (it lists global version 21, `accfecd2d` accepts only 24). That's a
version-table check, not a load: no `--savestate` boot of it has run, and one settles it. Every savestate
check in Giants in detail parsed it offline.

What it lacks:

- P6 is digital only, drives port 0 only (`rpcs3-patches/0006-P6-*.patch:65,76`) and holds each press with
  a wall-clock `sleep_for` (`:122`). No analog sticks, no frame lock, no second pad, so it can't drive
  walking, combat, couch battles or co-op.
- P4 reports `PE cmd=write` with no block id or payload (`rpcs3-patches/0004-P4-*.patch:107`). That's
  fine: figure-side ground truth (XP and gold awarded, upgrades bought) comes from the game's own state
  through memory peeks (MEMSNAP, later P9) or what it shows on screen, never from diffing a `.sky`, which
  would drag the format the full reset skips back in. The oracle still needs a `.sky` to put a figure on
  its emulated portal; that's the dev's own pack, dev-only like the rest of RPCS3.
- Determinism. RPCS3 has NO deterministic mode (no "determinis" anywhere under `rpcs3/Emu`): guest time
  comes from the host TSC, PPU and SPU run as host threads under the OS scheduler and RSX runs
  asynchronously, so replays from one savestate start identical and drift. By how much is unmeasured;
  that's AF's first job, with `Start Paused` (every replay stops after the same 2 frames), a dev
  `RESUME` and every input and comparison keyed to the flip counter P1's heartbeat already carries.
- P9 (a per-frame state stream: camera, instances, poses, RNG, entity transforms) doesn't exist and can't
  until the Giants struct layouts are reverse engineered. It comes after that RE, not before.

AF builds the foundation before anything gets called a regression test:

1. Measure divergence: same savestate, same input, 10 runs, on the Mac (the HTPC's runs wait for the
   Windows/HTPC bring-up).
2. A dev-only input patch in its own env-gated series, never in a product binary: frame-locked analog
   sticks, two pads, press-at-frame-N, an RNG peek and seed verb.
3. Tolerance-based comparisons: trajectory envelopes, event-sequence equality (spawns, hits, pickups,
   script events), damage, XP and gold totals, FLIP thresholds on frames. Frame-exact diffs are not the
   plan.
4. More dev verbs as the rungs need them: FSTRACE (asset loads with the guest return address), MEMSNAP
   (bulk peek), RSXCAP (scripted capture).

Pin policy: the oracle rides the `09d602fd5` copy (AC.2). Moving it back to the in-tree pin takes the two
upstream fixes (cherry-picked, or a bump past `d08d568d5e`) plus a boot that reaches gameplay, and costs
every savestate taken at `09d602fd5`; that's AF's pin choice, made by chotchki. With the product frozen, a
bump is his call, not a cadence. After G1 the pin moves only for OS
breakage, and it retires at G2.

## The extractor

Three crates. `guac-formats` holds pure parsers over byte slices (IGA, IGZ, `igImage2`, Edge, Havok
packfiles, FSB5, Bink headers), fuzzed. `guac-ir` holds the versioned serde IR for game-logic data (RON in
dev for diffing, postcard shipped). `guac-extract` is the CLI, and the library the app calls on first
run.

- Input is plaintext `PS3_GAME/USRDIR` from the dump the user pointed setup at, plus `PS3_GAME/TROPDIR`
  for achievement names and icons if its container turns out readable. Never the EBOOT: the field layouts
  RE recovers live in `guac-formats` as code, so an install needs no decryption and no firmware.
- Giants' parity target is the 1.00 disc (Decisions, third batch): the HTPC's Giants has no title update,
  so the extractor reads the disc alone and never looks for an update (`dev_hdd0/game/<TITLE_ID>/USRDIR`
  in RPCS3's layout, which setup doesn't know about anyway).
- Identity key: `(TITLE_ID, disc APP_VER, plaintext-USRDIR manifest hash)`, with no update field. All six
  discs here are APP_VER 01.00 (Trap Team's PARAM.SFO VERSION says 01.01); only Swap Force has an
  installed update on the Mac, and it replaces only the EBOOT. An unknown dump gets its own error code and
  a report, not a guess, and a dump a title update changed (files merged into USRDIR, a PARAM.SFO past
  01.00) is one of those: reported, never handled. A later game's target gets settled when that game
  starts.
- "Only the EBOOT is encrypted" rests on magic sniffing (`SCE\0`, `NPD\0`). AB adds an entropy scan over
  USRDIR.
- Output: one extract dir per identity with a manifest (extractor version, IR version, identity, source
  hashes). The engine refuses a stale manifest and offers to re-extract. An IR version bump forces
  re-extraction, which is why a proposal must say whether it changes the layout or the IR version. The
  extract dir is a cache, never a save (Persistence and setup).
- Commodity assets transcode to formats Bevy loads natively. Textures → KTX2 carrying the original BC
  blocks (untile and endian fix only, no re-encode). Meshes, skeletons and animations → glTF `.glb`, with
  engine semantics in the IR. FSB5 MPEG → MP3 (a remux, not a byte copy). Bink 1 → ffmpeg → AV1 or VP9
  with the locale's voice track. Logic data (scenes, collision, tfbScript graphs, tuning, localization) →
  IR. Content-hash dedupe (Giants is more than half duplicate bytes by name).
- The extractor needs an ffmpeg it can call (bundled or found on PATH); AB decides.
- Nothing in the repo: unit tests use synthetic byte fixtures built in code, and tests that need a disc
  are `#[ignore]`d and read the dump path from `.env.dev`.

Local-only upscale pipeline (Enhanced). A first-run "generate my pack" step on the user's machine; the
output never ships and, like the extract dir, is a cache. Texture roles route by the suffix in each
texture's authoring path (Giants uses suffixes like `_diffuse`, `_normal`, `_specular` and `_ao`, plus
per-character rim/matcap textures like `armorrim`). The chain: a 1x de-BC1 pass, a GAN upscale on albedo
only (Real-ESRGAN class, compact models by default), normals through a vector path (decode, denoise,
guided upsample, renormalize; never a colour GAN), spec/AO/masks by guided upsampling against the new
albedo, mips regenerated in linear light, then BC7 colour, BC5 normals and BC4 masks. Tileables get wrap
padding, atlases get island dilation (that needs mesh UVs, so it follows AC), flipbooks go cell by cell.
Giants is roughly 1.1-1.2 Gtexel unique (13,275 unique texture paths, mostly 128-512 px, full mip
chains); how long a full pass takes gets measured when the pipeline runs.

## Renderer

Faithful first, Enhanced as a toggle.

Faithful. Giants' material look is painted diffuse (lighting and AO baked in by the artists), tangent
normals, specular colour and per-character rim/matcap lookups, the same model SSA uses. Giants' render
pipeline itself isn't characterized yet. Trap Team's is (a data-driven `tfbRenderer.igx` with a G-buffer,
tiled deferred lights, CSM, AO, bloom, DoF, motion blur, FXAA and LUTs) and Giants is probably kin, but
that's unverified. The shading model goes through the method above (spec, WESL, FLIP against RSX
captures; RPCS3 already holds 155 fragment and 62 vertex programs for Giants). Gamma-space vs linear
output is unknown; an RSX capture's surface formats answer it.

Enhanced keeps the stylized model as the look and modernizes the light transport around it:
higher-resolution CSM with PCSS or contact shadows, GTAO, TAA (DLSS on the HTPC), tuned bloom and DoF,
an HDR tonemap matched to the original grade, the upscaled pack. No wholesale PBR conversion and no
de-lighting the albedo; that's re-art-direction, not a remaster. The toggle keeps both material sets
resident per entity (or one `ExtendedMaterial` with a mode uniform) and swaps the camera's post
components. Halo CE Anniversary and Unleashed Recompiled are the precedents: accuracy by default,
enhancements opt-in.

Frame rate: logic runs in `FixedUpdate` at the original tick and rendering interpolates transforms
between ticks, so 60 fps+ doesn't touch game logic. Adventure co-op is one shared camera with leashed
players, so parity needs no split screen.

The floor is the Mac until the Windows/HTPC bring-up (Decisions, fourth batch): the M3 Max driving the
TV, on Bevy's Metal backend. What that means now:

- No temporal upscaler. Bevy has TAA but no DLSS, FSR or MetalFX on macOS, so every frame the TV shows is
  either rendered at the TV's output resolution or upscaled with no temporal reconstruction. Enhanced gets
  sized to what the M3 Max holds at that resolution, not to what DLSS would buy (the ~1440p-internal plan
  below is the HTPC's).
- Synchronous pipeline compiles. macOS compiles each render pipeline inside the frame that first needs it,
  so a material, a post effect or the Faithful/Enhanced toggle seen for the first time hitches. Both
  material sets and both post stacks get pre-warmed at level load, and on the target machine that's a
  requirement, not polish.
- An open macOS rendering-corruption bug to watch
  ([bevy #25595](https://github.com/bevyengine/bevy/issues/25595)).
- Not measured: a frame with CSM, SSAO, TAA and bloom at the TV's resolution on the M3 Max. AE measures it
  (GPU timings from `RenderDiagnosticsPlugin`), and that number is the budget the Enhanced toggle has to
  fit until the bring-up.

The HTPC, at the bring-up:

- RTX 2080 Ti (Turing, 11 GB) is DLSS-capable, so the plan for the 86" 4K TV is ~1440p internal with DLSS
  up to 4K. Bevy's DLSS integration is Vulkan-only on Windows, so the HTPC runs Bevy's Vulkan backend.
- First-generation RT cores make ray tracing an optional toggle at most, never a target. Bevy's RT GI
  (Solari) also only takes `StandardMaterial`, can't render skinned meshes and doesn't light from
  point or spot lights yet, so it doesn't fit the faithful material model in any case.
- Not measured: a 4K frame with CSM, SSAO, TAA and bloom on the 2080 Ti, DLSS on Vulkan. The bring-up
  measures it.

Bevy 0.20 has no HDR display output on either machine; the TV's HDR waits on Bevy.

Bevy is 0.20.0, pinned per milestone. Upgrades land at milestone boundaries as their own change, because
every 0.x release breaks API (0.20's switch to WESL broke every custom shader written for 0.19).

## Controllers

Verdict: Guacamole reads the real pad, maps by position and shows the label printed at that position on
the pad in the player's hands.

- chotchki's pads are 8BitDo Ultimate 2s in both variants, PC and Switch. Same four face positions,
  different labels: the PC variant prints Xbox-style (A at the bottom, B on the right) and the Switch
  variant Nintendo-style (B at the bottom, A on the right). That's from the published layouts, checked
  against the pads in hand with the hardware checks below.
- Mapping is positional, because the original's controls are positional. Bevy already names face buttons
  by position (`GamepadButton::South`, `East`, `West`, `North`), so the game binds positions and only the
  prompt glyphs change per pad. Never the PS3's glyphs.
- On the HTPC (the bring-up), Steam Input passthrough is the decision: Steam Input off on the `guacamole`
  shortcut, so Guacamole sees the real device instead of Steam's virtual pad. Telling the variants apart
  is probably SDL3-class identification, vendor and product id against a gamepad database (SDL3 picks a
  pad's face labels that way). Bevy's `Gamepad` does carry the vendor and product id its gilrs backend
  reports; whether that's enough to tell the two 8BitDo variants apart is unverified on the Mac and on
  Windows, and if it isn't, the fallback is a glyph-set choice in settings.
- What the Home button does in Guacamole is open (Open question 1).
- Hardware checks, a backlog item and not a gate for any rung ("all non-urgent testing"):
  - On the Mac, with any `guacamole` build: does each variant show up with an identity Guacamole can map
    to the right glyphs, and does Home reach Guacamole?
  - On the HTPC at the bring-up, with Steam Input off: the same identity question, where the Switch
    variant is the shakier one (if it presents as a Switch Pro-class pad, Windows has no XInput path for
    it and only a HID-level driver reads it; SDL's HIDAPI is the reference). Does Home reach Guacamole, or
    does Steam keep it for its overlay? Can the per-shortcut Steam Input setting be applied automatically,
    or is it a one-time step by hand?
- AG's input spec carries the prompt rule, since walking a level is the first rung with a pad in hand.

## Figures

Verdict: a figure is an identity, toy type + variant, out of the disc's own roster, and its progress
lives in Guacamole's store per profile. Guacamole never reads a `.sky`.

- Identity: the phone picks a figure by id, and the server's figure ids already are the tag identity
  (`<toy type>-<variant>`; `ToyTypeId` and `TagVariant` in
  [`crates/core/src/figure.rs`](../../crates/core/src/figure.rs)). In Giants
  the toy type names the character archive directly (Giants in detail).
- Catalogue: the disc roster, all of it, with no per-kid filter. Names, elements and portraits come from
  the disc (where each lives in Giants' data is for AC and AE to find), and the roster lands with the
  first decoded models in AE. It doubles as the compatibility list: Giants offers what Giants has
  characters for, which replaces today's origin-game-and-later heuristic (`crates/core/src/compat.rs:48`).
- AA's catalogue is interim (AA.4,
  [`crates/guac-server/src/native_catalogue.rs`](../../crates/guac-server/src/native_catalogue.rs)): the
  wiki data already in the repo ([`data/figures.json`](../../data/figures.json), keyed by the same toy
  type + variant, plus `data/images/` portraits, under `DATA_ROOT`), cut to what Giants plays and checked
  row by row at boot, the same list for every profile, no `.sky` and no firmware pack. Checked because the
  data is patchy and partly mis-keyed (the rekey from wiki pages to tag ids put some pages on the wrong
  rows) and wrong is worse than missing: a TV that calls Spyro "Cynder" is the failure, a missing repose
  isn't. The rules:
  - a row is in when its `firstrelease` is Spyro's Adventure or Giants;
  - a base row needs a name, and a variant row's name must contain its own toy type's base name, which
    drops the mis-keyed rows (a "Cynder" on Spyro's toy type) and keeps real variants ("Legendary
    Ignitor", "Polar Whirlwind");
  - one name keeps one toy type (Wrecking Ball's row also sits on three SWAP Force toy types, really
    Stink Bomb, Rattle Shake and Wash Buckler, and only 0x17 keeps it), and a variant whose base lost its
    name goes too;
  - game and element (read case-insensitively) come from the row; category comes from the base row's
    `alignment`, which a variant takes too (Gnarly Tree Rex's own row says "Skylander" for a Giant), a
    variant's group is its base's name and its tag is the rest of its own name ("Legendary"); the
    portrait is `data/images/<id>/thumb.png` when it exists;
  - a hand-written supplement fills the four base characters the data lacks or mislabels: 0x06 Dino-Rang
    (Earth), 0x0D Wham-Shell (Water), 0x15 Drill Sergeant (Tech) and 0x64 Jet-Vac (Air), with no portrait
    (their `thumb.png` files show a screenshot, a crab and a building). chotchki's check of the four is
    AA.14.

  On the committed 489 rows: base 262 → 61 kept (197 later games, 1 nameless and 3 duplicate names
  dropped), variant 227 → 12 kept (199 later games and 16 failing the name rule dropped), plus the 4 = 77
  figures, every one on RPCS3's figure list after `& 0x0BFF`. The disc roster research
  (`~/workspace/guacamole-research/ae-roster/FINDINGS.md`, outside the repo) found 123 figures on the
  Giants disc, all 77 among them, but only 60 agree exactly: 12 variant ids differ from the disc's
  (Granite Crusher is 0x0202 here, 0x0402 on the disc), 5 items have no element where the disc's macro
  default says 0 (keep None, the research says) and 46 disc rows aren't here at all (Series 2, LightCore,
  locations, a few items). It confirmed AA.14's four supplement rows. The disc roster replacing this
  catalogue (below) closes the id and coverage gaps; which variant-id encoding it keeps, the disc's bits
  or AA's wiki words, is chotchki's call in the findings.

  `guacamole` stops reading it once the disc roster lands, and the files themselves
  (`data/images/`, `data/figures.json`, `data/figures.manual.json`, `data/rekey-log.json`,
  `data/vehicle_terrain.json`, `data/games/*.png`, `tools/wiki-scrape`) go at G1 with the product code
  that still reads them.
- Portraits, a recommendation: the game's own UI portrait textures if Giants ships them (`igImage2`, which
  AC decodes anyway, and it's the original art), rendered from the extracted model as the fallback.
  Either way the Giants disc only covers the figures Giants plays; Swap Force through Imaginators figures
  arrive with their own discs.
- Instances: one per profile per figure, keyed by profile + figure identity and shared across games, the
  way today's working copies are (`working/<profile_id>/<figure_id>.sky`, minus the file). The profile is
  one of `guacamole`'s own (Persistence and setup), so nothing is keyed to the controller's profile ids. A
  figure a profile has never placed starts fresh on its first placement. The store's columns grow with
  the rungs: XP and gold at AH, upgrades and path at AK, the rest (hats, nicknames, hero points ...) when a
  rung needs them.
- Phone editing: the editor the phone already has (level and gold, plus reset to fresh) is buggy today
  because `crates/server/src/sky_edit.rs` has to decrypt, edit and re-encrypt a `.sky`. Under Guacamole it
  edits rows in the store instead, from AH, and reset to fresh deletes the instance. `sky_edit` and
  `sky_stats` stay product-only in `crates/server` (the server split) and leave with it at G1.
- Ownership: the "profile that placed it" rule stays, so two phones keep working and co-op progress lands
  on the right kid (Deliberate departures).
- No scanned figures. The ACR122 reader leaves the product first, in AD (a product change that lands
  before AA), and `GameOfOrigin::Unknown`, which only scans produced, goes with it (Decisions, fourth
  batch).
- The AA card shows identity only (name, element, portrait). Level, XP and gold join it once the store
  exists (AH); before that every figure is fresh, so there's nothing to show.

## Milestone ladder

Every rung ends in something the kids can see except AC (the decode gate) and AF (the oracle
foundation), which the rungs after them stand on. A rung starts when the ones it stands on have landed;
there are no dates. Ids past AC are provisional: `openspec/config.yaml` allocates the next id when a
change is proposed, so a product change proposed first takes the next letter (Decisions, third batch).
That has happened once: the ACR122 removal (`ad-remove-acr122`) took AD, so every rung after AC moved
up a letter. The milestone names (M0-M9, G1, G2) are the stable handles. Every rung is built and checked
on the Mac driving the TV (Decisions, fourth batch); the Windows/HTPC bring-up isn't a rung, it sits
beside the ladder in `openspec/backlog.md`, gates no rung and starts on chotchki's call.

| Rung | Where | What the kids see | What it proves |
|---|---|---|---|
| AA | `aa-guacamole-shell` | place a figure on the phone, the TV shows its card (name, element, portrait), in `guacamole` | the embedded server, the native driver over a channel, Guacamole's one-question setup, its own database and profiles, the interim catalogue with no `.sky` and no firmware pack, one binary at a time, all on the Mac driving the TV (the MSI carrying both binaries is the bring-up's) |
| AB, M0 | `ab-giants-extract-and-intros` | place a figure, its original intro video plays | `guac-extract` skeleton (identity, IGA, manifest, Bink 1 transcode, entropy scan); Bevy video playback is a spike. The 48 intros are named by character, not toy type; the voice-over folder in each character archive bridges most of them and AB maps the rest. A figure without one keeps its AA card |
| AC | `ac-giants-decode-gate`, in parallel with AA and AB | nothing yet (a dev viewer) | IGZ v6, `igImage2`, Edge and a Havok 2011.2 skeleton plus one clip, matched against an RSX capture. Starts once AB's IGA reader has landed. If the decoded hero doesn't match the capture, AC proposes Trap Team and chotchki decides |
| AE, M1 | backlog | their figure as an animated, textured turntable on the TV | the first 3D; the Mac's renderer floor measured; the disc roster replaces the interim catalogue |
| AF | backlog | nothing | the oracle foundation: divergence measured, frame-locked two-pad input, tolerance metrics. Lands before any gameplay comparison |
| AG, M2 | backlog | walk one level | level scene, collision, character controller, camera, feel checked against the oracle; the input spec, with prompts that match the physical pad |
| AH, M3 | backlog | first fight: one enemy, XP and gold earned and kept on the kid's profile | combat basics; the per-profile figure-progression store, with the phone's figure editor on it |
| AI, M4 | backlog | first couch battle: one Giants PvP arena, two pads, two phones | a second pad and a second profile's figure in play; the PvP rules as the oracle shows them |
| AJ, M5 | backlog | the level comes alive | the tfbScript VM and its built-ins running the original scripts |
| AK, M6 | backlog | upgrades and the upgrade screen | upgrades and path kept in the store, movesets from script data |
| AL, M7 | backlog | co-op, two pads and two figures in the adventure | the second player on the host's story, the shared camera, each kid's figure progress on their own profile |
| AM, M8 | backlog | chapter one | a run of levels with story and cutscenes; profile-owned story saves (the host's, no slot picker); achievements, by the co-op rules |
| AN, M9 = G1 | backlog | the whole game | `guacamole` becomes the only thing this repo builds and ships; the launcher surface lives in Bevy; the installed controller keeps the other five games |
| AO, G2 | backlog | parity | the oracle retires |

## Risks

Ranked by how much each could move the plan, each with the experiment that answers it.

| Risk | Why it matters | Experiment | Where |
|---|---|---|---|
| IGZ v6 field layouts aren't in the files | no public Giants metaobject dump (ig-workshop's Giants data has enums and fields only; AlchemyMetadataDumper doesn't list Giants PS3 and is a PRX built with Sony's SDK) | RETIRED 2026-10-18: the EBOOT registers every type from static tables, read out of the decrypted ELF with no memory dump (AC.4: 989 types, 0 misses over 1,346,793 reference fixups), and the live registry in the June savestate agrees (AC.14: 980 types live, 272 of 272 gate field reads) (Giants in detail) | done, AC |
| Edge geometry on v6 | every mesh is Edge-compressed for SPU | decode one character mesh, compare to the vertex buffer in an RSX capture | AC |
| Havok 2011.2 animation | likely compressed clips; skeletons feed everything after M1 | one skeleton and one clip, posed against a capture | AC |
| RPCS3 run-to-run divergence | decides whether the oracle can be a test at all, and how loose the tolerances get | 10 replays from one savestate on the Mac, diff positions and RNG (the HTPC's at the bring-up) | AF |
| A tagged MSI reinstalls the product from current source | the kids' other five games run on it. (Guacamole tables in its `db.sqlite` were the old risk here; `guacamole`'s own database resolves that one by design.) | AA's split lands as moves only and passes the product's e2e suite on the Mac at its pre-AA count before any native code; the bring-up runs the HTPC live smoke and checks the installed product from the MSI | AA, then the Windows/HTPC bring-up and every tagged release |
| How much of Giants is script data | sizes the VM vs the native built-ins | RETIRED 2026-10-18: the whole-disc census (Giants in detail, tfbScript) confirmed the Wii offsets on PS3 and sized the built-ins as 414 engine members in 66 namespaces; `OpFlowBuiltInBehavior` was the wrong yardstick | done, before AJ |
| Bevy video playback | M0 depends on it, Bevy has no video player | decode one transcoded intro into a texture on the Mac (the HTPC's decode cost at the bring-up) | AB |
| Embedding the server in a Bevy app | the bootstrap was eframe-shaped | RETIRED 2026-10-18: AA.2 lifted it into `guac_server::boot`, `guacamole` boots it from Bevy with the native driver (AA.6) and the product's phone e2e on the Mac passes every test that passed at the pre-AA base (AA.10: 32 of 35, the 3 failures the base's) | done, AA |
| The Mac's renderer floor | the Enhanced stack at the TV's resolution with no temporal upscaler, plus synchronous pipeline compiles | a Bevy scene with CSM, SSAO, TAA and bloom at the TV's resolution on the M3 Max, GPU timings from `RenderDiagnosticsPlugin`, pipelines pre-warmed at load | AE |
| HTPC performance floor | 4K on a 2080 Ti with the Enhanced stack | the same scene at 4K on the 2080 Ti, DLSS on Vulkan | the Windows/HTPC bring-up (backlog) |
| Feel parity (camera, controller, collision, Havok-tuned physics) | the long tail, never retired | not a spike: measured continuously by the oracle from AG on | AG onward |
| Reading the real pads | wrong prompts, or a Switch-variant pad Windows can't read without Steam | read both variants' ids in a Bevy app on the Mac, then on the HTPC with Steam Input off for its shortcut at the bring-up | backlog (non-urgent) |
| Trophy data on the disc | achievements without the original names and icons | open `PS3_GAME/TROPDIR` on the Giants dump and see whether its container reads | before AM |
| Encrypted data outside the EBOOT | the extractor assumes plaintext USRDIR | an entropy scan over USRDIR | AB |
| Bevy churn | every 0.x release breaks API | pin per milestone, upgrade at boundaries as its own change | ongoing |

## No game bytes in the repo

HARD rule, no exceptions: no extracted assets or scripts, no decrypted EBOOTs, no decompiler or
disassembler output (Ghidra projects included), no RPCS3 memory dumps, no RSX captures, no keys.

- They live outside the tree: research scratch, the local Ghidra project and the user's own extract
  dir. In dev the extract dir ALSO points outside the working tree (a path in `.env.dev`), because a
  gitignored `./dev-data/` is one `git add -f` away from a leak.
- The repo holds our code, our specs in our own words, synthetic test fixtures and links to public work.
- Disc dumps are read-only. Nothing writes into them (`rpcs3 --decrypt` writes next to its input, so it
  only ever sees a copy).
- Enforced by construction once AB lands (AB.1): a `cargo test` that fails when any file git would commit
  or push starts with a game magic. That's the index (`git ls-files`) plus every blob HEAD or a local
  branch reaches that no remote has yet, because a push sends commits and a file committed then deleted
  still ships with its commit. The magics: IGA in both byte orders (`IGA\x1a` on Giants and Trap Team,
  `\x1aAGI` on every later `.pak`), IGZ in both (`IGZ\x01`, `\x01ZGI`), `BIKi`, `KB2`, `FSB5`, `SCE\0`,
  `NPD\0`, `\x7fPKG`, `RRC`, `\0PSF` and the disc's own TROPHY.TRP, LIC.DAT, PS3_DISC.SFB and
  PS3UPDAT.PUP heads. Only each blob's first 64 KiB is read, so a staged disc image costs that, not its
  size. Two of the rule's items don't start with a magic: RPCS3 writes RSX captures gzipped (`.rrc.gz`,
  `RRC` only after inflating), and a decrypted EBOOT is a plain big-endian PPC64 ELF. So a gzip file is
  checked again after inflating its first bytes, and a 64-bit big-endian ELF with machine 21 fails too.
  `.githooks/pre-push` runs `cargo test --workspace` on every push from the dev clone. The Windows CI
  lane runs it too, but push CI fires only on `main`, so on `2.0` a `--no-verify` push gets past the
  guard until a merge to `main` or a dispatched run. A guest memory dump has no magic at all; only the
  outside-the-tree rule above keeps it out.

## Measuring progress

No time or effort estimates (Decisions): each rung reports what it measured (layers decoded, oracle
deltas, what ran on which machine) and chotchki decides what comes next from that.

## Open questions

1. What does the pad's Home button do in Guacamole?
2. Where's the Wii SSA decompilation the tfbScript project's helpers read (`SSA_DECOMP`, a
   [decomp-toolkit](https://github.com/encounter/decomp-toolkit) layout for `SSPP52`)? Not found on
   2026-10-17: GitHub code search for its config files and symbol names, the project owner's repos and
   stars, decomp.dev (shutting down, its project list gone) and web searches all came up empty
   (`spyrosadventure/ReSSA` is a recompilation template, not it), so it's private, unpublished or
   somewhere those searches don't reach. Nothing waits on it; it would help AJ read the Wii engine's side
   of the VM.
   Searched again 2026-10-18 with the same result, plus a likely answer: every address the project's
   helpers name is a decomp-toolkit auto-name (33 distinct `fn_`/`lbl_`, no symbolic name anywhere) and the
   folder layout is [dtk-template](https://github.com/encounter/dtk-template)'s, so it's probably a local
   auto-split of the owner's own `main.dol`, not a decompilation (hypothesis; asking xirba13 settles it).
   Anyone with the disc can regenerate that layout, and the PS3 EBOOT plus its registry already tell AJ
   more, so this stays closed unless something turns up.
