//! `DriverKind::Native` with no hooks (openspec AA.3): the router and worker as
//! guacamole runs them, over file-less catalogue records.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as SyncMutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use skylander_core::{
    Category, Element, Event, Figure, FigureId, GameOfOrigin, GameSerial, InstalledGame,
    MaskedVariant, SLOT_COUNT, SlotIndex, SlotState, ToyTypeId,
};
use skylander_rpcs3_control::{Placement, PortalDriver};
use tokio::sync::{Mutex, broadcast};
use tower::ServiceExt;

use crate::config::{DriverKind, WindowMode};
use crate::hooks::Hooks;
use crate::profiles::{ProfileStore, RegistrationOutcome, SessionId, SessionRegistry};
use crate::state::{AppState, LauncherStatus, RpcsLifecycle, spawn_driver_worker};

/// guacamole runs Native on its own set.
static TEST_MIGRATOR: &sqlx::migrate::Migrator = &crate::setup::MIGRATOR;

const KEY: [u8; 32] = [3; 32];
const ERUPTOR: &str = "000009-0000";
const SPYRO: &str = "00001c-0000";

/// Records every call and mirrors its slots like the native driver; `load`
/// fails so a stray file load shows up as an error too. Advertises an IPC socket, so a BootDirect that missed the Native arm
/// would take the IPC spawn.
#[derive(Default)]
pub(crate) struct RecordingDriver {
    pub loads: SyncMutex<Vec<(u8, PathBuf)>>,
    pub places: SyncMutex<Vec<(u8, Placement)>>,
    pub clears: SyncMutex<Vec<u8>>,
    pub ipc_queried: AtomicBool,
    slots: SyncMutex<HashMap<usize, SlotState>>,
}

impl PortalDriver for RecordingDriver {
    fn open_dialog(&self) -> Result<()> {
        Ok(())
    }

    fn read_slots(&self) -> Result<[SlotState; SLOT_COUNT]> {
        let slots = self.slots.lock().unwrap();
        Ok(std::array::from_fn(|i| {
            slots.get(&i).cloned().unwrap_or(SlotState::Empty)
        }))
    }

    fn load(&self, slot: SlotIndex, path: &Path) -> Result<String> {
        self.loads
            .lock()
            .unwrap()
            .push((slot.as_u8(), path.to_path_buf()));
        anyhow::bail!("Native never loads a file")
    }

    fn clear(&self, slot: SlotIndex) -> Result<()> {
        self.clears.lock().unwrap().push(slot.as_u8());
        self.slots.lock().unwrap().remove(&slot.as_usize());
        Ok(())
    }

    fn place(&self, slot: SlotIndex, placement: &Placement) -> Result<()> {
        self.places
            .lock()
            .unwrap()
            .push((slot.as_u8(), placement.clone()));
        let loaded = SlotState::Loaded {
            figure_id: Some(placement.figure_id.clone()),
            display_name: placement.name.clone(),
            placed_by: placement.owner.as_ref().map(|o| o.profile_id.clone()),
        };
        self.slots.lock().unwrap().insert(slot.as_usize(), loaded);
        Ok(())
    }

    fn ipc_socket_path(&self) -> Option<PathBuf> {
        self.ipc_queried.store(true, Ordering::SeqCst);
        Some(PathBuf::from("/nonexistent/guac-native-test.sock"))
    }
}

/// A catalogue record with no file behind it.
fn record(id: &str, name: &str, element: Element) -> Figure {
    Figure {
        id: FigureId::new(id),
        canonical_name: name.into(),
        variant_group: name.into(),
        variant_tag: "base".into(),
        game: GameOfOrigin::SpyrosAdventure,
        element: Some(element),
        category: Category::Figure,
        vehicle_terrain: None,
        sky_path: PathBuf::new(),
        element_icon_path: None,
        tag_identity: None,
    }
}

struct Native {
    state: Arc<AppState>,
    driver: Arc<RecordingDriver>,
    root: tempfile::TempDir,
}

/// A Native `AppState` with no hooks under a tempdir runtime root:
/// `db.sqlite`, the controller's `config.json` and `data/images/` for Eruptor.
async fn native() -> Native {
    let root = tempfile::tempdir().unwrap();
    let data_root = root.path().join("data");
    let thumb = data_root.join("images").join(ERUPTOR).join("thumb.png");
    std::fs::create_dir_all(thumb.parent().unwrap()).unwrap();
    std::fs::write(&thumb, b"\x89PNG\r\n\x1a\n").unwrap();
    std::fs::write(
        root.path().join("config.json"),
        br#"{"window_mode":"tv","render_2x":false}"#,
    )
    .unwrap();

    let figures = vec![
        record(ERUPTOR, "Eruptor", Element::Fire),
        record(SPYRO, "Spyro", Element::Magic),
    ];
    let figure_index: HashMap<_, _> = figures
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id.clone(), i))
        .collect();
    let profiles = ProfileStore::open(&root.path().join("db.sqlite"), TEST_MIGRATOR)
        .await
        .unwrap();
    let sessions = Arc::new(SessionRegistry::default());
    let portal = Arc::new(Mutex::new(std::array::from_fn(|_| SlotState::Empty)));
    let (events, _) = broadcast::channel::<Event>(64);
    let rpcs3 = Arc::new(Mutex::new(RpcsLifecycle::default()));
    let launcher_status = Arc::new(std::sync::Mutex::new(LauncherStatus::default()));
    let driver = Arc::new(RecordingDriver::default());
    let driver_tx = spawn_driver_worker(
        driver.clone(),
        portal.clone(),
        events.clone(),
        profiles.clone(),
        sessions.clone(),
        Arc::new(figures.clone()),
        rpcs3.clone(),
        PathBuf::new(),
        PathBuf::new(),
        data_root.clone(),
        launcher_status.clone(),
        None,
    );
    let state = Arc::new(AppState {
        figures,
        figure_index,
        driver_kind: DriverKind::Native,
        window_mode: WindowMode::Desktop,
        render_2x: false,
        driver_tx,
        portal,
        events,
        connected_clients: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        launcher_status,
        games: Arc::new(std::sync::RwLock::new(vec![InstalledGame {
            serial: GameSerial::new("BLUS30968"),
            display_name: "Skylanders: Giants".into(),
        }])),
        games_yml: HashMap::new(),
        rpcs3_exe: PathBuf::new(),
        config_dir: PathBuf::new(),
        data_root,
        phone_dist: root.path().join("phone-dist"),
        hmac_key: KEY.to_vec(),
        boot_id: 0,
        rpcs3,
        profiles,
        sessions,
        join_qr_png: Arc::new(Vec::new()),
        hooks: Hooks::default(),
        #[cfg(feature = "test-hooks")]
        test_injector: None,
    });
    Native {
        state,
        driver,
        root,
    }
}

impl Native {
    /// A registered session with a fresh profile unlocked; returns both ids.
    async fn unlocked_session(&self) -> (SessionId, String) {
        let profile_id = self
            .state
            .profiles
            .create("Kaylee", "1234", "#e63946")
            .await
            .unwrap();
        let RegistrationOutcome::Admitted(sid) = self.state.sessions.register().await else {
            panic!("a fresh registry admits");
        };
        self.state
            .sessions
            .set_profile(sid, Some(profile_id.clone()))
            .await;
        (sid, profile_id)
    }

    /// A signed request through the full router.
    async fn send(
        &self,
        method: &str,
        uri: &str,
        body: &str,
        session: Option<SessionId>,
    ) -> StatusCode {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let sig = hex::encode(crate::http::compute_hmac(
            &KEY,
            ts,
            method,
            uri,
            body.as_bytes(),
        ));
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .header("x-skyportal-timestamp", ts.to_string())
            .header("x-skyportal-sig", sig);
        if let Some(SessionId(id)) = session {
            request = request.header("x-session-id", id.to_string());
        }
        crate::http::router(self.state.clone(), self.root.path().join("phone-dist"))
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
            .status()
    }

    /// Waits for the worker to settle `slot` on `figure`.
    async fn wait_loaded(&self, slot: usize, figure: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let SlotState::Loaded {
                figure_id: Some(id),
                ..
            } = &self.state.portal.lock().await[slot]
                && id.as_str() == figure
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "slot {slot} never loaded {figure}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Every file under the root except the database, with its bytes.
    fn files(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, root, out);
                } else if !path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("db.sqlite"))
                {
                    let rel = path.strip_prefix(root).unwrap().to_path_buf();
                    out.insert(rel, std::fs::read(&path).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(self.root.path(), self.root.path(), &mut out);
        out
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn launch_tracks_no_process_and_marks_the_game_current() {
    let n = native().await;
    let status = n
        .send("POST", "/api/launch", r#"{"serial":"BLUS30968"}"#, None)
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let life = n.state.rpcs3.lock().await;
    assert!(life.process.is_none(), "Native tracks no process");
    assert_eq!(
        life.current.as_ref().map(|g| g.serial.as_str()),
        Some("BLUS30968")
    );
    drop(life);
    {
        let st = n.state.launcher_status.lock().unwrap();
        assert_eq!(st.current_game.as_deref(), Some("Skylanders: Giants"));
        assert!(st.game_playable, "no process, so the game reveals at once");
    }
    assert!(
        !n.driver.ipc_queried.load(Ordering::SeqCst),
        "BootDirect reached a spawn arm"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_endpoints_answer_409_and_leave_config_json_alone() {
    let n = native().await;
    let before = std::fs::read(n.root.path().join("config.json")).unwrap();
    // The settings writer targets the global path, not the tempdir.
    let global = crate::paths::config_json_path().unwrap();
    let global_before = std::fs::read(&global).ok();

    for (uri, body) in [
        ("/api/launcher/window-mode", r#"{"window_mode":"desktop"}"#),
        ("/api/launcher/render-2x", r#"{"render_2x":true}"#),
        ("/api/rpcs3/settings", "{}"),
    ] {
        assert_eq!(
            n.send("POST", uri, body, None).await,
            StatusCode::CONFLICT,
            "{uri}"
        );
    }
    assert_eq!(
        std::fs::read(n.root.path().join("config.json")).unwrap(),
        before
    );
    assert_eq!(
        std::fs::read(&global).ok(),
        global_before,
        "{}",
        global.display()
    );
    assert!(n.state.rpcs3.lock().await.config_gui.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn figure_file_routes_and_slot_reset_answer_409() {
    let n = native().await;
    let (sid, profile_id) = n.unlocked_session().await;
    let figure = format!("/api/profiles/{profile_id}/figures/{ERUPTOR}");
    let reset_body = format!(r#"{{"figure_id":"{ERUPTOR}"}}"#);

    for (method, uri, body) in [
        ("GET", format!("{figure}/stats"), ""),
        ("POST", format!("{figure}/edit"), "{}"),
        ("POST", format!("{figure}/reset"), "{}"),
        ("POST", "/api/portal/slot/1/reset".to_string(), &reset_body),
    ] {
        assert_eq!(
            n.send(method, &uri, body, Some(sid)).await,
            StatusCode::CONFLICT,
            "{method} {uri}"
        );
    }
    assert!(matches!(n.state.portal.lock().await[0], SlotState::Empty));
}

/// After a run: no file load, nothing new under the root, no working-copy dir.
fn assert_no_files_touched(n: &Native, before: &BTreeMap<PathBuf, Vec<u8>>, profile_id: &str) {
    assert!(
        n.driver.loads.lock().unwrap().is_empty(),
        "load(slot, path)"
    );
    assert_eq!(&n.files(), before, "the runtime root changed");
    // Forks land under the global root until AA.5 roots `paths` at boot.
    let working = crate::paths::runtime_dir_unchecked()
        .unwrap()
        .join("working")
        .join(profile_id);
    assert!(!working.exists(), "forked into {}", working.display());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_load_places_by_identity_and_writes_no_file() {
    let n = native().await;
    let (sid, profile_id) = n.unlocked_session().await;
    let before = n.files();

    let body = format!(r#"{{"figure_id":"{ERUPTOR}"}}"#);
    let status = n
        .send("POST", "/api/portal/slot/1/load", &body, Some(sid))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    n.wait_loaded(0, ERUPTOR).await;

    assert_no_files_touched(&n, &before, &profile_id);
    let places = n.driver.places.lock().unwrap().clone();
    assert_eq!(places.len(), 1);
    let (slot, placement) = &places[0];
    assert_eq!(*slot, 0);
    assert_eq!(placement.figure_id.as_str(), ERUPTOR);
    assert_eq!(placement.toy_type, ToyTypeId(0x09));
    assert_eq!(placement.variant, MaskedVariant(0));
    assert_eq!(placement.name, "Eruptor");
    assert_eq!(placement.element, Some(Element::Fire));
    assert!(
        placement
            .portrait
            .as_ref()
            .is_some_and(|p| p.ends_with(format!("images/{ERUPTOR}/thumb.png")))
    );
    let owner = placement.owner.as_ref().expect("an unlocked profile");
    assert_eq!(owner.profile_id, profile_id);
    assert_eq!(owner.initial, "K");
    assert_eq!(owner.color, "#e63946");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_kaos_swap_places_by_identity_and_writes_no_file() {
    let n = native().await;
    let (sid, profile_id) = n.unlocked_session().await;
    let body = format!(r#"{{"figure_id":"{ERUPTOR}"}}"#);
    let status = n
        .send("POST", "/api/portal/slot/1/load", &body, Some(sid))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    n.wait_loaded(0, ERUPTOR).await;
    let before = n.files();

    let swap = crate::kaos::KaosSwap {
        slot: SlotIndex::new(0).unwrap(),
        old_figure_id: FigureId::new(ERUPTOR),
        new_figure_id: FigureId::new(SPYRO),
    };
    n.state.execute_kaos_swap(&swap, &profile_id, "Kaos!").await;
    n.wait_loaded(0, SPYRO).await;

    assert_no_files_touched(&n, &before, &profile_id);
    assert_eq!(*n.driver.clears.lock().unwrap(), vec![0]);
    let places = n.driver.places.lock().unwrap().clone();
    assert_eq!(places.len(), 2);
    assert_eq!(places[1].0, 0);
    assert_eq!(places[1].1.figure_id.as_str(), SPYRO);
    assert_eq!(
        places[1].1.owner.as_ref().map(|o| o.profile_id.as_str()),
        Some(profile_id.as_str())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restore_re_places_by_identity_and_writes_no_file() {
    let n = native().await;
    let (_, profile_id) = n.unlocked_session().await;
    let before = n.files();

    let to_restore = vec![
        (
            SlotIndex::new(2).unwrap(),
            FigureId::new(ERUPTOR),
            Some(profile_id.clone()),
        ),
        // Ownerless: no working copy to resolve, but Native needs none.
        (SlotIndex::new(5).unwrap(), FigureId::new(SPYRO), None),
        (
            SlotIndex::new(6).unwrap(),
            FigureId::new("0000ff-0000"),
            None,
        ),
    ];
    crate::state::restore_portal_figures(
        &n.state.portal,
        &n.state.events,
        &n.state.driver_tx,
        &n.state.figures,
        None,
        &to_restore,
    )
    .await;
    n.wait_loaded(2, ERUPTOR).await;
    n.wait_loaded(5, SPYRO).await;

    assert_no_files_touched(&n, &before, &profile_id);
    let mut places = n.driver.places.lock().unwrap().clone();
    places.sort_by_key(|(slot, _)| *slot);
    assert_eq!(places.len(), 2, "the unknown figure is skipped");
    assert_eq!(
        places[0].1.owner.as_ref().map(|o| o.profile_id.as_str()),
        Some(profile_id.as_str())
    );
    assert_eq!(places[1].1.owner, None);
    assert!(matches!(n.state.portal.lock().await[6], SlotState::Empty));
}

/// No process to kill under Native, so quit empties the driver too, and
/// leaves the saved layout for "resume last setup?" alone.
#[tokio::test(flavor = "multi_thread")]
async fn quit_empties_the_drivers_portal_and_keeps_the_saved_layout() {
    let n = native().await;
    let (sid, profile_id) = n.unlocked_session().await;
    let launch = r#"{"serial":"BLUS30968"}"#;
    assert_eq!(
        n.send("POST", "/api/launch", launch, Some(sid)).await,
        StatusCode::ACCEPTED
    );
    let body = format!(r#"{{"figure_id":"{ERUPTOR}"}}"#);
    let status = n
        .send("POST", "/api/portal/slot/1/load", &body, Some(sid))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    n.wait_loaded(0, ERUPTOR).await;
    let saved = n
        .state
        .profiles
        .load_portal_layout(&profile_id)
        .await
        .unwrap();
    assert!(saved.as_deref().is_some_and(|j| j.contains(ERUPTOR)));

    assert_eq!(
        n.send("POST", "/api/quit", "", Some(sid)).await,
        StatusCode::ACCEPTED
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while n.driver.clears.lock().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "the driver kept Eruptor");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert_eq!(*n.driver.clears.lock().unwrap(), vec![0]);
    let driver_slots = n.driver.read_slots().unwrap();
    assert!(driver_slots.iter().all(|s| matches!(s, SlotState::Empty)));
    let portal = n.state.portal.lock().await.clone();
    assert!(portal.iter().all(|s| matches!(s, SlotState::Empty)));
    assert_eq!(
        n.state
            .profiles
            .load_portal_layout(&profile_id)
            .await
            .unwrap(),
        saved
    );
}

#[tokio::test]
async fn the_image_route_serves_no_portrait_for_a_supplemented_figure() {
    let native = native().await;
    let jet_vac = native.state.data_root.join("images/000064-0000/thumb.png");
    std::fs::create_dir_all(jet_vac.parent().unwrap()).unwrap();
    std::fs::write(&jet_vac, b"\x89PNG\r\n\x1a\n").unwrap();

    let eruptor = format!("/api/figures/{ERUPTOR}/image");
    assert_eq!(native.send("GET", &eruptor, "", None).await, StatusCode::OK);
    let jet_vac = "/api/figures/000064-0000/image";
    assert_eq!(
        native.send("GET", jet_vac, "", None).await,
        StatusCode::NOT_FOUND
    );
}
