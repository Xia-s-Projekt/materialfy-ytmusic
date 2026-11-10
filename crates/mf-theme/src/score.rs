//! Score.score from materialyoucolor (score/score.py) — ranking of the quantizer's colors.
//! Our own copy rather than material_colors::score: in Python the population sum starts at 1e-5,
//! the hue is rounded with banker's rounding (round()), and the order for equal scores is that of a stable sort.
use material_colors::color::Rgb;
use material_colors::hct::Hct;

const TARGET_CHROMA: f64 = 48.0;
const WEIGHT_PROPORTION: f64 = 0.7;
const WEIGHT_CHROMA_ABOVE: f64 = 0.3;
const WEIGHT_CHROMA_BELOW: f64 = 0.1;
const CUTOFF_CHROMA: f64 = 5.0;
const CUTOFF_EXCITED_PROPORTION: f64 = 0.01;

pub const DESIRED: usize = 4;
pub const FALLBACK: u32 = 0xFF4285F4;

fn sanitize_degrees_int(degrees: i64) -> usize {
    degrees.rem_euclid(360) as usize
}

fn difference_degrees(a: f64, b: f64) -> f64 {
    180.0 - ((a - b).abs() - 180.0).abs()
}

/// `colors` — (ARGB, population) pairs in the quantizer's output order. Returns ARGB, best first.
pub fn score(colors: &[(u32, u32)]) -> Vec<u32> {
    let mut hcts = Vec::with_capacity(colors.len());
    let mut hue_population = [0u64; 360];
    let mut population_sum = 1e-5;

    for &(argb, population) in colors {
        let hct = Hct::new(Rgb::from_u32(argb));
        let hue = hct.get_hue() as usize % 360;
        hue_population[hue] += u64::from(population);
        population_sum += f64::from(population);
        hcts.push(hct);
    }

    let mut excited = [0.0f64; 360];
    for hue in 0..360i64 {
        let proportion = hue_population[hue as usize] as f64 / population_sum;
        for i in (hue - 14)..(hue + 16) {
            excited[sanitize_degrees_int(i)] += proportion;
        }
    }

    let mut scored: Vec<(Hct, f64)> = Vec::new();
    for hct in hcts {
        let hue = sanitize_degrees_int(hct.get_hue().round_ties_even() as i64);
        let proportion = excited[hue];
        let chroma = hct.get_chroma();
        if chroma < CUTOFF_CHROMA || proportion <= CUTOFF_EXCITED_PROPORTION {
            continue;
        }
        let proportion_score = proportion * 100.0 * WEIGHT_PROPORTION;
        let weight = if chroma < TARGET_CHROMA {
            WEIGHT_CHROMA_BELOW
        } else {
            WEIGHT_CHROMA_ABOVE
        };
        scored.push((hct, proportion_score + (chroma - TARGET_CHROMA) * weight));
    }
    // sort(reverse=True) in Python is stable: equal elements keep their original order
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));

    let mut chosen: Vec<Hct> = Vec::new();
    for min_difference in (15..=90).rev() {
        chosen.clear();
        for (hct, _) in &scored {
            let duplicate = chosen.iter().any(|c| {
                difference_degrees(hct.get_hue(), c.get_hue()) < f64::from(min_difference)
            });
            if !duplicate {
                chosen.push(*hct);
            }
            if chosen.len() >= DESIRED {
                break;
            }
        }
        if chosen.len() >= DESIRED {
            break;
        }
    }

    if chosen.is_empty() {
        return vec![FALLBACK];
    }
    chosen
        .into_iter()
        .map(|h| Rgb::from(h).as_argb_u32())
        .collect()
}
