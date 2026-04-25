//! Integration test for the profile + PIN flow (PLAN 3.9).
//!
//! Exercises the store + lockout together against an in-memory SQLite DB.
//! Deliberately *not* routed through HTTP — the HTTP plumbing is covered by
//! the e2e suite. This test pins down the policy: right-pin succeeds;
//! three wrong pins trigger a lockout; the 4th attempt inside the window
//! is rejected with a retry deadline.

use std::time::{Duration, Instant};

use skylander_server::profiles::{LOCKOUT_DURATION, LockoutCheck, ProfileStore};

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
