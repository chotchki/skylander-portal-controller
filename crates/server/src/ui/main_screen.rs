//! Main launcher surface — QR + heading + status strip (PLAN 4.15.1–.4).
//!
//! This is the default screen the launcher boots into: a starfield backdrop
//! (provided by [`crate::vortex`]), a gold-bezeled QR for the phone pairing
//! URL, a `SKYLANDER PORTAL` heading in Titan One, and a status strip with
//! the RPCS3 connection dot + current-game label.
//!
//! PLAN 4.15.6 bolts on a Y-axis card-flip that covers the QR with a
//! `MAXIMUM PLAYERS REACHED` back face when the session registry hits
//! `MAX_SESSIONS`. PLAN 4.15.7 paints a slow ellipse of gold-bezeled pips
//! around the QR — one per connected phone session, each tinted with the
//! session's unlocked-profile colour + initial.

use std::f32::consts::TAU;

use super::LauncherApp;
use super::launch_phase::LaunchPhase;
use crate::state::{LauncherStatus, SessionPip};
use crate::{fonts, palette};

/// Fixed square edge for the QR card. Bumped 320→420 on 2026-04-19
/// when the bezel went circular — a circle inscribed in a square reads
/// visually smaller than the same-sized square (the corners are gone),
/// so the round bezel needed more bounding room to retain the same
/// presence as the old rounded-square version. Exposed `pub(super)`
/// so other screens (e.g. `server_error`) can match the QR-card size
/// when reusing `paint_titled_card`.
pub(super) const CARD_SIZE: f32 = 420.0;

/// How long the QR → back-face flip animation takes, in seconds. `egui`'s
/// `animate_bool_with_time` uses this as the full 0→1 transition; we split
/// it into two halves (front scaling to 0, then back scaling from 0).
const FLIP_DURATION: f32 = 0.5;

/// Pip diameter in screen pixels. The mock uses 84px; we keep the same
/// physical size so the pip text is legible at 10 ft.
const PIP_DIAMETER: f32 = 84.0;

/// Orbit rotation rate (rad/s). Quiet idle motion — matches the vortex's
/// 0.08 rad/s so the two ambient animations don't compete for attention.
/// PLAN 4.19.8: pulled from 0.10 → 0.08 to match the comment + the
/// `navigation.md` §3.3 spec value.
const ORBIT_SPEED: f32 = 0.08;

// Removed at PLAN 10.7.10 along with the rest of the 2D back-card
// surface: BEZEL_RING_PX / SCREEN_RIM_PX were the inset distances
// the now-deleted `paint_titled_card` used to lay out the SF_3
// "monitor screen" ring inside the gold bezel disc. The 3D badge
// has its own analytic disc geometry (QR_RADIUS / OUTER_RADIUS in
// the shader) and doesn't need these. (Original doc said: visible
// thickness of the gold ring around the inner content disc; bumped
// 14→24 on 2026-04-19 after the dark inner ring read out of
// proportion with the gold.)

/// Width (in QR modules) of the clear quiet-zone ring between the QR
/// data and the surrounding circular noise field. Spike on 2026-04-19
/// (`examples/round_qr_spike.rs`) found 2 modules to be the sweet spot:
/// scanners lock on quickly while the noise still encroaches enough
/// to make the composition read as round/circular at a glance.
/// Variant C (gap=0) also scanned but took noticeably longer to lock,
/// so 2 is the user-validated default. Bump up if cross-room scanning
/// proves too slow on a real TV; the QR is encoded at ECC level H so
/// 0–4 are all functional, just trade off scan-speed vs visual.
const QR_NOISE_GAP_MODULES: u32 = 2;

impl LauncherApp {
    /// Render the Main surface. Called from the top-level dispatcher in
    /// [`super`] when `LauncherStatus::screen == LauncherScreen::Main`
    /// AND no game is running (game-running flips to `in_game::render`
    /// for the transparent in-game overlay).
    ///
    /// Layout matches mock state 4 (Awaiting Connect): centred QR card
    /// with heraldic "SCAN TO CONNECT" label below, plus the Exit to
    /// Desktop button as an implementation pragmatism (mock omits it
    /// because the mock can't trap you; we keep it so a stuck launcher
    /// has an obvious escape hatch independent of the phone).
    ///
    /// 4.19.22 stripped the prior brand heading, status strip, URL
    /// text, "Waiting for phone…", and figures-indexed counter — the
    /// orbit pips already convey "phone joined" and the rest was
    /// debug-info noise the mock omits. URL drop also closes 4.19.10a.
    pub(super) fn render_main(
        &self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        status_snapshot: &LauncherStatus,
        launch_phase: LaunchPhase,
    ) {
        // Paint the orbit pips *before* any widgets lay out. Each pip
        // is rendered as a shrunken `BadgeRig` coin (PLAN 10.7.8) so
        // it inherits the badge's GL pipeline + iris-aware
        // choreography — pips fade in/out with the same
        // `badge_alpha_3d` curve as the main badge so they don't
        // hang around as full-opacity dots through the iris close.
        let panel_rect = ui.max_rect();
        paint_player_orbit_3d(
            ui,
            ctx,
            panel_rect,
            ctx.input(|i| i.time) as f32,
            &status_snapshot.session_profiles,
            self.badge_rig.clone(),
            launch_phase.badge_alpha_3d(),
        );

        ui.vertical_centered(|ui| {
            // Centre the **QR card** at the panel midpoint — same point
            // the vortex's iris hole sits on. (Earlier cuts centred the
            // whole QR + label cluster, which put the QR ~60px above
            // the vortex centre — the cluster was visibly off-axis from
            // the swirl.) The label hangs below; the button takes the
            // remainder of the bottom space.
            let avail = ui.available_height();
            ui.add_space(((avail - CARD_SIZE) * 0.5).max(24.0));

            // Decide which face the centre card should show this frame.
            // Precedence: Starting > Switching > Loading > MaxPlayers >
            // QR front.
            //   - Starting outranks everything: while the server is
            //     booting (indexer, driver warmup, axum bind) no other
            //     state is meaningful and flipping in/out of Starting
            //     would read as noise.
            //   - Switching outranks Loading because the flag is only
            //     set between /api/quit?switch=true and the next
            //     /api/launch clearing it; during that window
            //     `loading_game` is still the previous (now-quit) game
            //     and would read wrong.
            //   - Loading outranks MaxPlayers — if the user picked a
            //     game while the session count happens to be saturated,
            //     the loading state is the more useful signal.
            let back_face = if !status_snapshot.server_ready {
                Some(BackFace::Starting)
            } else if status_snapshot.switching {
                Some(BackFace::Switching)
            } else if status_snapshot.cover_active {
                // PLAN 10.8.7b: graceful quit cover (no switch flag).
                // Sits below switching in precedence so a switch
                // request that briefly toggles cover_active still
                // shows the SWITCHING badge.
                Some(BackFace::Returning)
            } else if status_snapshot.loading_game.is_some() {
                Some(BackFace::Loading)
            } else if status_snapshot.session_slots_full {
                Some(BackFace::MaxPlayers)
            } else {
                None
            };

            // 3D badge takes over Main entirely (PLAN 10.7.7); the
            // shader rig owns the QR + back-face texture uploads, so
            // the egui-side `qr_texture` only feeds the in-game
            // reconnect overlay now.
            qr_card_flip(
                ui,
                ctx,
                back_face,
                self.badge_rig.clone(),
                launch_phase.badge_rotation_y(),
                launch_phase.badge_scale_3d(),
                launch_phase.badge_alpha_3d(),
            );
            // Mid-flip the qr_card_flip helper requests its own
            // repaints, but the multi-turn rotation curve in
            // `badge_rotation_y` itself runs from intro start
            // through the spin window, and egui won't repaint
            // unless something tells it to. The outer launcher
            // update loop hits 60fps already; keep this defensive
            // request for back-face states that previously relied
            // on the 2D halos for continuous frames.
            if matches!(
                back_face,
                Some(
                    BackFace::Starting
                        | BackFace::Loading
                        | BackFace::Switching
                        | BackFace::Returning
                ),
            ) {
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }

            ui.add_space(24.0);

            // Subtitle below the card. The text changes with the
            // back-face state so the loading state reads as "LOADING
            // / [game name]" rather than "LOADING / SCAN TO CONNECT".
            // MaxPlayers shares the QR's "SCAN TO CONNECT" since the
            // existing fifth-player-please-wait copy is implicit in
            // the back-face card itself.
            let subtitle: &str = match (back_face, status_snapshot.loading_game.as_deref()) {
                // Starting: subtitle lane is empty for now — the big
                // halo-spinning "STARTING" word carries the message.
                (Some(BackFace::Starting), _) => "",
                // Switching bridges two games — the phone hasn't picked
                // the next one yet, so there's no game name to show. A
                // bare "CHOOSE YOUR NEXT ADVENTURE" hint reads better
                // than an empty subtitle lane.
                (Some(BackFace::Switching), _) => "CHOOSE YOUR NEXT ADVENTURE",
                (Some(BackFace::Loading), Some(name)) => name,
                // Returning: brief beat between in-game and AwaitingConnect.
                // No subtitle — the badge text + iris-open animation
                // carry the "you quit, here's the launcher" message.
                (Some(BackFace::Returning), _) => "",
                _ => "SCAN TO CONNECT",
            };
            let label_height = 96.0;
            let label_rect = ui
                .allocate_exact_size(
                    egui::vec2(ui.available_width(), label_height),
                    egui::Sense::hover(),
                )
                .0;
            paint_heraldic_title(
                ui.painter(),
                label_rect.center(),
                subtitle,
                palette::HEADING_LG,
                launch_phase.badge_text_alpha(),
            );

            // Shader-compile progress (when the watchdog has detected
            // RPCS3 mid-compile). Italic dim status line below the
            // game-name title so the user knows progress is being
            // made during the multi-minute first-run shader compile.
            if let Some(text) = status_snapshot.shader_compile_text.as_deref() {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(text)
                        .size(palette::SUBHEAD)
                        .italics()
                        .color(with_alpha(
                            palette::TEXT_DIM,
                            launch_phase.badge_text_alpha(),
                        )),
                );
            }

            // Push the Exit to Desktop button toward the bottom of the
            // panel. Visible escape hatch — kept regardless of mock per
            // user 2026-04-19. Distinct red so it doesn't read as a
            // primary action competing with the QR.
            let remaining = ui.available_height();
            ui.add_space((remaining * 0.55).max(48.0));
            let btn = egui::Button::new(
                egui::RichText::new("Exit to Desktop")
                    .size(palette::HEADING)
                    .color(palette::TEXT),
            )
            .fill(palette::DANGER)
            .rounding(egui::Rounding::same(16.0))
            .min_size(egui::vec2(260.0, 60.0));
            if ui.add(btn).clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}

/// Which face the centre card is showing right now. The flip
/// animation drives the QR ↔ back-face transition; `back_face` then
/// picks what's actually painted on the back side.
///
/// `Farewell`, `Crashed`, and `ServerError` are reserved for the
/// in-flight ring-badge unification (each non-Main screen will fold
/// onto the dispatcher's standard card render). They're not
/// constructed yet — the per-screen render functions still hand-paint
/// — so suppress the dead-code warning until that wiring lands.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackFace {
    /// "STARTING" — server is booting (indexer, driver warmup, axum
    /// bind). Halo-spinning same as Loading so the transition from
    /// Starting → Loading (picked a game) / Starting → QR (ready to
    /// scan) stays a continuous spin rather than a card flip.
    Starting,
    /// "MAXIMUM PLAYERS REACHED" — session count is at MAX_SESSIONS.
    MaxPlayers,
    /// "LOADING" — RPCS3 is spawning + UIA-booting the picked game.
    /// Subtitle (game name) is rendered separately by the caller.
    Loading,
    /// "SWITCHING GAMES" — bridge face between quitting the current
    /// game and the phone selecting the next. Visually identical to
    /// Loading (same halos) so the loading→switching→loading handoff
    /// reads as one continuous spin rather than card flips in and out.
    Switching,
    /// "GOODBYE" — shutdown in progress. Iris pins to DarkHole behind
    /// this face; the launcher closes its viewport once the
    /// farewell_started_at countdown elapses.
    Farewell,
    /// "SOMETHING WENT WRONG" — RPCS3 crashed or failed to stay alive.
    /// Caller paints the diagnostic message as a separate overlay
    /// below the card; the card itself just carries the state title.
    Crashed,
    /// "SERVER FAILED TO START" — infrastructure error before the
    /// server finished booting (port busy, DB corrupt, etc.). Caller
    /// paints the error message as a separate overlay below the card.
    ServerError,
    /// "RETURNING TO PORTAL" — user quit a game (without switching).
    /// Bridges the in-game→AwaitingConnect transition: the launcher
    /// flips out of the transparent in-game render with this back-
    /// face during the cover-before-kill window so RPCS3 dies behind
    /// an opaque cover (PLAN 10.8.7b). Same halo-spin family as
    /// Loading / Switching for visual continuity.
    Returning,
}

impl BackFace {
    /// Every variant in a fixed enumeration order. The order doubles
    /// as the `BadgeRig` texture index (offset by 1 — index 0 is
    /// reserved for the QR raster), so don't reshuffle without
    /// also updating `texture_index`.
    pub(crate) const ALL: [BackFace; 8] = [
        BackFace::Starting,
        BackFace::MaxPlayers,
        BackFace::Loading,
        BackFace::Switching,
        BackFace::Farewell,
        BackFace::Crashed,
        BackFace::ServerError,
        BackFace::Returning,
    ];

    /// The static stack of words this back face shows on the disc
    /// — same content the legacy 2D `paint_titled_card` path
    /// renders via egui. Used at startup by `badge_text::render`
    /// to bake one texture per variant.
    pub(crate) fn lines(self) -> &'static [&'static str] {
        match self {
            BackFace::Starting => &["STARTING"],
            BackFace::MaxPlayers => &["PORTAL", "IS", "FULL"],
            BackFace::Loading => &["LOADING"],
            BackFace::Switching => &["SWITCHING", "GAMES"],
            // Three lines (heraldic farewell rhythm) — matches the
            // navigation-doc spec §3.5 and the legacy 2D farewell
            // copy that PLAN 4.19.14 landed on.
            BackFace::Farewell => &["FAREWELL", "PORTAL", "MASTER"],
            BackFace::Crashed => &["SOMETHING", "WENT", "WRONG"],
            BackFace::ServerError => &["SERVER", "FAILED", "TO START"],
            BackFace::Returning => &["RETURNING", "TO PORTAL"],
        }
    }

    /// The `BadgeRig` texture index this back face's text was
    /// uploaded to. Index 0 is the QR; back faces start at 1 in
    /// the order of `ALL`.
    pub(crate) fn texture_index(self) -> usize {
        // Position in `ALL` plus 1 for the QR offset. Linear search
        // is fine — the array has 7 entries and this runs once per
        // frame.
        Self::ALL
            .iter()
            .position(|bf| *bf == self)
            .map(|p| p + 1)
            .unwrap_or(0)
    }
}

/// Card-flip container (PLAN 4.15.6). Reserves a `CARD_SIZE × CARD_SIZE`
/// square and paints either the QR front face or one of the back
/// faces (MAXIMUM PLAYERS REACHED, LOADING) with a tent-wave
/// horizontal scale to simulate a Y-axis rotation.
///
/// egui doesn't ship 3D transforms, but a Y-axis rotation in 2D reads
/// identically to an X-axis scale (the vertical edges stay stationary;
/// the face just compresses horizontally, crosses zero at the midpoint,
/// and expands back). `animate_bool_with_time` drives the 0→1 progress;
/// the scale is `|2·progress − 1|`, peaking at 0 in the middle where we
/// swap content. `FLIP_DURATION` sets the overall transition time.
fn qr_card_flip(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    back_face: Option<BackFace>,
    badge_rig: std::sync::Arc<std::sync::Mutex<Option<crate::badge::BadgeRig>>>,
    badge_rotation_y: f32,
    badge_scale_3d: f32,
    badge_alpha_3d: f32,
) {
    // Reserve the full square so layout below doesn't shift during
    // the animation, regardless of whether the badge is currently a
    // pinpoint at the start of the intro spin or full-size at
    // steady state.
    let (rect, _resp) =
        ui.allocate_exact_size(egui::vec2(CARD_SIZE, CARD_SIZE), egui::Sense::hover());

    // PLAN 10.7.6b: rotate the disc whenever the displayed back-face
    // text would change. Persistent state lives in egui memory:
    // `displayed` is the back face the disc is currently *showing*
    // (lags behind `back_face` by up to one flip); `flip_start` is
    // the wall-clock time the current flip began (negative sentinel
    // = no flip in progress). Texture swap happens at flip midpoint
    // when the disc is showing its gold back face and the swap is
    // invisible.
    let state_id = egui::Id::new("badge_3d_displayed_back_face");
    let flip_id = egui::Id::new("badge_3d_flip_start");

    let now = ctx.input(|i| i.time);
    let mut displayed = ctx
        .memory(|m| m.data.get_temp::<Option<BackFace>>(state_id))
        .unwrap_or(back_face);
    let mut flip_start = ctx
        .memory(|m| m.data.get_temp::<f64>(flip_id))
        .unwrap_or(-1.0);

    // Detect new state changes; if there's no flip in flight and
    // the state has shifted, start one. Mid-flip changes are
    // deliberately ignored — the disc continues with the *previous*
    // target until it lands face-on, then a fresh flip kicks off if
    // the state has shifted again. Avoids chained flips visually
    // compounding into a fast spin.
    if back_face != displayed && flip_start < 0.0 {
        flip_start = now;
    }

    let flip_progress = if flip_start >= 0.0 {
        ((now - flip_start) / FLIP_DURATION as f64).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };

    if flip_progress >= 0.5 && displayed != back_face {
        displayed = back_face;
    }
    if flip_progress >= 1.0 {
        flip_start = -1.0;
    }

    ctx.memory_mut(|m| {
        m.data.insert_temp(state_id, displayed);
        m.data.insert_temp(flip_id, flip_start);
    });

    if flip_start >= 0.0 {
        // Mid-flip — keep frames coming so the rotation reads
        // smoothly even if nothing else in the launcher would
        // request a repaint this frame.
        ctx.request_repaint();
    }

    let texture_index = displayed.map(|bf| bf.texture_index()).unwrap_or(0);
    // Spec on for back-face metal text; off for the QR raster so
    // its data stays legible under the highlight band.
    let apply_spec = displayed.is_some();
    let additional_rotation = flip_progress * 2.0 * std::f32::consts::PI;

    crate::badge::paint_badge(
        ui.painter(),
        rect,
        badge_rig,
        badge_rotation_y + additional_rotation,
        badge_scale_3d,
        badge_alpha_3d,
        texture_index,
        apply_spec,
    );
}

// Removed at PLAN 10.7.7: `paint_loading_halos` + `paint_halo_arc`
// drove the rotating gold halos around the 2D back-face card during
// Starting / Loading / Switching states. The 3D badge owns those
// states now and conveys "something is happening" via the disc's
// per-state-change coin spin (PLAN 10.7.6b's flip-on-text-change),
// so a continuous-motion halo is no longer needed. If we want one
// back, prefer GL geometry attached to `BadgeRig` over re-introducing
// 2D egui paint — keeps the disc + halo lit by the same light source.

// Removed at PLAN 10.7.10: the entire 2D back-card surface
// (`paint_bezel`, `paint_radial_gradient_disc`, `lerp_color`,
// `paint_qr_front`, `paint_titled_card`, `paint_centered_back_card`,
// `BEZEL_RING_PX`, `SCREEN_RIM_PX`) is gone now that Crashed /
// Farewell / ServerError route through `paint_centered_3d_back_card`
// like Main does. The 3D `BadgeRig` owns every front-face render
// path — QR + back-face text + per-session pip alike.

/// Paint a circular bezel + dark "monitor screen" rim + SF_1 inner
/// disc carrying centred Titan One title text. Used by the QR card's
/// back face (`MAXIMUM PLAYERS REACHED`) and the server-error screen
/// (`SERVER FAILED TO START`). Geometry matches `paint_qr_front`'s
/// layered layout so different surfaces sharing a card silhouette
/// (e.g. card-flip transitions, screen swaps) read as one design
/// language.
///
/// Font size auto-scales from the inner disc radius and the line
/// count so 2-line vs 3-line vs 4-line titles all stay legible without
/// clipping the curved edges of the inscribed square.
/// Allocate a `CARD_SIZE × CARD_SIZE` square in the current ui's
/// vertical-centered column AT the same vertical-midpoint position
/// `render_main` uses for the QR card, then paint a back-face card
/// (lines = the title text inside the bezel) with the given coin-flip
/// scale + alpha. Returns the rect of the allocated square so callers
/// can stack their own content (subtitle, buttons, fade overlays)
/// below at the natural cursor.
///
/// Used by `crashed`, `server_error`, and `farewell` — three render
/// paths that all want the same gold-bezeled silhouette + the same
/// vertical anchor as Main, but with screen-specific copy underneath.
/// 2026-04-25 cleanup: pulled out the inlined ~30-line allocate +
/// half-w + from_center_size + paint dance from each of those files.
/// `vertical_scale` is the badge's vertical (Y-axis) scale: 1.0 for
/// the screens that only use the coin-flip horizontal spin (Crashed,
/// ServerError); Farewell passes its breathe pulse here so the badge
/// pulses in both axes during the goodbye countdown.
/// PLAN 10.7.10: 3D version of `paint_centered_back_card`. Allocates
/// the same `CARD_SIZE × CARD_SIZE` square at Main's vertical anchor
/// and paints the 3D `BadgeRig` with the back-face's pre-rasterised
/// text texture bound — so the goodbye / crash / server-error
/// surfaces read in the same visual language as Main now that the
/// 2D back-card path is gone.
///
/// `scale` carries both the intro spin-up and any per-screen scale
/// multipliers (Farewell passes its breathe pulse pre-multiplied);
/// uniform across both axes since 3D rotation handles the coin-flip
/// foreshortening, no separate horizontal vs vertical squash.
/// `alpha` is the badge body opacity after any per-screen fades
/// (Farewell breathes its alpha too). Returns the allocated full
/// square so callers can stack their own content (subtitle,
/// buttons, fade overlays) below at the natural cursor.
pub(super) fn paint_centered_3d_back_card(
    ui: &mut egui::Ui,
    back_face: BackFace,
    badge_rig: std::sync::Arc<std::sync::Mutex<Option<crate::badge::BadgeRig>>>,
    scale: f32,
    alpha: f32,
) -> egui::Rect {
    let avail = ui.available_height();
    ui.add_space(((avail - CARD_SIZE) * 0.5).max(24.0));

    let (rect, _) = ui.allocate_exact_size(egui::vec2(CARD_SIZE, CARD_SIZE), egui::Sense::hover());

    crate::badge::paint_badge(
        ui.painter(),
        rect,
        badge_rig,
        // No rotation — back-face screens land face-on. The
        // multi-turn coin-spin choreography belongs to Main; here
        // the badge appears, scales up, and stays put while the
        // surrounding text + buttons fade in.
        0.0,
        scale,
        alpha,
        back_face.texture_index(),
        // apply_texture_spec=true so the gold-on-blue back-face
        // text catches the same Blinn-Phong sheen as the
        // surrounding ring/torus/side-wall.
        true,
    );

    rect
}

// Removed at PLAN 10.7.10: `paint_centered_back_card` +
// `paint_titled_card` painted the legacy 2D back-face card that the
// Crashed / Farewell / ServerError screens used. All three render
// paths now route through `paint_centered_3d_back_card` (above) so
// the back-face surfaces share the Main screen's 3D `BadgeRig` look.

/// Re-tint a (likely solid) `Color32` by an additional alpha factor.
/// Used to fade text + content layers during launch-phase transitions
/// without touching the shape geometry. The shader treats Color32 as
/// premultiplied, so we scale RGB and alpha together.
pub(super) fn with_alpha(color: egui::Color32, alpha: f32) -> egui::Color32 {
    let a = alpha.clamp(0.0, 1.0);
    egui::Color32::from_rgba_premultiplied(
        ((color.r() as f32) * a) as u8,
        ((color.g() as f32) * a) as u8,
        ((color.b() as f32) * a) as u8,
        ((color.a() as f32) * a) as u8,
    )
}

/// Paint the player-orbit indicators (PLAN 4.15.7). Up to `MAX_SESSIONS`
/// gold-bezeled pips ride a slow ellipse centred on the panel; each pip is
/// tinted with the owning profile's colour and shows the profile's
/// initial. Called *before* the heading widget lays out so the pips sit
/// behind the text when their orbit intersects it (per the mock's
/// z-index 9 vs 10 layering).
///
/// The mock hard-codes `rx=560, ry=400` against a 1920×1080 surface. We
/// express the ellipse as a fraction of the panel's shorter axis so it
/// scales with the launcher's actual rect — dev builds are 900×1000, the
/// HTPC is 3840×2160, and both should look proportionate.
/// PLAN 10.7.8: paint each session's orbiting pip as a shrunken
/// `BadgeRig` coin so the pip inherits the badge's GL pipeline +
/// iris-respecting alpha. Replaces the 2D `paint_player_orbit` /
/// `paint_pip` pair that lived above the vortex iris and survived
/// the wipe instead of fading with it.
///
/// Per-pip choreography mirrors the back-face flip-on-text-change
/// from `qr_card_flip` (PLAN 10.7.6b): each slot's *displayed*
/// face lags behind the actual session profile by up to one flip,
/// and a 360° spin transitions between them — texture swap
/// happens at flip midpoint when the disc is on its gold back
/// face and the swap is invisible.
///
/// Default pip face = starfield blue + white "?" (matches the
/// back-face card aesthetic). Filled in to profile colour +
/// initial when the session unlocks; flips back to default on
/// disconnect (next frame the slot has a different `SessionPip`
/// or no pip at all).
fn paint_player_orbit_3d(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    rect: egui::Rect,
    time_s: f32,
    pips: &[SessionPip],
    badge_rig: std::sync::Arc<std::sync::Mutex<Option<crate::badge::BadgeRig>>>,
    badge_alpha: f32,
) {
    if pips.is_empty() || badge_alpha < 0.001 {
        return;
    }
    let centre = rect.center();
    let rx = (rect.width() * 0.29).min(rect.width() * 0.45);
    let ry = (rect.height() * 0.37).min(rect.height() * 0.45);

    let base_phase = time_s * ORBIT_SPEED;
    let painter = ui.painter().clone();
    let now = ctx.input(|i| i.time);

    for (i, pip) in pips.iter().enumerate().take(crate::profiles::MAX_SESSIONS) {
        let offset = TAU * (i as f32) / (crate::profiles::MAX_SESSIONS as f32);
        let t = base_phase + offset;
        let x = centre.x + t.cos() * rx;
        let y = centre.y + t.sin() * ry;
        let pip_rect =
            egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(PIP_DIAMETER, PIP_DIAMETER));

        let target_face = pip_face_for(pip);

        // Per-slot flip-on-change tracking via egui memory. Each
        // pip's slot has its own `displayed` + `flip_start` keys
        // so flips on slot 0 don't affect slot 1, etc.
        let state_id = egui::Id::new(("pip_3d_displayed", i));
        let flip_id = egui::Id::new(("pip_3d_flip_start", i));
        let mut displayed = ctx
            .memory(|m| m.data.get_temp::<PipFace>(state_id))
            .unwrap_or(target_face);
        let mut flip_start = ctx
            .memory(|m| m.data.get_temp::<f64>(flip_id))
            .unwrap_or(-1.0);

        if target_face != displayed && flip_start < 0.0 {
            flip_start = now;
        }

        let flip_progress = if flip_start >= 0.0 {
            ((now - flip_start) / FLIP_DURATION as f64).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };

        if flip_progress >= 0.5 && displayed != target_face {
            displayed = target_face;
        }
        if flip_progress >= 1.0 {
            flip_start = -1.0;
        }

        ctx.memory_mut(|m| {
            m.data.insert_temp(state_id, displayed);
            m.data.insert_temp(flip_id, flip_start);
        });

        if flip_start >= 0.0 {
            ctx.request_repaint();
        }

        let additional_rotation = flip_progress * 2.0 * std::f32::consts::PI;

        crate::badge::paint_pip(
            &painter,
            pip_rect,
            badge_rig.clone(),
            additional_rotation,
            1.0,
            badge_alpha,
            displayed.color,
            displayed.initial,
            displayed.ghost,
        );
    }
}

/// One pip's visual face — colour + glyph + ghost-state. `Copy +
/// Eq + 'static` so it can sit in egui memory and the flip-on-
/// change comparison is straightforward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PipFace {
    color: [u8; 3],
    initial: char,
    ghost: bool,
}

/// Translate a `SessionPip` into its rendered face. Unauthorised
/// sessions (no profile yet) and explicitly empty colour/initial
/// fields fall back to the default starfield-blue + "?" face;
/// authenticated profiles supply their colour and the first char
/// of their display name.
fn pip_face_for(pip: &SessionPip) -> PipFace {
    let color = pip
        .color
        .as_deref()
        .and_then(parse_hex_color)
        .map(|c| [c.r(), c.g(), c.b()])
        // Default = `palette::SF_1` (top starfield blue) so an
        // unauthenticated pip matches the back-face card's blue
        // aesthetic; flips to profile colour on unlock.
        .unwrap_or([palette::SF_1.r(), palette::SF_1.g(), palette::SF_1.b()]);
    let initial = pip
        .initial
        .as_deref()
        .and_then(|s| s.chars().next())
        .unwrap_or('?');
    PipFace {
        color,
        initial,
        ghost: pip.is_ghost,
    }
}

// Removed at PLAN 10.7.8: 2D `paint_pip` painted each orbiting
// session indicator as an egui-side composition (gold bezel + radial
// gradient + glyph). It survived the iris wipe because it lived
// above the GL vortex layer. Replaced by `paint_player_orbit_3d`
// which renders each pip via `crate::badge::paint_pip` — a shrunken
// `BadgeRig` coin so pips inherit the same iris-aware alpha curve
// the main badge uses.

/// Parse `#rrggbb` / `#rgb` into an `egui::Color32`. Profiles stored in
/// SQLite are validated to one of those two shapes, but be defensive —
/// a garbage value just falls through to `None` and the caller picks a
/// neutral default.
fn parse_hex_color(s: &str) -> Option<egui::Color32> {
    let s = s.strip_prefix('#')?;
    let (r, g, b) = match s.len() {
        6 => (
            u8::from_str_radix(&s[0..2], 16).ok()?,
            u8::from_str_radix(&s[2..4], 16).ok()?,
            u8::from_str_radix(&s[4..6], 16).ok()?,
        ),
        3 => {
            let r = u8::from_str_radix(&s[0..1], 16).ok()?;
            let g = u8::from_str_radix(&s[1..2], 16).ok()?;
            let b = u8::from_str_radix(&s[2..3], 16).ok()?;
            (r * 17, g * 17, b * 17)
        }
        _ => return None,
    };
    Some(egui::Color32::from_rgb(r, g, b))
}

/// Paint a Titan-One title with the heraldic embossed treatment from
/// `docs/aesthetic/design_language.md` §1: stacked shadow layers reading
/// as carved-into-the-bezel depth, plus a strong gold outer halo. egui's
/// `RichText` only ships a single solid colour per glyph, so the effect
/// is built by stacking multiple `Painter::text` calls in z-order. The
/// CSS spec the visual mimics:
///
///   text-shadow:
///     0 2px 0 var(--gs),                        ← carve, sharp
///     0 3px 0 var(--gi),                        ← carve, sharp
///     0 5px 10px rgba(0,0,0,0.5),               ← lift, blurred
///     0 0 24px rgba(245,198,52,0.25);           ← halo, wide blur
///
/// We can't do real Gaussian blur in the painter, so the blurred layers
/// (drop shadow + halo) are approximated by painting multiple copies on
/// concentric rings with falling alpha — accumulated additive paint
/// fakes the bloom. Tuned per Chris's feedback 2026-04-19 ("missing the
/// gold glow") — earlier single-ring 8-copy halo was too tight + too dim
/// to register against the dark starfield panel.
///
/// Layer order (bottom → top):
///   1. Outer gold halo (3 rings × 12 angular steps = 36 copies)
///   2. Soft black drop shadow (2 layers at +5/+7 px)
///   3. Carve under-layers (GOLD_SHADOW +2.5px, GOLD_INK +3.5px)
///   4. Bright body (GOLD_BRIGHT) on top
///
/// Called by `render_brand_intro` for the Startup beat. Render_main's
/// title still uses `ui.heading(...)` flat — switching that over is
/// 4.19.19's territory and would mean restructuring the layout stack.
pub(super) fn paint_heraldic_title(
    painter: &egui::Painter,
    pos: egui::Pos2,
    text: &str,
    size: f32,
    alpha: f32,
) {
    // Skip the whole stack if the title is fully faded — saves the
    // text-layout cost during launch-phase transitions where alpha
    // sits at 0 for the early/late portions of the close.
    if alpha <= 0.001 {
        return;
    }
    let font = egui::FontId::new(size, egui::FontFamily::Name(fonts::TITAN_ONE.into()));

    // 1. Outer gold halo. The first cut painted 36 offset copies of
    // the text glyph — but you could SEE the ghost copies (Chris
    // flagged 2026-04-19). The mock's CSS `text-shadow: 0 0 24px gold`
    // is a real Gaussian blur, which loses glyph detail and becomes a
    // soft blob roughly tracing the word's bounding box.
    //
    // We approximate that blob with a smooth radial-gradient ellipse
    // sized to the text bounds + halo padding. No glyph shape — just
    // a soft gold backlight. Looks like a real blur because it IS
    // smooth (vs. 36 offset copies that read as ghost letters).
    let galley = painter
        .ctx()
        .fonts(|f| f.layout_no_wrap(text.to_string(), font.clone(), palette::GOLD_BRIGHT));
    let text_size = galley.size();
    // Two stacked ellipses for a softer Gaussian-style falloff: wide
    // outer at low alpha (the diffuse outer glow), tighter inner at
    // medium alpha (the focal warmth around the text). A single
    // ellipse has a linear-ramp falloff that reads as harder-edged
    // than the mock's ~24px-blur shadow — the two-layer overpaint
    // approximates the soft Gaussian shoulder.
    // Outer ellipse uses pale GOLD_BRIGHT (#ffe58a) — fades to pale
    // gold at the rim. Inner uses richer GOLD (#f5c634) — keeps the
    // centre warm/honey instead of washing to white when the alphas
    // stack (Chris flagged 2026-04-19: pure GOLD_BRIGHT in both
    // layers reads white-hot at the centre, loses the gold tint).
    let outer = palette::GOLD_BRIGHT;
    let inner = palette::GOLD;
    let outer_pad = size * 1.45;
    let inner_pad = size * 0.75;
    // Each layer's color is multiplied by the launch-phase alpha so
    // the whole stack fades together — fade is uniform across glow,
    // shadow, carve, and body, no layers strobing relative to each
    // other.
    let scale_alpha = |a: u8| ((a as f32) * alpha.clamp(0.0, 1.0)) as u8;
    crate::vortex::paint_radial_ellipse(
        painter,
        pos,
        text_size.x * 0.5 + outer_pad,
        text_size.y * 0.5 + outer_pad,
        egui::Color32::from_rgba_unmultiplied(outer.r(), outer.g(), outer.b(), scale_alpha(5)),
    );
    crate::vortex::paint_radial_ellipse(
        painter,
        pos,
        text_size.x * 0.5 + inner_pad,
        text_size.y * 0.5 + inner_pad,
        egui::Color32::from_rgba_unmultiplied(inner.r(), inner.g(), inner.b(), scale_alpha(15)),
    );

    // 2. Soft drop shadow. Two layers at +5/+7px below — the lower one
    // half-alpha — approximates a 10px blur. Title lifts off the
    // starfield instead of sitting flat on it.
    painter.text(
        pos + egui::vec2(0.0, (size * 0.06).max(4.0)),
        egui::Align2::CENTER_CENTER,
        text,
        font.clone(),
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, scale_alpha(130)),
    );
    painter.text(
        pos + egui::vec2(0.0, (size * 0.085).max(6.0)),
        egui::Align2::CENTER_CENTER,
        text,
        font.clone(),
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, scale_alpha(70)),
    );

    // 3. Carve under-layers. Two stacked offsets in deepening gold
    // tones produce the engraved-into-the-bezel depth the heraldic
    // treatment calls for. Offsets scale with the size so the depth
    // reads at any resolution. Sharp (no spread) — the engraving
    // shouldn't blur.
    painter.text(
        pos + egui::vec2(0.0, (size * 0.035).max(2.5)),
        egui::Align2::CENTER_CENTER,
        text,
        font.clone(),
        with_alpha(palette::GOLD_INK, alpha),
    );
    painter.text(
        pos + egui::vec2(0.0, (size * 0.022).max(1.5)),
        egui::Align2::CENTER_CENTER,
        text,
        font.clone(),
        with_alpha(palette::GOLD_SHADOW, alpha),
    );

    // 4. Bright body on top.
    painter.text(
        pos,
        egui::Align2::CENTER_CENTER,
        text,
        font,
        with_alpha(palette::GOLD_BRIGHT, alpha),
    );
}

/// Rasterise the pairing URL into raw round-QR RGBA pixels. Called
/// once from [`LauncherApp::new`]; the pixels are kept on the app
/// for the life of the viewport, fed into both the egui-side
/// texture (via [`pixels_to_egui_texture`]) and the GL-side
/// `BadgeRig` (PLAN 10.7.2) so the two consumers stay byte-
/// identical.
///
/// Pixel composition (circular disc, noise ring, transparent
/// corners) lives in `crate::round_qr` so the phone's
/// `/api/join-qr.png` endpoint renders an identical image.
/// `QR_NOISE_GAP_MODULES` overrides the shared default gap — the
/// TV-distance scan tests settled on 2, which is tighter than the
/// 4-module standard quiet zone but still locks quickly at ECC
/// level H.
pub(super) fn render_qr_pixels(url: &str) -> crate::round_qr::RoundQrPixels {
    use crate::round_qr::{RoundQrConfig, render};
    let cfg = RoundQrConfig {
        gap_modules: QR_NOISE_GAP_MODULES,
        ..RoundQrConfig::launcher_default()
    };
    render(url, &cfg).expect("render round QR")
}

/// Repack the round-QR RGBA bytes into egui's premultiplied
/// `Color32` vec and load as a texture. The shared renderer returns
/// straight sRGB with alpha=0 in the corner triangles;
/// `Color32::from_rgba_unmultiplied` does the egui-side
/// premultiplication and preserves the transparency so the bezel
/// plate shows through.
pub(super) fn pixels_to_egui_texture(
    ctx: &egui::Context,
    pixels: &crate::round_qr::RoundQrPixels,
) -> egui::TextureHandle {
    let color_pixels: Vec<egui::Color32> = pixels
        .rgba
        .chunks_exact(4)
        .map(|c| egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
        .collect();
    let color_image = egui::ColorImage {
        size: [pixels.width as usize, pixels.height as usize],
        pixels: color_pixels,
    };
    ctx.load_texture("qr", color_image, egui::TextureOptions::NEAREST)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_color_six_char() {
        let c = parse_hex_color("#ff00aa").unwrap();
        assert_eq!(c, egui::Color32::from_rgb(0xff, 0x00, 0xaa));
    }

    #[test]
    fn hex_color_three_char_expands() {
        let c = parse_hex_color("#f0a").unwrap();
        // 0xf => 0xff, 0x0 => 0x00, 0xa => 0xaa (nibble * 17).
        assert_eq!(c, egui::Color32::from_rgb(0xff, 0x00, 0xaa));
    }

    #[test]
    fn hex_color_rejects_nonsense() {
        assert!(parse_hex_color("red").is_none());
        assert!(parse_hex_color("#xyz").is_none());
        assert!(parse_hex_color("").is_none());
        assert!(parse_hex_color("#ff00aabb").is_none());
    }
}
