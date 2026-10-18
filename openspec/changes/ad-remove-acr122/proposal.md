# Phase AD - Remove ACR122 support from the product

## Why

chotchki, 2026-10-17: drop ACR122 "as soon as it's convenient", and on why Guacamole doesn't need figure dumps, "people don't have to dump all their figures (we would drop the ACR122 support too)". Guacamole never reads a `.sky`: a figure is an identity off the disc's own roster (`docs/native/guacamole-design.md`, Decisions), so a USB reader has nothing to feed. The product is frozen except for changes like this one (`openspec/config.yaml`), and CLAUDE.md names this one as the standing exception.

What the feature is today, and how much of it actually ships:
- The scanner: `crates/nfc-reader` drives an ACR122U over PC/SC (`pcsc` 2.9), a worker thread dumps each tapped figure to `<runtime root>/scanned/<uid>.sky` and broadcasts `Event::FigureScanned`, and boot merges those dumps into the library under `scan:<UID>` ids. All of it sits behind the server's `nfc-import` feature, which no release has EVER compiled: both release lanes build `--features sky-stats,mock-driver-runtime` (`release.yml:105`, `:463`; CI's copy of that line, `ci.yml:194`, sits in the macOS job, off since 2026-05-30, `:155`), the feature was never a default (`crates/server/Cargo.toml:185`) and `git log -S nfc-import -- .github/` is empty. The only build that compiles it is CI's Windows `cargo build --workspace --all-features` (`ci.yml:102-103`).
- The half that ships in every release since v1.0.0 (the scan UI landed in `07493a5` on 2026-04-22, two days before v1.0.0):
  - the phone's SCAN NEW card at the head of the toy box. It opens an overlay that waits 30 seconds for a scan no shipped build can deliver, then says "NO FIGURE YET" (`phone/src/components/scan_overlay.rs:36`, `:175`);
  - the setup wizard, whose reader probe is hard-wired to false without the feature (`crates/server/src/wizard.rs:524-525`), so every install reads "No NFC reader detected. Plug in an ACR122U and re-run setup..." (`:735-736`), on Windows and on the Mac (the wizard runs on both since U.6).

So AD is mostly deleting dead code, plus closing two dead ends a real user can hit. It lands before AA, so AA's server split starts without a module it would otherwise carry to G1.

## What Changes

- `crates/nfc-reader` goes whole (the lib and the `nfc-dump` bin), with its workspace member (`Cargo.toml:6`). `Cargo.lock` drops `skylander-nfc-reader`, `pcsc` and `pcsc-sys`.
- `crates/server`:
  - the `nfc-import` feature and its optional dependency (`Cargo.toml:20`, `:204-211`), `src/nfc.rs` and its module line (`lib.rs:17-18`);
  - `main.rs`: the scanned-figure merge and tag-identity map (`:93-177`, where the `cfg(not)` arm at `:176-177` becomes the only path), the bindings that only feed the worker (`:269-273`, `:296-297`), the worker spawn (`:443-456`), the comments that describe the merge (`:67-76`) and the `runtime_dir` binding with its comment (`:84-91`), whose only readers are the merge (`:125`) and the worker's clone (`:273`), so it'd fail `clippy -D warnings` as an unused variable once they go. Dropping it loses no side effect: every runtime path helper creates the directory itself (`paths.rs:151`, `:159`, `:168`). The library becomes the pack and nothing else;
  - `wizard.rs`: the `reader_present` field, its probe and its init (`:442-448`, `:519-525`, `:551`), the Welcome status line and the reader clauses in the Welcome and pack copy (`:720-738`, `:916-923`, plus the doc at `:481`). The pack stays optional, since an empty library still boots for Imaginators' in-game creation and demo mode. Design decision 4 has the new copy;
  - `http.rs`: the `scan:` arm of `is_safe_figure_id` and its two doc mentions (`:443`, `:518`, `:538-540`), with a unit test that pins the two forms left (`{6-hex}-{4-hex}` and `sha:{16-hex}`) and rejects a `scan:` id;
  - `config.rs:117-118`'s "reader-only users" comment.
- `crates/core`:
  - `Event::FigureScanned` (`protocol.rs:144-167`), `MifareNuid` (`figure.rs:140-166`, re-exported at `lib.rs:15`), `FigureId::from_scanned_nuid` (`figure.rs:198-201`) and the `scan:` form in `FigureId`'s doc (`:174-175`), plus the pack-vs-scan wording at `:14`, `:111-114` and `:356-357`;
  - `GameOfOrigin::Unknown` (`figure.rs:321-324`), because scanned figures are the only thing that produce it (design decision 3), with its arms in `compat.rs:24-28` and `crates/server/src/sky_edit.rs:261`, `:302-305`;
  - `TagIdentity`, `MaskedVariant`, `VARIANT_IDENTITY_MASK` and `Figure.tag_identity` stay: pack figure ids are built from them (`crates/indexer/src/lib.rs:72-80`) and `tools/rekey-figure-ids` reads `tag_identity`.
- `crates/indexer`: `scan_runtime` (`lib.rs:115-212`) and the two helpers only it calls, `canonical_name_for_scan` (`:214-230`) and `figure_kind_to_category` (`:257-269`), which `clippy -D warnings` would flag as dead once it goes; the `Unknown` arms at `:661` and `tests/real_pack.rs:85`.
- `phone/`:
  - the SCAN NEW card (`screens/browser.rs:171-187`), its prop, import and doc mention (`:7`, `:10`, `:68`), and the scan-shaped grouping test (`:632-651`), which covers no branch `base_variant_sorts_first_then_alphabetical` doesn't. Two comments there go stale too: the search filter's "scan-only CC" example (`:96-99`) and the Crystals chip's reason (`:466-470`), which credits scan-only crystals; after AD the pack walk's `Creation Crystals` folders (`crates/indexer/src/lib.rs:381-391`) are what fill that chip;
  - `components/scan_overlay.rs` and its module lines (`components/mod.rs:15`, `:33`); `ScanOverlayState` and the overlay's mount (`lib.rs:20`, `:82-99`, `:210`, `:273`, `:423`, `:444`);
  - the `FigureScanned` mirror (`model.rs:235-252`), its handler (`ws.rs:340-383`) and the `scan_overlay` signal threaded through `ws.rs` (`:37`, `:74`, `:98`, `:127`, `:176`, `:212`, `:429`, `:447`, `:479`, `:509`);
  - the `GameOfOrigin::Unknown` mirror (`model.rs:76-80`) and its "SCANNED" label (`screens/figure_detail.rs:523`, comment `:491-492`);
  - `styles/components/scan_overlay.css` and its import (`input.css:255`), the plus-card rules and the header lines about them (`browser.css:8-14`, `:330-366`).
- Tests and the recorder: every `.fig-card-p4:not(.scan-new)` loses its `:not(.scan-new)`, which matches nothing once no card carries the class, in the e2e harness (`crates/e2e-tests/src/lib.rs:1200-1209`, `:1241-1249`), `tests/figure_edit.rs` (`:28`, `:42`, `:357`), `multi_phone.rs` (`:84`, `:88`, `:366`, `:518`), `regressions.rs` (`:57`, `:154`, `:193`, `:225`, `:392`, `:404`), `screenshot_tour.rs` (`:211`, `:262`), `working_copies.rs` (`:61`, `:123`, `:129`) and `tools/playthrough/src/beats.rs` (`:303`, `:315`, `:361`, `:617`, `:651`, `:818`, `:904`).
- Packaging and CI: `.gitignore:31-34` (`/data/scanned/`, left from a layout where the scanner wrote under the asset dir) and the `release.yml:203` comment that names it. `ci.yml` needs no edit, its `--all-features` build stops compiling the scanner by itself. `wix/` and `tools/*.sh` never referenced it.
- Docs: CLAUDE.md Data & paths and the Guacamole section's standing exception; `docs/aesthetic/mocks/scan_import.html` and its `mocks/index.html:58` entry; the install caption in `docs/dev/demo-reel-captions.md:125`; the Phase 6 sentence in `docs/roadmap.md:31`; the "(the `<uid>.sky` key)" aside in `crates/rpcs3-control/src/ipc/proto.rs:162`.
- Swept and kept:
  - `crates/sky-parser`'s Mifare wording (`README.md:4`, `lib.rs:3-13`, `:61`), the indexer's dump-format notes (`crates/indexer/src/lib.rs:22`, `:69`) and the phone credits' ".sky Tag Format" entry (`components/credits_overlay.rs:111`). They describe the `.sky` format, which the pack walk and the product's stats and edit read until G1.
  - `VARIANT_IDENTITY_MASK`'s doc (`crates/core/src/figure.rs:66-76`), whose example is a live-scanned Snap Shot. The mask still shapes every pack figure id, so it stays exactly as it is, and the example still shows what the masked year bits are.
  - The NUID in the IPC driver (`crates/rpcs3-control/src/ipc/mod.rs:44`, `proto.rs:158-166`) and in P1 (`rpcs3-patches/0001-*.patch:377`). That's the serial RPCS3 reports for the emulated tag, nothing to do with the reader.
  - `tools/rekey-figure-ids/src/main.rs:342-345`, which passes `scan:` ids through untouched when it rekeys an old database. It leaves with the wiki data at G1.
  - Word collisions: Unicode NFC normalization (`tools/wiki-scrape/src/main.rs:533-535`, `docs/research/firmware-inventory.md:172`), QR scanners (`crates/server/src/round_qr.rs:17`, `:43`, `:50`, `ui/main_screen.rs:58-60`) and "scan the folder" in SPEC.md's Q&A (`:176-177`, `:221`, `:364`). SPEC.md has no scanner requirement to retire.
  - Frozen records: `PLAN_ARCHIVE.md` and `openspec/changes/archive/2026-10-17-a-demo-reel/proposal.md`.

## Capabilities

### New Capabilities

None. `.openspec.yaml` sets `skip_specs: true`. AD removes behavior, but `openspec/specs/` is empty, so there's no requirement to REMOVE: the scanner, the SCAN NEW card and the overlay were never specified, and a spec written only so AD can delete it would be invented.

### Modified Capabilities

None.

## Impact

- Track: product, the one exception to the freeze chotchki named. No Guacamole code changes and no disc is read.
- Owning doc: CLAUDE.md, Data & paths (the Release root line, the `DATA_ROOT` bullet and the Scanned-figure layout paragraph). It's already wrong in two places: the scanner writes under the runtime root (`crates/server/src/main.rs:125`, `:454`), not `<data_root>`, and `main.rs:84-89` says setting `DATA_ROOT=./dev-data` broke image lookup. That comment is the only place the lesson lives and AD.1 deletes it with the dead binding, so AD.6 rewrites the `DATA_ROOT` bullet to say what the variable actually is instead of dropping it. Also touched: CLAUDE.md's Guacamole section, `docs/roadmap.md`, `docs/dev/demo-reel-captions.md` and the mocks index.
- Scanned figures already on disk are left alone: after AD nothing reads `<runtime root>/scanned/` and nothing deletes it (design decision 1). Only a root an `nfc-import` build ran against can have one, so no release install does. The dev Mac's `dev-data/scanned/` holds two dumps today.
- Profile data keyed by a `scan:` id: possible in those same dev roots, never in a release install. Working copies are keyed by figure id with no special case (`crates/server/src/working_copies.rs:28-29`, `paths.rs:178-179`), so a profile that placed a scanned figure owns `working/<profile>/scan:<UID>.sky`, a `figure_usage` row and maybe a saved layout naming it. After AD the library has no `scan:` ids:
  - a resumed layout's slot for one gets 404 "unknown figure_id" from `/load` (`http.rs:644-647`) and the phone toasts "Resume slot N: ..." while the other slots load (`phone/src/screens/modals.rs:115-121`). Every release build already behaves this way, since none of them indexes scans;
  - the `figure_usage` row never matches a library figure (`http.rs:402`);
  - the working copy is never opened.
  No migration (design decision 2).
- AA gets smaller, and AD lands before AA.1 starts. AA's artifacts already assume it: AA.1's cut no longer carries `nfc` or the `nfc-import` feature, and the bootstrap AA.2 lifts (`main.rs:67-645`) no longer counts the scan merge (`:93-177`) or the worker spawn (`:443-456`), which would otherwise be left behind or lifted. AD also shifts line numbers AA cites in `main.rs` and `http.rs`, so AA re-reads them when it starts, and AD.4's e2e pass count can serve as the baseline AA.0 compares against. AD stands on nothing.
- Tests: `cargo test --workspace` loses `crates/nfc-reader`'s five unit tests (Mifare key-A derivation, CRC48 and UID hex, which only a physical reader needs) and gains the `is_safe_figure_id` one; the phone loses one grouping test. The wizard is release-only (`wizard.rs:411`), so pre-push never compiles it, and no CI lane that runs does either: `--all-features` turns on `dev-tools`, which compiles it out, and CI's release-feature build (`ci.yml:193-194`) is in the disabled macOS job. AD.1 builds the release feature set on the Mac to cover it. None of the wizard lines AD touches sits under a `cfg(target_os)` gate (those blocks, `:527-531` the nearest, stay as they are), so the Mac build stands for Windows too, and the first Windows compile is the next tag's `release.yml:104-105`. The phone's unit tests run in neither pre-push nor CI, so AD.3 runs them by hand. The chromedriver mock suite has been off in CI since 2026-05-30 (`ci.yml:205`) and its selectors change, so it runs by hand on the Mac. No live RPCS3 lane is touched.
- Release reach: none of its own. AD reaches users only through the next `v*` tag, on chotchki's word, whichever change cuts it: its dmg and tarball carry AD to Macs and its MSI and zip to Windows. Nothing in AD needs one sooner, and Windows comes later (Mac first, design doc Decisions, fourth batch), so the first Windows MSI that also carries `guacamole` is the Windows/HTPC bring-up's, not AA's. What a user sees: no SCAN NEW card, and a wizard that no longer mentions a reader.
- One-way doors: none. No file on disk or database row changes, so reverting AD brings the feature back as it was.
