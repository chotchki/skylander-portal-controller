#![no_main]

use guac_formats::exports::Exports;
use guac_formats::igz::Igz;
use guac_formats::registry::{unbundle, walk_toy_table};
use guac_formats::roster::figures;
use guac_formats::toys::ToyTable;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = walk_toy_table(data);
    let parts = unbundle(data);
    let [init, global, boot, collection, ..] = parts[..] else {
        return;
    };
    let (Ok(init), Ok(global), Ok(boot), Ok(collection)) = (
        Igz::parse(init),
        Igz::parse(global),
        Igz::parse(boot),
        Igz::parse(collection),
    ) else {
        return;
    };
    let Ok(exports) = Exports::new(&boot, &global) else {
        return;
    };
    let Ok(table) = ToyTable::read(&init, &global, &exports) else {
        return;
    };
    // A table that reads hangs together: every index it hands out is held.
    for alt in &table.alts {
        assert!(alt.toy < table.toys.len());
    }
    for a in &table.alt_characters {
        assert!(a.normal < table.toys.len() && a.alt < table.toys.len());
    }
    if let Ok(figures) = figures(&table, &global, &collection) {
        for f in &figures {
            assert!(f.portrait.is_none_or(|p| p < collection.objects().len()));
            assert!(f.element.is_none_or(|e| e < 8));
        }
    }
});
