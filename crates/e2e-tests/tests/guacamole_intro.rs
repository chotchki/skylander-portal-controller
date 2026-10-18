//! guacamole's character intros against the phone (openspec AB.11, the
//! `guacamole/character-intros` spec). guacamole runs over an
//! [`IntroFixture`]: a synthetic Giants dump and a finished extract whose
//! intros ffmpeg's `testsrc` and `sine` make at run time, so no game bytes are
//! involved. What the TV plays is read from `/api/_test/tv`'s `intro` and
//! `intro_queue`, not pixels; skips go through `/api/_test/tv/skip`, which
//! skips as a key would (the key and controller paths are AB.10's headless
//! tests and the M0 run's).
//!
//! Guacamole-only, and needs ffmpeg. Run with
//!
//! ```text
//! E2E_APP=guacamole cargo test -p skylander-e2e-tests --test guacamole_intro \
//!     -- --ignored --test-threads=1
//! ```

use std::time::{Duration, Instant};

use fantoccini::Locator;
use serde_json::{Value, json};

use skylander_e2e_tests::{
    FixtureIntro, IntroFixture, Phone, TestServer, guacamole_only, inject_profile,
    set_session_profile, skip_intro, tv, wait_for_tv,
};

/// The spec's bound from the phone showing a figure to its intro playing.
const INTRO_WITHIN: Duration = Duration::from_secs(2);
/// How soon a removal or skip shows on the TV.
const TV_WITHIN: Duration = Duration::from_secs(1);

const ERUPTOR: FixtureIntro = FixtureIntro {
    toy_type: 0x09,
    movie: "eruptor",
    seconds: 30,
};
const SPYRO: FixtureIntro = FixtureIntro {
    toy_type: 0x10,
    movie: "spyro",
    seconds: 30,
};
const BASH: FixtureIntro = FixtureIntro {
    toy_type: 0x04,
    movie: "bash",
    seconds: 30,
};

fn spawn(intros: &[FixtureIntro]) -> (TestServer, IntroFixture) {
    let fixture = IntroFixture::build(intros).expect("build the intro fixture");
    let server = TestServer::spawn_guacamole_with_intros(&fixture).expect("spawn guacamole");
    (server, fixture)
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

/// A phone with Alice's profile open: (phone, session id).
async fn phone(server: &TestServer) -> (Phone, u64) {
    let pid = inject_profile(&server.url, "Alice", "1234", "#e8423f")
        .await
        .unwrap();
    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .unwrap();
    let sid = session_id(&phone).await;
    set_session_profile(&server.url, sid, &pid).await.unwrap();
    (phone, sid)
}

/// Picks Giants on the phone, then waits for the extract to read as current:
/// a placement before that would make no intro (the spec's extraction wait).
async fn pick_giants(server: &TestServer, phone: &Phone, intros: usize) {
    let card = phone
        .wait_for(
            Locator::Css(".game-picker .game-card"),
            Duration::from_secs(15),
        )
        .await
        .expect("game picker");
    tokio::time::sleep(Duration::from_millis(600)).await;
    card.click().await.expect("click Giants");
    phone
        .wait_for_portal(Duration::from_secs(15))
        .await
        .expect("phone portal");
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(10), |s| {
        s["screen"] == "Portal" && s["extraction"]["state"] == "Ready"
    })
    .await
    .expect("TV portal with the extract ready");
    let x = &snap["extraction"];
    assert_eq!(x["ran"], false, "the fixture's extract is current: {x}");
    assert_eq!(x["intros"], intros, "{x}");
    assert_eq!(snap["intro"], Value::Null);
}

async fn figure_id(server: &TestServer, sid: u64, name: &str) -> String {
    let figs: Vec<Value> = reqwest::Client::new()
        .get(format!("{}/api/figures", server.url))
        .header("x-session-id", sid.to_string())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    figs.iter()
        .find(|f| f["canonical_name"] == name)
        .unwrap_or_else(|| panic!("no figure named {name}"))["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// The request the phone's place or remove button sends.
async fn slot_action(server: &TestServer, sid: u64, slot: u8, action: &str, body: Value) {
    let resp = reqwest::Client::new()
        .post(format!("{}/api/portal/slot/{slot}/{action}", server.url))
        .header("x-session-id", sid.to_string())
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 202, "{}", resp.text().await.unwrap());
}

async fn load(server: &TestServer, sid: u64, slot: u8, name: &str) {
    let id = figure_id(server, sid, name).await;
    slot_action(server, sid, slot, "load", json!({ "figure_id": id })).await;
}

async fn clear(server: &TestServer, sid: u64, slot: u8) {
    slot_action(server, sid, slot, "clear", json!({})).await;
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
    phone.tap_figure_named(name).await.unwrap();
    phone
        .wait_for(Locator::Css(".detail-btn-primary"), Duration::from_secs(5))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
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

fn playing(snap: &Value) -> Option<&str> {
    snap["intro"]["movie"].as_str()
}

fn queue(snap: &Value) -> Vec<(u64, String)> {
    snap["intro_queue"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| {
            (
                q["slot"].as_u64().unwrap(),
                q["movie"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn card_slots(snap: &Value) -> Vec<u64> {
    snap["cards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["slot"].as_u64().unwrap())
        .collect()
}

/// Waits for `movie` to be on screen with a frame shown.
async fn wait_playing(server: &TestServer, movie: &str, within: Duration) -> (Value, Instant) {
    wait_for_tv(&server.url, within, |s| {
        playing(s) == Some(movie)
            && s["intro"]["shown"].as_u64() > Some(0)
            && s["intro"]["full_screen"] == true
    })
    .await
    .unwrap_or_else(|e| panic!("{movie}'s intro didn't play within {within:?}: {e}"))
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver and ffmpeg; E2E_APP=guacamole"]
async fn a_placement_plays_its_intro_and_a_removal_stops_it() {
    if guacamole_only() {
        return;
    }
    let (server, _fixture) = spawn(&[ERUPTOR]);
    let (phone, _) = phone(&server).await;
    pick_giants(&server, &phone, 1).await;

    place_named(&phone, "Eruptor").await;
    let shown = wait_slot(&phone, 1, "Eruptor").await;
    let (snap, at) = wait_playing(&server, "eruptor", INTRO_WITHIN).await;
    eprintln!(
        "eruptor's first frame on the TV {} ms after the phone poll showed the figure",
        at.duration_since(shown).as_millis()
    );
    let intro = &snap["intro"];
    assert_eq!(intro["slot"], 1, "{intro}");
    assert_eq!(intro["figure_id"], "000009-0000", "{intro}");
    assert_eq!(
        intro["frames"],
        30 * 30,
        "testsrc at 30 fps for 30 s: {intro}"
    );
    assert!(
        intro["video_rect"].is_array(),
        "the picture is drawn: {intro}"
    );

    // It keeps playing, with a sound (muted in e2e, still a sink).
    let first = intro["shown"].as_u64().unwrap();
    let (later, _) = wait_for_tv(&server.url, Duration::from_secs(3), |s| {
        playing(s) == Some("eruptor")
            && s["intro"]["audio"] == "playing"
            && s["intro"]["shown"].as_u64().unwrap_or(0) >= first + 15
    })
    .await
    .expect("eruptor plays on with its sound");
    eprintln!("eruptor a moment later: {}", later["intro"]);

    phone.remove_slot(1).await.unwrap();
    phone
        .wait_for_slot_empty(1, Duration::from_secs(5))
        .await
        .unwrap();
    let gone = Instant::now();
    let (after, at) = wait_for_tv(&server.url, TV_WITHIN, |s| {
        s["intro"].is_null() && card_slots(s).is_empty()
    })
    .await
    .expect("the removal stops the intro within 1 s");
    eprintln!(
        "eruptor's intro stopped {} ms after the phone showed the slot empty",
        at.duration_since(gone).as_millis()
    );
    assert!(queue(&after).is_empty(), "{after}");
    assert_eq!(after["screen"], "Portal");
    phone.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver and ffmpeg; E2E_APP=guacamole"]
async fn an_intro_plays_to_its_end_then_the_card_is_there() {
    if guacamole_only() {
        return;
    }
    let short = FixtureIntro {
        seconds: 2,
        ..ERUPTOR
    };
    let (server, _fixture) = spawn(&[short]);
    let (phone, sid) = phone(&server).await;
    pick_giants(&server, &phone, 1).await;

    load(&server, sid, 1, "Eruptor").await;
    let (mut last, started) = wait_playing(&server, "eruptor", INTRO_WITHIN).await;
    let deadline = started + Duration::from_secs(6);
    let (snap, ended) = loop {
        let snap = tv(&server.url).await.unwrap();
        if snap["intro"].is_null() {
            break (snap, Instant::now());
        }
        assert!(
            Instant::now() < deadline,
            "the 2 s intro never ended: {snap}"
        );
        last = snap;
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    let ran = ended.duration_since(started);
    let intro = &last["intro"];
    eprintln!(
        "the 2 s intro was on screen {} ms; last seen {intro}",
        ran.as_millis()
    );
    // A broken video ends at once, a stalled one 2 s past its end
    // (END_GRACE); this one kept pace and ran out.
    let through = intro["shown"].as_u64().unwrap() + intro["skipped"].as_u64().unwrap();
    assert_eq!(intro["frames"], 60);
    assert!(
        through >= 50,
        "only {through} of 60 frames went by: {intro}"
    );
    assert!(
        (Duration::from_millis(1500)..Duration::from_millis(3500)).contains(&ran),
        "ended after {ran:?}"
    );
    assert_eq!(card_slots(&snap), [1], "{snap}");
    assert_eq!(snap["cards"][0]["texts"][0], "Eruptor");
    phone.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver and ffmpeg; E2E_APP=guacamole"]
async fn intros_queue_and_a_skip_or_removal_moves_on() {
    if guacamole_only() {
        return;
    }
    let (server, _fixture) = spawn(&[ERUPTOR, SPYRO, BASH]);
    let (phone, sid) = phone(&server).await;
    pick_giants(&server, &phone, 3).await;

    load(&server, sid, 1, "Eruptor").await;
    wait_playing(&server, "eruptor", INTRO_WITHIN).await;
    load(&server, sid, 2, "Spyro").await;
    load(&server, sid, 3, "Bash").await;
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(5), |s| queue(s).len() == 2)
        .await
        .expect("two intros queued");
    assert_eq!(playing(&snap), Some("eruptor"));
    assert_eq!(
        queue(&snap),
        [(2, "spyro".to_string()), (3, "bash".to_string())],
        "placement order"
    );

    // Removed before its turn: Bash's never plays.
    clear(&server, sid, 3).await;
    let (snap, _) = wait_for_tv(&server.url, TV_WITHIN, |s| queue(s).len() == 1)
        .await
        .expect("bash leaves the queue");
    assert_eq!(queue(&snap), [(2, "spyro".to_string())]);
    assert_eq!(playing(&snap), Some("eruptor"), "the playing one goes on");

    // A skip: Spyro's starts.
    skip_intro(&server.url).await.unwrap();
    let (snap, _) = wait_playing(&server, "spyro", TV_WITHIN).await;
    assert!(queue(&snap).is_empty(), "{snap}");
    assert_eq!(snap["intro"]["slot"], 2);

    // A magic item has no intro: its card, nothing queued.
    load(&server, sid, 4, "Sky-Iron Shield").await;
    let (snap, _) = wait_for_tv(&server.url, Duration::from_secs(3), |s| {
        card_slots(s).contains(&4)
    })
    .await
    .expect("the magic item's card");
    assert!(queue(&snap).is_empty(), "{snap}");
    assert_eq!(playing(&snap), Some("spyro"));

    // The last skip: back to the cards, Bash's never having played.
    skip_intro(&server.url).await.unwrap();
    let (snap, _) = wait_for_tv(&server.url, TV_WITHIN, |s| s["intro"].is_null())
        .await
        .expect("back to the cards");
    assert_eq!(card_slots(&snap), [1, 2, 4], "{snap}");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let still = tv(&server.url).await.unwrap();
    assert!(
        still["intro"].is_null() && queue(&still).is_empty(),
        "{still}"
    );
    phone.close().await.unwrap();
}
