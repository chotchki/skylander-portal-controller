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
    E2E_DAMAGED, E2E_GOLD_CAP, E2E_LEVEL_XP, E2E_MODELLED, E2E_SCRIPTS, GIANTS_SERIAL, Phone,
    TestServer, fire_kaos_swap, guacamole_only, inject_load_outcomes, inject_profile, press_tv,
    set_session_profile, tv, wait_for_tv,
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
