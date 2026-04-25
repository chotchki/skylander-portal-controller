//! Visual debug — diagnose the stubborn iPhone scroll problems on
//! Konami gate + PIN entry (Chris play-test 2026-04-24).
//!
//! Headless Chrome sized to a realistic iPhone portrait viewport
//! (~390x660), navigates to the target screens, probes scrollHeight
//! vs clientHeight + absolute positions of the back button / dpad /
//! submit row, captures PNGs. The numbers narrow the fix (container
//! too short? specific child overflowing?) without needing a device.

use std::path::PathBuf;
use std::time::Duration;

use fantoccini::{ClientBuilder, Locator};
use serde_json::json;
use skylander_e2e_tests::{TestServer, inject_profile};

/// iPhone 14 Pro portrait viewport, mobile Safari with address bar
/// showing — the state where Chris's bugs manifest. Chrome headless
/// --window-size includes window chrome (tabs, address bar, menu
/// bar) that Safari on iPhone doesn't have; we target a total size
/// that yields ~660px web-viewable area after Chrome's chrome eats
/// its share. Rough calibration from prior runs: 508px viewport
/// from --window-size=390,660 was too tight; 420x900 → 474x508
/// showed everything but was misleadingly short.
const PHONE_W: i64 = 430;
const PHONE_H: i64 = 820;

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver + built phone SPA; visual inspection only"]
async fn capture_konami_and_pin_entry_at_iphone_size() {
    let server = TestServer::spawn().expect("spawn server");

    // Seed a profile so we can drill into PIN entry. PIN 1234 /
    // color arbitrary; PIN doesn't matter for the layout shot.
    inject_profile(&server.url, "Chris", "1234", "#da5ad6")
        .await
        .expect("inject profile");

    // Browser manually configured here (not via Phone::new helper)
    // because we need a narrower iPhone-sized viewport than the
    // harness default (420x900).
    let caps = serde_json::from_value::<serde_json::Value>(json!({
        "goog:chromeOptions": {
            "args": [
                "--headless=new",
                "--no-sandbox",
                "--disable-gpu",
                format!("--window-size={PHONE_W},{PHONE_H}"),
            ]
        }
    }))
    .unwrap();

    let client = ClientBuilder::native()
        .capabilities(caps.as_object().unwrap().clone())
        .connect(&server.chromedriver_url)
        .await
        .expect("connect chromedriver");

    let phone_url = server.phone_url().await.expect("phone url");
    client.goto(&phone_url).await.expect("goto");

    // Wait for welcome heading to confirm the SPA mounted.
    wait_for(&client, ".pp-welcome-wrap", 10).await;

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("visual_debug");
    std::fs::create_dir_all(&out_dir).expect("mkdir visual_debug");

    tokio::time::sleep(Duration::from_millis(300)).await;
    screenshot(&client, &out_dir.join("00_profile_picker.png")).await;

    // --- Open Konami gate via kebab → MANAGE PROFILES ---
    click(&client, ".kebab-btn").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    // MenuOverlay shows a list; find the "MANAGE" action.
    if click_text(&client, "MANAGE PROFILES").await.is_err() {
        let _ = click_text(&client, "MANAGE").await;
    }

    // Wait for konami gate to render.
    wait_for(&client, ".konami-gate", 8).await;
    tokio::time::sleep(Duration::from_millis(400)).await;

    let konami_probe = probe_scroll(
        &client,
        &[
            ".screen-profile-picker",
            ".konami-gate",
            ".konami-header",
            ".gate-progress",
            ".gate-hint",
            ".gate-pad",
            ".dpad",
            ".ab-wrap",
            ".gate-actions",
            ".btn-back",
        ],
    )
    .await;
    // Dump every direct child of screen-profile-picker so we can see
    // what is (if anything) taking flow space above konami-gate.
    let kids = client
        .execute(
            r#"
            const p = document.querySelector('.screen-profile-picker');
            if (!p) return null;
            return Array.from(p.children).map(c => ({
                tag: c.tagName,
                classes: c.className,
                id: c.id,
                rectTop: c.getBoundingClientRect().top,
                rectBottom: c.getBoundingClientRect().bottom,
                rectHeight: c.getBoundingClientRect().height,
                display: getComputedStyle(c).display,
            }));
            "#,
            vec![],
        )
        .await
        .expect("children probe");
    println!("---- screen-profile-picker children ----");
    println!("{}", serde_json::to_string_pretty(&kids).unwrap());
    println!("---- konami scroll probe ----");
    println!("{}", serde_json::to_string_pretty(&konami_probe).unwrap());
    screenshot(&client, &out_dir.join("01_konami.png")).await;

    // --- Back to picker, then click the seeded profile → PIN entry ---
    click(&client, ".btn-back").await;
    wait_for(&client, ".pp-welcome-wrap", 6).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Tap the first profile card — the only one is Chris.
    click(&client, ".profile-card").await;
    wait_for(&client, ".pin-entry-screen", 6).await;
    tokio::time::sleep(Duration::from_millis(400)).await;

    let pin_probe = probe_scroll(
        &client,
        &[
            ".screen-profile-picker",
            ".pin-entry-screen",
            ".pin-identity",
            ".pin-profile-bezel",
            ".pin-prompt-name",
            ".pin-keypad-panel",
            ".btn-back",
        ],
    )
    .await;
    println!("---- pin entry scroll probe ----");
    println!("{}", serde_json::to_string_pretty(&pin_probe).unwrap());
    screenshot(&client, &out_dir.join("02_pin_entry.png")).await;

    client.close().await.unwrap();
}

async fn wait_for(client: &fantoccini::Client, selector: &str, secs: u64) {
    client
        .wait()
        .at_most(Duration::from_secs(secs))
        .for_element(Locator::Css(selector))
        .await
        .unwrap_or_else(|_| panic!("waiting for {selector}"));
}

async fn click(client: &fantoccini::Client, selector: &str) {
    let el = client
        .find(Locator::Css(selector))
        .await
        .unwrap_or_else(|_| panic!("find {selector}"));
    el.click()
        .await
        .unwrap_or_else(|_| panic!("click {selector}"));
}

async fn click_text(client: &fantoccini::Client, text: &str) -> Result<(), String> {
    let xpath = format!("//*[normalize-space(text())='{text}']");
    let el = client
        .find(Locator::XPath(&xpath))
        .await
        .map_err(|e| format!("find text {text}: {e}"))?;
    el.click()
        .await
        .map_err(|e| format!("click text {text}: {e}"))?;
    Ok(())
}

async fn screenshot(client: &fantoccini::Client, path: &std::path::Path) {
    let png = client.screenshot().await.expect("screenshot");
    std::fs::write(path, &png).expect("write png");
    println!("---- screenshot → {} ----", path.display());
}

/// For each selector: return scrollHeight/clientHeight + its
/// bounding rect relative to the viewport. Also dump the window
/// size so the test log has one coherent snapshot.
async fn probe_scroll(client: &fantoccini::Client, selectors: &[&str]) -> serde_json::Value {
    let script = format!(
        r#"
        const selectors = {};
        const pick = (sel) => {{
            const el = document.querySelector(sel);
            if (!el) return {{ exists: false }};
            const r = el.getBoundingClientRect();
            const s = getComputedStyle(el);
            return {{
                exists: true,
                top: r.top,
                bottom: r.bottom,
                height: r.height,
                width: r.width,
                scrollHeight: el.scrollHeight,
                clientHeight: el.clientHeight,
                overflowY: s.overflowY,
                position: s.position,
                visible: r.height > 0 && r.top < window.innerHeight && r.bottom > 0,
            }};
        }};
        const out = {{
            viewport: {{ w: window.innerWidth, h: window.innerHeight }},
            scroll: {{
                docScrollTop: document.documentElement.scrollTop,
                docScrollHeight: document.documentElement.scrollHeight,
                bodyScrollHeight: document.body.scrollHeight,
            }},
        }};
        for (const s of selectors) out[s] = pick(s);
        return out;
        "#,
        serde_json::to_string(selectors).unwrap(),
    );
    client
        .execute(&script, vec![])
        .await
        .expect("probe_scroll JS")
}
