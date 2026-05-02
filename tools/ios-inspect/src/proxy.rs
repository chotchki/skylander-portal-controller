//! `ios_webkit_debug_proxy` lifecycle + HTTP discovery.
//!
//! The proxy binary must be on `PATH` (installed via `brew install
//! ios-webkit-debug-proxy`). The sim-webinspector socket path is
//! dynamic (created per launchd boot under
//! `/private/tmp/com.apple.launchd.*/`), so we discover it via `lsof`
//! at boot time.
//!
//! **Multi-device model (PLAN 10.2):** one proxy per booted simulator,
//! each with its own port pair (control + device). Spawned with
//! `-s unix:<sock> -p <ctrl>:<dev>`. The control port serves the
//! HTML root listing; the device port serves the WS endpoints. State
//! pins both ports so cross-process commands don't have to re-parse
//! HTML to find the right endpoint.

use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Find every *live* webinspectord_sim.socket open by `launchd_s`.
/// Returns the deduplicated set of paths. Uses `lsof -U` because
/// glob-based discovery is unreliable — the sim creates new sockets
/// under fresh `launchd.*` paths each time webinspectord_sim restarts
/// (which happens whenever a proxy attached to it dies, leaving stale
/// files alongside live ones).
pub async fn find_live_sim_sockets() -> Result<Vec<PathBuf>> {
    let out = tokio::process::Command::new("lsof")
        .args(["-U", "-c", "launchd_s"])
        .output()
        .await
        .context("run lsof")?;
    // lsof returns non-zero if there are no matches; ignore status.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut out_paths: Vec<PathBuf> = Vec::new();
    for line in stdout.lines() {
        if !line.contains("webinspectord_sim.socket") {
            continue;
        }
        if let Some(path) = line.split_whitespace().last() {
            let pb = PathBuf::from(path);
            if pb.exists() && seen.insert(pb.clone()) {
                out_paths.push(pb);
            }
        }
    }
    Ok(out_paths)
}

/// Wait for at least one new webinspectord_sim socket to appear that
/// wasn't in the `before` snapshot. Returns the first new path. Used
/// at boot time to attribute a freshly-created socket to the
/// just-booted simulator (boot sequentially, snapshot-diff, claim).
pub async fn wait_for_new_socket(
    before: &HashSet<PathBuf>,
    timeout: Duration,
) -> Result<PathBuf> {
    let deadline = Instant::now() + timeout;
    loop {
        let now = find_live_sim_sockets().await?;
        for p in now {
            if !before.contains(&p) {
                return Ok(p);
            }
        }
        if Instant::now() >= deadline {
            bail!(
                "no new webinspectord_sim.socket appeared within {timeout:?} — \
                 is Simulator running and did the sim finish booting?"
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Spawn `ios_webkit_debug_proxy -s unix:<socket> -c
/// "null:<ctrl>,:<dev>"` detached from this process so it survives
/// across CLI invocations. Returns the child PID.
///
/// Flag note: the proxy uses `-c` (config CSV) for port mapping, NOT
/// `-p`. `null:<port>` is the listing/control port; the bare `:<port>`
/// is "any device on this port". Spawning per-device with a single-
/// port range pins each device's WS endpoint to a deterministic port.
pub async fn spawn(socket: &Path, control_port: u16, device_port: u16) -> Result<u32> {
    use std::os::unix::process::CommandExt;

    let sock_arg = format!("unix:{}", socket.display());
    let cfg_arg = format!("null:{control_port},:{device_port}");
    let mut cmd = std::process::Command::new("ios_webkit_debug_proxy");
    cmd.args([
        "-s",
        &sock_arg,
        "-c",
        &cfg_arg,
        "-f",
        "chrome-devtools://devtools/bundled/inspector.html",
    ])
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null());

    // `setsid` puts the child in a new process group so when our CLI
    // exits, the proxy doesn't receive SIGHUP.
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

/// Poor-man's setsid via libc — avoid pulling in the `nix` crate for
/// one syscall. Returns io::Result so pre_exec is happy.
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

/// Wait for a specific proxy's HTTP control port to start accepting
/// requests.
pub async fn wait_for_ready(control_port: u16, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let client = reqwest::Client::new();
    let url = format!("http://localhost:{control_port}/");
    loop {
        match client
            .get(&url)
            .timeout(Duration::from_millis(400))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => return Ok(()),
            _ => {}
        }
        if Instant::now() >= deadline {
            bail!("ios_webkit_debug_proxy on :{control_port} didn't become ready within {timeout:?}");
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

/// Query the device-port HTML listing for the tabs visible on a
/// specific device. The proxy doesn't expose a JSON tab endpoint;
/// HTML parsing is stable enough for our use since the output is
/// mechanical and the format hasn't changed in years.
pub async fn list_tabs(device_port: u16) -> Result<Vec<Tab>> {
    let html = reqwest::get(format!("http://localhost:{device_port}/"))
        .await?
        .text()
        .await?;
    Ok(parse_tabs_html(&html, device_port))
}

fn parse_tabs_html(html: &str, port: u16) -> Vec<Tab> {
    // Each tab entry looks like:
    //   <li value="1"><a href="chrome-devtools://...?ws=localhost:9222/devtools/page/1"
    //                    title="Skylander Portal">http://192.168.1.155:8090/</a></li>
    // Pull out the `ws=localhost:<port>/devtools/page/<N>`, the title,
    // and the visible URL (the `<a>` text node).
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

/// Pick the most recently-registered tab (highest page number) for a
/// given device. Matches the spike's "just grab the active page"
/// heuristic.
pub async fn pick_current_tab(device_port: u16) -> Result<Tab> {
    let tabs = list_tabs(device_port).await?;
    if tabs.is_empty() {
        bail!(
            "no Safari tabs visible to the proxy on :{device_port} — \
             open a page with `ios-inspect open <url>`"
        );
    }
    Ok(tabs.into_iter().max_by_key(|t| t.page_num).unwrap())
}
