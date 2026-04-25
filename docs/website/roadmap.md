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

## Phase 4 — Aesthetic and UX pass (mostly done)

Rewrote the phone UI to match the Skylanders game aesthetic: gold-bezeled circular figure portraits, starfield background, bold titles with gold outline, Titan One type. Added safe-area handling for iPhones with a notch, service-worker scaffolding toward PWA install, toy-box-lid interaction for the collection overlay, and the egui launcher's cloud + iris boot animations. Residuals — iris tuning per game state, a dedicated "booting" surface on the launcher, a handful of judgment-call tweaks on the phone — are still open.

## Phase 5 — Kaos (held for last)

The surprise feature. A wall-clock timer, 20-minute warmup then randomised hourly windows, text-only catchphrase overlay, one-for-one random compatible-figure swap on the portal driven through the same automation path as manual swaps, and a dark purple / pink "mind magic" skin for the phone UI. Deliberately held until the core app is shippable — cool features do not matter if the boring flow is broken.

## Phase 6 — Post-Kaos polish

A handful of known papercuts that are worth fixing but do not block release. Window-Z-order work to suppress the RPCS3 flicker during menu navigation. A `.sky` firmware parser to surface per-figure stats (level, XP, gold, current hat, quest progress) on the phone. NFC-scan import of physical figures via a scanner tool (already landed; follow-ups open). A demo harness for screen recording.

## Phase 7 — Packaging and release (next milestone)

This is the upcoming focus. Everything ships as one `skylander-portal.exe` with phone SPA, images, figure metadata, fonts, and icons embedded via `include_dir!` or `rust-embed`. A GitHub Actions workflow on version-tag push builds Windows release bits, runs the fast test suite (unit plus integration plus workspace build — not the `#[ignore]`-gated live RPCS3 tests), and attaches the zip to a release. A release README walks through the user-supplied bits (RPCS3 path, firmware pack path) and the first-launch wizard. Every shipped zip gets verified on a second Windows machine before the release tag is public. Trademark and IP review of any visual asset derived from Skylanders imagery happens here, not after.

## Non-goals

- No bundling of RPCS3, game ISOs, or figure firmware.
- No Linux or macOS support.
- No CI until the app works end-to-end.
- No live wiki scraping at runtime — figure data is committed to the repo.
- No audio (text-only Kaos to dodge copyright).
- No user-entered figure names.
