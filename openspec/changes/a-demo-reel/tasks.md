# Tasks

## A. Auto-generated demo reel (macOS) — LIVE

- [ ] A.6 Site / README embed — both videos (was 15.7)
  - [x] A.6.1 Finalize both MP4s (Hook + Tour, AV1+HEVC) as OFF-REPO artifacts — host on beta.hotchkiss.io, NOT committed to git (40MB+ would bloat history forever)
  - [x] A.6.2 Generate per-feature stills (ffmpeg -ss from the Tour timeline.json beat boundaries) as artifacts for the personal-site gallery — no Jekyll
  - [x] A.6.3 Wire Hook + Tour into beta.hotchkiss.io/pages/projects/skylander-portal-controller; repo README gets a 'Watch the demo' link out (no github.io)
  - [x] A.6.4 Drop GitHub Pages: disable the live deployment (now serving main:/docs at chotchki.github.io), remove Jekyll infra (docs/_config.yml, _layouts/, _includes/, assets/main.scss), de-Jekyllify or delete the 6 front-matter site pages (about/features/index/roadmap/setup/tour), strip stray liquid from dev/research docs, repoint README's github.io link
- [x] A.7 **Video 1 (the Hook) — voice pass on the `ingame` marquee.** Captions ARE the script; pipeline needs no changes (edit → re-capture → render).
  - [x] A.7.1 Rewrite the captioned `ingame` beats (`connect`, `pick_profile`, `pick_game`, `open_toybox`, `place_figure_ipc`, `see_in_game`, `kaos`) in chotchki voice — verdict-first, one CAPS word per beat, parenthetical asides; fix the tap-to-add vs drag-drop wording
  - [x] A.7.2 Minor flow/timing tweaks if the re-watch flags them — `beats.rs` editorial fields only (`realtime_head`/`realtime_tail`/`filler_speed`/`crop`, beat order)
  - [x] A.7.3 Re-capture `ingame` (real RPCS3, Mac) + render; chotchki sign-off on the Hook
- [ ] A.8 Video 2 (Tour) — comprehensive feature walkthrough (`walkthrough`, IpcCold / real RPCS3)
  - [x] A.8.1 Author the new `beat_*` drive fns for screens the `ingame` narrative doesn't cover: create-profile wizard, search + GAMES/ELEMENTS/CATEGORY filters, repose "N variants" cycle, figure-detail APPEARANCE swap, slot REMOVE, resume modal, kebab "INVITE A PLAYER" join-QR, Konami admin tour (delete hold-confirm, PIN-reset, window-mode + 2× toggles). REUSE the existing `ingame` beats for the spine (`connect`, `pick_profile`, `pick_game_ipc_cold` + classifier nav, `place_figure_ipc`, `see_in_game`, `kaos`)
  - [ ] A.8.2 Harness wiring for beats today's `BeatCtx` can't reach: a second profile (`bob`) placing via REAL IPC for ownership pips, a non-captured background session (or `fire_takeover` test-hook) for the 3rd-conn takeover, `fire_kaos_swap` for the Kaos beat — extend `BeatCtx` / `crates/e2e-tests/src/lib.rs`. Single captured phone, NO composite change
  - [x] A.8.3 Register `Narrative{name: "walkthrough", flavor: IpcCold, beats}` in narratives(); update registry name-list test
  - [x] A.8.4 Voice captions on every Tour beat (full voice + tics; short = a caption budget, NOT a register downshift); tap-to-add not drag-drop. Shared-spine beats may need Tour-specific caption variants vs the Hook (separate constructors or a caption override) — the Hook is punchy, the Tour explains
  - [x] A.8.5 Capture + render the Tour via `capture-ingame-embed.sh` on the Mac (real RPCS3 env: `RPCS3_EXE`/`RPCS3_CONFIG_DIR`/`FIRMWARE_PACK_ROOT` + classifier gates; build `test-hooks` + `sky-stats`); it's a long live session — sequence beats so an RPCS3 crash is recoverable; verify collection populated, captions timed, 2-pane composite clean
  - [ ] A.8.6 chotchki review pass on the rendered Tour; iterate beats/captions
  - [x] A.8.7 `remove` beat — tap a placed figure → REMOVE
  - [x] A.8.8 `takeover` beat — fire_takeover evicts the captured (oldest) session → Kaos 'taken over' screen + kick-back hold
  - [x] A.8.9 `ownership` pips — inject a 2nd profile (Bob); a headless non-captured phone binds via set_session_profile + places via REAL IPC so the captured phone shows per-slot ownership pips. Extend BeatCtx (bob id + chromedriver url)
  - [x] A.8.10 Mac product gap (chotchki) — easy first-launch wizard re-trigger: a --reconfigure flag that runs the wizard ignoring existing config (today you hand-delete config.json); doc it. Serves the `install` beat too
  - [x] A.8.11 `install` beat — boot via --reconfigure into the egui wizard, capture it as a standalone clip, splice into the Tour at render (new render-concat capability)
  - [x] A.8.12 `farewell` beat — graceful-shutdown egui Farewell surface as the in-narrative closer (trigger shutdown + hold the launcher pane; no phone drive)
- [x] A.9 **Speed-tuning workflow — durable raw + readable timestamps → fast re-render loop.** The raw (un-sped) capture + `timeline.json` already get written, and `render` re-reads the manifest (editing the speed fields + re-running `render` needs NO re-capture, NO recompile). Make that loop ergonomic so the speed-ups get tuned by timestamp.
  - [x] A.9.1 Persist the raw mp4 + `timeline.json` to a stable, known, gitignored dir (not `$TMPDIR`) so the tuning loop + `render` have a fixed input path.
  - [x] A.9.2 **Burned beat-name "review" cut (chotchki picked B, 2026-06-28).** A `render`-review mode that re-emits the raw at 1× (no speed ramps) with each beat's NAME banner-ed on screen for its raw `[t_start,t_end]` window (reuse `caption::render_caption_png`); lean on QuickTime's scrubber for the timecode (this ffmpeg has no `drawtext`). One cheap H.264 encode → `<stem>-review.mp4`. NON-GOAL: a running burned timecode + the absolute `{start,end,speed}` speed-map (option C) — Backlog unless the per-beat model proves too coarse (scope: save editing rounds, NOT build an editor).
  - [x] A.9.3 Doc the loop in `docs/dev/recorder-beats-framework.md` (watch raw → tune → `render` → repeat; bake the final numbers back into `beats.rs` editorial fields so a re-capture reproduces the tuning — timeline.json tweaks are per-capture).
