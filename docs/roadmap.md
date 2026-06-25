---
layout: page
title: Roadmap
permalink: /roadmap/
---

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

## Phase 5 — Kaos (shipped in v1.1.0; refined in v1.4.x)

The surprise feature, gated behind a per-profile opt-in toggle. (Originally on the kebab menu; v1.4.x moved it to the per-profile EDIT screen behind the manage-profiles Konami gate, where profile-scoped preferences belong.) A wall-clock timer fires after a 20-minute warmup, then re-arms at uniformly random offsets between 1 minute and 1 hour. Each fire swaps a random placed figure for a compatibility-aware random pick from the owned collection (vehicles only get pulled in for SuperChargers, figures only swap into games at or after their game-of-origin). Text-only catchphrase overlay sticks until the user holds the "hold to dismiss" pill — the v1.1 auto-dismiss timer was short-circuiting kids before they finished reading the insult. The taunt also routes through the v1.1.0 ghost-session replay buffer, so a backgrounded phone catches it on reconnect.

## Phase 6 — Post-Kaos polish (mostly shipped; per-kind decoders open)

Known papercuts worth fixing but not blocking release. The `.sky` firmware parser for the core "figure" kind shipped in v1.1.x and surfaces level, XP, and gold on the phone's figure-detail screen. Per-kind decoders for traps (captured villain identity), vehicles (SuperChargers SSCR level), and Imaginators creation crystals (class / element / nickname) are scoped and waiting on a UI determination pass — the wire path is already in place. NFC-scan import of physical figures via a separate scanner tool landed; tag-identity dedup follow-ups are open. Window-Z-order work to suppress the brief RPCS3 flicker during menu navigation is also still open.

## Phase 7 — Packaging and release (done in v1.0.0)

Single-exe distribution. The phone SPA, images, figure metadata, fonts, and icons all embed into the binary via `rust-embed`. A GitHub Actions workflow on version-tag push builds the Windows release exe with `dev-tools` stripped (release builds enforce HMAC signing and don't expose the dev log endpoint), runs the fast test suite, and attaches a zip to a draft release. v1.0.0 was cut on April 25, 2026; v1.1.0 followed the same day with the Phase 8 features.

## Phase 8 — Sticky disconnects, Kaos, kid playtest fixes (shipped in v1.1.0)

The first round of changes after a real session in front of the kids. Ghost sessions: a phone backgrounding no longer clears the user's portal — figures stay placed for an hour or until a reclaim, with a 32-event replay buffer catching up the phone on what landed during the gap. Kickback cooldown countdown so the takeover screen's "kick back" button reads honestly. The Kaos feature itself (Phase 5 above). Empty-portal-slot cleanup so kids don't tap inert bezels expecting them to do something. Round-trip ghost/reclaim test pins the contract. v1.1.0 packages it all.

## Phase 9 — Tailwind v4 + iPad/iPhone responsive pass (shipped in v1.4.x)

The phone CSS rewrite that 1.x deferred. The legacy ~5800-line monolithic stylesheet got replaced by a Tailwind v4 utility-first build (cached CLI downloader, Trunk pre_build hook, `@theme`-driven design tokens, per-component CSS files). Rem-based typography with a tablet root-size bump means iPad and iPhone share one stylesheet — no separate "iPad version". The v1.4.x playtest pass on top added element-tinted bezel rings, Trap and Land/Sky/Sea badges so unfamiliar figure thumbnails read at a glance, the Kaos sigil silhouette (the asset wasn't being copied into `dist/` — sigil never actually rendered before), and a flotilla of contrast and tap-target fixes that only showed up under a real kid using a real iPad.

## Phase 10 — macOS as a first-class platform (shipped; signing tracked under Phase 14)

Mac is a production target. Server compiles + runs on macOS against the mock RPCS3 driver. The external [`ios-inspect`](https://github.com/chotchki/ios-inspect) CLI drives the iOS Simulator + WebKit Web Inspector for layout probes during development. The chromedriver e2e harness ports cleanly to macOS. The simultaneous iPad + iPhone simulator e2e lane catches multi-device product bugs that a single browser viewport hides. The release artefact ships as a tar.gz today; the Developer ID notarisation flow is wired in CI but disabled pending secret provisioning (Phase 14 tracks the bring-up). In the interim, Mac users right-click → Open to clear Gatekeeper.

## Phase 11 — Stat editing (shipped v1.5.0 → v1.6.0)

Per-figure level + gold editing from the phone, end to end. The `sky-parser` crate gained write paths for level (with the per-generation cap), gold, and an unwritten-vs-empty discriminator that v1.5.0 needed for round-trip-safe encryption — RPCS3 rejected v1.5.0's first cut because the legacy encrypt path silently dropped real ciphertext for empty-but-keyed blocks; v1.5.1 fixed it via `encrypt_figure_preserving_unwritten` using the source ciphertext as the discriminator. v1.6.0 followed with edit-sheet polish: gold capped at 65 000 (the games refuse `u16::MAX`), outer chevrons jump-to-bounds, iOS Safari text-callout suppressed, cross-phone stat refresh via the FigureUpdated WS broadcast, save toast for instant confirmation, the resume modal waits for a game to launch before offering to put figures back, and the long-disabled APPEARANCE button on figure detail finally cycles through a figure's variant cluster.

## Phase 12 — Real Mac RPCS3 driver via AXUIElement (planned)

macOS originally shipped as mock-only; Phase 11's write-path bugs proved the Mac → Windows zip round-trip is too slow for iteration. Phase 12 explores porting the Windows UIA driver to macOS's accessibility API (AXUIElement). The go/no-go gate is an exposure probe — Qt apps generally expose accessibility on both platforms, but the RPCS3 build's AX exposure is unknown until measured. Fallback paths (AppleScript, keystroke synthesis) are documented if AX doesn't pan out. Not started.

## Phase 13 — winget distribution (planned)

Windows users currently hit SmartScreen's "unknown publisher" prompt on direct MSI downloads. Phase 13 submits the MSI to `microsoft/winget-pkgs` so `winget install ChristopherHotchkiss.SkylanderPortalController` becomes the primary install path — winget bypasses the SmartScreen friction without an Authenticode cert. First submission is a manual `wingetcreate` flow on the HTPC; subsequent releases auto-PR via a CI step. Runbook in `docs/dev/winget-submission.md`.

## Phase 14 — macOS code-signing bring-up (planned)

Activate the wired-but-disabled Developer ID signing + notarisation pipeline so Gatekeeper accepts the `.dmg` without the right-click → Open dance. Walks through provisioning the Apple Developer cert, generating the seven GitHub Environment secrets, re-enabling the four currently-`if: false` signing steps in `release.yml`, and validating with a dry-run tag. Runbook in `docs/dev/release-signing.md`.

## Beyond Phase 14 — open

Service-worker for PWA cache + update detection (so an iOS Add-to-Home-Screen survives long backgrounding), the rest of the Phase 6 per-kind `.sky` stat decoders (traps / vehicles / creation crystals), demo harness for screen recording. None of these block daily use. Detailed checklist lives in `PLAN.md` in the repo.

## Non-goals

- No bundling of RPCS3, game ISOs, or figure firmware.
- No Linux support.
- No live wiki scraping at runtime — figure data is committed to the repo.
- No audio (text-only Kaos to dodge copyright).
- No user-entered figure names.
