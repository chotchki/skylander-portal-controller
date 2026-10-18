//! guacamole's TV and shell against the phone (AA.10; the `guacamole/figure-cards`
//! and `guacamole/shell` specs). What the TV shows is read from
//! `/api/_test/tv`, a copy of the laid-out UI tree taken each frame.
//!
//! Guacamole-only: skipped unless `E2E_APP=guacamole`. Run with
//!
//! ```text
//! E2E_APP=guacamole cargo test -p skylander-e2e-tests --test guacamole_tv \
//!     -- --ignored --test-threads=1
//! ```

use std::time::{Duration, Instant};

use fantoccini::Locator;
use serde_json::{Value, json};

use skylander_e2e_tests::{
    GIANTS_SERIAL, Phone, TestServer, fire_kaos_swap, guacamole_only, inject_load_outcomes,
    inject_profile, set_session_profile, tv, wait_for_tv,
};

/// The spec's bound from the phone showing a change to the TV showing it.
const TV_WITHIN: Duration = Duration::from_secs(1);

const ALICE: &str = "#e8423f";
const BOB: &str = "#3fa7e8";

fn spawn() -> TestServer {
    TestServer::spawn_guacamole(&[GIANTS_SERIAL], "").expect("spawn guacamole")
}

async fn session_id(phone: &Phone) -> u64 {
    for _ in 0..100 {
        if let Ok(Some(id)) = phone.session_id().await {
            return id;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("phone never received Event::Welcome");
}

/// A phone with `name`'s profile open: (phone, profile id, session id).
async fn phone_as(server: &TestServer, name: &str, color: &str) -> (Phone, String, u64) {
    let pid = inject_profile(&server.url, name, "1234", color)
        .await
        .unwrap();
    // Not `unlock_session`: it also rebinds the newest open session, which
    // would hand an earlier phone to this profile.
    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .unwrap();
    let sid = session_id(&phone).await;
    set_session_profile(&server.url, sid, &pid).await.unwrap();
    (phone, pid, sid)
}

/// Picks Giants on the phone (a signed `/api/launch`, no RPCS3) and waits
/// for both the phone's portal and the TV's.
async fn pick_giants(server: &TestServer, phone: &Phone) {
    let card = phone
        .wait_for(
            Locator::Css(".game-picker .game-card"),
            Duration::from_secs(15),
        )
        .await
        .expect("game picker");
    // Past the cards' rise animation.
    tokio::time::sleep(Duration::from_millis(600)).await;
    card.click().await.expect("click Giants");
    phone
        .wait_for_portal(Duration::from_secs(15))
        .await
        .expect("phone portal");
    wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        s["screen"] == "Portal"
    })
    .await
    .expect("TV portal");
}

/// `/api/figures` as the phone in session `sid` gets it.
async fn figures(server: &TestServer, sid: u64) -> Vec<Value> {
    reqwest::Client::new()
        .get(format!("{}/api/figures", server.url))
        .header("x-session-id", sid.to_string())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

fn id_named(figs: &[Value], name: &str) -> String {
    figs.iter()
        .find(|f| f["canonical_name"] == name)
        .unwrap_or_else(|| panic!("no figure named {name}"))["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// The request the phone's place button sends, for the steps the phone's
/// own UI won't take (a figure already on the portal, a given slot).
async fn load(server: &TestServer, sid: u64, slot: u8, figure_id: &str) {
    let resp = reqwest::Client::new()
        .post(format!("{}/api/portal/slot/{slot}/load", server.url))
        .header("x-session-id", sid.to_string())
        .json(&json!({ "figure_id": figure_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 202, "{}", resp.text().await.unwrap());
}

/// Search, tap the card, PLACE: lands in the first empty slot.
async fn place_named(phone: &Phone, name: &str) {
    phone.open_search().await.unwrap();
    let input = phone
        .client
        .find(Locator::Css(".search-input-p4"))
        .await
        .unwrap();
    input.clear().await.unwrap();
    input.send_keys(name).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    phone.place_figure_named(name).await.unwrap();
}

async fn wait_slot(phone: &Phone, slot: u8, name: &str) -> Instant {
    let want = name.to_lowercase();
    phone
        .wait_until(Duration::from_secs(8), || async {
            phone
                .slot_text(slot)
                .await
                .map(|t| t.to_lowercase().contains(&want))
                .unwrap_or(false)
        })
        .await
        .unwrap_or_else(|_| panic!("phone slot {slot} never showed {name}"));
    Instant::now()
}

fn card(snap: &Value, slot: u8) -> Option<&Value> {
    snap["cards"]
        .as_array()?
        .iter()
        .find(|c| c["slot"].as_u64() == Some(slot.into()))
}

fn card_slots(snap: &Value) -> Vec<u64> {
    snap["cards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["slot"].as_u64().unwrap())
        .collect()
}

fn texts(card: &Value) -> Vec<String> {
    card["texts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap().to_string())
        .collect()
}

/// Waits for `slot`'s card to read `name` within [`TV_WITHIN`] of `since`.
async fn tv_card(server: &TestServer, slot: u8, name: &str, since: Instant) -> Value {
    let left = TV_WITHIN.saturating_sub(since.elapsed());
    let (snap, at) = wait_for_tv(&server.url, left, |s| {
        card(s, slot).is_some_and(|c| c["texts"][0] == name)
    })
    .await
    .unwrap_or_else(|e| panic!("TV slot {slot} didn't show {name} within 1 s: {e}"));
    eprintln!(
        "TV showed {name} in slot {slot} {} ms after the phone",
        at.duration_since(since).as_millis()
    );
    snap
}

fn assert_pip(card: &Value, initial: &str, color: &str, profile_id: &str) {
    let pip = &card["pip"];
    assert_eq!(pip["initial"], initial, "{card}");
    assert!(
        pip["color"].as_str().unwrap().eq_ignore_ascii_case(color),
        "{card}"
    );
    assert_eq!(pip["profile_id"], profile_id, "{card}");
}

// ---- guacamole/figure-cards ------------------------------------------------

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn the_tv_shows_the_join_code_and_counts_phones() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["screen"] == "Join"
    })
    .await
    .unwrap();
    assert_eq!(snap["players"], 0);
    let address = snap["join_address"].as_str().unwrap();
    assert!(
        address.starts_with("http://") && !address.contains("k="),
        "{address}"
    );

    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .unwrap();
    let opened = Instant::now();
    session_id(&phone).await;
    wait_for_tv(&server.url, Duration::from_secs(2), |s| s["players"] == 1)
        .await
        .expect("player count reaches 1 within 2 s");
    eprintln!("players=1 {} ms after open", opened.elapsed().as_millis());
    phone.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn every_profile_gets_the_same_figure_list() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (old, _, old_sid) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &old).await;
    // Alice has history: Eruptor placed and taken off again.
    let before = figures(&server, old_sid).await;
    load(&server, old_sid, 1, &id_named(&before, "Eruptor")).await;
    wait_slot(&old, 1, "Eruptor").await;
    old.remove_slot(1).await.unwrap();
    old.wait_for_slot_empty(1, Duration::from_secs(5))
        .await
        .unwrap();

    let (new, _, new_sid) = phone_as(&server, "Bob", BOB).await;
    new.wait_for_portal(Duration::from_secs(15)).await.unwrap();

    let set = |figs: Vec<Value>| {
        let mut rows: Vec<String> = figs
            .iter()
            .map(|f| format!("{} {} {}", f["id"], f["canonical_name"], f["element"]))
            .collect();
        rows.sort();
        rows
    };
    let old_list = set(figures(&server, old_sid).await);
    let new_list = set(figures(&server, new_sid).await);
    assert_eq!(
        old_list, new_list,
        "old and new profiles see different lists"
    );
    assert!(!old_list.is_empty());
    let names: Vec<String> = figures(&server, new_sid)
        .await
        .iter()
        .map(|f| f["canonical_name"].as_str().unwrap().to_string())
        .collect();
    for want in ["Spyro", "Tree Rex"] {
        assert!(names.iter().any(|n| n == want), "{want} missing: {names:?}");
    }
    eprintln!("{} figures for both profiles", old_list.len());

    // The phones' toy boxes list the same cards.
    let mut boxes = Vec::new();
    for phone in [&old, &new] {
        phone.open_toy_box_lid().await.unwrap();
        phone
            .wait_for(Locator::Css(".fig-card-p4"), Duration::from_secs(5))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        let listed = phone
            .client
            .execute(
                "return Array.from(document.querySelectorAll('.fig-card-p4 .fig-name-p4')).map(e => e.innerText).sort();",
                vec![],
            )
            .await
            .unwrap();
        boxes.push(listed);
    }
    assert_eq!(boxes[0], boxes[1]);
    let listed: Vec<&str> = boxes[0]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        listed.contains(&"Spyro") && listed.contains(&"Tree Rex"),
        "{listed:?}"
    );
    old.close().await.unwrap();
    new.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn a_placement_shows_its_card_within_a_second() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (phone, pid, _) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &phone).await;
    let (empty, _) = wait_for_tv(&server.url, Duration::from_secs(2), |s| {
        s["screen"] == "Portal"
    })
    .await
    .unwrap();
    assert!(card_slots(&empty).is_empty());
    assert_eq!(empty["game"], "Skylanders: Giants");

    let clicked = Instant::now();
    place_named(&phone, "Eruptor").await;
    let shown = wait_slot(&phone, 1, "Eruptor").await;
    let snap = tv_card(&server, 1, "Eruptor", shown).await;
    eprintln!("click to TV card: {} ms", clicked.elapsed().as_millis());
    assert_eq!(card_slots(&snap), [1], "{snap}");
    let eruptor = card(&snap, 1).unwrap();
    assert_eq!(texts(eruptor), ["Eruptor", "Fire"], "name and element only");
    assert_eq!(eruptor["figure_id"], "000009-0000");
    assert_eq!(eruptor["portrait"], true, "Eruptor has a thumb.png");
    assert_pip(eruptor, "A", ALICE, &pid);
    let drawn = format!("{eruptor}").to_lowercase();
    for word in ["level", "xp", "experience", "gold"] {
        assert!(!drawn.contains(word), "card shows {word}: {eruptor}");
    }
    phone.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn the_same_figure_on_two_profiles_is_two_cards() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (alice, alice_id, _) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &alice).await;
    let (bob, bob_id, bob_sid) = phone_as(&server, "Bob", BOB).await;
    bob.wait_for_portal(Duration::from_secs(15)).await.unwrap();

    place_named(&alice, "Eruptor").await;
    let shown = wait_slot(&alice, 1, "Eruptor").await;
    tv_card(&server, 1, "Eruptor", shown).await;
    // Bob's phone refuses a figure already on the portal, whoever placed it
    // (figure_detail.rs's `already` check), so his copy goes as the request
    // his place button would send.
    let figs = figures(&server, bob_sid).await;
    load(&server, bob_sid, 2, &id_named(&figs, "Eruptor")).await;
    let shown = wait_slot(&bob, 2, "Eruptor").await;
    let snap = tv_card(&server, 2, "Eruptor", shown).await;

    assert_eq!(card_slots(&snap), [1, 2]);
    assert_pip(card(&snap, 1).unwrap(), "A", ALICE, &alice_id);
    assert_pip(card(&snap, 2).unwrap(), "B", BOB, &bob_id);
    alice.close().await.unwrap();
    bob.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn removing_a_figure_drops_its_card_and_the_rest_stay_put() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (phone, _, sid) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &phone).await;
    let figs = figures(&server, sid).await;
    for (slot, name) in [(1, "Eruptor"), (2, "Spyro"), (3, "Tree Rex")] {
        load(&server, sid, slot, &id_named(&figs, name)).await;
        let shown = wait_slot(&phone, slot, name).await;
        tv_card(&server, slot, name, shown).await;
    }
    let before = tv(&server.url).await.unwrap();
    assert_eq!(card_slots(&before), [1, 2, 3]);

    // Script clicks: in the 420 px headless window slot 3's owner initial
    // sits over its remove button and takes a WebDriver click.
    let slot3 = "const s = Array.from(document.querySelectorAll('.portal-p4 .p4-slot'))
                   .find(s => s.querySelector('.p4-slot-index')?.textContent.trim() === '3');";
    let arm = format!("{slot3} if (!s) return false; s.click(); return true;");
    assert!(
        phone
            .client
            .execute(&arm, vec![])
            .await
            .unwrap()
            .as_bool()
            .unwrap()
    );
    let remove = format!(
        "{slot3} const b = s && s.querySelector('.p4-slot-action--remove');
         if (!b) return false; b.click(); return true;"
    );
    phone
        .wait_until(Duration::from_secs(2), || async {
            phone
                .client
                .execute(&remove, vec![])
                .await
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        })
        .await
        .expect("slot 3's remove button");
    phone
        .wait_for_slot_empty(3, Duration::from_secs(5))
        .await
        .unwrap();
    let gone = Instant::now();
    let (after, at) = wait_for_tv(&server.url, TV_WITHIN, |s| card(s, 3).is_none())
        .await
        .expect("slot 3's card leaves within 1 s");
    eprintln!(
        "TV dropped slot 3 {} ms after the phone",
        at.duration_since(gone).as_millis()
    );
    assert_eq!(card_slots(&after), [1, 2]);
    assert!(after["empty_slots"].as_array().unwrap().contains(&json!(3)));
    for slot in [1, 2] {
        assert_eq!(
            card(&after, slot).unwrap()["rect"],
            card(&before, slot).unwrap()["rect"],
            "slot {slot} moved"
        );
    }
    phone.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn a_swap_puts_the_new_card_in_the_old_ones_place() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (phone, alice_id, sid) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &phone).await;
    let figs = figures(&server, sid).await;

    // A phone swap: a load over slot 1 replaces Eruptor with Spyro.
    load(&server, sid, 1, &id_named(&figs, "Eruptor")).await;
    let shown = wait_slot(&phone, 1, "Eruptor").await;
    let before = tv_card(&server, 1, "Eruptor", shown).await;
    load(&server, sid, 1, &id_named(&figs, "Spyro")).await;
    let shown = wait_slot(&phone, 1, "Spyro").await;
    let after = tv_card(&server, 1, "Spyro", shown).await;
    assert_eq!(card_slots(&after), [1]);
    assert_eq!(
        card(&after, 1).unwrap()["rect"],
        card(&before, 1).unwrap()["rect"]
    );

    // A Kaos swap on slot 2: Bob owns slot 1, so Alice's slot 2 is the only
    // one Kaos can take from her.
    let bob_id = inject_profile(&server.url, "Bob", "2222", BOB)
        .await
        .unwrap();
    set_session_profile(&server.url, sid, &bob_id)
        .await
        .unwrap();
    load(&server, sid, 1, &id_named(&figs, "Eruptor")).await;
    let shown = wait_slot(&phone, 1, "Eruptor").await;
    tv_card(&server, 1, "Eruptor", shown).await;
    set_session_profile(&server.url, sid, &alice_id)
        .await
        .unwrap();
    load(&server, sid, 2, &id_named(&figs, "Tree Rex")).await;
    let shown = wait_slot(&phone, 2, "Tree Rex").await;
    let before = tv_card(&server, 2, "Tree Rex", shown).await;

    fire_kaos_swap(&server.url, &alice_id).await.unwrap();
    let (after, _) = wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        card(s, 2).is_some_and(|c| c["texts"][0] != "Tree Rex")
    })
    .await
    .expect("Kaos swapped slot 2");
    let swapped = card(&after, 2).unwrap();
    eprintln!("Kaos swapped Tree Rex for {}", swapped["texts"][0]);
    assert_eq!(card_slots(&after), [1, 2], "the old card is gone, no extra");
    assert_eq!(swapped["rect"], card(&before, 2).unwrap()["rect"]);
    assert_eq!(card(&after, 1).unwrap()["texts"][0], "Eruptor");
    // Kaos picks at random; the server's portal and the TV must agree on its
    // pick. (The phone covers its portal with the Kaos overlay meanwhile.)
    let portal: Vec<Value> = reqwest::get(format!("{}/api/portal", server.url))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(portal[1]["kind"], "loaded", "{portal:?}");
    assert_eq!(portal[1]["figure_id"], swapped["figure_id"]);
    assert_eq!(portal[1]["display_name"], swapped["texts"][0]);
    phone.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn eight_figures_are_eight_cards_none_hidden() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (phone, _, sid) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &phone).await;
    let figs = figures(&server, sid).await;
    let names = [
        "Eruptor",
        "Spyro",
        "Tree Rex",
        "Ignitor",
        "Gill Grunt",
        "Trigger Happy",
        "Bash",
        "Jet-Vac",
    ];
    for (i, name) in names.iter().enumerate() {
        load(&server, sid, i as u8 + 1, &id_named(&figs, name)).await;
    }
    wait_slot(&phone, 8, "Jet-Vac").await;
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(3), |s| {
        card_slots(s).len() == 8
    })
    .await
    .expect("eight cards");
    for (i, name) in names.iter().enumerate() {
        assert_eq!(card(&snap, i as u8 + 1).unwrap()["texts"][0], *name);
    }
    assert_eq!(snap["empty_slots"], json!([]));
    assert_eq!(snap["cards_outside"], json!([]), "{snap}");
    assert_eq!(snap["overlapping"], 0, "{snap}");
    let window = &snap["window"];
    assert!(window[0].as_f64().unwrap() > 0.0, "{snap}");
    for c in snap["cards"].as_array().unwrap() {
        let r = c["rect"].as_array().unwrap();
        assert!(r[2].as_f64().unwrap() > 0.0 && r[3].as_f64().unwrap() > 0.0);
    }
    eprintln!("window {window}, first card {}", snap["cards"][0]["rect"]);
    phone.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn a_failed_placement_shows_no_card() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (phone, _, _) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &phone).await;
    inject_load_outcomes(
        &server.url,
        json!([{ "kind": "qt_modal", "message": "injected boom" }]),
    )
    .await
    .unwrap();

    place_named(&phone, "Eruptor").await;
    phone
        .wait_until(Duration::from_secs(8), || async {
            phone.toast_count().await.unwrap_or(0) > 0
        })
        .await
        .expect("the phone shows an error");
    phone
        .wait_for_slot_empty(1, Duration::from_secs(5))
        .await
        .expect("slot 1 back to empty");
    let toast = phone.last_toast_text().await.unwrap().unwrap_or_default();
    eprintln!("phone toast: {toast}");
    // Long enough for a card that was coming to have come.
    tokio::time::sleep(TV_WITHIN + Duration::from_millis(500)).await;
    let snap = tv(&server.url).await.unwrap();
    assert_eq!(snap["screen"], "Portal");
    assert!(card_slots(&snap).is_empty(), "{snap}");
    phone.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn figure_details_have_no_progress_and_stats_and_reset_are_off() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (phone, _, _) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &phone).await;
    phone.open_search().await.unwrap();
    phone.search("Eruptor").await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    phone.tap_figure_named("Eruptor").await.unwrap();

    // The buttons start enabled while the stats fetch is in flight (AA.9).
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .inner_text(".detail-stats-soon-label")
                .await
                .ok()
                .flatten()
                .is_some_and(|t| t.trim() == "NOT AVAILABLE YET")
        })
        .await
        .expect("NOT AVAILABLE YET strip");
    for label in ["Edit stats", "Reset to pack-fresh"] {
        let btn = phone
            .client
            .find(Locator::Css(&format!(
                ".detail-action-btn[aria-label='{label}']"
            )))
            .await
            .unwrap();
        assert!(
            btn.attr("disabled").await.unwrap().is_some(),
            "{label} should be disabled"
        );
        let title = btn.attr("title").await.unwrap().unwrap_or_default();
        assert_eq!(title, "Not available yet", "{label}");
    }
    let stat_labels = phone
        .client
        .find_all(Locator::Css(".detail-stat-l"))
        .await
        .unwrap();
    assert!(stat_labels.is_empty(), "no level, gold or playtime cells");
    let detail = phone
        .inner_text(".detail-surface")
        .await
        .unwrap()
        .unwrap_or_default()
        .to_uppercase();
    for word in ["LEVEL", "GOLD", "XP"] {
        assert!(!detail.contains(word), "details show {word}: {detail}");
    }
    phone.close().await.unwrap();
}

// ---- guacamole/shell -----------------------------------------------------

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn the_controllers_settings_are_unavailable() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    inject_profile(&server.url, "Alice", "1234", ALICE)
        .await
        .unwrap();
    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .unwrap();
    phone
        .wait_for(Locator::Css(".profile-picker"), Duration::from_secs(15))
        .await
        .unwrap();

    // Kebab → MANAGE PROFILES → the Konami gate → the grown-ups list.
    phone
        .wait_for(Locator::Css(".kebab-btn"), Duration::from_secs(5))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    phone
        .wait_until(Duration::from_secs(4), || async {
            phone
                .client
                .execute(
                    "const el = Array.from(document.querySelectorAll('button, a, div, span'))
                       .find(e => e.innerText && e.innerText.trim() === 'MANAGE PROFILES');
                     if (!el) return false; el.click(); return true;",
                    vec![],
                )
                .await
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        })
        .await
        .expect("MANAGE PROFILES");
    phone
        .wait_for(Locator::Css(".konami-gate"), Duration::from_secs(5))
        .await
        .unwrap();
    for key in [
        ".dpad-btn.up",
        ".dpad-btn.up",
        ".dpad-btn.down",
        ".dpad-btn.down",
        ".dpad-btn.left",
        ".dpad-btn.right",
        ".dpad-btn.left",
        ".dpad-btn.right",
        ".ab-btn.ab-b",
        ".ab-btn.ab-a",
    ] {
        assert!(phone.js_click(key).await.unwrap(), "{key}");
        tokio::time::sleep(Duration::from_millis(60)).await;
    }
    assert!(phone.js_click(".gate-actions .btn-submit").await.unwrap());
    phone
        .wait_for(Locator::Css(".admin-hub"), Duration::from_secs(5))
        .await
        .expect("grown-ups list");

    // The window-mode toggle: the phone says it failed and rolls back.
    let label = ".admin-display-panel .edit-toggle-label";
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone.inner_text(label).await.ok().flatten().as_deref() == Some("DESKTOP")
        })
        .await
        .expect("window mode reads DESKTOP (the e2e .env.dev)");
    assert!(
        phone
            .js_click(".admin-display-panel .edit-toggle")
            .await
            .unwrap()
    );
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .last_toast_text()
                .await
                .ok()
                .flatten()
                .is_some_and(|t| t.contains("Window mode change failed"))
        })
        .await
        .expect("the phone reports the window mode as unavailable");
    eprintln!("phone toast: {:?}", phone.last_toast_text().await.unwrap());
    phone
        .wait_until(Duration::from_secs(3), || async {
            phone.inner_text(label).await.ok().flatten().as_deref() == Some("DESKTOP")
        })
        .await
        .expect("the toggle rolls back");

    // The three settings writes, as the phone sends them: 409, nothing written.
    let client = reqwest::Client::new();
    for (path, body) in [
        ("/api/launcher/window-mode", json!({ "window_mode": "tv" })),
        ("/api/launcher/render-2x", json!({ "render_2x": true })),
        ("/api/rpcs3/settings", json!({})),
    ] {
        let resp = client
            .post(format!("{}{path}", server.url))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 409, "{path}");
    }
    // Only guacamole's own data under the run's dev-data: no controller
    // config.json or db.sqlite.
    let dev_data = server.dev_data_dir();
    let top: Vec<String> = std::fs::read_dir(&dev_data)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(top, ["guacamole"], "{}", dev_data.display());
    assert!(dev_data.join("guacamole/db.sqlite").is_file());
    phone.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn no_giants_dump_offers_no_game() {
    if guacamole_only() {
        return;
    }
    // A SWAP Force dump alone.
    let server = TestServer::spawn_guacamole(&["BLUS31076"], "").expect("spawn guacamole");
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["screen"] == "NoGiants"
    })
    .await
    .expect("TV says no Giants dump");
    assert_eq!(snap["found"], json!(["Skylanders: SWAP Force (BLUS31076)"]));
    let folders = snap["folders"].as_array().unwrap();
    assert!(
        folders.len() == 1 && folders[0].as_str().unwrap().ends_with("dumps"),
        "{snap}"
    );
    let games: Vec<Value> = reqwest::get(format!("{}/api/games", server.url))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(games.is_empty(), "{games:?}");

    let (phone, _, _) = phone_as(&server, "Alice", ALICE).await;
    phone
        .wait_for(
            Locator::Css(".game-picker .empty-msg"),
            Duration::from_secs(15),
        )
        .await
        .expect("the phone's no-games state");
    assert!(
        phone
            .client
            .find_all(Locator::Css(".game-picker .game-card"))
            .await
            .unwrap()
            .is_empty()
    );
    phone.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn profiles_are_guacamoles_own() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let profiles: Vec<Value> = reqwest::get(format!("{}/api/profiles", server.url))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(profiles.is_empty(), "a first start has none: {profiles:?}");

    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .unwrap();
    phone
        .wait_for(Locator::Css(".profile-picker"), Duration::from_secs(15))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let cards = phone
        .client
        .find_all(Locator::Css(".profile-card:not(.add)"))
        .await
        .unwrap();
    assert!(cards.is_empty(), "the picker lists no profiles");

    // Create one through the phone's wizard: name, colour, PIN, confirm.
    phone
        .wait_for(Locator::Css(".profile-card.add"), Duration::from_secs(5))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    phone
        .wait_for(Locator::Css(".edit-input"), Duration::from_secs(5))
        .await
        .unwrap();
    // The wizard starts on a random Skylander name.
    let input = phone
        .client
        .find(Locator::Css(".edit-input"))
        .await
        .unwrap();
    input.clear().await.unwrap();
    input.send_keys("Ann").await.unwrap();
    let next = ".create-profile-wizard .btn-primary";
    assert!(phone.js_click(next).await.unwrap()); // → colour
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(phone.js_click(next).await.unwrap()); // → PIN
    for _ in 0..2 {
        phone
            .wait_for(
                Locator::Css(".create-step-body .pin-hkey"),
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        for d in ["1", "2", "3", "4"] {
            let xpath =
                format!("//button[contains(@class,'pin-hkey') and normalize-space(text())='{d}']");
            phone
                .client
                .find(Locator::XPath(&xpath))
                .await
                .unwrap()
                .click()
                .await
                .unwrap();
        }
        assert!(phone.js_click(next).await.unwrap()); // → confirm, then CREATE
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    phone
        .wait_until(Duration::from_secs(8), || async {
            phone
                .client
                .find(Locator::Css(".profile-card:not(.add)"))
                .await
                .is_ok()
        })
        .await
        .expect("the new profile is listed");
    let profiles: Vec<Value> = reqwest::get(format!("{}/api/profiles", server.url))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0]["display_name"], "Ann");

    // PIN 4321 is refused, 1234 opens the profile.
    let enter = |pin: &'static str| {
        let phone = &phone;
        async move {
            phone
                .wait_for(Locator::Css(".pin-keypad-heraldic"), Duration::from_secs(5))
                .await
                .unwrap();
            for d in pin.chars() {
                let xpath = format!(
                    "//button[contains(@class,'pin-hkey') and normalize-space(text())='{d}']"
                );
                phone
                    .client
                    .find(Locator::XPath(&xpath))
                    .await
                    .unwrap()
                    .click()
                    .await
                    .unwrap();
            }
        }
    };
    phone
        .client
        .find(Locator::Css(".profile-card:not(.add)"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    enter("4321").await;
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .last_toast_text()
                .await
                .ok()
                .flatten()
                .is_some_and(|t| t.contains("Unlock failed"))
        })
        .await
        .expect("PIN 4321 is refused");
    assert!(
        phone
            .client
            .find(Locator::Css(".game-picker"))
            .await
            .is_err()
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    enter("1234").await;
    phone
        .wait_for(
            Locator::Css(".game-picker .game-card"),
            Duration::from_secs(10),
        )
        .await
        .expect("PIN 1234 opens the game picker");
    assert!(server.dev_data_dir().join("guacamole/db.sqlite").is_file());
    assert!(!server.dev_data_dir().join("db.sqlite").exists());
    phone.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn a_phone_paired_elsewhere_scans_guacamoles_code() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    inject_profile(&server.url, "Alice", "1234", ALICE)
        .await
        .unwrap();
    // A key guacamole never issued: the controller's, say.
    let stale = format!("{}/#k={}", server.url, "ab".repeat(32));
    let phone = Phone::new(&stale, &server.chromedriver_url).await.unwrap();
    phone
        .wait_for(Locator::Css(".pairing-required"), Duration::from_secs(15))
        .await
        .expect("the pairing screen");

    // Scan guacamole's join code: the QR's URL carries its key.
    phone.client.goto("about:blank").await.unwrap();
    phone
        .client
        .goto(&server.phone_url().await.unwrap())
        .await
        .unwrap();
    phone
        .wait_for(Locator::Css(".profile-picker"), Duration::from_secs(15))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        phone
            .client
            .find(Locator::Css(".pairing-required"))
            .await
            .is_err(),
        "pairing screen gone after the scan"
    );
    // Signed commands now land: the PIN, then picking Giants.
    phone
        .wait_for(
            Locator::Css(".profile-card:not(.add)"),
            Duration::from_secs(8),
        )
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    phone
        .wait_for(Locator::Css(".pin-keypad-heraldic"), Duration::from_secs(5))
        .await
        .unwrap();
    for d in ["1", "2", "3", "4"] {
        let xpath =
            format!("//button[contains(@class,'pin-hkey') and normalize-space(text())='{d}']");
        phone
            .client
            .find(Locator::XPath(&xpath))
            .await
            .unwrap()
            .click()
            .await
            .unwrap();
    }
    pick_giants(&server, &phone).await;
    phone.close().await.unwrap();
}
