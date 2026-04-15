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
use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
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
pub fn resolve_db_path() -> Result<PathBuf> {
    let dir = resolve_runtime_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create runtime dir {}", dir.display()))?;
    Ok(dir.join("db.sqlite"))
}

#[cfg(feature = "dev-tools")]
fn resolve_runtime_dir() -> Result<PathBuf> {
    Ok(PathBuf::from("dev-data"))
}

#[cfg(not(feature = "dev-tools"))]
fn resolve_runtime_dir() -> Result<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("APPDATA not set"))?;
    Ok(base.join("skylander-portal-controller"))
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

// ---- Session registry (single-session for 3.9; extends to [_;2] in 3.10) --

/// Opaque per-connection id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

#[derive(Debug, Clone)]
pub struct SessionState {
    pub profile_id: Option<String>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self { profile_id: None }
    }
}

#[derive(Default)]
pub struct SessionRegistry {
    sessions: RwLock<HashMap<SessionId, SessionState>>,
    next_id: std::sync::atomic::AtomicU64,
}

impl SessionRegistry {
    pub fn mint(&self) -> SessionId {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        SessionId(id)
    }

    pub async fn insert_default(&self, id: SessionId) {
        self.sessions.write().await.insert(id, SessionState::default());
    }

    pub async fn remove(&self, id: SessionId) {
        self.sessions.write().await.remove(&id);
    }

    #[allow(dead_code)] // kept for 3.10's per-session queries
    pub async fn get(&self, id: SessionId) -> Option<SessionState> {
        self.sessions.read().await.get(&id).cloned()
    }

    #[allow(dead_code)] // kept for 3.10's per-session mutations
    pub async fn set_profile(&self, id: SessionId, profile_id: Option<String>) {
        let mut map = self.sessions.write().await;
        map.entry(id).or_default().profile_id = profile_id;
    }

    /// Returns the single currently-unlocked profile, if any. For 3.9 there
    /// is at most one session, so this is simply "find one". Extends trivially
    /// to per-session lookup in 3.10.
    pub async fn any_unlocked_profile(&self) -> Option<String> {
        self.sessions
            .read()
            .await
            .values()
            .find_map(|s| s.profile_id.clone())
    }

    /// Clear the unlock on every session. For 3.9 we treat lock as a global
    /// event; 3.10 will key this per-session.
    pub async fn lock_all(&self) {
        let mut map = self.sessions.write().await;
        for s in map.values_mut() {
            s.profile_id = None;
        }
    }

    /// Unlock a profile on the "current" session. For 3.9 this means: if any
    /// session exists, use it; otherwise park on a fresh synthetic session.
    /// 3.10 replaces this with the real per-WS session id.
    pub async fn unlock_current(&self, profile_id: String) -> SessionId {
        let mut map = self.sessions.write().await;
        if let Some((sid, state)) = map.iter_mut().next() {
            state.profile_id = Some(profile_id);
            return *sid;
        }
        drop(map);
        let sid = self.mint();
        self.sessions
            .write()
            .await
            .insert(sid, SessionState { profile_id: Some(profile_id) });
        sid
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
    pub async fn create(
        &self,
        display_name: &str,
        pin: &str,
        color: &str,
    ) -> Result<String> {
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
            l.check("p", t0 + LOCKOUT_DURATION + Duration::from_millis(1)).await,
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
}
