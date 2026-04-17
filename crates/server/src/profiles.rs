//! Profile system: SQLite-backed profiles with argon2 PIN hashes, an
//! in-memory three-strikes lockout, and a per-connection session registry.
//!
//! This module is deliberately self-contained — HTTP handlers live in
//! `http.rs` and pull the pieces they need from [`ProfileStore`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use argon2::Argon2;
use argon2::password_hash::{
    PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Sqlite};
use tokio::sync::{Mutex, RwLock};

/// Maximum number of profiles. SPEC Q20 — the family-size cap.
pub const MAX_PROFILES: usize = 4;

/// Lockout policy. Three wrong PINs in a row → 5-second freeze for the
/// offending profile. Cleared on the next successful unlock.
pub const LOCKOUT_STRIKES: u32 = 3;
pub const LOCKOUT_DURATION: Duration = Duration::from_secs(5);

// ---- DB row + public view -------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProfileRow {
    pub id: String,
    pub display_name: String,
    pub pin_hash: String,
    pub color: String,
    #[allow(dead_code)] // surfaced via ORDER BY, not read directly
    pub created_at: String,
}

/// Public shape returned to the phone — never includes `pin_hash`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicProfile {
    pub id: String,
    pub display_name: String,
    pub color: String,
}

impl From<&ProfileRow> for PublicProfile {
    fn from(r: &ProfileRow) -> Self {
        PublicProfile {
            id: r.id.clone(),
            display_name: r.display_name.clone(),
            color: r.color.clone(),
        }
    }
}

// ---- Path resolution ------------------------------------------------------

/// Resolve the SQLite DB path. Dev builds put it under `./dev-data/`,
/// release under `%APPDATA%/skylander-portal-controller/`.
///
/// Re-exported from [`crate::paths::db_path`] so existing callers keep
/// working; the dev/release split lives in `paths.rs`.
pub fn resolve_db_path() -> Result<PathBuf> {
    crate::paths::db_path()
}

// ---- Lockout state machine ------------------------------------------------

#[derive(Debug, Default, Clone)]
struct LockoutEntry {
    strikes: u32,
    locked_until: Option<Instant>,
}

/// In-memory map of profile_id → lockout state. Not persisted — a server
/// restart clears strikes (acceptable; this is anti-sibling, not anti-adversary).
#[derive(Default)]
pub struct Lockouts {
    inner: Mutex<HashMap<String, LockoutEntry>>,
}

/// Outcome of a PIN attempt from the lockout's perspective.
#[derive(Debug, PartialEq, Eq)]
pub enum LockoutCheck {
    /// No active lockout — proceed with the verification.
    Allowed,
    /// Profile is frozen; `retry_after` is the remaining duration.
    LockedOut { retry_after: Duration },
}

impl Lockouts {
    /// Check lockout before verifying a PIN. `now` is the reference instant
    /// (injectable for tests; callers normally pass `Instant::now()`).
    pub async fn check(&self, profile_id: &str, now: Instant) -> LockoutCheck {
        let map = self.inner.lock().await;
        match map.get(profile_id).and_then(|e| e.locked_until) {
            Some(until) if until > now => LockoutCheck::LockedOut {
                retry_after: until - now,
            },
            _ => LockoutCheck::Allowed,
        }
    }

    /// Record a failed attempt. Returns true if this attempt triggered a
    /// fresh lockout.
    pub async fn record_failure(&self, profile_id: &str, now: Instant) -> bool {
        let mut map = self.inner.lock().await;
        let entry = map.entry(profile_id.to_string()).or_default();
        entry.strikes += 1;
        if entry.strikes >= LOCKOUT_STRIKES {
            entry.locked_until = Some(now + LOCKOUT_DURATION);
            entry.strikes = 0;
            true
        } else {
            false
        }
    }

    /// Record a successful unlock — clears strikes and any lockout.
    pub async fn record_success(&self, profile_id: &str) {
        let mut map = self.inner.lock().await;
        map.remove(profile_id);
    }
}

// ---- Session registry (2-slot FIFO with forced-eviction cooldown, PLAN 3.10) -

/// Maximum concurrent phone sessions. Matches the co-op player count per
/// SPEC Round 4. A 3rd connection evicts the oldest session (FIFO).
pub const MAX_SESSIONS: usize = 2;

/// Minimum time between forced evictions. Anti-ping-pong guard — SPEC Q31.
pub const FORCED_EVICT_COOLDOWN: Duration = Duration::from_secs(60);

/// Opaque per-connection id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

#[derive(Debug, Clone)]
pub struct SessionState {
    pub profile_id: Option<String>,
    pub created_at: Instant,
}

/// Outcome of registering a new WS connection via [`SessionRegistry::register`].
#[derive(Debug, Clone)]
pub enum RegistrationOutcome {
    /// Session slot was free; registered without touching anyone else.
    Admitted(SessionId),
    /// Both slots were full and we evicted the oldest session to make room.
    /// The caller must send `Event::TakenOver` to `evicted` and remove its
    /// WS task. Updates the registry's cooldown clock.
    AdmittedByEvicting {
        session: SessionId,
        evicted: SessionId,
    },
    /// Both slots were full and the forced-evict cooldown hasn't elapsed yet.
    /// Caller should close the WS with a `Retry-After`-style signal and not
    /// touch the existing sessions.
    RejectedByCooldown { retry_after: Duration },
}

pub struct SessionRegistry {
    sessions: RwLock<HashMap<SessionId, SessionState>>,
    last_forced_evict_at: RwLock<Option<Instant>>,
    next_id: std::sync::atomic::AtomicU64,
    /// When set, the next session registered adopts this profile_id as its
    /// unlock and the field clears itself. Used by the `test-hooks`
    /// `inject_profile` + `unlock_session` flow so tests can wire profile
    /// state before the phone's WS handshake. Also useful during
    /// eviction-then-reconnect flows to preserve intent across sessions
    /// (not currently exercised outside tests).
    pending_unlock: RwLock<Option<String>>,
}

impl Default for SessionRegistry {
    fn default() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            last_forced_evict_at: RwLock::new(None),
            next_id: std::sync::atomic::AtomicU64::new(1),
            pending_unlock: RwLock::new(None),
        }
    }
}

impl SessionRegistry {
    fn mint(&self) -> SessionId {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        SessionId(id)
    }

    /// Admit a new WS connection, enforcing the 2-session FIFO cap and the
    /// forced-eviction cooldown. Test-only callers can drive the clock via
    /// [`Self::register_at`].
    pub async fn register(&self) -> RegistrationOutcome {
        self.register_at(Instant::now()).await
    }

    /// Like [`Self::register`] but with a caller-supplied `now`. Used by
    /// tests to fast-forward the cooldown without real sleeping.
    pub async fn register_at(&self, now: Instant) -> RegistrationOutcome {
        let mut sessions = self.sessions.write().await;
        if sessions.len() < MAX_SESSIONS {
            let sid = self.mint();
            let pending = self.pending_unlock.write().await.take();
            sessions.insert(
                sid,
                SessionState {
                    profile_id: pending,
                    created_at: now,
                },
            );
            return RegistrationOutcome::Admitted(sid);
        }

        // Both slots full — forced-eviction path. Check cooldown first.
        if let Some(last) = *self.last_forced_evict_at.read().await
            && let Some(remaining) =
                FORCED_EVICT_COOLDOWN.checked_sub(now.saturating_duration_since(last))
            && !remaining.is_zero()
        {
            return RegistrationOutcome::RejectedByCooldown {
                retry_after: remaining,
            };
        }

        // Pick the oldest session to evict.
        let evicted = sessions
            .iter()
            .min_by_key(|(_, s)| s.created_at)
            .map(|(sid, _)| *sid)
            .expect("len >= MAX_SESSIONS > 0");
        sessions.remove(&evicted);
        let sid = self.mint();
        let pending = self.pending_unlock.write().await.take();
        sessions.insert(
            sid,
            SessionState {
                profile_id: pending,
                created_at: now,
            },
        );
        drop(sessions);
        *self.last_forced_evict_at.write().await = Some(now);
        RegistrationOutcome::AdmittedByEvicting {
            session: sid,
            evicted,
        }
    }

    /// Polite removal — WS connection closed on its own. Does NOT touch the
    /// cooldown clock; the freed seat can be filled immediately.
    pub async fn remove(&self, id: SessionId) {
        self.sessions.write().await.remove(&id);
    }

    pub async fn get(&self, id: SessionId) -> Option<SessionState> {
        self.sessions.read().await.get(&id).cloned()
    }

    /// All currently-registered session ids. Useful for fan-out decisions.
    pub async fn all_ids(&self) -> Vec<SessionId> {
        self.sessions.read().await.keys().copied().collect()
    }

    pub async fn set_profile(&self, id: SessionId, profile_id: Option<String>) {
        let mut map = self.sessions.write().await;
        if let Some(s) = map.get_mut(&id) {
            s.profile_id = profile_id;
        }
    }

    pub async fn profile_of(&self, id: SessionId) -> Option<String> {
        self.sessions
            .read()
            .await
            .get(&id)
            .and_then(|s| s.profile_id.clone())
    }

    /// Pre-seed the unlock that the *next* registered session will adopt.
    /// Intended for `test-hooks` flows; production code should route REST
    /// unlocks to a specific `SessionId`.
    pub async fn set_pending_unlock(&self, profile_id: Option<String>) {
        *self.pending_unlock.write().await = profile_id;
    }

    /// Clear the forced-eviction cooldown clock. Used by the `test-hooks`
    /// e2e suite to validate cooldown behaviour without real 60-second
    /// sleeps. Production code never calls this.
    pub async fn clear_forced_evict_cooldown(&self) {
        *self.last_forced_evict_at.write().await = None;
    }
}

// ---- Store: DB + argon2 + lockout ----------------------------------------

/// Thin wrapper over the sqlite pool with the PIN hashing helpers and all
/// profile CRUD. Construct once at startup via [`ProfileStore::open`].
#[derive(Clone)]
pub struct ProfileStore {
    pub pool: Pool<Sqlite>,
    pub lockouts: Arc<Lockouts>,
}

impl ProfileStore {
    pub async fn open(db_path: &Path) -> Result<Self> {
        let opts = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .with_context(|| format!("open sqlite at {}", db_path.display()))?;

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .context("run migrations")?;

        Ok(Self {
            pool,
            lockouts: Arc::new(Lockouts::default()),
        })
    }

    /// In-memory pool for tests. Gated loosely (exposed for the
    /// `tests/profiles.rs` integration test) — no data-loss risk since
    /// callers must explicitly opt in.
    pub async fn open_in_memory() -> Result<Self> {
        let opts = SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self {
            pool,
            lockouts: Arc::new(Lockouts::default()),
        })
    }

    pub async fn list(&self) -> Result<Vec<ProfileRow>> {
        let rows = sqlx::query_as::<_, ProfileRow>(
            "SELECT id, display_name, pin_hash, color, created_at FROM profiles ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn count(&self) -> Result<i64> {
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM profiles")
            .fetch_one(&self.pool)
            .await?;
        Ok(n)
    }

    /// Return `(figure_id, last_used_at_rfc3339)` for every row the given
    /// profile owns. Empty map when the profile has no usage yet.
    pub async fn fetch_usage(&self, profile_id: &str) -> Result<HashMap<String, String>> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT figure_id, last_used_at FROM figure_usage WHERE profile_id = ?1",
        )
        .bind(profile_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().collect())
    }

    /// Update `figure_usage.last_used_at` for this (profile_id, figure_id)
    /// pair. Creates the row on first use. Called from `load_slot` after a
    /// successful working-copy resolve (PLAN 3.11.2).
    pub async fn record_figure_usage(&self, profile_id: &str, figure_id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO figure_usage (profile_id, figure_id, last_used_at) \
             VALUES (?1, ?2, ?3) \
             ON CONFLICT (profile_id, figure_id) \
             DO UPDATE SET last_used_at = excluded.last_used_at",
        )
        .bind(profile_id)
        .bind(figure_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Read + write `sessions.last_portal_layout_json` for a profile. The
    /// layout is an opaque JSON blob (the 8-slot array, serialised via
    /// `serde_json`) — shape is enforced by the caller, not the DB.
    pub async fn save_portal_layout(&self, profile_id: &str, layout_json: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO sessions (profile_id, last_portal_layout_json, updated_at) \
             VALUES (?1, ?2, ?3) \
             ON CONFLICT (profile_id) \
             DO UPDATE SET last_portal_layout_json = excluded.last_portal_layout_json, \
                           updated_at = excluded.updated_at",
        )
        .bind(profile_id)
        .bind(layout_json)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn load_portal_layout(&self, profile_id: &str) -> Result<Option<String>> {
        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT last_portal_layout_json FROM sessions WHERE profile_id = ?1")
                .bind(profile_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.and_then(|(json,)| json))
    }

    pub async fn get(&self, id: &str) -> Result<Option<ProfileRow>> {
        let row = sqlx::query_as::<_, ProfileRow>(
            "SELECT id, display_name, pin_hash, color, created_at FROM profiles WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Create a profile. Returns the new id. Caller must have already
    /// enforced `count() < MAX_PROFILES`.
    pub async fn create(&self, display_name: &str, pin: &str, color: &str) -> Result<String> {
        validate_pin(pin)?;
        validate_name(display_name)?;
        validate_color(color)?;
        let id = ulid::Ulid::new().to_string();
        let hash = hash_pin(pin)?;
        let created_at = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO profiles (id, display_name, pin_hash, color, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(&id)
        .bind(display_name)
        .bind(&hash)
        .bind(color)
        .bind(&created_at)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let res = sqlx::query("DELETE FROM profiles WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn reset_pin(&self, id: &str, new_pin: &str) -> Result<()> {
        validate_pin(new_pin)?;
        let hash = hash_pin(new_pin)?;
        sqlx::query("UPDATE profiles SET pin_hash = ?1 WHERE id = ?2")
            .bind(&hash)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Verify a plaintext PIN against the stored hash. Does **not** consult
    /// or update the lockout — callers combine these so tests can drive the
    /// lockout with injected `Instant`s.
    pub async fn verify_pin(&self, id: &str, pin: &str) -> Result<bool> {
        let row = match self.get(id).await? {
            Some(r) => r,
            None => return Ok(false),
        };
        Ok(verify_hash(pin, &row.pin_hash))
    }
}

// ---- Argon2 helpers -------------------------------------------------------

pub fn hash_pin(pin: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(pin.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("argon2 hash: {e}"))?
        .to_string();
    Ok(hash)
}

pub fn verify_hash(pin: &str, hash: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(p) => p,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(pin.as_bytes(), &parsed)
        .is_ok()
}

// ---- Validators -----------------------------------------------------------

fn validate_pin(pin: &str) -> Result<()> {
    if pin.len() != 4 || !pin.chars().all(|c| c.is_ascii_digit()) {
        anyhow::bail!("PIN must be exactly 4 digits");
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 32 {
        anyhow::bail!("name must be 1-32 chars");
    }
    Ok(())
}

fn validate_color(color: &str) -> Result<()> {
    if !(color.starts_with('#')
        && (color.len() == 7 || color.len() == 4)
        && color[1..].chars().all(|c| c.is_ascii_hexdigit()))
    {
        anyhow::bail!("color must be a hex string like #aabbcc");
    }
    Ok(())
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argon2_roundtrip() {
        let h = hash_pin("1234").unwrap();
        assert!(verify_hash("1234", &h));
        assert!(!verify_hash("1235", &h));
        assert!(!verify_hash("", &h));
    }

    #[test]
    fn argon2_distinct_hashes_same_pin() {
        // Two calls produce different hashes (salted) but both verify.
        let a = hash_pin("0000").unwrap();
        let b = hash_pin("0000").unwrap();
        assert_ne!(a, b);
        assert!(verify_hash("0000", &a));
        assert!(verify_hash("0000", &b));
    }

    #[tokio::test]
    async fn lockout_three_strikes_then_freeze() {
        let l = Lockouts::default();
        let t0 = Instant::now();

        assert_eq!(l.check("p", t0).await, LockoutCheck::Allowed);
        assert!(!l.record_failure("p", t0).await); // strike 1
        assert!(!l.record_failure("p", t0).await); // strike 2
        assert!(l.record_failure("p", t0).await); // strike 3 → lockout

        match l.check("p", t0).await {
            LockoutCheck::LockedOut { retry_after } => {
                assert!(retry_after <= LOCKOUT_DURATION);
                assert!(retry_after > Duration::from_millis(100));
            }
            other => panic!("expected lockout, got {other:?}"),
        }

        // Still locked partway through the window.
        assert!(matches!(
            l.check("p", t0 + Duration::from_secs(2)).await,
            LockoutCheck::LockedOut { .. }
        ));
        // Cleared once the window elapses.
        assert_eq!(
            l.check("p", t0 + LOCKOUT_DURATION + Duration::from_millis(1))
                .await,
            LockoutCheck::Allowed
        );
    }

    #[tokio::test]
    async fn lockout_success_clears_strikes() {
        let l = Lockouts::default();
        let t0 = Instant::now();
        l.record_failure("p", t0).await;
        l.record_failure("p", t0).await;
        l.record_success("p").await;
        // A third failure after success starts fresh — should NOT lock out.
        assert!(!l.record_failure("p", t0).await);
    }

    #[tokio::test]
    async fn crud_and_pin_verify() {
        let s = ProfileStore::open_in_memory().await.unwrap();
        assert_eq!(s.count().await.unwrap(), 0);
        let id = s.create("Alice", "1234", "#ff00aa").await.unwrap();
        assert_eq!(s.count().await.unwrap(), 1);
        assert!(s.verify_pin(&id, "1234").await.unwrap());
        assert!(!s.verify_pin(&id, "0000").await.unwrap());

        s.reset_pin(&id, "9999").await.unwrap();
        assert!(!s.verify_pin(&id, "1234").await.unwrap());
        assert!(s.verify_pin(&id, "9999").await.unwrap());

        assert!(s.delete(&id).await.unwrap());
        assert_eq!(s.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn rejects_bad_pin_and_color() {
        let s = ProfileStore::open_in_memory().await.unwrap();
        assert!(s.create("A", "12", "#ffffff").await.is_err());
        assert!(s.create("A", "abcd", "#ffffff").await.is_err());
        assert!(s.create("A", "1234", "red").await.is_err());
        assert!(s.create("", "1234", "#ffffff").await.is_err());
    }

    // ---- SessionRegistry (3.10a) ----

    fn sid(outcome: &RegistrationOutcome) -> SessionId {
        match outcome {
            RegistrationOutcome::Admitted(s) => *s,
            RegistrationOutcome::AdmittedByEvicting { session, .. } => *session,
            RegistrationOutcome::RejectedByCooldown { .. } => {
                panic!("expected admission, got RejectedByCooldown")
            }
        }
    }

    #[tokio::test]
    async fn registry_admits_up_to_two_without_eviction() {
        let reg = SessionRegistry::default();
        let a = sid(&reg.register().await);
        let b = sid(&reg.register().await);
        assert_ne!(a, b);
        let ids = reg.all_ids().await;
        assert_eq!(ids.len(), 2);
    }

    #[tokio::test]
    async fn registry_third_connection_evicts_oldest() {
        let reg = SessionRegistry::default();
        let base = Instant::now();
        let a = sid(&reg.register_at(base).await);
        let b = sid(&reg.register_at(base + Duration::from_secs(1)).await);

        let outcome = reg
            .register_at(base + Duration::from_secs(FORCED_EVICT_COOLDOWN.as_secs() + 2))
            .await;
        match outcome {
            RegistrationOutcome::AdmittedByEvicting { session, evicted } => {
                assert_eq!(evicted, a, "oldest (a) should be evicted, not b");
                assert_ne!(session, a);
                assert_ne!(session, b);
                let ids = reg.all_ids().await;
                assert!(ids.contains(&b));
                assert!(ids.contains(&session));
                assert!(!ids.contains(&a));
            }
            other => panic!("expected AdmittedByEvicting, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn registry_forced_eviction_triggers_cooldown() {
        let reg = SessionRegistry::default();
        let base = Instant::now();
        let _a = sid(&reg.register_at(base).await);
        let _b = sid(&reg.register_at(base + Duration::from_secs(1)).await);
        // Third — evicts a.
        let _c = sid(&reg.register_at(base + Duration::from_secs(2)).await);

        // Fourth immediately after: still 2 seats, and within cooldown → reject.
        let outcome = reg.register_at(base + Duration::from_secs(3)).await;
        match outcome {
            RegistrationOutcome::RejectedByCooldown { retry_after } => {
                assert!(retry_after.as_secs() > 50 && retry_after.as_secs() <= 60);
            }
            other => panic!("expected RejectedByCooldown, got {other:?}"),
        }

        // After the cooldown elapses, a fresh forced evict succeeds.
        let outcome = reg
            .register_at(base + Duration::from_secs(2 + FORCED_EVICT_COOLDOWN.as_secs() + 1))
            .await;
        assert!(matches!(
            outcome,
            RegistrationOutcome::AdmittedByEvicting { .. }
        ));
    }

    #[tokio::test]
    async fn registry_polite_disconnect_frees_seat_without_cooldown() {
        let reg = SessionRegistry::default();
        let base = Instant::now();
        let a = sid(&reg.register_at(base).await);
        let _b = sid(&reg.register_at(base + Duration::from_secs(1)).await);
        reg.remove(a).await;

        let outcome = reg.register_at(base + Duration::from_secs(2)).await;
        assert!(matches!(outcome, RegistrationOutcome::Admitted(_)));
    }

    #[tokio::test]
    async fn registry_set_profile_is_scoped_to_one_session() {
        let reg = SessionRegistry::default();
        let a = sid(&reg.register().await);
        let b = sid(&reg.register().await);
        reg.set_profile(a, Some("alpha".into())).await;
        assert_eq!(reg.profile_of(a).await.as_deref(), Some("alpha"));
        assert_eq!(reg.profile_of(b).await, None);
        reg.set_profile(a, None).await;
        assert_eq!(reg.profile_of(a).await, None);
    }

    #[tokio::test]
    async fn registry_pending_unlock_applies_to_next_session_only() {
        let reg = SessionRegistry::default();
        reg.set_pending_unlock(Some("alpha".into())).await;
        let a = sid(&reg.register().await);
        let b = sid(&reg.register().await);
        assert_eq!(reg.profile_of(a).await.as_deref(), Some("alpha"));
        assert_eq!(reg.profile_of(b).await, None);
    }
}
