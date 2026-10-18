//! Every parser in the registry under one target, picked by the first byte, so
//! a newly registered parser is fuzzed before it gets a target of its own.

#![no_main]

use guac_formats::registry::PARSERS;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&pick, input)) = data.split_first() else {
        return;
    };
    if PARSERS.is_empty() {
        return;
    }
    let _ = (PARSERS[usize::from(pick) % PARSERS.len()].parse)(input);
});
