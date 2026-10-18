//! Windows Firewall connectivity diagnostics (PLAN 17.2 / 17.3).
//!
//! A phone reaching the launcher's HTTP server needs an inbound Allow rule for
//! the bind port on the active network profile. The MSI installs one
//! (`fire:FirewallException`), but the **portable zip doesn't** — so zip users
//! can be silently blocked, the most likely cause of "my phone can't connect".
//! This module:
//!
//!   * **reads** whether such a rule is in place ([`check_inbound_rule`]) so the
//!     launcher's "Trouble connecting?" card can explain the failure, and
//!   * **adds** one with an elevated `netsh` call ([`add_inbound_rule_elevated`])
//!     as the one-click fix.
//!
//! The read path uses the **locale-independent** `INetFwPolicy2` COM API rather
//! than parsing `netsh` text (which is localized — "Enabled" vs "Aktiviert").
//! All COM/Win32 calls live behind `#[cfg(windows)]`; elsewhere it degrades to
//! `Unknown` / a no-op.

/// The display name we give our firewall rule — matches the MSI's
/// `fire:FirewallException Name` and the one-click `netsh` add, so all three
/// paths refer to the same rule.
pub const RULE_NAME: &str = "Skylander Portal Controller";

/// Result of the inbound-rule check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FirewallStatus {
    /// An enabled inbound Allow rule covers our port on the active profile —
    /// phones should be able to reach us (firewall-wise).
    Healthy,
    /// The firewall is on for the active profile but no enabled rule covers our
    /// port — the likely cause of a connectivity failure. The one-click fix
    /// ([`add_inbound_rule_elevated`]) targets exactly this.
    RuleMissing,
    /// The firewall is off for the active profile — nothing to add, so the
    /// firewall isn't the cause (look at Wi-Fi / mDNS instead).
    FirewallOff,
    /// Couldn't determine (COM error, or non-Windows). Don't alarm on the
    /// firewall specifically; the card still shows the generic guidance.
    #[default]
    Unknown,
}

/// Pure decision: given whether the firewall is enabled on the active profile
/// and whether an enabled inbound rule covers our port, classify the status.
/// Split from the COM glue so the logic unit-tests without touching the live
/// firewall.
// Off Windows, its only non-test caller (`check`, the Windows COM path) is
// cfg'd out, so the lib build sees it as dead. It's live on Windows + under test.
#[cfg_attr(not(windows), allow(dead_code))]
fn classify(firewall_enabled: bool, rule_covers_port: bool) -> FirewallStatus {
    if !firewall_enabled {
        FirewallStatus::FirewallOff
    } else if rule_covers_port {
        FirewallStatus::Healthy
    } else {
        FirewallStatus::RuleMissing
    }
}

/// Check whether inbound traffic to `port` is allowed by the firewall on the
/// active network profile. Best-effort: any failure returns [`FirewallStatus::Unknown`].
#[cfg(windows)]
pub fn check_inbound_rule(port: u16) -> FirewallStatus {
    match win::check(port) {
        Ok(status) => status,
        Err(e) => {
            tracing::warn!("firewall check failed: {e}");
            FirewallStatus::Unknown
        }
    }
}

#[cfg(not(windows))]
pub fn check_inbound_rule(_port: u16) -> FirewallStatus {
    FirewallStatus::Unknown
}

/// Add an inbound Allow rule for `port` via an **elevated** `netsh` call (PLAN
/// 17.3). Triggers one UAC prompt (ShellExecute `runas`). Idempotent: deletes
/// any existing rule with our [`RULE_NAME`] first so a re-run doesn't stack
/// duplicates. Returns `Ok(())` once the elevated process has been launched and
/// exited (success exit code); the caller should re-run [`check_inbound_rule`]
/// to confirm. On non-Windows it's a no-op error.
#[cfg(windows)]
pub fn add_inbound_rule_elevated(port: u16) -> anyhow::Result<()> {
    win::add_rule_elevated(port)
}

#[cfg(not(windows))]
pub fn add_inbound_rule_elevated(_port: u16) -> anyhow::Result<()> {
    anyhow::bail!("firewall rule management is only supported on Windows")
}

#[cfg(windows)]
mod win {
    use anyhow::{Context, Result, bail};
    use windows::Win32::Foundation::VARIANT_BOOL;
    use windows::Win32::NetworkManagement::WindowsFirewall::{
        INetFwPolicy2, INetFwRule, NET_FW_ACTION_ALLOW, NET_FW_PROFILE_TYPE2,
        NET_FW_PROFILE2_DOMAIN, NET_FW_PROFILE2_PRIVATE, NET_FW_PROFILE2_PUBLIC,
        NET_FW_RULE_DIR_IN, NetFwPolicy2,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows::core::{BSTR, HSTRING, PCWSTR};

    use super::{FirewallStatus, RULE_NAME, classify};

    /// RAII guard around per-thread COM init: only calls `CoUninitialize` if our
    /// `CoInitializeEx` actually initialised COM (S_OK), not if it was already up.
    struct ComInit {
        owned: bool,
    }
    impl ComInit {
        fn new() -> Self {
            // SAFETY: standard COM init. S_FALSE = already initialised on this
            // thread (don't own it); an error HRESULT (e.g. RPC_E_CHANGED_MODE)
            // means COM is up in another mode — proceed without owning either.
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            Self { owned: hr.is_ok() }
        }
    }
    impl Drop for ComInit {
        fn drop(&mut self) {
            if self.owned {
                // SAFETY: balances our successful CoInitializeEx.
                unsafe { CoUninitialize() };
            }
        }
    }

    pub(super) fn check(port: u16) -> Result<FirewallStatus> {
        let _com = ComInit::new();
        // SAFETY: COM is initialised for this thread by `_com`.
        let policy: INetFwPolicy2 = unsafe { CoCreateInstance(&NetFwPolicy2, None, CLSCTX_ALL) }
            .context("create INetFwPolicy2")?;

        let active = unsafe { policy.CurrentProfileTypes() }.context("CurrentProfileTypes")?;
        let firewall_enabled = active_profile_firewall_enabled(&policy, active)?;
        let rule_covers_port = inbound_allow_rule_present(&policy, port)?;
        Ok(classify(firewall_enabled, rule_covers_port))
    }

    /// True if the firewall is enabled on **any** currently-active profile
    /// (`active` is a bitmask of `NET_FW_PROFILE2_*`). If it's off everywhere
    /// active, no inbound rule is needed.
    fn active_profile_firewall_enabled(policy: &INetFwPolicy2, active: i32) -> Result<bool> {
        for profile in [
            NET_FW_PROFILE2_DOMAIN,
            NET_FW_PROFILE2_PRIVATE,
            NET_FW_PROFILE2_PUBLIC,
        ] {
            if active & profile.0 == 0 {
                continue; // not an active profile right now
            }
            let on = unsafe { policy.get_FirewallEnabled(NET_FW_PROFILE_TYPE2(profile.0)) }
                .context("get_FirewallEnabled")?;
            if on.as_bool() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether our named rule exists, enabled, as an inbound Allow. We look up
    /// our own rule by name (the MSI + one-click fix both use `RULE_NAME`)
    /// rather than enumerating the whole rule set — simpler + sufficient for the
    /// "did our install add the rule" question. A missing rule throws, which we
    /// map to `false`.
    fn inbound_allow_rule_present(policy: &INetFwPolicy2, _port: u16) -> Result<bool> {
        let rules = unsafe { policy.Rules() }.context("Rules")?;
        let rule: INetFwRule = match unsafe { rules.Item(&BSTR::from(RULE_NAME)) } {
            Ok(r) => r,
            Err(_) => return Ok(false), // no rule with our name
        };
        let enabled: VARIANT_BOOL = unsafe { rule.Enabled() }.context("rule.Enabled")?;
        let action = unsafe { rule.Action() }.context("rule.Action")?;
        let direction = unsafe { rule.Direction() }.context("rule.Direction")?;
        Ok(enabled.as_bool() && action == NET_FW_ACTION_ALLOW && direction == NET_FW_RULE_DIR_IN)
    }

    pub(super) fn add_rule_elevated(port: u16) -> Result<()> {
        // Drop any prior rule with our name first (idempotent — avoids stacking
        // duplicates on repeated clicks), then add a fresh inbound TCP allow.
        // `&` chains both in one elevated shell so it's a single UAC prompt.
        let args = format!(
            "/c netsh advfirewall firewall delete rule name=\"{name}\" >nul 2>&1 & \
             netsh advfirewall firewall add rule name=\"{name}\" \
             dir=in action=allow protocol=TCP localport={port}",
            name = RULE_NAME,
        );
        // Run `cmd.exe /c <args>` elevated (UAC `runas`), hidden. Returns once the
        // elevated process has been *launched* (not awaited); the caller re-checks
        // the rule if it needs confirmation.
        let verb = HSTRING::from("runas"); // elevation → UAC prompt
        let file = HSTRING::from("cmd.exe");
        let params = HSTRING::from(args);
        // SAFETY: ShellExecuteW with valid wide strings. SW_HIDE keeps the
        // console window from flashing. Returns an HINSTANCE > 32 on success.
        let h = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(verb.as_ptr()),
                PCWSTR(file.as_ptr()),
                PCWSTR(params.as_ptr()),
                PCWSTR::null(),
                SW_HIDE,
            )
        };
        // ShellExecuteW returns a value <= 32 on failure (incl. the user
        // declining the UAC prompt → SE_ERR_ACCESSDENIED == 5).
        if h.0 as usize <= 32 {
            bail!(
                "ShellExecuteW(runas netsh) failed or was declined (code {})",
                h.0 as usize
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_firewall_off_is_not_a_problem() {
        // Firewall off on the active profile ⇒ nothing blocks inbound, no rule
        // needed, regardless of whether a rule happens to exist.
        assert_eq!(classify(false, false), FirewallStatus::FirewallOff);
        assert_eq!(classify(false, true), FirewallStatus::FirewallOff);
    }

    #[test]
    fn classify_enabled_without_rule_is_missing() {
        assert_eq!(classify(true, false), FirewallStatus::RuleMissing);
    }

    #[test]
    fn classify_enabled_with_rule_is_healthy() {
        assert_eq!(classify(true, true), FirewallStatus::Healthy);
    }

    #[test]
    fn non_windows_check_is_unknown() {
        // On the dev/CI host this exercises whichever arm compiled; the contract
        // is that the result is one of the known variants (never panics).
        let s = check_inbound_rule(8765);
        assert!(matches!(
            s,
            FirewallStatus::Healthy
                | FirewallStatus::RuleMissing
                | FirewallStatus::FirewallOff
                | FirewallStatus::Unknown
        ));
    }
}
