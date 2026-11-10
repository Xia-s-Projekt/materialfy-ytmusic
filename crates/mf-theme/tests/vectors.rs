//! Cross-check against Python's output (theme.build_scheme / seed_from_image).
use std::path::Path;

use serde_json::Value;
use mf_theme::{
    CrtRand, DEFAULT_SEED, ROLE_NAMES, Variant, build_scheme, format_color, parse_color,
    seed_from_image, seed_from_image_with, seed_from_pixels_with,
};

fn manifest(rel: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

#[test]
fn schemes_match_python() {
    let text = std::fs::read_to_string(manifest("tests/vectors.json")).unwrap();
    let vectors: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(vectors.len(), 56);
    let mut mismatches = Vec::new();
    for v in &vectors {
        let seed = parse_color(v["seed"].as_str().unwrap()).unwrap();
        let dark = v["dark"].as_bool().unwrap();
        let variant = Variant::from_name(v["variant"].as_str().unwrap()).unwrap();
        let contrast = v["contrast"].as_f64().unwrap();
        let scheme = build_scheme(seed, dark, variant, contrast);
        let expected = v["colors"].as_object().unwrap();
        assert_eq!(expected.len(), ROLE_NAMES.len());
        assert_eq!(scheme.colors.len(), ROLE_NAMES.len());
        for (role, want) in expected {
            let got = scheme.colors.get(role.as_str()).map(|&c| format_color(c));
            if got.as_deref() != want.as_str() {
                mismatches.push(format!(
                    "{} dark={dark} {variant}: {role} = {got:?}, expected {want}",
                    v["seed"]
                ));
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

#[test]
fn stable_roles_default_seed() {
    // brand purple, tonalSpot; the values come from Python's theme.build_scheme
    let dark = build_scheme(DEFAULT_SEED, true, Variant::TonalSpot, 0.0);
    let light = build_scheme(DEFAULT_SEED, false, Variant::TonalSpot, 0.0);
    let hex = |s: &mf_theme::Scheme, role: &str| format_color(s.colors[role]);
    for (role, want) in [
        ("primary", "#FFCDC0EC"),
        ("onPrimary", "#FF443A5F"),
        ("primaryContainer", "#FF574D72"),
        ("surface", "#FF0F0D12"),
        ("onSurface", "#FFEAE3EF"),
        ("primaryFixed", "#FFDED0FE"),
        ("primaryDim", "#FFBFB2DE"),
        ("error", "#FFF97386"),
    ] {
        assert_eq!(hex(&dark, role), want, "dark {role}");
    }
    for (role, want) in [
        ("primary", "#FF655789"),
        ("onPrimary", "#FFFDF7FF"),
        ("primaryContainer", "#FFD4C3FD"),
        ("surface", "#FFFDF7FE"),
        ("onSurface", "#FF34313A"),
        ("primaryFixedDim", "#FFC6B6EE"),
        ("inversePrimary", "#FFD4C3FD"),
    ] {
        assert_eq!(hex(&light, role), want, "light {role}");
    }
    for s in [&dark, &light] {
        assert_eq!(s.colors["shadow"], 0xFF000000);
        assert_eq!(s.colors["scrim"], 0xFF000000);
        assert_eq!(s.colors["surfaceTint"], s.colors["primary"]);
        assert_eq!(s.colors.len(), ROLE_NAMES.len());
    }
    assert_eq!(dark.seed, DEFAULT_SEED);
    assert!(dark.dark && !light.dark);
}

/// The 112×112 pixels Qt returns (QImage.scaled Smooth → RGB888) for seed.png.
/// On the same pixels, quantization and ranking must match Python exactly.
/// The references were recorded by Python on Windows, so rand() is MSVC (on other OSes Python itself gives something else).
#[test]
fn seed_from_qt_pixels_matches_python() {
    let raw = std::fs::read(manifest("tests/seed_112.rgb")).unwrap();
    assert_eq!(raw.len(), 112 * 112 * 3);
    let pixels: Vec<[u8; 3]> = raw.as_chunks::<3>().0.to_vec();
    assert_eq!(seed_from_pixels_with(&pixels, CrtRand::Msvc), 0xFFFFDB00);
}

/// The full path from a file: decoding `image` and the qSmoothScaleImage port (for seed.png 17 of 12544
/// pixels differ from Qt by 1), the seed matches Python — #FFFFDB00.
#[test]
fn seed_from_image_matches_python() {
    let png = manifest("tests/seed.png");
    assert_eq!(
        seed_from_image_with(&png, CrtRand::Msvc).unwrap(),
        0xFFFFDB00
    );
    // for this image the result does not depend on rand()
    assert_eq!(
        seed_from_image_with(&png, CrtRand::Glibc).unwrap(),
        0xFFFFDB00
    );
    assert_eq!(seed_from_image(&png).unwrap() >> 24, 0xFF);
}

#[test]
fn seed_from_missing_image_is_error() {
    assert!(seed_from_image(Path::new("no/such/image.png")).is_err());
}

/// A broad cross-check with Python: one line per scheme — seed dark variant contrast and 59 ARGB in ROLE_NAMES order.
/// By default — tests/schemes_extra.txt (217 schemes: hues around the circle, black/white, contrast −1…1, a sample
/// of 12852 schemes checked during the port); the full dump can be supplied via MF_THEME_DUMP=<file>.
#[test]
fn schemes_match_python_dump() {
    let path = std::env::var("MF_THEME_DUMP")
        .unwrap_or_else(|_| manifest("tests/schemes_extra.txt").display().to_string());
    let text = std::fs::read_to_string(path).unwrap();
    let mut bad = Vec::new();
    let mut total = 0;
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let seed = u32::from_str_radix(parts[0], 16).unwrap();
        let dark = parts[1] == "1";
        let variant = Variant::from_name(parts[2]).unwrap();
        let contrast: f64 = parts[3].parse().unwrap();
        let scheme = build_scheme(seed, dark, variant, contrast);
        total += 1;
        for (name, want) in ROLE_NAMES.iter().zip(&parts[4..]) {
            let got = format!("{:08X}", scheme.colors[name]);
            if got != *want {
                bad.push(format!(
                    "{seed:08X} dark={dark} {variant} c={contrast}: {name} = {got}, expected {want}"
                ));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{total} schemes, {} mismatches:\n{}",
        bad.len(),
        bad[..bad.len().min(80)].join("\n")
    );
}

/// Cross-check of seed_from_image with Python (Windows) on a set of images: MF_THEME_SEEDS=<file with "path ARGB" per line>.
/// During the port: 40 images, PNG — all matched, JPEG — 10 of 20 differ because of the decoder (see seed_from_image).
#[test]
#[ignore]
fn seeds_match_python_dump() {
    let Ok(path) = std::env::var("MF_THEME_SEEDS") else {
        return;
    };
    let text = std::fs::read_to_string(path).unwrap();
    let mut bad = Vec::new();
    let mut total = 0;
    for line in text.lines() {
        let (file, want) = line.rsplit_once(' ').unwrap();
        let got = format!(
            "{:08X}",
            seed_from_image_with(Path::new(file), CrtRand::Msvc).unwrap()
        );
        total += 1;
        let lower = file.to_ascii_lowercase();
        if got != want {
            // JPEG — a different decoder, the mismatch is expected; we only print it
            if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
                println!("JPEG {file}: {got}, Python has {want}");
            } else {
                bad.push(format!("{file}: {got}, expected {want}"));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{total} images, {} mismatches:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// The same, but on pixels already shrunk by Qt (a <path>.rgb file sits next to the image): it checks only
/// quantization and ranking (during the port — 40 out of 40).
#[test]
#[ignore]
fn seeds_from_qt_pixels_match_python_dump() {
    let Ok(path) = std::env::var("MF_THEME_SEEDS") else {
        return;
    };
    let text = std::fs::read_to_string(path).unwrap();
    let mut bad = Vec::new();
    for line in text.lines() {
        let (file, want) = line.rsplit_once(' ').unwrap();
        let raw = std::fs::read(format!("{file}.rgb")).unwrap();
        let pixels: Vec<[u8; 3]> = raw.as_chunks::<3>().0.to_vec();
        let got = format!("{:08X}", seed_from_pixels_with(&pixels, CrtRand::Msvc));
        if got != want {
            bad.push(format!("{file}: {got}, expected {want}"));
        }
    }
    assert!(
        bad.is_empty(),
        "{} mismatches:\n{}",
        bad.len(),
        bad.join("\n")
    );
}
