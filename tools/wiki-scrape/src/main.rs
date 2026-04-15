//! One-shot wiki scraper for Skylanders figure metadata + hero images.
//!
//! Reads `docs/research/firmware-inventory.json`, resolves each entry against
//! the Fandom MediaWiki API at <https://skylanders.fandom.com/api.php>,
//! downloads the infobox hero image, derives a 128×128 center-cropped
//! thumbnail, and writes everything under `data/`.
//!
//! Re-running is idempotent: existing `data/images/<id>/*.png` files are not
//! re-downloaded; existing `data/figures.json` entries with a resolved
//! `wiki_page` are re-used unless `--force` is passed.
//!
//! Run:
//!     cargo run -p skylander-wiki-scrape
//!     cargo run -p skylander-wiki-scrape -- --force         # redo everything
//!     cargo run -p skylander-wiki-scrape -- --limit 5       # smoke test

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use image::imageops::FilterType;
use image::GenericImageView;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{debug, info, warn};
use unicode_normalization::UnicodeNormalization;

const USER_AGENT: &str =
    "skylander-portal-controller/0.1 (https://github.com/chotchki/skylander-portal-controller; \
     one-shot figure-metadata scrape; contact via GitHub issues)";
const API_BASE: &str = "https://skylanders.fandom.com/api.php";
const WIKI_BASE: &str = "https://skylanders.fandom.com";
const THUMB_SIZE: u32 = 128;
/// Width we request from MediaWiki for the source download. We then downscale
/// the saved `hero.png` to `HERO_SAVE_WIDTH` to keep the committed repo footprint
/// under a few MB across ~500 figures.
const HERO_WIDTH: u32 = 512;
const HERO_SAVE_WIDTH: u32 = 320;
// Politeness: one request per second to Fandom (both API + image CDN share it).
const REQ_INTERVAL: Duration = Duration::from_millis(1100);

// ---------------- Input (firmware-inventory.json) ----------------

#[derive(Deserialize, Debug, Clone)]
struct InventoryEntry {
    id: String,
    #[allow(dead_code)]
    game: String,
    #[allow(dead_code)]
    element: Option<String>,
    category: String,
    variant_group: String,
    variant_tag: String,
    name: String,
    #[allow(dead_code)]
    relative_path: String,
}

// ---------------- Output (figures.json) ----------------

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct WikiFigure {
    figure_id: String,
    /// Full URL of the wiki page we matched (None if no match).
    wiki_page: Option<String>,
    soul_gem: Option<String>,
    #[serde(default)]
    signature_moves: Vec<String>,
    alignment: Option<String>,
    /// Everything else parsed out of the page Characterbox, unnormalised.
    #[serde(default)]
    attributes: HashMap<String, String>,
}

// ---------------- Rate-limited HTTP client ----------------

struct Client {
    http: reqwest::Client,
    gate: Mutex<std::time::Instant>,
}

impl Client {
    fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .gzip(true)
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            gate: Mutex::new(std::time::Instant::now() - REQ_INTERVAL),
        })
    }

    /// Wait out the rate-limit window, then perform the request with
    /// exponential-backoff retry on 429/5xx.
    async fn get(&self, url: &str) -> Result<reqwest::Response> {
        // Gate.
        {
            let mut last = self.gate.lock().await;
            let elapsed = last.elapsed();
            if elapsed < REQ_INTERVAL {
                sleep(REQ_INTERVAL - elapsed).await;
            }
            *last = std::time::Instant::now();
        }

        let mut backoff = Duration::from_secs(2);
        for attempt in 0..4 {
            let resp = self.http.get(url).send().await;
            match resp {
                Ok(r) if r.status().is_success() => return Ok(r),
                Ok(r) if r.status() == 429 || r.status().is_server_error() => {
                    warn!(attempt, status = %r.status(), "HTTP {} — backing off {:?}", r.status(), backoff);
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
                Ok(r) => return Ok(r), // e.g. 404 — caller decides
                Err(e) if attempt < 3 => {
                    warn!(?e, "request error — backing off {:?}", backoff);
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(anyhow!("exhausted retries for {url}"))
    }
}

// ---------------- Wiki lookup ----------------

#[derive(Deserialize)]
#[allow(dead_code)]
struct OpensearchArray3(
    String,
    Vec<String>,
    Vec<String>,
    Vec<String>,
);

#[derive(Deserialize)]
struct QueryResponse {
    query: Option<QueryPages>,
}

#[derive(Deserialize)]
struct QueryPages {
    pages: HashMap<String, WikiPage>,
}

#[derive(Deserialize)]
struct WikiPage {
    #[serde(default)]
    missing: Option<serde_json::Value>,
    #[allow(dead_code)]
    #[serde(default)]
    pageid: Option<u64>,
    title: Option<String>,
    #[serde(default)]
    thumbnail: Option<Thumbnail>,
    #[serde(default)]
    pageimage: Option<String>,
    #[serde(default)]
    categories: Option<Vec<Category>>,
    #[serde(default)]
    revisions: Option<Vec<Revision>>,
}

#[derive(Deserialize)]
struct Thumbnail {
    source: String,
    #[allow(dead_code)]
    width: u32,
    #[allow(dead_code)]
    height: u32,
}

#[derive(Deserialize)]
struct Category {
    title: String,
}

#[derive(Deserialize)]
struct Revision {
    slots: RevSlots,
}
#[derive(Deserialize)]
struct RevSlots {
    main: RevMain,
}
#[derive(Deserialize)]
struct RevMain {
    #[serde(rename = "*")]
    content: Option<String>,
}

/// Try a set of candidate titles in order, returning the first one that exists.
async fn resolve_title(client: &Client, candidates: &[String]) -> Result<Option<String>> {
    for cand in candidates {
        let url = format!(
            "{API_BASE}?action=query&titles={}&format=json&redirects=1",
            urlencode(cand)
        );
        let resp = client.get(&url).await?;
        if !resp.status().is_success() {
            continue;
        }
        let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
        if let Some(pages) = body.pointer("/query/pages").and_then(|p| p.as_object()) {
            for (pageid, page) in pages {
                if pageid == "-1" || page.get("missing").is_some() {
                    continue;
                }
                if let Some(title) = page.get("title").and_then(|t| t.as_str()) {
                    // Fandom frequently redirects a plain figure title to its
                    // `/Gallery` subpage (the image-dump article), which has no
                    // Characterbox. When that happens, strip the suffix and
                    // re-resolve.
                    let title = if let Some(base) = title.strip_suffix("/Gallery") {
                        base.to_string()
                    } else {
                        title.to_string()
                    };
                    debug!(candidate = %cand, resolved = %title, "title match");
                    return Ok(Some(title));
                }
            }
        }
        // Fall back to opensearch for a fuzzier hit on this candidate.
        let url = format!(
            "{API_BASE}?action=opensearch&search={}&format=json&limit=1&namespace=0",
            urlencode(cand)
        );
        let resp = client.get(&url).await?;
        if !resp.status().is_success() {
            continue;
        }
        if let Ok(arr) = resp.json::<OpensearchArray3>().await {
            if let Some(first) = arr.1.first() {
                debug!(candidate = %cand, resolved = %first, "opensearch match");
                return Ok(Some(first.clone()));
            }
        }
    }
    Ok(None)
}

async fn fetch_page(client: &Client, title: &str) -> Result<Option<WikiPage>> {
    let url = format!(
        "{API_BASE}?action=query&titles={}&prop=pageimages|categories|revisions\
         &pithumbsize={}&cllimit=max&rvprop=content&rvslots=main&format=json&redirects=1",
        urlencode(title),
        HERO_WIDTH
    );
    let resp = client.get(&url).await?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let body: QueryResponse = resp.json().await?;
    let Some(pages) = body.query else {
        return Ok(None);
    };
    for (_pid, page) in pages.pages {
        if page.missing.is_some() {
            continue;
        }
        return Ok(Some(page));
    }
    Ok(None)
}

// ---------------- Characterbox parsing ----------------

fn parse_characterbox(wikitext: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    // Find `{{Characterbox ... }}` with nested-brace awareness.
    let Some(start) = find_ci(wikitext, "{{Characterbox") else {
        return out;
    };
    let bytes = wikitext.as_bytes();
    let mut i = start + 2; // past the {{
    let mut depth = 1usize;
    let mut end = bytes.len();
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            depth += 1;
            i += 2;
        } else if bytes[i] == b'}' && bytes[i + 1] == b'}' {
            depth -= 1;
            i += 2;
            if depth == 0 {
                end = i - 2;
                break;
            }
        } else {
            i += 1;
        }
    }
    let inner = &wikitext[start + 2..end];
    // Split on `|` at depth 0 (respect nested templates & [[links]]).
    let segments = split_top_pipes(inner);
    for seg in segments.into_iter().skip(1) {
        // first seg is "Characterbox"
        if let Some((k, v)) = seg.split_once('=') {
            let k = k.trim().to_ascii_lowercase();
            let v = clean_wikitext(v.trim());
            if !k.is_empty() && !v.is_empty() {
                out.insert(k, v);
            }
        }
    }
    out
}

fn split_top_pipes(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut depth_br = 0i32; // {{}}
    let mut depth_sq = 0i32; // [[]]
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            depth_br += 1;
            buf.push_str("{{");
            i += 2;
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'}' && bytes[i + 1] == b'}' {
            depth_br -= 1;
            buf.push_str("}}");
            i += 2;
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'[' && bytes[i + 1] == b'[' {
            depth_sq += 1;
            buf.push_str("[[");
            i += 2;
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b']' && bytes[i + 1] == b']' {
            depth_sq -= 1;
            buf.push_str("]]");
            i += 2;
            continue;
        }
        let c = bytes[i] as char;
        if c == '|' && depth_br == 0 && depth_sq == 0 {
            out.push(std::mem::take(&mut buf));
            i += 1;
            continue;
        }
        buf.push(c);
        i += 1;
    }
    out.push(buf);
    out
}

fn clean_wikitext(s: &str) -> String {
    // Strip `[[link|text]]` → text, `[[link]]` → link, HTML comments, `'''`/`''`.
    let s = Regex::new(r"<!--[^-]*-->").unwrap().replace_all(s, "");
    let s = Regex::new(r"\[\[([^|\]]*)\|([^\]]*)\]\]")
        .unwrap()
        .replace_all(&s, "$2");
    let s = Regex::new(r"\[\[([^\]]*)\]\]").unwrap().replace_all(&s, "$1");
    let s = s.replace("'''", "").replace("''", "");
    let s = Regex::new(r"<br\s*/?>").unwrap().replace_all(&s, "\n");
    s.trim().to_string()
}

fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let hay_lc = haystack.to_ascii_lowercase();
    hay_lc.find(&needle.to_ascii_lowercase())
}

/// Pull a bullet list (`*item\n*item`) from a Characterbox field.
fn parse_bullets(val: &str) -> Vec<String> {
    val.lines()
        .map(|l| l.trim_start_matches('*').trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}

// ---------------- Image handling ----------------

async fn download_image(client: &Client, url: &str) -> Result<Vec<u8>> {
    let resp = client.get(url).await?;
    if !resp.status().is_success() {
        anyhow::bail!("image HTTP {}", resp.status());
    }
    let bytes = resp.bytes().await?;
    Ok(bytes.to_vec())
}

fn write_images(bytes: &[u8], id: &str, out_root: &Path, skip_hero: bool) -> Result<u64> {
    let img = image::load_from_memory(bytes)
        .context("decode downloaded image")?;
    let dir = out_root.join("images").join(id);
    std::fs::create_dir_all(&dir)?;

    let mut total = 0u64;

    if !skip_hero {
        // Hero: downscaled to HERO_SAVE_WIDTH, aspect preserved. A full-resolution
        // hero would blow past the repo size budget (~500 figures × ~200KB PNG).
        let (w, _h) = img.dimensions();
        let hero_img = if w > HERO_SAVE_WIDTH {
            img.resize(HERO_SAVE_WIDTH, u32::MAX, FilterType::Lanczos3)
        } else {
            img.clone()
        };
        let hero_path = dir.join("hero.png");
        hero_img.save_with_format(&hero_path, image::ImageFormat::Png)?;
        total += std::fs::metadata(&hero_path)?.len();
    }

    // Thumb: center-cropped square, resized to THUMB_SIZE.
    let (w, h) = img.dimensions();
    let side = w.min(h);
    let x = (w - side) / 2;
    let y = (h - side) / 2;
    let cropped = img.crop_imm(x, y, side, side);
    let thumb = cropped.resize_exact(THUMB_SIZE, THUMB_SIZE, FilterType::Lanczos3);
    let thumb_path = dir.join("thumb.png");
    thumb.save_with_format(&thumb_path, image::ImageFormat::Png)?;
    total += std::fs::metadata(&thumb_path)?.len();

    Ok(total)
}

// ---------------- Candidate-name generation ----------------

/// Build ordered candidate titles for a figure, best guess first.
fn candidate_titles(entry: &InventoryEntry) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut push = |s: String| {
        let t = s.trim().to_string();
        if !t.is_empty() && seen.insert(t.clone()) {
            out.push(t);
        }
    };

    let name = clean_name(&entry.name);
    let clean_group = clean_name(&entry.variant_group);
    let clean_name = name;

    // Creation Crystals: ignore the CRYSTAL_-_ prefix; just use the variant_group.
    if entry.category == "creation-crystal" {
        push(clean_group.clone());
        return out;
    }

    // Imaginators' dash-suffix names ("Hoodsickle-Steelplated"): base comes from variant_group.
    push(clean_name.clone());
    push(clean_group.clone());

    // "Series 2 Eruptor" may redirect; try base too.
    if entry.variant_tag != "base" && clean_group != clean_name {
        push(clean_group.clone());
    }

    // For vehicles: try "<Name> (vehicle)".
    if entry.category == "other" || entry.category == "vehicle" {
        push(format!("{clean_group} (vehicle)"));
    }

    // For traps: try "<Name> (trap)" as a disambiguator.
    if entry.category == "trap" {
        push(format!("{clean_group} (trap)"));
    }

    out
}

/// Normalise a firmware-inventory name for wiki lookup:
///  - Replace underscores with spaces.
///  - Normalise Unicode apostrophe to ASCII (wiki prefers U+2019 but both redirect).
///  - Strip the `.key` second extension from Creation Crystals.
///  - Strip `CRYSTAL_-_` / `CRYSTAL - ` prefixes.
///  - NFC normalise.
fn clean_name(raw: &str) -> String {
    let s: String = raw.nfc().collect();
    let s = s.replace('_', " ");
    let s = s
        .strip_prefix("CRYSTAL -  ")
        .or_else(|| s.strip_prefix("CRYSTAL - "))
        .unwrap_or(&s)
        .to_string();
    // `AIR Lantern` → `Lantern`: drop an uppercase element token at the start.
    let s = Regex::new(r"^(?:AIR|EARTH|FIRE|WATER|LIFE|UNDEAD|TECH|MAGIC|LIGHT|DARK)\s+")
        .unwrap()
        .replace(&s, "")
        .to_string();
    // Drop trailing ".key" (from .key.sky double extension that survived).
    let s = s.trim_end_matches(".key").to_string();
    s.trim().to_string()
}

fn urlencode(s: &str) -> String {
    // MediaWiki likes `_` for spaces in `titles=`, but `%20` also works; accept both.
    // Also need to encode other unsafe chars. Use manual percent-encoding.
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

// ---------------- Main drive ----------------

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let force = args.iter().any(|a| a == "--force");
    let skip_hero = args.iter().any(|a| a == "--no-hero");
    let limit: Option<usize> = args
        .windows(2)
        .find(|w| w[0] == "--limit")
        .and_then(|w| w[1].parse().ok());

    // Find repo root: CWD, else the parent of this crate.
    let repo_root = find_repo_root()?;
    let inventory_path = repo_root.join("docs/research/firmware-inventory.json");
    let data_root = repo_root.join("data");
    let figures_json = data_root.join("figures.json");
    let manual_json = data_root.join("figures.manual.json");
    let license_md = data_root.join("LICENSE.md");

    std::fs::create_dir_all(&data_root)?;
    std::fs::create_dir_all(data_root.join("images"))?;
    if !manual_json.exists() {
        std::fs::write(&manual_json, "{}\n")?;
    }
    if !license_md.exists() {
        std::fs::write(&license_md, LICENSE_MD)?;
    }

    // Load existing figures.json for resume support.
    let existing: HashMap<String, WikiFigure> = if figures_json.exists() && !force {
        let raw = std::fs::read_to_string(&figures_json)?;
        let v: Vec<WikiFigure> = serde_json::from_str(&raw).unwrap_or_default();
        v.into_iter().map(|f| (f.figure_id.clone(), f)).collect()
    } else {
        HashMap::new()
    };

    let raw = std::fs::read_to_string(&inventory_path)
        .with_context(|| format!("reading {}", inventory_path.display()))?;
    let mut entries: Vec<InventoryEntry> = serde_json::from_str(&raw)?;

    // Scrape order: figures first (most likely to have wiki pages), then others.
    entries.sort_by_key(|e| category_priority(&e.category));

    if let Some(n) = limit {
        entries.truncate(n);
        info!(limit = n, "truncating entries for smoke-test");
    }

    let total = entries.len();
    info!(total, "starting wiki scrape");

    let client = Client::new()?;
    let mut results: Vec<WikiFigure> = Vec::with_capacity(total);
    let mut found = 0usize;
    let mut skipped_cached = 0usize;
    let mut failed: Vec<(String, String)> = Vec::new();
    let mut total_bytes: u64 = 0;

    for (i, entry) in entries.iter().enumerate() {
        if !force {
            if let Some(prev) = existing.get(&entry.id) {
                if prev.wiki_page.is_some() {
                    // Image files already committed? skip re-download.
                    let hero = data_root
                        .join("images")
                        .join(&entry.id)
                        .join("hero.png");
                    if hero.exists() {
                        results.push(prev.clone());
                        skipped_cached += 1;
                        found += 1;
                        continue;
                    }
                }
            }
        }

        let progress = format!("[{}/{}]", i + 1, total);
        info!("{progress} scraping {}", entry.name);

        match scrape_one(&client, entry, &data_root, skip_hero).await {
            Ok((fig, bytes)) => {
                if fig.wiki_page.is_some() {
                    found += 1;
                    total_bytes += bytes;
                } else {
                    failed.push((entry.id.clone(), entry.name.clone()));
                    warn!("no wiki page for {}", entry.name);
                }
                results.push(fig);
            }
            Err(e) => {
                warn!("scrape error for {}: {e}", entry.name);
                failed.push((entry.id.clone(), format!("{}: {e}", entry.name)));
                results.push(WikiFigure {
                    figure_id: entry.id.clone(),
                    ..Default::default()
                });
            }
        }

        // Periodic save so a crash doesn't lose progress.
        if (i + 1) % 25 == 0 {
            write_figures(&figures_json, &results)?;
        }
    }

    // Preserve any prior entries for figures that weren't in this run (limit mode).
    if limit.is_some() {
        let present: HashSet<String> = results.iter().map(|f| f.figure_id.clone()).collect();
        for (id, f) in existing {
            if !present.contains(&id) {
                results.push(f);
            }
        }
    }

    write_figures(&figures_json, &results)?;

    let hit_rate = if total == 0 {
        0.0
    } else {
        (found as f64 / total as f64) * 100.0
    };
    info!(
        found,
        failed = failed.len(),
        cached = skipped_cached,
        total,
        hit_rate_pct = format!("{hit_rate:.1}"),
        total_image_bytes = total_bytes,
        "scrape complete"
    );
    println!();
    println!("=============================================");
    println!(" wiki-scrape: found={found}  failed={}  total={total}", failed.len());
    println!(" hit rate: {hit_rate:.1}%");
    println!(" image bytes (this run): {total_bytes}");
    println!("=============================================");
    if !failed.is_empty() {
        println!("Failures ({}):", failed.len());
        for (id, name) in &failed {
            println!("  {id}  {name}");
        }
    }

    Ok(())
}

async fn scrape_one(
    client: &Client,
    entry: &InventoryEntry,
    data_root: &Path,
    skip_hero: bool,
) -> Result<(WikiFigure, u64)> {
    let mut fig = WikiFigure {
        figure_id: entry.id.clone(),
        ..Default::default()
    };

    let candidates = candidate_titles(entry);
    let Some(title) = resolve_title(client, &candidates).await? else {
        return Ok((fig, 0));
    };
    let Some(page) = fetch_page(client, &title).await? else {
        return Ok((fig, 0));
    };
    let final_title = page.title.clone().unwrap_or(title.clone());
    fig.wiki_page = Some(format!(
        "{WIKI_BASE}/wiki/{}",
        final_title.replace(' ', "_")
    ));

    // Parse Characterbox.
    if let Some(rev) = page.revisions.as_ref().and_then(|r| r.first()) {
        if let Some(text) = rev.slots.main.content.as_deref() {
            let cbox = parse_characterbox(text);

            if let Some(soul_gem) = cbox.get("soul gem").or_else(|| cbox.get("soulgem")) {
                fig.soul_gem = Some(soul_gem.clone());
            }
            if let Some(attack) = cbox.get("attack") {
                fig.signature_moves = parse_bullets(attack);
            }
            if let Some(align) = cbox
                .get("alignment")
                .or_else(|| cbox.get("role"))
                .or_else(|| cbox.get("faction"))
            {
                fig.alignment = Some(align.clone());
            }
            fig.attributes = cbox;
        }
    }
    // Fallback alignment from categories for Trap Team traps.
    if fig.alignment.is_none() {
        if let Some(cats) = &page.categories {
            for c in cats {
                let t = c.title.trim_start_matches("Category:");
                if t == "Light Traps" || t == "Dark Traps" {
                    fig.alignment = Some(t.to_string());
                }
            }
        }
    }

    // Image.
    let mut bytes = 0u64;
    if let Some(thumb) = page.thumbnail.as_ref() {
        let hero_dir = data_root.join("images").join(&entry.id);
        let hero_path = hero_dir.join("hero.png");
        let thumb_path = hero_dir.join("thumb.png");
        let need_download = if skip_hero {
            !thumb_path.exists()
        } else {
            !hero_path.exists() || !thumb_path.exists()
        };
        if need_download {
            match download_image(client, &thumb.source).await {
                Ok(raw) => match write_images(&raw, &entry.id, data_root, skip_hero) {
                    Ok(b) => bytes = b,
                    Err(e) => warn!("image write failed for {}: {e}", entry.name),
                },
                Err(e) => warn!("image download failed for {}: {e}", entry.name),
            }
        }
    } else if let Some(pi) = page.pageimage.as_ref() {
        debug!(pageimage = %pi, "no thumbnail URL returned; skipping image");
    }

    Ok((fig, bytes))
}

fn write_figures(path: &Path, figures: &[WikiFigure]) -> Result<()> {
    let mut sorted: Vec<&WikiFigure> = figures.iter().collect();
    sorted.sort_by(|a, b| a.figure_id.cmp(&b.figure_id));
    let json = serde_json::to_string_pretty(&sorted)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn category_priority(cat: &str) -> u8 {
    match cat {
        "figure" | "giant" => 0,
        "sidekick" | "trap" => 1,
        "vehicle" | "other" => 2,
        "creation-crystal" | "kaos" => 3,
        "item" => 4,
        "adventure-pack" => 5,
        _ => 6,
    }
}

fn find_repo_root() -> Result<PathBuf> {
    let mut cur = std::env::current_dir()?;
    loop {
        if cur.join("Cargo.toml").exists() && cur.join("docs/research").exists() {
            return Ok(cur);
        }
        if !cur.pop() {
            anyhow::bail!("couldn't locate repo root (looked for docs/research and Cargo.toml)");
        }
    }
}

const LICENSE_MD: &str = r#"# Data Licensing

The files in this `data/` directory are **derivative of the Skylanders Fandom
wiki** (<https://skylanders.fandom.com>), scraped by `tools/wiki-scrape`.

## Text (`figures.json`, `figures.manual.json`)

Text content (names, soul-gem names, signature moves, alignment) is licensed
**CC BY-SA 3.0** per the Fandom wiki terms.

## Images (`images/**/*.png`)

Images are hero portraits of Skylanders figures. Skylanders characters,
names, and artwork are trademarks and copyright of **Activision Publishing,
Inc.**. This project is an unofficial fan tool; images are used in low
resolution for identification purposes under a fair-use / nominative-fair-use
theory. Users must own the corresponding physical figures and firmware
backups to use this app. No game assets are redistributed.

## Attribution

Wiki pages cited inline per-figure via `wiki_page` in `figures.json`. See the
app's About screen for user-facing attribution copy.
"#;
