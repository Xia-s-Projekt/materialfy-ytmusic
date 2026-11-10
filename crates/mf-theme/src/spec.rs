//! Port of role resolution from materialyoucolor 3.0.4 (dynamiccolor/color_spec_2021.py, color_spec_2025.py,
//! ColorCalculationDelegateImpl2021/2025 in color_spec.py), platform "phone".
//!
//! Why not DynamicScheme::get_rgb from material-colors: Python diverges from it in a few places,
//! and the values must match bit for bit. Known differences:
//! - the *Dim roles in spec 2021 schemes (content/fidelity/monochrome): for the "darker" polarity Python takes
//!   the "nearer" role in the dark theme, material-colors in the light one;
//! - expressive, dark: primaryContainer = t_max_c(30, 93), not t_min_c;
//! - expressive primary: the yellow/cyan checks are the same for dark and light;
//! - TonalPalette.tone(99) for yellow hues — the mean of T98 and T100 (2021 calculation only);
//! - banker's rounding of round() in Dislike.
//!
//! Palettes, HCT and the solver come from material-colors (they match Python).
use std::cell::Cell;

use material_colors::color::{Rgb, linearized, y_from_lstar};
use material_colors::dynamic_color::{DynamicScheme, Role, SpecVersion, Variant};
use material_colors::hct::Hct;
use material_colors::palette::TonalPalette;

// L* as in utils/color_utils.py: material-colors computes it via mul_add/cbrt and at the edges gives -2e-16
// instead of 0 (for a black seed this changes onPrimaryContainer), so the tones that feed into contrast are our own.

fn lab_f(t: f64) -> f64 {
    let e = 216.0 / 24389.0;
    let kappa = 24389.0 / 27.0;
    if t > e {
        t.powf(1.0 / 3.0)
    } else {
        (kappa * t + 16.0) / 116.0
    }
}

fn lstar_from_y(y: f64) -> f64 {
    lab_f(y / 100.0) * 116.0 - 16.0
}

/// The color's Hct.tone (lstar_from_argb).
fn lstar(rgb: Rgb) -> f64 {
    let y = linearized(rgb.red) * 0.2126
        + linearized(rgb.green) * 0.7152
        + linearized(rgb.blue) * 0.0722;
    116.0 * lab_f(y / 100.0) - 16.0
}

fn tone_of(hct: Hct) -> f64 {
    lstar(hct.into())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Spec {
    V2021,
    V2025,
}

#[derive(Clone, Copy, Debug)]
struct Curve {
    low: f64,
    normal: f64,
    medium: f64,
    high: f64,
}

const fn curve(low: f64, normal: f64, medium: f64, high: f64) -> Curve {
    Curve {
        low,
        normal,
        medium,
        high,
    }
}

fn lerp(start: f64, stop: f64, amount: f64) -> f64 {
    (1.0 - amount) * start + amount * stop
}

impl Curve {
    fn get(self, level: f64) -> f64 {
        if level <= -1.0 {
            self.low
        } else if level < 0.0 {
            lerp(self.low, self.normal, level + 1.0)
        } else if level < 0.5 {
            lerp(self.normal, self.medium, level / 0.5)
        } else if level < 1.0 {
            lerp(self.medium, self.high, (level - 0.5) / 0.5)
        } else {
            self.high
        }
    }
}

/// get_curve from color_spec_2025.py.
fn std_curve(default: f64) -> Curve {
    match default {
        1.5 => curve(1.5, 1.5, 3.0, 5.5),
        3.0 => curve(3.0, 3.0, 4.5, 7.0),
        4.5 => curve(4.5, 4.5, 7.0, 11.0),
        6.0 => curve(6.0, 6.0, 7.0, 11.0),
        7.0 => curve(7.0, 7.0, 11.0, 21.0),
        9.0 => curve(9.0, 9.0, 11.0, 21.0),
        11.0 => curve(11.0, 11.0, 21.0, 21.0),
        21.0 => curve(21.0, 21.0, 21.0, 21.0),
        d => curve(d, d, 7.0, 21.0),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Polarity {
    Darker,
    Lighter,
    Nearer,
    RelativeLighter,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Constraint {
    Exact,
    Farther,
}

#[derive(Clone, Copy, Debug)]
struct Pair {
    a: Role,
    b: Role,
    delta: f64,
    polarity: Polarity,
    stay_together: bool,
    constraint: Constraint,
}

const fn pair(
    a: Role,
    b: Role,
    delta: f64,
    polarity: Polarity,
    stay_together: bool,
    constraint: Constraint,
) -> Pair {
    Pair {
        a,
        b,
        delta,
        polarity,
        stay_together,
        constraint,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pal {
    Primary,
    Secondary,
    Tertiary,
    Neutral,
    NeutralVariant,
    Error,
}

/// clamp_double(min, max, x) from Python: no panic when min > max.
fn clamp(min: f64, max: f64, x: f64) -> f64 {
    if x < min {
        min
    } else if x > max {
        max
    } else {
        x
    }
}

// --- Contrast (contrast/contrast.py) ---------------------------------------------------------------

fn ratio_of_ys(y1: f64, y2: f64) -> f64 {
    let lighter = y1.max(y2);
    let darker = if lighter == y2 { y1 } else { y2 };
    (lighter + 5.0) / (darker + 5.0)
}

fn ratio_of_tones(a: f64, b: f64) -> f64 {
    ratio_of_ys(
        y_from_lstar(clamp(0.0, 100.0, a)),
        y_from_lstar(clamp(0.0, 100.0, b)),
    )
}

fn lighter(tone: f64, ratio: f64) -> f64 {
    if !(0.0..=100.0).contains(&tone) {
        return -1.0;
    }
    let dark_y = y_from_lstar(tone);
    let light_y = ratio * (dark_y + 5.0) - 5.0;
    let real = ratio_of_ys(light_y, dark_y);
    if real < ratio && (real - ratio).abs() > 0.04 {
        return -1.0;
    }
    let value = lstar_from_y(light_y) + 0.4;
    if !(0.0..=100.0).contains(&value) {
        -1.0
    } else {
        value
    }
}

fn darker(tone: f64, ratio: f64) -> f64 {
    if !(0.0..=100.0).contains(&tone) {
        return -1.0;
    }
    let light_y = y_from_lstar(tone);
    let dark_y = (light_y + 5.0) / ratio - 5.0;
    let real = ratio_of_ys(light_y, dark_y);
    if real < ratio && (real - ratio).abs() > 0.04 {
        return -1.0;
    }
    let value = lstar_from_y(dark_y) - 0.4;
    if !(0.0..=100.0).contains(&value) {
        -1.0
    } else {
        value
    }
}

fn lighter_unsafe(tone: f64, ratio: f64) -> f64 {
    let v = lighter(tone, ratio);
    if v < 0.0 { 100.0 } else { v }
}

fn darker_unsafe(tone: f64, ratio: f64) -> f64 {
    let v = darker(tone, ratio);
    if v < 0.0 { 0.0 } else { v }
}

fn prefers_light_foreground(tone: f64) -> bool {
    tone.round_ties_even() < 60.0
}

fn foreground_tone(bg_tone: f64, ratio: f64) -> f64 {
    let lighter_tone = lighter_unsafe(bg_tone, ratio);
    let darker_tone = darker_unsafe(bg_tone, ratio);
    let lighter_ratio = ratio_of_tones(lighter_tone, bg_tone);
    let darker_ratio = ratio_of_tones(darker_tone, bg_tone);
    if prefers_light_foreground(bg_tone) {
        let negligible = (lighter_ratio - darker_ratio).abs() < 0.1
            && lighter_ratio < ratio
            && darker_ratio < ratio;
        if lighter_ratio >= ratio || lighter_ratio >= darker_ratio || negligible {
            lighter_tone
        } else {
            darker_tone
        }
    } else if darker_ratio >= ratio || darker_ratio >= lighter_ratio {
        darker_tone
    } else {
        lighter_tone
    }
}

// --- palettes and tones ---------------------------------------------------------------------------------

fn find_best_tone_for_chroma(
    hue: f64,
    chroma: f64,
    mut tone: f64,
    by_decreasing_tone: bool,
) -> f64 {
    let mut answer = tone;
    let mut best = Hct::from(hue, chroma, answer);
    while best.get_chroma() < chroma {
        if !(0.0..=100.0).contains(&tone) {
            break;
        }
        tone += if by_decreasing_tone { -1.0 } else { 1.0 };
        let candidate = Hct::from(hue, chroma, tone);
        if best.get_chroma() < candidate.get_chroma() {
            best = candidate;
            answer = tone;
        }
    }
    answer
}

/// find_desired_chroma_by_tone from color_spec_2021.py.
fn find_desired_chroma_by_tone(hue: f64, chroma: f64, tone: f64, by_decreasing_tone: bool) -> f64 {
    let mut answer = tone;
    let mut closest = Hct::from(hue, chroma, tone);
    if closest.get_chroma() < chroma {
        let mut peak = closest.get_chroma();
        while closest.get_chroma() < chroma {
            answer += if by_decreasing_tone { -1.0 } else { 1.0 };
            let candidate = Hct::from(hue, chroma, answer);
            if peak > candidate.get_chroma() {
                break;
            }
            if (candidate.get_chroma() - chroma).abs() < 0.4 {
                break;
            }
            if (candidate.get_chroma() - chroma).abs() < (closest.get_chroma() - chroma).abs() {
                closest = candidate;
            }
            peak = peak.max(candidate.get_chroma());
        }
    }
    answer
}

/// TonalPalette.tone(t) from Python: T99 for yellows is the mean of T98 and T100.
fn palette_tone(palette: &TonalPalette, tone: f64) -> Rgb {
    if tone == 99.0 && Hct::is_yellow(palette.hue()) {
        let a = palette_tone(palette, 98.0);
        let b = palette_tone(palette, 100.0);
        let avg = |x: u8, y: u8| ((f64::from(x) + f64::from(y)) / 2.0).round_ties_even() as u8;
        return Rgb::new(
            avg(a.red, b.red),
            avg(a.green, b.green),
            avg(a.blue, b.blue),
        );
    }
    Hct::from(palette.hue(), palette.chroma(), tone).into()
}

fn fix_if_disliked(hct: Hct) -> Hct {
    let hue = hct.get_hue().round_ties_even();
    let disliked = (90.0..=111.0).contains(&hue)
        && hct.get_chroma().round_ties_even() > 16.0
        && tone_of(hct).round_ties_even() < 65.0;
    if disliked {
        Hct::from(hct.get_hue(), hct.get_chroma(), 70.0)
    } else {
        hct
    }
}

fn is_dim(role: Role) -> bool {
    matches!(
        role,
        Role::PrimaryDim | Role::SecondaryDim | Role::TertiaryDim | Role::ErrorDim
    )
}

fn is_fixed_dim(role: Role) -> bool {
    matches!(
        role,
        Role::PrimaryFixedDim | Role::SecondaryFixedDim | Role::TertiaryFixedDim
    )
}

/// is_background of the extended color — always from the 2021 definition (for *Dim — from 2025); they agree.
fn is_background(role: Role) -> bool {
    use Role::*;
    matches!(
        role,
        Background
            | Surface
            | SurfaceDim
            | SurfaceBright
            | SurfaceContainerLowest
            | SurfaceContainerLow
            | SurfaceContainer
            | SurfaceContainerHigh
            | SurfaceContainerHighest
            | SurfaceVariant
            | InverseSurface
            | SurfaceTint
            | Primary
            | PrimaryDim
            | PrimaryContainer
            | PrimaryFixed
            | PrimaryFixedDim
            | Secondary
            | SecondaryDim
            | SecondaryContainer
            | SecondaryFixed
            | SecondaryFixedDim
            | Tertiary
            | TertiaryDim
            | TertiaryContainer
            | TertiaryFixed
            | TertiaryFixedDim
            | Error
            | ErrorDim
            | ErrorContainer
    )
}

/// Role resolution for a single scheme. Tones are cached (pure functions of the scheme and the role).
pub(crate) struct Resolver<'a> {
    scheme: &'a DynamicScheme,
    dark: bool,
    contrast: f64,
    spec: Spec,
    tones: Vec<Cell<f64>>,
    peaks: [Cell<f64>; 12],
}

impl<'a> Resolver<'a> {
    pub(crate) fn new(scheme: &'a DynamicScheme) -> Self {
        Self::with_mode(scheme, scheme.is_dark, scheme.contrast_level)
    }

    fn with_mode(scheme: &'a DynamicScheme, dark: bool, contrast: f64) -> Self {
        let spec = match scheme.spec_version {
            SpecVersion::Spec2021 => Spec::V2021,
            _ => Spec::V2025,
        };
        Self {
            scheme,
            dark,
            contrast,
            spec,
            tones: (0..Role::COUNT).map(|_| Cell::new(f64::NAN)).collect(),
            peaks: std::array::from_fn(|_| Cell::new(f64::NAN)),
        }
    }

    /// The role's ARGB (get_argb in Python).
    pub(crate) fn argb(&self, role: Role) -> u32 {
        let tone = self.tone(role);
        let palette = self.palette(role);
        let rgb: Rgb = match self.spec {
            Spec::V2021 => palette_tone(palette, tone),
            Spec::V2025 => Hct::from(
                palette.hue(),
                palette.chroma() * self.chroma_multiplier(role),
                tone,
            )
            .into(),
        };
        rgb.as_argb_u32()
    }

    // --- shared helpers ---

    fn variant(&self) -> Variant {
        self.scheme.variant
    }

    fn pal(&self, pal: Pal) -> &TonalPalette {
        let s = self.scheme;
        match pal {
            Pal::Primary => &s.primary_palette,
            Pal::Secondary => &s.secondary_palette,
            Pal::Tertiary => &s.tertiary_palette,
            Pal::Neutral => &s.neutral_palette,
            Pal::NeutralVariant => &s.neutral_variant_palette,
            Pal::Error => &s.error_palette,
        }
    }

    fn peak(&self, pal: Pal, max: bool) -> f64 {
        let slot = &self.peaks[pal as usize * 2 + usize::from(max)];
        if slot.get().is_nan() {
            let p = self.pal(pal);
            slot.set(find_best_tone_for_chroma(
                p.hue(),
                p.chroma(),
                if max { 100.0 } else { 0.0 },
                max,
            ));
        }
        slot.get()
    }

    fn t_max_c(&self, pal: Pal, lower: f64, upper: f64) -> f64 {
        clamp(lower, upper, self.peak(pal, true))
    }

    fn t_min_c(&self, pal: Pal, lower: f64, upper: f64) -> f64 {
        clamp(lower, upper, self.peak(pal, false))
    }

    fn highest_surface(&self) -> Role {
        if self.dark {
            Role::SurfaceBright
        } else {
            Role::SurfaceDim
        }
    }

    fn neutral_yellow(&self) -> bool {
        Hct::is_yellow(self.scheme.neutral_palette.hue())
    }

    /// Which version's definition applies to a role: in a 2025 scheme — 2025, if the role is overridden;
    /// in a 2021 scheme — 2021, except for the *Dim roles, which 2021 does not have.
    fn def_spec(&self, role: Role) -> Spec {
        use Role::*;
        let has_2025 = !matches!(
            role,
            PrimaryPaletteKeyColor
                | SecondaryPaletteKeyColor
                | TertiaryPaletteKeyColor
                | NeutralPaletteKeyColor
                | NeutralVariantPaletteKeyColor
                | ErrorPaletteKeyColor
                | Shadow
                | Scrim
        );
        match self.spec {
            Spec::V2025 if has_2025 => Spec::V2025,
            Spec::V2021 if is_dim(role) => Spec::V2025,
            _ => Spec::V2021,
        }
    }

    /// A role whose 2025 definition this one copies (clone() in Python): same properties, its own name.
    fn alias_2025(role: Role) -> Role {
        match role {
            Role::SurfaceVariant => Role::SurfaceContainerHighest,
            Role::SurfaceTint => Role::Primary,
            Role::Background => Role::Surface,
            Role::OnBackground => Role::OnSurface,
            r => r,
        }
    }

    fn palette(&self, role: Role) -> &TonalPalette {
        use Role::*;
        let spec = self.def_spec(role);
        let pal = match role {
            PrimaryPaletteKeyColor
            | SurfaceTint
            | Primary
            | PrimaryDim
            | OnPrimary
            | PrimaryContainer
            | OnPrimaryContainer
            | InversePrimary
            | PrimaryFixed
            | PrimaryFixedDim
            | OnPrimaryFixed
            | OnPrimaryFixedVariant => Pal::Primary,
            SecondaryPaletteKeyColor
            | Secondary
            | SecondaryDim
            | OnSecondary
            | SecondaryContainer
            | OnSecondaryContainer
            | SecondaryFixed
            | SecondaryFixedDim
            | OnSecondaryFixed
            | OnSecondaryFixedVariant => Pal::Secondary,
            TertiaryPaletteKeyColor
            | Tertiary
            | TertiaryDim
            | OnTertiary
            | TertiaryContainer
            | OnTertiaryContainer
            | TertiaryFixed
            | TertiaryFixedDim
            | OnTertiaryFixed
            | OnTertiaryFixedVariant => Pal::Tertiary,
            ErrorPaletteKeyColor | Error | ErrorDim | OnError | ErrorContainer
            | OnErrorContainer => Pal::Error,
            NeutralVariantPaletteKeyColor => Pal::NeutralVariant,
            SurfaceVariant | OnSurfaceVariant | Outline | OutlineVariant => {
                if spec == Spec::V2025 {
                    Pal::Neutral
                } else {
                    Pal::NeutralVariant
                }
            }
            _ => Pal::Neutral,
        };
        self.pal(pal)
    }

    fn chroma_multiplier(&self, role: Role) -> f64 {
        use Role::*;
        if self.def_spec(role) != Spec::V2025 {
            return 1.0;
        }
        let role = Self::alias_2025(role);
        let v = self.variant();
        let yellow = self.neutral_yellow();
        let table = |neutral: f64,
                     tonal_spot: f64,
                     expressive_yellow: f64,
                     expressive: f64,
                     vibrant: f64| match v {
            Variant::Neutral => neutral,
            Variant::TonalSpot => tonal_spot,
            Variant::Expressive => {
                if yellow {
                    expressive_yellow
                } else {
                    expressive
                }
            }
            Variant::Vibrant => vibrant,
            _ => 1.0,
        };
        match role {
            SurfaceDim if !self.dark => table(2.5, 1.7, 2.7, 1.75, 1.36),
            SurfaceBright if self.dark => table(2.5, 1.7, 2.7, 1.75, 1.36),
            SurfaceContainerLow => table(1.3, 1.25, 1.3, 1.15, 1.08),
            SurfaceContainer => table(1.6, 1.4, 1.6, 1.3, 1.15),
            SurfaceContainerHigh => table(1.9, 1.5, 1.95, 1.45, 1.22),
            SurfaceContainerHighest => table(2.2, 1.7, 2.3, 1.6, 1.29),
            OnSurface | OnSurfaceVariant | Outline | OutlineVariant => {
                let expressive_yellow = if self.dark { 3.0 } else { 2.3 };
                match v {
                    Variant::Neutral => 2.2,
                    Variant::TonalSpot => 1.7,
                    Variant::Expressive => {
                        if yellow {
                            expressive_yellow
                        } else {
                            1.6
                        }
                    }
                    _ => 1.0,
                }
            }
            _ => 1.0,
        }
    }

    fn background(&self, role: Role) -> Option<Role> {
        match self.def_spec(role) {
            Spec::V2021 => self.background_2021(role),
            Spec::V2025 => self.background_2025(Self::alias_2025(role)),
        }
    }

    fn second_background(&self, role: Role) -> Option<Role> {
        use Role::*;
        if self.def_spec(role) != Spec::V2021 {
            return None;
        }
        match role {
            OnPrimaryFixed | OnPrimaryFixedVariant => Some(PrimaryFixed),
            OnSecondaryFixed | OnSecondaryFixedVariant => Some(SecondaryFixed),
            OnTertiaryFixed | OnTertiaryFixedVariant => Some(TertiaryFixed),
            _ => None,
        }
    }

    fn contrast_curve(&self, role: Role) -> Option<Curve> {
        match self.def_spec(role) {
            Spec::V2021 => Self::curve_2021(role),
            Spec::V2025 => self.curve_2025(Self::alias_2025(role)),
        }
    }

    fn tone_delta_pair(&self, role: Role) -> Option<Pair> {
        match self.def_spec(role) {
            Spec::V2021 => Self::pair_2021(role),
            Spec::V2025 => Self::pair_2025(Self::alias_2025(role)),
        }
    }

    /// The source tone (the definition's tone lambda), before contrast and pairs.
    fn raw_tone(&self, role: Role) -> f64 {
        match self.def_spec(role) {
            Spec::V2021 => self.raw_tone_2021(role),
            Spec::V2025 => self.raw_tone_2025(role),
        }
    }

    /// The final tone (get_tone): computed by the scheme's version.
    fn tone(&self, role: Role) -> f64 {
        let slot = &self.tones[role as usize];
        if slot.get().is_nan() {
            let tone = match self.spec {
                Spec::V2021 => self.calc_tone_2021(role),
                Spec::V2025 => self.calc_tone_2025(role),
            };
            slot.set(tone);
        }
        slot.get()
    }

    /// The default tone for from_palette without tone: the background's tone, or 50.
    fn tone_from_background(&self, role: Role) -> f64 {
        self.background(role).map_or(50.0, |bg| self.tone(bg))
    }

    // --- spec 2021 ---

    fn background_2021(&self, role: Role) -> Option<Role> {
        use Role::*;
        Some(match role {
            OnBackground => Background,
            OnSurface | OnSurfaceVariant | Outline | OutlineVariant | Primary
            | PrimaryContainer | Secondary | SecondaryContainer | Tertiary | TertiaryContainer
            | Error | ErrorContainer | PrimaryFixed | PrimaryFixedDim | SecondaryFixed
            | SecondaryFixedDim | TertiaryFixed | TertiaryFixedDim => self.highest_surface(),
            InverseOnSurface | InversePrimary => InverseSurface,
            OnPrimary => Primary,
            OnPrimaryContainer => PrimaryContainer,
            OnSecondary => Secondary,
            OnSecondaryContainer => SecondaryContainer,
            OnTertiary => Tertiary,
            OnTertiaryContainer => TertiaryContainer,
            OnError => Error,
            OnErrorContainer => ErrorContainer,
            OnPrimaryFixed | OnPrimaryFixedVariant => PrimaryFixedDim,
            OnSecondaryFixed | OnSecondaryFixedVariant => SecondaryFixedDim,
            OnTertiaryFixed | OnTertiaryFixedVariant => TertiaryFixedDim,
            _ => return None,
        })
    }

    fn curve_2021(role: Role) -> Option<Curve> {
        use Role::*;
        Some(match role {
            OnBackground => curve(3.0, 3.0, 4.5, 7.0),
            OnSurface | InverseOnSurface | OnPrimary | OnSecondary | OnTertiary | OnError
            | OnPrimaryFixed | OnSecondaryFixed | OnTertiaryFixed => curve(4.5, 7.0, 11.0, 21.0),
            OnSurfaceVariant
            | OnPrimaryContainer
            | OnSecondaryContainer
            | OnTertiaryContainer
            | OnErrorContainer
            | OnPrimaryFixedVariant
            | OnSecondaryFixedVariant
            | OnTertiaryFixedVariant => curve(3.0, 4.5, 7.0, 11.0),
            Outline => curve(1.5, 3.0, 4.5, 7.0),
            OutlineVariant | PrimaryContainer | SecondaryContainer | TertiaryContainer
            | ErrorContainer | PrimaryFixed | PrimaryFixedDim | SecondaryFixed
            | SecondaryFixedDim | TertiaryFixed | TertiaryFixedDim => curve(1.0, 1.0, 3.0, 4.5),
            Primary | Secondary | Tertiary | Error | InversePrimary => curve(3.0, 4.5, 7.0, 7.0),
            _ => return None,
        })
    }

    fn pair_2021(role: Role) -> Option<Pair> {
        use Role::*;
        let container = |a, b| Some(pair(a, b, 10.0, Polarity::Nearer, false, Constraint::Exact));
        let fixed = |a, b| Some(pair(a, b, 10.0, Polarity::Lighter, true, Constraint::Exact));
        match role {
            Primary | PrimaryContainer => container(PrimaryContainer, Primary),
            Secondary | SecondaryContainer => container(SecondaryContainer, Secondary),
            Tertiary | TertiaryContainer => container(TertiaryContainer, Tertiary),
            Error | ErrorContainer => container(ErrorContainer, Error),
            PrimaryFixed | PrimaryFixedDim => fixed(PrimaryFixed, PrimaryFixedDim),
            SecondaryFixed | SecondaryFixedDim => fixed(SecondaryFixed, SecondaryFixedDim),
            TertiaryFixed | TertiaryFixedDim => fixed(TertiaryFixed, TertiaryFixedDim),
            _ => None,
        }
    }

    fn raw_tone_2021(&self, role: Role) -> f64 {
        use Role::*;
        let d = self.dark;
        let c = self.contrast;
        let mono = self.variant() == Variant::Monochrome;
        let fidelity = matches!(self.variant(), Variant::Fidelity | Variant::Content);
        let pick = |dark: f64, light: f64| if d { dark } else { light };
        let source_tone = tone_of(self.scheme.source_color_hct);
        match role {
            PrimaryPaletteKeyColor => tone_of(self.pal(Pal::Primary).key_color()),
            SecondaryPaletteKeyColor => tone_of(self.pal(Pal::Secondary).key_color()),
            TertiaryPaletteKeyColor => tone_of(self.pal(Pal::Tertiary).key_color()),
            NeutralPaletteKeyColor => tone_of(self.pal(Pal::Neutral).key_color()),
            NeutralVariantPaletteKeyColor => tone_of(self.pal(Pal::NeutralVariant).key_color()),
            ErrorPaletteKeyColor => tone_of(self.pal(Pal::Error).key_color()),
            Background | Surface => pick(6.0, 98.0),
            OnBackground | OnSurface => pick(90.0, 10.0),
            SurfaceDim => {
                if d {
                    6.0
                } else {
                    curve(87.0, 87.0, 80.0, 75.0).get(c)
                }
            }
            SurfaceBright => {
                if d {
                    curve(24.0, 24.0, 29.0, 34.0).get(c)
                } else {
                    98.0
                }
            }
            SurfaceContainerLowest => {
                if d {
                    curve(4.0, 4.0, 2.0, 0.0).get(c)
                } else {
                    100.0
                }
            }
            SurfaceContainerLow => {
                if d {
                    curve(10.0, 10.0, 11.0, 12.0).get(c)
                } else {
                    curve(96.0, 96.0, 96.0, 95.0).get(c)
                }
            }
            SurfaceContainer => {
                if d {
                    curve(12.0, 12.0, 16.0, 20.0).get(c)
                } else {
                    curve(94.0, 94.0, 92.0, 90.0).get(c)
                }
            }
            SurfaceContainerHigh => {
                if d {
                    curve(17.0, 17.0, 21.0, 25.0).get(c)
                } else {
                    curve(92.0, 92.0, 88.0, 85.0).get(c)
                }
            }
            SurfaceContainerHighest => {
                if d {
                    curve(22.0, 22.0, 26.0, 30.0).get(c)
                } else {
                    curve(90.0, 90.0, 84.0, 80.0).get(c)
                }
            }
            SurfaceVariant => pick(30.0, 90.0),
            OnSurfaceVariant => pick(80.0, 30.0),
            InverseSurface => pick(90.0, 20.0),
            InverseOnSurface => pick(20.0, 95.0),
            Outline => pick(60.0, 50.0),
            OutlineVariant => pick(30.0, 80.0),
            Shadow | Scrim => 0.0,
            SurfaceTint => pick(80.0, 40.0),
            Primary => {
                if mono {
                    pick(100.0, 0.0)
                } else {
                    pick(80.0, 40.0)
                }
            }
            OnPrimary => {
                if mono {
                    pick(10.0, 90.0)
                } else {
                    pick(20.0, 100.0)
                }
            }
            PrimaryContainer => {
                if fidelity {
                    source_tone
                } else if mono {
                    pick(85.0, 25.0)
                } else {
                    pick(30.0, 90.0)
                }
            }
            OnPrimaryContainer => {
                if fidelity {
                    foreground_tone(self.raw_tone(PrimaryContainer), 4.5)
                } else if mono {
                    pick(0.0, 100.0)
                } else {
                    pick(90.0, 30.0)
                }
            }
            InversePrimary => pick(40.0, 80.0),
            Secondary => pick(80.0, 40.0),
            OnSecondary => {
                if mono {
                    pick(10.0, 100.0)
                } else {
                    pick(20.0, 100.0)
                }
            }
            SecondaryContainer => {
                if mono {
                    pick(30.0, 85.0)
                } else if fidelity {
                    let p = self.pal(Pal::Secondary);
                    find_desired_chroma_by_tone(p.hue(), p.chroma(), pick(30.0, 90.0), !d)
                } else {
                    pick(30.0, 90.0)
                }
            }
            OnSecondaryContainer => {
                if mono {
                    pick(90.0, 10.0)
                } else if fidelity {
                    foreground_tone(self.raw_tone(SecondaryContainer), 4.5)
                } else {
                    pick(90.0, 30.0)
                }
            }
            Tertiary => {
                if mono {
                    pick(90.0, 25.0)
                } else {
                    pick(80.0, 40.0)
                }
            }
            OnTertiary => {
                if mono {
                    pick(10.0, 90.0)
                } else {
                    pick(20.0, 100.0)
                }
            }
            TertiaryContainer => {
                if mono {
                    pick(60.0, 49.0)
                } else if fidelity {
                    let hct: Hct = palette_tone(self.pal(Pal::Tertiary), source_tone).into();
                    tone_of(fix_if_disliked(hct))
                } else {
                    pick(30.0, 90.0)
                }
            }
            OnTertiaryContainer => {
                if mono {
                    pick(0.0, 100.0)
                } else if fidelity {
                    foreground_tone(self.raw_tone(TertiaryContainer), 4.5)
                } else {
                    pick(90.0, 30.0)
                }
            }
            Error => pick(80.0, 40.0),
            OnError => pick(20.0, 100.0),
            ErrorContainer => pick(30.0, 90.0),
            OnErrorContainer => {
                if mono {
                    pick(90.0, 10.0)
                } else {
                    pick(90.0, 30.0)
                }
            }
            PrimaryFixed | TertiaryFixed => {
                if mono {
                    40.0
                } else {
                    90.0
                }
            }
            PrimaryFixedDim | TertiaryFixedDim => {
                if mono {
                    30.0
                } else {
                    80.0
                }
            }
            OnPrimaryFixed | OnTertiaryFixed => {
                if mono {
                    100.0
                } else {
                    10.0
                }
            }
            OnPrimaryFixedVariant | OnTertiaryFixedVariant => {
                if mono {
                    90.0
                } else {
                    30.0
                }
            }
            SecondaryFixed => {
                if mono {
                    80.0
                } else {
                    90.0
                }
            }
            SecondaryFixedDim => {
                if mono {
                    70.0
                } else {
                    80.0
                }
            }
            OnSecondaryFixed => 10.0,
            OnSecondaryFixedVariant => {
                if mono {
                    25.0
                } else {
                    30.0
                }
            }
            // *Dim is not defined in 2021 — they do not end up here (def_spec)
            PrimaryDim | SecondaryDim | TertiaryDim | ErrorDim => self.raw_tone_2025(role),
        }
    }

    // --- spec 2025 (phone) ---

    fn background_2025(&self, role: Role) -> Option<Role> {
        use Role::*;
        Some(match role {
            OnSurface | OnSurfaceVariant | Outline | OutlineVariant | Primary
            | PrimaryContainer | PrimaryFixed | Secondary | SecondaryContainer | SecondaryFixed
            | Tertiary | TertiaryContainer | TertiaryFixed | Error | ErrorContainer => {
                self.highest_surface()
            }
            PrimaryDim | SecondaryDim | TertiaryDim | ErrorDim => SurfaceContainerHigh,
            InverseOnSurface | InversePrimary => InverseSurface,
            OnPrimary => Primary,
            OnPrimaryContainer => PrimaryContainer,
            OnPrimaryFixed | OnPrimaryFixedVariant => PrimaryFixedDim,
            OnSecondary => Secondary,
            OnSecondaryContainer => SecondaryContainer,
            OnSecondaryFixed | OnSecondaryFixedVariant => SecondaryFixedDim,
            OnTertiary => Tertiary,
            OnTertiaryContainer => TertiaryContainer,
            OnTertiaryFixed | OnTertiaryFixedVariant => TertiaryFixedDim,
            OnError => Error,
            OnErrorContainer => ErrorContainer,
            _ => return None,
        })
    }

    fn curve_2025(&self, role: Role) -> Option<Curve> {
        use Role::*;
        let containers = || (self.contrast > 0.0).then(|| std_curve(1.5));
        Some(match role {
            OnSurface => std_curve(if self.dark { 11.0 } else { 9.0 }),
            OnSurfaceVariant => std_curve(if self.dark { 6.0 } else { 4.5 }),
            Outline => std_curve(3.0),
            OutlineVariant => std_curve(1.5),
            InverseOnSurface | OnPrimaryFixed | OnSecondaryFixed | OnTertiaryFixed => {
                std_curve(7.0)
            }
            Primary
            | PrimaryDim
            | Secondary
            | SecondaryDim
            | Tertiary
            | TertiaryDim
            | Error
            | ErrorDim
            | OnPrimaryFixedVariant
            | OnSecondaryFixedVariant
            | OnTertiaryFixedVariant
            | OnErrorContainer => std_curve(4.5),
            OnPrimary | OnPrimaryContainer | InversePrimary | OnSecondary
            | OnSecondaryContainer | OnTertiary | OnTertiaryContainer | OnError => std_curve(6.0),
            PrimaryContainer | PrimaryFixed | SecondaryContainer | SecondaryFixed
            | TertiaryContainer | TertiaryFixed | ErrorContainer => return containers(),
            _ => return None,
        })
    }

    fn pair_2025(role: Role) -> Option<Pair> {
        use Role::*;
        let lighter = |a, b| {
            Some(pair(
                a,
                b,
                5.0,
                Polarity::RelativeLighter,
                true,
                Constraint::Farther,
            ))
        };
        let dim = |a, b| Some(pair(a, b, 5.0, Polarity::Darker, true, Constraint::Farther));
        let fixed = |a, b| Some(pair(a, b, 5.0, Polarity::Darker, true, Constraint::Exact));
        match role {
            Primary => lighter(PrimaryContainer, Primary),
            Secondary => lighter(SecondaryContainer, Secondary),
            Tertiary => lighter(TertiaryContainer, Tertiary),
            Error => lighter(ErrorContainer, Error),
            PrimaryDim => dim(PrimaryDim, Primary),
            SecondaryDim => dim(SecondaryDim, Secondary),
            TertiaryDim => dim(TertiaryDim, Tertiary),
            ErrorDim => dim(ErrorDim, Error),
            PrimaryFixedDim => fixed(PrimaryFixedDim, PrimaryFixed),
            SecondaryFixedDim => fixed(SecondaryFixedDim, SecondaryFixed),
            TertiaryFixedDim => fixed(TertiaryFixedDim, TertiaryFixed),
            _ => None,
        }
    }

    /// The role's tone in the light scheme with standard contrast (for *Fixed).
    fn light_standard_tone(&self, role: Role) -> f64 {
        if !self.dark && self.contrast == 0.0 {
            return self.tone(role);
        }
        Resolver::with_mode(self.scheme, false, 0.0).tone(role)
    }

    fn raw_tone_2025(&self, role: Role) -> f64 {
        use Role::*;
        let d = self.dark;
        let v = self.variant();
        let yellow = self.neutral_yellow();
        // surfaces: dark / yellow / vibrant / the rest
        let surface = |dark: f64, yellow_t: f64, vibrant: f64, other: f64| {
            if d {
                dark
            } else if yellow {
                yellow_t
            } else if v == Variant::Vibrant {
                vibrant
            } else {
                other
            }
        };
        let p_hue = self.pal(Pal::Primary).hue();
        let t_hue = self.pal(Pal::Tertiary).hue();
        match role {
            Surface | Background => surface(4.0, 99.0, 97.0, 98.0),
            SurfaceDim => surface(4.0, 90.0, 85.0, 87.0),
            SurfaceBright => surface(18.0, 99.0, 97.0, 98.0),
            SurfaceContainerLowest => {
                if d {
                    0.0
                } else {
                    100.0
                }
            }
            SurfaceContainerLow => surface(6.0, 98.0, 95.0, 96.0),
            SurfaceContainer => surface(9.0, 96.0, 92.0, 94.0),
            SurfaceContainerHigh => surface(12.0, 94.0, 90.0, 92.0),
            SurfaceContainerHighest | SurfaceVariant => surface(15.0, 92.0, 88.0, 90.0),
            OnSurface => {
                if v == Variant::Vibrant {
                    let p = self.pal(Pal::Neutral);
                    clamp(
                        0.0,
                        100.0,
                        find_best_tone_for_chroma(p.hue(), p.chroma() * 1.1, 100.0, true),
                    )
                } else {
                    self.tone_from_background(role)
                }
            }
            OnBackground => self.tone(OnSurface),
            InverseSurface => {
                if d {
                    98.0
                } else {
                    4.0
                }
            }
            Primary | SurfaceTint => match v {
                Variant::Neutral => {
                    if d {
                        80.0
                    } else {
                        40.0
                    }
                }
                Variant::TonalSpot => {
                    if d {
                        80.0
                    } else {
                        self.t_max_c(Pal::Primary, 0.0, 100.0)
                    }
                }
                Variant::Expressive => {
                    if Hct::is_yellow(p_hue) {
                        self.t_max_c(Pal::Primary, 0.0, 25.0)
                    } else if Hct::is_cyan(p_hue) {
                        self.t_max_c(Pal::Primary, 0.0, 88.0)
                    } else {
                        self.t_max_c(Pal::Primary, 0.0, 98.0)
                    }
                }
                Variant::Vibrant => {
                    if Hct::is_cyan(p_hue) {
                        self.t_max_c(Pal::Primary, 0.0, 88.0)
                    } else {
                        self.t_max_c(Pal::Primary, 0.0, 98.0)
                    }
                }
                _ => 0.0,
            },
            PrimaryDim => match v {
                Variant::Neutral => 85.0,
                Variant::TonalSpot => self.t_max_c(Pal::Primary, 0.0, 90.0),
                _ => self.t_max_c(Pal::Primary, 0.0, 100.0),
            },
            PrimaryContainer => match v {
                Variant::Neutral => {
                    if d {
                        30.0
                    } else {
                        90.0
                    }
                }
                Variant::TonalSpot => {
                    if d {
                        self.t_min_c(Pal::Primary, 35.0, 93.0)
                    } else {
                        self.t_max_c(Pal::Primary, 0.0, 90.0)
                    }
                }
                Variant::Expressive => {
                    if d {
                        self.t_max_c(Pal::Primary, 30.0, 93.0)
                    } else if Hct::is_cyan(p_hue) {
                        self.t_max_c(Pal::Primary, 78.0, 88.0)
                    } else {
                        self.t_max_c(Pal::Primary, 78.0, 90.0)
                    }
                }
                Variant::Vibrant => {
                    if d {
                        self.t_min_c(Pal::Primary, 66.0, 93.0)
                    } else if Hct::is_cyan(p_hue) {
                        self.t_max_c(Pal::Primary, 66.0, 88.0)
                    } else {
                        self.t_max_c(Pal::Primary, 66.0, 93.0)
                    }
                }
                _ => 0.0,
            },
            PrimaryFixed => self.light_standard_tone(PrimaryContainer),
            PrimaryFixedDim => self.tone(PrimaryFixed),
            InversePrimary => self.t_max_c(Pal::Primary, 0.0, 100.0),
            Secondary => match v {
                Variant::Neutral => {
                    if d {
                        self.t_min_c(Pal::Secondary, 0.0, 98.0)
                    } else {
                        self.t_max_c(Pal::Secondary, 0.0, 100.0)
                    }
                }
                Variant::Vibrant => {
                    if d {
                        self.t_max_c(Pal::Secondary, 0.0, 90.0)
                    } else {
                        self.t_max_c(Pal::Secondary, 0.0, 98.0)
                    }
                }
                _ => {
                    if d {
                        80.0
                    } else {
                        self.t_max_c(Pal::Secondary, 0.0, 100.0)
                    }
                }
            },
            SecondaryDim => {
                if v == Variant::Neutral {
                    85.0
                } else {
                    self.t_max_c(Pal::Secondary, 0.0, 90.0)
                }
            }
            SecondaryContainer => match v {
                Variant::Vibrant => {
                    if d {
                        self.t_min_c(Pal::Secondary, 30.0, 40.0)
                    } else {
                        self.t_max_c(Pal::Secondary, 84.0, 90.0)
                    }
                }
                Variant::Expressive => {
                    if d {
                        15.0
                    } else {
                        self.t_max_c(Pal::Secondary, 90.0, 95.0)
                    }
                }
                _ => {
                    if d {
                        25.0
                    } else {
                        90.0
                    }
                }
            },
            SecondaryFixed => self.light_standard_tone(SecondaryContainer),
            SecondaryFixedDim => self.tone(SecondaryFixed),
            Tertiary => match v {
                Variant::Expressive | Variant::Vibrant => {
                    if Hct::is_cyan(t_hue) {
                        self.t_max_c(Pal::Tertiary, 0.0, 88.0)
                    } else if d {
                        self.t_max_c(Pal::Tertiary, 0.0, 98.0)
                    } else {
                        self.t_max_c(Pal::Tertiary, 0.0, 100.0)
                    }
                }
                _ => {
                    if d {
                        self.t_max_c(Pal::Tertiary, 0.0, 98.0)
                    } else {
                        self.t_max_c(Pal::Tertiary, 0.0, 100.0)
                    }
                }
            },
            TertiaryDim => {
                if v == Variant::TonalSpot {
                    self.t_max_c(Pal::Tertiary, 0.0, 90.0)
                } else {
                    self.t_max_c(Pal::Tertiary, 0.0, 100.0)
                }
            }
            TertiaryContainer => match v {
                Variant::Neutral => {
                    if d {
                        self.t_max_c(Pal::Tertiary, 0.0, 93.0)
                    } else {
                        self.t_max_c(Pal::Tertiary, 0.0, 96.0)
                    }
                }
                Variant::TonalSpot => {
                    if d {
                        self.t_max_c(Pal::Tertiary, 0.0, 93.0)
                    } else {
                        self.t_max_c(Pal::Tertiary, 0.0, 100.0)
                    }
                }
                Variant::Expressive => {
                    if Hct::is_cyan(t_hue) {
                        self.t_max_c(Pal::Tertiary, 75.0, 88.0)
                    } else if d {
                        self.t_max_c(Pal::Tertiary, 75.0, 93.0)
                    } else {
                        self.t_max_c(Pal::Tertiary, 75.0, 100.0)
                    }
                }
                Variant::Vibrant => {
                    if d {
                        self.t_max_c(Pal::Tertiary, 0.0, 93.0)
                    } else {
                        self.t_max_c(Pal::Tertiary, 72.0, 100.0)
                    }
                }
                _ => 0.0,
            },
            TertiaryFixed => self.light_standard_tone(TertiaryContainer),
            TertiaryFixedDim => self.tone(TertiaryFixed),
            Error => {
                if d {
                    self.t_min_c(Pal::Error, 0.0, 98.0)
                } else {
                    self.t_max_c(Pal::Error, 0.0, 100.0)
                }
            }
            ErrorDim => self.t_min_c(Pal::Error, 0.0, 100.0),
            ErrorContainer => {
                if d {
                    self.t_min_c(Pal::Error, 30.0, 93.0)
                } else {
                    self.t_max_c(Pal::Error, 0.0, 90.0)
                }
            }
            // the key colors, shadow, scrim are not overridden in 2025 — they do not end up here (def_spec)
            PrimaryPaletteKeyColor
            | SecondaryPaletteKeyColor
            | TertiaryPaletteKeyColor
            | NeutralPaletteKeyColor
            | NeutralVariantPaletteKeyColor
            | ErrorPaletteKeyColor
            | Shadow
            | Scrim => self.raw_tone_2021(role),
            // on*, outline* — from the background
            _ => self.tone_from_background(role),
        }
    }

    // --- tone calculation ---

    /// ColorCalculationDelegateImpl2021.get_tone.
    fn calc_tone_2021(&self, role: Role) -> f64 {
        let level = self.contrast;
        let decreasing = level < 0.0;
        if let Some(p) = self.tone_delta_pair(role) {
            let a_is_nearer = p.polarity == Polarity::Nearer
                || (p.polarity == Polarity::Lighter && !self.dark)
                || (p.polarity == Polarity::Darker && self.dark);
            let (nearer, farther) = if a_is_nearer { (p.a, p.b) } else { (p.b, p.a) };
            let am_nearer = role == nearer;
            let expansion = if self.dark { 1.0 } else { -1.0 };
            let mut n_tone = self.raw_tone(nearer);
            let mut f_tone = self.raw_tone(farther);

            if let (Some(bg), Some(n_curve), Some(f_curve)) = (
                self.background(role),
                self.contrast_curve(nearer),
                self.contrast_curve(farther),
            ) {
                let bg_tone = self.tone(bg);
                let n_contrast = n_curve.get(level);
                let f_contrast = f_curve.get(level);
                if ratio_of_tones(bg_tone, n_tone) < n_contrast {
                    n_tone = foreground_tone(bg_tone, n_contrast);
                }
                if ratio_of_tones(bg_tone, f_tone) < f_contrast {
                    f_tone = foreground_tone(bg_tone, f_contrast);
                }
                if decreasing {
                    n_tone = foreground_tone(bg_tone, n_contrast);
                    f_tone = foreground_tone(bg_tone, f_contrast);
                }
            }

            if (f_tone - n_tone) * expansion < p.delta {
                f_tone = clamp(0.0, 100.0, n_tone + p.delta * expansion);
                if (f_tone - n_tone) * expansion < p.delta {
                    n_tone = clamp(0.0, 100.0, f_tone - p.delta * expansion);
                }
            }

            let awkward = |t: f64| (50.0..60.0).contains(&t);
            let move_both = |n: &mut f64, f: &mut f64| {
                if expansion > 0.0 {
                    *n = 60.0;
                    *f = f.max(*n + p.delta * expansion);
                } else {
                    *n = 49.0;
                    *f = f.min(*n + p.delta * expansion);
                }
            };
            if awkward(n_tone) {
                move_both(&mut n_tone, &mut f_tone);
            } else if awkward(f_tone) {
                if p.stay_together {
                    move_both(&mut n_tone, &mut f_tone);
                } else {
                    f_tone = if expansion > 0.0 { 60.0 } else { 49.0 };
                }
            }
            return if am_nearer { n_tone } else { f_tone };
        }

        let mut answer = self.raw_tone(role);
        let (Some(bg), Some(c)) = (self.background(role), self.contrast_curve(role)) else {
            return answer;
        };
        let bg_tone = self.tone(bg);
        let desired = c.get(level);
        if ratio_of_tones(bg_tone, answer) < desired {
            answer = foreground_tone(bg_tone, desired);
        }
        if decreasing {
            answer = foreground_tone(bg_tone, desired);
        }
        if is_background(role) && (50.0..60.0).contains(&answer) {
            answer = if ratio_of_tones(49.0, bg_tone) >= desired {
                49.0
            } else {
                60.0
            };
        }
        self.dual_background(role, answer, desired)
    }

    /// ColorCalculationDelegateImpl2025.get_tone.
    fn calc_tone_2025(&self, role: Role) -> f64 {
        let level = self.contrast;
        let clamp_background = |tone: f64| {
            if is_background(role) && !is_fixed_dim(role) {
                if tone >= 57.0 {
                    clamp(65.0, 100.0, tone)
                } else {
                    clamp(0.0, 49.0, tone)
                }
            } else {
                tone
            }
        };
        if let Some(p) = self.tone_delta_pair(role) {
            let negative = p.polarity == Polarity::Darker
                || (p.polarity == Polarity::RelativeLighter && self.dark);
            let absolute_delta = if negative { -p.delta } else { p.delta };
            let am_a = role == p.a;
            let (self_role, ref_role) = if am_a { (p.a, p.b) } else { (p.b, p.a) };
            let mut self_tone = self.raw_tone(self_role);
            let ref_tone = self.tone(ref_role);
            let relative = absolute_delta * if am_a { 1.0 } else { -1.0 };
            self_tone = match p.constraint {
                Constraint::Exact => clamp(0.0, 100.0, ref_tone + relative),
                Constraint::Farther => {
                    if relative > 0.0 {
                        clamp(ref_tone + relative, 100.0, self_tone)
                    } else {
                        clamp(0.0, ref_tone + relative, self_tone)
                    }
                }
            };
            if let (Some(bg), Some(c)) = (self.background(role), self.contrast_curve(role)) {
                let bg_tone = self.tone(bg);
                let contrast = c.get(level);
                if !(ratio_of_tones(bg_tone, self_tone) >= contrast && level >= 0.0) {
                    self_tone = foreground_tone(bg_tone, contrast);
                }
            }
            return clamp_background(self_tone);
        }

        let mut answer = self.raw_tone(role);
        let (Some(bg), Some(c)) = (self.background(role), self.contrast_curve(role)) else {
            return answer;
        };
        let bg_tone = self.tone(bg);
        let desired = c.get(level);
        if !(ratio_of_tones(bg_tone, answer) >= desired && level >= 0.0) {
            answer = foreground_tone(bg_tone, desired);
        }
        answer = clamp_background(answer);
        self.dual_background(role, answer, desired)
    }

    /// The shared tail of both calculations: the second background (onXFixed in 2021).
    fn dual_background(&self, role: Role, answer: f64, desired: f64) -> f64 {
        let (Some(bg1), Some(bg2)) = (self.background(role), self.second_background(role)) else {
            return answer;
        };
        let t1 = self.tone(bg1);
        let t2 = self.tone(bg2);
        let upper = t1.max(t2);
        let lower = t1.min(t2);
        if ratio_of_tones(upper, answer) >= desired && ratio_of_tones(lower, answer) >= desired {
            return answer;
        }
        let light = lighter(upper, desired);
        let dark = darker(lower, desired);
        let available = usize::from(light != -1.0) + usize::from(dark != -1.0);
        if prefers_light_foreground(t1) || prefers_light_foreground(t2) {
            return if light < 0.0 { 100.0 } else { light };
        }
        if available == 1 {
            return if light != -1.0 { light } else { dark };
        }
        if dark < 0.0 { 0.0 } else { dark }
    }
}
