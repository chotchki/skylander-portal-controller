---
layout: page
title: Roadmap
permalink: /roadmap/
---

# Roadmap

High-level phasing. Detailed checklists live in [PLAN.md](https://github.com/chotchki/skylander-portal-controller/blob/main/PLAN.md) and [PLAN_ARCHIVE.md](https://github.com/chotchki/skylander-portal-controller/blob/main/PLAN_ARCHIVE.md) in the repo.

## Phase 0 — Scaffolding (done)

Cargo workspace skeleton, baseline `.gitignore`, research-output folders, and the first `cargo run` smoke. Nothing user-facing; just confirmed the tools were installed.

## Phase 1 — Research spikes (done)

Answered the unknowns that could have killed the project before it started: whether RPCS3's Qt dialog could be driven programmatically (yes, via Windows UI Automation plus synthesised keystrokes for Qt menus), whether the off-screen hide trick actually worked (yes, via Win32 `SetWindowPos`), whether the Skylanders Fandom wiki could seed a figure catalogue at acceptable hit rates (yes, 504 of 504), and whether Axum, Leptos, and egui could coexist in one Rust binary on Windows (yes). Each spike produced a short writeup under `docs/research/`.

## Phase 2 — Minimal end-to-end slice (done)

Stripped down to the smallest useful thing: one phone, no profiles, no PINs, no game launching. RPCS3 already running with the Skylanders Manager open. Phone connects, fetches the figure list, taps a slot, taps a figure, figure loads into the emulated portal. Proved the architecture and gave Phase 3 a real system to iterate on instead of a design document.

## Phase 3 — Testing infrastructure, profiles, game launching (done)

Added a real end-to-end test harness using fantoccini against ChromeDriver before doing any more feature work, so every subsequent bug could land as a named regression test. Built out RPCS3 process lifecycle management — launching a game, shutting it down gracefully, recovering from crashes — and the game-picker UI. Introduced up-to-four profiles with PIN gating, working-copy semantics for firmware files, resume-last-setup on unlock, and multi-phone takeover with FIFO eviction.

## Phase 4 — Aesthetic and UX pass (done)

Rewrote the phone UI to match the Skylanders game aesthetic: gold-bezeled circular figure portraits, starfield background, bold titles with gold outline, Titan One type. Added safe-area handling for iPhones with a notch, service-worker scaffolding toward PWA install, toy-box-lid interaction for the collection overlay, and the egui launcher's cloud + iris boot animations. Per-game display-mode persistence so RPCS3 launches don't trigger a TV resolution flicker. Per-slot ownership chips when two phones are co-op'ing.

## Phase 5 — Kaos (shipped in v1.1.0)

The surprise feature, gated behind a per-profile opt-in toggle in the kebab menu. A wall-clock timer fires after a 20-minute warmup, then re-arms at uniformly random offsets between 1 minute and 1 hour. Each fire swaps a random placed figure for a compatibility-aware random pick from the owned collection (vehicles only get pulled in for SuperChargers, figures only swap into games at or after their game-of-origin). Text-only catchphrase overlay auto-dismisses after about five seconds. The taunt also routes through the v1.1.0 ghost-session replay buffer, so a backgrounded phone catches it on reconnect.

## Phase 6 — Post-Kaos polish (in progress)

Known papercuts worth fixing but not blocking release. Window-Z-order work to suppress the RPCS3 flicker during menu navigation. A `.sky` firmware parser to surface per-figure stats (level, XP, gold, current hat, quest progress) on the phone — the wire path is in place; per-kind decoders for traps / vehicles / creation-crystals are still landing. NFC-scan import of physical figures via a scanner tool (already landed; tag-identity dedup follow-ups open). A demo harness for screen recording.

## Phase 7 — Packaging and release (done in v1.0.0)

Single-exe distribution. The phone SPA, images, figure metadata, fonts, and icons all embed into the binary via `rust-embed`. A GitHub Actions workflow on version-tag push builds the Windows release exe with `dev-tools` stripped (release builds enforce HMAC signing and don't expose the dev log endpoint), runs the fast test suite, and attaches a zip to a draft release. v1.0.0 was cut on April 25, 2026; v1.1.0 followed the same day with the Phase 8 features.

## Phase 8 — Sticky disconnects, Kaos, kid playtest fixes (shipped in v1.1.0)

The first round of changes after a real session in front of the kids. Ghost sessions: a phone backgrounding no longer clears the user's portal — figures stay placed for an hour or until a reclaim, with a 32-event replay buffer catching up the phone on what landed during the gap. Kickback cooldown countdown so the takeover screen's "kick back" button reads honestly. The Kaos feature itself (Phase 5 above). Empty-portal-slot cleanup so kids don't tap inert bezels expecting them to do something. Round-trip ghost/reclaim test pins the contract. v1.1.0 packages it all.

## Phase 9 and beyond — open

CSS modularization to support iPad alongside the phone (post-1.0 commitment), service-worker for PWA cache + update detection, demo harness for screen recording, and the rest of the Phase 6 stat-decoding pass. None of these block daily use. Detailed checklist lives in `PLAN.md` in the repo.

## Non-goals

- No bundling of RPCS3, game ISOs, or figure firmware.
- No Linux or macOS support.
- No live wiki scraping at runtime — figure data is committed to the repo.
- No audio (text-only Kaos to dodge copyright).
- No user-entered figure names.
