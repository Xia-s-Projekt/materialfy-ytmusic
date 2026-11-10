//! QuantizeCelebi from materialyoucolor (the C++ extension celebi.pyd = material-color-utilities/cpp:
//! quantize/celebi.cc, wu.cc, wsmeans.cc, lab.cc). Our own port rather than material_colors::quantize: there the
//! XYZ→sRGB matrix and the random number generator differ, and the clusters come out ±1 per channel differently.
//!
//! WSMeans lays points out across clusters via srand(42688)/rand() from the CRT, which is why Python itself gives different
//! results on different OSes (on the test images, for half of them); here the rand() of the required C library is reproduced,
//! by default that of the build platform (CrtRand::native). Verified against Python on Windows (MSVC).
use std::collections::{BTreeMap, HashMap};

// --- Lab (quantize/lab.cc) -------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Lab {
    l: f64,
    a: f64,
    b: f64,
}

impl Lab {
    fn delta_e(self, other: Lab) -> f64 {
        let (dl, da, db) = (self.l - other.l, self.a - other.a, self.b - other.b);
        dl * dl + da * da + db * db
    }
}

const WHITE_POINT_D65: [f64; 3] = [95.047, 100.0, 108.883];
const E: f64 = 216.0 / 24389.0;
const KAPPA: f64 = 24389.0 / 27.0;

fn linearized(component: u32) -> f64 {
    let normalized = f64::from(component) / 255.0;
    if normalized <= 0.040449936 {
        normalized / 12.92 * 100.0
    } else {
        ((normalized + 0.055) / 1.055).powf(2.4) * 100.0
    }
}

fn delinearized(component: f64) -> u32 {
    let normalized = component / 100.0;
    let v = if normalized <= 0.0031308 {
        normalized * 12.92
    } else {
        1.055 * normalized.powf(1.0 / 2.4) - 0.055
    };
    ((v * 255.0).round() as i64).clamp(0, 255) as u32
}

fn lab_f(t: f64) -> f64 {
    if t > E {
        t.powf(1.0 / 3.0)
    } else {
        (KAPPA * t + 16.0) / 116.0
    }
}

fn lab_from_argb(argb: u32) -> Lab {
    let r = linearized((argb >> 16) & 0xff);
    let g = linearized((argb >> 8) & 0xff);
    let b = linearized(argb & 0xff);
    let x = 0.41233895 * r + 0.35762064 * g + 0.18051042 * b;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = 0.01932141 * r + 0.11916382 * g + 0.95034478 * b;
    let fx = lab_f(x / WHITE_POINT_D65[0]);
    let fy = lab_f(y / WHITE_POINT_D65[1]);
    let fz = lab_f(z / WHITE_POINT_D65[2]);
    Lab {
        l: 116.0 * fy - 16.0,
        a: 500.0 * (fx - fy),
        b: 200.0 * (fy - fz),
    }
}

fn argb_from_lab(lab: Lab) -> u32 {
    let fy = (lab.l + 16.0) / 116.0;
    let fx = lab.a / 500.0 + fy;
    let fz = fy - lab.b / 200.0;
    let fx3 = fx * fx * fx;
    let xn = if fx3 > E {
        fx3
    } else {
        (116.0 * fx - 16.0) / KAPPA
    };
    let yn = if lab.l > 8.0 {
        fy * fy * fy
    } else {
        lab.l / KAPPA
    };
    let fz3 = fz * fz * fz;
    let zn = if fz3 > E {
        fz3
    } else {
        (116.0 * fz - 16.0) / KAPPA
    };
    let x = xn * WHITE_POINT_D65[0];
    let y = yn * WHITE_POINT_D65[1];
    let z = zn * WHITE_POINT_D65[2];
    let r = delinearized(3.2406 * x - 1.5372 * y - 0.4986 * z);
    let g = delinearized(-0.9689 * x + 1.8758 * y + 0.0415 * z);
    let b = delinearized(0.0557 * x - 0.2040 * y + 1.0570 * z);
    argb_from_rgb(r, g, b)
}

fn argb_from_rgb(r: u32, g: u32, b: u32) -> u32 {
    0xFF00_0000 | (r & 0xff) << 16 | (g & 0xff) << 8 | (b & 0xff)
}

// --- rand() from the CRT -----------------------------------------------------------------------------------

/// Whose srand/rand to reproduce: the C library the Python module on this OS was built with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrtRand {
    /// MSVC (Windows): holdrand = holdrand * 214013 + 2531011; (holdrand >> 16) & 0x7fff.
    Msvc,
    /// glibc (Linux): TYPE_3, r[i] = r[i-31] + r[i-3], the first 310 values are discarded.
    Glibc,
    /// BSD libc (macOS): Park–Miller "minimal standard".
    Bsd,
}

impl CrtRand {
    /// The generator of the platform the program was built for.
    pub const fn native() -> Self {
        if cfg!(any(target_os = "linux", target_os = "android")) {
            Self::Glibc
        } else if cfg!(any(target_os = "macos", target_os = "ios")) {
            Self::Bsd
        } else {
            Self::Msvc
        }
    }
}

enum Rng {
    Msvc(u32),
    Glibc(Vec<i32>),
    Bsd(u64),
}

impl Rng {
    fn new(kind: CrtRand, seed: u32) -> Self {
        match kind {
            CrtRand::Msvc => Self::Msvc(seed),
            CrtRand::Bsd => Self::Bsd(u64::from(seed)),
            CrtRand::Glibc => {
                let mut r = vec![0i32; 34];
                r[0] = if seed == 0 { 1 } else { seed as i32 };
                for i in 1..31 {
                    let prev = i64::from(r[i - 1]);
                    let mut word = 16807 * (prev % 127773) - 2836 * (prev / 127773);
                    if word < 0 {
                        word += 2147483647;
                    }
                    r[i] = word as i32;
                }
                for i in 31..34 {
                    r[i] = r[i - 31];
                }
                let mut rng = Self::Glibc(r);
                for _ in 34..344 {
                    rng.next();
                }
                rng
            }
        }
    }

    fn next(&mut self) -> u32 {
        match self {
            Self::Msvc(state) => {
                *state = state.wrapping_mul(214013).wrapping_add(2531011);
                (*state >> 16) & 0x7fff
            }
            Self::Glibc(r) => {
                let n = r.len();
                let v = r[n - 31].wrapping_add(r[n - 3]);
                r.push(v);
                if r.len() > 4096 {
                    r.drain(..r.len() - 31);
                }
                (v as u32) >> 1
            }
            Self::Bsd(next) => {
                // srand(1) → 16807, 282475249, … (this is how rand() behaves in macOS; not verified against Python on macOS)
                if *next == 0 {
                    *next = 123459876;
                }
                let x = *next as i64;
                let (hi, lo) = (x / 127773, x % 127773);
                let mut x = 16807 * lo - 2836 * hi;
                if x < 0 {
                    x += 0x7fff_ffff;
                }
                *next = x as u64;
                x as u32
            }
        }
    }
}

// --- Wu (quantize/wu.cc) -----------------------------------------------------------------------------

const INDEX_BITS: usize = 5;
const INDEX_COUNT: usize = (1 << INDEX_BITS) + 1;
const TOTAL_SIZE: usize = INDEX_COUNT * INDEX_COUNT * INDEX_COUNT;
const MAX_COLORS: usize = 256;

fn index(r: usize, g: usize, b: usize) -> usize {
    (r << (INDEX_BITS * 2)) + (r << (INDEX_BITS + 1)) + (g << INDEX_BITS) + r + g + b
}

#[derive(Clone, Copy, Default)]
struct Cube {
    r0: usize,
    r1: usize,
    g0: usize,
    g1: usize,
    b0: usize,
    b1: usize,
    vol: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Red,
    Green,
    Blue,
}

struct Moments {
    weights: Vec<i64>,
    m_r: Vec<i64>,
    m_g: Vec<i64>,
    m_b: Vec<i64>,
    moments: Vec<f64>,
}

fn vol(c: &Cube, m: &[i64]) -> i64 {
    m[index(c.r1, c.g1, c.b1)] - m[index(c.r1, c.g1, c.b0)] - m[index(c.r1, c.g0, c.b1)]
        + m[index(c.r1, c.g0, c.b0)]
        - m[index(c.r0, c.g1, c.b1)]
        + m[index(c.r0, c.g1, c.b0)]
        + m[index(c.r0, c.g0, c.b1)]
        - m[index(c.r0, c.g0, c.b0)]
}

fn bottom(c: &Cube, d: Direction, m: &[i64]) -> i64 {
    match d {
        Direction::Red => {
            -m[index(c.r0, c.g1, c.b1)] + m[index(c.r0, c.g1, c.b0)] + m[index(c.r0, c.g0, c.b1)]
                - m[index(c.r0, c.g0, c.b0)]
        }
        Direction::Green => {
            -m[index(c.r1, c.g0, c.b1)] + m[index(c.r1, c.g0, c.b0)] + m[index(c.r0, c.g0, c.b1)]
                - m[index(c.r0, c.g0, c.b0)]
        }
        Direction::Blue => {
            -m[index(c.r1, c.g1, c.b0)] + m[index(c.r1, c.g0, c.b0)] + m[index(c.r0, c.g1, c.b0)]
                - m[index(c.r0, c.g0, c.b0)]
        }
    }
}

fn top(c: &Cube, d: Direction, p: usize, m: &[i64]) -> i64 {
    match d {
        Direction::Red => {
            m[index(p, c.g1, c.b1)] - m[index(p, c.g1, c.b0)] - m[index(p, c.g0, c.b1)]
                + m[index(p, c.g0, c.b0)]
        }
        Direction::Green => {
            m[index(c.r1, p, c.b1)] - m[index(c.r1, p, c.b0)] - m[index(c.r0, p, c.b1)]
                + m[index(c.r0, p, c.b0)]
        }
        Direction::Blue => {
            m[index(c.r1, c.g1, p)] - m[index(c.r1, c.g0, p)] - m[index(c.r0, c.g1, p)]
                + m[index(c.r0, c.g0, p)]
        }
    }
}

impl Moments {
    fn new(pixels: &[u32]) -> Self {
        let mut m = Self {
            weights: vec![0; TOTAL_SIZE],
            m_r: vec![0; TOTAL_SIZE],
            m_g: vec![0; TOTAL_SIZE],
            m_b: vec![0; TOTAL_SIZE],
            moments: vec![0.0; TOTAL_SIZE],
        };
        let shift = 8 - INDEX_BITS;
        for &p in pixels {
            let (r, g, b) = ((p >> 16) & 0xff, (p >> 8) & 0xff, p & 0xff);
            let i = index(
                (r >> shift) as usize + 1,
                (g >> shift) as usize + 1,
                (b >> shift) as usize + 1,
            );
            m.weights[i] += 1;
            m.m_r[i] += i64::from(r);
            m.m_g[i] += i64::from(g);
            m.m_b[i] += i64::from(b);
            m.moments[i] += f64::from(r * r + g * g + b * b);
        }
        m.compute();
        m
    }

    fn compute(&mut self) {
        for r in 1..INDEX_COUNT {
            let mut area = [0i64; INDEX_COUNT];
            let mut area_r = [0i64; INDEX_COUNT];
            let mut area_g = [0i64; INDEX_COUNT];
            let mut area_b = [0i64; INDEX_COUNT];
            let mut area_2 = [0f64; INDEX_COUNT];
            for g in 1..INDEX_COUNT {
                let (mut line, mut line_r, mut line_g, mut line_b, mut line_2) =
                    (0i64, 0i64, 0i64, 0i64, 0f64);
                for b in 1..INDEX_COUNT {
                    let i = index(r, g, b);
                    line += self.weights[i];
                    line_r += self.m_r[i];
                    line_g += self.m_g[i];
                    line_b += self.m_b[i];
                    line_2 += self.moments[i];
                    area[b] += line;
                    area_r[b] += line_r;
                    area_g[b] += line_g;
                    area_b[b] += line_b;
                    area_2[b] += line_2;
                    let prev = index(r - 1, g, b);
                    self.weights[i] = self.weights[prev] + area[b];
                    self.m_r[i] = self.m_r[prev] + area_r[b];
                    self.m_g[i] = self.m_g[prev] + area_g[b];
                    self.m_b[i] = self.m_b[prev] + area_b[b];
                    self.moments[i] = self.moments[prev] + area_2[b];
                }
            }
        }
    }

    fn variance(&self, c: &Cube) -> f64 {
        let dr = vol(c, &self.m_r) as f64;
        let dg = vol(c, &self.m_g) as f64;
        let db = vol(c, &self.m_b) as f64;
        let m = &self.moments;
        let xx =
            m[index(c.r1, c.g1, c.b1)] - m[index(c.r1, c.g1, c.b0)] - m[index(c.r1, c.g0, c.b1)]
                + m[index(c.r1, c.g0, c.b0)]
                - m[index(c.r0, c.g1, c.b1)]
                + m[index(c.r0, c.g1, c.b0)]
                + m[index(c.r0, c.g0, c.b1)]
                - m[index(c.r0, c.g0, c.b0)];
        let hypotenuse = dr * dr + dg * dg + db * db;
        let volume = vol(c, &self.weights) as f64;
        xx - hypotenuse / volume
    }

    #[allow(clippy::too_many_arguments)]
    fn maximize(
        &self,
        c: &Cube,
        d: Direction,
        first: usize,
        last: usize,
        whole: [i64; 4],
    ) -> (f64, i64) {
        let [whole_w, whole_r, whole_g, whole_b] = whole;
        let bottom_r = bottom(c, d, &self.m_r);
        let bottom_g = bottom(c, d, &self.m_g);
        let bottom_b = bottom(c, d, &self.m_b);
        let bottom_w = bottom(c, d, &self.weights);
        let mut max = 0.0;
        let mut cut = -1i64;
        for i in first..last {
            let mut half_r = bottom_r + top(c, d, i, &self.m_r);
            let mut half_g = bottom_g + top(c, d, i, &self.m_g);
            let mut half_b = bottom_b + top(c, d, i, &self.m_b);
            let mut half_w = bottom_w + top(c, d, i, &self.weights);
            if half_w == 0 {
                continue;
            }
            let sq = |r: i64, g: i64, b: i64, w: i64| {
                ((r as f64) * (r as f64) + (g as f64) * (g as f64) + (b as f64) * (b as f64))
                    / (w as f64)
            };
            let mut temp = sq(half_r, half_g, half_b, half_w);
            half_r = whole_r - half_r;
            half_g = whole_g - half_g;
            half_b = whole_b - half_b;
            half_w = whole_w - half_w;
            if half_w == 0 {
                continue;
            }
            temp += sq(half_r, half_g, half_b, half_w);
            if temp > max {
                max = temp;
                cut = i as i64;
            }
        }
        (max, cut)
    }

    fn cut(&self, one: &mut Cube, two: &mut Cube) -> bool {
        let whole = [
            vol(one, &self.weights),
            vol(one, &self.m_r),
            vol(one, &self.m_g),
            vol(one, &self.m_b),
        ];
        let (max_r, cut_r) = self.maximize(one, Direction::Red, one.r0 + 1, one.r1, whole);
        let (max_g, cut_g) = self.maximize(one, Direction::Green, one.g0 + 1, one.g1, whole);
        let (max_b, cut_b) = self.maximize(one, Direction::Blue, one.b0 + 1, one.b1, whole);
        let direction = if max_r >= max_g && max_r >= max_b {
            if cut_r < 0 {
                return false;
            }
            Direction::Red
        } else if max_g >= max_r && max_g >= max_b {
            Direction::Green
        } else {
            Direction::Blue
        };
        two.r1 = one.r1;
        two.g1 = one.g1;
        two.b1 = one.b1;
        match direction {
            Direction::Red => {
                one.r1 = cut_r as usize;
                two.r0 = one.r1;
                two.g0 = one.g0;
                two.b0 = one.b0;
            }
            Direction::Green => {
                one.g1 = cut_g as usize;
                two.r0 = one.r0;
                two.g0 = one.g1;
                two.b0 = one.b0;
            }
            Direction::Blue => {
                one.b1 = cut_b as usize;
                two.r0 = one.r0;
                two.g0 = one.g0;
                two.b0 = one.b1;
            }
        }
        one.vol = (one.r1 - one.r0) * (one.g1 - one.g0) * (one.b1 - one.b0);
        two.vol = (two.r1 - two.r0) * (two.g1 - two.g0) * (two.b1 - two.b0);
        true
    }
}

fn quantize_wu(pixels: &[u32], max_colors: usize) -> Vec<u32> {
    if max_colors == 0 || max_colors > MAX_COLORS || pixels.is_empty() {
        return Vec::new();
    }
    let m = Moments::new(pixels);
    let mut cubes = vec![Cube::default(); MAX_COLORS];
    cubes[0].r1 = INDEX_COUNT - 1;
    cubes[0].g1 = INDEX_COUNT - 1;
    cubes[0].b1 = INDEX_COUNT - 1;
    let mut volume_variance = vec![0.0f64; MAX_COLORS];
    let mut max_colors = max_colors;
    let mut next = 0usize;
    let mut i = 1usize;
    while i < max_colors {
        let (mut one, mut two) = (cubes[next], cubes[i]);
        if m.cut(&mut one, &mut two) {
            cubes[next] = one;
            cubes[i] = two;
            volume_variance[next] = if one.vol > 1 { m.variance(&one) } else { 0.0 };
            volume_variance[i] = if two.vol > 1 { m.variance(&two) } else { 0.0 };
        } else {
            cubes[next] = one;
            volume_variance[next] = 0.0;
            // in C++ there is an i-- here, then the loop's ++i — the same iteration repeats
            i -= 1;
        }
        next = 0;
        let mut temp = volume_variance[0];
        for (j, &v) in volume_variance.iter().enumerate().take(i + 1).skip(1) {
            if v > temp {
                temp = v;
                next = j;
            }
        }
        if temp <= 0.0 {
            max_colors = i + 1;
            break;
        }
        i += 1;
    }
    cubes
        .iter()
        .take(max_colors)
        .filter_map(|c| {
            let weight = vol(c, &m.weights);
            (weight > 0).then(|| {
                let r = (vol(c, &m.m_r) / weight) as u32;
                let g = (vol(c, &m.m_g) / weight) as u32;
                let b = (vol(c, &m.m_b) / weight) as u32;
                argb_from_rgb(r, g, b)
            })
        })
        .collect()
}

// --- WSMeans (quantize/wsmeans.cc) -------------------------------------------------------------------

const MAX_ITERATIONS: usize = 100;
const MIN_DELTA_E: f64 = 3.0;

fn quantize_wsmeans(
    input: &[u32],
    starting_clusters: &[u32],
    max_colors: usize,
    crt: CrtRand,
) -> BTreeMap<u32, u32> {
    if max_colors == 0 || input.is_empty() {
        return BTreeMap::new();
    }
    let max_colors = max_colors.min(256);

    let mut pixel_to_count: HashMap<u32, u32> = HashMap::new();
    let mut pixels = Vec::new();
    let mut points = Vec::new();
    for &p in input {
        let count = pixel_to_count.entry(p).or_insert(0);
        if *count == 0 {
            pixels.push(p);
            points.push(lab_from_argb(p));
        }
        *count += 1;
    }

    let mut cluster_count = max_colors.min(points.len());
    if !starting_clusters.is_empty() {
        cluster_count = cluster_count.min(starting_clusters.len());
    }
    let mut clusters: Vec<Lab> = starting_clusters
        .iter()
        .map(|&c| lab_from_argb(c))
        .collect();

    let mut rand = Rng::new(crt, 42688);
    if starting_clusters.is_empty() {
        for _ in clusters.len()..cluster_count {
            let i = rand.next() as usize % points.len();
            clusters.push(points[i]);
        }
    }

    let mut rand = Rng::new(crt, 42688);
    let mut cluster_indices: Vec<usize> = (0..points.len())
        .map(|_| rand.next() as usize % cluster_count)
        .collect();

    let mut distances = vec![vec![0.0f64; cluster_count]; cluster_count];
    let mut pixel_count_sums = [0u32; 256];

    for iteration in 0..MAX_ITERATIONS {
        for i in 0..cluster_count {
            distances[i][i] = 0.0;
            for j in i + 1..cluster_count {
                let d = clusters[i].delta_e(clusters[j]);
                distances[j][i] = d;
                distances[i][j] = d;
            }
        }

        let mut color_moved = false;
        for (i, &point) in points.iter().enumerate() {
            let previous = cluster_indices[i];
            let previous_distance = point.delta_e(clusters[previous]);
            let mut minimum = previous_distance;
            let mut new_index: Option<usize> = None;
            for j in 0..cluster_count {
                if distances[previous][j] >= 4.0 * previous_distance {
                    continue;
                }
                let d = point.delta_e(clusters[j]);
                if d < minimum {
                    minimum = d;
                    new_index = Some(j);
                }
            }
            if let Some(j) = new_index
                && (minimum.sqrt() - previous_distance.sqrt()).abs() > MIN_DELTA_E
            {
                color_moved = true;
                cluster_indices[i] = j;
            }
        }

        if !color_moved && iteration != 0 {
            break;
        }

        let mut sums = [[0.0f64; 3]; 256];
        pixel_count_sums[..cluster_count].fill(0);
        for (i, point) in points.iter().enumerate() {
            let k = cluster_indices[i];
            let count = pixel_to_count[&pixels[i]];
            pixel_count_sums[k] += count;
            let c = f64::from(count);
            sums[k][0] += point.l * c;
            sums[k][1] += point.a * c;
            sums[k][2] += point.b * c;
        }
        for i in 0..cluster_count {
            let count = pixel_count_sums[i];
            clusters[i] = if count == 0 {
                Lab::default()
            } else {
                let c = f64::from(count);
                Lab {
                    l: sums[i][0] / c,
                    a: sums[i][1] / c,
                    b: sums[i][2] / c,
                }
            };
        }
    }

    // clusters that are identical after rounding merge; the result is a std::map (ascending ARGB)
    let mut swatches: Vec<(u32, u32)> = Vec::new();
    for i in 0..cluster_count {
        let count = pixel_count_sums[i];
        if count == 0 {
            continue;
        }
        let argb = argb_from_lab(clusters[i]);
        match swatches.iter_mut().find(|s| s.0 == argb) {
            Some(s) => s.1 += count,
            None => swatches.push((argb, count)),
        }
    }
    swatches.into_iter().collect()
}

/// QuantizeCelebi(pixels, max_colors): ARGB → population, in ascending ARGB order (like a dict from std::map).
pub fn quantize_celebi(pixels: &[u32], max_colors: usize, crt: CrtRand) -> Vec<(u32, u32)> {
    let max_colors = max_colors.min(256);
    let opaque: Vec<u32> = pixels.iter().copied().filter(|p| p >> 24 == 0xff).collect();
    let wu = quantize_wu(&opaque, max_colors);
    quantize_wsmeans(&opaque, &wu, max_colors, crt)
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lab_roundtrip() {
        for argb in [
            0xFF000000u32,
            0xFFFFFFFF,
            0xFF6750A4,
            0xFFFFCC00,
            0xFF1DB954,
        ] {
            assert_eq!(argb_from_lab(lab_from_argb(argb)), argb);
        }
    }

    #[test]
    fn simple_clusters() {
        let red = 0xFFFF0000;
        let green = 0xFF00FF00;
        let result = quantize_celebi(&[red, red, green, green, green], 128, CrtRand::native());
        assert_eq!(result, vec![(green, 3), (red, 2)]);
        assert!(quantize_celebi(&[], 128, CrtRand::native()).is_empty());
        // semi-transparent ones are discarded
        assert!(quantize_celebi(&[0x80FF0000], 128, CrtRand::native()).is_empty());
    }

    #[test]
    fn rand_sequences() {
        let first = |kind| {
            let mut r = Rng::new(kind, 1);
            [r.next(), r.next(), r.next()]
        };
        assert_eq!(first(CrtRand::Msvc), [41, 18467, 6334]);
        assert_eq!(first(CrtRand::Glibc), [1804289383, 846930886, 1681692777]);
        assert_eq!(first(CrtRand::Bsd), [16807, 282475249, 1622650073]);
    }
}
