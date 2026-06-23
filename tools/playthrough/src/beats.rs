//! PLAN 15.x — recorder **beats & narrative** framework (design:
//! `docs/dev/recorder-beats-framework.md`, phases 1-3).
//!
//! A demo is modelled as ordered **beats**, grouped into a **narrative**
//! (one stitched MP4) locked to a single [`ServerFlavor`]. Each beat is a
//! boxed async closure over a [`BeatCtx`] — the imperative `js_click` →
//! `wait_for` → `sleep` drive bodies are moved here **verbatim** from the
//! monolithic scenarios that used to live in `main.rs` (no behaviour change).
//!
//! The framework adds **editorial** metadata per beat ([`Beat::realtime_head`]
//! / `realtime_tail` / `filler_speed` / `crop`) that the recorder stamps into a
//! `timeline.json` manifest (design §5). That manifest drives the `-- render`
//! post-pass (phase 4, `render.rs`) — it does not change what the recorder
//! captures.
//!
//! The IPC marquee ends with a `kaos` beat that fires a REAL Kaos swap
//! (PLAN A.3 — `fire_kaos_swap` → server `select_swap` + `execute_kaos_swap`).
//! Per-beat `caption` text (PLAN A.5) flows through the manifest into the
//! render pass's lower-third overlay.

use std::time::Duration;

use anyhow::{Context, Result};
use fantoccini::Locator;
use serde_json::json;
use skylander_e2e_tests::{
    Phone, TestServer, fire_kaos_swap, inject_load_outcomes, unlock_session,
};

use crate::timeline::CropRect;

/// Per-beat execution context. The harness is `&self`-async throughout, so a
/// beat just borrows these for the duration of its drive. The profile id
/// ([`BeatCtx::alice`]) is injected once at boot and threaded through — beats
/// must **not** re-inject per beat (design §10 "State threading").
pub struct BeatCtx<'a> {
    pub phone: &'a Phone,
    pub server: &'a TestServer,
    /// `place_figure`'s reload-to-foreground step needs the phone URL.
    pub phone_url: &'a str,
    /// Profile id from the boot's `inject_profile` — used by `pick_profile`'s
    /// PIN-bypass unlock.
    pub alice: &'a str,
}

/// A beat's drive future. Boxed + pinned so the `fn`-pointer registry stays
/// `'static` (design §2): each beat is a free `async fn` wrapped by a one-line
/// `|c| Box::pin(beat_x(c))` shim that performs the HRTB lifetime coercion.
pub type BeatFut<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + 'a>>;

/// Plain `fn` pointer to a beat drive (no `Box<dyn Fn>` — keeps [`Beat`]
/// `'static` and the registry a simple builder). The `for<'a>` HRTB binds the
/// context borrow to the returned future.
pub type DriveFn = for<'a> fn(&'a BeatCtx<'a>) -> BeatFut<'a>;

/// One beat: a named drive plus editorial intent for the render post-pass.
/// The crop type lives in [`crate::timeline`] (the manifest schema module);
/// all v1 beats use `crop: None` — the narrative-wide `stage` (PLAN 15.14)
/// provides the framing instead.
pub struct Beat {
    /// CLI key for `-- beat <name>`.
    pub name: &'static str,
    /// Imperative JS+wait sequence (verbatim from today's scenarios).
    pub drive: DriveFn,
    /// Whether this beat's drive requires the IPC flavor (design §7). Set on
    /// `pick_game_ipc` / `place_figure_ipc` / `see_in_game`; `false` elsewhere.
    /// The flavor-lock guard reads this field directly — a data-driven flag,
    /// not fn-pointer identity (which is unspecified across codegen units).
    pub requires_ipc: bool,
    // --- editorial (design §5; consumed by the render pass, not capture) ---
    /// Keep this much at 1× at the start (the action).
    pub realtime_head: Duration,
    /// Keep this much at 1× at the end (the reveal).
    pub realtime_tail: Duration,
    /// Play the dead middle at this speed (e.g. 8.0); 1.0 = no speed-up.
    pub filler_speed: f32,
    /// Post-crop framing (None = full desktop frame).
    pub crop: Option<CropRect>,
    /// Lower-third caption shown during this beat's OUTPUT window (PLAN A.5).
    /// `None` = no caption. Flows through `entry_for` → the manifest →
    /// `render_caption_png` → an `overlay` (this ffmpeg has no `drawtext`).
    pub caption: Option<&'static str>,
}

/// Server setup a narrative requires. A narrative is **locked to one flavor**
/// (design §7) — beats are not freely composable across Mock and IPC because
/// `place_figure` genuinely differs (injected mock outcome vs real IPC `LOAD`)
/// and `pick_game`/`see_in_game` are IPC-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerFlavor {
    /// Mock driver, Desktop window mode, Giants pre-launched (no RPCS3).
    Mock,
    /// IPC driver booting a real Spyro save state on the patched RPCS3.
    IpcSavestate,
}

/// An ordered, flavor-locked sequence of beats rendered to one stitched MP4.
pub struct Narrative {
    pub name: &'static str,
    pub flavor: ServerFlavor,
    pub beats: Vec<Beat>,
}

// ---------------------------------------------------------------- beat drives
//
// Each `beat_*` async fn holds a drive body extracted VERBATIM from the
// scenarios that used to live in `main.rs` (`main()` body, `ingame()`,
// `place_figures()`, `place_one()`). Keep the steps byte-for-byte so the
// recorded flow is unchanged.

/// `connect` — wait for the profile picker to mount. The QR/connect framing is
/// the hold here.
async fn connect(ctx: &BeatCtx<'_>) -> Result<()> {
    ctx.phone
        .wait_for(Locator::Css(".profile-picker"), Duration::from_secs(20))
        .await
        .context("profile picker never mounted")?;
    Ok(())
}

/// `pick_profile` — unlock Alice via the PIN-bypass test hook.
async fn pick_profile(ctx: &BeatCtx<'_>) -> Result<()> {
    unlock_session(&ctx.server.url, ctx.alice).await?;
    Ok(())
}

/// `reach_portal` (Mock) — after the PIN-bypass unlock, wait (warn-on-fail) for
/// the portal view to mount. Extracted verbatim from the old mock `main()`,
/// which gated on `.portal-p4` (15s) before any further Mock drive. **Does not
/// hard-error** — a warn keeps the recording going even if the portal is slow,
/// matching the original. The IPC narrative reaches the portal inside
/// `pick_game_ipc` instead, so this beat is Mock-only.
async fn reach_portal(ctx: &BeatCtx<'_>) -> Result<()> {
    if let Err(e) = ctx
        .phone
        .wait_for(Locator::Css(".portal-p4"), Duration::from_secs(15))
        .await
    {
        tracing::warn!(error = %e, "portal view never mounted (continuing anyway)");
    }
    Ok(())
}

/// `pick_game` (IPC) — tap the Spyro card (falls back to the first) to fire a
/// REAL signed `/api/launch`, which the server turns into a save-state boot.
/// Then wait out the save-state resume to the in-game portal. Extracted
/// verbatim from `ingame()`.
async fn pick_game_ipc(ctx: &BeatCtx<'_>) -> Result<()> {
    let phone = ctx.phone;
    phone
        .wait_for(Locator::Css(".game-card"), Duration::from_secs(15))
        .await
        .context("game picker never showed game cards")?;

    // Tap the Spyro card (falls back to the first) — a REAL signed /api/launch,
    // which the server turns into a save-state boot (SKYLANDER_BOOT_SAVESTATE
    // overrides the resolved EBOOT).
    let launched = phone
        .client
        .execute(
            r#"const cards=[...document.querySelectorAll('.game-card')];
               const c=cards.find(el=>/spyro/i.test(el.textContent||''))||cards[0];
               if(c){c.click();return true;} return false;"#,
            vec![],
        )
        .await?
        .as_bool()
        .unwrap_or(false);
    if !launched {
        anyhow::bail!("no game card to launch in the picker");
    }
    tracing::info!("launched a game from the picker → server booting the save state");

    // Save-state resume + the server's is_playable wait can take a while
    // (shader compile is mostly cached for a resumed state, but allow headroom).
    phone
        .wait_for(Locator::Css(".portal-p4"), Duration::from_secs(180))
        .await
        .context("in-game portal never reached (save-state boot)")?;
    tracing::info!("portal reached — RPCS3 resumed the save state at the in-game portal");
    tokio::time::sleep(Duration::from_secs(2)).await; // let the portal settle
    Ok(())
}

/// `open_toybox` — two lid-grabber pointer taps to fully open the drawer, then
/// wait for the collection grid (`.fig-card-p4`). This is the "browse
/// collection" beat (the grid lives in the drawer). Extracted verbatim from
/// `place_figures()` / `ingame()`.
async fn open_toybox(ctx: &BeatCtx<'_>) -> Result<()> {
    let phone = ctx.phone;
    // Open the toy box: the lid grabber cycles Closed → Compact → Expanded
    // on PointerEvents (two taps to fully open).
    phone.tap_pointer(".lid-grabber-p4").await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    phone.tap_pointer(".lid-grabber-p4").await?;
    phone
        .wait_for(Locator::Css(".fig-card-p4"), Duration::from_secs(8))
        .await
        .context("toy box cards never appeared")?;
    tokio::time::sleep(Duration::from_millis(400)).await;
    Ok(())
}

/// `place_figure` (Mock) — inject two `ok` load outcomes, place two figures,
/// then reload to the lid-closed foreground so the loaded slots are the focus.
/// Extracted verbatim from `place_figures()` (minus the lid-open, which is now
/// the `open_toybox` beat).
async fn place_figure_mock(ctx: &BeatCtx<'_>) -> Result<()> {
    let phone = ctx.phone;
    let server = ctx.server;
    let phone_url = ctx.phone_url;

    inject_load_outcomes(&server.url, json!([{"kind": "ok"}, {"kind": "ok"}])).await?;

    // Place the first figure, wait for a Loaded slot.
    place_one(phone, ".fig-card-p4:not(.scan-new)").await?;
    let _ = phone
        .wait_until(Duration::from_secs(8), || async {
            phone
                .client
                .find(Locator::Css(".p4-slot--loaded"))
                .await
                .is_ok()
        })
        .await;

    // Place a second (skipping the one now on the portal), wait for two Loaded.
    place_one(phone, ".fig-card-p4:not(.scan-new):not(.on-portal)").await?;
    let _ = phone
        .wait_until(Duration::from_secs(8), || async {
            phone
                .client
                .find_all(Locator::Css(".p4-slot--loaded"))
                .await
                .unwrap_or_default()
                .len()
                >= 2
        })
        .await;

    // Reload to the default lid-Closed foreground (synthetic lid taps don't
    // reliably reach the tap detector); placed figures persist via ghost
    // reclaim, so the loaded slots come back as the focus.
    phone.client.goto(phone_url).await?;
    phone
        .wait_for(Locator::Css(".portal-p4"), Duration::from_secs(15))
        .await?;
    let _ = phone
        .wait_until(Duration::from_secs(8), || async {
            phone
                .client
                .find(Locator::Css(".p4-slot--loaded"))
                .await
                .is_ok()
        })
        .await;
    Ok(())
}

/// `place_figure` (IPC) — pick a recognizable figure by name and place it as a
/// REAL IPC `LOAD` onto the resumed save state (NO mock outcomes). Extracted
/// verbatim from `ingame()`'s placement block. Assumes the toy box is open (the
/// `open_toybox` beat ran first).
async fn place_figure_ipc(ctx: &BeatCtx<'_>) -> Result<()> {
    let phone = ctx.phone;

    // Pick by name from the toy box (the `.fig-name-p4` label holds the group
    // name), so the demo places an iconic figure rather than whatever sorts
    // first. Falls back Eruptor → Spyro → first card; scrolls it into view so
    // the click lands.
    let picked = phone
        .client
        .execute(
            r#"const cards=[...document.querySelectorAll('.fig-card-p4:not(.scan-new):not(.on-portal)')];
               const byName=re=>cards.find(c=>{const n=c.querySelector('.fig-name-p4');return n&&re.test((n.textContent||'').trim());});
               const pick=byName(/^eruptor$/i)||byName(/spyro/i)||cards[0];
               if(pick){pick.scrollIntoView({block:'center'});pick.click();
                 return ((pick.querySelector('.fig-name-p4')||{}).textContent||'?').trim();}
               return '';"#,
            vec![],
        )
        .await?;
    let figure_name = picked.as_str().unwrap_or("").to_string();
    if figure_name.is_empty() {
        anyhow::bail!("no figure card available to place in the toy box");
    }
    tracing::info!(figure = %figure_name, "picked figure from toy box → opening detail");

    // PLACE-ON-PORTAL flow (the card click above opened the detail overlay).
    phone
        .wait_for(Locator::Css(".detail-btn-primary"), Duration::from_secs(6))
        .await
        .context("figure detail PLACE button never appeared")?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    phone.js_click(".detail-btn-primary").await?; // PLACE ON PORTAL
    tokio::time::sleep(Duration::from_millis(400)).await;
    phone.js_click(".detail-btn-secondary").await.ok(); // BACK TO BOX (best-effort)
    let _ = phone
        .wait_until(Duration::from_secs(15), || async {
            phone
                .client
                .find(Locator::Css(".p4-slot--loaded"))
                .await
                .is_ok()
        })
        .await;
    tracing::info!(figure = %figure_name, "figure placed — real IPC LOAD onto the save state");
    Ok(())
}

/// `see_in_game` (IPC only) — hold ~16s so the resumed game re-reads the portal
/// and the figure visibly LANDS on RPCS3's own in-game portal (the climax).
/// Extracted verbatim from `ingame()`'s final settle.
async fn see_in_game(_ctx: &BeatCtx<'_>) -> Result<()> {
    // Hold ~16s so the resumed game re-reads the portal and the figure visibly
    // LANDS on the in-game portal (and stays on screen for the clip). The
    // previous 4s wasn't enough to catch it landing.
    tokio::time::sleep(Duration::from_secs(16)).await;
    Ok(())
}

/// `kaos` (IPC marquee ending) — the playful twist: pause, then fire a REAL
/// Kaos swap (PLAN A.3) so a portal figure is cleared and a compatible
/// replacement LOADs in its place (the overlay + taunt show on the phone, the
/// new figure lands in-game). Holds while it lands so the swap reads on the clip.
async fn kaos_swap(ctx: &BeatCtx<'_>) -> Result<()> {
    // Let `see_in_game` settle so the swap reads as a separate "and then… Kaos
    // strikes" beat rather than blurring into the placement.
    tokio::time::sleep(Duration::from_secs(3)).await;
    // Real swap: ClearSlot -> LoadFigure on Alice's portal slot + the taunt
    // overlay. Non-fatal server-side if the portal has nothing swappable.
    fire_kaos_swap(&ctx.server.url, ctx.alice).await?;
    // Hold while the overlay plays + the new figure loads onto the portal and
    // re-reads in-game (mirrors see_in_game's landing window).
    tokio::time::sleep(Duration::from_secs(14)).await;
    Ok(())
}

/// `hold_portal` (Mock, `portal` narrative only) — hold the empty portal for
/// 5s so the QR → profile → empty-portal arc reads on the clip. Extracted
/// verbatim from the old `portal` scenario's trailing `sleep(5s)` (on top of
/// the shared run-narrative tail). No DOM; the `place` narrative does NOT use
/// this (it goes straight to `open_toybox`).
async fn hold_portal(_ctx: &BeatCtx<'_>) -> Result<()> {
    tokio::time::sleep(Duration::from_secs(5)).await;
    Ok(())
}

/// `settle_after_reconnect` (IPC marquee only) — after the save-state RECONNECT
/// (the hot-plug re-attach fired server-side during `pick_game`), let the guest
/// finish re-enumerating the portal before the live LOAD. The first marquee run
/// loaded ~2s after RECONNECT and the RSX stalled; the working manual P5 test
/// spaced the LOAD ~27s after RECONNECT. No DOM — just a settle (the render's
/// fast filler skips it).
async fn settle_after_reconnect(_ctx: &BeatCtx<'_>) -> Result<()> {
    tokio::time::sleep(Duration::from_secs(20)).await;
    Ok(())
}

/// Tap the first card matching `card_sel`, hit PLACE ON PORTAL, return to box.
/// All clicks go through `js_click` (bypasses WebDriver interactability so a
/// card caught mid-animation or behind a closing overlay still fires), and we
/// wait for the detail overlay to fully dismiss before returning so the next
/// pick lands on an interactable grid. Extracted verbatim from `place_one()`.
async fn place_one(phone: &Phone, card_sel: &str) -> Result<()> {
    if !phone.js_click(card_sel).await? {
        tracing::warn!(card_sel, "no figure card to place");
        return Ok(());
    }
    phone
        .wait_for(Locator::Css(".detail-btn-primary"), Duration::from_secs(5))
        .await
        .context("figure detail PLACE button")?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    phone.js_click(".detail-btn-primary").await?; // PLACE ON PORTAL
    tokio::time::sleep(Duration::from_millis(400)).await;
    phone.js_click(".detail-btn-secondary").await.ok(); // BACK TO BOX (best-effort)
    let _ = phone
        .wait_until(Duration::from_secs(4), || async {
            phone
                .client
                .find(Locator::Css(".detail-btn-primary"))
                .await
                .is_err()
        })
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    Ok(())
}

// ---------------------------------------------------------------- registry
//
// Each beat is constructed with a `|c| Box::pin(beat_x(c))` shim for the
// `DriveFn` HRTB coercion (design §2). Editorial defaults are picked per beat
// (design §9.1): action beats get a short head + 1× filler; the `see_in_game`
// reveal gets a small head, a large tail (the figure-appears moment), and a
// fast filler for the dead resume.

/// `connect` — the QR/connect framing hold.
fn beat_connect() -> Beat {
    Beat {
        name: "connect",
        drive: |c| Box::pin(connect(c)),
        requires_ipc: false,
        realtime_head: Duration::from_secs(1),
        realtime_tail: Duration::from_secs(2),
        filler_speed: 1.0,
        crop: None,
        caption: Some("Scan the code. Your phone's the portal now."),
    }
}

/// `pick_profile` — PIN-bypass unlock of Alice.
fn beat_pick_profile() -> Beat {
    Beat {
        name: "pick_profile",
        drive: |c| Box::pin(pick_profile(c)),
        requires_ipc: false,
        realtime_head: Duration::from_millis(500),
        realtime_tail: Duration::from_secs(1),
        filler_speed: 1.0,
        crop: None,
        caption: Some("Welcome back, Portal Master."),
    }
}

/// `reach_portal` — Mock-only warn-on-fail wait for the portal view (design
/// fix 1). Editorial mirrors a short browse hold.
fn beat_reach_portal() -> Beat {
    Beat {
        name: "reach_portal",
        drive: |c| Box::pin(reach_portal(c)),
        requires_ipc: false,
        realtime_head: Duration::from_secs(1),
        realtime_tail: Duration::from_secs(2),
        filler_speed: 1.0,
        crop: None,
        caption: None,
    }
}

/// `hold_portal` — `portal`-narrative-only empty-portal 5s hold (design fix 2).
fn beat_hold_portal() -> Beat {
    Beat {
        name: "hold_portal",
        drive: |c| Box::pin(hold_portal(c)),
        requires_ipc: false,
        realtime_head: Duration::from_secs(1),
        realtime_tail: Duration::from_secs(2),
        filler_speed: 1.0,
        crop: None,
        caption: None,
    }
}

/// `pick_game` — IPC variant (real `/api/launch` → save-state boot). The dead
/// resume wait is long, so filler runs fast and only the tail (portal appears)
/// is kept at 1×.
fn beat_pick_game_ipc() -> Beat {
    Beat {
        name: "pick_game",
        drive: |c| Box::pin(pick_game_ipc(c)),
        requires_ipc: true,
        realtime_head: Duration::from_secs(1),
        realtime_tail: Duration::from_secs(3),
        filler_speed: 8.0,
        crop: None,
        caption: Some("Pick a game — it boots on the TV."),
    }
}

/// `settle_after_reconnect` — IPC marquee only; let the guest re-enumerate the
/// portal after RECONNECT before the LOAD (the timing fix). Fast filler.
fn beat_settle_after_reconnect() -> Beat {
    Beat {
        name: "settle_after_reconnect",
        drive: |c| Box::pin(settle_after_reconnect(c)),
        requires_ipc: false,
        realtime_head: Duration::from_secs(1),
        realtime_tail: Duration::from_secs(1),
        filler_speed: 8.0,
        crop: None,
        caption: None,
    }
}

/// `open_toybox` — browse the collection grid in the drawer.
fn beat_open_toybox() -> Beat {
    Beat {
        name: "open_toybox",
        drive: |c| Box::pin(open_toybox(c)),
        requires_ipc: false,
        realtime_head: Duration::from_secs(1),
        realtime_tail: Duration::from_secs(2),
        filler_speed: 1.0,
        crop: None,
        caption: Some("Your collection, digital."),
    }
}

/// `place_figure` — Mock variant (injected outcomes, two figures). Shares the
/// `place_figure` CLI name with [`beat_place_figure_ipc`]; flavor-locked.
fn beat_place_figure_mock() -> Beat {
    Beat {
        name: "place_figure",
        drive: |c| Box::pin(place_figure_mock(c)),
        requires_ipc: false,
        realtime_head: Duration::from_secs(1),
        realtime_tail: Duration::from_secs(2),
        filler_speed: 1.0,
        crop: None,
        caption: None,
    }
}

/// `place_figure` — IPC variant (real LOAD onto the save state).
fn beat_place_figure_ipc() -> Beat {
    Beat {
        name: "place_figure",
        drive: |c| Box::pin(place_figure_ipc(c)),
        requires_ipc: true,
        realtime_head: Duration::from_secs(1),
        realtime_tail: Duration::from_secs(2),
        filler_speed: 1.0,
        crop: None,
        caption: Some("Tap a figure onto the portal."),
    }
}

/// `see_in_game` — the climax: a small head, a large tail for the figure-
/// appears reveal, fast filler for the dead settle (design §9.1).
fn beat_see_in_game() -> Beat {
    Beat {
        name: "see_in_game",
        drive: |c| Box::pin(see_in_game(c)),
        requires_ipc: true,
        realtime_head: Duration::from_secs(1),
        realtime_tail: Duration::from_secs(6),
        filler_speed: 8.0,
        crop: None,
        caption: Some("It's in the game. No toy touched."),
    }
}

/// `kaos` — the IPC marquee's playful ending: fire a real Kaos swap. Small
/// head (the taunt hits), large tail (the new figure lands — the reveal), fast
/// filler for the dead hold. IPC-only (the swap LOADs onto the save state).
fn beat_kaos() -> Beat {
    Beat {
        name: "kaos",
        drive: |c| Box::pin(kaos_swap(c)),
        requires_ipc: true,
        realtime_head: Duration::from_secs(2),
        realtime_tail: Duration::from_secs(6),
        filler_speed: 4.0,
        crop: None,
        caption: Some("Beware, Kaos can strike anytime!"),
    }
}

/// Build every narrative. **Fails fast** (design §7) if a Mock narrative lists
/// an IPC-only beat — a clear error at startup, not mid-run.
pub fn narratives() -> Result<Vec<Narrative>> {
    let list = vec![
        // `portal` = [connect, pick_profile, reach_portal, hold_portal] — Mock.
        Narrative {
            name: "portal",
            flavor: ServerFlavor::Mock,
            beats: vec![
                beat_connect(),
                beat_pick_profile(),
                beat_reach_portal(),
                beat_hold_portal(),
            ],
        },
        // `place` = [connect, pick_profile, reach_portal, open_toybox,
        // place_figure(mock)] — Mock.
        Narrative {
            name: "place",
            flavor: ServerFlavor::Mock,
            beats: vec![
                beat_connect(),
                beat_pick_profile(),
                beat_reach_portal(),
                beat_open_toybox(),
                beat_place_figure_mock(),
            ],
        },
        // `ingame` / "marquee" = [connect, pick_profile, pick_game,
        // settle_after_reconnect, open_toybox, place_figure(ipc), see_in_game] — IPC.
        Narrative {
            name: "ingame",
            flavor: ServerFlavor::IpcSavestate,
            beats: vec![
                beat_connect(),
                beat_pick_profile(),
                beat_pick_game_ipc(),
                beat_settle_after_reconnect(),
                beat_open_toybox(),
                beat_place_figure_ipc(),
                beat_see_in_game(),
                beat_kaos(),
            ],
        },
    ];

    for n in &list {
        validate_flavor_lock(n)?;
    }
    Ok(list)
}

/// Enforce the §7 flavor lock: a Mock narrative must not contain any IPC-only
/// beat. (IPC narratives accept any beat — IPC is the superset.) Reads the
/// data-driven [`Beat::requires_ipc`] flag — no fn-pointer identity (design
/// fix 5).
fn validate_flavor_lock(n: &Narrative) -> Result<()> {
    if n.flavor == ServerFlavor::Mock {
        for b in &n.beats {
            if b.requires_ipc {
                anyhow::bail!(
                    "flavor-lock violation: narrative {:?} is Mock but lists IPC-only beat {:?} \
                     — IPC-only beats (pick_game real-launch / place_figure_ipc / see_in_game) \
                     require the IpcSavestate flavor (design §7)",
                    n.name,
                    b.name,
                );
            }
        }
    }
    Ok(())
}

/// The default narrative when `-- narrative` is given no name and for a bare
/// invocation: the IPC marquee (design §6).
pub const MARQUEE: &str = "ingame";

/// Map the back-compat bare aliases (`portal` / `place` / `ingame`) to their
/// narrative names. They are 1:1 with the registry names today, but the
/// indirection keeps the alias contract explicit (design §6).
pub fn resolve_alias(arg: &str) -> Option<&'static str> {
    match arg {
        "portal" => Some("portal"),
        "place" => Some("place"),
        "ingame" => Some("ingame"),
        _ => None,
    }
}

/// Find a narrative by name in the validated registry.
pub fn find_narrative(narrs: Vec<Narrative>, name: &str) -> Option<Narrative> {
    narrs.into_iter().find(|n| n.name == name)
}

/// Find the narrative that OWNS a given beat name (for `-- beat <name>`,
/// which boots the flavor of the beat's owning narrative — design §6). Returns
/// the owning narrative's flavor + the matching [`Beat`]. When a beat name
/// appears in multiple narratives (the dual-flavor `pick_game` / `place_figure`),
/// the marquee (IPC) wins so a single-beat clip exercises the richer path.
pub fn find_beat(narrs: Vec<Narrative>, beat_name: &str) -> Option<(ServerFlavor, Beat)> {
    // Prefer the marquee so dual-flavor beats default to their IPC variant.
    let mut ordered: Vec<Narrative> = Vec::with_capacity(narrs.len());
    let mut rest: Vec<Narrative> = Vec::new();
    for n in narrs {
        if n.name == MARQUEE {
            ordered.push(n);
        } else {
            rest.push(n);
        }
    }
    ordered.extend(rest);

    for n in ordered {
        let flavor = n.flavor;
        if let Some(b) = n.beats.into_iter().find(|b| b.name == beat_name) {
            return Some((flavor, b));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped registry builds + passes the flavor lock.
    #[test]
    fn registry_builds_and_validates() {
        let narrs = narratives().expect("registry should build + validate");
        let names: Vec<_> = narrs.iter().map(|n| n.name).collect();
        assert_eq!(names, vec!["portal", "place", "ingame"]);
    }

    /// Fix 1: the Mock `portal`/`place` narratives include the `reach_portal`
    /// gate, and `portal` ends on `hold_portal` (fix 2).
    #[test]
    fn mock_narratives_have_reach_portal_gate() {
        let narrs = narratives().unwrap();
        let portal = narrs.iter().find(|n| n.name == "portal").unwrap();
        let portal_beats: Vec<_> = portal.beats.iter().map(|b| b.name).collect();
        assert_eq!(
            portal_beats,
            vec!["connect", "pick_profile", "reach_portal", "hold_portal"]
        );

        let place = narrs.iter().find(|n| n.name == "place").unwrap();
        let place_beats: Vec<_> = place.beats.iter().map(|b| b.name).collect();
        assert_eq!(
            place_beats,
            vec![
                "connect",
                "pick_profile",
                "reach_portal",
                "open_toybox",
                "place_figure"
            ]
        );
        // `hold_portal` is portal-only (fix 2): never in `place`.
        assert!(!place_beats.contains(&"hold_portal"));
    }

    #[test]
    fn resolve_alias_maps_known_and_rejects_unknown() {
        assert_eq!(resolve_alias("portal"), Some("portal"));
        assert_eq!(resolve_alias("place"), Some("place"));
        assert_eq!(resolve_alias("ingame"), Some("ingame"));
        assert_eq!(resolve_alias("nope"), None);
    }

    /// Fix 5: the flavor lock is data-driven and bails when a Mock narrative
    /// contains a `requires_ipc` beat.
    #[test]
    fn flavor_lock_rejects_ipc_beat_in_mock_narrative() {
        let bad = Narrative {
            name: "bad",
            flavor: ServerFlavor::Mock,
            beats: vec![beat_connect(), beat_see_in_game()], // see_in_game is IPC-only
        };
        let err = validate_flavor_lock(&bad)
            .expect_err("a Mock narrative with an IPC-only beat must fail fast");
        let msg = err.to_string();
        assert!(msg.contains("flavor-lock violation"), "got: {msg}");
        assert!(msg.contains("see_in_game"), "got: {msg}");
    }

    /// Fix 5: a Mock narrative containing only Mock beats passes.
    #[test]
    fn flavor_lock_accepts_all_mock_narrative() {
        let ok = Narrative {
            name: "ok",
            flavor: ServerFlavor::Mock,
            beats: vec![beat_connect(), beat_pick_profile(), beat_reach_portal()],
        };
        validate_flavor_lock(&ok).expect("all-Mock narrative should pass");
    }

    /// `requires_ipc` is set exactly on the three IPC-only beats.
    #[test]
    fn requires_ipc_flag_is_set_on_ipc_beats_only() {
        assert!(beat_pick_game_ipc().requires_ipc);
        assert!(beat_place_figure_ipc().requires_ipc);
        assert!(beat_see_in_game().requires_ipc);
        assert!(!beat_connect().requires_ipc);
        assert!(!beat_pick_profile().requires_ipc);
        assert!(!beat_reach_portal().requires_ipc);
        assert!(!beat_hold_portal().requires_ipc);
        assert!(!beat_open_toybox().requires_ipc);
        assert!(!beat_place_figure_mock().requires_ipc);
    }

    /// `find_beat` resolves a unique beat to its owning narrative's flavor, and
    /// resolves a dual-flavor beat to the IPC marquee variant (design §6).
    #[test]
    fn find_beat_resolves_owner_and_prefers_marquee() {
        // Mock-only beats (`reach_portal` / `hold_portal` live only in the Mock
        // narratives) → Mock flavor.
        let (flavor, beat) = find_beat(narratives().unwrap(), "reach_portal").unwrap();
        assert_eq!(flavor, ServerFlavor::Mock);
        assert_eq!(beat.name, "reach_portal");
        let (flavor, _) = find_beat(narratives().unwrap(), "hold_portal").unwrap();
        assert_eq!(flavor, ServerFlavor::Mock);

        // IPC-only beat → IPC flavor.
        let (flavor, beat) = find_beat(narratives().unwrap(), "see_in_game").unwrap();
        assert_eq!(flavor, ServerFlavor::IpcSavestate);
        assert_eq!(beat.name, "see_in_game");

        // `open_toybox` lives in BOTH `place` (Mock) and `ingame` (IPC); the
        // marquee wins, so it resolves to the IPC flavor (design §6).
        let (flavor, _) = find_beat(narratives().unwrap(), "open_toybox").unwrap();
        assert_eq!(flavor, ServerFlavor::IpcSavestate);

        // Dual-flavor `place_figure` → IPC marquee wins (richer path).
        let (flavor, beat) = find_beat(narratives().unwrap(), "place_figure").unwrap();
        assert_eq!(flavor, ServerFlavor::IpcSavestate);
        assert!(beat.requires_ipc);

        // Unknown beat → None.
        assert!(find_beat(narratives().unwrap(), "no_such_beat").is_none());
    }
}
