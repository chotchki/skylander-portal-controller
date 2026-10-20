//! guacamole's figure store (openspec AH.1, design decision 8): one row per
//! profile per figure in `figure_instances`, which only guacamole's migration
//! set has. Every query here runs on guacamole's arm alone (no `FigureFiles`
//! hook), so the product's database never sees one.
//!
//! Each row carries a generation. An edit or a reset moves it on, and values the
//! TV sends under an older one are dropped, so nothing earned before an edit or
//! a reset lands on top of it. Every generation comes from one counter
//! (`figure_generation`), so a row made fresh after a delete never reuses one.

use anyhow::{Context, Result};
use chrono::Utc;
use skylander_rpcs3_control::FigureProgress;
use sqlx::{Pool, Sqlite, SqliteConnection};

use crate::profiles::ProfileStore;

/// The most experience and gold a figure holds. They're the game's, read off
/// the disc; the store clamps every write at what its caller passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub xp: u32,
    pub gold: u32,
}

impl Caps {
    /// No clamp but the columns' range, for a caller with no game table yet.
    pub const NONE: Caps = Caps {
        xp: u32::MAX,
        gold: u32::MAX,
    };

    pub fn clamp(self, xp: u32, gold: u32) -> (u32, u32) {
        (xp.min(self.xp), gold.min(self.gold))
    }
}

/// A profile's progress for a figure, keyed by both, with the generation the
/// TV's values are checked against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FigureInstance {
    pub profile_id: String,
    pub figure_id: String,
    pub progress: FigureProgress,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(sqlx::FromRow)]
struct Row {
    profile_id: String,
    figure_id: String,
    xp: i64,
    gold: i64,
    playtime_secs: i64,
    generation: i64,
    created_at: String,
    updated_at: String,
}

/// A column back to its value; nothing out of `u32` is ever written.
fn count(v: i64) -> u32 {
    u32::try_from(v.max(0)).unwrap_or(u32::MAX)
}

impl From<Row> for FigureInstance {
    fn from(r: Row) -> Self {
        FigureInstance {
            profile_id: r.profile_id,
            figure_id: r.figure_id,
            progress: FigureProgress {
                xp: count(r.xp),
                gold: count(r.gold),
                playtime_secs: count(r.playtime_secs),
                generation: r.generation,
            },
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

const COLUMNS: &str =
    "profile_id, figure_id, xp, gold, playtime_secs, generation, created_at, updated_at";

impl ProfileStore {
    /// guacamole's figure rows. Only guacamole's migration set has the table.
    pub fn instances(&self) -> Instances<'_> {
        Instances { pool: &self.pool }
    }
}

/// The figure store's queries, over the profile store's pool.
#[derive(Clone, Copy)]
pub struct Instances<'a> {
    pool: &'a Pool<Sqlite>,
}

impl Instances<'_> {
    /// The profile's row for the figure, made fresh (no experience, gold or
    /// playtime, a new generation) when it has none. An existing row comes
    /// back untouched.
    pub async fn fresh(self, profile_id: &str, figure_id: &str) -> Result<FigureInstance> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(row) = select(&mut tx, profile_id, figure_id).await? {
            tx.commit().await?;
            return Ok(row);
        }
        let generation = next_generation(&mut tx).await?;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO figure_instances \
             (profile_id, figure_id, xp, gold, playtime_secs, generation, created_at, updated_at) \
             VALUES (?1, ?2, 0, 0, 0, ?3, ?4, ?4)",
        )
        .bind(profile_id)
        .bind(figure_id)
        .bind(generation)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .with_context(|| format!("make {profile_id}'s {figure_id} fresh"))?;
        let row = select(&mut tx, profile_id, figure_id)
            .await?
            .context("the row just made")?;
        tx.commit().await?;
        Ok(row)
    }

    pub async fn read(self, profile_id: &str, figure_id: &str) -> Result<Option<FigureInstance>> {
        let row = sqlx::query_as::<_, Row>(&format!(
            "SELECT {COLUMNS} FROM figure_instances WHERE profile_id = ?1 AND figure_id = ?2"
        ))
        .bind(profile_id)
        .bind(figure_id)
        .fetch_optional(self.pool)
        .await?;
        Ok(row.map(Into::into))
    }

    /// Values the TV sent, written as they are (absolute, clamped at `caps`)
    /// when their generation is no older than the row's; playtime only when
    /// `playtime` says so. `None` when the row is gone or has moved on, so
    /// nothing was written.
    pub async fn write(
        self,
        profile_id: &str,
        figure_id: &str,
        sent: FigureProgress,
        playtime: bool,
        caps: Caps,
    ) -> Result<Option<FigureInstance>> {
        let (xp, gold) = caps.clamp(sent.xp, sent.gold);
        let playtime_secs = playtime.then_some(i64::from(sent.playtime_secs));
        let row = sqlx::query_as::<_, Row>(&format!(
            "UPDATE figure_instances \
             SET xp = ?1, gold = ?2, playtime_secs = COALESCE(?3, playtime_secs), updated_at = ?4 \
             WHERE profile_id = ?5 AND figure_id = ?6 AND generation <= ?7 \
             RETURNING {COLUMNS}"
        ))
        .bind(i64::from(xp))
        .bind(i64::from(gold))
        .bind(playtime_secs)
        .bind(Utc::now().to_rfc3339())
        .bind(profile_id)
        .bind(figure_id)
        .bind(sent.generation)
        .fetch_optional(self.pool)
        .await?;
        Ok(row.map(Into::into))
    }

    /// The phone's edit: experience and gold set (clamped at `caps`),
    /// playtime kept, under a new generation so nothing sent under the old
    /// one lands on top. Makes the row when the profile has none.
    pub async fn set(
        self,
        profile_id: &str,
        figure_id: &str,
        xp: u32,
        gold: u32,
        caps: Caps,
    ) -> Result<FigureInstance> {
        let (xp, gold) = caps.clamp(xp, gold);
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let generation = next_generation(&mut tx).await?;
        let now = Utc::now().to_rfc3339();
        let row = sqlx::query_as::<_, Row>(&format!(
            "INSERT INTO figure_instances \
             (profile_id, figure_id, xp, gold, playtime_secs, generation, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?6) \
             ON CONFLICT (profile_id, figure_id) DO UPDATE SET \
                xp = excluded.xp, gold = excluded.gold, \
                generation = excluded.generation, updated_at = excluded.updated_at \
             RETURNING {COLUMNS}"
        ))
        .bind(profile_id)
        .bind(figure_id)
        .bind(i64::from(xp))
        .bind(i64::from(gold))
        .bind(generation)
        .bind(&now)
        .fetch_one(&mut *tx)
        .await
        .with_context(|| format!("set {profile_id}'s {figure_id}"))?;
        tx.commit().await?;
        Ok(row.into())
    }

    /// Drops the row, so the profile's next placement of the figure is
    /// fresh. False when there was none.
    pub async fn delete(self, profile_id: &str, figure_id: &str) -> Result<bool> {
        let res =
            sqlx::query("DELETE FROM figure_instances WHERE profile_id = ?1 AND figure_id = ?2")
                .bind(profile_id)
                .bind(figure_id)
                .execute(self.pool)
                .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Moves the row to a new generation, values untouched; `None` when there
    /// is no row.
    pub async fn bump_generation(self, profile_id: &str, figure_id: &str) -> Result<Option<i64>> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if select(&mut tx, profile_id, figure_id).await?.is_none() {
            return Ok(None);
        }
        let generation = next_generation(&mut tx).await?;
        sqlx::query(
            "UPDATE figure_instances SET generation = ?1, updated_at = ?2 \
             WHERE profile_id = ?3 AND figure_id = ?4",
        )
        .bind(generation)
        .bind(Utc::now().to_rfc3339())
        .bind(profile_id)
        .bind(figure_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(generation))
    }
}

async fn select(
    conn: &mut SqliteConnection,
    profile_id: &str,
    figure_id: &str,
) -> Result<Option<FigureInstance>> {
    let row = sqlx::query_as::<_, Row>(&format!(
        "SELECT {COLUMNS} FROM figure_instances WHERE profile_id = ?1 AND figure_id = ?2"
    ))
    .bind(profile_id)
    .bind(figure_id)
    .fetch_optional(conn)
    .await?;
    Ok(row.map(Into::into))
}

/// The counter's next value, inside the caller's transaction.
async fn next_generation(conn: &mut SqliteConnection) -> Result<i64> {
    sqlx::query_scalar("UPDATE figure_generation SET last = last + 1 WHERE id = 1 RETURNING last")
        .fetch_one(conn)
        .await
        .context("take the next figure generation")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ERUPTOR: &str = "000009-0000";
    const TREE_REX: &str = "000070-0000";
    /// Giants' caps, as AH.2 reads them off the disc.
    const GIANTS: Caps = Caps {
        xp: 96_500,
        gold: 65_000,
    };

    async fn store() -> ProfileStore {
        ProfileStore::open_in_memory(&crate::setup::MIGRATOR)
            .await
            .unwrap()
    }

    async fn profile(s: &ProfileStore, name: &str) -> String {
        s.create(name, "1234", "#e63946").await.unwrap()
    }

    fn earned(row: &FigureInstance, xp: u32, gold: u32, playtime_secs: u32) -> FigureProgress {
        FigureProgress {
            xp,
            gold,
            playtime_secs,
            ..row.progress
        }
    }

    #[tokio::test]
    async fn fresh_makes_an_empty_row_and_leaves_an_existing_one_alone() {
        let s = store().await;
        let alice = profile(&s, "Alice").await;
        let first = s.instances().fresh(&alice, ERUPTOR).await.unwrap();
        assert_eq!((first.progress.xp, first.progress.gold), (0, 0));
        assert_eq!(first.progress.playtime_secs, 0);
        assert!(first.progress.generation > 0);

        let p = earned(&first, 16, 3, 61);
        s.instances()
            .write(&alice, ERUPTOR, p, true, GIANTS)
            .await
            .unwrap()
            .unwrap();
        let second = s.instances().fresh(&alice, ERUPTOR).await.unwrap();
        assert_eq!(second.progress, p, "a second fresh touched the row");
    }

    #[tokio::test]
    async fn one_figure_on_two_profiles_is_two_rows() {
        let s = store().await;
        let (alice, bob) = (profile(&s, "Alice").await, profile(&s, "Bob").await);
        let bobs = s.instances().fresh(&bob, ERUPTOR).await.unwrap();
        s.instances()
            .set(&bob, ERUPTOR, 0, 500, GIANTS)
            .await
            .unwrap();
        let alices = s.instances().fresh(&alice, ERUPTOR).await.unwrap();
        assert_eq!((alices.progress.xp, alices.progress.gold), (0, 0));
        assert_ne!(alices.progress.generation, bobs.progress.generation);
        let bobs = s.instances().read(&bob, ERUPTOR).await.unwrap().unwrap();
        assert_eq!(bobs.progress.gold, 500);
    }

    #[tokio::test]
    async fn the_callers_caps_clamp_writes_and_edits() {
        let s = store().await;
        let alice = profile(&s, "Alice").await;
        let row = s.instances().fresh(&alice, ERUPTOR).await.unwrap();
        let over = earned(&row, 100_000, 70_000, 5);
        let after = s
            .instances()
            .write(&alice, ERUPTOR, over, true, GIANTS)
            .await
            .unwrap()
            .unwrap();
        assert_eq!((after.progress.xp, after.progress.gold), (96_500, 65_000));
        let at_caps = earned(&row, 96_500, 65_000, 5);
        assert_eq!(after.progress, at_caps);

        let set = s
            .instances()
            .set(&alice, ERUPTOR, 200_000, 99_999, GIANTS)
            .await
            .unwrap();
        assert_eq!((set.progress.xp, set.progress.gold), (96_500, 65_000));
        let unclamped = s
            .instances()
            .set(&alice, ERUPTOR, 200_000, 99_999, Caps::NONE)
            .await
            .unwrap();
        assert_eq!(
            (unclamped.progress.xp, unclamped.progress.gold),
            (200_000, 99_999)
        );
    }

    #[tokio::test]
    async fn deleting_a_profile_takes_its_rows() {
        let s = store().await;
        let (alice, bob) = (profile(&s, "Alice").await, profile(&s, "Bob").await);
        for figure in [ERUPTOR, TREE_REX] {
            s.instances().fresh(&alice, figure).await.unwrap();
        }
        s.instances().fresh(&bob, ERUPTOR).await.unwrap();
        assert!(s.delete(&alice).await.unwrap());
        for figure in [ERUPTOR, TREE_REX] {
            assert_eq!(s.instances().read(&alice, figure).await.unwrap(), None);
        }
        assert!(s.instances().read(&bob, ERUPTOR).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn values_under_an_older_generation_are_dropped_after_an_edit_or_a_reset() {
        let s = store().await;
        let alice = profile(&s, "Alice").await;
        let row = s.instances().fresh(&alice, ERUPTOR).await.unwrap();
        let stale = earned(&row, 4, 1, 30);

        let edited = s
            .instances()
            .set(&alice, ERUPTOR, 33_000, 50, GIANTS)
            .await
            .unwrap();
        assert!(edited.progress.generation > row.progress.generation);
        let w = s.instances().write(&alice, ERUPTOR, stale, true, GIANTS);
        assert_eq!(w.await.unwrap(), None);
        let now = s.instances().read(&alice, ERUPTOR).await.unwrap().unwrap();
        assert_eq!((now.progress.xp, now.progress.gold), (33_000, 50));

        // A reset: the row goes, and the one made at the next placement is
        // newer than anything sent before, so the stale values stay out.
        assert!(s.instances().delete(&alice, ERUPTOR).await.unwrap());
        let w = s.instances().write(&alice, ERUPTOR, stale, true, GIANTS);
        assert_eq!(w.await.unwrap(), None, "a write made a row");
        assert_eq!(s.instances().read(&alice, ERUPTOR).await.unwrap(), None);
        let again = s.instances().fresh(&alice, ERUPTOR).await.unwrap();
        assert!(again.progress.generation > edited.progress.generation);
        let edited_stale = FigureProgress {
            generation: edited.progress.generation,
            ..stale
        };
        let w = s
            .instances()
            .write(&alice, ERUPTOR, edited_stale, true, GIANTS);
        assert_eq!(w.await.unwrap(), None);
        let fresh = s.instances().read(&alice, ERUPTOR).await.unwrap().unwrap();
        assert_eq!((fresh.progress.xp, fresh.progress.gold), (0, 0));
    }

    #[tokio::test]
    async fn a_write_keeps_playtime_unless_told_and_bump_moves_only_the_generation() {
        let s = store().await;
        let alice = profile(&s, "Alice").await;
        let row = s.instances().fresh(&alice, ERUPTOR).await.unwrap();
        let after = s
            .instances()
            .write(&alice, ERUPTOR, earned(&row, 4, 0, 59), false, GIANTS)
            .await
            .unwrap()
            .unwrap();
        assert_eq!((after.progress.xp, after.progress.playtime_secs), (4, 0));

        let bumped = s.instances().bump_generation(&alice, ERUPTOR).await;
        let bumped = bumped.unwrap().unwrap();
        assert!(bumped > row.progress.generation);
        let now = s.instances().read(&alice, ERUPTOR).await.unwrap().unwrap();
        assert_eq!(
            now.progress,
            FigureProgress {
                generation: bumped,
                ..after.progress
            }
        );
        let none = s.instances().bump_generation(&alice, TREE_REX).await;
        assert_eq!(none.unwrap(), None);
        assert_eq!(s.instances().read(&alice, TREE_REX).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_never_placed_figures_first_edit_makes_its_row() {
        let s = store().await;
        let alice = profile(&s, "Alice").await;
        let set = s
            .instances()
            .set(&alice, TREE_REX, 33_000, 1_234, GIANTS)
            .await
            .unwrap();
        assert_eq!(
            (
                set.progress.xp,
                set.progress.gold,
                set.progress.playtime_secs
            ),
            (33_000, 1_234, 0)
        );
        // A later placement finds it.
        let placed = s.instances().fresh(&alice, TREE_REX).await.unwrap();
        assert_eq!(placed.progress, set.progress);
    }

    #[tokio::test]
    async fn the_database_reopened_keeps_the_row() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db.sqlite");
        let (alice, kept) = {
            let s = ProfileStore::open(&db, &crate::setup::MIGRATOR)
                .await
                .unwrap();
            let alice = profile(&s, "Alice").await;
            let row = s.instances().fresh(&alice, ERUPTOR).await.unwrap();
            let kept = s
                .instances()
                .write(&alice, ERUPTOR, earned(&row, 16, 3, 120), true, GIANTS)
                .await
                .unwrap()
                .unwrap();
            s.pool.close().await;
            (alice, kept)
        };
        let s = ProfileStore::open(&db, &crate::setup::MIGRATOR)
            .await
            .unwrap();
        let row = s.instances().read(&alice, ERUPTOR).await.unwrap().unwrap();
        assert_eq!(row, kept);
        // And the counter carried on: nothing reuses the row's generation.
        let other = s.instances().fresh(&alice, TREE_REX).await.unwrap();
        assert!(other.progress.generation > kept.progress.generation);
    }
}
