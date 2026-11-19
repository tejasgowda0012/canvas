//! Handwriting refine: tidies the user's own handwriting and keeps it
//! handwritten. It does not recognise or retype anything. It only moves,
//! scales, shears and smooths the strokes that are already there:
//!
//! * calms shaky strokes (tremor and touch-panel jitter);
//! * straightens a tilted line of writing;
//! * sits every word on one shared baseline;
//! * evens out word size, slant and the gaps between words.
//!
//! Each correction moves a word only part of the way (by [`STRENGTH`])
//! toward the norm of its own line, so the writing still looks like the
//! writer's and not like a font.

use crate::geom::Pt;

/// How far each word moves toward its line's norm: 0 leaves it alone,
/// 1 makes every word match exactly.
pub const STRENGTH: f64 = 0.8;

/// Steepest line tilt that is straightened, in degrees. Steeper writing
/// is taken to be deliberate.
const MAX_TILT_DEG: f64 = 20.0;

/// Refines `strokes`, which may hold several lines of writing. Returns one
/// new stroke for each input stroke, in the same order.
pub fn refine(strokes: &[Vec<Pt>]) -> Vec<Vec<Pt>> {
    let mut out: Vec<Vec<Pt>> = strokes.to_vec();
    if strokes.iter().all(|s| s.len() < 2) {
        return out;
    }
    let angle = dominant_tilt(strokes);
    for line in split_lines(strokes, angle) {
        let picked: Vec<Vec<Pt>> = line.iter().map(|&i| strokes[i].clone()).collect();
        for (k, s) in refine_line(&picked).into_iter().enumerate() {
            out[line[k]] = s;
        }
    }
    out
}

/// Refines a single line of writing.
pub fn refine_line(strokes: &[Vec<Pt>]) -> Vec<Vec<Pt>> {
    let all: Vec<Pt> = strokes.iter().flatten().copied().collect();
    if all.len() < 4 {
        return strokes.to_vec();
    }
    // Level the line first, so heights and gaps can be measured.
    let tilt = dominant_tilt(strokes);
    let pivot = centroid(&all);
    let level = |p: Pt| rotate_about(p, pivot, -tilt);
    let mut lines: Vec<Vec<Pt>> = strokes.iter().map(|s| s.iter().map(|&p| level(p)).collect()).collect();
    let h = core_height(&lines.iter().flatten().copied().collect::<Vec<_>>());
    if h <= 0.5 {
        return strokes.to_vec();
    }
    for s in lines.iter_mut() {
        *s = smooth(s, h);
    }

    let mut words = group_words(&lines, h);
    let mut info: Vec<Word> = words.iter().map(|w| Word::measure(&lines, w)).collect();

    // What remains of the tilt after levelling shows in the word baselines.
    if info.len() >= 3 {
        let xs: Vec<(f64, f64)> = info.iter().map(|w| (w.center_x, w.baseline)).collect();
        let slope = fit_slope(&xs);
        if slope.abs() < 0.35 {
            let a = slope.atan();
            let c = centroid(&lines.iter().flatten().copied().collect::<Vec<_>>());
            for s in lines.iter_mut() {
                for p in s.iter_mut() {
                    *p = rotate_about(*p, c, -a);
                }
            }
            info = words.iter().map(|w| Word::measure(&lines, w)).collect();
        }
    }

    let base = median(info.iter().map(|w| w.baseline).collect());
    let height = median(info.iter().filter(|w| w.core > 0.0).map(|w| w.core).collect());
    let slant = median(info.iter().filter_map(|w| w.slant).collect());
    for (w, m) in words.iter().zip(&info) {
        let anchor = Pt::new(m.left, m.baseline);
        let k = if m.core > 0.0 && height > 0.0 {
            (height / m.core).powf(STRENGTH).clamp(0.75, 1.33)
        } else {
            1.0
        };
        let shear = m.slant.map_or(0.0, |s| (slant - s) * STRENGTH);
        let dy = (base - m.baseline) * STRENGTH;
        for &i in w {
            for p in lines[i].iter_mut() {
                let d = *p - anchor;
                let d = Pt::new(d.x + shear * d.y, d.y) * k;
                *p = Pt::new(anchor.x + d.x, anchor.y + d.y + dy);
            }
        }
    }

    // Even out the gaps between words, measured after the changes above.
    words.sort_by(|a, b| extent(&lines, a).0.total_cmp(&extent(&lines, b).0));
    let spans: Vec<(f64, f64)> = words.iter().map(|w| extent(&lines, w)).collect();
    if spans.len() >= 2 {
        let gaps: Vec<f64> = spans.windows(2).map(|p| p[1].0 - p[0].1).collect();
        let target = median(gaps.clone()).max(0.5 * h);
        let mut shift = 0.0;
        for (i, w) in words.iter().enumerate().skip(1) {
            let g = gaps[i - 1];
            let wanted = (g + (target - g) * STRENGTH).max(0.35 * h);
            shift += wanted - g;
            for &s in w {
                for p in lines[s].iter_mut() {
                    p.x += shift;
                }
            }
        }
    }

    // Keep the line where it was written: same start, same baseline.
    let before = left_baseline(strokes, tilt);
    let after = left_baseline(&lines, 0.0);
    let d = before - after;
    for s in lines.iter_mut() {
        for p in s.iter_mut() {
            *p = *p + d;
        }
    }
    lines
}

/// One word's measurements on a levelled line.
struct Word {
    left: f64,
    center_x: f64,
    /// Where the letters sit (largest y of the letter bottoms).
    baseline: f64,
    /// Height of the small letters (baseline to the tops of a, c, e...).
    core: f64,
    /// Horizontal lean per unit of height of the downstrokes.
    slant: Option<f64>,
}

impl Word {
    fn measure(lines: &[Vec<Pt>], word: &[usize]) -> Word {
        let (left, right) = extent(lines, word);
        let pts: Vec<Pt> = word.iter().flat_map(|&i| lines[i].iter().copied()).collect();
        let (mut bottoms, mut tops) = (Vec::new(), Vec::new());
        for &i in word {
            let (b, t) = turning_points(&lines[i]);
            bottoms.extend(b);
            tops.extend(t);
        }
        let ys: Vec<f64> = pts.iter().map(|p| p.y).collect();
        let baseline = if bottoms.is_empty() { percentile(ys.clone(), 0.85) } else { median(bottoms) };
        let midline = if tops.is_empty() { percentile(ys, 0.15) } else { median(tops) };
        Word {
            left,
            center_x: (left + right) / 2.0,
            baseline,
            core: (baseline - midline).max(0.0),
            slant: slant_of(word.iter().map(|&i| lines[i].as_slice())),
        }
    }
}

/// Letter bottoms (local maxima of y) and tops (local minima) along a
/// stroke, ignoring wiggles smaller than a few pixels.
fn turning_points(s: &[Pt]) -> (Vec<f64>, Vec<f64>) {
    let (mut bottoms, mut tops) = (Vec::new(), Vec::new());
    if s.len() < 3 {
        return (bottoms, tops);
    }
    const MIN: f64 = 2.0;
    // Hysteresis walk: a turn counts once y has come back by MIN.
    let mut extreme = s[0].y;
    let mut going_down: Option<bool> = None;
    for p in &s[1..] {
        match going_down {
            None => {
                if p.y - extreme > MIN {
                    going_down = Some(true);
                    extreme = p.y;
                } else if extreme - p.y > MIN {
                    going_down = Some(false);
                    extreme = p.y;
                }
            }
            Some(true) => {
                if p.y > extreme {
                    extreme = p.y;
                } else if extreme - p.y > MIN {
                    bottoms.push(extreme);
                    going_down = Some(false);
                    extreme = p.y;
                }
            }
            Some(false) => {
                if p.y < extreme {
                    extreme = p.y;
                } else if p.y - extreme > MIN {
                    tops.push(extreme);
                    going_down = Some(true);
                    extreme = p.y;
                }
            }
        }
    }
    (bottoms, tops)
}

/// Lean of the near-vertical parts of the strokes: x change per unit of y,
/// weighted by length.
fn slant_of<'a>(strokes: impl Iterator<Item = &'a [Pt]>) -> Option<f64> {
    let (mut sum, mut weight) = (0.0, 0.0);
    for s in strokes {
        for seg in s.windows(2) {
            let d = seg[1] - seg[0];
            if d.y.abs() > 2.0 * d.x.abs() && d.y.abs() > 1e-9 {
                let l = d.len();
                sum += d.x / d.y * l;
                weight += l;
            }
        }
    }
    (weight > 1.0).then(|| sum / weight)
}

/// The angle (radians) that lines up most of the ink on horizontal rows,
/// found by trying tilts and keeping the sharpest row profile.
fn dominant_tilt(strokes: &[Vec<Pt>]) -> f64 {
    let pts: Vec<Pt> = strokes.iter().flatten().copied().collect();
    if pts.len() < 4 {
        return 0.0;
    }
    let c = centroid(&pts);
    let sharpness = |a: f64| {
        let (sin, cos) = (-a).sin_cos();
        let mut rows = std::collections::HashMap::<i64, f64>::new();
        for p in &pts {
            let d = *p - c;
            let y = d.x * sin + d.y * cos;
            *rows.entry((y / 2.0).floor() as i64).or_default() += 1.0;
        }
        rows.values().map(|n| n * n).sum::<f64>()
    };
    let mut best = (0.0, sharpness(0.0));
    let max = MAX_TILT_DEG.to_radians();
    let mut a = -max;
    while a <= max + 1e-9 {
        let v = sharpness(a);
        if v > best.1 {
            best = (a, v);
        }
        a += 0.5f64.to_radians();
    }
    let mut a = best.0 - 0.5f64.to_radians();
    while a <= best.0 + 0.5f64.to_radians() {
        let v = sharpness(a);
        if v > best.1 {
            best = (a, v);
        }
        a += 0.1f64.to_radians();
    }
    // The sharpest profile of a short scribble can be anything: only trust
    // lines that are clearly wider than tall.
    let along: Vec<f64> = pts.iter().map(|p| rotate_about(*p, c, -best.0).x).collect();
    let across: Vec<f64> = pts.iter().map(|p| rotate_about(*p, c, -best.0).y).collect();
    let span = |v: &[f64]| v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min);
    if span(&along) < 2.5 * span(&across) {
        return 0.0;
    }
    best.0
}

/// Splits strokes into lines of writing (indices into `strokes`), top to
/// bottom, measured across the writing direction `angle`.
fn split_lines(strokes: &[Vec<Pt>], angle: f64) -> Vec<Vec<usize>> {
    let pts: Vec<Pt> = strokes.iter().flatten().copied().collect();
    let c = centroid(&pts);
    let level = |p: Pt| rotate_about(p, c, -angle);
    let mut spans: Vec<(usize, f64, f64)> = strokes
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.is_empty())
        .map(|(i, s)| {
            let ys: Vec<f64> = s.iter().map(|&p| level(p).y).collect();
            (i, ys.iter().cloned().fold(f64::MAX, f64::min), ys.iter().cloned().fold(f64::MIN, f64::max))
        })
        .collect();
    // Typical letter size from single strokes: the whole selection's
    // spread would include the space between lines.
    let h = median(strokes.iter().filter(|s| s.len() > 1).map(|s| core_height(&s.iter().map(|&p| level(p)).collect::<Vec<_>>())).collect()).max(1.0);
    spans.sort_by(|a, b| ((a.1 + a.2) / 2.0).total_cmp(&((b.1 + b.2) / 2.0)));
    let mut lines: Vec<(Vec<usize>, f64)> = Vec::new();
    for (i, top, bottom) in spans {
        let mid = (top + bottom) / 2.0;
        match lines.last_mut() {
            Some((line, centre)) if mid - *centre < 1.6 * h => {
                line.push(i);
                *centre = (*centre * (line.len() - 1) as f64 + mid) / line.len() as f64;
            }
            _ => lines.push((vec![i], mid)),
        }
    }
    lines.into_iter().map(|(l, _)| l).collect()
}

/// Groups strokes of a levelled line into words: strokes whose horizontal
/// extents overlap or nearly touch belong to the same word.
fn group_words(lines: &[Vec<Pt>], h: f64) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..lines.len()).filter(|&i| !lines[i].is_empty()).collect();
    order.sort_by(|&a, &b| extent(lines, &[a]).0.total_cmp(&extent(lines, &[b]).0));
    let mut words: Vec<(Vec<usize>, f64)> = Vec::new();
    for i in order {
        let (l, r) = extent(lines, &[i]);
        match words.last_mut() {
            Some((w, right)) if l - *right < 0.6 * h => {
                w.push(i);
                *right = right.max(r);
            }
            _ => words.push((vec![i], r)),
        }
    }
    words.into_iter().map(|(w, _)| w).collect()
}

/// Calms tremor and jitter: resamples evenly and applies a short Gaussian
/// along the stroke. The ends stay put.
fn smooth(s: &[Pt], h: f64) -> Vec<Pt> {
    if s.len() < 3 {
        return s.to_vec();
    }
    let step = (h / 16.0).clamp(0.75, 3.0);
    let pts = resample(s, step);
    if pts.len() < 5 {
        return pts;
    }
    let sigma = (h * 0.07 / step).max(0.8); // in samples
    let r = (sigma * 2.5).ceil() as isize;
    let kernel: Vec<f64> = (-r..=r).map(|i| (-(i * i) as f64 / (2.0 * sigma * sigma)).exp()).collect();
    let n = pts.len() as isize;
    let mut out = Vec::with_capacity(pts.len());
    for i in 0..n {
        // Narrow the window near the ends so they are not pulled inwards.
        let reach = r.min(i).min(n - 1 - i);
        let (mut acc, mut w) = (Pt::ZERO, 0.0);
        for j in -reach..=reach {
            let k = kernel[(j + r) as usize];
            acc = acc + pts[(i + j) as usize] * k;
            w += k;
        }
        out.push(acc / w);
    }
    out
}

fn resample(s: &[Pt], step: f64) -> Vec<Pt> {
    let mut out = vec![s[0]];
    let mut carry = 0.0;
    for seg in s.windows(2) {
        let (a, b) = (seg[0], seg[1]);
        let l = a.dist(b);
        if l <= 0.0 {
            continue;
        }
        let mut t = step - carry;
        while t <= l {
            out.push(a + (b - a) * (t / l));
            t += step;
        }
        carry = l - (t - step);
    }
    let last = *s.last().unwrap();
    if out.last().unwrap().dist(last) > 1e-6 {
        out.push(last);
    }
    out
}

/// Height of the band that holds most of the ink: a stand-in for the size
/// of the small letters.
fn core_height(pts: &[Pt]) -> f64 {
    let ys: Vec<f64> = pts.iter().map(|p| p.y).collect();
    percentile(ys.clone(), 0.8) - percentile(ys, 0.2)
}

fn left_baseline(strokes: &[Vec<Pt>], tilt: f64) -> Pt {
    let pts: Vec<Pt> = strokes.iter().flatten().copied().collect();
    let c = centroid(&pts);
    let level: Vec<Pt> = pts.iter().map(|&p| rotate_about(p, c, -tilt)).collect();
    let left = level.iter().map(|p| p.x).fold(f64::MAX, f64::min);
    let base = percentile(level.iter().map(|p| p.y).collect(), 0.85);
    rotate_about(Pt::new(left, base), c, tilt)
}

fn extent(lines: &[Vec<Pt>], word: &[usize]) -> (f64, f64) {
    let mut l = f64::MAX;
    let mut r = f64::MIN;
    for &i in word {
        for p in &lines[i] {
            l = l.min(p.x);
            r = r.max(p.x);
        }
    }
    (l, r)
}

fn fit_slope(xy: &[(f64, f64)]) -> f64 {
    let n = xy.len() as f64;
    let (mx, my) = (xy.iter().map(|p| p.0).sum::<f64>() / n, xy.iter().map(|p| p.1).sum::<f64>() / n);
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for &(x, y) in xy {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx) * (x - mx);
    }
    if sxx <= 1e-9 { 0.0 } else { sxy / sxx }
}

fn centroid(pts: &[Pt]) -> Pt {
    pts.iter().fold(Pt::ZERO, |a, &p| a + p) / pts.len().max(1) as f64
}

fn rotate_about(p: Pt, c: Pt, a: f64) -> Pt {
    let (sin, cos) = a.sin_cos();
    let d = p - c;
    Pt::new(c.x + d.x * cos - d.y * sin, c.y + d.x * sin + d.y * cos)
}

fn median(v: Vec<f64>) -> f64 {
    percentile(v, 0.5)
}

fn percentile(mut v: Vec<f64>, q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    let i = q * (v.len() - 1) as f64;
    let (lo, hi) = (i.floor() as usize, i.ceil() as usize);
    v[lo] + (v[hi] - v[lo]) * (i - lo as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A word of `n` "u"-like humps: x-height `h`, baseline `base`.
    fn word(x: f64, base: f64, h: f64, n: usize, lean: f64) -> Vec<Pt> {
        let mut s = Vec::new();
        for i in 0..=n * 20 {
            let t = i as f64 / 20.0;
            let y = base - h * (0.5 + 0.5 * (t * std::f64::consts::TAU).cos());
            let up = base - y;
            s.push(Pt::new(x + t * h * 0.8 + lean * up, y));
        }
        s
    }

    fn baseline_spread(words: &[Vec<Pt>]) -> f64 {
        let b: Vec<f64> = words.iter().map(|w| w.iter().map(|p| p.y).fold(f64::MIN, f64::max)).collect();
        b.iter().cloned().fold(f64::MIN, f64::max) - b.iter().cloned().fold(f64::MAX, f64::min)
    }

    #[test]
    fn words_settle_on_one_baseline() {
        let input = vec![word(0., 100., 20., 3, 0.), word(70., 112., 20., 3, 0.), word(140., 95., 20., 3, 0.)];
        let out = refine(&input);
        assert!(baseline_spread(&input) > 15.0);
        assert!(baseline_spread(&out) < 5.0, "spread {}", baseline_spread(&out));
    }

    #[test]
    fn a_tilted_line_is_straightened() {
        let a = 8f64.to_radians();
        let flat = vec![word(0., 100., 20., 3, 0.), word(70., 100., 20., 3, 0.), word(140., 100., 20., 3, 0.), word(210., 100., 20., 3, 0.)];
        let tilted: Vec<Vec<Pt>> = flat.iter().map(|s| s.iter().map(|&p| rotate_about(p, Pt::new(0., 100.), a)).collect()).collect();
        let out = refine(&tilted);
        assert!(baseline_spread(&tilted) > 25.0);
        assert!(baseline_spread(&out) < 4.0, "spread {}", baseline_spread(&out));
    }

    #[test]
    fn sizes_and_slants_move_toward_the_line() {
        let input = vec![word(0., 100., 20., 3, 0.), word(70., 100., 30., 3, 0.3), word(170., 100., 20., 3, 0.)];
        let out = refine(&input);
        let height = |s: &[Pt]| s.iter().map(|p| p.y).fold(f64::MIN, f64::max) - s.iter().map(|p| p.y).fold(f64::MAX, f64::min);
        assert!(height(&out[1]) < 24.0, "{}", height(&out[1]));
        let lean = |s: &[Pt]| slant_of(std::iter::once(s)).unwrap().abs();
        assert!(lean(&out[1]) < lean(&input[1]) * 0.5);
    }

    #[test]
    fn shaky_strokes_get_calmer_and_keep_their_ends() {
        let clean = word(0., 100., 24., 4, 0.);
        let shaky: Vec<Pt> = clean.iter().enumerate().map(|(i, p)| Pt::new(p.x + (i as f64 * 1.3).sin() * 1.2, p.y + (i as f64 * 1.7).cos() * 1.2)).collect();
        let out = refine(&[shaky.clone()]);
        let rough = |s: &[Pt]| s.windows(3).map(|w| (w[0] + w[2] - w[1] * 2.0).len()).sum::<f64>() / s.len() as f64;
        assert!(rough(&out[0]) < rough(&shaky) * 0.5);
        assert!(out[0][0].dist(shaky[0]) < 3.0);
    }

    #[test]
    fn two_lines_are_refined_separately() {
        let input = vec![word(0., 100., 20., 3, 0.), word(70., 106., 20., 3, 0.), word(0., 180., 20., 3, 0.), word(70., 172., 20., 3, 0.)];
        let out = refine(&input);
        assert!(baseline_spread(&out[..2]) < 3.0, "first {}", baseline_spread(&out[..2]));
        assert!(baseline_spread(&out[2..]) < 3.0);
        assert!((out[2][0].y - out[0][0].y) > 50.0, "lines stay apart");
    }

    #[test]
    fn tiny_input_is_left_alone() {
        let dot = vec![vec![Pt::new(1., 1.)]];
        assert_eq!(refine(&dot), dot);
    }
}
