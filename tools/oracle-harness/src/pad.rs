//! O4's frame-locked input as run steps (openspec AF.7, design decision 3): an
//! input script ([`guac_oracle::input`]) turned into `PAD` lines against a base
//! flip and loaded at a pause, and `PAD_STATE` / `PAD_CLEAR` as calls.
//!
//! A schedule goes in only at a pause, because O4 refuses an entry at or behind
//! the flip counter (the current flip's reads may already have happened), and
//! a script's offset 0 is the first flip after that pause.

use anyhow::{Context, Result, bail};
use guac_oracle::input::InputScript;
use skylander_rpcs3_control::ipc::proto::EmuState;

use crate::control::Control;
use crate::verbs::{PadRead, Refusal, Reply, ReplyError, Verb};

/// The `PAD` verbs that load `script` with its offset 0 at flip `base`: port
/// 0's entries in offset order, then port 1's.
pub fn script_verbs(script: &InputScript, base: u64) -> Result<Vec<Verb>> {
    let mut verbs = Vec::new();
    for (port, entries) in script.entries().iter().enumerate() {
        for entry in entries {
            let flip = base
                .checked_add(entry.at)
                .with_context(|| format!("offset {} past flip {base} overflows", entry.at))?;
            verbs.push(Verb::Pad {
                port: u8::try_from(port).expect("two ports"),
                flip,
                pad: entry.pad.ds3(),
            });
        }
    }
    Ok(verbs)
}

/// Where a loaded script sits against the run: its base flip and the pause it
/// went in at.
#[derive(Debug, Clone)]
pub struct Loaded {
    /// The flip its offset 0 landed on.
    pub base: u64,
    /// The pause it was loaded at, unchanged after the last `PAD`.
    pub paused_at: EmuState,
    /// `PAD` lines sent.
    pub entries: usize,
}

impl Control {
    /// `PAD_CLEAR`, then `script` with offset 0 on the first flip after the
    /// pause the run is in. Refused unless `STATE` reads paused.
    pub fn load_script(&self, script: &InputScript) -> Result<Loaded> {
        let state = self.state()?;
        self.load_script_at(script, state.frames + 1)
    }

    /// [`Control::load_script`] with offset 0 at `base`, which has to be ahead
    /// of the paused counter. The run has to hold still throughout: a `STATE`
    /// that moves between the first line and the last fails the load, since an
    /// entry could then have raced the guest's reads. A load that fails once
    /// lines have gone out sends `PAD_CLEAR` again, so a refusal part way (or a
    /// run that moved) leaves the host's input, not part of the script; the
    /// error says so when that clear fails too.
    pub fn load_script_at(&self, script: &InputScript, base: u64) -> Result<Loaded> {
        self.send_script(script, base, true)
    }

    /// [`Control::load_script_at`] without its `PAD_CLEAR`: the script's lines
    /// go in beside what's scheduled, so the flip the run is paused at keeps
    /// the state the schedule already gives it. That flip's reads come after
    /// the resume (AH.7's first drive: a clear at each pause handed every
    /// paused flip to the host, and pausing every two flips let go of the
    /// stick every other frame), so a run that pauses as it goes appends. A
    /// failure part way clears both schedules, as a load's does.
    pub fn append_script_at(&self, script: &InputScript, base: u64) -> Result<Loaded> {
        self.send_script(script, base, false)
    }

    fn send_script(&self, script: &InputScript, base: u64, clear: bool) -> Result<Loaded> {
        let before = self.state()?;
        if before.status != "paused" {
            bail!(
                "load a script at a pause: STATE reads {} frames={}",
                before.status,
                before.frames
            );
        }
        if base <= before.frames {
            bail!(
                "a script's base flip {base} isn't ahead of the pause at {}",
                before.frames
            );
        }
        let mut verbs = if clear {
            vec![Verb::PadClear]
        } else {
            Vec::new()
        };
        verbs.extend(script_verbs(script, base)?);
        let sent = (|| {
            for (verb, reply) in verbs.iter().zip(self.verbs().send_all(&verbs)?) {
                reply
                    .with_context(|| format!("loading the script: {}", verb.encode().trim_end()))?;
            }
            let after = self.state()?;
            if after != before {
                bail!("the run moved while the script loaded: {before:?} -> {after:?}");
            }
            Ok(())
        })();
        if let Err(err) = sent {
            return Err(match self.verbs().send(Verb::PadClear) {
                Ok(_) => err,
                Err(clear) => err.context(format!(
                    "PAD_CLEAR after the failed load failed too ({clear:#}): part of the \
                     script may still be loaded"
                )),
            });
        }
        Ok(Loaded {
            base,
            paused_at: before,
            entries: verbs.len() - usize::from(clear),
        })
    }

    /// `PAD_CLEAR`: both ports read the host's input again.
    pub fn pad_clear(&self) -> Result<()> {
        self.verbs().send(Verb::PadClear)?;
        Ok(())
    }

    /// `PAD_STATE`: the guest's last read on `port`, or the first read of
    /// `flip`; `None` when there's no such read (O4's `ERR no_read`).
    pub fn pad_state(&self, port: u8, flip: Option<u64>) -> Result<Option<PadRead>> {
        match self.verbs().send(Verb::PadState { port, flip }) {
            Ok(Reply::PadRead(read)) => Ok(Some(read)),
            Ok(other) => bail!("PAD_STATE answered {other:?}"),
            Err(err) => match err
                .downcast_ref::<ReplyError>()
                .and_then(ReplyError::refusal)
            {
                Some(Refusal::NoRead) => Ok(None),
                _ => Err(err),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_compiles_to_the_exact_pad_lines() {
        let script = InputScript::from_ron(
            r#"(
                steps: [
                    (at: 30, pads: {1: (buttons: [South, Start], left: (x: 0, y: 255))}),
                    (at: 0, pads: {0: (left: (x: 128, y: 0)), 1: ()}),
                    (at: 4, pads: {0: (buttons: [West, DPadLeft, RightTrigger2], right: (x: 7, y: 200))}),
                    (at: 34, pads: {1: ()}),
                ],
            )"#,
        )
        .unwrap();
        let lines: Vec<String> = script_verbs(&script, 1_000)
            .unwrap()
            .iter()
            .map(|v| v.encode())
            .collect();
        assert_eq!(
            lines,
            [
                "PAD 0 1000 00 00 128 0 128 128\n",
                "PAD 0 1004 80 82 128 128 7 200\n",
                "PAD 1 1000 00 00 128 128 128 128\n",
                "PAD 1 1030 08 40 0 255 128 128\n",
                "PAD 1 1034 00 00 128 128 128 128\n",
            ]
        );
        // Offsets past the counter's range are an error, not a wrap.
        let far =
            InputScript::from_ron("(steps: [(at: 18446744073709551615, pads: {0: ()})])").unwrap();
        assert!(script_verbs(&far, 2).is_err());
        assert_eq!(script_verbs(&far, 0).unwrap().len(), 1);
    }
}
