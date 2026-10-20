//! The whitelist (design decision 2): the placements whose scripts run.
//! Everything off it stays as AG left it, and what a listed script wakes off
//! it stays inert (decision 6).

/// Which placements run, by placement name or by the script they run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Whitelist {
    /// Every placement (tests and the census runs).
    All,
    /// Placements whose name is listed exactly, or whose script's source
    /// path ends with a listed script (`009_Eruptor.ai`).
    Only {
        placements: Vec<String>,
        scripts: Vec<String>,
    },
}

impl Whitelist {
    pub fn only(placements: &[&str], scripts: &[&str]) -> Self {
        Whitelist::Only {
            placements: placements.iter().map(|s| (*s).to_owned()).collect(),
            scripts: scripts.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    pub fn lists(&self, placement: &str, script: &str) -> bool {
        match self {
            Whitelist::All => true,
            Whitelist::Only {
                placements,
                scripts,
            } => {
                placements.iter().any(|p| p == placement)
                    || scripts
                        .iter()
                        .any(|s| !s.is_empty() && script.ends_with(s.as_str()))
            }
        }
    }
}
