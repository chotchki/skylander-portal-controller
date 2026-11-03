//! guacamole's story saves (openspec AU.3, design decision 4): one save per
//! profile per game, the values the original's save scripts keep named as
//! they name them, in `story_saves`, which only guacamole's migration set
//! has. Every query here runs on guacamole's arm alone.
//!
//! A row carries the save's layout: what the build that wrote it kept and how
//! (guacamole's own string, read nowhere here but for equality). A build
//! reads and writes only the row of its own layout. A row of another layout
//! is kept, named in the log and ignored, and the profile plays on as a
//! fresh one; what it saves then goes in a row of its own beside the old
//! one, never over it, so a build that reads the old row can take it back.
//! No query here deletes a save: one goes only with its profile.
//!
//! The host (the profile whose phone started the game, [`GameHost`]) is the
//! only one whose save the TV writes; the TV's keeper is [`StorySink`].

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::profiles::ProfileStore;

/// What a story save holds: each of the save scripts' values by the name
/// they give it (`level flags`, `flags - global`, `header - slot level ids`),
/// its fields in order, and the adventures in the order the per-adventure
/// values index them (`level flags`' field 0 is the first one's).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveValues {
    pub params: BTreeMap<String, Vec<i64>>,
    pub adventures: Vec<String>,
}

/// A profile's save for a game, with the layout the build that wrote it
/// kept it by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorySave {
    pub profile_id: String,
    pub game: String,
    pub layout: String,
    /// `None` for a row whose data doesn't read as [`SaveValues`]: kept,
    /// never written over.
    pub values: Option<SaveValues>,
    pub created_at: String,
    pub updated_at: String,
}

/// A profile's save of one layout, picked out of its rows for a game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Loaded {
    /// The row of the layout asked for, when it reads.
    pub save: Option<StorySave>,
    /// Every other row: another layout's, or one that doesn't read. Kept,
    /// and named in the log by whoever picked.
    pub ignored: Vec<StorySave>,
}

/// `rows` split by `layout`: its row, when there's one and it reads, and
/// the rest, each logged as kept and ignored.
pub fn choose(rows: Vec<StorySave>, layout: &str) -> Loaded {
    let mut out = Loaded::default();
    for row in rows {
        if row.layout == layout && row.values.is_some() {
            out.save = Some(row);
            continue;
        }
        let why = match row.layout == layout {
            true => "its data doesn't read",
            false => "another save layout",
        };
        warn!(
            profile_id = %row.profile_id,
            game = %row.game,
            layout = %row.layout,
            updated_at = %row.updated_at,
            reason = why,
            "a story save this build doesn't read: kept and ignored"
        );
        out.ignored.push(row);
    }
    out
}

#[derive(sqlx::FromRow)]
struct Row {
    profile_id: String,
    game: String,
    layout: String,
    data: String,
    created_at: String,
    updated_at: String,
}

impl From<Row> for StorySave {
    fn from(r: Row) -> Self {
        StorySave {
            values: serde_json::from_str(&r.data).ok(),
            profile_id: r.profile_id,
            game: r.game,
            layout: r.layout,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

const COLUMNS: &str = "profile_id, game, layout, data, created_at, updated_at";

impl ProfileStore {
    /// guacamole's story saves. Only guacamole's migration set has the table.
    pub fn saves(&self) -> Saves<'_> {
        Saves { pool: &self.pool }
    }
}

/// The story saves' queries; only guacamole's arm calls them.
#[derive(Clone, Copy)]
pub struct Saves<'a> {
    pool: &'a Pool<Sqlite>,
}

impl Saves<'_> {
    /// Every row the profile has for the game, any layout, oldest first.
    pub async fn all(self, profile_id: &str, game: &str) -> Result<Vec<StorySave>> {
        let rows = sqlx::query_as::<_, Row>(&format!(
            "SELECT {COLUMNS} FROM story_saves WHERE profile_id = ?1 AND game = ?2 \
             ORDER BY created_at, layout"
        ))
        .bind(profile_id)
        .bind(game)
        .fetch_all(self.pool)
        .await
        .with_context(|| format!("read {profile_id}'s saves for {game}"))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// The profile's save of `layout` for the game, the others kept and
    /// logged ([`choose`]).
    pub async fn load(self, profile_id: &str, game: &str, layout: &str) -> Result<Loaded> {
        Ok(choose(self.all(profile_id, game).await?, layout))
    }

    /// `values` as the profile's save of `layout` for the game: its row made
    /// or written over, every other layout's row untouched. Refused, nothing
    /// written, when the row of `layout` there doesn't read: no build writes
    /// over a save it can't read.
    pub async fn write(
        self,
        profile_id: &str,
        game: &str,
        layout: &str,
        values: &SaveValues,
    ) -> Result<StorySave> {
        let data = serde_json::to_string(values).context("the save as JSON")?;
        let now = Utc::now().to_rfc3339();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let there: Option<String> = sqlx::query_scalar(
            "SELECT data FROM story_saves WHERE profile_id = ?1 AND game = ?2 AND layout = ?3",
        )
        .bind(profile_id)
        .bind(game)
        .bind(layout)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(d) = there
            && serde_json::from_str::<SaveValues>(&d).is_err()
        {
            bail!("{profile_id}'s {layout} save for {game} doesn't read: not written over");
        }
        let row = sqlx::query_as::<_, Row>(&format!(
            "INSERT INTO story_saves ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?5) \
             ON CONFLICT (profile_id, game, layout) DO UPDATE SET \
                data = excluded.data, updated_at = excluded.updated_at \
             RETURNING {COLUMNS}"
        ))
        .bind(profile_id)
        .bind(game)
        .bind(layout)
        .bind(&data)
        .bind(&now)
        .fetch_one(&mut *tx)
        .await
        .with_context(|| format!("write {profile_id}'s save for {game}"))?;
        tx.commit().await?;
        Ok(row.into())
    }
}

/// The game's host: the profile whose phone started it (CLAUDE.md, Story
/// saves belong to profiles), with the game's serial, as the launch saw it;
/// `None` before a launch, after a quit, or for a launch with no profile
/// unlocked on its phone. Only the host's story save advances.
#[derive(Clone, Default, Debug)]
pub struct GameHost(Arc<RwLock<Option<Host>>>);

/// A host: set at the launch, from the launching phone's unlocked profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host {
    pub profile_id: String,
    pub game: String,
}

impl GameHost {
    pub fn get(&self) -> Option<Host> {
        self.0.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set(&self, host: Option<Host>) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = host;
    }
}

/// What the TV asks of the store, each under a ticket of its own.
#[derive(Debug)]
enum Job {
    Fetch {
        ticket: u64,
        profile_id: String,
        game: String,
    },
    Write {
        ticket: u64,
        profile_id: String,
        game: String,
        layout: String,
        values: SaveValues,
    },
}

/// What the keeper did with a ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    /// Every row the profile has for the game ([`Saves::all`]).
    Fetched {
        ticket: u64,
        result: std::result::Result<Vec<StorySave>, String>,
    },
    /// The write in ([`Saves::write`]), or why not.
    Written {
        ticket: u64,
        result: std::result::Result<(), String>,
    },
}

#[derive(Default)]
struct Shared {
    tx: Mutex<Option<mpsc::UnboundedSender<Job>>>,
    done: Mutex<Vec<Done>>,
    next: AtomicU64,
}

/// The TV's handle on the story saves' keeper: every call returns at once,
/// the keeper on the server's runtime does each job in the order asked and
/// leaves what it did for [`StorySink::take_done`]. Unconnected (no keeper:
/// the product's arm, or a server that never came up), every ask returns
/// `None` and nothing is kept.
#[derive(Clone, Default)]
pub struct StorySink {
    shared: Arc<Shared>,
}

impl StorySink {
    pub fn new() -> Self {
        Self::default()
    }

    /// False with no keeper: every ask then returns `None`.
    pub fn connected(&self) -> bool {
        self.lock_tx().is_some()
    }

    /// Every row `profile_id` has for `game`, asked under the returned
    /// ticket; `None` with no keeper.
    pub fn fetch(&self, profile_id: &str, game: &str) -> Option<u64> {
        let ticket = self.ticket();
        self.send(Job::Fetch {
            ticket,
            profile_id: profile_id.to_owned(),
            game: game.to_owned(),
        })
        .then_some(ticket)
    }

    /// `values` written as `profile_id`'s save of `layout` for `game`,
    /// under the returned ticket; `None` with no keeper.
    pub fn write(
        &self,
        profile_id: &str,
        game: &str,
        layout: &str,
        values: SaveValues,
    ) -> Option<u64> {
        let ticket = self.ticket();
        self.send(Job::Write {
            ticket,
            profile_id: profile_id.to_owned(),
            game: game.to_owned(),
            layout: layout.to_owned(),
            values,
        })
        .then_some(ticket)
    }

    /// What the keeper has done since the last take, in the order done.
    pub fn take_done(&self) -> Vec<Done> {
        std::mem::take(&mut *self.shared.done.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn ticket(&self) -> u64 {
        self.shared.next.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn send(&self, job: Job) -> bool {
        self.lock_tx()
            .as_ref()
            .is_some_and(|tx| tx.send(job).is_ok())
    }

    fn lock_tx(&self) -> std::sync::MutexGuard<'_, Option<mpsc::UnboundedSender<Job>>> {
        self.shared.tx.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Connects `sink` to a keeper over `store` on the current runtime, which
/// does each job in the order asked. Call it once, for guacamole.
pub fn spawn_keeper(sink: StorySink, store: ProfileStore) -> tokio::task::JoinHandle<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    *sink.lock_tx() = Some(tx);
    let shared = sink.shared.clone();
    tokio::spawn(async move {
        info!("story save keeper running");
        while let Some(job) = rx.recv().await {
            let done = match job {
                Job::Fetch {
                    ticket,
                    profile_id,
                    game,
                } => Done::Fetched {
                    ticket,
                    result: store
                        .saves()
                        .all(&profile_id, &game)
                        .await
                        .map_err(|e| format!("{e:#}")),
                },
                Job::Write {
                    ticket,
                    profile_id,
                    game,
                    layout,
                    values,
                } => {
                    let result = store
                        .saves()
                        .write(&profile_id, &game, &layout, &values)
                        .await
                        .map(|_| ())
                        .map_err(|e| format!("{e:#}"));
                    match &result {
                        Ok(()) => info!(%profile_id, %game, %layout, "story save written"),
                        Err(e) => warn!(%profile_id, %game, %layout, "story save not written: {e}"),
                    }
                    Done::Written { ticket, result }
                }
            };
            shared
                .done
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(done);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIANTS: &str = "BLUS30968";
    const LAYOUT: &str = "1:0d1f2e3c";

    async fn store() -> ProfileStore {
        ProfileStore::open_in_memory(&crate::setup::MIGRATOR)
            .await
            .unwrap()
    }

    async fn profile(s: &ProfileStore, name: &str) -> String {
        s.create(name, "1234", "#e63946").await.unwrap()
    }

    /// Chapter 1 done: its `level flags` field's first bit, as the level
    /// end leaves it.
    fn chapter_1_done() -> SaveValues {
        SaveValues {
            params: BTreeMap::from([
                ("level flags".to_owned(), vec![1, 0, 0]),
                ("flags - player".to_owned(), vec![1, 0]),
                ("header - slot level ids".to_owned(), vec![2, 0, 0, 0]),
            ]),
            adventures: vec![
                "Level_213_Tutorial".into(),
                "Level_214_Tutorial_2".into(),
                "Level_201_Rescue_the_Hermit".into(),
            ],
        }
    }

    #[tokio::test]
    async fn a_save_is_written_and_read_back() {
        let s = store().await;
        let alice = profile(&s, "Alice").await;
        assert_eq!(
            s.saves().load(&alice, GIANTS, LAYOUT).await.unwrap(),
            Loaded::default()
        );
        let first = s
            .saves()
            .write(&alice, GIANTS, LAYOUT, &chapter_1_done())
            .await
            .unwrap();
        let loaded = s.saves().load(&alice, GIANTS, LAYOUT).await.unwrap();
        assert_eq!(loaded.save.as_ref(), Some(&first));
        assert_eq!(first.values, Some(chapter_1_done()));
        assert!(loaded.ignored.is_empty());

        // Written again: the same row, its values replaced, made when it was.
        let mut more = chapter_1_done();
        more.params.insert("level flags".into(), vec![1, 1, 0]);
        let second = s
            .saves()
            .write(&alice, GIANTS, LAYOUT, &more)
            .await
            .unwrap();
        assert_eq!(second.created_at, first.created_at);
        assert_eq!(second.values, Some(more));
        assert_eq!(s.saves().all(&alice, GIANTS).await.unwrap(), [second]);
    }

    /// Co-op: the host's save advances, the second profile's stays as it
    /// was, and another game's row is the profile's own too.
    #[tokio::test]
    async fn a_hosts_save_leaves_the_second_profiles_untouched() {
        let s = store().await;
        let (host, guest) = (profile(&s, "Alice").await, profile(&s, "Bob").await);
        let guests = s
            .saves()
            .write(&guest, GIANTS, LAYOUT, &SaveValues::default())
            .await
            .unwrap();
        s.saves()
            .write(&host, GIANTS, LAYOUT, &chapter_1_done())
            .await
            .unwrap();
        let mut later = chapter_1_done();
        later.params.insert("level flags".into(), vec![1, 1, 0]);
        s.saves()
            .write(&host, GIANTS, LAYOUT, &later)
            .await
            .unwrap();
        assert_eq!(s.saves().all(&guest, GIANTS).await.unwrap(), [guests]);
        let hosts = s.saves().load(&host, GIANTS, LAYOUT).await.unwrap();
        assert_eq!(hosts.save.unwrap().values, Some(later));
        assert_eq!(s.saves().all(&host, "BLUS31076").await.unwrap(), []);
    }

    /// A save of another layout: kept as it was, named among the ignored
    /// (what the load logs), the profile reading as one with no save; a new
    /// save goes in beside it, and the old one is still there to read.
    #[tokio::test]
    async fn a_save_of_another_layout_is_kept_and_ignored_and_a_new_one_kept_beside_it() {
        let s = store().await;
        let alice = profile(&s, "Alice").await;
        let old = s
            .saves()
            .write(&alice, GIANTS, "0:older", &chapter_1_done())
            .await
            .unwrap();
        let loaded = s.saves().load(&alice, GIANTS, LAYOUT).await.unwrap();
        assert_eq!(loaded.save, None, "another layout's save read");
        assert_eq!(loaded.ignored, [old.clone()]);

        let new = s
            .saves()
            .write(&alice, GIANTS, LAYOUT, &SaveValues::default())
            .await
            .unwrap();
        let loaded = s.saves().load(&alice, GIANTS, LAYOUT).await.unwrap();
        assert_eq!(loaded.save, Some(new.clone()));
        assert_eq!(loaded.ignored, [old.clone()], "the old save went");
        assert_eq!(
            s.saves().all(&alice, GIANTS).await.unwrap(),
            [old.clone(), new]
        );
        // A build of the old layout still reads its own.
        let theirs = s.saves().load(&alice, GIANTS, "0:older").await.unwrap();
        assert_eq!(theirs.save, Some(old));
    }

    /// A row of the build's own layout whose data doesn't read: ignored,
    /// and never written over.
    #[tokio::test]
    async fn a_save_that_doesnt_read_is_never_written_over() {
        let s = store().await;
        let alice = profile(&s, "Alice").await;
        sqlx::query(
            "INSERT INTO story_saves (profile_id, game, layout, data, created_at, updated_at) \
             VALUES (?1, ?2, ?3, '{\"no\": true', 'then', 'then')",
        )
        .bind(&alice)
        .bind(GIANTS)
        .bind(LAYOUT)
        .execute(&s.pool)
        .await
        .unwrap();
        let loaded = s.saves().load(&alice, GIANTS, LAYOUT).await.unwrap();
        assert_eq!(loaded.save, None);
        assert_eq!(loaded.ignored.len(), 1);
        assert_eq!(loaded.ignored[0].values, None);
        let w = s
            .saves()
            .write(&alice, GIANTS, LAYOUT, &chapter_1_done())
            .await;
        assert!(w.is_err(), "wrote over a save that doesn't read");
        let data: String = sqlx::query_scalar("SELECT data FROM story_saves")
            .fetch_one(&s.pool)
            .await
            .unwrap();
        assert_eq!(data, "{\"no\": true");
    }

    #[tokio::test]
    async fn deleting_a_profile_takes_its_saves() {
        let s = store().await;
        let (alice, bob) = (profile(&s, "Alice").await, profile(&s, "Bob").await);
        for p in [&alice, &bob] {
            s.saves()
                .write(p, GIANTS, LAYOUT, &chapter_1_done())
                .await
                .unwrap();
        }
        assert!(s.delete(&alice).await.unwrap());
        assert_eq!(s.saves().all(&alice, GIANTS).await.unwrap(), []);
        assert_eq!(s.saves().all(&bob, GIANTS).await.unwrap().len(), 1);
    }

    /// The migration on a database from before it: the table gained empty,
    /// every row and table there before as it was.
    #[tokio::test]
    async fn the_migration_adds_the_table_empty_and_changes_nothing_else() {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

        const THIS: &str = "20261102000001_story_saves.sql";
        let tmp = tempfile::tempdir().unwrap();
        let earlier = tmp.path().join("migrations");
        std::fs::create_dir_all(&earlier).unwrap();
        let ours = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let mut copied = 0;
        for entry in std::fs::read_dir(&ours).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().unwrap().to_str().unwrap() < THIS {
                std::fs::copy(&path, earlier.join(path.file_name().unwrap())).unwrap();
                copied += 1;
            }
        }
        assert_eq!(copied, 4);
        let before = sqlx::migrate::Migrator::new(earlier).await.unwrap();
        let opts = SqliteConnectOptions::new()
            .filename(tmp.path().join("db.sqlite"))
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new().connect_with(opts).await.unwrap();
        before.run(&pool).await.unwrap();
        // A profile, its figure and its settings, as an older build left them.
        for q in [
            "INSERT INTO profiles (id, display_name, pin_hash, color, created_at) \
             VALUES ('a', 'Alice', 'x', '#000000', 'now')",
            "INSERT INTO figure_instances \
             (profile_id, figure_id, xp, gold, playtime_secs, generation, created_at, updated_at) \
             VALUES ('a', '000009-0000', 1000, 25, 61, 1, 'now', 'now')",
            "INSERT INTO settings (key, value) VALUES ('port', '8765')",
        ] {
            sqlx::query(q).execute(&pool).await.unwrap();
        }
        let dump = |pool: Pool<Sqlite>| async move {
            let mut out = Vec::new();
            let tables: Vec<String> = sqlx::query_scalar(
                "SELECT name FROM sqlite_master WHERE type = 'table' \
                 AND name NOT LIKE '_sqlx%' AND name <> 'story_saves' ORDER BY name",
            )
            .fetch_all(&pool)
            .await
            .unwrap();
            for t in tables {
                let schema: String =
                    sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name = ?1")
                        .bind(&t)
                        .fetch_one(&pool)
                        .await
                        .unwrap();
                let rows: Vec<String> = sqlx::query_scalar(&format!(
                    "SELECT json_array(*) FROM (SELECT * FROM \"{t}\") ORDER BY 1"
                ))
                .fetch_all(&pool)
                .await
                .unwrap_or_default();
                out.push((t, schema, rows));
            }
            out
        };
        let was = dump(pool.clone()).await;
        assert!(
            was.iter()
                .any(|(t, _, rows)| t == "figure_instances" && rows.len() == 1)
        );

        crate::setup::MIGRATOR.run(&pool).await.unwrap();

        assert_eq!(
            dump(pool.clone()).await,
            was,
            "the migration changed another table"
        );
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM story_saves")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
        // The store's queries run on the migrated file as on a new one.
        let s = ProfileStore {
            pool: pool.clone(),
            lockouts: Default::default(),
        };
        assert_eq!(s.saves().all("a", GIANTS).await.unwrap(), []);
        s.saves()
            .write("a", GIANTS, LAYOUT, &chapter_1_done())
            .await
            .unwrap();
        assert_eq!(s.saves().all("a", GIANTS).await.unwrap().len(), 1);
        pool.close().await;
    }

    /// The keeper does each job in the order asked; with none, nothing's
    /// asked.
    #[tokio::test]
    async fn the_keeper_fetches_and_writes_in_order() {
        let s = store().await;
        let alice = profile(&s, "Alice").await;
        let lone = StorySink::new();
        assert!(!lone.connected());
        assert_eq!(lone.fetch(&alice, GIANTS), None);
        assert_eq!(
            lone.write(&alice, GIANTS, LAYOUT, SaveValues::default()),
            None
        );

        let sink = StorySink::new();
        spawn_keeper(sink.clone(), s.clone());
        assert!(sink.connected());
        let w = sink
            .write(&alice, GIANTS, LAYOUT, chapter_1_done())
            .unwrap();
        let f = sink.fetch(&alice, GIANTS).unwrap();
        let mut done = Vec::new();
        for _ in 0..200 {
            done.extend(sink.take_done());
            if done.len() == 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(done.len(), 2, "{done:?}");
        assert_eq!(
            done[0],
            Done::Written {
                ticket: w,
                result: Ok(())
            }
        );
        match &done[1] {
            Done::Fetched {
                ticket,
                result: Ok(rows),
            } if *ticket == f => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].values, Some(chapter_1_done()));
            }
            other => panic!("{other:?}"),
        }
    }
}
