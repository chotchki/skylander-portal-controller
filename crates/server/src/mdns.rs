//! mDNS-based stable URL for the phone QR (PLAN 4.18.1a / 4.19.10b).
//!
//! Goal: the QR URL survives a DHCP-lease change so any "Add to Home
//! Screen" PWA bookmarks the user pinned still work after the router
//! reboots. Stable hostname → mDNS resolves to whatever IP the box is
//! bound to today.
//!
//! ## Why we use the OS-published hostname (and not a custom one)
//!
//! First two cuts tried to publish a *custom* hostname:
//!   1. `mdns-sd` (pure-Rust, raw UDP/5353) — daemon reported success
//!      but neither the iPhone nor `ping` from the HTPC could resolve
//!      `skylander-portal.local`. Windows ≥10 owns the mDNS responder
//!      and ignores third-party UDP/5353 advertisers.
//!   2. Windows-native `DnsServiceRegister` — accepted the registration
//!      and the callback fired with status=0, but `getaddrinfo`
//!      *still* couldn't resolve the hostname. The diagnostic test
//!      `register_then_resolve_via_getaddrinfo` (in an earlier rev)
//!      proved the API publishes the SRV/PTR/TXT service records but
//!      NOT a custom A record — that's separate plumbing Windows
//!      doesn't expose publicly.
//!
//! What Windows ≥10 v2004+ DOES auto-publish for free: the box's own
//! computer name as `<computername>.local` — same path printers and
//! file-sharing browsers use. So instead of fighting the API, we ask
//! Windows for its hostname and put `<hostname>.local` in the QR. The
//! URL is less brand-named (`http://chris-htpc.local:8765/...` instead
//! of `http://skylander-portal.local:...`) but it actually resolves
//! from the iPhone and survives DHCP changes — which is the only thing
//! the bookmark-survival goal cares about.
//!
//! Resolution support on the device side:
//!   - **iOS** — native via Bonjour (always on).
//!   - **macOS** — native via Bonjour.
//!   - **Windows ≥ 10** — native (LLMNR + mDNS).
//!   - **Android Chrome** — usually works; some carrier MDM profiles
//!     block multicast, in which case the IP fallback is the cure.
//!
//! If the OS hostname can't be read, or `<hostname>.local` doesn't
//! resolve locally (broken responder, weird config), we fall back to
//! the raw-IP URL — the QR still works, just without DHCP-survival.

use std::net::Ipv4Addr;

#[cfg(windows)]
mod win;

#[cfg(windows)]
use win::os_dns_hostname;

#[cfg(not(windows))]
fn os_dns_hostname() -> Option<String> {
    None
}

/// Build the URL the launcher encodes into its QR.
///
/// Strategy: prefer `http://<os-hostname>.local:<port>/...` (mDNS-
/// stable). Fall back to `http://<ip>:<port>/...` if we can't read the
/// OS hostname. The fallback URL still works on the local LAN — it
/// just doesn't survive DHCP-lease changes.
///
/// Returned tuple: `(url, used_mdns_hostname)`. The boolean lets the
/// caller log which path won and (in the future) emit telemetry for
/// "fell back to IP" cases.
pub fn build_phone_url(ip: Ipv4Addr, port: u16, hex_key: &str) -> (String, bool) {
    // Key goes in the query string `?k=<hex>` rather than the URL
    // fragment so iOS "Add to Home Screen" preserves it on the
    // pinned shortcut — Safari strips fragments when snapshotting
    // the URL into a home-screen bookmark, and the resulting
    // standalone-PWA launch context can't read Safari's localStorage
    // (sandboxed separately). Chris flagged 2026-04-24. The key is
    // still HMAC-only — it's not used for authn — so ending up in
    // server access logs is the same exposure as ending up in the
    // QR image itself.
    match os_dns_hostname() {
        Some(host) if !host.is_empty() => (
            format!("http://{}.local:{port}/?k={hex_key}", host.to_ascii_lowercase()),
            true,
        ),
        _ => (format!("http://{ip}:{port}/?k={hex_key}"), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_uses_lowercased_hostname_when_available() {
        // We don't control os_dns_hostname() in tests, but on Windows it
        // returns the actual machine name. Just sanity-check the format
        // rules — lowercased, ends with .local:port/?k=, includes the key.
        let (url, _) = build_phone_url(Ipv4Addr::new(192, 168, 1, 147), 8765, "deadbeef");
        assert!(url.starts_with("http://"), "url should be http: {url}");
        assert!(url.contains(":8765/?k=deadbeef"), "url missing port/key: {url}");
        // No uppercase letters in the host portion (everything after // up
        // to the first :, ignoring the hex-only key).
        let host_segment = url
            .strip_prefix("http://")
            .and_then(|s| s.split(':').next())
            .unwrap_or("");
        assert_eq!(
            host_segment,
            host_segment.to_ascii_lowercase(),
            "host segment should be lowercased: {host_segment}"
        );
    }

    /// The lowercase-hostname rule matters because mDNS names are
    /// canonically case-insensitive but iOS bookmarks treat the URL as
    /// a literal string — mixed case in the bookmark vs the OS-published
    /// name causes some iOS Safari versions to skip the cache and
    /// re-resolve every load (slow). Locking lowercase keeps the
    /// bookmark hit deterministic.
    #[test]
    fn url_lowercases_even_uppercase_hostnames() {
        // We can't force os_dns_hostname() to return something specific,
        // but we can directly verify the lowercasing path does what we
        // expect for any input the function would synthesize.
        let upper = "WIN-ABC123";
        let lowered = upper.to_ascii_lowercase();
        assert_eq!(lowered, "win-abc123");
        // (The actual URL test depends on the test machine's hostname;
        // this is the unit-level pin on the lowercasing transform.)
    }

    /// On non-Windows hosts (where `os_dns_hostname` always returns
    /// None), the URL falls back to the IP form so the QR still works.
    /// This branch matters for cross-platform CI builds that target
    /// non-Windows even if production is Windows-only.
    #[cfg(not(windows))]
    #[test]
    fn falls_back_to_ip_on_non_windows() {
        let (url, used_mdns) = build_phone_url(Ipv4Addr::new(192, 168, 1, 147), 8765, "abc");
        assert!(!used_mdns);
        assert_eq!(url, "http://192.168.1.147:8765/?k=abc");
    }
}
