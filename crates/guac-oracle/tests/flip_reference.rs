//! FLIP against NVlabs' published example (openspec AF.10): the reference
//! repo's `images/reference.png` and `images/test.png`, whose LDR mean its
//! `src/tests/test.py` pins at 0.159693 (0.159691 before its f64 mean,
//! `414653b`). The images are the reference's, not ours, so they stay in a
//! clone outside the tree: `GUAC_FLIP_REF` (process env) is its root. Unset,
//! the test says so and passes. Rerun it after any nv-flip bump, since AC.18
//! and AE.16 report numbers through this crate.

use guac_oracle::frames::{Frame, flip, reference_pixels_per_degree};

fn load(path: &std::path::Path) -> Frame {
    let image = image::open(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .into_rgb8();
    let (width, height) = image.dimensions();
    Frame::from_rgb8(width, height, image.into_raw()).unwrap()
}

#[test]
#[ignore = "needs a clone of github.com/NVlabs/flip at GUAC_FLIP_REF"]
fn the_reference_example_pair_gives_the_published_mean() {
    let Some(root) = std::env::var_os("GUAC_FLIP_REF") else {
        eprintln!("skipped: GUAC_FLIP_REF isn't set to a clone of NVlabs/flip");
        return;
    };
    let images = std::path::Path::new(&root).join("images");
    let reference = load(&images.join("reference.png"));
    let test = load(&images.join("test.png"));
    assert_eq!((reference.width(), reference.height()), (1920, 1080));

    let ppd = reference_pixels_per_degree();
    assert!((ppd - 67.020_645).abs() < 1e-5, "{ppd}");
    let map = flip(&reference, &test, ppd).unwrap();
    let six = |v: f64| (v * 1e6).round() / 1e6;
    assert_eq!(six(map.mean()), 0.159693, "mean {}", map.mean());
    assert_eq!(six(f64::from(map.max())), 0.928241);
    let min = map.values().iter().copied().fold(1.0f32, f32::min);
    assert_eq!(six(f64::from(min)), 0.000119);

    // nv-flip's DEFAULT_PIXELS_PER_DEGREE, 67 flat, isn't the reference's
    // viewing condition and doesn't reproduce it.
    let rounded = flip(&reference, &test, 67.0).unwrap();
    assert_eq!(six(rounded.mean()), 0.159717, "mean {}", rounded.mean());
}
