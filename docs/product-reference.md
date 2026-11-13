# Product reference (the frozen controller)

Working notes for the shipped product: the phone's look and CSS policy, the Kaos feature, macOS support, distribution and the legacy UIA driver's gotchas. Moved out of [`CLAUDE.md`](../CLAUDE.md) on 2026-11-13. The product is frozen (2026-10-17) and changes only when Guacamole needs it or something breaks, so these sections are read rarely; the project instructions keep what both binaries share and point here. The phone carries into Guacamole, so the Aesthetic section still binds any phone work.

## Aesthetic

- Match Skylanders game UI: starfield blue backgrounds, circular gold-bezeled figure portraits, bold white titles with gold outline, cartoony feel. Reference: `docs/aesthetic/ui_style_example.png`.
- Implement via CSS (wiki asset resolution isn't enough for high-res phones).
- Phone UI is theme-able (prepping for the Kaos "mind magic" takeover skin — dark purple/pink).
- **Launcher badge is 3D** (PLAN 10.7). The QR card / back-face title cards / orbiting player pips all share one GL pipeline (`crates/server/src/badge.rs::BadgeRig`): cylinder coin geometry (front fan + back fan + side wall + flatter elliptical torus), Lambert diffuse + Blinn-Phong specular, multi-turn intro spin via `LaunchPhase::badge_rotation_y`, 360° flip-on-state-change for back-face text swaps. Each unique front-face texture (QR raster, per-`BackFace` text, per-profile pip face) is rasterised once via `badge_text` (TitanOne via `ab_glyph`, faux-emboss in 3 glyph passes) and cached as a GL texture. Surfaces other than Main (Crashed / Farewell / ServerError) route through `paint_centered_3d_back_card` so the visual language is consistent across screens. The legacy 2D `paint_titled_card` / `paint_bezel` family is gone.
- **Mocks live in `docs/aesthetic/mocks/`** as standalone HTML files — open directly or via a local server. `docs/aesthetic/mocks/index.html` lists every mock grouped by flow.
- **Review mocks on a real iPhone, not just desktop preview.** Safe-area insets, Dynamic Island collisions, mobile Safari viewport behavior (address-bar hiding, pinch-zoom, orientation lock) all differ from desktop devtools. Serve with `python3 -m http.server 8089` from `docs/aesthetic/mocks/` and open `http://<mac-en0-ip>:8089/` on the iPhone (`ipconfig getifaddr en0` to find the IP). Requires the iPhone and Mac to share a network — Mac-as-hotspot (Internet Sharing) works; iPhone-as-hotspot blocks incoming connections to the Mac.
- **Safe-area pattern for top-of-screen padding:** `max(Npx, calc(env(safe-area-inset-top) + 12px))` where N is the desktop-preview value. Preserves desktop look; adapts on devices with a notch/island. Same pattern applies to L/R when content hugs screen edges.
- **Claude-driven iOS repro ([`ios-inspect`](https://github.com/chotchki/ios-inspect)).** Mac-only CLI that drives the iOS Simulator + Safari via `xcrun simctl` and the WebKit Web Inspector protocol (through `ios-webkit-debug-proxy`). Use it for **layout/CSS probes** — `safe-area-inset-*` values, computed styles on specific selectors, DOM-subtree dumps, web-content-only screenshots. Use the chromedriver e2e harness for **functional regressions** that don't depend on iOS-specific rendering. One-shot bringup from the repo root: `cargo install --git https://github.com/chotchki/ios-inspect --tag v0.0.1 && (cd phone && trunk serve --address 0.0.0.0 --port 8090 &) && ios-inspect boot && ios-inspect open http://$(ipconfig getifaddr en0):8090/`. Then iterate with `ios-inspect eval '<js>'`, `ios-inspect computed-style <selector> --filter …`, `ios-inspect screenshot --web-only -o /tmp/x.png`. Proxy lifecycle is self-healing — no manual restart mid-session. **Sim fidelity gap to remember**: non-standalone sim Safari reports `safe-area-inset-bottom = 0` (vs ~34px on real Dynamic Island hardware); bugs that depend on the bottom inset need PWA-standalone mode (Add-to-Home-Screen in the sim) or a real-device fallback. See the [`ios-inspect` repo](https://github.com/chotchki/ios-inspect) README for full usage + limitations.

### CSS escape-hatch policy (PLAN 9.5)

The phone is on Tailwind v4 (PLAN 9.x). When adding or editing styles, default to inline utilities; reach for an `@apply` rule only when readability suffers; reach for raw CSS only for things Tailwind can't shorten. Three tiers:

- **Inline utilities (default).** Layout (`flex`, `grid`, `absolute`), spacing (`p-4`, `gap-3`), typography (`font-display`, `text-body-italic`), colour (`text-gold-bright`, `bg-sf-1`), single-layer effects (`rounded-xl`, `shadow-sm`). Put them on the element in the Leptos `view! {}` macro. This is the home of 95%+ of styling.

- **`@apply` in a per-component CSS file.** Reach for this when an inline class string would exceed roughly **12 utility classes on a single element**, when the same cluster repeats across siblings (an action button group, a row of swatches), or when the element uses pseudo-element trickery (`::before` / `::after`) that needs to share styling vocabulary with the parent's utilities. Files live at `phone/styles/components/<component>.css`, are imported from `phone/styles/input.css`, and are wrapped in `@layer components { … }` so they slot beneath utility specificity.

- **Raw CSS (rare).** Reserve for things Tailwind genuinely can't help with: `@keyframes` (named animations), `@font-face` (font loading), multi-layer pseudo-element decoration with stacked-gradient `background-image`, `:has()` selectors driving cross-component layout, masked SVG silhouettes (`-webkit-mask: url(…)`), `mix-blend-mode` overlays, complex shadow stacks beyond what `shadow-[…]` arbitrary values reasonably express. Co-locate raw CSS with the component that uses it (next to its `@apply` block in the same file), not in a global stylesheet.

The migration's reference exemplars: `gold_bezel.css` (mixes all three tiers — `@apply` for layout primitives, raw `box-shadow` stacks + radial gradients), `kaos_overlay.css` (raw-heavy: six stacked layers, seven keyframes, masked SVG sigil), `framed_panel.css` (a clean two-rule file with one `::before` decoration). When in doubt, look at how a similar component handled it.

Hidden costs to keep in mind: every `@apply` in a component file means class names that don't appear inline on the element, which makes the element harder to grep for visually. Prefer `@apply` only when the readability gain on the markup side outweighs that. The 12-class rule of thumb is just a heuristic — a 6-class string with three repetitions across siblings is more @apply-worthy than an unrepeated 14-class one-off.

## Kaos feature (LAST — post-MVP)

- Wall-clock timer: 20-min warmup, then random within every hour window.
- Text-only overlay (Kaos catchphrases from wiki — text avoids copyright issues).
- 1-for-1 swap of a portal figure with a randomly-chosen compatible figure from the owned collection.
- Compatibility rule (heuristic, can enhance later): figure works in its game of origin and all later games, with known exceptions (vehicles only in SuperChargers, etc.).
- Reposes: collapsed in browse view with a "N variants" badge. Cycle button on card for variant swap.

## macOS support

- **Production target on both Windows and macOS.** Both drive a real
  patched RPCS3 over the cross-platform AF_UNIX **IPC** path
  (`IpcPortalDriver`); UIA is the Windows-only fallback for a *stock*
  RPCS3. The Phase-16 IPC pivot removed the Windows lock-in, so the
  same control path works on macOS — `UnixRpcsProcess` is wired in
  (PLAN 16.11) and `SKYLANDER_PORTAL_DRIVER=ipc` spawns/drives the
  patched Mac binary (build: `.ci-local/build-mac.sh`; run:
  `docs/dev/macos-rpcs3-build.md`). There is **no AXUIElement-based
  driver** — that's an explicit non-goal; IPC supersedes it.
  macOS status (Phase U, shipped v1.9.19+): (1) the **shipped Mac
  release BUNDLES the patched RPCS3** (nested in the signed `.app`,
  pulled from `rpcs3-patched-macos-arm64`) and **defaults to IPC** — the
  wizard persists `Ipc`, so a fresh install drives the real emulator.
  `SKYLANDER_PORTAL_DRIVER=mock` is the explicit **demo-mode** override
  (in-memory portal: demo, family-member play, iOS-Sim e2e, no
  firmware). **Dev `cargo run` mirrors prod** — the compile-time default
  is IPC on both shipping targets (Win + macOS); only Linux/non-target
  builds default to Mock (`config.rs` cfg-gate). `.env.dev` sets `=ipc`,
  `.env.dev.mock` sets `=mock` for a no-emulator demo. (2) **window
  coordination on macOS is surface-embed** (P8 /
  `crates/server/src/compositor.rs` — the game's `CALayer` is hosted
  INSIDE the launcher window via `CALayerHost`, one window, egui frames a
  rect around it), NOT the Win32 `SetWindowPos` sibling-window path. Mac
  users get the same SPA + launcher surface, now driving a real game.
- Dev: `cargo run -p skylander-server` boots cleanly with
  `SKYLANDER_PORTAL_DRIVER=mock` + a sentinel `RPCS3_EXE` path. The QR
  URL uses Bonjour (`<LocalHostName>.local`, read via `scutil --get
  LocalHostName`) so PWA bookmarks survive DHCP changes the same way
  they do on Windows.
- Full dev bringup steps + expected log lines:
  `docs/dev/macos-bringup.md`.
- iOS Simulator iteration: the [`ios-inspect` repo](https://github.com/chotchki/ios-inspect). The CLI
  drives `xcrun simctl` + `ios-webkit-debug-proxy` for layout/CSS
  probes against booted simulators; multi-device (iPad + iPhone)
  in parallel is supported (PLAN 10.2 — `ios-inspect boot
  --device <udid>` per simulator, then `--device` on every
  read/eval/screenshot).
- Release artifact: a **signed + notarized + stapled `.dmg`** (a nested
  `.app` bundling the patched RPCS3), attached to GitHub Releases
  alongside the Windows zip (PLAN 10.9.2/.4 — Apple Developer ID import +
  `xcrun notarytool` + `stapler`, live as of **v1.9.19**). Gatekeeper
  opens it on a normal double-click; no right-click + Open dance.

## Distribution

- GitHub Releases zip + winget MSI. Do **not** bundle `.sky` files or game/firmware
  content (no piracy).
- **Users supply their own** PS3 firmware + game backups, via their **existing RPCS3
  install** (the v1 model, below).
- **RPCS3 is vendored + patched (Phase 16).** `vendor/rpcs3` is a git submodule pinned
  to a pristine upstream commit; the IPC patches — **P1** (AF_UNIX portal control on the
  emulated Skylander device) and **P2** (borderless, no-focus-steal game window +
  native-handle publishing) — live in `rpcs3-patches/` (apply via
  `rpcs3-patches/apply.sh`; README has the pin + rebase procedure). This makes the repo
  **GPL-2.0-only** (RPCS3 is GPLv2; the patches are a derivative).
- **v1 distribution model (decided 2026-05-30 — PLAN 16.9.0b).** The IPC control path
  needs *our patched* RPCS3 (stock RPCS3 has no IPC listener), which users can't supply
  themselves, so the Windows release **bundles the patched RPCS3 build** next to the app
  at `<app>/rpcs3/rpcs3.exe` (legal under GPL — source is public; the binary comes from
  the gated `rpcs3-patched.yml` Windows build). For **firmware + games**, the user still
  **points the first-launch wizard at their existing RPCS3 install**; that becomes
  `config_dir` (RPCS3's data/config root — installed firmware + `config/games.yml`),
  decoupled from the bundled control binary (the `config_dir` decoupling, 16.9.0). So v1
  ships no firmware/games and writes no RPCS3 config (16.9.1/.2 deferred). **macOS now ships
  the same way** (Phase U, v1.9.19+): the signed/notarized `.dmg` bundles the patched RPCS3
  (nested in the `.app`) and defaults to IPC; firmware + games still come from the user's own
  RPCS3 install. The mock driver stays as the `SKYLANDER_PORTAL_DRIVER=mock` demo-mode override.
- Steam Big Picture shell behavior is a compatibility-pass concern, not a day-1 constraint.

## RPCS3 window/menu gotchas (UIA fallback — see `docs/research/game-launch-window-mgmt.md`)

> These apply to the **legacy UIA driver** (`SKYLANDER_PORTAL_DRIVER=uia`, stock RPCS3).
> The production IPC path (Phase 16, patched RPCS3) has **no Skylanders Manager dialog and
> no menu nav** — it drives `g_skyportal` directly over AF_UNIX and reads liveness from the
> `STATE`/heartbeat, so most of these don't apply there. Kept because UIA is the fallback.

- While a game runs, RPCS3 has **multiple top-level Qt windows that all share the same generic class** `Qt6110QWindowIcon` — main (title prefix `"RPCS3 "`), game viewport (title prefix `"FPS:"`), and the Skylanders Manager dialog (title `"Skylanders Manager"`) are only distinguishable by **title**, never by classname. Anything that grabs windows by class will mis-match. Older RPCS3 builds had distinct classes (`main_window`, `skylander_dialog`); current Qt 6.11 unified them.
- **UIA pattern menu nav works on current Qt 6 RPCS3** (verified 2026-05-03 against `0.0.40-19296` via `tools/uia-probe`). Every `MenuItem` advertises `Invoke`, `ExpandCollapse`, `LegacyIAccessible`, and `Value`. The Manage → Portals and Gates → Skylanders Portal chain reduces to three pattern calls — no keystroke synthesis, no focus dependency, no Steam Overlay competition. Submenus populate lazily on `ExpandCollapse.expand()`, so re-walk the tree (or wait ~100ms) before searching for child items. Older notes claimed Qt 6 didn't honour these patterns — that was true at the time, but no longer.
- **Dialog opens once per RPCS3 session** — `open_dialog` navigates the menu on first call, then keeps the Skylanders Manager off-screen for the rest of the session. Subsequent calls short-circuit. If RPCS3 restarts, first `open_dialog` re-does the nav (brief once-per-session flicker during boot).
- **Modal vs non-modal Qt action invokes.** `UIInvokePattern.invoke()` on a menu item that triggers a *non-modal* dialog (like Skylanders Manager) returns immediately. On an item that triggers a *modal* (like File → Boot Game's QFileDialog), the call **blocks until the modal closes** — Qt's `exec()` event loop runs synchronously. Don't drive modal-popping menu items from the main thread without a worker.
- **Focus thieves are no longer a problem for menu nav** (UIA patterns don't need focus). Still relevant for any remaining keystroke-driven path: the game viewport, RPCS3's **update-check popup** at boot — tell users to disable Settings → Advanced → "Automatically check for updates at startup".
- `RPCS3.buf` singleton lockfile next to `rpcs3.exe` survives forced kill → next launch fails. `RpcsProcess::shutdown_graceful` deletes it after the `Forced` path. Spawned processes are also wrapped in a Win32 Job Object with `KILL_ON_JOB_CLOSE` so RPCS3's re-exec shims and worker children don't leak across test runs.
- **Booting a game programmatically:** pass the game's `EBOOT.BIN` to `rpcs3.exe` as the first CLI argument (`UiaRpcsProcess::launch_with_eboot`). Direct-boot used to break menu-bar nav via synthesised keystrokes — that caveat was keystroke-specific and no longer applies now that the menu nav is pattern-driven. The serial → game directory map lives in `<rpcs3>/config/games.yml` (flat YAML); EBOOT.BIN is at `<game_dir>/PS3_GAME/USRDIR/EBOOT.BIN`. The library-view + select+Enter approach is retired (PLAN 10.8.4) — fragile under load and didn't survive cell-vs-row selection ambiguity.
- **Session isolation:** all UIA + SendInput automation is session-bound. Tests that exercise the real driver must run on the user's interactive desktop — SSH connects in session 0 and cannot see/touch windows in session 2+ at all. `RpcsProcess` launches, `EnumWindows`, UIA tree walk all return empty under SSH. Run RPCS3-live tests on the physical machine.
