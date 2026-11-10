//! MD3 color scheme (src/materialfy/theme.py): materialyoucolor 3.0.4, spec "2025", platform "phone".
//! The reference is .venv/Lib/site-packages/materialyoucolor; values must match Python bit for bit
//! (see the test vectors in tests/vectors.json). Only HCT and the scheme's palettes are taken from material-colors;
//! role resolution (spec.rs), QuantizeCelebi (quantize.rs), Score (score.rs) and Qt's smoothing scale
//! (scale.rs) are ported from the reference — in material-colors they diverge from Python.
use std::collections::BTreeMap;
use std::path::Path;

use material_colors::color::Rgb;
use material_colors::dynamic_color::{
    DynamicScheme, Platform, Role, SpecVersion, Variant as MaterialVariant,
};
use material_colors::hct::Hct;

pub mod icons;
mod quantize;
pub use quantize::CrtRand;
mod scale;
mod score;
mod spec;

pub const DEFAULT_SEED: u32 = 0xFF6750A4;

/// 59 roles — exactly COLOR_NAMES from materialyoucolor (the keys of ThemeEngine.colors in QML).
pub const ROLE_NAMES: [&str; 59] = [
    "background",
    "onBackground",
    "surface",
    "surfaceDim",
    "surfaceBright",
    "surfaceContainerLowest",
    "surfaceContainerLow",
    "surfaceContainer",
    "surfaceContainerHigh",
    "surfaceContainerHighest",
    "onSurface",
    "surfaceVariant",
    "onSurfaceVariant",
    "outline",
    "outlineVariant",
    "inverseSurface",
    "inverseOnSurface",
    "shadow",
    "scrim",
    "surfaceTint",
    "primary",
    "primaryDim",
    "onPrimary",
    "primaryContainer",
    "onPrimaryContainer",
    "inversePrimary",
    "primaryFixed",
    "primaryFixedDim",
    "onPrimaryFixed",
    "onPrimaryFixedVariant",
    "secondary",
    "secondaryDim",
    "onSecondary",
    "secondaryContainer",
    "onSecondaryContainer",
    "secondaryFixed",
    "secondaryFixedDim",
    "onSecondaryFixed",
    "onSecondaryFixedVariant",
    "tertiary",
    "tertiaryDim",
    "onTertiary",
    "tertiaryContainer",
    "onTertiaryContainer",
    "tertiaryFixed",
    "tertiaryFixedDim",
    "onTertiaryFixed",
    "onTertiaryFixedVariant",
    "error",
    "errorDim",
    "onError",
    "errorContainer",
    "onErrorContainer",
    "primaryPaletteKeyColor",
    "secondaryPaletteKeyColor",
    "tertiaryPaletteKeyColor",
    "neutralPaletteKeyColor",
    "neutralVariantPaletteKeyColor",
    "errorPaletteKeyColor",
];

/// The material-colors roles in ROLE_NAMES order.
const ROLES: [Role; 59] = [
    Role::Background,
    Role::OnBackground,
    Role::Surface,
    Role::SurfaceDim,
    Role::SurfaceBright,
    Role::SurfaceContainerLowest,
    Role::SurfaceContainerLow,
    Role::SurfaceContainer,
    Role::SurfaceContainerHigh,
    Role::SurfaceContainerHighest,
    Role::OnSurface,
    Role::SurfaceVariant,
    Role::OnSurfaceVariant,
    Role::Outline,
    Role::OutlineVariant,
    Role::InverseSurface,
    Role::InverseOnSurface,
    Role::Shadow,
    Role::Scrim,
    Role::SurfaceTint,
    Role::Primary,
    Role::PrimaryDim,
    Role::OnPrimary,
    Role::PrimaryContainer,
    Role::OnPrimaryContainer,
    Role::InversePrimary,
    Role::PrimaryFixed,
    Role::PrimaryFixedDim,
    Role::OnPrimaryFixed,
    Role::OnPrimaryFixedVariant,
    Role::Secondary,
    Role::SecondaryDim,
    Role::OnSecondary,
    Role::SecondaryContainer,
    Role::OnSecondaryContainer,
    Role::SecondaryFixed,
    Role::SecondaryFixedDim,
    Role::OnSecondaryFixed,
    Role::OnSecondaryFixedVariant,
    Role::Tertiary,
    Role::TertiaryDim,
    Role::OnTertiary,
    Role::TertiaryContainer,
    Role::OnTertiaryContainer,
    Role::TertiaryFixed,
    Role::TertiaryFixedDim,
    Role::OnTertiaryFixed,
    Role::OnTertiaryFixedVariant,
    Role::Error,
    Role::ErrorDim,
    Role::OnError,
    Role::ErrorContainer,
    Role::OnErrorContainer,
    Role::PrimaryPaletteKeyColor,
    Role::SecondaryPaletteKeyColor,
    Role::TertiaryPaletteKeyColor,
    Role::NeutralPaletteKeyColor,
    Role::NeutralVariantPaletteKeyColor,
    Role::ErrorPaletteKeyColor,
];

/// Scheme variants; the names are the values of Settings.schemeVariant and ThemeEngine.variants (in this order).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Variant {
    #[default]
    Content,
    TonalSpot,
    Vibrant,
    Expressive,
    Fidelity,
    Neutral,
    Monochrome,
}

impl Variant {
    pub const ALL: [Variant; 7] = [
        Variant::Content,
        Variant::TonalSpot,
        Variant::Vibrant,
        Variant::Expressive,
        Variant::Fidelity,
        Variant::Neutral,
        Variant::Monochrome,
    ];

    /// "content" | "tonalSpot" | "vibrant" | "expressive" | "fidelity" | "neutral" | "monochrome"
    pub fn name(self) -> &'static str {
        match self {
            Variant::Content => "content",
            Variant::TonalSpot => "tonalSpot",
            Variant::Vibrant => "vibrant",
            Variant::Expressive => "expressive",
            Variant::Fidelity => "fidelity",
            Variant::Neutral => "neutral",
            Variant::Monochrome => "monochrome",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.name() == name)
    }

    /// The names in ThemeEngine.variants order.
    pub fn names() -> [&'static str; 7] {
        Self::ALL.map(Variant::name)
    }

    fn material(self) -> MaterialVariant {
        match self {
            Variant::Content => MaterialVariant::Content,
            Variant::TonalSpot => MaterialVariant::TonalSpot,
            Variant::Vibrant => MaterialVariant::Vibrant,
            Variant::Expressive => MaterialVariant::Expressive,
            Variant::Fidelity => MaterialVariant::Fidelity,
            Variant::Neutral => MaterialVariant::Neutral,
            Variant::Monochrome => MaterialVariant::Monochrome,
        }
    }
}

impl std::fmt::Display for Variant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for Variant {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        Self::from_name(s).ok_or(())
    }
}

/// The ARGB of all 59 roles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scheme {
    pub seed: u32,
    pub dark: bool,
    pub variant: Variant,
    pub colors: BTreeMap<&'static str, u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
    #[error("could not open image: {0}")]
    Image(String),
}

/// A scheme from a seed (build_scheme in theme.py). ~1 ms; safe to call from the GUI thread for the startup scheme.
pub fn build_scheme(seed: u32, dark: bool, variant: Variant, contrast: f64) -> Scheme {
    // Content/Fidelity/Monochrome fall back to spec 2021 by themselves — like _maybe_fallback_spec_version in Python
    let scheme = DynamicScheme::from_spec(
        Hct::new(Rgb::from_u32(seed)),
        variant.material(),
        dark,
        Some(contrast),
        Platform::Phone,
        SpecVersion::Spec2025,
    );
    // palettes — from material-colors, role resolution — a Python port (spec.rs)
    let resolver = spec::Resolver::new(&scheme);
    let colors = ROLE_NAMES
        .iter()
        .zip(ROLES)
        .map(|(&name, role)| (name, resolver.argb(role)))
        .collect();
    Scheme {
        seed,
        dark,
        variant,
        colors,
    }
}

/// The side of the image it is shrunk to before quantization.
pub const SAMPLE_SIZE: u32 = 112;
/// The number of QuantizeCelebi clusters.
pub const MAX_COLORS: usize = 128;

/// The dominant color of an image (seed_from_image): scale to 112×112 (smoothing, aspect ratio not kept),
/// RGB, QuantizeCelebi(pixels, 128) → Score.score → the first; empty → DEFAULT_SEED.
///
/// Differences from Python: PNGs give the same pixels as Qt, up to one unit in rare edge pixels;
/// JPEG is decoded by zune-jpeg rather than libjpeg-turbo (≈1 % of samples by 1–2 levels), so for some covers the seed
/// comes out as a neighboring hue. Quantization and ranking on the same pixels match exactly.
/// Blocking (decoding + ~10 ms of quantization) — call from spawn_blocking.
pub fn seed_from_image(path: &Path) -> Result<u32, ThemeError> {
    seed_from_image_with(path, CrtRand::native())
}

/// seed_from_image with an explicit choice of rand() (see CrtRand).
pub fn seed_from_image_with(path: &Path, crt: CrtRand) -> Result<u32, ThemeError> {
    let image = image::ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| ThemeError::Image(format!("{}: {e}", path.display())))?
        .decode()
        .map_err(|e| ThemeError::Image(format!("{}: {e}", path.display())))?;
    Ok(seed_from_rgba_with(&image.to_rgba8(), crt))
}

/// The same for an already decoded image (for example from memory).
pub fn seed_from_rgba(image: &image::RgbaImage) -> u32 {
    seed_from_rgba_with(image, CrtRand::native())
}

pub fn seed_from_rgba_with(image: &image::RgbaImage, crt: CrtRand) -> u32 {
    seed_from_pixels_with(
        &scale::smooth_scale_rgb(image, SAMPLE_SIZE, SAMPLE_SIZE),
        crt,
    )
}

/// The same for ready RGB pixels.
pub fn seed_from_pixels(pixels: &[[u8; 3]]) -> u32 {
    seed_from_pixels_with(pixels, CrtRand::native())
}

pub fn seed_from_pixels_with(pixels: &[[u8; 3]], crt: CrtRand) -> u32 {
    if pixels.is_empty() {
        return DEFAULT_SEED;
    }
    let argb: Vec<u32> = pixels
        .iter()
        .map(|&[r, g, b]| 0xFF00_0000 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))
        .collect();
    let population = quantize::quantize_celebi(&argb, MAX_COLORS, crt);
    score::score(&population)
        .first()
        .copied()
        .unwrap_or(DEFAULT_SEED)
}

/// "#RRGGBB" / "#AARRGGBB" → ARGB (alpha FF), like QColor(str); an invalid string → None.
/// Like QColor, it also understands "#RGB", "#RRRGGGBBB" and "#RRRRGGGGBBBB" (the top 8 bits of the channel are taken).
pub fn parse_color(text: &str) -> Option<u32> {
    let hex = text.trim().strip_prefix('#')?;
    if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let value = |s: &str| u32::from_str_radix(s, 16).ok();
    match hex.len() {
        3 => {
            let v = value(hex)?;
            let (r, g, b) = ((v >> 8) & 0xF, (v >> 4) & 0xF, v & 0xF);
            Some(0xFF00_0000 | (r * 0x11) << 16 | (g * 0x11) << 8 | (b * 0x11))
        }
        6 => Some(0xFF00_0000 | value(hex)?),
        8 => value(hex),
        9 | 12 => {
            let n = hex.len() / 3;
            let channel = |i: usize| value(&hex[i * n..(i + 1) * n]).map(|c| c >> ((n - 2) * 4));
            Some(0xFF00_0000 | channel(0)? << 16 | channel(1)? << 8 | channel(2)?)
        }
        _ => None,
    }
}

/// ARGB → "#AARRGGBB" (upper case, as in the test vectors).
pub fn format_color(argb: u32) -> String {
    format!("#{argb:08X}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camel(snake: &str) -> String {
        let mut out = String::new();
        let mut upper = false;
        for c in snake.chars() {
            if c == '_' {
                upper = true;
            } else if upper {
                out.extend(c.to_uppercase());
                upper = false;
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn roles_match_names() {
        for (name, role) in ROLE_NAMES.iter().zip(ROLES) {
            assert_eq!(*name, camel(role.name()));
        }
    }

    #[test]
    fn variant_names() {
        assert_eq!(
            Variant::names(),
            [
                "content",
                "tonalSpot",
                "vibrant",
                "expressive",
                "fidelity",
                "neutral",
                "monochrome"
            ]
        );
        for v in Variant::ALL {
            assert_eq!(Variant::from_name(v.name()), Some(v));
        }
        assert_eq!(Variant::from_name("rainbow"), None);
    }

    #[test]
    fn parse_colors() {
        assert_eq!(parse_color("#6750A4"), Some(0xFF6750A4));
        assert_eq!(parse_color("#806750a4"), Some(0x806750A4));
        assert_eq!(parse_color("#fc0"), Some(0xFFFFCC00));
        assert_eq!(parse_color("#ffffcccc0000"), Some(0xFFFFCC00));
        assert_eq!(parse_color(""), None);
        assert_eq!(parse_color("6750A4"), None);
        assert_eq!(parse_color("#6750A"), None);
        assert_eq!(parse_color("#zzzzzz"), None);
        assert_eq!(format_color(0xFF6750A4), "#FF6750A4");
    }

    #[test]
    fn empty_pixels_default_seed() {
        assert_eq!(seed_from_pixels(&[]), DEFAULT_SEED);
    }
}
