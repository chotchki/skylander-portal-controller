//! PLAN 15 — desktop-mode play-through recorder, **beats & narrative** edition.
//!
//! Boots the launcher in Phase-20 **Desktop mode** (windowed), records the
//! **whole primary monitor to an MP4** via `windows-capture` (no external
//! ffmpeg), and — while recording — drives the phone SPA in a VISIBLE Chrome
//! window beside the launcher through a chosen **narrative** (an ordered set of
//! **beats**; see `beats.rs` + `docs/dev/recorder-beats-framework.md`). Outputs
//! one MP4 spanning all beats, a `<out>.timeline.json` editorial manifest
//! (beat-boundary timestamps for the later `-- render` post-pass), and a still
//! PNG.
//!
//! Modes (design §6):
//!   - `-- narrative <name>` — one capture spanning all the narrative's beats,
//!     run sequentially. Default name is the IPC marquee (`ingame`).
//!   - `-- beat <name>`      — boot the flavor of that beat's owning narrative,
//!     run just that one beat → a per-screen clip (+ still PNG).
//!   - `-- render <raw.mp4> [timeline.json] [final.mp4]` — pure post-pass (no
//!     server / registry / Chrome): apply the editorial manifest to the raw
//!     capture → one H.265 final cut (`render.rs`, design §5; PLAN 15.13.4).
//!     The optionals derive from the raw path.
//!   - `-- render-review <raw.mp4> [timeline.json] [review.mp4]` — A.9.2 tuning
//!     cut: re-emit the raw at 1× with each beat's name+plan banner-ed on, so
//!     the speed-ups can be called by scrubbing the un-sped footage.
//!   - bare `portal` / `place` / `ingame` — BACK-COMPAT aliases to those
//!     narratives.
//!
//! A narrative is locked to one [`ServerFlavor`] (design §7): **Mock** (mock
//! driver, Desktop window mode, Giants pre-launched, no RPCS3) or
//! **IpcSavestate** (real Spyro save state on the patched RPCS3 over IPC —
//! HTPC-only; requires RPCS3_EXE + RPCS3_CONFIG_DIR + SKYLANDER_BOOT_SAVESTATE
//! in the env).
//!
//! Run (build the phone with the harness's pinned token first; point
//! CHROMEDRIVER at a build matching your installed Chrome):
//!   cd phone && BUILD_TOKEN=e2e-test trunk build
//!   CHROMEDRIVER=<matching chromedriver.exe> cargo run -p skylander-playthrough -- narrative place
//!   # in-game marquee (HTPC, patched RPCS3 + a Spyro save state at the portal):
//!   RPCS3_EXE=…\rpcs3.exe  RPCS3_CONFIG_DIR=…\rpcs3\  \
//!     SKYLANDER_BOOT_SAVESTATE=…\BLUS30779_1_0.SAVESTAT.zst  \
//!     CHROMEDRIVER=…  cargo run -p skylander-playthrough -- narrative ingame
//!   # a single per-screen clip:
//!   CHROMEDRIVER=…  cargo run -p skylander-playthrough -- beat open_toybox

mod beats;
mod caption;
mod capture;
mod render;
mod screen;
mod stage;
mod timeline;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use beats::{Beat, BeatCtx, MARQUEE, Narrative, ServerFlavor};
use capture::{DesktopCapture, SceneCapture};
use skylander_e2e_tests::{Phone, TestServer, inject_profile, launch_giants};
use timeline::{TimelineEntry, TimelineFile};

/// Parsed CLI intent.
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Narrative(String),
    Beat(String),
    /// `-- render` post-pass (PLAN 15.13.4): pure post-processing of an
    /// existing raw capture + manifest — never boots the server, the
    /// narrative registry, or chromedriver.
    Render {
        raw: PathBuf,
        manifest: PathBuf,
        out: PathBuf,
    },
    /// `-- render-review <raw.mp4> [timeline.json] [review.mp4]` (A.9.2): a pure
    /// post-pass like [`Mode::Render`], but re-emits the raw at 1× (NO speed
    /// ramps) with each beat's name + current plan banner-ed on for its window —
    /// the tuning cut chotchki scrubs to call the speed-ups. Optionals derive
    /// from the raw path.
    RenderReview {
        raw: PathBuf,
        manifest: PathBuf,
        out: PathBuf,
    },
    /// `-- capture-smoke <secs> <out.mp4>` — dev smoke test of the capture
    /// backend in isolation (no server / browser): record the screen for `secs`
    /// and assert a non-empty file. PLAN A.1 (verifies the macOS backend).
    CaptureSmoke {
        secs: u64,
        out: PathBuf,
    },
    /// `-- composite <controller.mp4> <game.mp4> <out.mp4>` — PLAN A.5 2-pane
    /// composite (controller-left + game-right) → a high-quality intermediate
    /// the `-- render` pass then speed-ramps + dual-encodes. Pure ffmpeg post-
    /// pass; never boots the server/registry/Chrome.
    Composite {
        controller: PathBuf,
        game: PathBuf,
        out: PathBuf,
    },
    /// `-- nav-portal <EBOOT.BIN>` (PLAN A.2.4) — cold-boot the game and mash
    /// CROSS (per the gates manifest) until the classifier sees the in-game
    /// portal. No server/SPA, just RPCS3 + the IPC. Needs RPCS3_EXE in the env.
    NavPortal {
        eboot: PathBuf,
    },
    /// `-- capture-window <APP> <TITLE-SUBSTR> <SECS> <out.mp4>` (Phase B.1 spike) —
    /// per-window SCKit capture of ONE named window for `secs`, to test whether an
    /// occluded window (behind the fullscreen game) captures real content or black.
    CaptureWindow {
        app: String,
        title: String,
        secs: u64,
        out: PathBuf,
    },
    /// `-- render-concat <out> <wizard-raw> <caption> <body-final>` (A.8.11) —
    /// stitch the standalone `install` wizard clip (captioned + normalised onto
    /// the 1920×1080 canvas) in FRONT of the already-rendered Tour body.
    RenderConcat {
        out: PathBuf,
        wizard_raw: PathBuf,
        caption: String,
        body_final: PathBuf,
    },
}

/// The no-arg / empty-args default narrative: the self-contained Mock `place`
/// flow (design fix 4). Works on any dev box — the IPC marquee (`MARQUEE`)
/// hard-requires the patched RPCS3 + a save state, so it is opt-in via an
/// explicit `-- narrative ingame` / bare `ingame`, never the bare default.
const DEFAULT_NARRATIVE: &str = "place";

/// A.7.2 — minimum on-screen time for a CAPTIONED beat (chotchki's "minimum
/// pause"). The test-hook unlock + an instantly-mounting screen can finish in
/// <1s, which flashes the caption by; a captioned beat that drove faster than
/// this holds the remainder at 1× so the lower-third stays legible. Uncaptioned
/// beats are untouched (they can fly by). Tunable.
const MIN_CAPTION_DWELL_MS: u128 = 2800;

fn parse_mode(args: &[String]) -> Result<Mode> {
    // args[0] is the program name; the recorder is invoked `… -- <rest>`, and
    // cargo strips the `--`, so we see e.g. ["narrative", "place"] or
    // ["beat", "open_toybox"] or a bare ["place"].
    let rest = &args[1..];
    match rest {
        [] => Ok(Mode::Narrative(DEFAULT_NARRATIVE.to_string())),
        [kw, name, ..] if kw == "narrative" => Ok(Mode::Narrative(name.clone())),
        [kw, name, ..] if kw == "beat" => Ok(Mode::Beat(name.clone())),
        [kw] if kw == "narrative" => Ok(Mode::Narrative(MARQUEE.to_string())),
        [kw] if kw == "beat" => {
            anyhow::bail!("`-- beat` needs a beat name (e.g. `-- beat open_toybox`)")
        }
        // `-- render <raw.mp4> [timeline.json] [final.mp4]` (PLAN 15.13.4).
        // Both optionals derive from the raw path: `foo.mp4` →
        // `foo.timeline.json` (what the recorder writes next to the capture)
        // and a sibling `foo-final.mp4`.
        [kw, raw, manifest, out, ..] if kw == "render" => Ok(Mode::Render {
            raw: PathBuf::from(raw),
            manifest: PathBuf::from(manifest),
            out: PathBuf::from(out),
        }),
        [kw, raw, manifest] if kw == "render" => {
            let raw = PathBuf::from(raw);
            Ok(Mode::Render {
                manifest: PathBuf::from(manifest),
                out: render::default_out_path(&raw),
                raw,
            })
        }
        [kw, raw] if kw == "render" => {
            let raw = PathBuf::from(raw);
            Ok(Mode::Render {
                manifest: render::default_manifest_path(&raw),
                out: render::default_out_path(&raw),
                raw,
            })
        }
        [kw] if kw == "render" => anyhow::bail!(
            "`-- render` needs a raw capture \
             (usage: `-- render <raw.mp4> [timeline.json] [final.mp4]`)"
        ),
        // `-- render-review <raw.mp4> [timeline.json] [review.mp4]` (A.9.2): the
        // 1× beat-labelled tuning cut. Optionals derive from the raw path, same
        // as `render` (manifest = sibling `.timeline.json`, out = `-review.mp4`).
        [kw, raw, manifest, out, ..] if kw == "render-review" => Ok(Mode::RenderReview {
            raw: PathBuf::from(raw),
            manifest: PathBuf::from(manifest),
            out: PathBuf::from(out),
        }),
        [kw, raw, manifest] if kw == "render-review" => {
            let raw = PathBuf::from(raw);
            Ok(Mode::RenderReview {
                manifest: PathBuf::from(manifest),
                out: render::default_review_out_path(&raw),
                raw,
            })
        }
        [kw, raw] if kw == "render-review" => {
            let raw = PathBuf::from(raw);
            Ok(Mode::RenderReview {
                manifest: render::default_manifest_path(&raw),
                out: render::default_review_out_path(&raw),
                raw,
            })
        }
        [kw] if kw == "render-review" => anyhow::bail!(
            "`-- render-review` needs a raw capture \
             (usage: `-- render-review <raw.mp4> [timeline.json] [review.mp4]`)"
        ),
        // `-- capture-smoke <secs> <out.mp4>` (PLAN A.1).
        [kw, secs, out] if kw == "capture-smoke" => Ok(Mode::CaptureSmoke {
            secs: secs
                .parse()
                .with_context(|| format!("capture-smoke <secs> must be a number, got {secs:?}"))?,
            out: PathBuf::from(out),
        }),
        [kw, ..] if kw == "capture-smoke" => {
            anyhow::bail!("`-- capture-smoke` needs <secs> <out.mp4>")
        }
        // `-- composite <controller.mp4> <game.mp4> <out.mp4>` (PLAN A.5).
        [kw, controller, game, out] if kw == "composite" => Ok(Mode::Composite {
            controller: PathBuf::from(controller),
            game: PathBuf::from(game),
            out: PathBuf::from(out),
        }),
        [kw, ..] if kw == "composite" => {
            anyhow::bail!("`-- composite` needs <controller.mp4> <game.mp4> <out.mp4>")
        }
        // `-- nav-portal <EBOOT.BIN>` (PLAN A.2.4): cold boot + mash CROSS to the portal.
        [kw, eboot] if kw == "nav-portal" => Ok(Mode::NavPortal {
            eboot: PathBuf::from(eboot),
        }),
        [kw, ..] if kw == "nav-portal" => {
            anyhow::bail!("`-- nav-portal` needs <path/to/EBOOT.BIN>")
        }
        // `-- capture-window <APP> <TITLE-SUBSTR> <SECS> <out.mp4>` (Phase B.1 occlusion spike).
        [kw, app, title, secs, out] if kw == "capture-window" => Ok(Mode::CaptureWindow {
            app: app.clone(),
            title: title.clone(),
            secs: secs
                .parse()
                .with_context(|| format!("capture-window <secs> must be a number, got {secs:?}"))?,
            out: PathBuf::from(out),
        }),
        [kw, ..] if kw == "capture-window" => {
            anyhow::bail!("`-- capture-window` needs <app> <title-substr> <secs> <out.mp4>")
        }
        [kw, out, wizard, caption, body] if kw == "render-concat" => Ok(Mode::RenderConcat {
            out: PathBuf::from(out),
            wizard_raw: PathBuf::from(wizard),
            caption: caption.clone(),
            body_final: PathBuf::from(body),
        }),
        [kw, ..] if kw == "render-concat" => {
            anyhow::bail!(
                "`-- render-concat` needs <out.mp4> <wizard-raw.mp4> <caption> <body-final.mp4>"
            )
        }
        // Bare back-compat aliases: `portal` / `place` / `ingame`.
        [alias, ..] => match beats::resolve_alias(alias) {
            Some(narr) => Ok(Mode::Narrative(narr.to_string())),
            None => anyhow::bail!(
                "unknown arg {alias:?} — use `-- narrative <name>`, `-- beat <name>`, \
                 or a bare alias (portal/place/ingame)"
            ),
        },
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    // PER_MONITOR_AWARE_V2 must be set before ANY window/monitor API runs
    // (capture's Monitor::primary, the tiling's SPI_GETWORKAREA /
    // SetWindowPos) so they all speak physical pixels — PLAN 15.14.
    stage::set_dpi_aware();

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args: Vec<String> = std::env::args().collect();
    let mode = parse_mode(&args)?;

    // The recording modes build + validate the registry inside their lookup
    // (flavor-lock enforced in `narratives()` — design §7: a clear failure
    // before any heavy work; the lookup consumes the owned, freshly-validated
    // `Vec<Narrative>`). Render deliberately never touches it — it must work
    // on a box with only ffmpeg and the recorded artifacts.
    match mode {
        Mode::Render { raw, manifest, out } => render::run(&raw, &manifest, &out),
        Mode::RenderConcat {
            out,
            wizard_raw,
            caption,
            body_final,
        } => render::concat_tour(&out, &wizard_raw, &caption, &body_final),
        Mode::RenderReview { raw, manifest, out } => render::review(&raw, &manifest, &out),
        Mode::CaptureSmoke { secs, out } => capture_smoke(secs, &out),
        Mode::Composite {
            controller,
            game,
            out,
        } => render::composite(&controller, &game, &out),
        Mode::NavPortal { eboot } => nav_portal(&eboot),
        Mode::CaptureWindow {
            app,
            title,
            secs,
            out,
        } => capture_window(&app, &title, secs, &out),
        Mode::Narrative(name) => {
            let narr = beats::find_narrative(
                beats::narratives().context("build + validate narrative registry")?,
                &name,
            )
            .with_context(|| format!("no narrative named {name:?}"))?;
            tracing::info!(narrative = %narr.name, flavor = ?narr.flavor, "running narrative");
            run_narrative(narr).await
        }
        Mode::Beat(name) => {
            let (flavor, beat) = beats::find_beat(
                beats::narratives().context("build + validate narrative registry")?,
                &name,
            )
            .with_context(|| format!("no beat named {name:?} in any narrative"))?;
            tracing::info!(beat = %beat.name, flavor = ?flavor, "running single beat");
            run_single_beat(flavor, beat).await
        }
    }
}

/// PLAN A.1 — dev smoke test: exercise the capture backend (macOS:
/// ScreenCaptureKit; Windows: windows-capture) in isolation, no server/browser.
/// Records the screen for `secs`, then asserts a non-empty file. Run via
/// `tools/playthrough/run.sh` so the binary carries a stable Screen Recording grant.
fn capture_smoke(secs: u64, out: &std::path::Path) -> Result<()> {
    tracing::info!(secs, out = %out.display(), "capture-smoke: recording the screen…");
    let cap = DesktopCapture::start(out).context("start capture")?;
    std::thread::sleep(Duration::from_secs(secs));
    cap.stop().context("stop capture")?;
    let bytes = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    tracing::info!(out = %out.display(), bytes, "capture-smoke: done");
    anyhow::ensure!(
        bytes > 0,
        "capture produced an empty file: {}",
        out.display()
    );
    Ok(())
}

/// PLAN A.2.4 — cold-boot the game and mash CROSS (per `assets/screens/gates.json`)
/// until the classifier sees the in-game portal-placement prompt. No server / SPA:
/// just RPCS3 + the AF_UNIX IPC. Needs `RPCS3_EXE` in the env (RPCS3 reads its own
/// `RPCS3_CONFIG_DIR`). Leaves RPCS3 running on success so the portal stays up.
fn nav_portal(eboot: &std::path::Path) -> Result<()> {
    use std::process::Command;

    let rpcs3 = std::env::var("RPCS3_EXE").context("RPCS3_EXE must be set (see .env.dev)")?;
    let sock = skylander_rpcs3_control::ipc::default_socket_path();
    let _ = std::fs::remove_file(&sock);

    tracing::info!(eboot = %eboot.display(), "nav-portal: cold-booting the game");
    let mut child = Command::new(&rpcs3)
        .arg(eboot)
        .spawn()
        .with_context(|| format!("spawn RPCS3 {rpcs3:?}"))?;

    // Wait for the portal device's IPC socket (= the game is past initial boot).
    let start = Instant::now();
    while !sock.exists() {
        if start.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            anyhow::bail!("IPC socket never appeared — RPCS3 didn't reach the portal device");
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    tracing::info!("nav-portal: IPC up; mashing CROSS to the portal");
    std::thread::sleep(Duration::from_secs(2)); // let cellPadInit land so presses register

    let driver = skylander_rpcs3_control::ipc::IpcPortalDriver::with_path(&sock);
    let gates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/screens/gates.json");
    let lib = screen::ScreenLibrary::load(&gates).context("load gates.json")?;

    let result = screen::nav_to_portal(&driver, &lib, Duration::from_secs(300));
    match &result {
        Ok(()) => tracing::info!("nav-portal: reached the portal, leaving RPCS3 running"),
        Err(e) => {
            tracing::error!(error = %e, "nav-portal: failed");
            let _ = child.kill();
        }
    }
    result
}

/// Phase B.1 occlusion spike — capture ONE named window for `secs` via SCKit
/// per-window capture (`DesktopCapture::start_window`, the recorder's real path).
/// Run it with the target window shoved behind the fullscreen game, then extract a
/// frame: real content = SCKit captures occluded windows (recorder needs no window
/// coordination); black = it doesn't (recorder also needs Phase B).
fn capture_window(app: &str, title: &str, secs: u64, out: &std::path::Path) -> Result<()> {
    tracing::info!(app, title, secs, out = %out.display(), "capture-window: recording one window");
    let cap = DesktopCapture::start_window(app, title, out).context("start per-window capture")?;
    std::thread::sleep(Duration::from_secs(secs));
    cap.stop().context("stop per-window capture")?;
    let bytes = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    tracing::info!(out = %out.display(), bytes, "capture-window: done");
    anyhow::ensure!(
        bytes > 0,
        "window capture produced an empty file: {}",
        out.display()
    );
    Ok(())
}

/// Spawn the server for `flavor`, seed Alice (+ Bob for Mock), tile the
/// launcher into the work area (PLAN 15.14), start the desktop capture, and
/// open the headed app-mode phone in the right-hand column. Returns the live
/// pieces the beats drive. Mirrors the shared boot the old monolithic
/// scenarios did (design §6).
struct Boot {
    server: TestServer,
    phone: Phone,
    phone_url: String,
    alice: String,
    /// Second profile id (Bob) for the `ownership` beat's headless placement.
    bob: String,
    cap: SceneCapture,
    mp4: std::path::PathBuf,
    /// Wall-clock instant captured at `DesktopCapture::start` — i.e. capture
    /// frame 0 (design §5: `timeline.json` timestamps are relative to
    /// DesktopCapture start, NOT to first-beat-start). Both `run_narrative` and
    /// `run_single_beat` derive every beat's `t_*_ms` from this origin, so the
    /// ~1s lead-in (capture start + just-the-launcher hold + Chrome open) is
    /// included in the offsets and t=0 lines up with the first captured frame
    /// (design fix 3).
    timeline_origin: Instant,
    /// The tiled launcher+phone region in physical capture pixels (PLAN
    /// 15.14), emitted as the manifest's `stage` so the render pass can crop
    /// out taskbar/desktop clutter. `Some` only when BOTH window placements
    /// verifiably succeeded — `stage::place_window` reads the visible frame
    /// back after `SetWindowPos` and errors on a mismatch (min-size clamps
    /// report success otherwise) — because a half-tiled frame cropped to the
    /// stage would cut content off, so any failure degrades to `None`
    /// (full-frame; the render still works).
    stage: Option<timeline::CropRect>,
}

/// A.9.1 — durable output dir for the raw capture + `timeline.json` (+ still
/// PNG), so the un-sped raw survives for the tune→`render` loop. `$TMPDIR` is
/// ephemeral AND reaped around `capture-ingame-embed.sh`, so default to a stable
/// gitignored `tools/playthrough/out/`; override with `SKYLANDER_PLAYTHROUGH_OUT`.
fn out_dir() -> Result<PathBuf> {
    let dir = std::env::var_os("SKYLANDER_PLAYTHROUGH_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out"));
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create playthrough out dir {}", dir.display()))?;
    Ok(dir)
}

async fn boot(flavor: ServerFlavor, out_stem: &str) -> Result<Boot> {
    // 1. Server by flavor (design §6/§7).
    let server = match flavor {
        ServerFlavor::Mock => TestServer::spawn_with_env_lines("WINDOW_MODE=desktop\n")
            .context("spawn server in desktop window mode (mock)")?,
        ServerFlavor::IpcSavestate => TestServer::spawn_ipc_savestate().context(
            "spawn IPC server (savestate tier) — set RPCS3_EXE, RPCS3_CONFIG_DIR, SKYLANDER_BOOT_SAVESTATE",
        )?,
        ServerFlavor::IpcCold => TestServer::spawn_ipc_cold().context(
            "spawn IPC server (cold-boot in-game tier) — set RPCS3_EXE, RPCS3_CONFIG_DIR (firmware + games.yml)",
        )?,
    };
    tracing::info!(url = %server.url, ?flavor, "server up");

    // 2. Seed the family. Alice is the driven profile; Bob exists in the Mock
    //    flow for a fuller picker (matches the old scenarios). The IPC flow
    //    injected only Alice. The Mock flow pre-launches Giants at boot
    //    (test-hook; no RPCS3) so the portal is reachable — verbatim from the
    //    old mock `main()` body. The IPC flow launches a real game from the
    //    picker in the `pick_game` beat instead.
    let alice = inject_profile(&server.url, "Alice", "1111", "#f5c634").await?;
    // Bob is the SECOND profile, now injected in EVERY flavor (was Mock-only):
    // the Tour's `ownership` beat (A.8.9) binds a headless phone to Bob and
    // places as him so the captured phone shows the per-slot ownership pip.
    // His magenta matches the Kaos skin palette so the two pips read distinct.
    let bob = inject_profile(&server.url, "Bob", "2222", "#da28a8").await?;
    if flavor == ServerFlavor::Mock {
        launch_giants(&server.url).await?;
    }

    // 3. Tiling (PLAN 15.14): compute the edge-to-edge layout from the
    //    primary work area, then place the launcher BEFORE capture starts so
    //    frame 0 is already tiled. RPCS3 later fits itself to the launcher
    //    via the server's PLAN-20.4 window-fit — no recorder action. Every
    //    step degrades on failure (warn, keep recording untiled): the demo
    //    must never abort over framing.
    let layout = match stage::work_area() {
        Ok(work) => Some(stage::compute_layout(work)),
        Err(e) => {
            tracing::warn!(error = %e, "no work area — recording untiled (stage = None)");
            None
        }
    };
    let mut launcher_placed = false;
    if let Some(l) = &layout {
        // Exact title — dev boxes have editor/terminal windows whose titles
        // CONTAIN "skylander-portal-controller" (stage.rs).
        match stage::wait_find_window_exact("Skylander Portal Controller", Duration::from_secs(10))
            .await
        {
            Some(hwnd) => match stage::place_window(hwnd, l.launcher) {
                Ok(()) => {
                    tracing::info!("placed launcher at {:?}", l.launcher);
                    launcher_placed = true;
                }
                Err(e) => tracing::warn!(error = %e, "launcher placement failed — untiled"),
            },
            None => tracing::warn!("launcher window not found by exact title — untiled"),
        }
    }

    // 4. The raw MP4 path. The capture starts LATER — after BOTH windows
    //    (launcher + phone) are up + titled — because the macOS 2-pane path
    //    (SceneCapture) records them as per-window streams, which need the windows
    //    present and matchable by title. Windows still captures the whole desktop,
    //    just from the same (slightly later) anchor.
    let mp4 = out_dir()?.join(format!("{out_stem}.mp4"));

    // 5. Visible app-mode (chromeless) phone window in the right-hand column.
    //    Chrome reads --window-position/--window-size in DIPs, so the
    //    physical-pixel tile is divided by the primary monitor's effective
    //    scale first — passed raw on a scaled display, the window opens
    //    scale× too large and flashes over the launcher in the already-
    //    rolling capture's pre-roll until the placement below corrects it.
    //    The hint stays approximate either way (the Win32 placement is the
    //    physical-pixel authority); with no layout, fall back to the legacy
    //    fixed geometry (pre-15.14) with no placement.
    let phone_url = server.phone_url().await?;
    let (px, py, pw, ph) = match &layout {
        Some(l) => {
            let hint = stage::to_dips(l.phone, stage::primary_scale());
            (hint.x, hint.y, hint.w as u32, hint.h as u32)
        }
        None => (1180, 40, 470, 940),
    };
    let phone = Phone::new_headed_app(&phone_url, &server.chromedriver_url, px, py, pw, ph)
        .await
        .context("open headed app-mode phone browser")?;
    tracing::info!("phone browser open (headed, app-mode) — driving the flow");

    // 6. Retitle the page ALWAYS (the app-mode window title IS the document
    //    title) — both the Win32 exact-find placement AND the macOS per-window
    //    capture (SceneCapture) match the phone pane on this title, and it reads
    //    nicely in the captured title bar.
    if let Err(e) = phone
        .client
        .execute("document.title='Skylander Portal Phone'", vec![])
        .await
    {
        tracing::warn!(error = %e, "phone retitle failed — capture/placement may not find it");
    }

    // 6b. Inject a tap-ripple visualiser into the phone page (PLAN A.5 — chotchki).
    //     The capture hides the OS cursor, so make interaction obvious instead: a
    //     gold ripple blooms at each tap. The beats tap via `el.click()` (JS) which
    //     dispatches ONLY a `click` event (no pointerdown) with clientX/Y=0, so we
    //     listen for click/pointerdown/mousedown/touchstart and fall back to the
    //     target element's centre when there's no real pointer position; a short
    //     dedupe window collapses the multiple events of one real tap into one
    //     ripple. The SPA is a CSR (no full reloads), so one injection lasts the
    //     whole session.
    //
    //     EARLY-CLEAR (A.8.6 — chotchki): a tap that opens a detail / menu or
    //     swaps screens left the ripple lingering 650ms over UI that no longer
    //     exists at that point ("click targets against non-existent UI"). Fix:
    //     snapshot `elementFromPoint(x,y)` at ripple time (our capture-phase
    //     listener runs BEFORE Leptos's bubble-phase on:click, so it's the
    //     original element), then poll — the instant that point resolves to
    //     unrelated UI (element unmounted OR a modal covered it), kill the
    //     ripple. A tap that DOESN'T navigate (filter chip, toggle) keeps the
    //     point related and rides out the full 0.6s.
    const TAP_RIPPLE_JS: &str = "(function(){\
        if(window.__skyTap)return;window.__skyTap=1;\
        var s=document.createElement('style');\
        s.textContent='@keyframes skytap{from{transform:translate(-50%,-50%) scale(.3);opacity:.9}to{transform:translate(-50%,-50%) scale(1.7);opacity:0}}';\
        document.head.appendChild(s);\
        var lt=0,lx=-1e4,ly=-1e4;\
        function related(a,b){return a===b||(a&&b&&(a.contains(b)||b.contains(a)));}\
        function ripple(x,y){\
            if(x==null||y==null)return;var t=Date.now();\
            if(t-lt<400&&Math.abs(x-lx)<60&&Math.abs(y-ly)<60)return;\
            lt=t;lx=x;ly=y;\
            var origin=document.elementFromPoint(x,y);\
            var d=document.createElement('div');\
            d.style.cssText='position:fixed;left:'+x+'px;top:'+y+'px;width:64px;height:64px;border-radius:50%;border:4px solid #f5c634;background:rgba(245,198,52,.25);pointer-events:none;z-index:2147483647;animation:skytap .6s ease-out forwards';\
            document.body.appendChild(d);\
            var done=false;function kill(){if(done)return;done=true;clearInterval(iv);d.remove();}\
            var n=0;var iv=setInterval(function(){\
                if(++n>16){clearInterval(iv);return;}\
                if(!related(origin,document.elementFromPoint(x,y)))kill();\
            },40);\
            setTimeout(kill,650);\
        }\
        function pos(e){\
            if(e.changedTouches&&e.changedTouches[0])return[e.changedTouches[0].clientX,e.changedTouches[0].clientY];\
            var x=e.clientX,y=e.clientY;\
            if((!x&&!y)&&e.target&&e.target.getBoundingClientRect){var r=e.target.getBoundingClientRect();return[r.left+r.width/2,r.top+r.height/2];}\
            return[x,y];\
        }\
        ['pointerdown','mousedown','click','touchstart'].forEach(function(ev){\
            addEventListener(ev,function(e){var p=pos(e);ripple(p[0],p[1]);},true);\
        });\
    })();";
    if let Err(e) = phone.client.execute(TAP_RIPPLE_JS, vec![]).await {
        tracing::warn!(error = %e, "tap-ripple injection failed — taps won't be visualised");
    }

    // Win32 placement (tiling) — Windows-only; on macOS `layout` is None, so the
    // windows stay where they opened and the per-window capture handles framing.
    let mut phone_placed = false;
    if let Some(l) = &layout {
        match stage::wait_find_window_exact("Skylander Portal Phone", Duration::from_secs(5)).await
        {
            Some(hwnd) => match stage::place_window(hwnd, l.phone) {
                Ok(()) => {
                    tracing::info!("placed phone at {:?}", l.phone);
                    phone_placed = true;
                }
                Err(e) => tracing::warn!(error = %e, "phone placement failed — untiled"),
            },
            None => tracing::warn!("phone window not found by exact title — untiled"),
        }
    }

    // Emit the stage crop ONLY when both windows verifiably sit at their tiled
    // rects (Boot::stage docs) — else the manifest degrades to full-frame. On
    // macOS this is always None: the 2-pane composite IS the framing.
    let stage = match (&layout, launcher_placed && phone_placed) {
        (Some(l), true) => Some(l.stage),
        _ => None,
    };
    tracing::info!("stage = {stage:?}");

    // 7. Both windows are now up + titled → start the capture. Anchor the
    //    editorial timeline at frame 0 here. A short settle lets the freshly-set
    //    phone title propagate to the window server before the per-window find,
    //    and gives a brief both-panes lead-in.
    tokio::time::sleep(Duration::from_millis(600)).await;
    let timeline_origin = Instant::now();
    let cap = SceneCapture::start(out_stem, &mp4).context("start scene capture")?;
    tracing::info!(mp4 = %mp4.display(), "recording…");
    tokio::time::sleep(Duration::from_secs(1)).await; // brief both-panes lead-in

    Ok(Boot {
        server,
        phone,
        phone_url,
        alice,
        bob,
        cap,
        mp4,
        timeline_origin,
        stage,
    })
}

/// Run all of a narrative's beats inside ONE `DesktopCapture` (design §6: do
/// NOT per-beat-capture — each `stop()` finalizes a separate MP4). Emits the
/// raw MP4 + `<out>.timeline.json` + a still PNG.
async fn run_narrative(narr: Narrative) -> Result<()> {
    let out_stem = format!("playthrough-{}", narr.name);
    let boot = boot(narr.flavor, &out_stem).await?;
    let Boot {
        server,
        phone,
        phone_url,
        alice,
        bob,
        cap,
        mp4,
        timeline_origin,
        stage,
    } = boot;

    let ctx = BeatCtx {
        phone: &phone,
        server: &server,
        phone_url: &phone_url,
        alice: &alice,
        bob: &bob,
    };

    // Stamp wall-clock at each beat boundary, relative to `timeline_origin`
    // (captured at `DesktopCapture::start` in `boot()`) — so `t=0` lines up
    // with capture frame 0 and the ~1s lead-in is reflected in the first beat's
    // `t_start_ms` (design §5 / fix 3). The render pass (15.13.4) gap-fills
    // anything the brackets don't cover — the pre-roll, inter-beat slack, and
    // the trailing 3s post-beat hold below — at 1×, so no manifest entry is
    // needed for them.
    let mut timeline: Vec<TimelineEntry> = Vec::with_capacity(narr.beats.len());

    for beat in &narr.beats {
        let t_start = timeline_origin.elapsed().as_millis();
        tracing::info!(beat = %beat.name, "beat start");
        (beat.drive)(&ctx)
            .await
            .with_context(|| format!("beat {:?}", beat.name))?;
        dwell_for_caption(beat, t_start, timeline_origin).await;
        let t_end = timeline_origin.elapsed().as_millis();
        tracing::info!(beat = %beat.name, t_start_ms = t_start, t_end_ms = t_end, "beat done");
        timeline.push(entry_for(beat, t_start, t_end));
    }

    // Hold so the final state is on screen, then stop + flush the MP4. The
    // `farewell` closer QUITS the launcher (~3.8s after its shutdown POST:
    // ui/farewell.rs 3s countdown + 0.8s fade), so skip the trailing hold there
    // — the beat's own ~2.3s hold already captured the Farewell badge, and
    // sleeping longer would finalize over a closed launcher window (dead left
    // pane / ended SCKit stream). The final phone screenshot is skipped too: the
    // server is mid-shutdown so the page is tearing down.
    let last_closes_launcher = narr.beats.last().map(|b| b.name) == Some("farewell");
    if !last_closes_launcher {
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    cap.finalize(&mp4)
        .context("finalize capture → raw mp4 (macOS: composite the 2 panes)")?;
    tracing::info!(mp4 = %mp4.display(), "MP4 written");

    write_timeline(&mp4, stage, timeline)?;

    if !last_closes_launcher {
        let png = out_dir()?.join(format!("{out_stem}.png"));
        phone.screenshot(&png).await?;
        tracing::info!(screenshot = %png.display(), "still captured");
    }

    phone.close().await.ok();
    tracing::info!("done");
    Ok(())
}

/// Run a single beat (design §6: `-- beat <name>` boots the flavor of that
/// beat's owning narrative, runs just that beat → a per-screen clip + PNG).
/// Still emits a one-entry timeline so the render pass works uniformly.
async fn run_single_beat(flavor: ServerFlavor, beat: Beat) -> Result<()> {
    let out_stem = format!("playthrough-beat-{}", beat.name);
    let boot = boot(flavor, &out_stem).await?;
    let Boot {
        server,
        phone,
        phone_url,
        alice,
        bob,
        cap,
        mp4,
        timeline_origin,
        stage,
    } = boot;

    let ctx = BeatCtx {
        phone: &phone,
        server: &server,
        phone_url: &phone_url,
        alice: &alice,
        bob: &bob,
    };

    // `timeline_origin` is capture frame 0 (set in `boot()`), so this single
    // beat's `t_start_ms` includes the ~1s capture lead-in (design fix 3).
    let t_start = timeline_origin.elapsed().as_millis();
    tracing::info!(beat = %beat.name, "beat start (single)");
    (beat.drive)(&ctx)
        .await
        .with_context(|| format!("beat {:?}", beat.name))?;
    dwell_for_caption(&beat, t_start, timeline_origin).await;
    let t_end = timeline_origin.elapsed().as_millis();
    let timeline = vec![entry_for(&beat, t_start, t_end)];

    tokio::time::sleep(Duration::from_secs(3)).await;
    cap.finalize(&mp4)
        .context("finalize capture → raw mp4 (macOS: composite the 2 panes)")?;
    tracing::info!(mp4 = %mp4.display(), "MP4 written");

    write_timeline(&mp4, stage, timeline)?;

    let png = out_dir()?.join(format!("{out_stem}.png"));
    phone.screenshot(&png).await?;
    tracing::info!(screenshot = %png.display(), "still captured");

    phone.close().await.ok();
    tracing::info!("done (single beat)");
    Ok(())
}

// `fantoccini` is still a direct dependency (the harness re-exports use it), but
// the recorder's own selectors now live in the beats; no top-level `Locator`
// import is needed here.

/// Hold a captioned beat to [`MIN_CAPTION_DWELL_MS`] of 1× footage if its drive
/// finished sooner, so the caption stays legible (A.7.2 — chotchki's "minimum
/// pause"). No-op for uncaptioned beats or ones that already ran long enough.
async fn dwell_for_caption(beat: &Beat, t_start_ms: u128, origin: Instant) {
    if beat.caption.is_none() {
        return;
    }
    let dwelled = origin.elapsed().as_millis().saturating_sub(t_start_ms);
    if dwelled < MIN_CAPTION_DWELL_MS {
        tokio::time::sleep(Duration::from_millis(
            (MIN_CAPTION_DWELL_MS - dwelled) as u64,
        ))
        .await;
    }
}

/// Build a manifest row from a beat + its measured boundaries.
fn entry_for(beat: &Beat, t_start_ms: u128, t_end_ms: u128) -> TimelineEntry {
    TimelineEntry {
        beat: beat.name.to_string(),
        t_start_ms: t_start_ms as u64,
        t_end_ms: t_end_ms as u64,
        realtime_head_ms: beat.realtime_head.as_millis() as u64,
        realtime_tail_ms: beat.realtime_tail.as_millis() as u64,
        filler_speed: beat.filler_speed,
        crop: beat.crop,
        // PLAN A.5 — flows the beat's caption into the manifest → render overlay.
        // All beats are `None` today; the narration copy is chotchki's to set.
        caption: beat.caption.map(str::to_string),
    }
}

/// Write `<mp4-stem>.timeline.json` next to the MP4 (design §5; v2 object
/// schema with the 15.14 `stage`). `foo.mp4` → `foo.timeline.json`.
fn write_timeline(
    mp4: &std::path::Path,
    stage: Option<timeline::CropRect>,
    beats: Vec<TimelineEntry>,
) -> Result<()> {
    let json_path = mp4.with_extension("timeline.json");
    let n = beats.len();
    TimelineFile { stage, beats }.save(&json_path)?;
    tracing::info!(timeline = %json_path.display(), beats = n, "timeline.json written");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `parse_mode` sees `args[0]` as the program name and `args[1..]` as the
    /// CLI rest, so prepend a dummy program name like the real `args()` does.
    fn parse(rest: &[&str]) -> Result<Mode> {
        let mut argv = vec!["skylander-playthrough".to_string()];
        argv.extend(rest.iter().map(|s| s.to_string()));
        parse_mode(&argv)
    }

    /// Fix 4: a bare / empty invocation defaults to the self-contained Mock
    /// `place` narrative (NOT the IPC marquee, which needs RPCS3).
    #[test]
    fn no_args_defaults_to_place() {
        assert_eq!(
            parse(&[]).unwrap(),
            Mode::Narrative(DEFAULT_NARRATIVE.to_string())
        );
        assert_eq!(parse(&[]).unwrap(), Mode::Narrative("place".to_string()));
    }

    #[test]
    fn narrative_keyword_takes_explicit_name() {
        assert_eq!(
            parse(&["narrative", "portal"]).unwrap(),
            Mode::Narrative("portal".to_string())
        );
        assert_eq!(
            parse(&["narrative", "ingame"]).unwrap(),
            Mode::Narrative("ingame".to_string())
        );
    }

    /// `-- narrative` with no name keeps the marquee default (design §6); only
    /// the *bare* no-arg case flips to `place` (fix 4).
    #[test]
    fn bare_narrative_keyword_defaults_to_marquee() {
        assert_eq!(
            parse(&["narrative"]).unwrap(),
            Mode::Narrative(MARQUEE.to_string())
        );
    }

    #[test]
    fn beat_keyword_takes_name() {
        assert_eq!(
            parse(&["beat", "open_toybox"]).unwrap(),
            Mode::Beat("open_toybox".to_string())
        );
    }

    #[test]
    fn beat_keyword_without_name_errors() {
        let err = parse(&["beat"]).expect_err("`-- beat` with no name must error");
        assert!(err.to_string().contains("beat"), "got: {err}");
    }

    #[test]
    fn bare_aliases_map_to_narratives() {
        assert_eq!(
            parse(&["portal"]).unwrap(),
            Mode::Narrative("portal".to_string())
        );
        assert_eq!(
            parse(&["place"]).unwrap(),
            Mode::Narrative("place".to_string())
        );
        assert_eq!(
            parse(&["ingame"]).unwrap(),
            Mode::Narrative("ingame".to_string())
        );
    }

    #[test]
    fn render_raw_only_derives_manifest_and_out() {
        // `foo.mp4` → `foo.timeline.json` (the recorder's `write_timeline`
        // naming) + a sibling `foo-final.mp4`.
        assert_eq!(
            parse(&["render", "captures/run.mp4"]).unwrap(),
            Mode::Render {
                raw: PathBuf::from("captures/run.mp4"),
                manifest: PathBuf::from("captures/run.timeline.json"),
                out: PathBuf::from("captures/run-final.mp4"),
            }
        );
    }

    #[test]
    fn render_with_manifest_keeps_out_default() {
        assert_eq!(
            parse(&["render", "run.mp4", "custom.timeline.json"]).unwrap(),
            Mode::Render {
                raw: PathBuf::from("run.mp4"),
                manifest: PathBuf::from("custom.timeline.json"),
                out: PathBuf::from("run-final.mp4"),
            }
        );
    }

    #[test]
    fn render_fully_explicit_paths_pass_through() {
        assert_eq!(
            parse(&["render", "a.mp4", "b.json", "c.mp4"]).unwrap(),
            Mode::Render {
                raw: PathBuf::from("a.mp4"),
                manifest: PathBuf::from("b.json"),
                out: PathBuf::from("c.mp4"),
            }
        );
    }

    #[test]
    fn bare_render_errors_with_usage() {
        let err = parse(&["render"]).expect_err("`-- render` with no raw must error");
        assert!(err.to_string().contains("render"), "got: {err}");
    }

    #[test]
    fn render_review_raw_only_derives_manifest_and_out() {
        // `foo.mp4` → `foo.timeline.json` + a sibling `foo-review.mp4` (A.9.2).
        assert_eq!(
            parse(&["render-review", "captures/run.mp4"]).unwrap(),
            Mode::RenderReview {
                raw: PathBuf::from("captures/run.mp4"),
                manifest: PathBuf::from("captures/run.timeline.json"),
                out: PathBuf::from("captures/run-review.mp4"),
            }
        );
    }

    #[test]
    fn render_review_fully_explicit_paths_pass_through() {
        assert_eq!(
            parse(&["render-review", "a.mp4", "b.json", "c.mp4"]).unwrap(),
            Mode::RenderReview {
                raw: PathBuf::from("a.mp4"),
                manifest: PathBuf::from("b.json"),
                out: PathBuf::from("c.mp4"),
            }
        );
    }

    #[test]
    fn bare_render_review_errors_with_usage() {
        let err = parse(&["render-review"]).expect_err("`-- render-review` with no raw must error");
        assert!(err.to_string().contains("render-review"), "got: {err}");
    }

    #[test]
    fn unknown_bare_arg_errors() {
        let err = parse(&["bogus"]).expect_err("an unknown bare arg must error");
        assert!(err.to_string().contains("unknown arg"), "got: {err}");
    }

    #[test]
    fn render_concat_parses_four_positional_args() {
        assert_eq!(
            parse(&[
                "render-concat",
                "tour.mp4",
                "wizard.mp4",
                "Setup once.",
                "body-final.mp4"
            ])
            .unwrap(),
            Mode::RenderConcat {
                out: PathBuf::from("tour.mp4"),
                wizard_raw: PathBuf::from("wizard.mp4"),
                caption: "Setup once.".to_string(),
                body_final: PathBuf::from("body-final.mp4"),
            }
        );
    }

    #[test]
    fn render_concat_with_missing_args_errors_with_usage() {
        let err = parse(&["render-concat", "tour.mp4", "wizard.mp4"])
            .expect_err("`-- render-concat` with too few args must error");
        assert!(err.to_string().contains("render-concat"), "got: {err}");
    }
}
