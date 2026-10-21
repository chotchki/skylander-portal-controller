//! The whitelist (design decision 2): the placements whose scripts run.
//! Everything off it stays as AG left it, and what a listed script wakes off
//! it stays inert (decision 6). Some run only their first tick: `global.bld`'s
//! running placements set the game up in theirs, and their per-tick tails
//! belong to the boxes that whitelist them (the portal manager's to the
//! player loop's).

/// What [`Whitelist::listing`] makes of a placement: named or its script
/// listed, a script listed for its first tick, or neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listed {
    /// Off the whitelist: inert, never ticked.
    Off,
    /// Ticked every logic tick.
    Every,
    /// Ticked until its STARTUP has run, then held: listed, never ticked
    /// again.
    FirstTick,
}

/// Which placements run, by placement name or by the script they run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Whitelist {
    /// Every placement (tests and the census runs).
    All,
    /// Placements whose name is listed exactly, or whose script's source
    /// path ends with a listed script (`009_Eruptor.ai`); `first_tick` lists
    /// scripts that run their first tick alone.
    Only {
        placements: Vec<String>,
        scripts: Vec<String>,
        first_tick: Vec<String>,
    },
}

impl Whitelist {
    pub fn only(placements: &[&str], scripts: &[&str]) -> Self {
        Self::with_first_ticks(placements, scripts, &[])
    }

    /// [`Whitelist::only`], plus scripts whose placements run their first
    /// tick and are then held.
    pub fn with_first_ticks(placements: &[&str], scripts: &[&str], first_tick: &[&str]) -> Self {
        let own = |l: &[&str]| l.iter().map(|s| (*s).to_owned()).collect();
        Whitelist::Only {
            placements: own(placements),
            scripts: own(scripts),
            first_tick: own(first_tick),
        }
    }

    pub fn lists(&self, placement: &str, script: &str) -> bool {
        self.listing(placement, script) != Listed::Off
    }

    pub fn listing(&self, placement: &str, script: &str) -> Listed {
        let ends = |l: &[String]| {
            l.iter()
                .any(|s| !s.is_empty() && script.ends_with(s.as_str()))
        };
        match self {
            Whitelist::All => Listed::Every,
            Whitelist::Only {
                placements,
                scripts,
                first_tick,
            } => {
                if placements.iter().any(|p| p == placement) || ends(scripts) {
                    Listed::Every
                } else if ends(first_tick) {
                    Listed::FirstTick
                } else {
                    Listed::Off
                }
            }
        }
    }
}
