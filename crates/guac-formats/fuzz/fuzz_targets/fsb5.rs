#![no_main]

use guac_formats::fsb::{Bank, Codec};
use guac_formats::mpeg;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(bank) = Bank::parse(data) else {
        return;
    };
    assert!(bank.len() <= data.len());
    for s in &bank.samples {
        assert!(s.data_at + s.data.len() <= bank.len());
        if bank.codec != Codec::Mpeg {
            continue;
        }
        // A remux only drops bytes, and its frames account for its samples.
        if let Ok(r) = mpeg::remux(s.data, s.samples) {
            assert_eq!(r.bytes.len() + r.padding + r.cut, s.data.len());
            assert!(r.frames > 0);
            assert_eq!(r.samples % u64::from(r.first.samples()), 0);
            assert!(r.samples < u64::from(s.samples) + u64::from(r.first.samples()));
        }
    }
});
