//! guacamole's loader, settings and database (openspec AA.5), including a full
//! boot beside a product runtime root that must come out byte-identical.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use guac_formats::sfo::builder::SfoBuilder;
use sqlx::Connection;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection, SqlitePool};

use crate::boot::{BootOptions, BuiltDriver, Catalogue, boot};
use crate::config::{DriverKind, WindowMode};
use crate::dumps::GIANTS_TITLE_ID;
use crate::hooks::Hooks;
use crate::profiles::ProfileStore;
use crate::setup::{DEFAULT_PORT, MIGRATOR, Setup, load_with};

static PRODUCT_MIGRATOR: Migrator = sqlx::migrate!("../server/migrations");

const PRODUCT_KEY: [u8; 32] = [9; 32];

fn no_env() -> HashMap<String, String> {
    HashMap::new()
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

fn giants_dump(dir: &Path) {
    let game = dir.join("PS3_GAME");
    std::fs::create_dir_all(&game).unwrap();
    let sfo = SfoBuilder::disc_game(GIANTS_TITLE_ID, "01.00", "01.00").build();
    std::fs::write(game.join("PARAM.SFO"), sfo).unwrap();
}

#[test]
fn a_first_load_writes_nothing_and_needs_no_rpcs3() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("guacamole");
    let loaded = load_with(&root, &no_env()).unwrap();

    assert!(!root.exists(), "the first load created the data root");
    assert!(loaded.needs_setup());
    assert!(!loaded.dump_dirs_from_env);
    let cfg = &loaded.config;
    assert_eq!(cfg.driver_kind, DriverKind::Native);
    assert_eq!(cfg.bind_port, DEFAULT_PORT);
    assert_eq!(cfg.window_mode, WindowMode::Tv);
    assert_eq!(cfg.hmac_key.len(), 32);
    assert!(cfg.rpcs3_exe.as_os_str().is_empty());
    assert!(cfg.firmware_pack_root.as_os_str().is_empty());
}

#[test]
fn dev_env_overrides_the_stored_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("guacamole");
    let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
    std::fs::create_dir_all(&a).unwrap();
    Setup::open(&root).unwrap().set_dump_dirs(&[a]).unwrap();

    let list = std::env::join_paths([&b, &tmp.path().join("c")]).unwrap();
    let env: HashMap<String, String> = [
        ("GUAC_DUMP_DIRS", list.to_str().unwrap()),
        ("BIND_PORT", "9123"),
        ("PHONE_DIST", "/x/phone"),
        ("DATA_ROOT", "/x/data"),
        ("WINDOW_MODE", "desktop"),
        ("RPCS3_EXE", "/ignored"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let loaded = load_with(&root, &env).unwrap();

    assert!(loaded.dump_dirs_from_env);
    assert_eq!(loaded.dump_dirs, [b, tmp.path().join("c")]);
    assert!(!loaded.needs_setup());
    assert_eq!(loaded.config.bind_port, 9123);
    assert_eq!(loaded.config.phone_dist_dir, PathBuf::from("/x/phone"));
    assert_eq!(loaded.config.data_root, PathBuf::from("/x/data"));
    assert_eq!(loaded.config.window_mode, WindowMode::Desktop);
    assert!(loaded.config.rpcs3_exe.as_os_str().is_empty());
}

#[test]
fn settings_round_trip_through_the_database() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("guacamole");
    let discs = tmp.path().join("discs");
    giants_dump(&discs.join("Giants"));
    let other = tmp.path().join("more discs");
    std::fs::create_dir_all(&other).unwrap();

    {
        let setup = Setup::open(&root).unwrap();
        assert!(setup.needs_setup().unwrap());
        assert!(setup.set_dump_dirs(&[]).is_err());
        assert!(setup.set_dump_dirs(&[tmp.path().join("nope")]).is_err());
        let scan = setup
            .set_dump_dirs(&[discs.clone(), other.clone()])
            .unwrap();
        assert_eq!(
            scan.offered().map(|d| d.title_id.as_str()),
            Some(GIANTS_TITLE_ID)
        );
        setup.set_bind_port(9001).unwrap();
        assert!(setup.set_bind_port(0).is_err());
        setup.keep_pairing_key(&[5; 32]).unwrap();
    }

    let setup = Setup::open(&root).unwrap();
    let stored = setup.settings().unwrap();
    assert!(!setup.needs_setup().unwrap());
    assert_eq!(stored.dump_dirs, [discs.clone(), other]);
    assert_eq!(stored.bind_port, Some(9001));
    assert_eq!(stored.pairing_key, Some(vec![5; 32]));
    drop(setup);

    let loaded = load_with(&root, &no_env()).unwrap();
    assert_eq!(loaded.dump_dirs, stored.dump_dirs);
    assert_eq!(loaded.config.bind_port, 9001);
    assert_eq!(loaded.config.hmac_key, vec![5; 32]);
    assert_eq!(loaded.scan().games().len(), 1);
}

#[test]
fn a_key_is_generated_on_the_first_start_and_reused_on_the_next() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("guacamole");

    let first = load_with(&root, &no_env()).unwrap();
    let kept = Setup::open(&root)
        .unwrap()
        .keep_pairing_key(&first.config.hmac_key)
        .unwrap();
    assert_eq!(kept, first.config.hmac_key);

    // A load reads the database without writing to it.
    let db_before = std::fs::read(root.join("db.sqlite")).unwrap();
    let second = load_with(&root, &no_env()).unwrap();
    assert_eq!(second.config.hmac_key, first.config.hmac_key);
    assert_eq!(std::fs::read(root.join("db.sqlite")).unwrap(), db_before);
    // A later candidate never replaces the stored key.
    let again = Setup::open(&root)
        .unwrap()
        .keep_pairing_key(&[1; 32])
        .unwrap();
    assert_eq!(again, first.config.hmac_key);
}

#[test]
fn an_unmigrated_database_reads_as_never_set() {
    let tmp = tempfile::tempdir().unwrap();
    // A first start killed after `ProfileStore::open` created the file.
    let empty = tmp.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    std::fs::write(empty.join("db.sqlite"), b"").unwrap();
    assert!(load_with(&empty, &no_env()).unwrap().needs_setup());

    // ...or after the first migration only.
    let partial = tmp.path().join("partial");
    let db = crate::paths::db_path_under(&partial).unwrap();
    rt().block_on(async {
        let opts = SqliteConnectOptions::new()
            .filename(&db)
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        sqlx::raw_sql(include_str!("../migrations/20261018000001_profiles.sql"))
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    });
    assert!(load_with(&partial, &no_env()).unwrap().needs_setup());
}

#[test]
fn a_crashed_writes_hot_journal_rolls_back_on_load() {
    let tmp = tempfile::tempdir().unwrap();
    let live = tmp.path().join("live");
    let (before, after) = (tmp.path().join("before"), tmp.path().join("after"));
    std::fs::create_dir_all(&before).unwrap();
    std::fs::create_dir_all(&after).unwrap();
    Setup::open(&live)
        .unwrap()
        .set_dump_dirs(std::slice::from_ref(&before))
        .unwrap();

    // Spill a transaction to disk, then copy the files while it's open: the
    // copy is what a crash mid-commit leaves behind.
    let crashed = tmp.path().join("crashed");
    std::fs::create_dir_all(&crashed).unwrap();
    rt().block_on(async {
        let opts = SqliteConnectOptions::new().filename(live.join("db.sqlite"));
        let mut conn = SqliteConnection::connect_with(&opts).await.unwrap();
        sqlx::raw_sql("PRAGMA cache_size = 1; BEGIN")
            .execute(&mut conn)
            .await
            .unwrap();
        let moved = serde_json::to_string(&[&after]).unwrap();
        sqlx::query("UPDATE settings SET value = ?1 WHERE key = 'dump_dirs'")
            .bind(moved)
            .execute(&mut conn)
            .await
            .unwrap();
        for i in 0..200 {
            sqlx::query("INSERT INTO settings (key, value) VALUES (?1, ?2)")
                .bind(format!("pad{i}"))
                .bind("x".repeat(1000))
                .execute(&mut conn)
                .await
                .unwrap();
        }
        for name in ["db.sqlite", "db.sqlite-journal"] {
            std::fs::copy(live.join(name), crashed.join(name)).unwrap();
        }
        sqlx::raw_sql("ROLLBACK").execute(&mut conn).await.unwrap();
    });

    let loaded = load_with(&crashed, &no_env()).unwrap();
    assert_eq!(loaded.dump_dirs, [before]);
    assert!(!crashed.join("db.sqlite-journal").exists());
}

#[test]
fn a_first_start_has_no_profiles_and_a_pin_unlocks_only_its_own() {
    let tmp = tempfile::tempdir().unwrap();
    let db = crate::paths::db_path_under(&tmp.path().join("guacamole")).unwrap();
    let rt = rt();

    let id = rt.block_on(async {
        let store = ProfileStore::open(&db, &MIGRATOR).await.unwrap();
        assert!(store.list().await.unwrap().is_empty());
        let id = store.create("Kid", "1234", "#ff00aa").await.unwrap();
        assert!(store.verify_pin(&id, "1234").await.unwrap());
        assert!(!store.verify_pin(&id, "4321").await.unwrap());
        store.pool.close().await;
        id
    });

    // A restart keeps the profile and its PIN.
    rt.block_on(async {
        let store = ProfileStore::open(&db, &MIGRATOR).await.unwrap();
        let rows = store.list().await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert!(store.verify_pin(&id, "1234").await.unwrap());
        assert!(!store.verify_pin(&id, "4321").await.unwrap());
    });
}

/// A product runtime root as the product leaves it: `config.json` and a
/// `db.sqlite` holding one profile.
fn product_root(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let config = serde_json::json!({ "hmac_key": hex::encode(PRODUCT_KEY), "bind_port": 8765 });
    std::fs::write(dir.join("config.json"), config.to_string()).unwrap();
    rt().block_on(async {
        let store = ProfileStore::open(&dir.join("db.sqlite"), &PRODUCT_MIGRATOR)
            .await
            .unwrap();
        store
            .create("Product kid", "1111", "#00ff00")
            .await
            .unwrap();
        store.pool.close().await;
    });
}

fn ephemeral_port() -> u16 {
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|l| l.local_addr())
        .unwrap()
        .port()
}

fn request(addr: SocketAddr, key: &[u8], method: &str, path: &str, body: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let sig = hex::encode(crate::http::compute_hmac(
        key,
        ts,
        method,
        path,
        body.as_bytes(),
    ));
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nX-Skyportal-Timestamp: {ts}\r\n\
         X-Skyportal-Sig: {sig}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn a_boot_beside_the_product_root_leaves_it_byte_identical() {
    let tmp = tempfile::tempdir().unwrap();
    // As in dev: the product's `dev-data/` holds guacamole's root.
    let product = tmp.path().join("dev-data");
    product_root(&product);
    let config_before = std::fs::read(product.join("config.json")).unwrap();
    let db_before = std::fs::read(product.join("db.sqlite")).unwrap();
    let root = product.join("guacamole");

    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(
        data.join("figures.json"),
        r#"[{"figure_id": "000009-0000", "alignment": "Core Skylander",
             "attributes": {"name": "Eruptor", "element": "Fire",
                            "firstrelease": "Skylanders: Spyro's Adventure"}}]"#,
    )
    .unwrap();
    let discs = tmp.path().join("discs");
    giants_dump(&discs.join("Skylanders Giants"));
    let env: HashMap<String, String> = [
        ("GUAC_DUMP_DIRS", discs.to_str().unwrap().to_string()),
        ("BIND_PORT", ephemeral_port().to_string()),
        ("DATA_ROOT", data.to_str().unwrap().to_string()),
        (
            "PHONE_DIST",
            tmp.path().join("dist").to_str().unwrap().to_string(),
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();

    let loaded = load_with(&root, &env).unwrap();
    assert_ne!(loaded.config.hmac_key, PRODUCT_KEY);
    let games = loaded.scan().games();
    let key = loaded.config.hmac_key.clone();
    let handles = boot(BootOptions {
        config: loaded.config,
        runtime_root: root.clone(),
        migrator: &MIGRATOR,
        driver_factory: Box::new(|_| {
            Ok(BuiltDriver::new(Arc::new(
                skylander_rpcs3_control::MockPortalDriver::new(),
            )))
        }),
        hooks: Hooks::default(),
        extra_routes: None,
        catalogue: Catalogue::Native { games },
        bind_ip: Some(Ipv4Addr::LOCALHOST),
    })
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !handles.launcher_status.lock().unwrap().server_ready {
        assert!(Instant::now() < deadline, "server never became ready");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        Setup::open(&root).unwrap().keep_pairing_key(&key).unwrap(),
        key
    );
    let addr = handles.bind_addr;

    let profiles = request(addr, &key, "GET", "/api/profiles", "");
    assert!(profiles.starts_with("HTTP/1.1 200"), "{profiles}");
    assert!(profiles.ends_with("[]"), "{profiles}");
    let games = request(addr, &key, "GET", "/api/games", "");
    assert!(games.contains(GIANTS_TITLE_ID), "{games}");

    // The controller's key doesn't pair with guacamole; its own does.
    let body = r##"{"display_name":"Kid","pin":"1234","color":"#ff00aa"}"##;
    let refused = request(addr, &PRODUCT_KEY, "POST", "/api/profiles", body);
    assert!(refused.starts_with("HTTP/1.1 401"), "{refused}");
    let created = request(addr, &key, "POST", "/api/profiles", body);
    assert!(created.starts_with("HTTP/1.1 201"), "{created}");

    rt().block_on(async {
        let store = ProfileStore::open(&root.join("db.sqlite"), &MIGRATOR)
            .await
            .unwrap();
        let rows = store.list().await.unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].display_name, "Kid");
        assert!(store.verify_pin(&rows[0].id, "1234").await.unwrap());
        assert!(!store.verify_pin(&rows[0].id, "4321").await.unwrap());
        assert!(!store.verify_pin(&rows[0].id, "1111").await.unwrap());
    });

    assert_eq!(
        std::fs::read(product.join("config.json")).unwrap(),
        config_before
    );
    assert_eq!(std::fs::read(product.join("db.sqlite")).unwrap(), db_before);
    let mut entries: Vec<String> = std::fs::read_dir(&product)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();
    assert_eq!(entries, ["config.json", "db.sqlite", "guacamole"]);
}
