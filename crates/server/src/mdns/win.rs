//! Windows path: read the OS's DNS hostname (the same name Windows
//! auto-publishes via mDNS for `<name>.local` resolution). See parent
//! module doc for the rationale on going this route instead of trying
//! to publish a custom hostname via DnsServiceRegister.

use windows::Win32::System::SystemInformation::{
    ComputerNameDnsHostname, GetComputerNameExW,
};
use windows::core::PWSTR;

/// Read the local machine's DNS hostname — the same string Windows
/// publishes as `<hostname>.local` via its built-in mDNS responder.
/// Returns `None` if the API call fails (which would be very unusual
/// on a healthy Windows install).
pub fn os_dns_hostname() -> Option<String> {
    // Two-pass call: first ask for the required size, then allocate
    // and read. Windows's ComputerName APIs use this length-then-buffer
    // pattern across the board.
    let mut required: u32 = 0;
    unsafe {
        // First call expects a NULL buffer + zero size; it sets
        // `required` to the needed length (in WCHARs, including the
        // null terminator) and returns ERROR_MORE_DATA. The Rust
        // binding maps that to Err(_), which we ignore — we only
        // care about the size in `required`.
        let _ = GetComputerNameExW(ComputerNameDnsHostname, None, &mut required);
    }
    if required == 0 {
        return None;
    }

    let mut buf: Vec<u16> = vec![0u16; required as usize];
    let ok = unsafe {
        GetComputerNameExW(
            ComputerNameDnsHostname,
            Some(PWSTR(buf.as_mut_ptr())),
            &mut required,
        )
    };
    if ok.is_err() {
        return None;
    }

    // After success, `required` is the length WITHOUT the trailing
    // null. Truncate, then convert from UTF-16 LE.
    buf.truncate(required as usize);
    String::from_utf16(&buf).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::ToSocketAddrs;
    use std::time::Duration;

    /// On any Windows host the OS hostname must be readable and non-
    /// empty. If this test ever fails, something's broken in the OS
    /// install — flag it loudly rather than silently falling back to
    /// the IP form.
    #[test]
    fn os_hostname_is_readable() {
        let host = os_dns_hostname()
            .expect("GetComputerNameExW should always succeed on a Windows host");
        assert!(!host.is_empty(), "OS hostname should not be empty");
        // Reasonable upper bound — Windows caps NetBIOS names at 15 chars,
        // DNS hostnames at 63. 256 covers either with room to spare.
        assert!(host.len() < 256, "OS hostname suspiciously long: {host}");
    }

    /// Diagnostic test for the actual mDNS publishing path: ask the OS
    /// resolver to resolve `<os-hostname>.local`. If this succeeds,
    /// Windows's built-in mDNS responder is publishing the local
    /// hostname properly and our QR URL will work for any client on
    /// the LAN that can do mDNS (iOS, macOS, Windows ≥10 v2004).
    ///
    /// Gated `#[ignore]` because it requires a working network +
    /// mDNS-enabled DNS Client service. Run explicitly with:
    ///
    ///     cargo test -p skylander-server --lib mdns::win::tests \
    ///         -- --ignored --nocapture
    ///
    /// If this test fails on the dev box, the launcher will silently
    /// fall back to the IP-form URL — the launcher still works,
    /// PWA bookmarks just don't survive DHCP changes.
    #[test]
    #[ignore = "requires working network + mDNS responder; run explicitly"]
    fn os_hostname_resolves_via_local() {
        let host = os_dns_hostname().expect("read OS hostname");
        let target = format!("{}.local:8765", host.to_ascii_lowercase());

        // Give the responder a moment in case the test runs immediately
        // after a network state change.
        std::thread::sleep(Duration::from_millis(500));

        let resolved = target
            .to_socket_addrs()
            .map(|iter| iter.collect::<Vec<_>>())
            .unwrap_or_else(|e| {
                panic!(
                    "OS resolver failed to resolve {target}: {e}\n\
                     Windows's built-in mDNS responder isn't publishing the \
                     local hostname. The launcher will fall back to the raw-IP \
                     URL form, which works but doesn't survive DHCP changes."
                );
            });
        assert!(
            !resolved.is_empty(),
            "OS resolver returned empty for {target}"
        );
    }
}
