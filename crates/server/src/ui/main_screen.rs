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
const ORBIT_SPEED: f32 = 0.10;

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
    /// Render the gold "STARTING" title, centred on the panel. Used by
    /// the launcher Startup beat (PLAN 4.19.2a) to give the calm-starfield
    /// window a focal element — without it the screen reads as broken for
    /// the first second(s). The copy matches the spec's "state title"
    /// pattern (§3.1 state 1 reads as the launcher waking up); the full
    /// Main surface (QR + status + orbit pips) takes over after the
    /// transition completes.
    ///
    /// Painted via direct `Painter::text` calls (not `ui.heading`) so the
    /// embossed shadow stack + outer glow can land — `RichText` only
    /// supports a single solid colour, which produced the flat sticker
    /// look Chris flagged 2026-04-19. See `paint_heraldic_title`.
    pub(super) fn render_brand_intro(&self, ui: &mut egui::Ui, alpha: f32) {
        let rect = ui.max_rect();
        let pos = egui::pos2(rect.center().x, rect.top() + rect.height() * 0.5);
        // Spec §3.7 calls 96px the "TV display hero" floor for state
        // titles. The mock's actual rendered title is visibly larger —
        // takes ~50% of panel width vs ours at 25%. Bumped to 140px to
        // match the mock's on-screen presence (Chris's screenshot
        // 2026-04-19). render_main's "SKYLANDER PORTAL" steady-state
        // title is a separate drift item (4.19.19) still on 80px.
        //
        // `alpha` lets the dispatcher cross-fade the title against
        // the main content layer during the intro reveal so the
        // hand-off doesn't pop. paint_heraldic_title early-exits at
        // alpha≈0 so we don't burn the text-layout cost when faded
        // out.
        paint_heraldic_title(ui.painter(), pos, "STARTING", 140.0, alpha);
    }

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

            if let Some(tex) = &self.qr_texture {
                // The card-flip helper reserves a fixed-size square and
                // paints the front / back face directly via Painter so the
                // Y-axis rotation reads as a horizontal scale without any
                // custom matrix math — egui-native, see the helper doc.
                // The launch_phase factors layer ONTO the flip scale —
                // intro spin-in, close spin-out — and tint the QR/text
                // alpha so content fades independently of the scale.
                qr_card_flip(
                    ui,
                    ctx,
                    tex,
                    status_snapshot.session_slots_full,
                    launch_phase.badge_scale(),
                    launch_phase.badge_alpha(),
                    launch_phase.badge_text_alpha(),
                );
            }

            ui.add_space(24.0);

            // "SCAN TO CONNECT" label, heraldic — closes 4.19.10. Mock
            // calls this "TV display lg" (64px). Allocate a fixed-height
            // strip to land the painter call in the right rect, then
            // paint the title centred inside it. Alpha tracks the
            // badge text fade so the label disappears alongside the
            // QR content during the close-to-in-game animation.
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
                "SCAN TO CONNECT",
                64.0,
                launch_phase.badge_text_alpha(),
            );

            // Push the Exit to Desktop button toward the bottom of the
            // panel. Visible escape hatch — kept regardless of mock per
            // user 2026-04-19. Distinct red so it doesn't read as a
            // primary action competing with the QR.
            let remaining = ui.available_height();
            ui.add_space((remaining * 0.55).max(48.0));
            let btn = egui::Button::new(
                egui::RichText::new("Exit to Desktop")
                    .size(28.0)
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

/// Card-flip container (PLAN 4.15.6). Reserves a `CARD_SIZE × CARD_SIZE`
/// square and paints either the QR front face or the `MAXIMUM PLAYERS
/// REACHED` back face, with a tent-wave horizontal scale to simulate a
/// Y-axis rotation.
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
    flipped: bool,
    phase_scale: f32,
    bezel_alpha: f32,
    content_alpha: f32,
) {
    // `animate_bool_with_time` interpolates 0.0 → 1.0 when `flipped` goes
    // true, and 1.0 → 0.0 when it goes false. Unique id so multiple
    // instances don't alias animations.
    let id = egui::Id::new("launcher_qr_card_flip");
    let progress = ctx.animate_bool_with_time(id, flipped, FLIP_DURATION);

    // Tent wave: 1 at the edges, 0 in the middle. Represents the x-scale
    // of the visible face from the flip animation; hits 0 at the flip
    // midpoint where we swap content.
    let flip_scale = (progress * 2.0 - 1.0).abs();
    let show_back = progress > 0.5;

    // Final horizontal scale = flip × phase. The phase factor handles
    // the intro spin-in (0 → 1) and close spin-out (1 → 0); the flip
    // factor handles the in-place QR↔back-face card flip when the
    // session count saturates. Multiplying composes them naturally —
    // a flip mid-spin would scale to (mostly invisible) × (mostly
    // invisible), which is the right visual.
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
        paint_back_face(painter, inner, bezel_alpha, content_alpha);
    } else {
        paint_qr_front(painter, inner, tex, bezel_alpha, content_alpha);
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
    let shadow = with_alpha(
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 127),
        alpha,
    );
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

/// Back face — same circular bezel + dark screen rim + SF_1 content
/// disc as the QR front face, but with a 3-line `MAXIMUM PLAYERS
/// REACHED` title in Titan One instead of a QR. Thin wrapper around
/// the shared [`paint_titled_card`] helper so the back face and the
/// `server_error` surface (and any future card-style screens) stay
/// visually identical.
fn paint_back_face(
    painter: &egui::Painter,
    rect: egui::Rect,
    bezel_alpha: f32,
    text_alpha: f32,
) {
    paint_titled_card(
        painter,
        rect,
        &["MAXIMUM", "PLAYERS", "REACHED"],
        bezel_alpha,
        text_alpha,
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
    // don't blow up and tight 4-line cards stay readable.
    let font_size = (line_h * 0.55).clamp(20.0, 56.0);
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
    // Dark hairline well outside the bezel — matches the mock's
    // `0 0 0 2px #000` outer shadow, gives the pip a clean silhouette
    // against either the vortex or the starfield.
    painter.circle_stroke(
        centre,
        radius + 2.0,
        egui::Stroke::new(2.0, egui::Color32::BLACK),
    );
    // Gold bezel body.
    painter.circle_filled(centre, radius, palette::GOLD);
    // Inner gold-ink ring (the mock's `inset 0 0 0 3px var(--gi)`).
    painter.circle_stroke(
        centre,
        radius - 4.0,
        egui::Stroke::new(3.0, palette::GOLD_INK),
    );

    // Profile-colour fill inside the ring.
    let fill_radius = radius - 10.0;
    let fill = pip
        .color
        .as_deref()
        .and_then(parse_hex_color)
        .unwrap_or(palette::GOLD_BRIGHT);
    painter.circle_filled(centre, fill_radius, fill);

    // Initial (or a small dot if unknown).
    match pip.initial.as_deref() {
        Some(ch) if !ch.is_empty() => {
            painter.text(
                centre,
                egui::Align2::CENTER_CENTER,
                ch,
                egui::FontId::new(36.0, egui::FontFamily::Name(fonts::TITAN_ONE.into())),
                palette::GOLD_INK,
            );
        }
        _ => {
            painter.circle_filled(centre, 6.0, palette::GOLD_INK);
        }
    }
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
fn paint_heraldic_title(
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

/// Rasterise the pairing URL into a QR texture. Called once from
/// [`LauncherApp::new`]; the texture is cached on the app for the life of
/// the viewport.
/// Build the QR texture used by [`paint_qr_front`].
///
/// The texture is a circular composition: a white "screen" disc with the
/// real QR centered inside, surrounded by a ring of random blue
/// "noise" modules that make the overall shape read as round (not
/// square) — the portal aesthetic the launcher is built around.
///
/// Encoded at ECC level H so the ~30% Reed-Solomon recovery margin
/// covers any visual encroachment of the noise ring on the QR data.
/// The clear `QR_NOISE_GAP_MODULES`-wide quiet zone keeps standard
/// scanners (iOS Camera, Google Lens) locking on quickly. See the
/// `round_qr_spike` example for the variant trade-offs.
///
/// Pixels outside the inscribed noise circle are TRANSPARENT, so the
/// plate behind the texture shows through in the four corner triangles
/// of the texture's bounding rect. That intentional transparency is
/// what keeps the visual "round" inside a square card.
pub(super) fn render_qr_texture(ctx: &egui::Context, url: &str) -> egui::TextureHandle {
    use rand_core::{OsRng, RngCore};

    let code = qrcode::QrCode::with_error_correction_level(url, qrcode::EcLevel::H)
        .expect("qr encode");
    let n = code.width() as u32;
    let qr_modules: Vec<bool> = code
        .to_colors()
        .into_iter()
        .map(|c| matches!(c, qrcode::Color::Dark))
        .collect();

    // Pixel size of one QR module on the texture. Matches the spike
    // (`examples/round_qr_spike.rs`) value the user validated for scan
    // reliability at TV viewing distance.
    let scale: u32 = 14;
    // Canvas leaves the standard 4-module quiet zone reserved in its
    // dimensions, plus N modules of breathing room outside the noise
    // ring. Bumped 6→8 on 2026-04-19: at 6 the QR's diagonal CORNERS
    // sat at ~29.0 modules from center while the noise circle was at
    // ~29.5 modules — only ~0.5 module of diagonal quiet zone, which
    // mapped to ~3 screen pixels and made the QR read as touching
    // the SF_3 rim. 8 pushes the noise circle out to ~31.5 modules
    // for a comfortable diagonal quiet zone without shrinking the
    // QR data so much that scanning gets harder.
    let breathing: u32 = 8;
    let canvas_modules = n + 2 * (4 + breathing);
    let canvas_px = canvas_modules * scale;
    let img_w = canvas_px as usize;
    let img_h = canvas_px as usize;

    let qr_origin = (canvas_modules - n) / 2;
    let qr_center_px = (canvas_px / 2) as f32;
    // Noise circle inscribes within the canvas with one module of edge
    // padding so it doesn't kiss the bounding box.
    let noise_radius_px = (canvas_px as f32 / 2.0) - (scale as f32);
    // Half-extent (px) of the reserved central square: QR data + the
    // configurable gap. Noise modules whose centers fall inside this
    // axis-aligned square are skipped, leaving a clear quiet zone.
    let reserved_half_px =
        (n as f32 / 2.0 + QR_NOISE_GAP_MODULES as f32) * scale as f32;

    // Pre-generate enough random bits for the noise ring. One bit per
    // candidate module — the loop below skips most (outside the circle
    // or inside the reserved square), so the buffer is sized by total
    // module count and any unused bits are just discarded.
    let mut noise_buf = vec![0u8; (canvas_modules * canvas_modules / 8 + 1) as usize];
    OsRng.fill_bytes(&mut noise_buf);
    let bit_at = |idx: u32| -> bool {
        let i = (idx as usize) % (noise_buf.len() * 8);
        (noise_buf[i / 8] >> (i % 8)) & 1 == 1
    };

    // Colour roles:
    // - QR data dark modules: pure black for max scanner contrast on
    //   the white screen disc.
    // - Noise dots: SF_2 — darker than SF_1, gives stronger contrast
    //   against the white screen at typical viewing distance. Tried
    //   SF_1 first (2026-04-19) but at downsampled scale the noise
    //   washed out; SF_2 reads more clearly without going so dark
    //   that it competes with the QR data itself.
    // - Screen background: white (standard QR quiet-zone colour).
    // - Outside the circle: transparent so the SF_3 screen rim behind
    //   the texture shows through in the inscribed-square corners.
    let qr_dark = egui::Color32::BLACK;
    let noise_blue = palette::SF_2;
    let screen_white = egui::Color32::WHITE;
    let outside = egui::Color32::TRANSPARENT;

    let mut pixels = vec![outside; img_w * img_h];

    // Pass 1: fill the inscribed circle with white. Per-pixel circle
    // test — the noise ring + QR will overpaint specific modules on
    // top, but every pixel inside the circle starts as white so the
    // scanner sees clean quiet-zone background where noise is sparse.
    let r2 = noise_radius_px * noise_radius_px;
    for y in 0..canvas_px {
        let dy = y as f32 + 0.5 - qr_center_px;
        let dy2 = dy * dy;
        for x in 0..canvas_px {
            let dx = x as f32 + 0.5 - qr_center_px;
            if dx * dx + dy2 <= r2 {
                pixels[(y as usize) * img_w + (x as usize)] = screen_white;
            }
        }
    }

    // Pass 2: noise modules. Iterate per-module (not per-pixel) so each
    // "noise dot" is a crisp QR-sized square — visually consistent
    // with the real QR's data modules. Per-pixel circle clip during
    // painting: a module whose CENTER is just inside the noise circle
    // would otherwise have its 14×14 box extend a few pixels past
    // the circle boundary into the texture's transparent corner zone,
    // producing blue dots that bleed into the SF_3 screen rim around
    // the bezel (Chris flagged 2026-04-19). The center test alone
    // isn't enough; we also clip each painted pixel to the circle.
    let mut bit_idx = 0u32;
    for my in 0..canvas_modules {
        for mx in 0..canvas_modules {
            let cx = (mx as f32 + 0.5) * scale as f32;
            let cy = (my as f32 + 0.5) * scale as f32;
            let dx = cx - qr_center_px;
            let dy = cy - qr_center_px;
            let dist_sq = dx * dx + dy * dy;
            if dist_sq > r2 {
                continue;
            }
            if dx.abs() < reserved_half_px && dy.abs() < reserved_half_px {
                continue;
            }
            let dark = bit_at(bit_idx);
            bit_idx = bit_idx.wrapping_add(1);
            if !dark {
                continue;
            }
            let x0 = (mx * scale) as usize;
            let y0 = (my * scale) as usize;
            let s = scale as usize;
            for ddy in 0..s {
                let py = y0 + ddy;
                let dpy = py as f32 + 0.5 - qr_center_px;
                let dpy2 = dpy * dpy;
                for ddx in 0..s {
                    let px = x0 + ddx;
                    let dpx = px as f32 + 0.5 - qr_center_px;
                    if dpx * dpx + dpy2 <= r2 {
                        pixels[py * img_w + px] = noise_blue;
                    }
                }
            }
        }
    }

    // Pass 3: real QR data on top — guaranteed not to overlap with
    // noise (the reserved square is bigger than the QR by at least
    // QR_NOISE_GAP_MODULES on every side).
    for y in 0..n {
        for x in 0..n {
            if qr_modules[(y * n + x) as usize] {
                paint_module_into(
                    &mut pixels,
                    img_w,
                    qr_origin + x,
                    qr_origin + y,
                    scale,
                    qr_dark,
                );
            }
        }
    }

    let color_image = egui::ColorImage {
        size: [img_w, img_h],
        pixels,
    };
    ctx.load_texture("qr", color_image, egui::TextureOptions::NEAREST)
}

/// Fill a `scale × scale` block in the texture pixel buffer at module
/// coordinates `(mx, my)` with `color`. Used by [`render_qr_texture`].
fn paint_module_into(
    pixels: &mut [egui::Color32],
    stride: usize,
    mx: u32,
    my: u32,
    scale: u32,
    color: egui::Color32,
) {
    let x0 = (mx * scale) as usize;
    let y0 = (my * scale) as usize;
    let s = scale as usize;
    for dy in 0..s {
        let row = (y0 + dy) * stride;
        for dx in 0..s {
            pixels[row + x0 + dx] = color;
        }
    }
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
