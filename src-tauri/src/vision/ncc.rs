// ============================================================
// Synapse — Normalized Cross-Correlation (CPU)
// ============================================================
// Zero-mean NCC score for placing template T at (sx, sy) on the
// screen S, per RGB channel (alpha ignored), clamped to [0, 1]:
//
//   score = Σ (S − μS)·(T − μT) / (‖T − μT‖ · ‖S − μS‖)
//
// Because Σ (T − μT) = 0, the numerator reduces to Σ S·T′ with
// T′ = T − μT precomputed once, and n·‖S − μS‖² equals
// n·Σ S² − (Σ S)², which is evaluated with exact integer sums.
// Each candidate position therefore needs a single pass over the
// template, and rows are scored in parallel with rayon.
//
// This is also the reference the GPU matcher is verified against:
// GPU candidates are re-scored here in f64 (see `gpu.rs`).
// ============================================================

use super::capture::ScreenBuffer;
use rayon::prelude::*;

/// Best template position found by a matcher, in buffer coordinates
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NccMatch {
    /// Top-left X of the match inside the searched buffer
    pub x: u32,
    /// Top-left Y of the match inside the searched buffer
    pub y: u32,
    /// NCC score in [0, 1]
    pub score: f64,
}

/// A template preprocessed for NCC matching
#[derive(Debug, Clone)]
pub struct PreparedTemplate {
    pub width: u32,
    pub height: u32,
    /// `T − μT` per pixel as `[r, g, b]`, row-major, in 0..255 units
    pub centered: Vec<[f64; 3]>,
    /// `‖T − μT‖`
    pub norm: f64,
}

impl PreparedTemplate {
    pub fn new(tmpl: &ScreenBuffer) -> Result<Self, String> {
        let n = tmpl.width as usize * tmpl.height as usize;
        if n == 0 {
            return Err("Template image is empty".into());
        }

        let mut sum = [0u64; 3];
        for y in 0..tmpl.height {
            for x in 0..tmpl.width {
                let (r, g, b) = rgb_at(tmpl, x, y);
                sum[0] += r as u64;
                sum[1] += g as u64;
                sum[2] += b as u64;
            }
        }
        let mean = sum.map(|s| s as f64 / n as f64);

        let mut centered = Vec::with_capacity(n);
        let mut sum_sq = 0.0f64;
        for y in 0..tmpl.height {
            for x in 0..tmpl.width {
                let (r, g, b) = rgb_at(tmpl, x, y);
                let c = [r as f64 - mean[0], g as f64 - mean[1], b as f64 - mean[2]];
                sum_sq += c[0] * c[0] + c[1] * c[1] + c[2] * c[2];
                centered.push(c);
            }
        }

        let norm = sum_sq.sqrt();
        if norm < 1e-10 {
            return Err("Template image is blank or nearly uniform".into());
        }

        Ok(Self {
            width: tmpl.width,
            height: tmpl.height,
            centered,
            norm,
        })
    }

    pub fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }
}

/// Size of the search space `(columns, rows)`, or `None` if the template
/// does not fit inside the screen buffer
pub fn search_dims(screen: &ScreenBuffer, tmpl: &PreparedTemplate) -> Option<(u32, u32)> {
    if tmpl.width > screen.width || tmpl.height > screen.height {
        return None;
    }
    Some((screen.width - tmpl.width + 1, screen.height - tmpl.height + 1))
}

/// Exact NCC score of the template placed with its top-left at `(sx, sy)`.
/// The caller guarantees the template fits at that position.
pub fn score_at(screen: &ScreenBuffer, tmpl: &PreparedTemplate, sx: u32, sy: u32) -> f64 {
    let stride = screen.stride as usize;
    let tw = tmpl.width as usize;
    let n = tmpl.pixel_count() as u64;

    let mut sum = [0u64; 3];
    let mut sum_sq = [0u64; 3];
    let mut cross = 0.0f64;

    for ty in 0..tmpl.height as usize {
        let start = (sy as usize + ty) * stride + sx as usize * 4;
        let row = &screen.data[start..start + tw * 4];
        let tmpl_row = &tmpl.centered[ty * tw..(ty + 1) * tw];

        for (px, t) in row.chunks_exact(4).zip(tmpl_row) {
            // Buffers are BGRA
            let (r, g, b) = (px[2] as u64, px[1] as u64, px[0] as u64);
            sum[0] += r;
            sum[1] += g;
            sum[2] += b;
            sum_sq[0] += r * r;
            sum_sq[1] += g * g;
            sum_sq[2] += b * b;
            cross += r as f64 * t[0] + g as f64 * t[1] + b as f64 * t[2];
        }
    }

    // n·‖S − μS‖² = Σ_c (n·Σ S_c² − (Σ S_c)²), exact and never negative
    // (Cauchy–Schwarz), so no catastrophic cancellation on flat patches.
    let n_var: u128 = (0..3)
        .map(|c| n as u128 * sum_sq[c] as u128 - sum[c] as u128 * sum[c] as u128)
        .sum();
    let patch_norm = (n_var as f64 / n as f64).sqrt();

    let denom = tmpl.norm * patch_norm;
    if denom < 1e-10 {
        0.0
    } else {
        (cross / denom).clamp(0.0, 1.0)
    }
}

/// Pick the better of two matches: higher score wins, ties go to the
/// earlier position in row-major order (matches a sequential scan).
pub fn better(a: NccMatch, b: NccMatch) -> NccMatch {
    if b.score > a.score || (b.score == a.score && (b.y, b.x) < (a.y, a.x)) {
        b
    } else {
        a
    }
}

/// Exhaustively search the screen buffer for the best template position,
/// scoring rows in parallel. Returns `None` if the template does not fit.
pub fn find_best_cpu(screen: &ScreenBuffer, tmpl: &PreparedTemplate) -> Option<NccMatch> {
    let (search_w, search_h) = search_dims(screen, tmpl)?;

    (0..search_h)
        .into_par_iter()
        .map(|sy| {
            let mut best = NccMatch {
                x: 0,
                y: sy,
                score: f64::NEG_INFINITY,
            };
            for sx in 0..search_w {
                let score = score_at(screen, tmpl, sx, sy);
                if score > best.score {
                    best = NccMatch { x: sx, y: sy, score };
                }
            }
            best
        })
        .reduce_with(better)
}

/// Re-score candidate positions exactly and return the best one
pub fn best_of_candidates(
    screen: &ScreenBuffer,
    tmpl: &PreparedTemplate,
    candidates: impl IntoIterator<Item = (u32, u32)>,
) -> Option<NccMatch> {
    candidates
        .into_iter()
        .map(|(x, y)| NccMatch {
            x,
            y,
            score: score_at(screen, tmpl, x, y),
        })
        .reduce(better)
}

#[inline]
fn rgb_at(buf: &ScreenBuffer, x: u32, y: u32) -> (u8, u8, u8) {
    let o = y as usize * buf.stride as usize + x as usize * 4;
    (buf.data[o + 2], buf.data[o + 1], buf.data[o])
}

#[cfg(test)]
pub(crate) mod test_util {
    use super::super::capture::ScreenBuffer;

    /// Small deterministic PRNG (xorshift64*) so tests need no seeding API
    pub struct Rng(u64);

    impl Rng {
        pub fn new(seed: u64) -> Self {
            Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
        }
        pub fn next_u32(&mut self) -> u32 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
        }
        pub fn below(&mut self, n: u32) -> u32 {
            self.next_u32() % n
        }
    }

    pub fn random_image(rng: &mut Rng, width: u32, height: u32) -> ScreenBuffer {
        let data = (0..width * height)
            .flat_map(|_| {
                let v = rng.next_u32().to_le_bytes();
                [v[0], v[1], v[2], 255]
            })
            .collect();
        ScreenBuffer {
            data,
            width,
            height,
            stride: width * 4,
        }
    }

    /// Smooth gradients plus mild noise: neighbouring positions score
    /// similarly, which stresses tie-breaking and numeric stability
    pub fn smooth_image(rng: &mut Rng, width: u32, height: u32) -> ScreenBuffer {
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let noise = rng.below(5) as i32 - 2;
                let r = ((x * 255 / width.max(1)) as i32 + noise).clamp(0, 255) as u8;
                let g = ((y * 255 / height.max(1)) as i32 - noise).clamp(0, 255) as u8;
                let b = (((x + y) * 3) % 256) as u8;
                data.extend_from_slice(&[b, g, r, 255]);
            }
        }
        ScreenBuffer {
            data,
            width,
            height,
            stride: width * 4,
        }
    }

    pub fn solid_image(width: u32, height: u32, bgra: [u8; 4]) -> ScreenBuffer {
        ScreenBuffer {
            data: bgra.repeat((width * height) as usize),
            width,
            height,
            stride: width * 4,
        }
    }

    pub fn crop(src: &ScreenBuffer, x: u32, y: u32, width: u32, height: u32) -> ScreenBuffer {
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for row in y..y + height {
            let start = (row * src.stride + x * 4) as usize;
            data.extend_from_slice(&src.data[start..start + (width * 4) as usize]);
        }
        ScreenBuffer {
            data,
            width,
            height,
            stride: width * 4,
        }
    }

    pub fn paste(dst: &mut ScreenBuffer, src: &ScreenBuffer, x: u32, y: u32) {
        for row in 0..src.height {
            let s = (row * src.stride) as usize;
            let d = ((y + row) * dst.stride + x * 4) as usize;
            let len = (src.width * 4) as usize;
            dst.data[d..d + len].copy_from_slice(&src.data[s..s + len]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_util::*;
    use super::*;

    /// The original two-pass f64 NCC implementation, kept as the
    /// ground truth the optimized matcher must agree with.
    fn reference_score(screen: &ScreenBuffer, tmpl: &ScreenBuffer, ox: u32, oy: u32) -> f64 {
        let n = (tmpl.width * tmpl.height) as f64;
        let px = |b: &ScreenBuffer, x: u32, y: u32| {
            let (r, g, b, _) = b.get_pixel(x, y).unwrap();
            [r as f64, g as f64, b as f64]
        };

        let (mut tm, mut sm) = ([0.0; 3], [0.0; 3]);
        for y in 0..tmpl.height {
            for x in 0..tmpl.width {
                let (t, s) = (px(tmpl, x, y), px(screen, ox + x, oy + y));
                for c in 0..3 {
                    tm[c] += t[c] / n;
                    sm[c] += s[c] / n;
                }
            }
        }

        let (mut cross, mut t_sq, mut s_sq) = (0.0, 0.0, 0.0);
        for y in 0..tmpl.height {
            for x in 0..tmpl.width {
                let (t, s) = (px(tmpl, x, y), px(screen, ox + x, oy + y));
                for c in 0..3 {
                    let (dt, ds) = (t[c] - tm[c], s[c] - sm[c]);
                    cross += dt * ds;
                    t_sq += dt * dt;
                    s_sq += ds * ds;
                }
            }
        }

        let denom = t_sq.sqrt() * s_sq.sqrt();
        if denom < 1e-10 {
            0.0
        } else {
            (cross / denom).clamp(0.0, 1.0)
        }
    }

    fn reference_best(screen: &ScreenBuffer, tmpl: &ScreenBuffer) -> NccMatch {
        let mut best = NccMatch { x: 0, y: 0, score: 0.0 };
        for y in 0..=screen.height - tmpl.height {
            for x in 0..=screen.width - tmpl.width {
                let score = reference_score(screen, tmpl, x, y);
                if score > best.score {
                    best = NccMatch { x, y, score };
                }
            }
        }
        best
    }

    #[test]
    fn score_at_agrees_with_reference_everywhere() {
        let mut rng = Rng::new(7);
        let screen = smooth_image(&mut rng, 37, 23);
        let raw = crop(&screen, 9, 4, 7, 6);
        let tmpl = PreparedTemplate::new(&raw).unwrap();

        for y in 0..=screen.height - raw.height {
            for x in 0..=screen.width - raw.width {
                let fast = score_at(&screen, &tmpl, x, y);
                let slow = reference_score(&screen, &raw, x, y);
                assert!((fast - slow).abs() < 1e-9, "({x},{y}): {fast} vs {slow}");
            }
        }
    }

    #[test]
    fn finds_embedded_template_like_reference() {
        for seed in 1..=6 {
            let mut rng = Rng::new(seed);
            let (w, h) = (30 + rng.below(20), 20 + rng.below(15));
            let screen = random_image(&mut rng, w, h);
            let (tw, th) = (3 + rng.below(6), 3 + rng.below(6));
            let (tx, ty) = (rng.below(w - tw + 1), rng.below(h - th + 1));
            let raw = crop(&screen, tx, ty, tw, th);

            let fast = find_best_cpu(&screen, &PreparedTemplate::new(&raw).unwrap()).unwrap();
            let slow = reference_best(&screen, &raw);

            assert_eq!((fast.x, fast.y), (tx, ty), "seed {seed}");
            assert_eq!((fast.x, fast.y), (slow.x, slow.y), "seed {seed}");
            assert!((fast.score - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn best_matches_reference_on_smooth_images() {
        for seed in 10..14 {
            let mut rng = Rng::new(seed);
            let screen = smooth_image(&mut rng, 41, 29);
            // A template from a *different* image, so no exact match exists
            let other = smooth_image(&mut Rng::new(seed + 100), 41, 29);
            let raw = crop(&other, 5, 5, 8, 7);

            let fast = find_best_cpu(&screen, &PreparedTemplate::new(&raw).unwrap()).unwrap();
            let slow = reference_best(&screen, &raw);
            assert_eq!((fast.x, fast.y), (slow.x, slow.y), "seed {seed}");
            assert!((fast.score - slow.score).abs() < 1e-9);
        }
    }

    #[test]
    fn flat_regions_never_match() {
        let screen = solid_image(40, 30, [40, 80, 120, 255]);
        let mut rng = Rng::new(3);
        let raw = random_image(&mut rng, 5, 5);
        let best = find_best_cpu(&screen, &PreparedTemplate::new(&raw).unwrap()).unwrap();
        assert_eq!(best.score, 0.0);
        assert_eq!((best.x, best.y), (0, 0));
    }

    #[test]
    fn ties_resolve_to_first_position() {
        let mut rng = Rng::new(21);
        let raw = random_image(&mut rng, 4, 4);
        let mut screen = solid_image(30, 20, [0, 0, 0, 255]);
        paste(&mut screen, &raw, 20, 3);
        paste(&mut screen, &raw, 2, 11);
        paste(&mut screen, &raw, 9, 3);

        let best = find_best_cpu(&screen, &PreparedTemplate::new(&raw).unwrap()).unwrap();
        assert_eq!((best.x, best.y), (9, 3));
    }

    #[test]
    fn blank_template_is_rejected() {
        let raw = solid_image(6, 6, [9, 9, 9, 255]);
        assert!(PreparedTemplate::new(&raw).is_err());
    }

    #[test]
    fn oversized_template_does_not_fit() {
        let mut rng = Rng::new(5);
        let screen = random_image(&mut rng, 8, 8);
        let raw = random_image(&mut rng, 9, 4);
        assert!(find_best_cpu(&screen, &PreparedTemplate::new(&raw).unwrap()).is_none());
    }

    /// `cargo test --release --lib -- --ignored --nocapture bench_cpu`
    #[test]
    #[ignore]
    fn bench_cpu_vs_reference() {
        let mut rng = Rng::new(99);
        let screen = smooth_image(&mut rng, 640, 360);
        let raw = crop(&screen, 400, 200, 32, 32);

        let t = std::time::Instant::now();
        let slow = reference_best(&screen, &raw);
        let reference = t.elapsed();

        let tmpl = PreparedTemplate::new(&raw).unwrap();
        let t = std::time::Instant::now();
        let fast = find_best_cpu(&screen, &tmpl).unwrap();
        let optimized = t.elapsed();

        assert_eq!((fast.x, fast.y), (slow.x, slow.y));
        println!(
            "640x360 / 32x32 on {} threads: reference {reference:?}, optimized {optimized:?} ({:.1}x)",
            rayon::current_num_threads(),
            reference.as_secs_f64() / optimized.as_secs_f64()
        );
    }
}
