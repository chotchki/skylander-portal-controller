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
