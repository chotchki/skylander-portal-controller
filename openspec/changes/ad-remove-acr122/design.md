# Design

## Context

The scanner path only ever ran in builds with `nfc-import` (proposal, Why): dev builds and CI's Windows `--all-features` build. What reaches users is the always-compiled half, the phone's card and overlay and the wizard's reader copy. So the only state AD has to reason about is what an `nfc-import` dev build left behind:
- dumps under `<runtime root>/scanned/<uid>.sky` (the runtime root, not `data_root`: `crates/server/src/main.rs:125`, `:454`);
- library ids of the form `scan:<UID>`, which can sit in a profile's working copies, its `figure_usage` rows and its saved portal layout, because nothing in `working_copies.rs`, `profiles.rs` or `http.rs` treats a `scan:` id differently from a pack id.

The product's migration set is frozen at three files (`crates/server/migrations/`). An older controller's `sqlx::migrate!` refuses a database carrying a migration it doesn't know (CLAUDE.md, Guacamole).

## Goals / Non-Goals

**Goals:**
- The product compiles and behaves as if the scanner never existed, in every feature combination CI and the release lanes build.
- Nothing a household already has on disk changes or disappears.

**Non-Goals:**
- Retiring `crates/sky-parser`, `TagIdentity` or the `.sky` stats and edit. They serve the pack and leave at G1.
- Any change to the wiki data, the firmware-pack indexer's pack walk or how pack figure ids are formed.
- Re-rendering the published demo videos, whose install caption may mention the scanner.

## Decisions

**1. Leftover dumps stay on disk, unread.**
After AD nothing reads `<runtime root>/scanned/` and nothing deletes it. Those dumps are the only record of a physical tag's state (gold, level, nickname), so deleting them buys nothing and can't be undone. No release install has the directory, because no release compiled the writer. Rejected: a one-time cleanup on boot (destroys user data in a frozen product); keeping the indexer's read path without the reader (that's keeping the feature, minus the only way to add to it).

**2. No migration for `scan:` ids.**
Working copies, `figure_usage` rows and saved layouts that name a `scan:` id stay as they are. Each one already degrades on its own in every release build today:
- `lookup_figure` misses, so `/load` answers 404 before any working-copy path is computed (`http.rs:644-647`);
- the resume flow toasts that one slot and loads the rest (`phone/src/screens/modals.rs:115-121`);
- `/api/figures` looks usage up by library id, so an orphan row never matches (`http.rs:402`).
A cleanup migration would be the product's fourth, and an installed controller older than it would then refuse the database. Rejected for that one-way door, in exchange for tidying rows no release could have written.

**3. `GameOfOrigin::Unknown` goes, in core and in the phone's mirror.**
`scan_runtime` (`crates/indexer/src/lib.rs:197`) is its only producer; the pack walk maps an unrecognized folder to `CrossGame` (`:423`). Kept, it would be a variant nothing constructs, plus a "SCANNED" label on the phone, a Kaos compat arm and a stats-generation arm, all for scans. Strong typing says the type shouldn't admit a state nothing produces. The removal is compiler-guided (every `match` names it), and no version skew is possible: the wire form `"unknown"` is never sent once the server lacks the variant, and the phone ships embedded in the same binary behind the `BUILD_TOKEN` check. Rejected: keeping it as a harmless dead variant. chotchki's call, 2026-10-17: "Remove it."

**4. Wizard copy, matching the wizard's house style.**
Release builds always took the reader-absent branch, so that's the copy users have read. The `reader_present` field and the Welcome status line go; the pack stays optional.
- Welcome body (`wizard.rs:720-724`): "We need your RPCS3 install path. A Skylanders firmware pack is optional - you can start empty and use in-game creation (Imaginators)."
- Pack page (`wizard.rs:916-923`): "Pick the folder with your .sky files. You can leave this blank to start empty and re-run setup any time to add one." ("Re-run setup any time" is the phrase the Demo and Display pages already use, `:783-784`, `:977`.)
chotchki accepted both as written on 2026-10-17: "Don't care, we're going to touch it again later."

**5. The toy box without the card.**
The grid's first cell becomes the first figure group, and nothing replaces the card: the library is the pack, and Guacamole's phone offers every figure it knows (AA's interim catalogue, then AE's disc roster), so there's no "add a figure" action left to put there. The empty state is unchanged, since the card already sat inside the non-empty branch (`phone/src/screens/browser.rs:160-170`). The e2e and recorder selectors drop `:not(.scan-new)` rather than gaining a new class, because `.fig-card-p4` then means exactly "a figure card".

**6. Order: producers before types.**
`crates/nfc-reader` and the server's `cfg(feature = "nfc-import")` code are the last users of `Event::FigureScanned`, `MifareNuid` and `scan_runtime`. So AD.1 deletes them first and AD.2 deletes the types; the other way round, `cargo build --workspace --all-features` breaks between the two. The phone (AD.3) isn't in the workspace and has its own mirror types, so it can land in either order relative to AD.2.

## Risks / Trade-offs

- [The wizard is release-only (`wizard.rs:411`), so `.githooks/pre-push`, which builds default features, never compiles it, and neither does any CI lane that runs: `--all-features` includes `dev-tools`, and the release-feature step (`ci.yml:193-194`) is in the disabled macOS job (`:155`)] → AD.1 builds the release command line and looks at both wizard pages on the Mac. None of the wizard lines AD touches is under a `cfg(target_os)` gate, so that build covers Windows too; the first Windows compile is the tag's `release.yml:104-105`.
- [The phone's unit tests run in neither pre-push nor CI] → AD.3 runs `cargo test --lib` in `phone/` by hand.
- [Push CI only fires on `main`, and AD lands on `2.0`, so nothing compiles AD on Windows before a tag] → accepted, chotchki on 2026-10-17: "Meh doesn't matter, we're focused on mac for now, windows will come much later." The Mac's pre-push and AD.1's release-feature build are the checks; the Windows lane's `--all-features` build catches a Windows-only break whenever it next runs, and the tag's `release.yml:104-105` is the wizard's first Windows compile, fixed forward if it breaks.
- [The `runtime_dir` binding in `main.rs` (`:84-91`) loses its last readers] → AD.1 deletes it rather than leaving a variable `clippy -D warnings` rejects. Its eager `create_dir_all` isn't needed, since every runtime path helper creates the directory itself (`paths.rs:151`, `:159`, `:168`). Its comment holds the only record that `DATA_ROOT=./dev-data` broke image lookup, so AD.6 moves that into CLAUDE.md.
- [Dropping `/data/scanned/` from `.gitignore` exposes a stale `data/scanned/` in an old checkout, and figure dumps must never be committed] → AD.5 checks `git status --ignored` on the dev Mac first. Since the layout change the scanner writes under the runtime root, which `/dev-data/` already ignores.
- [An e2e failure could predate AD] → AD.4 runs the suite on the commit before AD.1 too and compares, so only a test that passed before and fails after counts against AD.
