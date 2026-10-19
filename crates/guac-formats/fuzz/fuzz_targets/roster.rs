#![no_main]

use guac_formats::exports::Exports;
use guac_formats::igz::Igz;
use guac_formats::language::{LanguagePack, Texts, join};
use guac_formats::registry::walk_roster;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = walk_roster(data);
    let Ok(file) = Igz::parse(data) else {
        return;
    };
    // A pack that joins pairs every string with a key, and the texts agree.
    if let Ok(pack) = LanguagePack::read(&file)
        && let Ok(joined) = join(&file, &pack)
    {
        assert_eq!(joined.len(), pack.strings.len());
        let lang = pack.lang_id;
        let pairs: Vec<(usize, String)> =
            joined.iter().map(|(k, t)| (*k, (*t).to_owned())).collect();
        let texts = Texts::new(&file, vec![pack]).expect("joined once, joins again");
        for (key, text) in pairs {
            assert_eq!(texts.text(key, lang), Some(text.as_str()));
        }
    }
    // An id that resolves lands on an object of its file.
    if let Ok(exports) = Exports::new(&file, &file) {
        for id in 0..exports.global_base() + 4 {
            if let Some(o) = exports.resolve(id).ok().and_then(|e| e.object()) {
                assert!(o < file.objects().len());
            }
        }
    }
});
