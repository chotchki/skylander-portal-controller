//! The material's inputs switched off one at a time (openspec AS.300), so a
//! look on the TV can be traced to the input that makes it:
//! `GUAC_TEST_SHADE_OFF`, read in `test-hooks` builds only, a comma-separated
//! list of [`SWITCHES`]' names. Each adds a shader def to every pipeline the
//! character material specializes (the level's, the props', the heroes'), so
//! it reaches every part at once; a kid's build reads nothing and adds none.

use std::sync::OnceLock;

/// The process env key [`asked`] reads.
pub const KEY: &str = "GUAC_TEST_SHADE_OFF";

/// Each switch's name and the shader def it sets.
pub const SWITCHES: [(&str, &str); 8] = [
    // The received shadow's term left out.
    ("shadows", "CHAR_OFF_SHADOWS"),
    // The first layer's colour map read as a flat light grey, its alpha kept.
    ("colour", "CHAR_OFF_COLOUR"),
    // Every spec map read as black: no sheen.
    ("spec", "CHAR_OFF_SPEC"),
    // The second layer left out: the first drawn as if alone.
    ("layer2", "CHAR_OFF_LAYER2"),
    // The third layer (an ambient-occlusion atlas on a level's) left out.
    ("layer3", "CHAR_OFF_LAYER3"),
    // The second layer's normal map read as the geometric normal.
    ("layer2_normal", "CHAR_OFF_LAYER2_NORMAL"),
    // Every normal map read as the geometric normal.
    ("normals", "CHAR_OFF_NORMALS"),
    // A normal map's z as sqrt(max(0, 1 - x^2 - y^2)), not of its abs.
    ("abs", "CHAR_OFF_ABS"),
];

/// The defs `value` asks for, in [`SWITCHES`]' order; an unknown name is
/// an error naming it, an empty item is skipped.
pub fn defs(value: &str) -> Result<Vec<&'static str>, String> {
    let mut on = [false; SWITCHES.len()];
    for item in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let i = SWITCHES
            .iter()
            .position(|(name, _)| *name == item)
            .ok_or_else(|| format!("{KEY}: no switch {item:?}"))?;
        on[i] = true;
    }
    Ok(SWITCHES
        .iter()
        .zip(on)
        .filter_map(|((_, def), on)| on.then_some(*def))
        .collect())
}

/// What a build reads off [`KEY`]'s `value`: nothing unless `test_hooks`,
/// nothing (logged) when it doesn't read.
pub fn read(test_hooks: bool, value: Option<&str>) -> Vec<&'static str> {
    let (true, Some(value)) = (test_hooks, value) else {
        return Vec::new();
    };
    match defs(value) {
        Ok(d) => {
            tracing::warn!(?d, "{KEY}: material inputs switched off");
            d
        }
        Err(e) => {
            tracing::error!("{e}");
            Vec::new()
        }
    }
}

/// What this build reads off [`KEY`]'s `value`: [`read`] behind the
/// `test-hooks` feature, so a kid's build reads nothing.
pub fn this_build(value: Option<&str>) -> Vec<&'static str> {
    read(cfg!(feature = "test-hooks"), value)
}

/// The defs this process's [`KEY`] asks for, read once ([`this_build`]).
pub fn asked() -> &'static [&'static str] {
    static ASKED: OnceLock<Vec<&'static str>> = OnceLock::new();
    ASKED.get_or_init(|| this_build(std::env::var(KEY).ok().as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_name_sets_its_own_def_and_nothing_else() {
        assert_eq!(defs(""), Ok(vec![]));
        for (name, def) in SWITCHES {
            assert_eq!(defs(name), Ok(vec![def]), "{name}");
        }
        assert_eq!(
            defs(" abs , shadows,"),
            Ok(vec!["CHAR_OFF_SHADOWS", "CHAR_OFF_ABS"])
        );
        assert!(defs("shadow").unwrap_err().contains("\"shadow\""));
    }

    /// A kid's build must ignore the key even when it's set, and a typo must
    /// not leave a half-switched TV that reads as the switch's look.
    #[test]
    fn only_a_test_hooks_build_switches_anything_off() {
        assert_eq!(read(true, Some("layer3")), vec!["CHAR_OFF_LAYER3"]);
        assert!(read(false, Some("layer3")).is_empty());
        assert!(read(true, None).is_empty());
        assert!(read(true, Some("layer3,bogus")).is_empty());
    }

    /// The build's own gate, in whichever build runs the test: without the
    /// feature the key is ignored even when set.
    #[test]
    fn this_build_reads_the_key_only_with_test_hooks() {
        let got = this_build(Some("layer3"));
        if cfg!(feature = "test-hooks") {
            assert_eq!(got, vec!["CHAR_OFF_LAYER3"]);
        } else {
            assert!(got.is_empty(), "{got:?}");
        }
    }
}
