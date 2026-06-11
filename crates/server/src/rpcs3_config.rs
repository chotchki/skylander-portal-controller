//! Ensure the user's RPCS3 emulator `config.yml` carries the one `Net`
//! configuration Skylanders games actually run under (PLAN 16.10.1).
//!
//! **Why this exists.** The HTPC live-debug (2026-05-30/31) traced the
//! "flapping" / freezes / "crashed on placement" to a single root cause: with
//! RPCS3 `Net: Internet enabled: Disconnected`, Skylanders: Spyro's Adventure
//! hits connect-errors on its **hardcoded `8.8.8.8` DNS probe** and
//! busy-retry-storms forever → ~96% CPU on every core → the RSX/render thread is
//! starved → the 8 s freeze-watchdog trips → restart = the "flap". The proven
//! stable mode is `Internet enabled: Connected` (the probe *connects* instead of
//! storming) + `PSN status: Disconnected` (so the game never reaches RPCN's
//! "online-required" fatal). DNS `8.8.8.8` just matches what the game hardcodes.
//!
//! **Why the controller writes it.** The v1 distribution model (PLAN 16.9.0b)
//! points RPCS3 at the *user's existing install* as `config_dir`, so these keys
//! live in the **user's** `<config_dir>/config.yml`, not in anything we ship —
//! the controller has to ensure them at startup. This is a narrow, justified
//! exception to the 16.9.1/.2 "write no RPCS3 config" deferral: without these
//! keys the games are *unplayable* (they freeze), so it's *required* config, not
//! tuning.
//!
//! **How.** [`ensure_rpcs3_net_config`] reads the file, sets exactly the three
//! keys (creating the `Net:` block if absent), preserves every other key
//! verbatim, and writes back only if something changed (idempotent — a second
//! run is a no-op). We deliberately do **not** pull in a YAML dependency: like
//! `games_yml`, the file is RPCS3-controlled with a predictable 2-space-indent
//! layout (every `Net:` child is a flat scalar), so a surgical line edit
//! preserves the file byte-for-byte except the keys we touch — safer than a
//! parse→reserialize round-trip that would reorder/reformat the whole file.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The three `Net:` keys we pin, written in RPCS3's own alphabetical key order
/// so an appended block matches what RPCS3 would emit on its next save.
const NET_KEYS: &[(&str, &str)] = &[
    ("DNS address", "8.8.8.8"),
    ("Internet enabled", "Connected"),
    ("PSN status", "Disconnected"),
];

/// Ensure `<config_dir>/config.yml` has the proven-stable `Net` keys (see the
/// module docs). Idempotent and surgical: sets only the three keys, preserves
/// everything else, and skips the write entirely when the file is already
/// correct. A missing file is created with just the `Net:` block (RPCS3 fills
/// the rest of its defaults on next launch). Best-effort by contract — the
/// caller treats any error as non-fatal (worst case the game keeps the prior
/// config, exactly as before this fix), so failures are returned, not panicked.
pub fn ensure_rpcs3_net_config(config_dir: &Path) -> Result<()> {
    let path = config_dir.join("config.yml");
    let existing = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!(
                path = %path.display(),
                "rpcs3 config.yml absent — creating it with the Net defaults"
            );
            String::new()
        }
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
    };

    let merged = merge_net_config(&existing);
    if merged == existing {
        tracing::debug!(path = %path.display(), "rpcs3 Net config already correct — no change");
        return Ok(());
    }

    // A fresh install may not have created `config/` yet.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create config dir {}", parent.display()))?;
    }
    std::fs::write(&path, &merged).with_context(|| format!("write {}", path.display()))?;
    tracing::info!(
        path = %path.display(),
        "applied RPCS3 Net config (Internet Connected + PSN Disconnected + DNS 8.8.8.8)"
    );
    Ok(())
}

/// Pure core of [`ensure_rpcs3_net_config`]: given the current `config.yml`
/// text, return the text with the three [`NET_KEYS`] set, every other key
/// preserved. Splitting the IO off keeps the merge logic unit-testable without
/// touching the filesystem.
///
/// Algorithm (line-based, indentation-aware):
///  * Find the top-level `Net:` line (indent 0). If absent, append a fresh
///    `Net:` block with all three keys.
///  * Otherwise the block runs until the next top-level key (the next non-blank
///    line whose first char isn't whitespace). Within it, replace any line whose
///    key matches one of ours; insert the ones that weren't present at the top of
///    the block. Sibling keys (DNS, IP address, …) are left untouched.
fn merge_net_config(existing: &str) -> String {
    let newline = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let had_trailing_newline = existing.is_empty() || existing.ends_with('\n');

    // Logical lines without their terminators (tolerate CRLF and LF).
    let mut lines: Vec<String> = if existing.is_empty() {
        Vec::new()
    } else {
        existing
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
            .collect()
    };
    // `split('\n')` on a trailing-newline file leaves a final "" element; drop
    // it (we re-add the trailing newline when rebuilding).
    if existing.ends_with('\n') {
        lines.pop();
    }

    match lines.iter().position(|l| l.trim_end() == "Net:") {
        None => {
            lines.push("Net:".to_string());
            for (k, v) in NET_KEYS {
                lines.push(format!("  {k}: {v}"));
            }
        }
        Some(start) => {
            // Block body = subsequent blank-or-indented lines, ending at the
            // next top-level (non-indented, non-blank) key.
            let mut end = start + 1;
            while end < lines.len()
                && (lines[end].is_empty() || lines[end].starts_with([' ', '\t']))
            {
                end += 1;
            }

            // Replace in place every target key already present; whatever's left
            // gets inserted at the top of the block.
            let mut remaining: Vec<(&str, &str)> = NET_KEYS.to_vec();
            for line in lines.iter_mut().take(end).skip(start + 1) {
                if let Some(key) = line_key(line)
                    && let Some(pos) = remaining.iter().position(|(k, _)| *k == key)
                {
                    let (k, v) = remaining.remove(pos);
                    *line = format!("  {k}: {v}");
                }
            }
            for (offset, (k, v)) in remaining.iter().enumerate() {
                lines.insert(start + 1 + offset, format!("  {k}: {v}"));
            }
        }
    }

    let mut out = lines.join(newline);
    if had_trailing_newline {
        out.push_str(newline);
    }
    out
}

/// The key portion of a `  Key: value` line (leading indent stripped, trailing
/// spaces trimmed), or `None` for a blank line or one with no `:`. We compare
/// the full key so `PSN status` never collides with `PSN Country`.
fn line_key(line: &str) -> Option<&str> {
    let (k, _) = line.trim_start().split_once(':')?;
    Some(k.trim_end())
}

// ---------------------------------------------------------------------------
// PLAN 15.12 — transient save-state config for the play-through recorder's
// in-game tier. Resuming a save state needs the ASMJIT SPU recompiler it was
// taken under + RPCS3's compatible-savestate mode — settings the user does NOT
// run generally — so the recorder swaps them in only around a recording run and
// restores afterwards. Target RPCS3's REAL global config: that's
// `<RPCS3_CONFIG_DIR>/config/config.yml` (the full 8 KB file), NOT the bare
// `<dir>/config.yml` the Net writer touches.
// ---------------------------------------------------------------------------

/// The two settings a save state needs to RESUME (not just capture).
const SAVESTATE_KEYS: &[(&str, &str)] = &[
    ("SPU Decoder", "Recompiler (ASMJIT)"),
    ("Compatible Savestate Mode", "true"),
];

/// Pure core of [`apply_savestate_config`]: set the [`SAVESTATE_KEYS`] in place
/// wherever they already appear (a full `config.yml` always has them),
/// preserving indentation, every other line, line endings, and the trailing
/// newline. Unlike the `Net` merge this never inserts — the keys live in
/// different blocks (Core / Savestate) and are always present, so an absent key
/// is simply left alone.
fn set_savestate_keys(existing: &str) -> String {
    let newline = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let had_trailing_newline = existing.is_empty() || existing.ends_with('\n');
    let mut lines: Vec<String> = if existing.is_empty() {
        Vec::new()
    } else {
        existing
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
            .collect()
    };
    if existing.ends_with('\n') {
        lines.pop();
    }
    for line in lines.iter_mut() {
        if let Some(key) = line_key(line)
            && let Some((k, v)) = SAVESTATE_KEYS.iter().find(|(kk, _)| *kk == key)
        {
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            *line = format!("{indent}{k}: {v}");
        }
    }
    let mut out = lines.join(newline);
    if had_trailing_newline {
        out.push_str(newline);
    }
    out
}

/// RAII guard from [`apply_savestate_config`]: restores the original config on
/// drop (and removes the backup). On a restore failure the backup is kept for
/// manual recovery.
pub struct SavestateConfigGuard {
    path: PathBuf,
    backup: PathBuf,
}

impl Drop for SavestateConfigGuard {
    fn drop(&mut self) {
        if !self.backup.exists() {
            return;
        }
        match std::fs::copy(&self.backup, &self.path) {
            Ok(_) => {
                let _ = std::fs::remove_file(&self.backup);
                tracing::info!(path = %self.path.display(), "restored RPCS3 config after recording");
            }
            Err(e) => tracing::warn!(
                error = %e,
                backup = %self.backup.display(),
                "failed to restore RPCS3 config — backup kept for manual recovery"
            ),
        }
    }
}

/// Transiently apply the save-state settings to `config_yml` for the life of the
/// returned guard (restored on drop). Backs up to `<name>.recorder-bak` and
/// self-heals a prior crashed run (a leftover backup means the live file is the
/// modified one → restore it first, then re-apply). Pass RPCS3's real global
/// config — `<RPCS3_CONFIG_DIR>/config/config.yml`.
pub fn apply_savestate_config(config_yml: &Path) -> Result<SavestateConfigGuard> {
    let name = config_yml
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config.yml");
    let backup = config_yml.with_file_name(format!("{name}.recorder-bak"));

    if backup.exists() {
        std::fs::copy(&backup, config_yml)
            .with_context(|| format!("restore stale backup {}", backup.display()))?;
    }
    let original = std::fs::read_to_string(config_yml)
        .with_context(|| format!("read {}", config_yml.display()))?;
    std::fs::write(&backup, &original)
        .with_context(|| format!("write backup {}", backup.display()))?;
    let swapped = set_savestate_keys(&original);
    std::fs::write(config_yml, &swapped)
        .with_context(|| format!("write save-state config {}", config_yml.display()))?;
    tracing::info!(
        path = %config_yml.display(),
        "applied transient save-state config (SPU=ASMJIT, Compatible Savestate Mode=true)"
    );
    Ok(SavestateConfigGuard {
        path: config_yml.to_path_buf(),
        backup,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net_block(yaml: &str) -> Vec<String> {
        // Extract the `Net:` block's child lines for focused assertions.
        let mut out = Vec::new();
        let mut in_net = false;
        for line in yaml.lines() {
            if line.trim_end() == "Net:" {
                in_net = true;
                continue;
            }
            if in_net {
                if line.is_empty() || line.starts_with([' ', '\t']) {
                    out.push(line.to_string());
                } else {
                    break;
                }
            }
        }
        out
    }

    #[test]
    fn creates_net_block_when_file_is_empty() {
        let out = merge_net_config("");
        assert_eq!(
            out,
            "Net:\n  DNS address: 8.8.8.8\n  Internet enabled: Connected\n  PSN status: Disconnected\n"
        );
    }

    #[test]
    fn appends_net_block_when_absent_preserving_other_sections() {
        let input = "Audio:\n  Master Volume: 100\nVideo:\n  Renderer: Vulkan\n";
        let out = merge_net_config(input);
        // The pre-existing sections are untouched...
        assert!(out.starts_with(input));
        // ...and a Net block is appended with all three keys.
        assert_eq!(
            net_block(&out),
            vec![
                "  DNS address: 8.8.8.8",
                "  Internet enabled: Connected",
                "  PSN status: Disconnected",
            ]
        );
    }

    #[test]
    fn rewrites_only_the_three_keys_preserving_siblings() {
        let input = "\
Net:
  Bind address: 0.0.0.0
  DNS address: 192.168.1.1
  Internet enabled: Disconnected
  IP address: 0.0.0.0
  PSN status: Simulated
  UPNP Enabled: false
System:
  Language: English (US)
";
        let out = merge_net_config(input);
        // Our three keys flipped to the working values.
        assert!(out.contains("  DNS address: 8.8.8.8"));
        assert!(out.contains("  Internet enabled: Connected"));
        assert!(out.contains("  PSN status: Disconnected"));
        // Every sibling key (and the next section) preserved verbatim.
        assert!(out.contains("  Bind address: 0.0.0.0"));
        assert!(out.contains("  IP address: 0.0.0.0"));
        assert!(out.contains("  UPNP Enabled: false"));
        assert!(out.contains("System:\n  Language: English (US)"));
        // No duplicate keys were inserted (replace-in-place, not append).
        assert_eq!(out.matches("Internet enabled:").count(), 1);
        assert_eq!(out.matches("PSN status:").count(), 1);
        assert_eq!(out.matches("DNS address:").count(), 1);
    }

    #[test]
    fn inserts_missing_keys_into_existing_block() {
        // A Net block that has only DNS — the other two must be added, not the
        // whole block re-appended.
        let input = "Net:\n  DNS address: 8.8.8.8\n";
        let out = merge_net_config(input);
        assert_eq!(out.matches("Net:").count(), 1);
        assert!(out.contains("  Internet enabled: Connected"));
        assert!(out.contains("  PSN status: Disconnected"));
    }

    #[test]
    fn is_idempotent() {
        let input = "Audio:\n  Master Volume: 100\nNet:\n  Internet enabled: Disconnected\n";
        let once = merge_net_config(input);
        let twice = merge_net_config(&once);
        assert_eq!(once, twice, "second pass must be a no-op");
        // And an already-correct file is returned byte-for-byte unchanged.
        assert_eq!(merge_net_config(&once), once);
    }

    #[test]
    fn does_not_collide_psn_status_with_psn_country() {
        let input = "Net:\n  PSN Country: us\n  PSN status: RPCN\n";
        let out = merge_net_config(input);
        // PSN Country untouched; only PSN status rewritten.
        assert!(out.contains("  PSN Country: us"));
        assert!(out.contains("  PSN status: Disconnected"));
        assert!(!out.contains("PSN status: RPCN"));
    }

    #[test]
    fn preserves_crlf_line_endings() {
        let input = "Net:\r\n  Internet enabled: Disconnected\r\n";
        let out = merge_net_config(input);
        assert!(out.contains("\r\n"), "CRLF style should be preserved");
        assert!(out.contains("  Internet enabled: Connected\r\n"));
    }

    #[test]
    fn savestate_swap_sets_both_keys_in_place() {
        let input = "\
Core:
  PPU Decoder: Recompiler (LLVM)
  SPU Decoder: Recompiler (LLVM)
Savestate:
  Compatible Savestate Mode: false
  Maximum SaveState Files: 4
";
        let out = set_savestate_keys(input);
        assert!(out.contains("  SPU Decoder: Recompiler (ASMJIT)"));
        assert!(out.contains("  Compatible Savestate Mode: true"));
        // PPU + the savestate sibling are untouched; no key duplicated.
        assert!(out.contains("  PPU Decoder: Recompiler (LLVM)"));
        assert!(out.contains("  Maximum SaveState Files: 4"));
        assert_eq!(out.matches("SPU Decoder:").count(), 1);
        assert_eq!(out.matches("Compatible Savestate Mode:").count(), 1);
    }

    #[test]
    fn savestate_swap_is_idempotent_and_keeps_crlf() {
        let input = "  SPU Decoder: Recompiler (LLVM)\r\n  Compatible Savestate Mode: false\r\n";
        let once = set_savestate_keys(input);
        assert!(once.contains("  SPU Decoder: Recompiler (ASMJIT)\r\n"));
        assert!(once.ends_with("\r\n"));
        assert_eq!(
            set_savestate_keys(&once),
            once,
            "second pass must be a no-op"
        );
    }

    #[test]
    fn savestate_swap_leaves_absent_keys_alone() {
        // No SPU Decoder line present → nothing inserted, file otherwise intact.
        let input = "Savestate:\n  Compatible Savestate Mode: false\n";
        let out = set_savestate_keys(input);
        assert!(!out.contains("SPU Decoder"));
        assert!(out.contains("  Compatible Savestate Mode: true"));
    }
}
