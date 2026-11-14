//! The material's inputs switched off one at a time (openspec AS.300), so a
//! look on the TV can be traced to the input that makes it:
//! `GUAC_TEST_SHADE_OFF`, read in `test-hooks` builds only, a comma-separated
//! list of [`SWITCHES`]' names. Each adds a shader def to every pipeline the
//! character material specializes (the level's, the props', the heroes'), so
//! it reaches every part at once; a kid's build reads nothing and adds none.
//! `GUAC_TEST_SHADOW_BIAS=<depth>,<normal>` (the same builds) sets the level's
//! shadow light's two biases in place of Bevy's defaults.

use std::sync::OnceLock;

/// The process env key [`asked`] reads.
pub const KEY: &str = "GUAC_TEST_SHADE_OFF";
/// The process env key [`shadow_bias`] reads.
pub const BIAS_KEY: &str = "GUAC_TEST_SHADOW_BIAS";

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

/// `<depth>,<normal>` as two finite biases.
pub fn parse_bias(value: &str) -> Result<(f32, f32), String> {
    let mut parts = value.split(',').map(|s| s.trim().parse::<f32>());
    match (parts.next(), parts.next(), parts.next()) {
        (Some(Ok(d)), Some(Ok(n)), None) if d.is_finite() && n.is_finite() => Ok((d, n)),
        _ => Err(format!("{BIAS_KEY}: want <depth>,<normal>, got {value:?}")),
    }
}

/// The defs this process's [`KEY`] asks for, read once; none outside a
/// `test-hooks` build, or when the value doesn't read (logged).
pub fn asked() -> &'static [&'static str] {
    static ASKED: OnceLock<Vec<&'static str>> = OnceLock::new();
    ASKED.get_or_init(|| {
        if !cfg!(feature = "test-hooks") {
            return Vec::new();
        }
        let Ok(value) = std::env::var(KEY) else {
            return Vec::new();
        };
        match defs(&value) {
            Ok(d) => {
                tracing::warn!(?d, "{KEY}: material inputs switched off");
                d
            }
            Err(e) => {
                tracing::error!("{e}");
                Vec::new()
            }
        }
    })
}

/// The level's shadow light's biases this process's [`BIAS_KEY`] asks for;
/// `None` outside a `test-hooks` build or when unset or unreadable (logged).
pub fn shadow_bias() -> Option<(f32, f32)> {
    if !cfg!(feature = "test-hooks") {
        return None;
    }
    let value = std::env::var(BIAS_KEY).ok()?;
    parse_bias(&value)
        .inspect_err(|e| tracing::error!("{e}"))
        .ok()
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

    #[test]
    fn a_bias_is_two_finite_numbers() {
        assert_eq!(parse_bias("0.05, 0.6"), Ok((0.05, 0.6)));
        assert!(parse_bias("0.05").is_err());
        assert!(parse_bias("0.05,0.6,1").is_err());
        assert!(parse_bias("nan,1").is_err());
    }

    /// A build without `test-hooks` (the unit tests' own) reads neither key.
    #[test]
    #[cfg(not(feature = "test-hooks"))]
    fn a_build_without_test_hooks_reads_neither_key() {
        assert!(asked().is_empty());
        assert_eq!(shadow_bias(), None);
    }
}
