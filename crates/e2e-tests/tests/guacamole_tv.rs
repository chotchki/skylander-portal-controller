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
    E2E_DAMAGED, E2E_FLOOR, E2E_GOLD_CAP, E2E_LEVEL_XP, E2E_MODELLED, E2E_SCRIPTS, E2E_START,
    E2E_WALL, GIANTS_SERIAL, Phone, TestServer, fire_kaos_swap, gaits_tv, guacamole_only,
    inject_load_outcomes, inject_profile, pad_tv, press_tv, scripts_tv, set_session_profile, tv,
    wait_for_tv,
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
    // The start's pass (AE.25) holds the TV on its getting-ready screen
    // until every census key is compiled, chapter 1's props' since AG.25
    // (144 keys), however soon the phone gets to its portal; a debug build
    // under load takes most of a minute.
    wait_for_tv(&server.url, Duration::from_secs(90), |s| {
        s["prepare"]["state"] == "done"
    })
    .await
    .expect("the start's pass");
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

/// The request the phone's place button sends, for the step the phone's own
/// UI won't take (a given slot).
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
/// Searches, opens the figure and presses PLACE; returns when PLACE was clicked.
async fn place_named(phone: &Phone, name: &str) -> Instant {
    phone.search_named(name).await.unwrap();
    phone.tap_figure_named(name).await.unwrap();
    let place = phone
        .wait_for(Locator::Css(".detail-btn-primary"), Duration::from_secs(5))
        .await
        .unwrap();
    let clicked = Instant::now();
    place.click().await.unwrap();
    clicked
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
    // A lower bound on the TV's lag: `since` is when the phone poll (every
    // 100 ms) saw the figure, so the TV has usually drawn it already.
    eprintln!(
        "TV showing {name} in slot {slot} {} ms after the phone poll saw it",
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
    assert_eq!(snap["players"], 0, "{snap}");
    assert_eq!(snap["join_qr"], true, "the join QR is drawn: {snap}");
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

    let clicked = place_named(&phone, "Eruptor").await;
    let shown = wait_slot(&phone, 1, "Eruptor").await;
    let snap = tv_card(&server, 1, "Eruptor", shown).await;
    eprintln!(
        "PLACE click to TV card: {} ms",
        clicked.elapsed().as_millis()
    );
    assert_eq!(card_slots(&snap), [1], "{snap}");
    let eruptor = card(&snap, 1).unwrap();
    assert_eq!(texts(eruptor), ["Eruptor", "Fire"], "name and element only");
    assert_eq!(eruptor["figure_id"], "000009-0000");
    assert_eq!(eruptor["portrait"], true, "Eruptor has a thumb.png");
    assert_pip(eruptor, "A", ALICE, &pid);
    // The drawn lines only: the whole card's JSON carries the profile's ULID,
    // which can spell "xp".
    let drawn = texts(eruptor).join(" ").to_lowercase();
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
    let (bob, bob_id, _) = phone_as(&server, "Bob", BOB).await;
    bob.wait_for_portal(Duration::from_secs(15)).await.unwrap();

    place_named(&alice, "Eruptor").await;
    let shown = wait_slot(&alice, 1, "Eruptor").await;
    tv_card(&server, 1, "Eruptor", shown).await;
    // Alice's Eruptor leaves Bob's own in his box (AA.18): his phone places it.
    wait_slot(&bob, 1, "Eruptor").await;
    place_named(&bob, "Eruptor").await;
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
    // one Kaos can take from her. Her Spyro comes off slot 1 first, since
    // guacamole refuses a load over another profile's figure (AB.22).
    let resp = reqwest::Client::new()
        .post(format!("{}/api/portal/slot/1/clear", server.url))
        .header("x-session-id", sid.to_string())
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 202, "{}", resp.text().await.unwrap());
    wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        card_slots(s).is_empty()
    })
    .await
    .expect("Spyro's card goes");
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

// ---- guacamole/figure-turntable (AE.13) ------------------------------------

/// The request the phone's remove button sends, for slot `slot`.
async fn clear(server: &TestServer, sid: u64, slot: u8) {
    let resp = reqwest::Client::new()
        .post(format!("{}/api/portal/slot/{slot}/clear", server.url))
        .header("x-session-id", sid.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 202, "{}", resp.text().await.unwrap());
}

/// The fixture's one figure with a model turns on its card within a second
/// of the phone showing it (it has no intro); removing it takes the card and
/// its turntable away; a figure the extract has no model for, and an item,
/// keep their portraits.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn a_modelled_figure_turns_on_its_card_and_leaves_with_it() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (phone, _, sid) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &phone).await;
    let figs = figures(&server, sid).await;
    let gill = format!("{E2E_MODELLED:06x}-0000");
    assert_eq!(id_named(&figs, "Gill Grunt"), gill);

    load(&server, sid, 1, &gill).await;
    let shown = wait_slot(&phone, 1, "Gill Grunt").await;
    let (snap, at) = wait_for_tv(&server.url, TV_WITHIN, |s| {
        card(s, 1).is_some_and(|c| c["state"] == "model")
    })
    .await
    .unwrap_or_else(|e| panic!("Gill Grunt's model wasn't on his card within 1 s: {e}"));
    eprintln!(
        "Gill Grunt's model on his card {} ms after the phone poll saw him",
        at.duration_since(shown).as_millis()
    );
    let gill_card = card(&snap, 1).unwrap();
    assert_eq!(texts(gill_card), ["Gill Grunt", "Water"]);
    assert_eq!(gill_card["portrait"], true, "the plate shows an image");
    assert_eq!(gill_card["reason"], Value::Null);
    // The model draws in the bezel's plate, inside his card.
    let rect = |v: &Value| -> Vec<f64> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_f64().unwrap())
            .collect()
    };
    let (plate, card_rect) = (rect(&gill_card["plate"]), rect(&gill_card["rect"]));
    assert!(plate[2] > 0.0 && plate[3] > 0.0, "{gill_card}");
    assert!(
        plate[0] >= card_rect[0]
            && plate[1] >= card_rect[1]
            && plate[0] + plate[2] <= card_rect[0] + card_rect[2]
            && plate[1] + plate[3] <= card_rect[1] + card_rect[3],
        "{gill_card}"
    );

    // Spyro's model isn't in this extract, and Sky-Iron Shield is an item.
    for (slot, name, why) in [
        (2, "Spyro", "016_e2e.ron"),
        (3, "Sky-Iron Shield", "no character model on the disc"),
    ] {
        load(&server, sid, slot, &id_named(&figs, name)).await;
        let shown = wait_slot(&phone, slot, name).await;
        let (snap, _) = wait_for_tv(
            &server.url,
            TV_WITHIN.saturating_sub(shown.elapsed()),
            |s| card(s, slot).is_some_and(|c| c["state"] == "portrait"),
        )
        .await
        .unwrap_or_else(|e| panic!("{name} didn't settle on its portrait: {e}"));
        let c = card(&snap, slot).unwrap();
        assert!(c["reason"].as_str().unwrap().contains(why), "{c}");
        assert_eq!(c["portrait"], true, "its portrait on the plate: {c}");
        assert_eq!(
            card(&snap, 1).unwrap()["state"],
            "model",
            "slot 1 untouched"
        );
    }

    clear(&server, sid, 1).await;
    phone
        .wait_for_slot_empty(1, Duration::from_secs(5))
        .await
        .unwrap();
    let gone = Instant::now();
    let (after, _) = wait_for_tv(&server.url, TV_WITHIN, |s| card(s, 1).is_none())
        .await
        .expect("Gill Grunt's card leaves within 1 s");
    eprintln!(
        "card and turntable gone {} ms after the phone",
        gone.elapsed().as_millis()
    );
    assert_eq!(card_slots(&after), [2, 3]);
    // Placed again, he turns again: the old scene went with the card.
    load(&server, sid, 1, &gill).await;
    let shown = wait_slot(&phone, 1, "Gill Grunt").await;
    wait_for_tv(
        &server.url,
        TV_WITHIN.saturating_sub(shown.elapsed()),
        |s| card(s, 1).is_some_and(|c| c["state"] == "model"),
    )
    .await
    .expect("his model again");
    phone.close().await.unwrap();
}

/// AG.19: picking Giants with its extract current loads chapter 1 behind
/// the portal, which stays up: nothing takes the TV into the level yet.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn the_pick_loads_the_level_behind_the_portal() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (phone, _, _) = phone_as(&server, "Alice", ALICE).await;
    let before = tv(&server.url).await.expect("the TV's mirror");
    assert_eq!(
        before["level"]["state"], "idle",
        "nothing loads before the pick"
    );
    pick_giants(&server, &phone).await;
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["level"]["state"] == "ready" || s["level"]["state"] == "failed"
    })
    .await
    .expect("the level loads");
    let level = &snap["level"];
    assert_eq!(level["state"], "ready", "{level}");
    assert_eq!(level["level"], "level_213_tutotial");
    // The fixture's chapter 1 draws nothing: no scene, so no keys.
    assert_eq!(
        (level["segments"].as_u64(), level["keys"].as_u64()),
        (Some(0), Some(0))
    );
    assert_eq!(level["shown"], false);
    assert_eq!(snap["screen"], "Portal", "the portal stays up");
    // AH.22: the script part links on a task of its own beside the load.
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        !s["level"]["scripts"].is_null() || !s["level"]["scripts_reason"].is_null()
    })
    .await
    .expect("the scripts link");
    let scripts = &snap["level"]["scripts"];
    assert_eq!(scripts["files"], 5, "{}", snap["level"]);
    assert_eq!(scripts["scripts"].as_u64(), Some(E2E_SCRIPTS as u64));
    assert!(snap["level"]["scripts_reason"].is_null());
    phone.close().await.unwrap();
}

/// The hero's pivot off `/api/_test/tv`.
fn hero_at(snap: &Value) -> [f64; 3] {
    let p = &snap["hero"]["position"];
    [0, 1, 2].map(|i| {
        p[i].as_f64()
            .unwrap_or_else(|| panic!("no position: {snap}"))
    })
}

/// AG.21: with the level loaded, placing Eruptor takes the TV into it with
/// him at the start, facing 0°, settled on the floor (5 mm up); with no pad
/// it waits for one, a press on a posted pad takes him over, and its stick
/// walks him against the camera's view (15°) until the made-up level's wall
/// holds him 5 mm off it (his 0.8644 m sphere, the triangles' 0.05 m).
/// AG.30: he stands in his idle, runs pushed along the wall at full stick
/// (his walk at 21.5 ft/s, the run at 102.4 %), every step's run share the
/// speed's that step, and comes back to his idle let go. AG.50: the
/// controller message and Start's overlay draw over the level's camera.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn placing_a_character_enters_the_level_at_its_start() {
    if guacamole_only() {
        return;
    }
    let server = TestServer::spawn_guacamole_walking().expect("spawn guacamole");
    let (phone, _, _) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &phone).await;
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["level"]["state"] == "ready" || s["level"]["state"] == "failed"
    })
    .await
    .expect("the level loads");
    assert_eq!(snap["level"]["state"], "ready", "{}", snap["level"]);
    assert_eq!(snap["mode"], "portal");
    let start = snap["level"]["start"].clone();
    for (i, want) in E2E_START.iter().enumerate() {
        let read = start[i].as_f64().unwrap_or(f64::NAN);
        assert!(
            (read - f64::from(*want)).abs() < 1e-5,
            "the waypoint, read: {}",
            snap["level"]
        );
    }
    assert_eq!(snap["level"]["triangles"], 4);

    place_named(&phone, "Eruptor").await;
    wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["mode"] != "portal"
    })
    .await
    .expect("the TV enters the level");
    // No pad: the level waits for one from its first frame of input.
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(2), |s| {
        s["mode"] == "paused"
    })
    .await
    .expect("the level waits for a pad");
    assert_eq!(snap["input"]["pause"], "controller");
    assert_eq!(snap["input"]["layer"], "level");
    assert_eq!(snap["level"]["shown"], true);
    // Its message over the level, not under it (AG.50).
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(2), |s| {
        s["input"]["overlay"]["kind"] == "controller"
    })
    .await
    .expect("the controller message");
    assert_eq!(
        snap["input"]["overlay"]["over_level"], true,
        "{}",
        snap["input"]
    );
    assert_eq!(snap["screen"], "Portal");
    let hero = &snap["hero"];
    assert_eq!(hero["load_file"], "009_Eruptor", "{hero}");
    assert_eq!(hero["slot"], 1);
    assert_eq!(hero["heading"], 0.0);
    assert_eq!(hero["gait"]["clip"], "idle", "{hero}");
    assert_eq!(
        hero["gait"]["weights"],
        json!([1.0, 0.0, 0.0, 0.0]),
        "{hero}"
    );
    let at = hero_at(&snap);
    assert_eq!(
        (at[0], at[2]),
        (start[0].as_f64().unwrap(), start[2].as_f64().unwrap())
    );
    let above = at[1] - f64::from(E2E_FLOOR);
    assert!((above - 0.005).abs() < 1e-3, "{above} m above the floor");
    // The camera the TV draws through, at camera 0's rest pose off his feet
    // (gameplay-camera's The camera starts behind the hero).
    let camera = &hero["camera"];
    let eye = [0, 1, 2].map(|i| camera["eye"][i].as_f64().unwrap_or(f64::NAN));
    let off = [eye[0] - at[0], eye[1] - at[1], eye[2] - at[2]];
    for (got, want) in off.iter().zip([-3.679547, 8.883567, -13.732257]) {
        assert!((got - want).abs() < 1e-3, "the eye off his feet: {camera}");
    }
    let num = |k: &str| camera[k].as_f64().unwrap_or(f64::NAN);
    assert!((num("heading") - 15.0).abs() < 1e-3, "{camera}");
    assert!((num("pitch") + 23.360138).abs() < 1e-3, "{camera}");
    assert!((num("fov_y") - 53.96).abs() < 1e-3, "{camera}");
    assert!((num("near") - 0.1).abs() < 1e-6, "{camera}");
    assert!((num("far") - 243.84).abs() < 1e-3, "{camera}");

    // A press on a posted pad takes him over; its stick walks him.
    pad_tv(
        &server.url,
        json!({"pad": 0, "connect": {"name": "e2e pad"}}),
    )
    .await
    .unwrap();
    pad_tv(&server.url, json!({"pad": 0, "press": "south"}))
        .await
        .unwrap();
    wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        s["mode"] == "level"
    })
    .await
    .expect("the pad takes him over");
    // Start pauses under its overlay, over the level too; the bottom button
    // plays on.
    pad_tv(&server.url, json!({"pad": 0, "press": "start"}))
        .await
        .unwrap();
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        s["input"]["overlay"]["kind"] == "paused"
    })
    .await
    .expect("Start's overlay");
    assert_eq!(snap["mode"], "paused");
    assert_eq!(
        snap["input"]["overlay"]["over_level"], true,
        "{}",
        snap["input"]
    );
    pad_tv(&server.url, json!({"pad": 0, "press": "south"}))
        .await
        .unwrap();
    wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        s["mode"] == "level" && s["input"]["overlay"].is_null()
    })
    .await
    .expect("the bottom button plays on");
    pad_tv(&server.url, json!({"pad": 0, "stick": [0.0, 1.0]}))
        .await
        .unwrap();
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        let p = hero_at(s);
        (p[2] - at[2]).hypot(p[0] - at[0]) > 1.0
    })
    .await
    .expect("he walks");
    let p = hero_at(&snap);
    let heading = (p[0] - at[0]).atan2(p[2] - at[2]).to_degrees();
    assert!((heading - 15.0).abs() < 0.5, "along the view: {heading}°");
    // On to the wall, then along it: he never goes past 5 mm off it.
    let wall = f64::from(E2E_START[2] + E2E_WALL);
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(8), |s| {
        hero_at(s)[0] - at[0] > 3.0
    })
    .await
    .expect("he slides along the wall");
    let p = hero_at(&snap);
    let gap = wall - (p[2] + 0.8644 + 0.05);
    assert!((gap - 0.005).abs() < 2e-3, "{gap} m off the wall at {p:?}");
    // Pushed along it at full stick his walk holds 21.5 ft/s: he runs.
    let gait = &snap["hero"]["gait"];
    let num = |v: &Value| v.as_f64().unwrap_or(f64::NAN);
    assert_eq!(gait["clip"], "run", "{gait}");
    assert_eq!(gait["weights"], json!([0.0, 0.0, 1.0, 0.0]), "{gait}");
    assert!((num(&gait["speed_ft"]) - 21.5).abs() < 1e-3, "{gait}");
    assert!(
        (num(&gait["rates"][1]) - 21.5 / 21.0017).abs() < 1e-3,
        "{gait}"
    );
    // Step by step, the run's share is the speed's that step: all walk to
    // 12.0019 ft/s, all run from 21.0017.
    let log = gaits_tv(&server.url, 0).await.unwrap();
    let steps = log["steps"].as_array().unwrap();
    assert!(steps.len() > 60, "{} steps", steps.len());
    let (walk, run) = (3.658 * 3.281, 6.401 * 3.281);
    for step in steps {
        let speed = num(&step["speed_ft"]);
        let share = ((speed - walk) / (run - walk)).clamp(0.0, 1.0);
        assert!((num(&step["run_share"]) - share).abs() < 1e-4, "{step}");
        let total: f64 = (0..4).map(|i| num(&step["weights"][i])).sum();
        assert!((total - 1.0).abs() < 1e-5, "{step}");
    }
    // The walk had him alone before the run joined (his idle the heaviest
    // through a full-stick set-off: it slides out over 0.25 s).
    assert!(
        steps.iter().any(|s| num(&s["speed_ft"]) > 0.0
            && num(&s["weights"][1]) > 0.0
            && num(&s["weights"][2]) == 0.0),
        "he walked first"
    );
    pad_tv(&server.url, json!({"pad": 0, "stick": [0.0, 0.0]}))
        .await
        .unwrap();
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        s["hero"]["gait"]["weights"][0] == 1.0
    })
    .await
    .expect("let go, back in his idle");
    assert_eq!(snap["hero"]["gait"]["clip"], "idle");
    phone.close().await.unwrap();
}

/// The scripts' steps after `since`, until `done` holds over them, up to
/// `timeout`; all of them then.
async fn scripts_until(
    url: &str,
    since: u64,
    timeout: Duration,
    done: impl Fn(&[Value]) -> bool,
) -> Vec<Value> {
    let deadline = Instant::now() + timeout;
    loop {
        let log = scripts_tv(url, since).await.unwrap();
        let steps = log["steps"].as_array().cloned().unwrap_or_default();
        if done(&steps) {
            return steps;
        }
        assert!(Instant::now() < deadline, "the scripts' steps: {log}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The names of what the steps spawned, or took off.
fn named(steps: &[Value], what: &str) -> Vec<String> {
    steps
        .iter()
        .flat_map(|s| s[what].as_array().cloned().unwrap_or_default())
        .map(|c| c["name"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// The palette slots the steps started on the hero.
fn started(steps: &[Value]) -> Vec<u64> {
    steps
        .iter()
        .flat_map(|s| s["started"].as_array().cloned().unwrap_or_default())
        .filter_map(|q| q["slot"].as_u64())
        .collect()
}

/// AH.24: a press throws, as `/api/_test/tv` reports. Over AH.24's synthetic
/// attack (written with AH.13's writer, our numbers): placed and taken over
/// by a posted pad, Eruptor's scripts join and his health reads off them
/// (120 of 150) with his figure's experience and gold (0, a fresh one); the
/// bottom button starts his throw's clip and spawns the ball, which falls
/// onto the made-up floor and is taken off; held it throws once, the
/// attack held reading in the input's; a right shoulder starts the second
/// attack's clip and awards a point, which his figure's words take and the
/// TV and Alice's row show (AH.23); the top button does nothing.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn a_press_throws_from_his_scripts() {
    if guacamole_only() {
        return;
    }
    let server = TestServer::spawn_guacamole_fighting().expect("spawn guacamole");
    let (phone, alice_id, _) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &phone).await;
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["level"]["state"] == "ready" && !s["level"]["scripts"].is_null()
    })
    .await
    .expect("the level and its scripts");
    assert_eq!(snap["level"]["scripts"]["scripts"], E2E_SCRIPTS);
    place_named(&phone, "Eruptor").await;
    wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["mode"] == "paused"
    })
    .await
    .expect("the level waits for a pad");
    pad_tv(
        &server.url,
        json!({"pad": 0, "connect": {"name": "e2e pad"}}),
    )
    .await
    .unwrap();
    pad_tv(&server.url, json!({"pad": 0, "press": "east"}))
        .await
        .unwrap();
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        s["mode"] == "level" && s["hero"]["scripts"] == "joined"
    })
    .await
    .expect("his scripts join");
    let hero = &snap["hero"];
    assert_eq!(
        (&hero["health"], &hero["health_max"]),
        (&json!(120), &json!(150)),
        "{hero}"
    );
    assert_eq!(
        (&hero["experience"], &hero["gold"]),
        (&json!(0), &json!(0)),
        "{hero}"
    );
    assert_eq!(
        snap["input"]["hero_actions"],
        json!([false, false, false, false])
    );

    // A press: the throw's clip, the ball, and the ball off once it lands.
    let since = scripts_tv(&server.url, u64::MAX).await.unwrap()["last"]
        .as_u64()
        .unwrap();
    pad_tv(&server.url, json!({"pad": 0, "press": "south"}))
        .await
        .unwrap();
    let steps = scripts_until(&server.url, since, Duration::from_secs(5), |s| {
        named(s, "gone").iter().any(|n| n == "009_LavaProjectile")
    })
    .await;
    assert_eq!(started(&steps), [20], "his throw's slot");
    assert_eq!(named(&steps, "spawned"), ["009_LavaProjectile"]);
    let spawned_at = steps
        .iter()
        .find(|s| !s["spawned"].as_array().unwrap().is_empty())
        .unwrap()["step"]
        .as_u64()
        .unwrap();
    let thrown_at = steps
        .iter()
        .find(|s| !s["started"].as_array().unwrap().is_empty())
        .unwrap()["step"]
        .as_u64()
        .unwrap();
    assert_eq!(spawned_at, thrown_at, "the ball with the press's step");
    let gone = steps
        .iter()
        .flat_map(|s| s["gone"].as_array().cloned().unwrap_or_default())
        .find(|c| c["name"] == "009_LavaProjectile")
        .unwrap();
    // Its solid resting on the floor puts its pivot on it: not still in the
    // air, and not through it.
    let y = gone["at"][1].as_f64().unwrap();
    let above = y - f64::from(E2E_FLOOR);
    assert!(
        above.abs() < 0.05,
        "the ball came down on the floor: {above} m over it"
    );

    // Held: one throw, the attack held as the input reads it.
    let since = scripts_tv(&server.url, u64::MAX).await.unwrap()["last"]
        .as_u64()
        .unwrap();
    pad_tv(&server.url, json!({"pad": 0, "hold": "south"}))
        .await
        .unwrap();
    wait_for_tv(&server.url, Duration::from_secs(2), |s| {
        s["input"]["hero_actions"] == json!([true, false, false, false])
    })
    .await
    .expect("the attack held");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    pad_tv(&server.url, json!({"pad": 0, "release": "south"}))
        .await
        .unwrap();
    let steps = scripts_until(&server.url, since, Duration::from_secs(5), |s| {
        named(s, "gone").iter().any(|n| n == "009_LavaProjectile")
    })
    .await;
    assert_eq!(started(&steps), [20], "held, one throw");

    // A right shoulder: the second attack's clip, and its award (a point,
    // through the synthetic `Game - Award Experience`) in his figure's
    // words, read back onto the TV and written to Alice's row (AH.23). The
    // top button: nothing.
    let since = scripts_tv(&server.url, u64::MAX).await.unwrap()["last"]
        .as_u64()
        .unwrap();
    pad_tv(&server.url, json!({"pad": 0, "press": "r1"}))
        .await
        .unwrap();
    let steps = scripts_until(&server.url, since, Duration::from_secs(5), |s| {
        !started(s).is_empty()
    })
    .await;
    assert_eq!(started(&steps), [32]);
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(2), |s| {
        s["hero"]["experience"] == json!(1)
    })
    .await
    .expect("the award on the TV");
    let figure = snap["hero"]["figure_id"].as_str().unwrap().to_owned();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let (status, body) = stats(&server, &alice_id, &figure).await;
        if status == 200 && body["xp"] == json!(1) {
            break;
        }
        assert!(Instant::now() < deadline, "the row: {status} {body}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let since = scripts_tv(&server.url, u64::MAX).await.unwrap()["last"]
        .as_u64()
        .unwrap();
    pad_tv(&server.url, json!({"pad": 0, "press": "north"}))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let steps = scripts_until(&server.url, since, Duration::from_secs(1), |_| true).await;
    assert!(started(&steps).is_empty() && named(&steps, "spawned").is_empty());
    phone.close().await.unwrap();
}

/// AH.27: a press breaks the synthetic barrel and its coin's gold reaches
/// the phone. Over AH.27's synthetic breakable (written with AH.13's
/// writer, our numbers: a barrel 3 m ahead of the start, a prop for its
/// solid, beside AH.24's attack): Alice's Eruptor placed and taken over,
/// his scripts joined, the barrel standing; the bottom button throws, the
/// ball takes the barrel off (its prop down, `/api/_test/tv`'s props
/// listing it taken off) and goes with it, the coin it drops comes out and
/// is taken, his figure's gold on the TV (3), in Alice's row and on her
/// phone's details strip, which had read 0.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn a_press_breaks_the_barrel_and_its_coins_gold_reaches_the_phone() {
    use guac_formats::script::builder::{ATTACK_BALL, SMASH_BARREL, SMASH_COIN, SMASH_GOLD};
    if guacamole_only() {
        return;
    }
    let server = TestServer::spawn_guacamole_smashing().expect("spawn guacamole");
    let (phone, alice_id, _) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &phone).await;
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["level"]["state"] == "ready" && !s["level"]["scripts"].is_null()
    })
    .await
    .expect("the level and its scripts");
    assert_eq!(snap["level"]["scripts"]["scripts"], E2E_SCRIPTS);
    place_named(&phone, "Eruptor").await;
    wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["mode"] == "paused"
    })
    .await
    .expect("the level waits for a pad");
    pad_tv(
        &server.url,
        json!({"pad": 0, "connect": {"name": "e2e pad"}}),
    )
    .await
    .unwrap();
    pad_tv(&server.url, json!({"pad": 0, "press": "east"}))
        .await
        .unwrap();
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        s["mode"] == "level" && s["hero"]["scripts"] == "joined"
    })
    .await
    .expect("his scripts join");
    let props = &snap["level"]["props"];
    assert!(
        props["names"]
            .as_array()
            .unwrap()
            .contains(&json!(SMASH_BARREL)),
        "the barrel stands: {props}"
    );
    assert_eq!(props["taken_off"], json!([]));
    assert_eq!(snap["hero"]["gold"], json!(0));
    let figure = snap["hero"]["figure_id"].as_str().unwrap().to_owned();
    open_details(&phone, "Eruptor").await;
    wait_strip_gold(&phone, "0").await;

    // The press: the ball, the barrel off, the coin out and taken.
    let since = scripts_tv(&server.url, u64::MAX).await.unwrap()["last"]
        .as_u64()
        .unwrap();
    pad_tv(&server.url, json!({"pad": 0, "press": "south"}))
        .await
        .unwrap();
    let gold = SMASH_GOLD as u64;
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        s["level"]["props"]["taken_off"] == json!([SMASH_BARREL]) && s["hero"]["gold"] == gold
    })
    .await
    .expect("the barrel broken and its coin's gold on the TV");
    let props = &snap["level"]["props"];
    assert!(
        !props["names"]
            .as_array()
            .unwrap()
            .contains(&json!(SMASH_BARREL)),
        "its prop down: {props}"
    );
    let steps = scripts_until(&server.url, since, Duration::from_secs(2), |s| {
        named(s, "gone").iter().any(|n| n == SMASH_COIN)
    })
    .await;
    assert_eq!(named(&steps, "spawned"), [ATTACK_BALL, SMASH_COIN]);
    let mut gone = named(&steps, "gone");
    gone.sort();
    assert_eq!(gone, [ATTACK_BALL, SMASH_COIN]);
    // The ball went at the barrel, over the floor, short of its middle.
    let ball = steps
        .iter()
        .flat_map(|s| s["gone"].as_array().cloned().unwrap_or_default())
        .find(|c| c["name"] == ATTACK_BALL)
        .unwrap();
    let at = &ball["at"];
    assert!(
        at[1].as_f64().unwrap() - f64::from(E2E_FLOOR) > 0.3,
        "over the floor: {at}"
    );
    assert!(
        at[2].as_f64().unwrap() < f64::from(E2E_START[2]) + 3.0,
        "short of the barrel's middle: {at}"
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let (status, body) = stats(&server, &alice_id, &figure).await;
        if status == 200 && body["gold"] == json!(gold) {
            break;
        }
        assert!(Instant::now() < deadline, "the row: {status} {body}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    wait_strip_gold(&phone, &gold.to_string()).await;
    phone.close().await.unwrap();
}

/// Waits up to 5 s for the details' stats strip to read `want` gold.
async fn wait_strip_gold(phone: &Phone, want: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let read = strip(phone).await;
        if read.get(1).map(String::as_str) == Some(want) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the strip read {read:?}, its gold never {want}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The distance across the ground between two of `hero_at`'s points.
fn ground_apart(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

/// The TV's alert's kind, `null` when none shows.
fn alert_kind(snap: &Value) -> &Value {
    &snap["alert"]["kind"]
}

/// AG.29 and AG.63 on the walking fixture, two phones: an item placed
/// first doesn't start the level; Eruptor and Tree Rex placed before the
/// start hold the portal view under the too-many alert instead of the
/// level, and once Tree Rex comes off the level shows with Eruptor at its
/// start; while the level shows the TV reports no cards and the small join
/// code stands in its corner. Mid-level: Bob's Eruptor beside Alice's holds
/// the level under the too-many alert, Alice's unmoved with the stick
/// pushed; Alice's off, Bob's comes in where she stood and walks on;
/// Bob's off, the TV asks for a Skylander, the level held; Alice's placed
/// again, she comes in where he stood and the level carries on.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn the_portal_holds_the_level_and_a_hero_comes_back_in() {
    if guacamole_only() {
        return;
    }
    let server = TestServer::spawn_guacamole_walking().expect("spawn guacamole");
    let (alice, _, alice_sid) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &alice).await;
    wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["level"]["state"] == "ready"
    })
    .await
    .expect("the level loads");
    let figs = figures(&server, alice_sid).await;
    let (shield, eruptor, tree_rex) = (
        id_named(&figs, "Sky-Iron Shield"),
        id_named(&figs, "Eruptor"),
        id_named(&figs, "Tree Rex"),
    );

    // An item first: the portal view and its card, no hero.
    load(&server, alice_sid, 1, &shield).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let snap = tv(&server.url).await.unwrap();
    assert_eq!(snap["mode"], "portal", "{snap}");
    assert!(snap["hero"].is_null() && snap["alert"].is_null(), "{snap}");
    assert_eq!(card_slots(&snap), [1]);

    // Two characters before the start: held under the too-many alert.
    load(&server, alice_sid, 2, &eruptor).await;
    load(&server, alice_sid, 3, &tree_rex).await;
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        alert_kind(s) == "too_many_skylanders"
    })
    .await
    .expect("the too-many alert at the start");
    assert_eq!(snap["mode"], "portal", "the level not entered: {snap}");
    assert_eq!(snap["alert"]["full_screen"], true, "{}", snap["alert"]);
    assert_eq!(snap["join_corner"], true, "{snap}");
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert_eq!(tv(&server.url).await.unwrap()["mode"], "portal");
    clear(&server, alice_sid, 3).await;
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["mode"] != "portal"
    })
    .await
    .expect("Eruptor enters once Tree Rex is off");
    assert!(snap["alert"].is_null(), "{}", snap["alert"]);
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        s["hero"]["position"].is_array()
    })
    .await
    .unwrap();
    assert_eq!(snap["hero"]["slot"], 2);
    let start = hero_at(&snap);
    assert!(
        (start[0] - f64::from(E2E_START[0])).abs() < 1e-4
            && (start[2] - f64::from(E2E_START[2])).abs() < 1e-4,
        "at the start: {start:?}"
    );
    // The level takes the TV: no cards, the join code in its corner.
    assert!(
        snap["cards"].as_array().unwrap().is_empty(),
        "{}",
        snap["cards"]
    );
    assert_eq!(snap["join_corner"], true);
    assert_eq!(snap["join_qr"], true, "the corner's QR");

    // A pad takes her over and walks her.
    pad_tv(
        &server.url,
        json!({"pad": 0, "connect": {"name": "e2e pad"}}),
    )
    .await
    .unwrap();
    pad_tv(&server.url, json!({"pad": 0, "press": "south"}))
        .await
        .unwrap();
    wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        s["mode"] == "level"
    })
    .await
    .expect("the pad takes her over");
    pad_tv(&server.url, json!({"pad": 0, "stick": [1.0, 0.3]}))
        .await
        .unwrap();
    wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        ground_apart(hero_at(s), start) > 1.5
    })
    .await
    .expect("she walks");

    // Bob's Eruptor beside her: the hold, her still under the pushed stick.
    let (bob, _, bob_sid) = phone_as(&server, "Bob", BOB).await;
    let placed = Instant::now();
    load(&server, bob_sid, 5, &eruptor).await;
    let (snap, at) = wait_for_tv(&server.url, Duration::from_secs(3), |s| {
        s["mode"] == "held" && alert_kind(s) == "too_many_skylanders"
    })
    .await
    .expect("the too-many hold mid-level");
    eprintln!(
        "held {} ms after the load was sent",
        at.duration_since(placed).as_millis()
    );
    assert!(at.duration_since(placed) < Duration::from_millis(1500));
    assert_eq!(snap["alert"]["over_level"], true, "{}", snap["alert"]);
    // The frame the hold begins still runs the steps already due (AG.20).
    tokio::time::sleep(Duration::from_millis(200)).await;
    let held_at = hero_at(&tv(&server.url).await.unwrap());
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let snap = tv(&server.url).await.unwrap();
    assert_eq!(hero_at(&snap), held_at, "still under the hold");
    assert_eq!(snap["hero"]["slot"], 2);

    // Alice's off: Bob's comes in where she stood and walks on.
    pad_tv(&server.url, json!({"pad": 0, "stick": [0.0, 0.0]}))
        .await
        .unwrap();
    clear(&server, alice_sid, 2).await;
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        s["mode"] == "level" && s["hero"]["slot"] == 5 && s["hero"]["position"].is_array()
    })
    .await
    .expect("Bob's Eruptor comes in");
    let came = hero_at(&snap);
    assert!(
        ground_apart(came, held_at) < 1e-3,
        "{came:?} against {held_at:?}"
    );
    assert!(snap["alert"].is_null(), "{}", snap["alert"]);
    pad_tv(&server.url, json!({"pad": 0, "stick": [0.0, 1.0]}))
        .await
        .unwrap();
    wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        ground_apart(hero_at(s), came) > 0.5
    })
    .await
    .expect("Bob's walks on");
    pad_tv(&server.url, json!({"pad": 0, "stick": [0.0, 0.0]}))
        .await
        .unwrap();

    // Bob's off: the call for a Skylander, held, no hero.
    tokio::time::sleep(Duration::from_millis(800)).await;
    let last = hero_at(&tv(&server.url).await.unwrap());
    clear(&server, bob_sid, 5).await;
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(3), |s| {
        alert_kind(s) == "place_a_skylander"
    })
    .await
    .expect("the call for a Skylander");
    assert_eq!(snap["mode"], "held");
    assert!(snap["hero"].is_null(), "{}", snap["hero"]);
    assert_eq!(snap["alert"]["over_level"], true);
    assert_eq!(snap["level"]["shown"], true);
    let left = tv(&server.url).await.unwrap();
    assert!(left["cards"].as_array().unwrap().is_empty());

    // Alice's again: in where he stood, the level carrying on.
    load(&server, alice_sid, 2, &eruptor).await;
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        s["mode"] == "level" && s["hero"]["position"].is_array()
    })
    .await
    .expect("she comes in");
    assert!(
        ground_apart(hero_at(&snap), last) < 2e-2,
        "{:?} against {last:?}",
        hero_at(&snap)
    );
    assert!(snap["alert"].is_null());
    bob.close().await.unwrap();
    alice.close().await.unwrap();
}

/// POSTs AG.73's test hook: a later game's figure listed beside the roster.
async fn foreign_figure(server: &TestServer, figure_id: &str, name: &str) {
    let resp = reqwest::Client::new()
        .post(format!("{}/api/_test/foreign_figure", server.url))
        .json(&json!({ "figure_id": figure_id, "name": name }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 202, "{}", resp.text().await.unwrap());
}

/// AG.73 on the walking fixture: a toy Giants can't use (a later game's
/// figure, listed through the test hook; guacamole's own catalogue is the
/// disc's roster) placed from the phone holds the portal view under the
/// wrong-toy alert until it comes off; placed while Eruptor walks it holds
/// the level the same way, over it, Eruptor unmoved and the pad's buttons
/// doing nothing, and once it comes off he walks on.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn a_toy_giants_cant_use_holds_the_tv_until_it_comes_off() {
    if guacamole_only() {
        return;
    }
    let server = TestServer::spawn_guacamole_walking().expect("spawn guacamole");
    let (phone, _, sid) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &phone).await;
    wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["level"]["state"] == "ready"
    })
    .await
    .expect("the level loads");
    const FRYNO: &str = "0001c2-0000";
    foreign_figure(&server, FRYNO, "Fryno").await;
    let eruptor = id_named(&figures(&server, sid).await, "Eruptor");

    // At the start.
    let placed = Instant::now();
    load(&server, sid, 4, FRYNO).await;
    let (snap, at) = wait_for_tv(&server.url, Duration::from_secs(3), |s| {
        alert_kind(s) == "wrong_toy"
    })
    .await
    .expect("the wrong-toy alert at the start");
    assert!(at.duration_since(placed) < Duration::from_millis(1500));
    assert_eq!(snap["mode"], "portal");
    assert_eq!(snap["alert"]["full_screen"], true);
    let texts = snap["alert"]["texts"].to_string();
    assert!(texts.contains("can't be used with this game"), "{texts}");
    clear(&server, sid, 4).await;
    wait_for_tv(&server.url, Duration::from_secs(1), |s| {
        s["alert"].is_null()
    })
    .await
    .expect("gone within a second of coming off");

    // Mid-level.
    load(&server, sid, 1, &eruptor).await;
    wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["hero"]["position"].is_array()
    })
    .await
    .expect("Eruptor enters");
    pad_tv(
        &server.url,
        json!({"pad": 0, "connect": {"name": "e2e pad"}}),
    )
    .await
    .unwrap();
    pad_tv(&server.url, json!({"pad": 0, "press": "south"}))
        .await
        .unwrap();
    wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        s["mode"] == "level"
    })
    .await
    .expect("the pad takes him over");
    pad_tv(&server.url, json!({"pad": 0, "stick": [0.0, 1.0]}))
        .await
        .unwrap();
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        let p = hero_at(s);
        ground_apart(p, [f64::from(E2E_START[0]), 0.0, f64::from(E2E_START[2])]) > 1.0
    })
    .await
    .expect("he walks");
    let _ = snap;
    load(&server, sid, 4, FRYNO).await;
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(3), |s| {
        s["mode"] == "held" && alert_kind(s) == "wrong_toy"
    })
    .await
    .expect("the wrong-toy alert over the level");
    assert_eq!(snap["alert"]["over_level"], true, "{}", snap["alert"]);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let held_at = hero_at(&tv(&server.url).await.unwrap());
    for button in ["start", "south", "east"] {
        pad_tv(&server.url, json!({"pad": 0, "press": button}))
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(800)).await;
    let snap = tv(&server.url).await.unwrap();
    assert_eq!(hero_at(&snap), held_at, "unmoved under the alert");
    assert!(
        snap["input"]["pause"].is_null(),
        "Start did nothing: {}",
        snap["input"]
    );
    clear(&server, sid, 4).await;
    wait_for_tv(&server.url, Duration::from_secs(1), |s| {
        s["mode"] == "level"
    })
    .await
    .expect("the level again within a second");
    wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        ground_apart(hero_at(s), held_at) > 0.5
    })
    .await
    .expect("he walks on");
    phone.close().await.unwrap();
}

/// The scripts' log's last step.
async fn scripts_last(url: &str) -> u64 {
    scripts_tv(url, u64::MAX).await.unwrap()["last"]
        .as_u64()
        .unwrap()
}

/// AG.29 over AH.24 on the fighting fixture: an alert's hold keeps his
/// attacks from his scripts, and the hero who comes in where the last one
/// stood joins them. Eruptor's scripts joined and a press throwing, Tree
/// Rex beside him holds the level under the too-many alert: the scripts'
/// run makes no step while South, a right shoulder and Start are pressed
/// and South held, Start pausing nothing; once Tree Rex is off, South held
/// through it throws nothing until it's let go and pressed again. Eruptor
/// off, the call for a Skylander; placed again, he comes in where he stood,
/// his scripts join (120 of 150 off them) and South throws.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn an_alert_holds_his_attacks_and_the_next_hero_joins_his_scripts() {
    if guacamole_only() {
        return;
    }
    let server = TestServer::spawn_guacamole_fighting().expect("spawn guacamole");
    let (phone, _, sid) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &phone).await;
    wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["level"]["state"] == "ready" && !s["level"]["scripts"].is_null()
    })
    .await
    .expect("the level and its scripts");
    let figs = figures(&server, sid).await;
    let (eruptor, tree_rex) = (id_named(&figs, "Eruptor"), id_named(&figs, "Tree Rex"));
    load(&server, sid, 1, &eruptor).await;
    wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["mode"] == "paused"
    })
    .await
    .expect("the level waits for a pad");
    pad_tv(
        &server.url,
        json!({"pad": 0, "connect": {"name": "e2e pad"}}),
    )
    .await
    .unwrap();
    pad_tv(&server.url, json!({"pad": 0, "press": "east"}))
        .await
        .unwrap();
    wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        s["mode"] == "level" && s["hero"]["scripts"] == "joined"
    })
    .await
    .expect("his scripts join");
    let since = scripts_last(&server.url).await;
    pad_tv(&server.url, json!({"pad": 0, "press": "south"}))
        .await
        .unwrap();
    let steps = scripts_until(&server.url, since, Duration::from_secs(5), |s| {
        named(s, "gone").iter().any(|n| n == "009_LavaProjectile")
    })
    .await;
    assert_eq!(started(&steps), [20], "a press throws");

    // Tree Rex beside him: the hold, nothing of the pad reaching his scripts.
    load(&server, sid, 3, &tree_rex).await;
    wait_for_tv(&server.url, Duration::from_secs(3), |s| {
        s["mode"] == "held" && alert_kind(s) == "too_many_skylanders"
    })
    .await
    .expect("the too-many hold");
    // The frame the hold begins still runs the steps already due (AG.20).
    tokio::time::sleep(Duration::from_millis(200)).await;
    let since = scripts_last(&server.url).await;
    for button in ["south", "r1", "start"] {
        pad_tv(&server.url, json!({"pad": 0, "press": button}))
            .await
            .unwrap();
    }
    pad_tv(&server.url, json!({"pad": 0, "hold": "south"}))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let log = scripts_tv(&server.url, since).await.unwrap();
    assert!(
        log["steps"].as_array().unwrap().is_empty(),
        "no step under the hold: {log}"
    );
    let snap = tv(&server.url).await.unwrap();
    assert!(snap["input"]["pause"].is_null(), "{}", snap["input"]);
    assert_eq!(
        snap["input"]["hero_actions"],
        json!([false, false, false, false])
    );
    clear(&server, sid, 3).await;
    wait_for_tv(&server.url, Duration::from_secs(2), |s| {
        s["mode"] == "level"
    })
    .await
    .expect("the level again");
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let steps = scripts_until(&server.url, since, Duration::from_secs(2), |s| s.len() > 30).await;
    assert!(
        started(&steps).is_empty() && named(&steps, "spawned").is_empty(),
        "held through the hold, nothing: {steps:?}"
    );
    pad_tv(&server.url, json!({"pad": 0, "release": "south"}))
        .await
        .unwrap();
    // Up a few frames before it goes down again: the hook posts a release
    // and a press in one frame if they come together, and a button down at
    // the end of the frame it was let go in still reads as held through.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let since = scripts_last(&server.url).await;
    pad_tv(&server.url, json!({"pad": 0, "press": "south"}))
        .await
        .unwrap();
    let steps = scripts_until(&server.url, since, Duration::from_secs(5), |s| {
        named(s, "gone").iter().any(|n| n == "009_LavaProjectile")
    })
    .await;
    assert_eq!(started(&steps), [20], "pressed again, a throw");

    // Eruptor off: the call; placed again, in where he stood, his scripts
    // joined and throwing.
    let stood = hero_at(&tv(&server.url).await.unwrap());
    clear(&server, sid, 1).await;
    wait_for_tv(&server.url, Duration::from_secs(3), |s| {
        s["mode"] == "held" && alert_kind(s) == "place_a_skylander"
    })
    .await
    .expect("the call for a Skylander");
    load(&server, sid, 2, &eruptor).await;
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["mode"] == "level" && s["hero"]["scripts"] == "joined"
    })
    .await
    .expect("he comes in and his scripts join");
    let hero = &snap["hero"];
    assert_eq!(hero["slot"], 2, "{hero}");
    assert!(
        ground_apart(hero_at(&snap), stood) < 2e-2,
        "{:?} against {stood:?}",
        hero_at(&snap)
    );
    assert_eq!(
        (&hero["health"], &hero["health_max"]),
        (&json!(120), &json!(150)),
        "{hero}"
    );
    let since = scripts_last(&server.url).await;
    pad_tv(&server.url, json!({"pad": 0, "press": "south"}))
        .await
        .unwrap();
    let steps = scripts_until(&server.url, since, Duration::from_secs(5), |s| {
        named(s, "gone").iter().any(|n| n == "009_LavaProjectile")
    })
    .await;
    assert_eq!(started(&steps), [20], "his throw");
    assert_eq!(named(&steps, "spawned"), ["009_LavaProjectile"]);
    phone.close().await.unwrap();
}

/// A damaged model leaves its card on the portrait, says why on the TV's
/// mirror and in the log, and the modelled figure beside it still turns.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn a_damaged_model_keeps_its_portrait_and_is_logged() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (phone, _, sid) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &phone).await;
    let figs = figures(&server, sid).await;
    let bash = format!("{E2E_DAMAGED:06x}-0000");
    assert_eq!(id_named(&figs, "Bash"), bash);

    load(&server, sid, 1, &bash).await;
    load(&server, sid, 2, &id_named(&figs, "Gill Grunt")).await;
    wait_slot(&phone, 2, "Gill Grunt").await;
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        card(s, 1).is_some_and(|c| c["state"] == "portrait")
            && card(s, 2).is_some_and(|c| c["state"] == "model")
    })
    .await
    .expect("Bash on his portrait, Gill Grunt turning");
    let bash_card = card(&snap, 1).unwrap();
    assert_eq!(bash_card["portrait"], true, "his portrait: {bash_card}");
    let reason = bash_card["reason"].as_str().unwrap();
    assert!(reason.starts_with("004_e2e.ron:"), "{reason}");

    // The log names the figure and why, in a line of its own.
    let mut logged = String::new();
    for _ in 0..20 {
        logged = server.log_text();
        if logged.contains("keeps its portrait") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let line = logged
        .lines()
        .find(|l| l.contains("keeps its portrait") && l.contains("Bash"))
        .unwrap_or_else(|| panic!("no log line for Bash in:\n{logged}"));
    assert!(line.contains("004_e2e.ron"), "{line}");
    assert!(line.contains(&bash), "{line}");
    eprintln!("logged: {line}");
    phone.close().await.unwrap();
}

// ---- guacamole/figure-progress: the phone's editor on the rows (AH.3) --------

/// The phone's details of `name`, opened from the toy box's search.
async fn open_details(phone: &Phone, name: &str) {
    phone.search_named(name).await.unwrap();
    phone.tap_figure_named(name).await.unwrap();
    phone
        .wait_for(Locator::Css(".detail-surface"), Duration::from_secs(5))
        .await
        .unwrap();
}

/// What the details' stats strip reads: level, gold and playtime, or the
/// line it shows instead.
async fn strip(phone: &Phone) -> Vec<String> {
    let read = phone
        .client
        .execute(
            "return Array.from(document.querySelectorAll(
               '.detail-stats-strip .detail-stat-v, .detail-stats-soon-label'))
               .map(e => e.innerText.trim());",
            vec![],
        )
        .await
        .unwrap();
    serde_json::from_value(read).unwrap()
}

/// Waits up to `within` for the strip to read `want`; when it did.
async fn wait_strip(phone: &Phone, want: &[&str], within: Duration) -> Instant {
    let deadline = Instant::now() + within;
    loop {
        let read = strip(phone).await;
        if read == want {
            return Instant::now();
        }
        assert!(
            Instant::now() < deadline,
            "the strip read {read:?}, never {want:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A details action by its aria label: (disabled, its title).
async fn action(phone: &Phone, label: &str) -> (bool, String) {
    let btn = phone
        .client
        .find(Locator::Css(&format!(
            ".detail-action-btn[aria-label='{label}']"
        )))
        .await
        .unwrap();
    let disabled = btn.attr("disabled").await.unwrap().is_some();
    (
        disabled,
        btn.attr("title").await.unwrap().unwrap_or_default(),
    )
}

/// Clicks `selector` and waits for `then` to show.
async fn click_then(phone: &Phone, selector: &str, then: &str) {
    phone
        .client
        .find(Locator::Css(selector))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    phone
        .wait_for(Locator::Css(then), Duration::from_secs(5))
        .await
        .unwrap_or_else(|e| panic!("{then} after clicking {selector}: {e}"));
}

/// Taps the edit sheet's button `label` `times` times.
async fn tap_sheet(phone: &Phone, label: &str, times: usize) {
    for _ in 0..times {
        phone
            .client
            .find(Locator::Css(&format!("[aria-label='{label}']")))
            .await
            .unwrap()
            .click()
            .await
            .unwrap();
    }
}

/// The edit sheet's SAVE; returns once the sheet closed.
async fn save_sheet(phone: &Phone) -> Instant {
    phone
        .client
        .find(Locator::Css(".edit-btn-primary"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    let clicked = Instant::now();
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .client
                .find(Locator::Css(".edit-scrim"))
                .await
                .is_err()
        })
        .await
        .expect("the sheet closes after SAVE");
    clicked
}

/// RESET on the details, held on the confirm's HOLD TO RESET until it fires.
async fn reset_from_details(phone: &Phone) {
    click_then(
        phone,
        ".detail-action-btn[aria-label='Reset to pack-fresh']",
        ".reset-btn-danger",
    )
    .await;
    let press = |kind: &str| {
        format!(
            "const el = document.querySelector('.reset-btn-danger');
             if (!el) return false;
             el.dispatchEvent(new PointerEvent('{kind}', {{pointerId: 1, isPrimary: true, bubbles: true}}));
             return true;"
        )
    };
    let down = phone.client.execute(&press("pointerdown"), vec![]).await;
    assert_eq!(down.unwrap(), json!(true));
    // The hold fires at 1.2 s.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let _ = phone.client.execute(&press("pointerup"), vec![]).await;
    // The confirm plays its flakes for 1.5 s after firing, then goes.
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .client
                .find(Locator::Css(".reset-btn-danger"))
                .await
                .is_err()
        })
        .await
        .expect("the reset confirm closes");
}

/// `GET .../stats` for `profile_id`'s `figure_id`: (status, body).
async fn stats(server: &TestServer, profile_id: &str, figure_id: &str) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .get(format!(
            "{}/api/profiles/{profile_id}/figures/{figure_id}/stats",
            server.url
        ))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or_default())
}

/// The edit the phone's SAVE sends, from session `sid`; its status.
async fn edit_from(
    server: &TestServer,
    sid: u64,
    profile_id: &str,
    figure_id: &str,
    level: u8,
    gold: u32,
) -> u16 {
    reqwest::Client::new()
        .post(format!(
            "{}/api/profiles/{profile_id}/figures/{figure_id}/edit",
            server.url
        ))
        .header("x-session-id", sid.to_string())
        .json(&json!({ "level": level, "gold": gold }))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

/// The figure reset the phone's RESET sends off the portal, from `sid`.
async fn reset_from(server: &TestServer, sid: u64, profile_id: &str, figure_id: &str) -> u16 {
    reqwest::Client::new()
        .post(format!(
            "{}/api/profiles/{profile_id}/figures/{figure_id}/reset",
            server.url
        ))
        .header("x-session-id", sid.to_string())
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

/// AA's `figure_details_have_no_progress_and_stats_and_reset_are_off`, now
/// that guacamole keeps figure progress (the figure-cards delta REMOVES AA's
/// "not shown or edited yet"): a never-placed figure says so and edits, the
/// sheet stops at the extract's own caps, and a first placement shows level
/// 1 with nothing earned ("A first placement"), STATS off while it stands on
/// the portal and RESET resetting it where it stands.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn figure_details_show_the_rows_progress_and_the_games_caps() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (phone, _, _) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &phone).await;
    open_details(&phone, "Eruptor").await;

    wait_strip(&phone, &["NEVER PLACED"], Duration::from_secs(5)).await;
    assert_eq!(
        action(&phone, "Edit stats").await,
        (false, "Edit level + gold".into())
    );
    assert!(!action(&phone, "Reset to pack-fresh").await.0);

    // The sheet's caps are the extract's: its table's 15 levels, its clamp.
    click_then(
        &phone,
        ".detail-action-btn[aria-label='Edit stats']",
        ".edit-scrim",
    )
    .await;
    let metas: Vec<String> = serde_json::from_value(
        phone
            .client
            .execute(
                "return Array.from(document.querySelectorAll('.edit-stepper-meta'))
                   .map(e => e.innerText.trim());",
                vec![],
            )
            .await
            .unwrap(),
    )
    .unwrap();
    let max_level = format!("max {}", E2E_LEVEL_XP.len());
    let max_gold = format!("max {E2E_GOLD_CAP}");
    assert_eq!(metas, [max_level.as_str(), max_gold.as_str()]);
    tap_sheet(&phone, "Increase level", 20).await;
    let level = phone.inner_text(".edit-stepper-value").await.unwrap();
    assert_eq!(level.as_deref().map(str::trim), Some("15"));
    tap_sheet(&phone, "Set gold to max", 1).await;
    let gold = phone
        .inner_text(".edit-stepper-gold .edit-stepper-value")
        .await
        .unwrap();
    assert_eq!(
        gold.as_deref().map(str::trim),
        Some(E2E_GOLD_CAP.to_string().as_str())
    );
    phone
        .client
        .find(Locator::Css(".edit-btn-secondary"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();

    // A first placement: level 1, no gold, no playtime.
    phone
        .wait_for(Locator::Css(".detail-btn-primary"), Duration::from_secs(5))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    wait_slot(&phone, 1, "Eruptor").await;
    open_details(&phone, "Eruptor").await;
    wait_strip(&phone, &["1", "0", "0m"], Duration::from_secs(5)).await;
    assert_eq!(
        action(&phone, "Edit stats").await,
        (true, "Remove from portal before editing".into())
    );
    assert_eq!(
        action(&phone, "Reset to pack-fresh").await,
        (false, "Reset to fresh where it stands".into())
    );
    let detail = phone
        .inner_text(".detail-surface")
        .await
        .unwrap()
        .unwrap_or_default();
    assert!(!detail.contains("NOT AVAILABLE YET"), "{detail}");
    phone.close().await.unwrap();
}

/// "Never placed" and "Kept across a restart": a figure the profile never
/// placed edited from the phone, its row made by the edit, its level at the
/// table's threshold, still there after guacamole is quit and started again.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn a_never_placed_figure_edited_is_kept_across_a_restart() {
    if guacamole_only() {
        return;
    }
    let mut server = spawn();
    let (phone, pid, sid) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &phone).await;
    let spyro = id_named(&figures(&server, sid).await, "Spyro");
    assert_eq!(stats(&server, &pid, &spyro).await.0, 404);

    open_details(&phone, "Spyro").await;
    wait_strip(&phone, &["NEVER PLACED"], Duration::from_secs(5)).await;
    click_then(
        &phone,
        ".detail-action-btn[aria-label='Edit stats']",
        ".edit-scrim",
    )
    .await;
    tap_sheet(&phone, "Increase level", 9).await;
    tap_sheet(&phone, "Increase gold by 100", 3).await;
    save_sheet(&phone).await;
    wait_strip(&phone, &["10", "300", "0m"], Duration::from_secs(5)).await;
    let want = json!({
        "level": 10, "xp": E2E_LEVEL_XP[9], "gold": 300, "playtime_secs": 0,
        "max_level": E2E_LEVEL_XP.len(), "max_gold": E2E_GOLD_CAP,
    });
    assert_eq!(stats(&server, &pid, &spyro).await, (200, want.clone()));
    phone.close().await.unwrap();

    server.restart_guacamole().expect("start guacamole again");
    // The table is published from the extract at start.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let (status, body) = stats(&server, &pid, &spyro).await;
        if status != 409 {
            assert_eq!((status, body), (200, want));
            break;
        }
        assert!(Instant::now() < deadline, "the table never came back");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// "Another profile's Eruptor on the portal" then "Her own Eruptor on the
/// portal": Bob's placement leaves Alice's edit free; her own refuses it, the
/// phone saying so in her words, and nothing changes.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn an_edit_is_refused_only_while_her_own_figure_is_on_the_portal() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (alice, alice_pid, alice_sid) = phone_as(&server, "Alice", ALICE).await;
    let (bob, bob_pid, bob_sid) = phone_as(&server, "Bob", BOB).await;
    pick_giants(&server, &alice).await;
    let eruptor = id_named(&figures(&server, alice_sid).await, "Eruptor");

    load(&server, bob_sid, 1, &eruptor).await;
    wait_slot(&alice, 1, "Eruptor").await;
    let bobs = stats(&server, &bob_pid, &eruptor).await;
    assert_eq!(bobs.1["level"], 1);
    open_details(&alice, "Eruptor").await;
    wait_strip(&alice, &["NEVER PLACED"], Duration::from_secs(5)).await;
    assert!(
        !action(&alice, "Edit stats").await.0,
        "Bob's doesn't lock hers"
    );
    click_then(
        &alice,
        ".detail-action-btn[aria-label='Edit stats']",
        ".edit-scrim",
    )
    .await;
    tap_sheet(&alice, "Increase level", 1).await;
    save_sheet(&alice).await;
    wait_strip(&alice, &["2", "0", "0m"], Duration::from_secs(5)).await;
    assert_eq!(
        stats(&server, &bob_pid, &eruptor).await,
        bobs,
        "Bob's untouched"
    );

    // The sheet open, then her own Eruptor lands on the portal.
    click_then(
        &alice,
        ".detail-action-btn[aria-label='Edit stats']",
        ".edit-scrim",
    )
    .await;
    tap_sheet(&alice, "Increase level", 1).await;
    load(&server, alice_sid, 2, &eruptor).await;
    wait_slot(&alice, 2, "Eruptor").await;
    alice
        .client
        .find(Locator::Css(".edit-btn-primary"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    alice
        .wait_until(Duration::from_secs(5), || async {
            alice
                .inner_text(".edit-error")
                .await
                .ok()
                .flatten()
                .is_some_and(|t| !t.is_empty())
        })
        .await
        .expect("the sheet says why");
    let said = alice.inner_text(".edit-error").await.unwrap().unwrap();
    assert_eq!(
        said.trim(),
        "Eruptor is on the portal! Take Eruptor off first, then try again."
    );
    let (_, after) = stats(&server, &alice_pid, &eruptor).await;
    assert_eq!(
        (after["level"].clone(), after["xp"].clone()),
        (json!(2), json!(E2E_LEVEL_XP[1]))
    );
    alice
        .client
        .find(Locator::Css(".edit-btn-secondary"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    assert_eq!(
        action(&alice, "Edit stats").await,
        (true, "Remove from portal before editing".into())
    );
    alice.close().await.unwrap();
    bob.close().await.unwrap();
}

/// "A sibling's figure": a phone with Alice unlocked asking to edit or reset
/// Bob's Eruptor is refused, and his Eruptor is unchanged.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn a_siblings_phone_can_neither_edit_nor_reset_his_figure() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (alice, _, alice_sid) = phone_as(&server, "Alice", ALICE).await;
    let (bob, bob_pid, bob_sid) = phone_as(&server, "Bob", BOB).await;
    let eruptor = id_named(&figures(&server, bob_sid).await, "Eruptor");
    assert_eq!(
        edit_from(&server, bob_sid, &bob_pid, &eruptor, 5, 500).await,
        202
    );
    let bobs = stats(&server, &bob_pid, &eruptor).await;
    assert_eq!((bobs.0, bobs.1["level"].clone()), (200, json!(5)));

    assert_eq!(
        edit_from(&server, alice_sid, &bob_pid, &eruptor, 15, E2E_GOLD_CAP).await,
        403
    );
    assert_eq!(
        reset_from(&server, alice_sid, &bob_pid, &eruptor).await,
        403
    );
    assert_eq!(stats(&server, &bob_pid, &eruptor).await, bobs);
    alice.close().await.unwrap();
    bob.close().await.unwrap();
}

/// "Reset, then placed": RESET off the portal drops her progress, the
/// details say never placed, and the next placement is level 1 with no gold
/// and no playtime.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn reset_to_fresh_and_the_next_placement_starts_fresh() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (phone, pid, sid) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &phone).await;
    let eruptor = id_named(&figures(&server, sid).await, "Eruptor");
    assert_eq!(edit_from(&server, sid, &pid, &eruptor, 10, 500).await, 202);

    open_details(&phone, "Eruptor").await;
    wait_strip(&phone, &["10", "500", "0m"], Duration::from_secs(5)).await;
    reset_from_details(&phone).await;
    wait_strip(&phone, &["NEVER PLACED"], Duration::from_secs(5)).await;
    assert_eq!(stats(&server, &pid, &eruptor).await.0, 404);

    phone
        .wait_for(Locator::Css(".detail-btn-primary"), Duration::from_secs(5))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    wait_slot(&phone, 1, "Eruptor").await;
    open_details(&phone, "Eruptor").await;
    wait_strip(&phone, &["1", "0", "0m"], Duration::from_secs(5)).await;
    let (_, fresh) = stats(&server, &pid, &eruptor).await;
    assert_eq!(
        (fresh["xp"].clone(), fresh["gold"].clone()),
        (json!(0), json!(0))
    );
    phone.close().await.unwrap();
}

/// "A second phone watching", for an edit: the strip on one phone follows an
/// edit the other makes within 1 second.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn a_second_phones_strip_follows_an_edit_within_a_second() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let pid = inject_profile(&server.url, "Alice", "1234", ALICE)
        .await
        .unwrap();
    let mut phones = Vec::new();
    for _ in 0..2 {
        let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
            .await
            .unwrap();
        let sid = session_id(&phone).await;
        set_session_profile(&server.url, sid, &pid).await.unwrap();
        phones.push(phone);
    }
    let (watching, editing) = (&phones[0], &phones[1]);
    pick_giants(&server, watching).await;
    editing
        .wait_for_portal(Duration::from_secs(15))
        .await
        .expect("the second phone's portal");
    open_details(watching, "Spyro").await;
    wait_strip(watching, &["NEVER PLACED"], Duration::from_secs(5)).await;

    open_details(editing, "Spyro").await;
    wait_strip(editing, &["NEVER PLACED"], Duration::from_secs(5)).await;
    click_then(
        editing,
        ".detail-action-btn[aria-label='Edit stats']",
        ".edit-scrim",
    )
    .await;
    tap_sheet(editing, "Increase level", 4).await;
    tap_sheet(editing, "Increase gold by 100", 2).await;
    let saved = save_sheet(editing).await;
    let shown = wait_strip(watching, &["5", "200", "0m"], Duration::from_secs(1)).await;
    eprintln!(
        "the watching phone showed the edit {} ms after SAVE",
        shown.duration_since(saved).as_millis()
    );
    for phone in phones {
        phone.close().await.unwrap();
    }
}

/// "A slot reset while waiting" (no level running yet, so no hero): RESET on
/// the details of her own figure standing on the portal puts it back fresh
/// where it stands; the slot and the TV's card keep it.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn a_slot_reset_puts_her_figure_back_fresh_where_it_stands() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (phone, pid, sid) = phone_as(&server, "Alice", ALICE).await;
    pick_giants(&server, &phone).await;
    let tree_rex = id_named(&figures(&server, sid).await, "Tree Rex");
    assert_eq!(
        edit_from(&server, sid, &pid, &tree_rex, 10, 1200).await,
        202
    );
    load(&server, sid, 1, &tree_rex).await;
    let seen = wait_slot(&phone, 1, "Tree Rex").await;
    tv_card(&server, 1, "Tree Rex", seen).await;

    open_details(&phone, "Tree Rex").await;
    wait_strip(&phone, &["10", "1200", "0m"], Duration::from_secs(5)).await;
    assert_eq!(
        action(&phone, "Reset to pack-fresh").await,
        (false, "Reset to fresh where it stands".into())
    );
    reset_from_details(&phone).await;
    wait_strip(&phone, &["1", "0", "0m"], Duration::from_secs(5)).await;
    let (_, fresh) = stats(&server, &pid, &tree_rex).await;
    assert_eq!(
        (
            fresh["level"].clone(),
            fresh["xp"].clone(),
            fresh["gold"].clone()
        ),
        (json!(1), json!(0), json!(0))
    );
    let seen = wait_slot(&phone, 1, "Tree Rex").await;
    tv_card(&server, 1, "Tree Rex", seen).await;
    let toast = phone.last_toast_text().await.unwrap();
    assert!(
        toast.as_deref().is_none_or(|t| !t.contains("Reset failed")),
        "{toast:?}"
    );
    phone.close().await.unwrap();
}

// ---- guacamole/shell -----------------------------------------------------

/// AP.5, the shell delta's "Nothing on the join code reacts", pad half: the
/// input router takes a posted South, East and Mode, and the join code stays
/// up (East closes only the startup-error screens; the app is still serving).
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn pad_presses_leave_the_join_code_up() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    let (before, _) = wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["screen"] == "Join"
    })
    .await
    .unwrap();
    for button in ["south", "east", "mode"] {
        press_tv(&server.url, button).await.unwrap();
    }
    // The router takes one press a frame; this is dozens of frames.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let after = tv(&server.url).await.expect("guacamole still serving");
    assert_eq!(after["screen"], "Join", "{after}");
    assert_eq!(after["join_qr"], true, "{after}");
    for field in ["join_address", "players"] {
        assert_eq!(after[field], before[field], "{field}");
    }
}

/// AG.20's test hook, in the real binary: a posted pad connects through
/// Bevy's own gamepad systems, the TV's only pad source under the harness's
/// `GUAC_TEST_POSTED_PADS_ONLY` (AG.52, AG.53), its move stick lands raw (a
/// first value inside Bevy's dead zone and under its change threshold
/// included), the resolver gives a pad it can't place the position, the
/// level's layer isn't up with no level, Start on the join code changes
/// nothing, and the pad goes away when posted off.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver; E2E_APP=guacamole"]
async fn a_posted_pad_reads_raw() {
    if guacamole_only() {
        return;
    }
    let server = spawn();
    wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["screen"] == "Join"
    })
    .await
    .unwrap();
    for body in [
        json!({"pad": 0, "connect": {"name": "e2e pad", "vendor": 11720, "product": 1}}),
        json!({"pad": 0, "stick": [0.26, -0.004]}),
    ] {
        pad_tv(&server.url, body).await.unwrap();
    }
    let (shot, _) = wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        s["input"]["pads"][0]["stick"] == json!([0.26, -0.004])
    })
    .await
    .unwrap();
    let pad = &shot["input"]["pads"][0];
    assert_eq!(pad["name"], "e2e pad", "{shot}");
    assert_eq!(
        (&pad["vendor"], &pad["product"]),
        (&json!(11720), &json!(1))
    );
    assert_eq!(pad["labels"], "position", "{shot}");
    assert_eq!(pad["walking"], false, "no level, no walking pad");
    assert_eq!(shot["input"]["layer"], "screen");
    assert_eq!(shot["input"]["hero_stick"], Value::Null);

    pad_tv(&server.url, json!({"pad": 0, "press": "start"}))
        .await
        .unwrap();
    pad_tv(&server.url, json!({"pad": 0, "disconnect": true}))
        .await
        .unwrap();
    let (shot, _) = wait_for_tv(&server.url, Duration::from_secs(5), |s| {
        s["input"]["pads"] == json!([])
    })
    .await
    .unwrap();
    assert_eq!(shot["screen"], "Join", "{shot}");
    assert_eq!(shot["input"]["pause"], Value::Null);
}

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
    // No RPCS3 settings to offer (AA.19): the action is absent.
    let actions = phone
        .menu_actions_until(|a| !a.iter().any(|t| t.contains("GAME SETTINGS")))
        .await
        .unwrap();
    eprintln!("guacamole's menu: {actions:?}");
    assert!(
        actions.iter().any(|t| t == "MANAGE PROFILES"),
        "{actions:?}"
    );
    assert!(
        !actions.iter().any(|t| t.contains("GAME SETTINGS")),
        "guacamole's menu offers the RPCS3 settings: {actions:?}"
    );
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

    // The window-mode toggle: the phone says it's unavailable and rolls back.
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
                .is_some_and(|t| t.contains("Window mode isn't available in guacamole"))
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
    // AP.6: its choose-folder button is already selected. Not pressed: that
    // would open the folder sheet.
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(1), |s| {
        !s["focus"].is_null()
    })
    .await
    .expect("a choice has the focus");
    assert_eq!(snap["focus"], "choose-folder", "{snap}");
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
