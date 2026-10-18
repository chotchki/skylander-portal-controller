# Phase A - Auto-generated demo reel (macOS) — LIVE

## Why

Pipeline **DONE + chotchki-approved (2026-06-24)**: per-window SCKit capture → `hstack`
2-pane composite (TV-left / phone-right, 60fps, dual title-bar crop) → cursor-hide + gold
tap-ripple → AV1/HEVC dual-encode. Content bugs #25 (Kaos swap) + #26 (portraits) fixed.
Capture script reaps the orphaned emulator/chromedriver after each run.

**Two videos ship off this one pipeline** — it's already scenario-keyed (a `Narrative` =
name + flavor + beats; capture/render/composite/caption are generic, untouched), so a second
video is new beats + one registry entry, not new infra:
- **Video 1 — the Hook** (`ingame`, real RPCS3): the ~30-60s marquee — scan, the figure's
  IN the game, no toy touched. Captured + approved 2026-06-24; it just needs its captions
  rewritten in my voice (A.7).
- **Video 2 — the Tour** (`walkthrough`, NEW, mock-driver): the full feature walk —
  profiles, search, the portal, the admin menu, takeover. Self-contained (no RPCS3, no NFC),
  re-records on any Mac (A.8).
Both then embed on the site (A.6).

## What Changes

- Shipped: A.7, A.9 and A.8's build boxes (A.8.1, A.8.3–A.8.5, A.8.7–A.8.12), all on 2026-06-28: `6fe9491` (Hook voice pass, Tour narrative, review cut), `647143e` + `e96b0a2` (remove/takeover/ownership beats), `ec0928f` (`--reconfigure`), `1f6e326` (farewell closer), `97f82d7` (wizard opener + `render-concat`).
  - `walkthrough` is registered at `tools/playthrough/src/beats.rs:1530-1554` as 19 beats on `ServerFlavor::IpcCold`, and `registry_builds_and_validates` (`beats.rs:1638`) pins the name list.
  - A.9's raw + `timeline.json` land in `tools/playthrough/out/` (gitignored, `.gitignore:82`); the tuning loop and `-- render-review` are written up in `docs/dev/recorder-beats-framework.md` §5.1 (`:228-257`), with the CLI line in §6 (`:272-273`).
  - A.8.10 is the only box that ships: `--reconfigure` (`crates/server/src/main.rs:49-54`, documented at `docs/setup.md:33`) reached users in v1.9.21.
  - A.8.1 is ticked but over-claims. It lists a repose-cycle beat and a resume-modal beat, and neither exists: both were dropped as fluff (`docs/dev/demo-reel-captions.md:19`, `:97`). Reposes survive only as a search caption and the `appearance_swap` beat (`beats.rs:1316`).
- A.6 is done but unticked (all four children are ticked).
  - `58613a3` (2026-06-28) retired GitHub Pages (the Pages API now 404s, `has_pages: false`), deleted the Jekyll infra and the three showcase pages, and pointed `README.md:7` and the repo homepage at <https://hotchkiss.io/pages/projects/skylander-portal-controller>.
  - That page serves both videos today, each as an HEVC + AV1 pair, plus seven stills (A.6.2): six in the screen-by-screen section and one under the Hook. The Hook's files match the byte sizes of `tools/playthrough/out/playthrough-ingame-final{,.av1}.mp4` (12,005,646 / 14,586,059 B, uploaded 2026-06-28 20:57Z).
  - A.6.1 and A.6.3 say beta.hotchkiss.io; everything points at the production host instead. The repo's `github-pages` environment (2026-04-25) outlived the deployment.
- A.8.2 (OPEN) is done. `647143e` delivered its two new parts under A.8.8/A.8.9's names, and the third was in place before the box was written.
  - `BeatCtx.bob` (`beats.rs:44-50`) drives a headless second phone through `ctx.server.chromedriver_url` (`beats.rs:934`), bound with `set_session_profile` (`crates/e2e-tests/src/lib.rs:1002`) and placing via a real IPC LOAD.
  - `takeover` calls `fire_takeover` (`beats.rs:995` → `lib.rs:1107`). Both harness hooks predate A.8 (`set_session_profile` 2026-04-15, `fire_takeover` 2026-04-24), so `647143e` wired beats to them without touching `lib.rs`.
  - The Kaos beat has called `fire_kaos_swap` (`beats.rs:419` → `lib.rs:1087`) since `e426cd5` (2026-06-23, A.3).
- A.8.6 (OPEN, chotchki's call): the Tour it asks him to review is already public. hotchkiss.io's "full run-through" serves 18,167,574 B HEVC + 24,495,294 B AV1, uploaded 2026-06-28 23:19Z. Those are exactly the sizes of `playthrough-tour-final{,.av1}.mp4`, rendered 16 minutes earlier, and `6fe9491` already filed one iterate fix under A.8.6 (the tap-ripple early-clear). Either that publish was the sign-off (tick A.8.6, then A.8), or the review is still owed on a video that's already live.
- The preamble went stale on its own Tour line ("mock-driver … Self-contained (no RPCS3, no NFC), re-records on any Mac"). chotchki moved the Tour to real RPCS3 on 2026-06-28 (A.8's header, `beats.rs:1532`), so a re-record needs the patched RPCS3, the firmware pack and a Giants (BLUS30968) dump on the Mac.
- Docs drift that no box owns:
  - `docs/dev/recorder-beats-framework.md`, the owning doc, has no word on the Tour. Its Status header (`:3-7`) stops at A.5, `ServerFlavor` (`:80`) lacks `IpcCold`, and the narrative list (`:111-114`) lacks `lifecycle`, `ingame-savestate` and `walkthrough`. §4 still describes the old single-desktop Windows capture.
  - `docs/dev/demo-reel-captions.md:16-17` locks `install` as a static title card. `97f82d7` replaced that with a captured run of the real wizard (`capture-wizard.sh`).
- CI never builds the reel on macOS, the OS it's captured on.
  - The 65 unit tests in `tools/playthrough/src/` run in CI only on the Windows lane (`screencapturekit` is macOS-gated, `tools/playthrough/Cargo.toml:28-29`).
  - The macOS CI lane has been `if: false` since 2026-05-30 (`ci.yml:151-155`), and `release.yml` excludes the crate from its test step (`:89`, `:450`, `a86935d`).
  - So the capture path is checked only by `.githooks/pre-push` on chotchki's Mac. `ci.yml:144-147` says macos-15's SDK is enough for `apple-metal`, and `release.yml:85-88` says it isn't. The `ci.yml` comment is the stale one: `f9142c6` wrote it with the macos-14 → macos-15 bump (2026-06-25), then v1.9.13 failed the same `apple-metal` build on macos-15 (run 28238672102) and `a86935d` excluded the crate.
- A then reduces to chotchki's ruling on A.8.6, ticking A.6 and A.8.2 (and A.8 after A.8.6), and a docs box for the two drifted docs (plus the stale `ci.yml` comment). No box exists for the docs yet.

## Capabilities

### New Capabilities

None declared yet. This change came over from PLAN.md on 2026-10-17 as a task list, not a spec, so `.openspec.yaml` carries `skip_specs: true`. When work resumes on a box that changes behavior, name the capability here, write its delta under `specs/` and drop the flag.

### Modified Capabilities

None yet.

## Impact

- Code:
  - `tools/playthrough/`: `beats.rs` (registry, captions, editorial speed fields), `main.rs`, `render.rs` (composite, review cut, `render-concat`), `caption.rs`, `capture.rs`, `screen.rs` (classifier gates), plus `capture-ingame-embed.sh`, `capture-wizard.sh` and `run.sh`.
  - `crates/e2e-tests/src/lib.rs`: the harness hooks the beats call.
  - `crates/server/src/main.rs` + `config.rs`: `--reconfigure`.
- Tests: the 65 playthrough unit tests (`beats.rs` 7, `main.rs` 16, `render.rs` 24, `screen.rs` 6, `stage.rs` 5, `timeline.rs` 5, `caption.rs` 2) and `ec0928f`'s `reconfigure_forces_the_wizard` (`crates/server/src/config.rs:534`). Capture itself is verified by eye on the rendered reel, not by a test.
- Source of truth: `docs/dev/recorder-beats-framework.md` (framework + tuning loop, stale on the Tour), `docs/dev/demo-reel-captions.md` (the caption script; `FINAL:` lines are baked into `beats.rs`), `docs/dev/recorder-macos-capture.md` (the SCKit backend), `docs/setup.md` (`--reconfigure`).
- Track: product. The reel films the product binary driving the patched RPCS3.
- Release reach:
  - The recorder never ships. `default-members` is `crates/server` (`Cargo.toml:24`) and both release lanes build `-p skylander-server`.
  - Only A.8.10 reached users (v1.9.21).
  - The videos live off-repo on hotchkiss.io (no `.mp4` is tracked), so what's left needs no tag and archives from main under the archive rule.
- One-way doors: none. Pages can be re-enabled, and until then the old chotchki.github.io links 404.
- Guacamole:
  - Both videos film the RPCS3 wrapper, which leaves what ships at G1, so from then on they show a retired architecture. Nothing in A re-shoots them against `guacamole`.
  - The `IpcCold` recorder needs the same patched RPCS3 + Giants dump that Guacamole keeps as its dev oracle until G2.
  - One trap: `link_repo_images` (`crates/e2e-tests/src/lib.rs:403-406`) symlinks the repo's `data/images/` (the fandom portraits) into every test server's temp data root, and returns silently when the folder is missing. Once AD retires the wiki data, every e2e run and reel capture renders blank bezels (bug #26 again) unless the server serves extracted portraits by then.
