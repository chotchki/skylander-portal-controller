//! Integration test for the profile + PIN flow (PLAN 3.9).
//!
//! Exercises the store + lockout together against an in-memory SQLite DB.
//! Deliberately *not* routed through HTTP — the HTTP plumbing is covered by
//! the e2e suite. This test pins down the policy: right-pin succeeds;
//! three wrong pins trigger a lockout; the 4th attempt inside the window
//! is rejected with a retry deadline.

use std::time::{Duration, Instant};

use skylander_server::profiles::{
    GHOST_TIMEOUT, LOCKOUT_DURATION, LockoutCheck, ProfileStore, RegistrationOutcome,
    SessionRegistry,
};

#[tokio::test]
async fn three_strikes_then_lockout_with_retry_after() {
    let store = ProfileStore::open_in_memory().await.expect("open");
    let id = store
        .create("Alice", "1234", "#ff00aa")
        .await
        .expect("create");

    // Right pin → allowed, verifies true.
    let t0 = Instant::now();
    assert_eq!(store.lockouts.check(&id, t0).await, LockoutCheck::Allowed);
    assert!(store.verify_pin(&id, "1234").await.unwrap());
    store.lockouts.record_success(&id).await;

    // Three wrong pins.
    for _ in 0..2 {
        assert_eq!(store.lockouts.check(&id, t0).await, LockoutCheck::Allowed);
        assert!(!store.verify_pin(&id, "0000").await.unwrap());
        assert!(!store.lockouts.record_failure(&id, t0).await);
    }
    // Third failure triggers the lockout.
    assert_eq!(store.lockouts.check(&id, t0).await, LockoutCheck::Allowed);
    assert!(!store.verify_pin(&id, "0000").await.unwrap());
    assert!(store.lockouts.record_failure(&id, t0).await);

    // Fourth attempt, even with the *correct* pin, is rejected by the
    // lockout gate.
    match store.lockouts.check(&id, t0).await {
        LockoutCheck::LockedOut { retry_after } => {
            assert!(retry_after > Duration::from_millis(100));
            assert!(retry_after <= LOCKOUT_DURATION);
        }
        other => panic!("expected lockout, got {other:?}"),
    }

    // After the freeze elapses, the correct pin works again.
    let after = t0 + LOCKOUT_DURATION + Duration::from_millis(1);
    assert_eq!(
        store.lockouts.check(&id, after).await,
        LockoutCheck::Allowed
    );
    assert!(store.verify_pin(&id, "1234").await.unwrap());
}

#[tokio::test]
async fn create_delete_roundtrip_and_max_profiles_enforced_externally() {
    // The store itself doesn't cap count (the HTTP handler does); verify
    // the count() helper is accurate so the handler can rely on it.
    let store = ProfileStore::open_in_memory().await.unwrap();
    for i in 0..4 {
        let pin = format!("{i:04}");
        store
            .create(&format!("P{i}"), &pin, "#112233")
            .await
            .unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 4);
    let rows = store.list().await.unwrap();
    for row in &rows {
        assert!(store.delete(&row.id).await.unwrap());
    }
    assert_eq!(store.count().await.unwrap(), 0);
}

/// Locks down the resume-modal "Start Fresh" path (PLAN 4.20.x):
/// save a layout → load returns it → clear → load returns None.
/// Pre-fix the modal only dismissed itself client-side, so the
/// server's saved JSON survived and re-fired on every subsequent
/// unlock. The clear step is the contract that broke that loop.
#[tokio::test]
async fn clear_portal_layout_drops_saved_layout() {
    let store = ProfileStore::open_in_memory().await.unwrap();
    let id = store.create("Alice", "1234", "#abcdef").await.unwrap();

    // Empty load before any save.
    assert_eq!(store.load_portal_layout(&id).await.unwrap(), None);

    // Save → load round-trips.
    let layout = r#"[{"Loaded":{"figure_id":"abc","display_name":"Eruptor"}}]"#;
    store.save_portal_layout(&id, layout).await.unwrap();
    assert_eq!(
        store.load_portal_layout(&id).await.unwrap().as_deref(),
        Some(layout),
    );

    // Clear nukes the JSON.
    store.clear_portal_layout(&id).await.unwrap();
    assert_eq!(store.load_portal_layout(&id).await.unwrap(), None);

    // Idempotent — clearing again on an already-cleared profile is fine.
    store.clear_portal_layout(&id).await.unwrap();
    assert_eq!(store.load_portal_layout(&id).await.unwrap(), None);

    // After clear, save still works (next time the user does a real
    // load on the portal we re-record the layout).
    store.save_portal_layout(&id, "[]").await.unwrap();
    assert_eq!(
        store.load_portal_layout(&id).await.unwrap().as_deref(),
        Some("[]"),
    );
}

/// Display-mode persistence (PLAN 4.20.x): unknown serial → None,
/// save a mode → get returns it, save again with different values →
/// the conflict-on-serial UPSERT overwrites cleanly.
#[tokio::test]
async fn display_mode_persistence_roundtrip_and_overwrite() {
    use skylander_server::display_mode::DisplayMode;

    let store = ProfileStore::open_in_memory().await.unwrap();

    // Cold cache — no mode for any serial yet.
    assert_eq!(store.get_display_mode("BLUS31442").await.unwrap(), None,);

    // Save once.
    let first = DisplayMode {
        width: 1920,
        height: 1080,
        refresh_hz: 60,
    };
    store.save_display_mode("BLUS31442", first).await.unwrap();
    assert_eq!(
        store.get_display_mode("BLUS31442").await.unwrap(),
        Some(first),
    );

    // Overwrite — the second save should completely replace the first
    // (this is what happens on a re-launch where RPCS3 picked a
    // different mode than we pre-set, e.g. native res differs).
    let second = DisplayMode {
        width: 3840,
        height: 2160,
        refresh_hz: 30,
    };
    store.save_display_mode("BLUS31442", second).await.unwrap();
    assert_eq!(
        store.get_display_mode("BLUS31442").await.unwrap(),
        Some(second),
    );

    // Different serial reads independently — modes are per-game.
    assert_eq!(store.get_display_mode("BLUS30906").await.unwrap(), None,);
}

// ---- Ghost-session lifecycle (PLAN 8.1) -----------------------------------

/// Ghost a live session and confirm:
///   - the entry remains in the registry (still counts against MAX_SESSIONS)
///   - the profile_id is returned so the WS exit path knows what cleanup
///     to defer
///   - the session reads back as a ghost via `is_ghost()`
#[tokio::test]
async fn ghost_keeps_session_in_registry_and_returns_profile() {
    let reg = SessionRegistry::default();
    let t0 = Instant::now();
    let sid = match reg.register_at(t0).await {
        RegistrationOutcome::Admitted(s) => s,
        other => panic!("expected Admitted, got {other:?}"),
    };
    reg.set_profile(sid, Some("alice".into())).await;

    let returned = reg.ghost(sid, t0).await;
    assert_eq!(returned.as_deref(), Some("alice"));

    // Ghosted session still occupies a slot.
    assert_eq!(reg.all_ids().await, vec![sid]);
    let s = reg.get(sid).await.expect("ghosted session readable");
    assert!(s.is_ghost(), "ghosted_at should be set");
    assert_eq!(s.profile_id.as_deref(), Some("alice"));
}

/// Ghosting an unknown session id is a no-op — handles the race where
/// a forced eviction removed the entry just before the WS exit path
/// reaches into the registry.
#[tokio::test]
async fn ghost_unknown_session_is_noop() {
    let reg = SessionRegistry::default();
    use skylander_server::profiles::SessionId;
    let returned = reg.ghost(SessionId(99_999), Instant::now()).await;
    assert_eq!(returned, None);
    assert!(reg.all_ids().await.is_empty());
}

/// Sweep removes only ghosts older than the timeout — live sessions
/// and recently-ghosted ones stay. Returns the (sid, profile_id) pairs
/// the caller needs to run `clear_slots_for_profile` on.
#[tokio::test]
async fn expire_ghosts_older_than_returns_only_stale_ghosts() {
    let reg = SessionRegistry::default();
    let t0 = Instant::now();

    // Live session (never ghosted).
    let live = match reg.register_at(t0).await {
        RegistrationOutcome::Admitted(s) => s,
        other => panic!("expected Admitted, got {other:?}"),
    };
    reg.set_profile(live, Some("live-alice".into())).await;

    // Recent ghost (within timeout).
    let recent = match reg.register_at(t0 + Duration::from_secs(1)).await {
        RegistrationOutcome::Admitted(s) => s,
        other => panic!("expected Admitted, got {other:?}"),
    };
    reg.set_profile(recent, Some("recent-bob".into())).await;
    reg.ghost(recent, t0 + Duration::from_secs(2)).await;

    // Sweep at GHOST_TIMEOUT - 1s after the recent ghost was set:
    // nothing should be evicted.
    let early = t0 + Duration::from_secs(2) + GHOST_TIMEOUT - Duration::from_secs(1);
    let removed = reg.expire_ghosts_older_than(GHOST_TIMEOUT, early).await;
    assert!(removed.is_empty());
    assert_eq!(reg.all_ids().await.len(), 2);

    // Sweep AFTER the timeout: only the ghost is evicted; the live
    // session stays.
    let late = t0 + Duration::from_secs(2) + GHOST_TIMEOUT + Duration::from_secs(1);
    let removed = reg.expire_ghosts_older_than(GHOST_TIMEOUT, late).await;
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].0, recent);
    assert_eq!(removed[0].1.as_deref(), Some("recent-bob"));

    // Live session intact post-sweep.
    let still_live = reg.all_ids().await;
    assert_eq!(still_live, vec![live]);
}

/// Ghosts count against the 2-session FIFO cap — a 3rd registration
/// still triggers forced eviction of the oldest, ghost or not. This
/// is what keeps the abandoned-PWA case from blocking real users.
#[tokio::test]
async fn ghost_counts_toward_max_sessions_and_can_be_force_evicted() {
    use skylander_server::profiles::MAX_SESSIONS;
    assert_eq!(MAX_SESSIONS, 2, "test assumes the 2-slot cap");

    let reg = SessionRegistry::default();
    let t0 = Instant::now();

    let s1 = match reg.register_at(t0).await {
        RegistrationOutcome::Admitted(s) => s,
        other => panic!("expected Admitted, got {other:?}"),
    };
    reg.set_profile(s1, Some("alice".into())).await;
    reg.ghost(s1, t0).await;

    let _s2 = match reg.register_at(t0 + Duration::from_secs(1)).await {
        RegistrationOutcome::Admitted(s) => s,
        other => panic!("expected Admitted (slot 2), got {other:?}"),
    };

    // 3rd registration: forced-evict cooldown is fresh (no prior
    // forced eviction), so it should evict the oldest (the ghost).
    let third_outcome = reg.register_at(t0 + Duration::from_secs(2)).await;
    match third_outcome {
        RegistrationOutcome::AdmittedByEvicting { evicted, .. } => {
            assert_eq!(evicted, s1, "ghost should be evicted as the oldest");
        }
        other => panic!("expected AdmittedByEvicting, got {other:?}"),
    }
}
