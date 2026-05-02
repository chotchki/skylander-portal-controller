//! macOS path: read the Bonjour-published LocalHostName via
//! `scutil --get LocalHostName`. macOS's `mDNSResponder` automatically
//! publishes `<LocalHostName>.local` for any device on the LAN — same
//! mechanism iOS Bonjour clients use, and the same name iOS / macOS /
//! Windows ≥10 LLMNR clients can resolve.
//!
//! Why `scutil` and not BSD `gethostname(2)`: on Mac the BSD hostname
//! is whatever `sudo scutil --set HostName` was set to (often unset on
//! freshly-imaged Macs), and may differ from the Bonjour name —
//! `LocalHostName` is the one that actually goes out on the wire.
//!
//! We could link `SystemConfiguration.framework`'s
//! `SCDynamicStoreCopyLocalHostName` for the same data without a
//! subprocess, but the framework adds a build-time dep for ~50µs of
//! saved runtime on a once-per-launch read. Not worth the link.

use std::process::Command;

/// Read the Bonjour-published LocalHostName. Returns `None` if the
/// `scutil` call fails or returns an empty value (very unusual on a
/// Mac with a working network stack).
pub fn os_dns_hostname() -> Option<String> {
    let out = Command::new("scutil")
        .args(["--get", "LocalHostName"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::ToSocketAddrs;
    use std::time::Duration;

    /// On any Mac with a working network stack `scutil --get LocalHostName`
    /// returns the Bonjour name. If this test ever fails, `scutil` is
    /// missing from PATH or the system has no LocalHostName configured —
    /// either way the launcher silently falls back to the raw-IP URL.
    #[test]
    fn os_hostname_is_readable() {
        let host = os_dns_hostname().expect(
            "scutil --get LocalHostName should always return a value on a healthy Mac",
        );
        assert!(!host.is_empty(), "LocalHostName should not be empty");
        assert!(host.len() < 256, "LocalHostName suspiciously long: {host}");
    }

    /// Diagnostic: ask the OS resolver to actually find
    /// `<LocalHostName>.local` over Bonjour. Equivalent of the
    /// Windows-side `os_hostname_resolves_via_local`. Gated `#[ignore]`
    /// because it requires a working network + mDNSResponder running
    /// (the latter is enabled by default on every Mac, but Wi-Fi-off
    /// CI runners would fail).
    ///
    /// Run explicitly:
    ///
    ///     cargo test -p skylander-server --lib mdns::mac::tests \
    ///         -- --ignored --nocapture
    #[test]
    #[ignore = "requires working network + mDNSResponder; run explicitly"]
    fn os_hostname_resolves_via_local() {
        let host = os_dns_hostname().expect("read LocalHostName");
        let target = format!("{}.local:8765", host.to_ascii_lowercase());

        std::thread::sleep(Duration::from_millis(500));

        let resolved = target
            .to_socket_addrs()
            .map(|iter| iter.collect::<Vec<_>>())
            .unwrap_or_else(|e| {
                panic!(
                    "OS resolver failed to resolve {target}: {e}\n\
                     macOS's Bonjour responder isn't publishing the local \
                     hostname. The launcher will fall back to the raw-IP \
                     URL form, which works but doesn't survive DHCP changes."
                );
            });
        assert!(
            !resolved.is_empty(),
            "OS resolver returned empty for {target}"
        );
    }
}
