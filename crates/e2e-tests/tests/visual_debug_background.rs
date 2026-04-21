//! Visual debug — diagnose "body gradient stops short, dark band at bottom"
//! (iPhone on-device 2026-04-21).
//!
//! Runs headless Chrome at roughly phone-portrait dimensions, captures a
//! PNG, and programmatically measures whether the body background covers
//! the full viewport. Output lands in `target/visual_debug/` so we can
//! inspect without a real device.

use std::path::PathBuf;
use std::time::Duration;

use fantoccini::Locator;
use skylander_e2e_tests::{Phone, TestServer};

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver + built phone SPA; visual inspection only"]
async fn capture_background_coverage_at_phone_size() {
    let server = TestServer::spawn().expect("spawn server");
    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .expect("connect phone");

    // ProfilePicker is the first screen; wait for the welcome heading.
    phone
        .wait_for(
            Locator::Css(".pp-welcome-wrap"),
            Duration::from_secs(10),
        )
        .await
        .expect("profile picker heading");

    // Give the starfield + animations a frame to paint.
    tokio::time::sleep(Duration::from_millis(250)).await;

    // Probe the layout with JS. Returns a JSON object with the metrics we
    // need to reason about the dark band.
    let probe = phone
        .client
        .execute(
            r#"
            const body = document.body;
            const html = document.documentElement;
            const cs = getComputedStyle(body);
            const bodyRect = body.getBoundingClientRect();
            const htmlRect = html.getBoundingClientRect();
            // Sample computed backgroundAttachment to confirm our fix
            // shipped.
            return {
                innerWidth: window.innerWidth,
                innerHeight: window.innerHeight,
                bodyHeight: bodyRect.height,
                bodyBottom: bodyRect.bottom,
                htmlHeight: htmlRect.height,
                htmlBottom: htmlRect.bottom,
                background: cs.background,
                backgroundAttachment: cs.backgroundAttachment,
            };
            "#,
            vec![],
        )
        .await
        .expect("probe JS");

    println!("---- background coverage probe ----");
    println!("{}", serde_json::to_string_pretty(&probe).unwrap());

    // Sample the rendered pixel colour at several y-positions. For this we
    // paint a 1-pixel canvas matching the viewport, draw the body's
    // current background via `html2canvas`-style sampling — but that's
    // heavy. Simpler: just measure via `getComputedStyle(childEl)` at
    // various known-position elements, OR via elementFromPoint. We go
    // with elementFromPoint so we can see what's actually rendering
    // near the viewport's bottom.
    let sampled = phone
        .client
        .execute(
            r#"
            const h = window.innerHeight;
            const samples = [0, 0.25, 0.5, 0.75, 0.9, 0.95, 0.99].map(frac => {
                const y = Math.floor(h * frac);
                const el = document.elementFromPoint(window.innerWidth / 2, y);
                return {
                    yFrac: frac,
                    y,
                    tag: el ? el.tagName : null,
                    classes: el ? el.className : null,
                };
            });
            return samples;
            "#,
            vec![],
        )
        .await
        .expect("sample JS");

    println!("---- elementFromPoint samples (x = centre, varying y) ----");
    println!("{}", serde_json::to_string_pretty(&sampled).unwrap());

    // Capture a screenshot for eyeball inspection.
    let png = phone.client.screenshot().await.expect("screenshot");
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("visual_debug");
    std::fs::create_dir_all(&out_dir).expect("mkdir visual_debug");
    let out = out_dir.join("profile_picker_background.png");
    std::fs::write(&out, &png).expect("write png");
    println!("---- screenshot written to {} ----", out.display());
    println!("png bytes: {}", png.len());

    phone.close().await.unwrap();
}
