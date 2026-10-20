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
    Element, Event, FigureId, GameSerial, InstalledGame, SLOT_COUNT, SlotIndex, SlotState,
    SubtypeWord, ToyTypeId,
};
use skylander_rpcs3_control::{FigureProgress, Placement, PortalDriver};
use tokio::sync::{Mutex, broadcast};
use tower::ServiceExt;

use crate::config::{DriverKind, WindowMode};
use crate::figures::{FigureList, FigureSet, RosterState, fixture};
use crate::hooks::Hooks;
use crate::instances::Caps;
use crate::profiles::{ProfileStore, RegistrationOutcome, SessionId, SessionRegistry};
use crate::progress::{LevelHero, ProgressionSlot};
use crate::state::{AppState, LauncherStatus, RpcsLifecycle, spawn_driver_worker};

/// guacamole runs Native on its own set.
static TEST_MIGRATOR: &sqlx::migrate::Migrator = &crate::setup::MIGRATOR;

const KEY: [u8; 32] = [3; 32];
const ERUPTOR: &str = "000009-0000";
const SPYRO: &str = "000010-0000";
/// Where `native()`'s roster keeps its portraits, under the root.
const EXTRACT: &str = "extract/BLUS30968-01.00-01010101";

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

struct Native {
    state: Arc<AppState>,
    driver: Arc<RecordingDriver>,
    root: tempfile::TempDir,
}

/// A Native `AppState` with no hooks under a tempdir runtime root:
/// `db.sqlite`, the controller's `config.json` and Eruptor's and Spyro's
/// roster already landed, Eruptor's portrait in the extract and Spyro's not.
async fn native() -> Native {
    let n = native_waiting().await;
    let extract = n.root.path().join(EXTRACT);
    let roster = fixture::eruptor_and_spyro();
    n.state
        .figures
        .publish(FigureSet::from_roster(&roster, &extract));
    n
}

/// [`native`] before its roster lands: reading it off the disc.
async fn native_waiting() -> Native {
    let root = tempfile::tempdir().unwrap();
    let portrait = root
        .path()
        .join(EXTRACT)
        .join(format!("portraits/{ERUPTOR}.png"));
    std::fs::create_dir_all(portrait.parent().unwrap()).unwrap();
    std::fs::write(&portrait, b"\x89PNG\r\n\x1a\n").unwrap();
    std::fs::write(
        root.path().join("config.json"),
        br#"{"window_mode":"tv","render_2x":false}"#,
    )
    .unwrap();

    let figures = FigureList::new(FigureSet::waiting(RosterState::Reading));
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
        figures.clone(),
        rpcs3.clone(),
        PathBuf::new(),
        PathBuf::new(),
        launcher_status.clone(),
        None,
    );
    let state = Arc::new(AppState {
        figures,
        held_resumes: Mutex::new(Vec::new()),
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
            art: None,
        }])),
        games_yml: HashMap::new(),
        rpcs3_exe: PathBuf::new(),
        config_dir: PathBuf::new(),
        data_root: PathBuf::new(),
        phone_dist: root.path().join("phone-dist"),
        hmac_key: KEY.to_vec(),
        boot_id: 0,
        rpcs3,
        profiles,
        sessions,
        join_qr_png: Arc::new(Vec::new()),
        hooks: Hooks::default(),
        progression: ProgressionSlot::default(),
        level_hero: LevelHero::default(),
        #[cfg(feature = "test-hooks")]
        test_injector: None,
    });
    crate::http::spawn_resume_release(state.clone());
    Native {
        state,
        driver,
        root,
    }
}

impl Native {
    /// Waits for the worker to empty `slot`.
    async fn wait_empty(&self, slot: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !matches!(self.state.portal.lock().await[slot], SlotState::Empty) {
            assert!(Instant::now() < deadline, "slot {slot} never emptied");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// How many rows guacamole's figure store holds.
    async fn instance_rows(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM figure_instances")
            .fetch_one(&self.state.profiles.pool)
            .await
            .unwrap()
    }

    /// The placement the driver got last.
    fn last_place(&self) -> (u8, Placement) {
        self.driver.places.lock().unwrap().last().cloned().unwrap()
    }

    /// A registered session with a fresh profile unlocked; returns both ids.
    async fn unlocked_session(&self) -> (SessionId, String) {
        self.unlocked_session_as("Kaylee", "#e63946").await
    }

    /// [`Native::unlocked_session`] for a profile named `name` (PIN 1234).
    async fn unlocked_session_as(&self, name: &str, color: &str) -> (SessionId, String) {
        let profile_id = self
            .state
            .profiles
            .create(name, "1234", color)
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
        self.send_read(method, uri, body, session).await.0
    }

    /// [`Native::send`], with the answer's body.
    async fn send_read(
        &self,
        method: &str,
        uri: &str,
        body: &str,
        session: Option<SessionId>,
    ) -> (StatusCode, String) {
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
        let response = crate::http::router(self.state.clone(), self.root.path().join("phone-dist"))
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).into_owned())
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

    /// Waits for `profile_id`'s saved layout to hold every figure in `figures`.
    async fn wait_saved(&self, profile_id: &str, figures: &[&str]) -> [SlotState; SLOT_COUNT] {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let json = self
                .state
                .profiles
                .load_portal_layout(profile_id)
                .await
                .unwrap();
            if let Some(json) = json
                && figures.iter().all(|f| json.contains(f))
            {
                return serde_json::from_str(&json).unwrap();
            }
            assert!(
                Instant::now() < deadline,
                "{profile_id}'s layout never saved {figures:?}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Unlocks `profile_id` on `session` with its PIN, as the phone does, and
    /// returns what "resume last setup?" offers it, if anything.
    async fn resume_offer(
        &self,
        session: SessionId,
        profile_id: &str,
    ) -> Option<[SlotState; SLOT_COUNT]> {
        let mut events = self.state.events.subscribe();
        let uri = format!("/api/profiles/{profile_id}/unlock");
        let status = self
            .send("POST", &uri, r#"{"pin":"1234"}"#, Some(session))
            .await;
        assert_eq!(status, StatusCode::OK, "unlock {profile_id}");
        // The handler broadcasts the prompt before it answers.
        let mut offer = None;
        while let Ok(event) = events.try_recv() {
            if let Event::ResumePrompt { session_id, slots } = event
                && session_id == session.0
            {
                offer = Some(slots);
            }
        }
        offer
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

/// The phone tells the apps apart by `/api/status`'s `app` (AA.18, AA.19).
#[tokio::test(flavor = "multi_thread")]
async fn status_says_guacamole() {
    let n = native().await;
    let response = crate::http::router(n.state.clone(), n.root.path().join("phone-dist"))
        .oneshot(Request::get("/api/status").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["app"], "guacamole", "{body}");
    assert_eq!(body["rpcs3_running"], false, "{body}");
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

/// Before the extract holds the game's table (AH.2), the stats and the edit
/// answer 409, as AA's did; the reset needs no table.
#[tokio::test(flavor = "multi_thread")]
async fn the_stats_and_the_edit_answer_409_until_the_table_is_read() {
    let n = native().await;
    let (sid, profile_id) = n.unlocked_session().await;
    let figure = format!("/api/profiles/{profile_id}/figures/{ERUPTOR}");
    assert_eq!(
        n.send("GET", &format!("{figure}/stats"), "", Some(sid))
            .await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        n.send("POST", &format!("{figure}/edit"), &edit(10, 0), Some(sid))
            .await,
        StatusCode::CONFLICT
    );
    assert_eq!(n.instance_rows().await, 0);
    assert_eq!(
        n.send("POST", &format!("{figure}/reset"), "", Some(sid))
            .await,
        StatusCode::ACCEPTED
    );
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
    assert_eq!(placement.variant, SubtypeWord(0));
    assert_eq!(placement.name, "Eruptor");
    assert_eq!(placement.element, Some(Element::Fire));
    assert_eq!(
        placement.portrait.as_deref(),
        Some(
            n.root
                .path()
                .join(EXTRACT)
                .join(format!("portraits/{ERUPTOR}.png"))
                .as_path()
        )
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
    // The swap is the profile's first placement of Spyro: his row is made (AH.1).
    let spyro = n.state.profiles.instances().read(&profile_id, SPYRO).await;
    let spyro = spyro.unwrap().expect("made at the swap's placement");
    assert_eq!(places[1].1.owner.as_ref().unwrap().progress, spyro.progress);
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
        n.state.figures.now().figures(),
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
    // The layout save lands after the slot reads Loaded, so poll for it.
    let deadline = Instant::now() + Duration::from_secs(5);
    let saved = loop {
        let saved = n
            .state
            .profiles
            .load_portal_layout(&profile_id)
            .await
            .unwrap();
        if saved.as_deref().is_some_and(|j| j.contains(ERUPTOR)) {
            break saved;
        }
        assert!(Instant::now() < deadline, "the layout never saved Eruptor");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };

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

fn owner(slot: &SlotState) -> Option<&str> {
    match slot {
        SlotState::Loaded { placed_by, .. } => placed_by.as_deref(),
        _ => None,
    }
}

/// openspec AB.22: the saved layout is the whole portal with each slot's
/// owner, saved for every unlocked profile, and "resume last setup?" offers
/// each profile only the figures it placed.
#[tokio::test(flavor = "multi_thread")]
async fn a_resume_offers_only_the_resumers_own_figures() {
    let n = native().await;
    let (alice_sid, alice) = n.unlocked_session_as("Alice", "#e8423f").await;
    let (bob_sid, bob) = n.unlocked_session_as("Bob", "#3f8ee8").await;

    // Alice's Eruptor lands in Bob's layout too, and offers Bob nothing.
    let eruptor = format!(r#"{{"figure_id":"{ERUPTOR}"}}"#);
    let status = n
        .send("POST", "/api/portal/slot/1/load", &eruptor, Some(alice_sid))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    n.wait_loaded(0, ERUPTOR).await;
    let saved = n.wait_saved(&bob, &[ERUPTOR]).await;
    assert_eq!(owner(&saved[0]), Some(alice.as_str()));
    assert_eq!(n.resume_offer(bob_sid, &bob).await, None);

    // Bob's Spyro: both layouts hold both figures, each with its owner.
    let spyro = format!(r#"{{"figure_id":"{SPYRO}"}}"#);
    let status = n
        .send("POST", "/api/portal/slot/2/load", &spyro, Some(bob_sid))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    n.wait_loaded(1, SPYRO).await;
    for profile in [&alice, &bob] {
        let saved = n.wait_saved(profile, &[ERUPTOR, SPYRO]).await;
        assert_eq!(owner(&saved[0]), Some(alice.as_str()), "{profile}'s layout");
        assert_eq!(owner(&saved[1]), Some(bob.as_str()), "{profile}'s layout");
    }

    // Each resume offers only that profile's own figure.
    let saved = n.wait_saved(&bob, &[ERUPTOR, SPYRO]).await;
    for (sid, profile, slot) in [(bob_sid, &bob, 1), (alice_sid, &alice, 0)] {
        let offer = n
            .resume_offer(sid, profile)
            .await
            .unwrap_or_else(|| panic!("{profile} was offered nothing"));
        for (i, s) in offer.iter().enumerate() {
            if i == slot {
                assert_eq!(s, &saved[i], "{profile}'s own slot {i}");
            } else {
                assert!(s.is_empty(), "{profile} was offered slot {i}: {s:?}");
            }
        }
    }
}

/// openspec AB.22: a load can't land on another profile's figure, so a
/// resume whose slot another kid filled after the prompt claims nothing.
/// The owner's own load still lands (a swap, or AB.21's same-figure resume).
#[tokio::test(flavor = "multi_thread")]
async fn a_load_over_another_profiles_figure_is_refused_and_changes_nothing() {
    let n = native().await;
    let (alice_sid, alice) = n.unlocked_session_as("Alice", "#e8423f").await;
    let (bob_sid, bob) = n.unlocked_session_as("Bob", "#3f8ee8").await;
    let eruptor = format!(r#"{{"figure_id":"{ERUPTOR}"}}"#);
    let spyro = format!(r#"{{"figure_id":"{SPYRO}"}}"#);
    let load = "/api/portal/slot/1/load";
    assert_eq!(
        n.send("POST", load, &eruptor, Some(alice_sid)).await,
        StatusCode::ACCEPTED
    );
    n.wait_loaded(0, ERUPTOR).await;
    let held = n.state.portal.lock().await[0].clone();
    assert_eq!(owner(&held), Some(alice.as_str()));

    for body in [&eruptor, &spyro] {
        assert_eq!(
            n.send("POST", load, body, Some(bob_sid)).await,
            StatusCode::CONFLICT,
            "{body}"
        );
    }
    assert_eq!(n.state.portal.lock().await[0], held);
    assert_eq!(n.driver.places.lock().unwrap().len(), 1);

    assert_eq!(
        n.send("POST", load, &spyro, Some(alice_sid)).await,
        StatusCode::ACCEPTED
    );
    n.wait_loaded(0, SPYRO).await;
    let slot2 = "/api/portal/slot/2/load";
    assert_eq!(
        n.send("POST", slot2, &eruptor, Some(bob_sid)).await,
        StatusCode::ACCEPTED
    );
    n.wait_loaded(1, ERUPTOR).await;
    assert_eq!(owner(&n.state.portal.lock().await[1]), Some(bob.as_str()));
}

/// AE.4: guacamole serves the portrait the roster stage extracted, and a
/// 404 for a figure without one or not on the list; nothing from `data/`.
#[tokio::test]
async fn the_image_route_serves_the_extracted_portrait_only() {
    let n = native().await;
    let eruptor = format!("/api/figures/{ERUPTOR}/image");
    assert_eq!(n.send("GET", &eruptor, "", None).await, StatusCode::OK);
    let hero = format!("{eruptor}?size=hero");
    assert_eq!(n.send("GET", &hero, "", None).await, StatusCode::OK);
    // Spyro's roster row names a portrait the extract doesn't hold.
    let spyro = format!("/api/figures/{SPYRO}/image");
    assert_eq!(n.send("GET", &spyro, "", None).await, StatusCode::NOT_FOUND);
    // A scraped thumb where the product keeps them is never read.
    let jet_vac = n.root.path().join("data/images/000064-0000/thumb.png");
    std::fs::create_dir_all(jet_vac.parent().unwrap()).unwrap();
    std::fs::write(&jet_vac, b"\x89PNG\r\n\x1a\n").unwrap();
    let jet_vac = "/api/figures/000064-0000/image";
    assert_eq!(
        n.send("GET", jet_vac, "", None).await,
        StatusCode::NOT_FOUND
    );
}

/// Saves `slots` as `profile_id`'s last portal layout.
async fn save_layout(n: &Native, profile_id: &str, slots: &[(usize, &str)]) {
    let mut layout: [SlotState; SLOT_COUNT] = std::array::from_fn(|_| SlotState::Empty);
    for (slot, id) in slots {
        layout[*slot] = SlotState::Loaded {
            figure_id: Some(FigureId::new(*id)),
            display_name: (*id).into(),
            placed_by: Some(profile_id.into()),
        };
    }
    n.state
        .profiles
        .save_portal_layout(profile_id, &serde_json::to_string(&layout).unwrap())
        .await
        .unwrap();
}

/// AE.4: a saved figure the roster doesn't know is skipped (and logged); the
/// rest is offered and places.
#[tokio::test(flavor = "multi_thread")]
async fn a_resume_holding_an_unknown_id_places_the_rest() {
    let n = native().await;
    let (sid, profile_id) = n.unlocked_session().await;
    save_layout(
        &n,
        &profile_id,
        &[(0, ERUPTOR), (2, "0000ff-0000"), (4, SPYRO)],
    )
    .await;

    let offer = n
        .resume_offer(sid, &profile_id)
        .await
        .expect("the known figures are offered");
    let offered: Vec<(usize, &str)> = offer
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            SlotState::Loaded {
                figure_id: Some(id),
                ..
            } => Some((i, id.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(offered, [(0, ERUPTOR), (4, SPYRO)]);

    // RESUME, as the phone sends it: one load per offered slot.
    for (slot, id) in &offered {
        let uri = format!("/api/portal/slot/{}/load", slot + 1);
        let body = format!(r#"{{"figure_id":"{id}"}}"#);
        assert_eq!(
            n.send("POST", &uri, &body, Some(sid)).await,
            StatusCode::ACCEPTED
        );
    }
    n.wait_loaded(0, ERUPTOR).await;
    n.wait_loaded(4, SPYRO).await;
    assert!(n.state.portal.lock().await[2].is_empty());
}

/// AE.4: an unlock before the roster lands offers nothing yet; the roster
/// landing sends the prompt, once, to the session still holding the profile.
#[tokio::test(flavor = "multi_thread")]
async fn a_resume_waits_for_the_roster() {
    let n = native_waiting().await;
    let (sid, profile_id) = n.unlocked_session().await;
    let (locked_sid, locked) = n.unlocked_session_as("Bob", "#3f8ee8").await;
    save_layout(&n, &profile_id, &[(1, ERUPTOR)]).await;
    save_layout(&n, &locked, &[(3, SPYRO)]).await;
    let mut events = n.state.events.subscribe();

    assert_eq!(n.resume_offer(sid, &profile_id).await, None);
    assert_eq!(n.resume_offer(locked_sid, &locked).await, None);
    assert_eq!(n.state.held_resumes.lock().await.len(), 2);
    // Bob locks before the roster lands: his prompt is dropped.
    n.state.sessions.set_profile(locked_sid, None).await;
    // A state change that isn't the roster sends nothing.
    n.state.figures.set_roster_state(RosterState::Unreadable);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(n.state.held_resumes.lock().await.len(), 2);

    let roster = fixture::eruptor_and_spyro();
    let extract = n.root.path().join(EXTRACT);
    n.state
        .figures
        .publish(FigureSet::from_roster(&roster, &extract));
    let deadline = Instant::now() + Duration::from_secs(5);
    let slots = loop {
        if let Ok(Ok(Event::ResumePrompt { session_id, slots })) =
            tokio::time::timeout(Duration::from_millis(100), events.recv()).await
        {
            assert_eq!(session_id, sid.0, "only the unlocked session's");
            break slots;
        }
        assert!(Instant::now() < deadline, "the held prompt never went out");
    };
    assert!(matches!(
        &slots[1],
        SlotState::Loaded { figure_id: Some(id), .. } if id.as_str() == ERUPTOR
    ));
    assert!(n.state.held_resumes.lock().await.is_empty());
    tokio::time::sleep(Duration::from_millis(200)).await;
    while let Ok(event) = events.try_recv() {
        assert!(
            !matches!(event, Event::ResumePrompt { .. }),
            "a second prompt: {event:?}"
        );
    }
}

/// AE.4: a load racing the roster's landing, and loads while the same roster
/// is published again (a pick over a current extract), each read one list
/// whole: refused before it lands, placed with its name and portrait after,
/// never an error in between.
#[tokio::test(flavor = "multi_thread")]
async fn a_load_racing_the_roster_swap_is_refused_or_placed_whole() {
    use std::sync::atomic::AtomicUsize;

    let n = native_waiting().await;
    let (sid, _) = n.unlocked_session().await;
    let mut events = n.state.events.subscribe();
    let extract = n.root.path().join(EXTRACT);
    let body = format!(r#"{{"figure_id":"{ERUPTOR}"}}"#);

    // The roster lands from another thread while the phone keeps asking.
    let tries = Arc::new(AtomicUsize::new(0));
    let publisher = {
        let (list, tries, extract) = (n.state.figures.clone(), tries.clone(), extract.clone());
        std::thread::spawn(move || {
            while tries.load(Ordering::SeqCst) < 3 {
                std::thread::yield_now();
            }
            list.publish(FigureSet::from_roster(
                &fixture::eruptor_and_spyro(),
                &extract,
            ));
        })
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut refused = 0;
    loop {
        let status = n
            .send("POST", "/api/portal/slot/1/load", &body, Some(sid))
            .await;
        tries.fetch_add(1, Ordering::SeqCst);
        match status {
            StatusCode::NOT_FOUND => refused += 1,
            StatusCode::ACCEPTED => break,
            other => panic!("a load during the swap answered {other}"),
        }
        assert!(Instant::now() < deadline, "the roster never landed");
    }
    publisher.join().unwrap();
    assert!(refused >= 3, "{refused}");
    n.wait_loaded(0, ERUPTOR).await;

    // The same roster published over and over while loads and clears run.
    let stop = Arc::new(AtomicBool::new(false));
    let republisher = {
        let (list, stop, extract) = (n.state.figures.clone(), stop.clone(), extract.clone());
        std::thread::spawn(move || {
            let mut published = 0;
            while !stop.load(Ordering::SeqCst) {
                list.publish(FigureSet::from_roster(
                    &fixture::eruptor_and_spyro(),
                    &extract,
                ));
                published += 1;
            }
            published
        })
    };
    for _ in 0..20 {
        let status = n
            .send("POST", "/api/portal/slot/1/clear", "{}", Some(sid))
            .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !n.state.portal.lock().await[0].is_empty() {
            assert!(Instant::now() < deadline, "slot 1 never cleared");
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let status = n
            .send("POST", "/api/portal/slot/1/load", &body, Some(sid))
            .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        n.wait_loaded(0, ERUPTOR).await;
    }
    stop.store(true, Ordering::SeqCst);
    assert!(republisher.join().unwrap() > 0);

    let places = n.driver.places.lock().unwrap().clone();
    assert_eq!(places.len(), 21);
    let portrait = extract.join(format!("portraits/{ERUPTOR}.png"));
    for (slot, p) in &places {
        assert_eq!(*slot, 0);
        assert_eq!(p.name, "Eruptor");
        assert_eq!(p.portrait.as_deref(), Some(portrait.as_path()));
    }
    loop {
        let event = match events.try_recv() {
            Ok(event) => event,
            Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
            Err(_) => break,
        };
        assert!(
            !matches!(
                event,
                Event::Error { .. }
                    | Event::SlotChanged {
                        state: SlotState::Error { .. },
                        ..
                    }
            ),
            "{event:?}"
        );
    }
}

/// AH.1: a profile's first placement of a figure makes its row fresh and the
/// placement carries it; a second leaves the row as play left it.
#[tokio::test(flavor = "multi_thread")]
async fn a_first_placement_makes_the_row_fresh_and_a_second_leaves_it_alone() {
    let n = native().await;
    let (sid, alice) = n.unlocked_session().await;
    let body = format!(r#"{{"figure_id":"{ERUPTOR}"}}"#);
    let load = |slot: u8| format!("/api/portal/slot/{slot}/load");
    assert_eq!(
        n.send("POST", &load(1), &body, Some(sid)).await,
        StatusCode::ACCEPTED
    );
    n.wait_loaded(0, ERUPTOR).await;
    let row = n.state.profiles.instances().read(&alice, ERUPTOR).await;
    let row = row.unwrap().expect("made at the first placement");
    let p = row.progress;
    assert_eq!((p.xp, p.gold, p.playtime_secs), (0, 0, 0));
    assert!(p.generation > 0);
    let (_, placed) = n.last_place();
    assert_eq!(placed.owner.unwrap().progress, p);

    // Earned in play, lifted, placed again.
    let earned = FigureProgress {
        xp: 16,
        gold: 3,
        playtime_secs: 61,
        ..p
    };
    let wrote = n
        .state
        .profiles
        .instances()
        .write(&alice, ERUPTOR, earned, true, Caps::NONE);
    assert!(wrote.await.unwrap().is_some());
    let clear = n.send("POST", "/api/portal/slot/1/clear", "", Some(sid));
    assert_eq!(clear.await, StatusCode::ACCEPTED);
    n.wait_empty(0).await;
    assert_eq!(
        n.send("POST", &load(2), &body, Some(sid)).await,
        StatusCode::ACCEPTED
    );
    n.wait_loaded(1, ERUPTOR).await;
    let again = n.state.profiles.instances().read(&alice, ERUPTOR).await;
    assert_eq!(
        again.unwrap().unwrap().progress,
        earned,
        "the row was touched"
    );
    let (slot, placed) = n.last_place();
    assert_eq!(slot, 1);
    assert_eq!(placed.owner.unwrap().progress, earned);
    assert_eq!(n.instance_rows().await, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn one_figure_placed_by_two_profiles_is_two_rows() {
    let n = native().await;
    let (alice_sid, alice) = n.unlocked_session().await;
    let (bob_sid, bob) = n.unlocked_session_as("Bob", "#2a9d8f").await;
    let body = format!(r#"{{"figure_id":"{ERUPTOR}"}}"#);
    for (slot, sid) in [(1usize, alice_sid), (2, bob_sid)] {
        let uri = format!("/api/portal/slot/{slot}/load");
        assert_eq!(
            n.send("POST", &uri, &body, Some(sid)).await,
            StatusCode::ACCEPTED
        );
        n.wait_loaded(slot - 1, ERUPTOR).await;
    }
    assert_eq!(n.instance_rows().await, 2);
    let instances = n.state.profiles.instances();
    let alices = instances.read(&alice, ERUPTOR).await.unwrap().unwrap();
    let bobs = instances.read(&bob, ERUPTOR).await.unwrap().unwrap();
    assert_ne!(alices.progress.generation, bobs.progress.generation);
    let mut places = n.driver.places.lock().unwrap().clone();
    places.sort_by_key(|(slot, _)| *slot);
    let owners: Vec<_> = places
        .iter()
        .map(|(_, p)| {
            let o = p.owner.clone().unwrap();
            (o.profile_id, o.progress)
        })
        .collect();
    assert_eq!(owners, vec![(alice, alices.progress), (bob, bobs.progress)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_placement_with_no_profile_unlocked_makes_no_row_and_carries_none() {
    let n = native().await;
    let RegistrationOutcome::Admitted(sid) = n.state.sessions.register().await else {
        panic!("a fresh registry admits");
    };
    let body = format!(r#"{{"figure_id":"{ERUPTOR}"}}"#);
    let status = n
        .send("POST", "/api/portal/slot/4/load", &body, Some(sid))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    n.wait_loaded(3, ERUPTOR).await;
    let (_, placed) = n.last_place();
    assert_eq!(placed.owner, None, "no owner, so no progress either");
    assert_eq!(n.instance_rows().await, 0);
}

// ---- AH.3: guacamole's figure editor on the rows ---------------------------

/// Giants' numbers, as AH.2 reads them off the disc.
fn giants() -> crate::progress::Progression {
    crate::progress::Progression {
        level_xp: vec![
            0, 1000, 2200, 3800, 6000, 9000, 13000, 18200, 24800, 33000, 42700, 53900, 66600,
            80800, 96500,
        ],
        gold_cap: 65_000,
    }
}

fn edit(level: u8, gold: u32) -> String {
    format!(r#"{{"level":{level},"gold":{gold}}}"#)
}

/// Every `FigureUpdated` sent since the last call, as (figure, level, gold).
fn figure_updates(events: &mut broadcast::Receiver<Event>) -> Vec<(String, u8, u16)> {
    let mut out = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let Event::FigureUpdated {
            figure_id,
            level,
            gold,
        } = event
        {
            out.push((figure_id.0, level, gold));
        }
    }
    out
}

impl Native {
    /// [`native`] with Giants' table read.
    async fn with_giants() -> Native {
        let n = native().await;
        n.state.progression.publish(giants());
        n
    }

    async fn row(&self, profile_id: &str, figure_id: &str) -> Option<FigureProgress> {
        let row = self.state.profiles.instances().read(profile_id, figure_id);
        row.await.unwrap().map(|r| r.progress)
    }

    /// Places `figure` in display slot `slot` from `sid` and waits for it.
    async fn place(&self, sid: SessionId, slot: u8, figure: &str) {
        let body = format!(r#"{{"figure_id":"{figure}"}}"#);
        let uri = format!("/api/portal/slot/{slot}/load");
        assert_eq!(
            self.send("POST", &uri, &body, Some(sid)).await,
            StatusCode::ACCEPTED
        );
        self.wait_loaded(usize::from(slot - 1), figure).await;
    }

    async fn edit(
        &self,
        sid: SessionId,
        profile_id: &str,
        figure: &str,
        level: u8,
        gold: u32,
    ) -> StatusCode {
        let uri = format!("/api/profiles/{profile_id}/figures/{figure}/edit");
        self.send("POST", &uri, &edit(level, gold), Some(sid)).await
    }

    async fn reset(&self, sid: SessionId, profile_id: &str, figure: &str) -> StatusCode {
        let uri = format!("/api/profiles/{profile_id}/figures/{figure}/reset");
        self.send("POST", &uri, "", Some(sid)).await
    }

    async fn reset_slot(&self, sid: SessionId, slot: u8, figure: &str) -> StatusCode {
        let uri = format!("/api/portal/slot/{slot}/reset");
        let body = format!(r#"{{"figure_id":"{figure}"}}"#);
        self.send("POST", &uri, &body, Some(sid)).await
    }

    async fn stats(&self, profile_id: &str, figure: &str) -> (StatusCode, serde_json::Value) {
        let uri = format!("/api/profiles/{profile_id}/figures/{figure}/stats");
        let (status, body) = self.send_read("GET", &uri, "", None).await;
        (status, serde_json::from_str(&body).unwrap_or_default())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_stats_read_the_row_with_the_games_caps() {
    let n = Native::with_giants().await;
    let (sid, alice) = n.unlocked_session().await;

    // Never placed: the phone's never-placed state, still told the caps.
    let (status, body) = n.stats(&alice, ERUPTOR).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        body,
        serde_json::json!({"max_level": 15, "max_gold": 65000})
    );

    // A first placement: fresh.
    n.place(sid, 1, ERUPTOR).await;
    let (status, body) = n.stats(&alice, ERUPTOR).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!({
            "level": 1, "xp": 0, "gold": 0, "playtime_secs": 0,
            "max_level": 15, "max_gold": 65000,
        })
    );

    // What the TV earned, by the table: 1,000 is level 2.
    let row = n.row(&alice, ERUPTOR).await.unwrap();
    let earned = FigureProgress {
        xp: 1000,
        gold: 3,
        playtime_secs: 61,
        ..row
    };
    let wrote = n
        .state
        .profiles
        .instances()
        .write(&alice, ERUPTOR, earned, true, giants().caps());
    assert!(wrote.await.unwrap().is_some());
    let (_, body) = n.stats(&alice, ERUPTOR).await;
    assert_eq!(body["level"], 2);
    assert_eq!(body["xp"], 1000);
    assert_eq!(body["gold"], 3);
    assert_eq!(body["playtime_secs"], 61);
}

/// The phone's edit: experience to the level's threshold, gold as asked,
/// playtime kept, a new generation each time, `FigureUpdated` each time; a
/// never-placed figure's first edit makes its row; out of range refused.
#[tokio::test(flavor = "multi_thread")]
async fn an_edit_sets_the_levels_threshold_and_the_gold_under_a_new_generation() {
    let n = Native::with_giants().await;
    let (sid, alice) = n.unlocked_session().await;
    let mut events = n.state.events.subscribe();

    // Never placed: the first edit makes the row.
    assert_eq!(
        n.edit(sid, &alice, ERUPTOR, 10, 1234).await,
        StatusCode::ACCEPTED
    );
    let first = n.row(&alice, ERUPTOR).await.expect("made by the edit");
    assert_eq!(
        (first.xp, first.gold, first.playtime_secs),
        (33_000, 1234, 0)
    );
    assert_eq!(
        figure_updates(&mut events),
        vec![(ERUPTOR.to_string(), 10, 1234)]
    );
    let (_, body) = n.stats(&alice, ERUPTOR).await;
    assert_eq!(
        (body["level"].clone(), body["xp"].clone()),
        (10.into(), 33_000.into())
    );

    // Playtime the TV wrote survives the next edit.
    let played = FigureProgress {
        playtime_secs: 120,
        ..first
    };
    let wrote = n
        .state
        .profiles
        .instances()
        .write(&alice, ERUPTOR, played, true, giants().caps());
    assert!(wrote.await.unwrap().is_some());

    let mut generation = first.generation;
    for (level, gold, xp) in [(1, 0, 0), (15, 65_000, 96_500), (10, 0, 33_000)] {
        assert_eq!(
            n.edit(sid, &alice, ERUPTOR, level, gold).await,
            StatusCode::ACCEPTED,
            "level {level}, gold {gold}"
        );
        let row = n.row(&alice, ERUPTOR).await.unwrap();
        assert_eq!((row.xp, row.gold, row.playtime_secs), (xp, gold, 120));
        assert!(row.generation > generation, "level {level}: {row:?}");
        generation = row.generation;
        let gold = u16::try_from(gold).unwrap();
        assert_eq!(
            figure_updates(&mut events),
            vec![(ERUPTOR.to_string(), level, gold)]
        );
    }

    // Out of range: refused, nothing changed, nothing announced.
    let before = n.row(&alice, ERUPTOR).await;
    for (level, gold) in [(0, 0), (16, 0), (1, 65_001)] {
        assert_eq!(
            n.edit(sid, &alice, ERUPTOR, level, gold).await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "level {level}, gold {gold}"
        );
    }
    let uri = format!("/api/profiles/{alice}/figures/{ERUPTOR}/edit");
    for bad in [r#"{"level":2}"#, r#"{"level":-1,"gold":0}"#, "nope"] {
        assert_eq!(
            n.send("POST", &uri, bad, Some(sid)).await,
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }
    assert_eq!(
        n.edit(sid, &alice, "000099-0000", 2, 0).await,
        StatusCode::NOT_FOUND,
        "a figure off the list"
    );
    assert_eq!(n.row(&alice, ERUPTOR).await, before);
    assert!(figure_updates(&mut events).is_empty());
}

/// AA.18's gate: an edit or a reset is refused while the viewer's own
/// placement holds the figure, not while only another profile's does.
#[tokio::test(flavor = "multi_thread")]
async fn only_the_viewers_own_placement_stops_an_edit_or_a_reset() {
    let n = Native::with_giants().await;
    let (alice_sid, alice) = n.unlocked_session().await;
    let (bob_sid, bob) = n.unlocked_session_as("Bob", "#2a9d8f").await;

    n.place(bob_sid, 1, ERUPTOR).await;
    let bobs = n.row(&bob, ERUPTOR).await.unwrap();
    assert_eq!(
        n.edit(alice_sid, &alice, ERUPTOR, 2, 50).await,
        StatusCode::ACCEPTED,
        "only Bob's Eruptor is on the portal"
    );
    assert_eq!(n.row(&alice, ERUPTOR).await.unwrap().xp, 1000);
    assert_eq!(n.row(&bob, ERUPTOR).await.unwrap(), bobs, "Bob's untouched");
    assert_eq!(
        n.edit(bob_sid, &bob, ERUPTOR, 2, 50).await,
        StatusCode::CONFLICT,
        "Bob's own is on the portal"
    );
    assert_eq!(n.reset(bob_sid, &bob, ERUPTOR).await, StatusCode::CONFLICT);
    assert_eq!(n.row(&bob, ERUPTOR).await.unwrap(), bobs);

    n.place(alice_sid, 2, ERUPTOR).await;
    let alices = n.row(&alice, ERUPTOR).await.unwrap();
    assert_eq!(
        n.edit(alice_sid, &alice, ERUPTOR, 5, 0).await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        n.reset(alice_sid, &alice, ERUPTOR).await,
        StatusCode::CONFLICT
    );
    assert_eq!(n.row(&alice, ERUPTOR).await.unwrap(), alices);
}

/// The path's profile has to be the one the requesting session unlocked: a
/// sibling's session, or one with nothing unlocked, changes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_siblings_session_can_neither_edit_nor_reset() {
    let n = Native::with_giants().await;
    let (alice_sid, _alice) = n.unlocked_session().await;
    let (bob_sid, bob) = n.unlocked_session_as("Bob", "#2a9d8f").await;
    assert_eq!(
        n.edit(bob_sid, &bob, ERUPTOR, 5, 500).await,
        StatusCode::ACCEPTED
    );
    let bobs = n.row(&bob, ERUPTOR).await.unwrap();
    let mut events = n.state.events.subscribe();

    let refused = || async {
        assert_eq!(
            n.edit(alice_sid, &bob, ERUPTOR, 15, 65_000).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            n.reset(alice_sid, &bob, ERUPTOR).await,
            StatusCode::FORBIDDEN
        );
    };
    refused().await;
    // Alice's phone locked: nothing unlocked at all.
    n.state.sessions.set_profile(alice_sid, None).await;
    refused().await;
    assert_eq!(n.row(&bob, ERUPTOR).await.unwrap(), bobs);
    assert!(figure_updates(&mut events).is_empty());
}

/// A reset drops the row; the next placement is fresh under a newer
/// generation, so nothing the TV sent before it lands.
#[tokio::test(flavor = "multi_thread")]
async fn a_reset_drops_the_row_and_the_next_placement_is_fresh() {
    let n = Native::with_giants().await;
    let (sid, alice) = n.unlocked_session().await;
    assert_eq!(
        n.edit(sid, &alice, ERUPTOR, 10, 500).await,
        StatusCode::ACCEPTED
    );
    let edited = n.row(&alice, ERUPTOR).await.unwrap();
    let mut events = n.state.events.subscribe();

    assert_eq!(n.reset(sid, &alice, ERUPTOR).await, StatusCode::ACCEPTED);
    assert_eq!(n.row(&alice, ERUPTOR).await, None);
    assert_eq!(
        figure_updates(&mut events),
        vec![(ERUPTOR.to_string(), 1, 0)]
    );
    assert_eq!(n.stats(&alice, ERUPTOR).await.0, StatusCode::NOT_FOUND);

    // What the TV sent under the edit's generation finds no row.
    let late = FigureProgress {
        xp: 34_000,
        ..edited
    };
    let wrote = n
        .state
        .profiles
        .instances()
        .write(&alice, ERUPTOR, late, true, giants().caps());
    assert!(wrote.await.unwrap().is_none());

    n.place(sid, 1, ERUPTOR).await;
    let fresh = n.row(&alice, ERUPTOR).await.unwrap();
    assert_eq!((fresh.xp, fresh.gold, fresh.playtime_secs), (0, 0, 0));
    assert!(fresh.generation > edited.generation);
    assert_eq!(n.last_place().1.owner.unwrap().progress, fresh);

    // A figure never placed resets to what it is.
    assert_eq!(n.reset(sid, &alice, SPYRO).await, StatusCode::ACCEPTED);
}

/// The slot reset: a waiting figure fresh where it stands, its row made again
/// under a new generation and placed again so the TV hears it; refused for
/// the hero in a level, for another player's figure and for a slot that
/// doesn't hold the figure.
#[tokio::test(flavor = "multi_thread")]
async fn the_slot_reset_makes_a_waiting_figure_fresh_and_refuses_the_hero() {
    let n = Native::with_giants().await;
    let (sid, alice) = n.unlocked_session().await;
    let (bob_sid, _bob) = n.unlocked_session_as("Bob", "#2a9d8f").await;
    assert_eq!(
        n.edit(sid, &alice, SPYRO, 10, 500).await,
        StatusCode::ACCEPTED
    );
    n.place(sid, 1, ERUPTOR).await;
    n.place(sid, 2, SPYRO).await;
    let spyro = n.row(&alice, SPYRO).await.unwrap();
    assert_eq!((spyro.xp, spyro.gold), (33_000, 500));
    let eruptor = n.row(&alice, ERUPTOR).await.unwrap();
    let placed = n.driver.places.lock().unwrap().len();
    let mut events = n.state.events.subscribe();

    // Eruptor is the hero in the level.
    n.state.level_hero.set(Some(SlotIndex::new(0).unwrap()));
    assert_eq!(n.reset_slot(sid, 1, ERUPTOR).await, StatusCode::CONFLICT);
    assert_eq!(n.row(&alice, ERUPTOR).await.unwrap(), eruptor);
    // Not hers, not there.
    assert_eq!(n.reset_slot(bob_sid, 2, SPYRO).await, StatusCode::FORBIDDEN);
    assert_eq!(n.reset_slot(sid, 2, ERUPTOR).await, StatusCode::NOT_FOUND);
    assert_eq!(n.reset_slot(sid, 3, SPYRO).await, StatusCode::NOT_FOUND);
    assert_eq!(n.row(&alice, SPYRO).await.unwrap(), spyro);
    assert_eq!(n.driver.places.lock().unwrap().len(), placed);
    assert!(figure_updates(&mut events).is_empty());

    // Spyro waits on the portal: fresh where he stands.
    assert_eq!(n.reset_slot(sid, 2, SPYRO).await, StatusCode::ACCEPTED);
    let deadline = Instant::now() + Duration::from_secs(5);
    while n.driver.places.lock().unwrap().len() == placed {
        assert!(Instant::now() < deadline, "never placed again");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    n.wait_loaded(1, SPYRO).await;
    let fresh = n.row(&alice, SPYRO).await.unwrap();
    assert_eq!((fresh.xp, fresh.gold, fresh.playtime_secs), (0, 0, 0));
    assert!(fresh.generation > spyro.generation);
    let (slot, placement) = n.last_place();
    assert_eq!(slot, 1);
    assert_eq!(placement.figure_id.as_str(), SPYRO);
    let owner = placement.owner.unwrap();
    assert_eq!((owner.profile_id, owner.progress), (alice.clone(), fresh));
    assert_eq!(figure_updates(&mut events), vec![(SPYRO.to_string(), 1, 0)]);
    assert_eq!(n.row(&alice, ERUPTOR).await.unwrap(), eruptor);

    // With no level, the hero's slot resets too.
    n.state.level_hero.set(None);
    assert_eq!(n.reset_slot(sid, 1, ERUPTOR).await, StatusCode::ACCEPTED);
    n.wait_loaded(0, ERUPTOR).await;
    let reset = n.row(&alice, ERUPTOR).await.unwrap();
    assert!(reset.generation > eruptor.generation);

    // A locked phone can't.
    n.state.sessions.set_profile(sid, None).await;
    assert_eq!(n.reset_slot(sid, 1, ERUPTOR).await, StatusCode::FORBIDDEN);
}
