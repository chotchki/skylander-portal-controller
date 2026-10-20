#![no_main]

use guac_formats::exports::Exports;
use guac_formats::igz::Igz;
use guac_formats::progression::{SaveKind, SaveTable};
use guac_formats::registry::{unbundle, walk_save_table};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = walk_save_table(data);
    let parts = unbundle(data);
    let [init, global, boot, ..] = parts[..] else {
        return;
    };
    let (Ok(init), Ok(global), Ok(boot)) = (Igz::parse(init), Igz::parse(global), Igz::parse(boot))
    else {
        return;
    };
    let Ok(exports) = Exports::new(&boot, &global) else {
        return;
    };
    let Ok(table) = SaveTable::read(&init, &global, &exports) else {
        return;
    };
    // A table that reads hangs together: level 1 starts at 0 and every level
    // after it costs more, and every row ran at least once, a spillover's
    // limit no more than the table's last entry.
    assert_eq!(table.level_xp.first(), Some(&0));
    assert!(table.level_xp.windows(2).all(|w| w[0] < w[1]));
    for p in &table.params {
        assert!(!p.fields.is_empty());
        if p.kind == SaveKind::Spillover {
            assert!(p.limit <= *table.level_xp.last().unwrap());
        }
    }
});
