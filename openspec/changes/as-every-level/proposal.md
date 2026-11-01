# Phase AS - every level through the extractor, and what their scripts need

## Why

Everything past chapter 1 stands on two things nobody has measured: whether the extractor reads every other level, and what every other level's scripts ask of the VM. The 2026-10-31 stock-take ([`stocktake/`](/Users/chotchki/workspace/guacamole-research/stocktake/), its `formats`, `vm` and `systems` notes) found:

- **The readers pass on all 143 `level.bld` files, but the extract plays one.** `guac_extract::levels::LEVELS` is `["level_213_tutotial"]` ([`levels.rs`](../../../crates/guac-extract/src/levels.rs)), nine parts and 331 MiB. The other 142 are 23 story and hub levels, 18 PvP arenas, 76 challenge and arena files (45 challenge levels, 21 arena challenges and the 10 shared mesh bundles they draw from), 19 ship parts, the title, the credits and 4 minion packs. At chapter 1's size every level on a first run is far more than a first run should write.
- **A draw under a transform is refused** (the same file's scene and prop readers): 10 levels hold 54 transforms, 28 of them not identity and 40 animated (ag-levels), so those levels' scenes don't extract at all today.
- **Materials and atmosphere are checked on level 213 alone.** `giants_materials` covers the character and item files but no level file, and the atmosphere reader, which replays a level definition's zone calls statically, has only ever met chapter 1's.
- **The 11 FMVs are indexed and never converted.** fmv01-fmv04 and fmv06-fmv12 (2,136 MiB) sit in the intro index as unclaimed movies; ten of them carry 13 audio tracks, so playing one needs the voice track picked, which the IR's voice choice already models.
- **No other level's scripts have ever been linked or run.** The disc's 139 scripted level files hold 12,996 scripts and 4.13M op positions; AJ (the level comes alive), AI (a PvP arena), AM (chapter one's run of levels) and AN all start from a gap list nobody has.

## What Changes

- **Levels extract on demand.** A first run still writes chapter 1 alone, byte for byte as today; any other level is extracted the first time it's asked for, keyed per level in the manifest with the per-part resume AG.15 built, so a start never goes stale for a level it hasn't entered. The extractor's level list comes off the dump (each `level/<name>.bld` the dump scan records), not a constant. The backlog's per-file sync line is settled here, since an on-demand level pays it while a kid waits.
- **A whole-disc level read, in memory**: every part's reader over all 143 levels with nothing written, a pass table per level and part, and the failures fixed: a static transform baked into the draws under it, an animated one at the matrix its file stores (its animation a box filed), and the material and atmosphere readers on every level.
- **Any named level extracted end to end** into a throwaway root, checked on a story level past chapter 1, a PvP arena and the hub, each one's size and time recorded.
- **The FMVs as an on-demand extract item** on the intros' transcode path, the English voice track picked off the movie's own header and mixed over the stereo bed.
- **The VM gap list**: a new `#[ignore]`d `guac-script` disc test linking `bootstrap.bld`, `global.bld` and `init_setup.bld` with each level straight from the dump and running it strict on a null host, its output the members and ops every level past chapter 1 stops on, ranked by how many levels reach them and how early. The gaps are filed as boxes for the VM lane that follows; none is fixed here.
- **The wiring lane's boxes**, filed unticked: a chosen level loaded on the TV, the pre-warm census per level, the scripts' own level loads, transitions and unloading, and a level's placed emitters.
- Out: playing any level past chapter 1 on the TV (the wiring boxes), modelling the members the gap list names (the VM lane), animated scene transforms, the cinematics' cameras and the FMVs on the TV, level audio (the audio lane), Windows.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `guacamole/disc-extraction`: two ADDED requirements, a level past chapter 1 extracted on its first entry and a cinematic movie converted on its first play. Both become observable on the TV with the wiring boxes, which check them.

## Impact

- Track: Guacamole, beside the ladder as AP is: it delivers no milestone itself and feeds AI, AJ, AM and AN. Its extractor half stands on nothing in flight; its wiring boxes stand on its extractor boxes and on AG.19's level load.
- Disc serial read: BLUS30968 01.00 only: every `level/*.bld`, `permanent/bootstrap.bld`, `permanent/global.bld`, `character/init_setup.bld` and `movies/fmv*.mov`. Tests that read it are `#[ignore]`d and take `GUAC_GIANTS_DUMP`; the throwaway extracts go under `GUAC_EXTRACT_DIR`, one level at a time, removed after.
- guac-extract's output: chapter 1's parts stay byte-identical (checked before and after); levels and FMVs past it are new records in the manifest under the existing layout 4 and IR version, written only when asked for. A first run's extract changes in nothing it writes.
- Kid-visible milestone: none in the extractor half; the wiring boxes put a chosen level on the TV.
- Code: `crates/guac-formats` (a Bink header reader with its property test and fuzz target, any reader fix the whole-disc read needs), `crates/guac-extract` (the level list, the on-demand level and movie entry points, the transform bake, the disc tests), `crates/guac-script` (the gap test). No guacamole, Bevy or phone change in the extractor half.
- Dependencies: none new.
- Owning doc: [`docs/native/guacamole-design.md`](../../../docs/native/guacamole-design.md) (The extractor; the Milestone ladder, beside which AS sits); [`CLAUDE.md`](../../../CLAUDE.md)'s Architecture line for `guac-extract` when its entry points grow.
- Release reach: none. guacamole runs from `cargo` on the Mac and no `v*` release carries it yet.
- One-way doors: none. The extract is a cache.
- Backlog: the "extract syncs every file it writes" line is promoted and settled in this change.
