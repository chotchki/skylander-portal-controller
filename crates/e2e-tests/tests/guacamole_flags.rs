//! A playtest's flags (openspec AS.440): the phone's "flag this" field sends
//! a note, the TV takes a capture with it, and the capture's folder holds
//! the note, the TV's state, the log's tail and the frame; a capture the
//! pad's capture button took (a posted pad's, through `/api/_test/tv/pad`)
//! takes the phone's next note into its own folder. With flags off (the
//! folder refused inside a git work tree) the server says so and the phone
//! draws no field.
//!
//! Guacamole-only: skipped unless `E2E_APP=guacamole`. Run with
//!
//! ```text
//! E2E_APP=guacamole cargo test -p skylander-e2e-tests --test guacamole_flags \
//!     -- --ignored --test-threads=1
//! ```

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fantoccini::Locator;
use serde_json::{Value, json};

use skylander_e2e_tests::{
    GIANTS_SERIAL, Phone, TestServer, guacamole_only, inject_profile, pad_tv,
};

async fn flags_on(base: &str) -> Value {
    reqwest::get(format!("{base}/api/_test/flag"))
        .await
        .expect("GET /api/_test/flag")
        .json()
        .await
        .expect("its JSON")
}

/// The folders under `dir`, sorted (their names sort by time).
fn folders(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().into_string().unwrap())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Polls for `path` until it's a file or `within` runs out: the frame lands
/// after the folder, once the window's capture comes back.
fn wait_for_file(path: &Path, within: Duration) -> bool {
    let until = Instant::now() + within;
    while Instant::now() < until {
        if path.is_file() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    path.is_file()
}

/// A phone at the profile picker with its menu open: the field lives in the
/// menu, which needs no game picked.
async fn phone_with_menu(server: &TestServer) -> Phone {
    inject_profile(&server.url, "Alice", "1234", "#e8423f")
        .await
        .unwrap();
    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .unwrap();
    phone
        .wait_for(Locator::Css(".profile-picker"), Duration::from_secs(15))
        .await
        .unwrap();
    phone
        .wait_for(Locator::Css(".kebab-btn"), Duration::from_secs(5))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    phone
        .menu_actions_until(|a| a.iter().any(|t| t == "MANAGE PROFILES"))
        .await
        .unwrap();
    phone
}

/// The toasts up now, one a line.
async fn toasts(phone: &Phone) -> Vec<String> {
    let text = phone
        .client
        .execute(
            "return Array.from(document.querySelectorAll('.toast-msg')).map(t => t.textContent)",
            vec![],
        )
        .await
        .unwrap();
    serde_json::from_value(text).unwrap_or_default()
}

/// Types `note` into the field and sends it; the flag's toast it answers
/// with (a new one: an earlier flag's may still be up).
async fn send_flag(phone: &Phone, note: &str) -> String {
    let before = toasts(phone).await;
    let field = phone
        .wait_for(
            Locator::Css(".flag-this .flag-note"),
            Duration::from_secs(5),
        )
        .await
        .expect("the flag field");
    field.send_keys(note).await.unwrap();
    phone
        .client
        .find(Locator::Css(".flag-this .flag-send"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(new) = toasts(phone)
            .await
            .into_iter()
            .find(|t| t.contains("Flag") && !before.contains(t))
        {
            return new;
        }
        assert!(Instant::now() < until, "no new flag toast: {before:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn capture_json(folder: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(folder.join("capture.json")).expect("capture.json"))
        .expect("its JSON")
}

#[tokio::test]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn a_flag_from_the_phone_lands_in_its_folder_and_joins_a_pads() {
    if guacamole_only() {
        return;
    }
    let flags = tempfile::tempdir().unwrap();
    let dir: PathBuf = flags.path().join("playtest-flags");
    let server = TestServer::spawn_guacamole(
        &[GIANTS_SERIAL],
        &format!("GUAC_FLAG_DIR={}\n", dir.display()),
    )
    .expect("spawn guacamole");
    assert_eq!(flags_on(&server.url).await, json!({"on": true}));
    let phone = phone_with_menu(&server).await;

    // The phone's own: a folder by time, the note in it and in its JSON.
    let toast = send_flag(&phone, "the ship is invisible here").await;
    let made = folders(&dir);
    assert_eq!(made.len(), 1, "{made:?}");
    let first = &made[0];
    assert!(first.ends_with("Z-phone"), "{first}");
    assert!(toast.contains(&format!("Flag saved as {first}")), "{toast}");
    let folder = dir.join(first);
    let json = capture_json(&folder);
    assert_eq!(json["source"], "phone");
    assert_eq!(json["note"], "the ship is invisible here");
    assert_eq!(json["build"], "e2e-test");
    // The hero's last autoaim and his last that picked something (openspec
    // AS.1042): keys of their own, null on the menu, where no hero has aimed.
    for key in ["aim", "aim_pick"] {
        assert!(
            json.as_object().is_some_and(|o| o.contains_key(key)) && json[key].is_null(),
            "{key}: {}",
            json[key]
        );
    }
    assert!(
        json["tv"]["screen"].as_str().is_some_and(|s| !s.is_empty()),
        "{}",
        json["tv"]
    );
    assert!(
        json["frames_recent"]["frames"]
            .as_array()
            .is_some_and(|f| !f.is_empty())
    );
    let note = std::fs::read_to_string(folder.join("note.txt")).unwrap();
    assert!(note.ends_with("] the ship is invisible here\n"), "{note}");
    let log = std::fs::read_to_string(folder.join("log.txt")).unwrap();
    assert!(
        log.lines().count() > 10 && log.contains("guacamole"),
        "{log}"
    );
    assert!(
        wait_for_file(&folder.join("frame.png"), Duration::from_secs(20)),
        "no frame in {}",
        folder.display()
    );

    // The pad's capture button, then a note: one folder, the note in it.
    pad_tv(
        &server.url,
        json!({"pad": 0, "connect": {"name": "Pro Controller"}}),
    )
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    pad_tv(&server.url, json!({"pad": 0, "press": "capture"}))
        .await
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while folders(&dir).len() < 2 {
        assert!(
            Instant::now() < until,
            "no pad capture: {:?}",
            folders(&dir)
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let pads = folders(&dir)
        .into_iter()
        .find(|f| f.ends_with("Z-pad"))
        .expect("the pad's");
    let pad_json = capture_json(&dir.join(&pads));
    assert_eq!(pad_json["source"], "pad");
    assert_eq!(pad_json["pad"], "Pro Controller");
    let toast = send_flag(&phone, "double swords").await;
    assert!(toast.contains(&format!("Flag added to {pads}")), "{toast}");
    assert_eq!(folders(&dir).len(), 2, "joined: no new folder");
    let note = std::fs::read_to_string(dir.join(&pads).join("note.txt")).unwrap();
    assert!(note.ends_with("] double swords\n"), "{note}");
    phone.close().await.ok();
}

#[tokio::test]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn with_flags_off_the_phone_shows_no_field() {
    if guacamole_only() {
        return;
    }
    // A flag folder inside a git work tree is refused: flags are off.
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("checkout");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let dir = repo.join("flags");
    let server = TestServer::spawn_guacamole(
        &[GIANTS_SERIAL],
        &format!("GUAC_FLAG_DIR={}\n", dir.display()),
    )
    .expect("spawn guacamole");
    assert_eq!(flags_on(&server.url).await, json!({"on": false}));
    let refused = reqwest::Client::new()
        .post(format!("{}/api/_test/flag", server.url))
        .json(&json!({"note": "x"}))
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(refused.as_u16(), 409);
    let phone = phone_with_menu(&server).await;
    // The menu is up with its actions; the field never comes.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let field = phone
        .client
        .find_all(Locator::Css(".flag-this"))
        .await
        .unwrap();
    assert!(field.is_empty(), "a flag field with flags off");
    assert!(!dir.exists(), "nothing made in a checkout");
    assert!(
        server
            .log_text()
            .contains("GUAC_FLAG_DIR is inside a git work tree"),
        "the refusal is logged"
    );
    phone.close().await.ok();
}
