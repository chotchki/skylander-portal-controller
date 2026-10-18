//! The files `oracle home init` writes into the oracle's RPCS3 config dir
//! (openspec AF, design decision 5). Each is partial on purpose: RPCS3 fills
//! every key a file leaves out with the pin's default, so what's here is exactly
//! what the oracle changes, and the pin (in the identity line) fixes the rest.

/// The shader mode that dodges the unfixed `accfecd2d` boot crash (the shader
/// interpreter's warm-up, which only the two interpreter modes run). It skips a
/// draw whose shader is still compiling.
pub const ASYNC_RECOMPILER: &str = "Async Recompiler (multi-threaded)";
/// RPCS3's default from `accfecd2d` on and the oracle's (AF.5): a draw whose
/// shader is still compiling goes through the interpreter instead of being
/// skipped, so what a frame holds doesn't hang on compile timing. O1 and O2 fix
/// its boot crash.
pub const ASYNC_WITH_INTERPRETER: &str = "Async Recompiler with Shader Interpreter";

/// The oracle's GDB stub port, off RPCS3's default 2345: a connection to the
/// stub pauses the emulator (and freezes it if the peer hangs up silently), so
/// a debugger or port probe aimed at the dev's own RPCS3 must not reach an
/// oracle run.
pub const ORACLE_GDB_PORT: u16 = 2346;

/// The oracle's `config.yml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigTemplate {
    /// `Video: Shader Mode`, verbatim as RPCS3 spells it.
    pub shader_mode: String,
    /// `Log: sys_fs: Trace`, for a run that wants every read and seek inside an
    /// archive; off, `sys_fs` keeps its default level (opens and stats).
    pub sys_fs_trace: bool,
}

impl Default for ConfigTemplate {
    fn default() -> Self {
        Self {
            shader_mode: ASYNC_WITH_INTERPRETER.to_owned(),
            sys_fs_trace: false,
        }
    }
}

impl ConfigTemplate {
    /// The YAML, sections in RPCS3's own (alphabetical) order.
    pub fn render(&self) -> String {
        let mut out = String::from(
            "# Written by `oracle home init` (openspec AF, design decision 5). Keys left\n\
             # out keep the pin's defaults.\n",
        );
        if self.sys_fs_trace {
            out.push_str("Log:\n  sys_fs: Trace\n");
        }
        out.push_str(&format!(
            "Miscellaneous:\n  GDB Server: 127.0.0.1:{ORACLE_GDB_PORT}\n"
        ));
        // Disconnected plus P3's offline path: a resumed state's dead sockets fail
        // fast instead of busy-retrying (the product recorder's resume setting).
        out.push_str("Net:\n  Internet enabled: Disconnected\n");
        // Start Paused stops a savestate boot after 2 flips (a replay's F0); suspend
        // mode would exit after a save and rename a loaded state.
        out.push_str(
            "Savestate:\n  Compatible Savestate Mode: true\n  Start Paused: true\n  Suspend Emulation Savestate Mode: false\n",
        );
        // FIFO reordering off keeps RSX's command order the guest's (AC.12).
        // Overlays (the pause message, trophy popups) stay out of a SCREENSHOT,
        // which is then the guest's display buffer alone.
        out.push_str(&format!(
            "Video:\n  Disable FIFO Reordering: true\n  Record With Overlays: false\n  Shader Mode: {}\n",
            self.shader_mode
        ));
        out
    }
}

/// `ipc.yml`: PINE on, on its default slot. RPCS3 reads it from the config dir,
/// which `--config` doesn't cover; the socket lands at `$TMPDIR/rpcs3.sock`.
pub fn ipc_yml() -> &'static str {
    "IPC Server enabled: true\nIPC Port: 28012\n"
}

/// `input_configs/global/Default.yml`: ports 0 and 1 on the keyboard handler,
/// the one handler RPCS3 reports connected with no device behind it. The oracle
/// drives both through P6 and the O-series; no key reaches a borderless run.
pub fn input_config() -> &'static str {
    "Player 1 Input:\n  Handler: Keyboard\n  Device: Keyboard\n\
     Player 2 Input:\n  Handler: Keyboard\n  Device: Keyboard\n"
}

/// `GuiConfigs/CurrentSettings.ini`: without it a fresh config dir opens the
/// first-run welcome dialog, which blocks a no-GUI boot forever (AF.2 hit it).
pub fn gui_settings() -> &'static str {
    "[main_window]\ninfoBoxEnabledWelcome=false\n"
}

/// `games.yml`: Giants alone, at the dev's dump. Quoted, since a dump path may
/// hold `: `.
pub fn games_yml(giants_dump: &std::path::Path) -> String {
    let mut dir = giants_dump.display().to_string();
    if !dir.ends_with('/') {
        dir.push('/');
    }
    format!(
        "BLUS30968: \"{}\"\n",
        dir.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `section -> key -> value` for the two-level YAML RPCS3's configs use.
    fn parse(yaml: &str) -> Vec<(String, String, String)> {
        let mut out = Vec::new();
        let mut section = String::new();
        for line in yaml.lines().filter(|l| !l.trim_start().starts_with('#')) {
            if let Some(name) = line.strip_suffix(':').filter(|_| !line.starts_with(' ')) {
                section = name.to_owned();
            } else if let Some(body) = line.strip_prefix("  ") {
                let (k, v) = body.split_once(": ").expect("key: value");
                out.push((section.clone(), k.to_owned(), v.to_owned()));
            } else {
                panic!("unexpected line {line:?}");
            }
        }
        out
    }

    fn value<'a>(entries: &'a [(String, String, String)], section: &str, key: &str) -> &'a str {
        let hits: Vec<_> = entries
            .iter()
            .filter(|(s, k, _)| s == section && k == key)
            .collect();
        assert_eq!(hits.len(), 1, "{section}: {key} in {entries:?}");
        &hits[0].2
    }

    #[test]
    fn the_template_carries_the_oracle_keys() {
        let entries = parse(&ConfigTemplate::default().render());
        assert_eq!(value(&entries, "Savestate", "Start Paused"), "true");
        assert_eq!(
            value(&entries, "Savestate", "Suspend Emulation Savestate Mode"),
            "false"
        );
        assert_eq!(
            value(&entries, "Savestate", "Compatible Savestate Mode"),
            "true"
        );
        assert_eq!(value(&entries, "Net", "Internet enabled"), "Disconnected");
        assert_eq!(value(&entries, "Video", "Disable FIFO Reordering"), "true");
        assert_eq!(value(&entries, "Video", "Record With Overlays"), "false");
        assert_eq!(
            value(&entries, "Miscellaneous", "GDB Server"),
            "127.0.0.1:2346"
        );
        assert_eq!(
            value(&entries, "Video", "Shader Mode"),
            ASYNC_WITH_INTERPRETER
        );
        // sys_fs stays at its default unless a run asks for trace.
        assert!(entries.iter().all(|(s, _, _)| s != "Log"));
        assert_eq!(entries.len(), 8);
    }

    #[test]
    fn trace_and_shader_mode_are_the_template_parameters() {
        let template = ConfigTemplate {
            shader_mode: ASYNC_RECOMPILER.to_owned(),
            sys_fs_trace: true,
        };
        let entries = parse(&template.render());
        assert_eq!(value(&entries, "Log", "sys_fs"), "Trace");
        assert_eq!(value(&entries, "Video", "Shader Mode"), ASYNC_RECOMPILER);
        assert_eq!(value(&entries, "Savestate", "Start Paused"), "true");
    }

    #[test]
    fn the_small_files_parse_as_rpcs3_writes_them() {
        let ipc = parse(&format!("IPC:\n{}", indent(ipc_yml())));
        assert_eq!(value(&ipc, "IPC", "IPC Server enabled"), "true");
        let pads = parse(input_config());
        for player in ["Player 1 Input", "Player 2 Input"] {
            assert_eq!(value(&pads, player, "Handler"), "Keyboard");
            assert_eq!(value(&pads, player, "Device"), "Keyboard");
        }
        assert_eq!(
            games_yml(std::path::Path::new("/d/Sky: \"Giants\"")),
            "BLUS30968: \"/d/Sky: \\\"Giants\\\"/\"\n"
        );
    }

    fn indent(text: &str) -> String {
        text.lines().map(|l| format!("  {l}\n")).collect()
    }
}
