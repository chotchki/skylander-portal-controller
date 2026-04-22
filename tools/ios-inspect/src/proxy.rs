//! `ios_webkit_debug_proxy` lifecycle + HTTP discovery.
//!
//! The proxy binary must be on `PATH` (installed via `brew install
//! ios-webkit-debug-proxy`). The sim-webinspector socket path is dynamic
//! (created per launchd boot under `/private/tmp/com.apple.launchd.*/`),
//! so we glob for it at boot time.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
/// Find the *live* webinspectord_sim.socket path. Uses `lsof -U` to find
/// the socket that `launchd_s` is currently holding open — glob-based
/// discovery is unreliable because the sim creates new sockets under fresh
/// `launchd.*` paths each time webinspectord_sim restarts (which happens
/// whenever our proxy dies, causing stale files to linger alongside live
/// ones in `/private/tmp` + `/private/var/tmp`).
pub async fn wait_for_sim_socket(timeout: Duration) -> Result<PathBuf> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(p) = find_live_sim_socket().await? {
            return Ok(p);
        }
        if Instant::now() >= deadline {
            bail!(
                "no live webinspectord_sim.socket found — is Simulator running? \
                 Try `open -a Simulator` manually and re-run."
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Use `lsof -U` to find the webinspectord_sim unix socket that `launchd_s`
/// is currently serving. Returns None if no such socket is open.
pub async fn find_live_sim_socket() -> Result<Option<PathBuf>> {
    let out = tokio::process::Command::new("lsof")
        .args(["-U", "-c", "launchd_s"])
        .output()
        .await
        .context("run lsof")?;
    // lsof returns non-zero if there are no matches; ignore status.
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        if !line.contains("webinspectord_sim.socket") {
            continue;
        }
        // Last whitespace-delimited token is the socket path.
        if let Some(path) = line.split_whitespace().last() {
            let pb = PathBuf::from(path);
            if pb.exists() {
                return Ok(Some(pb));
            }
        }
    }
    Ok(None)
}

/// Spawn `ios_webkit_debug_proxy -s unix:<socket> -f chrome-devtools://...`
/// detached from this process so it survives across CLI invocations. Returns
/// the child PID.
pub async fn spawn(socket: &Path) -> Result<u32> {
    use std::os::unix::process::CommandExt;

    let sock_arg = format!("unix:{}", socket.display());
    let mut cmd = std::process::Command::new("ios_webkit_debug_proxy");
    cmd.args([
        "-s",
        &sock_arg,
        "-f",
        "chrome-devtools://devtools/bundled/inspector.html",
    ])
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null());

    // `setsid` puts the child in a new process group so when our CLI exits,
    // the proxy doesn't receive SIGHUP.
    unsafe {
        cmd.pre_exec(|| {
            nix_setsid()?;
            Ok(())
        });
    }

    let child = cmd.spawn().context(
        "spawn ios_webkit_debug_proxy — is it installed? \
         `brew install ios-webkit-debug-proxy`",
    )?;
    Ok(child.id())
}

/// Poor-man's setsid via libc — avoid pulling in the `nix` crate for one
/// syscall. Returns io::Result so pre_exec is happy.
fn nix_setsid() -> std::io::Result<()> {
    // SAFETY: setsid is async-signal-safe (per POSIX).
    let rc = unsafe { libc_setsid() };
    if rc == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

extern "C" {
    #[link_name = "setsid"]
    fn libc_setsid() -> i32;
}

pub fn is_running(pid: u32) -> bool {
    // kill(pid, 0) returns 0 if the process exists and we have permission.
    // SAFETY: signal 0 is the "existence check" — no side effects.
    let rc = unsafe { libc_kill(pid as i32, 0) };
    rc == 0
}

pub fn kill(pid: u32) -> Result<()> {
    // SIGTERM = 15.
    let rc = unsafe { libc_kill(pid as i32, 15) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(3) {
            // ESRCH — process already gone.
            return Ok(());
        }
        return Err(err).context("kill proxy");
    }
    Ok(())
}

extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}

/// Wait for the proxy's HTTP endpoint at localhost:9221 to become ready.
pub async fn wait_for_ready(timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let client = reqwest::Client::new();
    loop {
        match client
            .get("http://localhost:9221/")
            .timeout(Duration::from_millis(400))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => return Ok(()),
            _ => {}
        }
        if Instant::now() >= deadline {
            bail!("ios_webkit_debug_proxy didn't become ready within {timeout:?}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

pub struct Tab {
    pub page_num: u32,
    pub title: String,
    pub url: String,
    pub ws_url: String,
}

/// Query the proxy for the simulator's device page, then parse the listing
/// of tabs out of its HTML. The proxy doesn't expose a JSON endpoint (at
/// least not a documented one); HTML parsing is stable enough for our use
/// since the output is mechanical.
pub async fn list_tabs() -> Result<Vec<Tab>> {
    let port = device_port().await?;
    let html = reqwest::get(format!("http://localhost:{port}/"))
        .await?
        .text()
        .await?;
    Ok(parse_tabs_html(&html, port))
}

async fn device_port() -> Result<u16> {
    let html = reqwest::get("http://localhost:9221/").await?.text().await?;
    // Proxy formats the device list in one of two ways:
    //   <a href="http://localhost:9222/">localhost:9222</a>    (USB device)
    //   <a>localhost:9222</a>                                  (simulator)
    // Both contain "localhost:<port>" — grab the first such occurrence.
    let needle = "localhost:";
    let start = html
        .find(needle)
        .context("proxy root has no device entries — is the simulator booted?")?
        + needle.len();
    let tail = &html[start..];
    let digits_end = tail
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(tail.len());
    if digits_end == 0 {
        bail!("proxy HTML had `localhost:` with no port number");
    }
    tail[..digits_end]
        .parse::<u16>()
        .context("parse device port from proxy HTML")
}

fn parse_tabs_html(html: &str, port: u16) -> Vec<Tab> {
    // Each tab entry looks like:
    //   <li value="1"><a href="chrome-devtools://...?ws=localhost:9222/devtools/page/1"
    //                    title="Skylander Portal">http://192.168.1.155:8090/</a></li>
    // Pull out the `ws=localhost:<port>/devtools/page/<N>`, the title, and the
    // visible URL (the `<a>` text node).
    let mut tabs = Vec::new();
    let ws_needle = format!("ws=localhost:{port}/devtools/page/");
    let mut cursor = 0;
    while let Some(rel) = html[cursor..].find(&ws_needle) {
        let idx = cursor + rel + ws_needle.len();
        // Page number runs to the next non-digit.
        let digits_end = html[idx..]
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(html[idx..].len());
        let Ok(page_num) = html[idx..idx + digits_end].parse::<u32>() else {
            break;
        };
        // Title: look backwards/forwards to find `title="..."`.
        let li_start = html[..idx].rfind("<li").unwrap_or(cursor);
        let li_end = html[idx..].find("</li>").map(|p| idx + p).unwrap_or(html.len());
        let li = &html[li_start..li_end];
        let title = extract_attr(li, "title=\"").unwrap_or_default();
        // URL: text between `">` and `</a>` of the first <a> in this <li>.
        let url = li
            .find(">h")
            .and_then(|s| li[s + 1..].find("</a>").map(|e| li[s + 1..s + 1 + e].to_string()))
            .unwrap_or_default();
        tabs.push(Tab {
            page_num,
            title,
            url,
            ws_url: format!("ws://localhost:{port}/devtools/page/{page_num}"),
        });
        cursor = li_end;
    }
    tabs
}

fn extract_attr(s: &str, key: &str) -> Option<String> {
    let start = s.find(key)? + key.len();
    let end = s[start..].find('"')?;
    Some(s[start..start + end].to_string())
}

/// Pick the most recently-registered tab (highest page number) — matches
/// the spike's "just grab the active page" heuristic. If the user wants
/// explicit control, they can add `--tab N` later.
pub async fn pick_current_tab() -> Result<Tab> {
    let tabs = list_tabs().await?;
    if tabs.is_empty() {
        bail!("no Safari tabs visible to the proxy — open a page with `ios-inspect open <url>`");
    }
    Ok(tabs.into_iter().max_by_key(|t| t.page_num).unwrap())
}
