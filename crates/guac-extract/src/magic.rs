//! Telling a file's kind from its first bytes. One list serves both the
//! game-magic guard (`tests/no_game_bytes.rs`, design decision 10) and the
//! census (AB.5): [`GAME_MAGICS`] is what no file in the repo may start with;
//! [`ALLOWED`] and [`is_text`] name what the census accepts that the guard
//! can't flag without catching the repo's own files.

/// Prefixes no file in this repo may start with. Alchemy writes its magics in
/// the platform's byte order, so IGA and IGZ come both ways round.
pub const GAME_MAGICS: &[(&[u8], &str)] = &[
    (
        b"IGA\x1a",
        "IGA archive, little-endian (Giants and Trap Team .arc/.bld)",
    ),
    (
        b"\x1aAGI",
        "IGA archive, big-endian (Swap Force, SuperChargers and Imaginators .pak)",
    ),
    (b"IGZ", "IGZ object file"),
    (b"\x01ZGI", "IGZ object file, little-endian"),
    (b"BIKi", "Bink 1 video"),
    (b"KB2", "Bink 2 video"),
    (b"FSB5", "FMOD sound bank"),
    (
        &[0x57, 0xE0, 0xE0, 0x57, 0x10, 0xC0, 0xC0, 0x10],
        "Havok packfile",
    ),
    (b"BKHD", "Wwise sound bank (Spyro's Adventure .bnk)"),
    (b"AKPK", "Wwise file package (Spyro's Adventure .pck)"),
    (b"APKF", "texture package (Spyro's Adventure .apk)"),
    (b"strm", "zlib stream file (Spyro's Adventure .str)"),
    (b"\0ASL", "shader archive (Spyro's Adventure .lsa)"),
    (
        b"SCE\0",
        "SCE container (SELF/SPRX, an encrypted executable)",
    ),
    (b"\x7fPKG", "PS3 PKG (update or DLC package)"),
    (b"NPD\0", "NPDRM EDAT/SDAT"),
    (b"\0PSF", "PARAM.SFO"),
    (b"\xdc\xa2\x4d\x00", "PS3 trophy pack (TROPHY.TRP)"),
    (b"PS3LICDA", "PS3 disc licence (LIC.DAT)"),
    (b".SFB", "PS3 disc boot file (PS3_DISC.SFB)"),
    (b"SCEUF", "PS3 system update (PS3UPDAT.PUP)"),
    (b"RRC", "RPCS3 RSX capture"),
    // Not a disc format: the extract holds the disc's textures in it (AE.6),
    // and the repo holds none (test fixtures are built in code).
    (
        b"\xabKTX 20\xbb\r\n\x1a\n",
        "KTX2 texture (an extract's disc textures)",
    ),
    // A whole-memory dump plus the hdd0/hdd1 tars; RPCS3 writes it raw, gzipped
    // or zstd-compressed (`.SAVESTAT.zst`, its default), and the guard inflates both.
    (b"RPCS3SAV", "RPCS3 savestate"),
];

pub const PPC64_ELF: &str = "big-endian PPC64 ELF (a decrypted EBOOT)";
pub const EM_PPC64: u16 = 21;

pub fn is_ppc64_be_elf(head: &[u8]) -> bool {
    head.len() >= 20
        && head.starts_with(b"\x7fELF")
        && head[4] == 2 // ELFCLASS64
        && head[5] == 2 // ELFDATA2MSB
        && u16::from_be_bytes([head[18], head[19]]) == EM_PPC64
}

/// The game format `head` starts with, or `None`. What the guard flags.
pub fn classify(head: &[u8]) -> Option<&'static str> {
    if let Some((_, what)) = GAME_MAGICS
        .iter()
        .find(|(magic, _)| head.starts_with(magic))
    {
        return Some(what);
    }
    is_ppc64_be_elf(head).then_some(PPC64_ELF)
}

/// A census-accepted kind the guard leaves alone: generic formats the repo
/// holds too, and disc formats whose leading bytes are too weak to guard.
/// `ext`, when set, must match the file name's extension (ASCII case-blind).
pub struct Allowed {
    pub prefix: &'static [u8],
    pub ext: Option<&'static str>,
    pub what: &'static str,
}

pub const ALLOWED: &[Allowed] = &[
    Allowed {
        prefix: b"\x89PNG\r\n\x1a\n",
        ext: None,
        what: "PNG image (ICON0, PIC1, PS3LOGO.DAT)",
    },
    Allowed {
        prefix: b"RIFF",
        ext: Some("at3"),
        what: "RIFF ATRAC3 audio (SND0.AT3, the XMB's background track)",
    },
    Allowed {
        prefix: b"\0\0\0\x05",
        ext: Some("lxb"),
        what: "Spyro's Adventure .lxb data (leading u32 5, too weak to guard)",
    },
    Allowed {
        prefix: b"\0 ",
        ext: Some("fnt"),
        what: "Spyro's Adventure .fnt font (leading u16 0x20, too weak to guard)",
    },
];

pub const EMPTY: &str = "empty file";
pub const TEXT: &str = "text";
pub const MARKUP: &str = "text, XML or markup";

/// No control bytes but tab, LF, CR and form feed, and no DEL. Not checked as
/// UTF-8: Spyro's Adventure has a Lua file in a single-byte codepage. Packed
/// or encrypted bytes hit a control byte within a few hundred, so a 4 KiB head
/// tells them apart.
pub fn is_text(head: &[u8]) -> bool {
    let body = head.strip_prefix(b"\xef\xbb\xbf").unwrap_or(head);
    !body.is_empty()
        && !body
            .iter()
            .any(|&b| (b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\r' | 0x0c)) || b == 0x7f)
}

/// What a file or archive entry is, by its first bytes and its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sniff {
    /// A game format the guard flags.
    Game(&'static str),
    /// Accepted, but not guarded: see [`ALLOWED`].
    Allowed(&'static str),
    /// Neither. The census measures its entropy.
    Unknown,
}

impl Sniff {
    pub fn what(&self) -> &'static str {
        match self {
            Self::Game(w) | Self::Allowed(w) => w,
            Self::Unknown => "unknown",
        }
    }
}

pub fn sniff(head: &[u8], name: &str) -> Sniff {
    if let Some(what) = classify(head) {
        return Sniff::Game(what);
    }
    if head.is_empty() {
        return Sniff::Allowed(EMPTY);
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e);
    if let Some(a) = ALLOWED.iter().find(|a| {
        head.starts_with(a.prefix)
            && a.ext
                .is_none_or(|want| ext.is_some_and(|e| e.eq_ignore_ascii_case(want)))
    }) {
        return Sniff::Allowed(a.what);
    }
    if is_text(head) {
        let body = head.strip_prefix(b"\xef\xbb\xbf").unwrap_or(head);
        let first = body.iter().find(|b| !b.is_ascii_whitespace());
        return Sniff::Allowed(if first == Some(&b'<') { MARKUP } else { TEXT });
    }
    Sniff::Unknown
}

/// Shannon entropy in bits per byte, fed in pieces.
#[derive(Debug, Clone)]
pub struct Entropy {
    counts: [u64; 256],
    total: u64,
}

impl Default for Entropy {
    fn default() -> Self {
        Self {
            counts: [0; 256],
            total: 0,
        }
    }
}

impl Entropy {
    pub fn update(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.counts[b as usize] += 1;
        }
        self.total += bytes.len() as u64;
    }

    /// 0.0 for nothing or one repeated byte, 8.0 for uniform bytes.
    pub fn bits_per_byte(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        let n = self.total as f64;
        -self
            .counts
            .iter()
            .filter(|&&c| c > 0)
            .map(|&c| {
                let p = c as f64 / n;
                p * p.log2()
            })
            .sum::<f64>()
    }
}

pub fn entropy(bytes: &[u8]) -> f64 {
    let mut e = Entropy::default();
    e.update(bytes);
    e.bits_per_byte()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn starting_with(magic: &[u8]) -> Vec<u8> {
        let mut b = magic.to_vec();
        b.resize(64, 0);
        b
    }

    /// xorshift64*: deterministic bytes with no magic and near-8-bit entropy.
    fn noise(len: usize, mut seed: u64) -> Vec<u8> {
        (0..len)
            .map(|_| {
                seed ^= seed >> 12;
                seed ^= seed << 25;
                seed ^= seed >> 27;
                (seed.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
            })
            .collect()
    }

    #[test]
    fn every_game_magic_is_game() {
        for (magic, what) in GAME_MAGICS {
            assert_eq!(sniff(&starting_with(magic), "x.bin"), Sniff::Game(what));
        }
    }

    #[test]
    fn a_ppc64_elf_is_game_and_a_host_elf_isnt() {
        let mut elf = vec![0u8; 64];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2;
        elf[5] = 2;
        elf[18..20].copy_from_slice(&EM_PPC64.to_be_bytes());
        assert_eq!(sniff(&elf, "EBOOT.elf"), Sniff::Game(PPC64_ELF));
        elf[5] = 1;
        elf[18..20].copy_from_slice(&62u16.to_le_bytes());
        assert_eq!(sniff(&elf, "a.elf"), Sniff::Unknown);
    }

    #[test]
    fn allowed_prefixes_need_their_extension() {
        let lxb = starting_with(b"\0\0\0\x05");
        assert!(matches!(sniff(&lxb, "camera.LXB"), Sniff::Allowed(_)));
        assert_eq!(sniff(&lxb, "camera.bin"), Sniff::Unknown);
        assert_eq!(sniff(&lxb, "noext"), Sniff::Unknown);
        let png = starting_with(b"\x89PNG\r\n\x1a\n");
        assert!(matches!(sniff(&png, "PS3LOGO.DAT"), Sniff::Allowed(w) if w.starts_with("PNG")));
    }

    #[test]
    fn text_and_markup() {
        assert_eq!(
            sniff(b"\xef\xbb\xbf  <?xml version", "a.igx"),
            Sniff::Allowed(MARKUP)
        );
        assert_eq!(
            sniff(b"-- lua\r\nlocal x = 1\n", "a.lua"),
            Sniff::Allowed(TEXT)
        );
        // Latin-1, not UTF-8: still text.
        assert_eq!(sniff(b"print(\"\xa1hola\")", "a.lua"), Sniff::Allowed(TEXT));
        // UTF-16 holds NULs.
        assert_eq!(sniff(b"\xff\xfet\0e\0", "a.txt"), Sniff::Unknown);
        assert_eq!(sniff(b"text\x01", "a.txt"), Sniff::Unknown);
    }

    #[test]
    fn an_all_zero_buffer_is_unknown_with_no_entropy() {
        let zeros = vec![0u8; 4096];
        assert_eq!(sniff(&zeros, "a.bin"), Sniff::Unknown);
        assert_eq!(entropy(&zeros), 0.0);
        assert_eq!(sniff(&[], "a.bin"), Sniff::Allowed(EMPTY));
    }

    #[test]
    fn a_random_buffer_is_unknown_with_high_entropy() {
        let bytes = noise(1 << 20, 0x9E37_79B9_7F4A_7C15);
        assert_eq!(sniff(&bytes, "a.bin"), Sniff::Unknown);
        let e = entropy(&bytes);
        assert!(e > 7.99, "{e}");
        // In pieces, the same.
        let mut acc = Entropy::default();
        for piece in bytes.chunks(777) {
            acc.update(piece);
        }
        assert_eq!(acc.bits_per_byte(), e);
    }

    #[test]
    fn two_equally_common_bytes_are_one_bit() {
        assert_eq!(entropy(&[0, 1, 0, 1]), 1.0);
    }
}
