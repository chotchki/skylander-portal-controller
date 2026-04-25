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

/// Visible thickness of the gold ring around the inner content disc,
/// in screen pixels. The bezel paints a full filled gold disc; the
/// inner SF_3 / SF_1 content disc is drawn smaller by this amount so
/// the gold shows around it as a ring. Bumped 14→24 on 2026-04-19
/// after Chris noted the dark inner ring was out of proportion with
/// the gold on a 420px bezel — at 14px the gold read as thinner than
/// the combined dark (GOLD_INK inset + SF_3 rim) inside it.
const BEZEL_RING_PX: f32 = 24.0;

/// Visible thickness of the dark "screen bezel" between the gold ring
/// and the QR/text content. Matches the look of a recessed monitor
/// screen sitting inside a gold frame — the screen rim is a darker
/// inner ring that frames the content. Total inset from the bezel
/// rect to the QR texture / text is `BEZEL_RING_PX + SCREEN_RIM_PX`.
/// Bounced 8→3→8→14 on 2026-04-19: 14 finally gives the dark border
/// enough screen presence that the QR doesn't read as kissing the
/// gold. Gold ring (24px) still dominates the dark (14px) ~1.7:1,
/// matching the phone pip's gold-dominant proportion.
const SCREEN_RIM_PX: f32 = 14.0;

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
        // Paint the orbit pips *before* any widgets lay out. Immediate-mode
        // egui paints shapes in submission order, so these land above the
        // vortex (already drawn by the dispatcher) but below the QR + label
        // rendered below — matching the mock's z-index 9 orbit passing
        // behind z-index 10 text.
        let panel_rect = ui.max_rect();
        paint_player_orbit(
            ui.painter(),
            panel_rect,
            ctx.input(|i| i.time) as f32,
            &status_snapshot.session_profiles,
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
            } else if status_snapshot.loading_game.is_some() {
                Some(BackFace::Loading)
            } else if status_snapshot.session_slots_full {
                Some(BackFace::MaxPlayers)
            } else {
                None
            };

            if let Some(tex) = &self.qr_texture {
                // The card-flip helper reserves a fixed-size square and
                // paints the front / back face directly via Painter so the
                // Y-axis rotation reads as a horizontal scale without any
                // custom matrix math — egui-native, see the helper doc.
                // The launch_phase factors layer ONTO the flip scale.
                qr_card_flip(
                    ui,
                    ctx,
                    tex,
                    back_face,
                    launch_phase.badge_scale(),
                    launch_phase.badge_alpha(),
                    launch_phase.badge_text_alpha(),
                );
                // Any halo-spinning face needs continuous frames; egui
                // is lazy by default and would only repaint on input.
                // The launcher's outer update loop already hits 60fps,
                // so this is defensive — safe to leave in case the
                // outer loop is ever gated.
                if matches!(
                    back_face,
                    Some(BackFace::Starting | BackFace::Loading | BackFace::Switching),
                ) {
                    ctx.request_repaint_after(std::time::Duration::from_millis(16));
                }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BackFace {
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
    tex: &egui::TextureHandle,
    back_face: Option<BackFace>,
    phase_scale: f32,
    bezel_alpha: f32,
    content_alpha: f32,
) {
    // `animate_bool_with_time` interpolates 0.0 → 1.0 when the back
    // face is wanted, and back to 0 when it's not. Driven by
    // `back_face.is_some()` so any back face (MaxPlayers OR Loading)
    // triggers the same coin-flip.
    let id = egui::Id::new("launcher_qr_card_flip");
    let progress = ctx.animate_bool_with_time(id, back_face.is_some(), FLIP_DURATION);

    // Tent wave: 1 at the edges, 0 in the middle. Represents the x-scale
    // of the visible face from the flip animation; hits 0 at the flip
    // midpoint where we swap content.
    let flip_scale = (progress * 2.0 - 1.0).abs();
    let show_back = progress > 0.5;

    // Final horizontal scale = flip × phase. The phase factor handles
    // the intro spin-in (0 → 1) and close spin-out (1 → 0); the flip
    // factor handles the in-place QR↔back-face card flip. Multiplying
    // composes them — a flip mid-intro reads as both layers
    // contributing to the squish.
    let scale = flip_scale * phase_scale;

    // Reserve the full square so layout below doesn't shift during the
    // animation.
    let (rect, _resp) =
        ui.allocate_exact_size(egui::vec2(CARD_SIZE, CARD_SIZE), egui::Sense::hover());

    // Squish horizontally, keep vertical alignment.
    let half_w = (rect.width() * scale) * 0.5;
    let inner =
        egui::Rect::from_center_size(rect.center(), egui::vec2(half_w * 2.0, rect.height()));

    // Guard against zero-width rendering — egui's rounding + stroke paint
    // gets messy at 0 extent, and nothing's visible anyway at the midpoint.
    if inner.width() < 1.0 {
        return;
    }

    let painter = ui.painter();
    if show_back {
        // Pick the lines based on which back face is wanted. If the
        // caller passed `None` but we're past the midpoint (rare race
        // — `back_face` flipped to None mid-animation), fall back to
        // the LAST visible back face by re-using whatever's stored —
        // simpler to just default to MaxPlayers; the next frame will
        // pick the right side.
        let lines: &[&str] = match back_face.unwrap_or(BackFace::MaxPlayers) {
            BackFace::Starting => &["STARTING"],
            // PLAN 4.19.9 — spec copy is "PORTAL IS FULL"; previously
            // "MAXIMUM PLAYERS REACHED". Shorter line reads better at
            // TV distance.
            BackFace::MaxPlayers => &["PORTAL", "IS", "FULL"],
            BackFace::Loading => &["LOADING"],
            BackFace::Switching => &["SWITCHING", "GAMES"],
            BackFace::Farewell => &["GOODBYE"],
            BackFace::Crashed => &["SOMETHING", "WENT", "WRONG"],
            BackFace::ServerError => &["SERVER", "FAILED", "TO START"],
        };
        paint_titled_card(painter, inner, lines, bezel_alpha, content_alpha);
        // "Waiting" faces (Starting / Loading / Switching) get two
        // rotating halos around the bezel rim — same look as
        // `mocks/transitions.html`'s `.state-loading` (slow outer halo
        // + fast inner halo, both gold conic gradients sweeping around
        // the badge). The badge content itself stays static; rotation
        // lives in the halos so the word stays readable. Sharing the
        // halos across Starting → Loading → Switching keeps their
        // transitions reading as one continuous spin.
        if matches!(
            back_face,
            Some(BackFace::Starting | BackFace::Loading | BackFace::Switching),
        ) && bezel_alpha > 0.001
            && inner.width() >= 1.0
        {
            paint_loading_halos(painter, inner, ctx.input(|i| i.time) as f32, bezel_alpha);
        }
    } else {
        paint_qr_front(painter, inner, tex, bezel_alpha, content_alpha);
    }
}

/// Paint the rotating loading halos around the badge — matches the
/// mock's `.state-loading` look from `docs/aesthetic/mocks/
/// transitions.html`: slow outer halo (1.4s period) + fast inner
/// halo (0.9s period), both gold conic-gradient arcs that sweep
/// around the bezel rim.
///
/// Multiplied by `outer_alpha` so the halos fade with the bezel
/// during the launch-phase intro/close transitions (badge_alpha =
/// 0 → 1 spin-in, then back to 0 on close).
fn paint_loading_halos(painter: &egui::Painter, rect: egui::Rect, time_s: f32, outer_alpha: f32) {
    let center = rect.center();
    let outer_r = rect.width().min(rect.height()) / 2.0;

    // Slow halo: broad bright sweep + secondary trailing sweep, both
    // at the same radius (just past the bezel) on a 1.4s rotation.
    paint_halo_arc(
        painter,
        center,
        outer_r + 18.0,
        5.0,
        time_s / 1.4,
        130.0,
        palette::GOLD_BRIGHT,
        0.7 * outer_alpha,
    );
    paint_halo_arc(
        painter,
        center,
        outer_r + 18.0,
        4.0,
        time_s / 1.4 + 0.5,
        80.0,
        palette::GOLD,
        0.4 * outer_alpha,
    );

    // Fast halo: single tight bright sweep right at the bezel edge,
    // 0.9s rotation. Reads as the "spinner needle" against the
    // wider slow halo's diffuse gold glow.
    paint_halo_arc(
        painter,
        center,
        outer_r + 6.0,
        3.0,
        time_s / 0.9,
        30.0,
        palette::GOLD_BRIGHT,
        outer_alpha,
    );
}

/// Paint a partial arc with a hump-shaped alpha (transparent → full
/// → transparent across the arc's span), giving the conic-gradient
/// sweep look. `rotation_cycles` = current rotation in turns (1.0 =
/// full revolution); the start angle is `rotation_cycles * 2π`.
///
/// Painted as THREE concentric stroke passes at decreasing alpha to
/// fake a Gaussian-ish blur — egui doesn't ship a real shape blur,
/// but stacking a wide-dim, mid-medium, narrow-bright triplet reads
/// as the soft glow the mock's `filter: blur(6px)` produces.
fn paint_halo_arc(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    stroke_width: f32,
    rotation_cycles: f32,
    span_deg: f32,
    color: egui::Color32,
    alpha: f32,
) {
    use std::f32::consts::{PI, TAU};
    let segments = ((span_deg / 3.0).ceil() as usize).max(8);
    let start_angle = rotation_cycles * TAU;
    let span_rad = span_deg.to_radians();
    let alpha = alpha.clamp(0.0, 1.0);

    // Three passes simulating a soft outer glow → bright core. Wider
    // strokes at lower alpha sit underneath; the bright narrow core
    // sits on top.
    let passes: [(f32, f32); 3] = [
        (stroke_width * 3.0, 0.20), // wide dim halo
        (stroke_width * 1.8, 0.40), // mid
        (stroke_width, 1.0),        // bright core
    ];

    let mut last_points: Vec<egui::Pos2> = Vec::with_capacity(segments + 1);
    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let angle = start_angle + t * span_rad;
        last_points.push(egui::pos2(
            center.x + angle.cos() * radius,
            center.y + angle.sin() * radius,
        ));
    }

    for (pass_width, pass_alpha) in passes {
        for i in 0..segments {
            let t = i as f32 / segments as f32;
            // Hump shape on the segment's leading vertex's t.
            let local_alpha = (PI * t).sin() * alpha * pass_alpha;
            let alpha_byte = (255.0 * local_alpha) as u8;
            if alpha_byte == 0 {
                continue;
            }
            let stroke_color =
                egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha_byte);
            painter.line_segment(
                [last_points[i], last_points[i + 1]],
                egui::Stroke::new(pass_width, stroke_color),
            );
        }
    }
}

/// Paint the circular gold bezel — matches the phone SPA's `.bezel-ring`
/// (`phone/assets/app.css` line 547). The phone is the source of truth
/// for the design language; the launcher mirrors it so the TV and
/// phone read as one product.
///
/// Phone CSS recipe:
/// ```css
/// background: radial-gradient(circle at 30% 25%,
///     var(--gb), var(--g) 22%, var(--gm) 55%, var(--gs) 100%);
/// box-shadow:
///     inset 0 0 0 2px var(--gi),                  /* dark inner border */
///     inset 0 3px 4px rgba(255,255,255,0.3),      /* top white highlight */
///     inset 0 -3px 4px rgba(0,0,0,0.5),           /* bottom dark shadow */
///     0 0 0 1px #000,                             /* outer black border */
///     0 4px 10px rgba(0,0,0,0.6);                 /* drop shadow */
/// ```
///
/// We approximate in egui as:
///   1. Halo glow (4.19.7).
///   2. 4-stop radial gradient disc with the highlight offset to
///      top-left — gives the embossed-metal look the phone has.
///   3. Inset top white highlight (thin bright crescent at the top
///      inner edge).
///   4. Inset bottom dark shadow (thin dark crescent at the bottom
///      inner edge).
///   5. Inner GOLD_INK ring (the inset 2px dark border).
///   6. Outer dark border (the 1px black halo around the whole disc).
///
/// Note for future bezel consumers: this is a *filled disc*, not a
/// ring. If you call `paint_bezel` and then paint nothing on top, you
/// get a solid gold circle. To make a ring visible, paint your screen
/// content (SF_3 / SF_1 / texture) inset enough that the gold shows
/// around it — see `paint_qr_front` for the pattern.
fn paint_bezel(painter: &egui::Painter, rect: egui::Rect, alpha: f32) {
    if alpha <= 0.001 {
        return;
    }
    let center = rect.center();
    let outer_r = rect.width().min(rect.height()) / 2.0;

    // 1. Halo glow — soft gold radial behind the disc.
    crate::vortex::paint_radial_ellipse(
        painter,
        center,
        outer_r * 1.7,
        outer_r * 1.7,
        with_alpha(
            egui::Color32::from_rgba_unmultiplied(
                palette::GOLD_BRIGHT.r(),
                palette::GOLD_BRIGHT.g(),
                palette::GOLD_BRIGHT.b(),
                55,
            ),
            alpha,
        ),
    );

    // 2. 4-stop radial gradient disc, highlight offset to (30%, 25%)
    // of the bounding rect — matches the phone CSS exactly. The
    // highlight position simulates a light source above-and-left of
    // the bezel; without the offset the gradient looks flat.
    let highlight_offset = egui::vec2(-0.4 * outer_r, -0.5 * outer_r);
    paint_radial_gradient_disc(
        painter,
        center,
        outer_r,
        center + highlight_offset,
        &[
            (0.00, with_alpha(palette::GOLD_BRIGHT, alpha)),
            (0.22, with_alpha(palette::GOLD, alpha)),
            (0.55, with_alpha(palette::GOLD_MID, alpha)),
            (1.00, with_alpha(palette::GOLD_SHADOW, alpha)),
        ],
    );

    // 3. Inset top highlight — thin crescent of white at the top
    // inner edge (the phone's `inset 0 3px 4px rgba(255,255,255,0.3)`).
    // We approximate as a stroke just inside the rim, top-half only,
    // by stacking two strokes with progressively tighter alpha and
    // smaller radii.
    let highlight = with_alpha(
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 76),
        alpha,
    );
    painter.circle_stroke(
        center - egui::vec2(0.0, 1.0),
        outer_r - 2.0,
        egui::Stroke::new(2.0, highlight),
    );

    // 4. Inset bottom shadow — same trick, dark crescent at the
    // bottom inner edge.
    let shadow = with_alpha(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 127), alpha);
    painter.circle_stroke(
        center + egui::vec2(0.0, 1.0),
        outer_r - 2.0,
        egui::Stroke::new(2.0, shadow),
    );

    // 5. Inner dark ring — the phone's `inset 0 0 0 2px var(--gi)`,
    // a thin GOLD_INK band just inside the rim that frames the gold.
    painter.circle_stroke(
        center,
        outer_r - 1.0,
        egui::Stroke::new(2.0, with_alpha(palette::GOLD_INK, alpha)),
    );

    // 6. Outer black border — the phone's `0 0 0 1px #000`, a 1px
    // black halo around the whole disc separating bezel from sky.
    painter.circle_stroke(
        center,
        outer_r + 0.5,
        egui::Stroke::new(1.0, with_alpha(egui::Color32::BLACK, alpha)),
    );
}

/// Paint a filled disc with a multi-stop RADIAL colour gradient
/// emanating from `highlight_center`. `stops` is `(distance_t, color)`
/// pairs where `distance_t` is normalised against the maximum distance
/// from `highlight_center` to any point on the disc rim — so `t=0`
/// is at the highlight, `t=1` is at the farthest rim point.
///
/// Implementation: a triangle fan from `highlight_center` plus a ring
/// of internal "rib" vertices to densify the mesh. Each vertex is
/// coloured by interpolating the stops at its `t = distance / max_d`.
/// Without rib vertices the gradient steps would be visible as
/// straight lines from the highlight outward; with them the mesh has
/// enough density to read as smooth.
///
/// The disc rim is approximated by 96 segments (slightly higher than
/// the linear gradient's 64 — the offset highlight makes any flat
/// edges more obvious).
///
/// **Stops MUST be sorted by `distance_t` ascending.** No bounds
/// checking — internal use only.
fn paint_radial_gradient_disc(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    highlight_center: egui::Pos2,
    stops: &[(f32, egui::Color32)],
) {
    use egui::epaint::{Mesh, Vertex, WHITE_UV};

    let segments: usize = 96;
    let rings: usize = 8; // intermediate radial rings for smoothness
    let mut mesh = Mesh::default();

    // Compute the maximum distance from highlight_center to any rim
    // point. Used to normalise distances into the [0, 1] stop range.
    let highlight_to_disc = highlight_center - center;
    let h_dist = highlight_to_disc.length();
    let max_d = radius + h_dist;

    let color_at = |t: f32| -> egui::Color32 {
        let t = t.clamp(0.0, 1.0);
        for w in stops.windows(2) {
            let (t0, c0) = w[0];
            let (t1, c1) = w[1];
            if t <= t1 {
                let span = (t1 - t0).max(1e-6);
                let local = ((t - t0) / span).clamp(0.0, 1.0);
                return lerp_color(c0, c1, local);
            }
        }
        stops.last().unwrap().1
    };

    let push_vertex = |mesh: &mut Mesh, pos: egui::Pos2| {
        let d = (pos - highlight_center).length();
        let t = (d / max_d).clamp(0.0, 1.0);
        mesh.vertices.push(Vertex {
            pos,
            uv: WHITE_UV,
            color: color_at(t),
        });
    };

    // Vertex 0: highlight centre.
    push_vertex(&mut mesh, highlight_center);

    // Ring 1..=rings: vertices on concentric circles around the disc
    // centre (NOT the highlight centre — we want the rings to follow
    // the disc shape so the outermost ring is the rim). The first
    // ring is small, the last is the rim.
    for ring in 1..=rings {
        let r = radius * (ring as f32) / (rings as f32);
        for seg in 0..segments {
            let angle = TAU * (seg as f32) / (segments as f32);
            let (s, c) = angle.sin_cos();
            let pos = egui::pos2(center.x + r * c, center.y + r * s);
            push_vertex(&mut mesh, pos);
        }
    }

    // Triangulate. First ring fans from vertex 0 to the first ring's
    // segments. Subsequent rings tile as quads between consecutive
    // rings.
    let seg = segments as u32;
    // Fan from highlight to ring 1.
    for s in 0..seg {
        let next = (s + 1) % seg;
        mesh.indices.push(0);
        mesh.indices.push(1 + s);
        mesh.indices.push(1 + next);
    }
    // Quad strips between rings.
    for ring in 0..(rings - 1) {
        let inner_base = 1 + (ring as u32) * seg;
        let outer_base = 1 + ((ring + 1) as u32) * seg;
        for s in 0..seg {
            let next = (s + 1) % seg;
            // Triangle 1: inner_s, outer_s, outer_next
            mesh.indices.push(inner_base + s);
            mesh.indices.push(outer_base + s);
            mesh.indices.push(outer_base + next);
            // Triangle 2: inner_s, outer_next, inner_next
            mesh.indices.push(inner_base + s);
            mesh.indices.push(outer_base + next);
            mesh.indices.push(inner_base + next);
        }
    }

    painter.add(egui::Shape::Mesh(mesh));
}

/// Linear RGBA interpolation for `Color32`. egui stores premultiplied
/// alpha so this only does the right thing for opaque or
/// fully-transparent endpoints — fine for our gold gradient where both
/// stops are 255-alpha.
fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let inv = 1.0 - t;
    egui::Color32::from_rgba_premultiplied(
        ((a.r() as f32) * inv + (b.r() as f32) * t) as u8,
        ((a.g() as f32) * inv + (b.g() as f32) * t) as u8,
        ((a.b() as f32) * inv + (b.b() as f32) * t) as u8,
        ((a.a() as f32) * inv + (b.a() as f32) * t) as u8,
    )
}

/// Front face — circular gold bezel + dark "screen rim" + round QR
/// composition. Layered outermost-to-innermost:
///
///   1. Gold bezel disc (`paint_bezel`)
///   2. SF_3 screen disc inset by `BEZEL_RING_PX` — visible as a thin
///      dark ring between gold and content, like a recessed monitor
///      screen sitting inside the gold frame
///   3. Thin GOLD_SHADOW stroke around the SF_3 disc for definition
///   4. QR texture inset by `BEZEL_RING_PX + SCREEN_RIM_PX` so the
///      SF_3 ring is visible around the white screen content
///
/// The texture's transparent corners (outside its inscribed circle)
/// land in the SF_3 disc area, so they read as continuous SF_3 rim
/// rather than gold — visually the noise/QR composition sits inside
/// a clean dark monitor frame with a gold bezel around the whole.
fn paint_qr_front(
    painter: &egui::Painter,
    rect: egui::Rect,
    tex: &egui::TextureHandle,
    bezel_alpha: f32,
    content_alpha: f32,
) {
    paint_bezel(painter, rect, bezel_alpha);

    let center = rect.center();
    let outer_r = rect.width().min(rect.height()) / 2.0;
    let screen_r = outer_r - BEZEL_RING_PX;
    // SF_3 inner disc + thin gold rim track the bezel's alpha so the
    // whole "monitor screen" face dissolves in/out together with the
    // surrounding gold frame.
    painter.circle_filled(center, screen_r, with_alpha(palette::SF_3, bezel_alpha));
    painter.circle_stroke(
        center,
        screen_r,
        egui::Stroke::new(1.0, with_alpha(palette::GOLD_SHADOW, bezel_alpha)),
    );

    let qr_rect = rect.shrink(BEZEL_RING_PX + SCREEN_RIM_PX);
    let tint_a = (content_alpha.clamp(0.0, 1.0) * 255.0) as u8;
    painter.image(
        tex.id(),
        qr_rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, tint_a),
    );
}

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
pub(super) fn paint_centered_back_card(
    ui: &mut egui::Ui,
    lines: &[&str],
    badge_scale: f32,
    vertical_scale: f32,
    bezel_alpha: f32,
    text_alpha: f32,
) -> egui::Rect {
    // Same vertical centring `render_main` uses for the QR card so
    // the badge silhouette anchors at the vortex iris regardless of
    // which screen the user lands on.
    let avail = ui.available_height();
    ui.add_space(((avail - CARD_SIZE) * 0.5).max(24.0));

    let (full_rect, _) =
        ui.allocate_exact_size(egui::vec2(CARD_SIZE, CARD_SIZE), egui::Sense::hover());
    let half_w = (full_rect.width() * badge_scale) * 0.5;
    let height = full_rect.height() * vertical_scale;
    let badge_rect =
        egui::Rect::from_center_size(full_rect.center(), egui::vec2(half_w * 2.0, height));
    if badge_rect.width() >= 1.0 {
        paint_titled_card(ui.painter(), badge_rect, lines, bezel_alpha, text_alpha);
    }
    full_rect
}

pub(super) fn paint_titled_card(
    painter: &egui::Painter,
    rect: egui::Rect,
    lines: &[&str],
    bezel_alpha: f32,
    text_alpha: f32,
) {
    paint_bezel(painter, rect, bezel_alpha);

    // Layered identically to paint_qr_front: SF_3 screen rim + thin
    // GOLD_SHADOW stroke + inner SF_1 disc. Same screen geometry as
    // the QR-front so the card-flip silhouette is stable. All three
    // layers track the bezel_alpha so the whole card dissolves
    // together.
    let center = rect.center();
    let outer_r = rect.width().min(rect.height()) / 2.0;
    let screen_r = outer_r - BEZEL_RING_PX;
    painter.circle_filled(center, screen_r, with_alpha(palette::SF_3, bezel_alpha));
    painter.circle_stroke(
        center,
        screen_r,
        egui::Stroke::new(1.0, with_alpha(palette::GOLD_SHADOW, bezel_alpha)),
    );

    let inner_r = screen_r - SCREEN_RIM_PX;
    painter.circle_filled(center, inner_r, with_alpha(palette::SF_1, bezel_alpha));

    // Layout box is the inscribed square so text never strays into the
    // curved edges of the inner disc where it would clip visually.
    let inscribed_half = inner_r * std::f32::consts::FRAC_1_SQRT_2;
    let n = lines.len().max(1) as f32;
    let line_h = (inscribed_half * 2.0) / n;
    // Font size = 55% of line height, clamped so single-word lines
    // don't blow up and tight 4-line cards stay readable. Bounds tied
    // to palette tokens so a global retune cascades here too.
    let font_size = (line_h * 0.55).clamp(palette::SUBHEAD, palette::COUNTDOWN);
    let text_color = with_alpha(palette::GOLD_BRIGHT, text_alpha);
    for (i, word) in lines.iter().enumerate() {
        let y = center.y - inscribed_half + line_h * (i as f32 + 0.5);
        painter.text(
            egui::pos2(center.x, y),
            egui::Align2::CENTER_CENTER,
            *word,
            egui::FontId::new(font_size, egui::FontFamily::Name(fonts::TITAN_ONE.into())),
            text_color,
        );
    }
}

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
fn paint_player_orbit(painter: &egui::Painter, rect: egui::Rect, time_s: f32, pips: &[SessionPip]) {
    if pips.is_empty() {
        return;
    }
    let centre = rect.center();
    // Ellipse proportions matched to the mock (560×400 on a 1920×1080
    // frame → rx = 29% of width, ry = 37% of height). Clamped against
    // the shorter axis so portrait and landscape surfaces both stay
    // sensible.
    let rx = (rect.width() * 0.29).min(rect.width() * 0.45);
    let ry = (rect.height() * 0.37).min(rect.height() * 0.45);

    let base_phase = time_s * ORBIT_SPEED;
    for (i, pip) in pips.iter().enumerate().take(crate::profiles::MAX_SESSIONS) {
        // Distribute pips evenly around the orbit — 2 pips → 180° apart.
        let offset = TAU * (i as f32) / (crate::profiles::MAX_SESSIONS as f32);
        let t = base_phase + offset;
        let x = centre.x + t.cos() * rx;
        let y = centre.y + t.sin() * ry;
        let pip_rect =
            egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(PIP_DIAMETER, PIP_DIAMETER));
        paint_pip(painter, pip_rect, pip);
    }
}

/// Single pip — a gold-bezeled circle with the profile's colour as fill
/// and the initial rendered in Titan One. Unknown profile (None fields)
/// falls back to neutral gold + a small dot.
fn paint_pip(painter: &egui::Painter, rect: egui::Rect, pip: &SessionPip) {
    let centre = rect.center();
    let radius = rect.width() * 0.5;

    // Outer gold bezel — reuses the QR card's `paint_bezel` (halo,
    // 4-stop radial gradient, inset highlight, inset shadow, inner
    // GOLD_INK ring, outer black). Visual consistency with every
    // other gold-rimmed surface in the launcher and a 1:1 match for
    // the phone's `.player-pip` design language (Chris flagged
    // 2026-04-19, "use the nice coloring of the phone profile badge").
    paint_bezel(painter, rect, 1.0);

    // Inner profile-colour disc with a soft radial gradient for
    // depth — highlight offset to top-left mimics ambient light from
    // above, the same "embossed" treatment paint_bezel uses for the
    // gold ring. Proportions match the phone: ~78% of pip diameter
    // for the inner disc leaves a ~11% gold-ring rim visible.
    let inner_radius = radius * 0.78;
    let base = pip
        .color
        .as_deref()
        .and_then(parse_hex_color)
        .unwrap_or(palette::GOLD_BRIGHT);
    let highlight = lerp_color(base, egui::Color32::WHITE, 0.25);
    let shadow = lerp_color(base, egui::Color32::BLACK, 0.30);
    paint_radial_gradient_disc(
        painter,
        centre,
        inner_radius,
        centre + egui::vec2(-inner_radius * 0.30, -inner_radius * 0.40),
        &[(0.0, highlight), (0.4, base), (1.0, shadow)],
    );

    // Glyph — white Titan One with a soft drop shadow so it reads
    // cleanly on any profile colour. "?" fallback when a session is
    // registered but the player hasn't unlocked a profile yet.
    let glyph = match pip.initial.as_deref() {
        Some(ch) if !ch.is_empty() => ch,
        _ => "?",
    };
    let font = egui::FontId::new(
        palette::HEADING,
        egui::FontFamily::Name(fonts::TITAN_ONE.into()),
    );
    painter.text(
        centre + egui::vec2(0.0, 2.0),
        egui::Align2::CENTER_CENTER,
        glyph,
        font.clone(),
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 120),
    );
    painter.text(
        centre,
        egui::Align2::CENTER_CENTER,
        glyph,
        font,
        egui::Color32::WHITE,
    );
}

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

/// Rasterise the pairing URL into a round QR texture. Called once from
/// [`LauncherApp::new`]; the texture is cached on the app for the life
/// of the viewport and reused by the corner reconnect panel in
/// `ui/in_game.rs`.
///
/// Pixel composition (circular disc, noise ring, transparent corners)
/// lives in `crate::round_qr` so the phone's `/api/join-qr.png`
/// endpoint renders an identical image. `QR_NOISE_GAP_MODULES` overrides
/// the shared default gap — the TV-distance scan tests settled on 2,
/// which is tighter than the 4-module standard quiet zone but still
/// locks quickly at ECC level H.
pub(super) fn render_qr_texture(ctx: &egui::Context, url: &str) -> egui::TextureHandle {
    use crate::round_qr::{RoundQrConfig, render};

    let cfg = RoundQrConfig {
        gap_modules: QR_NOISE_GAP_MODULES,
        ..RoundQrConfig::launcher_default()
    };
    let pixels = render(url, &cfg).expect("render round QR");

    // Repack RGBA bytes into egui's premultiplied `Color32` vec. The
    // shared renderer returns straight sRGB with alpha=0 in the corner
    // triangles; `Color32::from_rgba_unmultiplied` does the egui-side
    // premultiplication and preserves the transparency so the bezel
    // plate shows through.
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
