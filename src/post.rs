//! After the samples: an edge-aware denoiser guided by albedo, normals, depth and each pixel's own noise, and bloom.
use crate::render::Frame;
use rayon::prelude::*;

fn lum(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// Five a-trous wavelet passes over the light with the surface colour divided out, each weighing a neighbour by how
/// alike its normal, depth and brightness are, brightness measured against the noise left in both.
pub fn denoise(f: &mut Frame) {
    let (w, h) = (f.w, f.h);
    let alb = |i: usize| {
        let a = &f.aux[i];
        [a[0].max(0.02), a[1].max(0.02), a[2].max(0.02)]
    };
    let mut c: Vec<[f32; 3]> = (0..w * h).map(|i| {
        let a = alb(i);
        [f.px[i][0] / a[0], f.px[i][1] / a[1], f.px[i][2] / a[2]]
    }).collect();
    let mut var: Vec<f32> = (0..w * h).map(|i| f.var[i] / lum(alb(i)).powi(2)).collect();
    let kern = [1.0 / 16.0, 0.25, 0.375, 0.25, 1.0 / 16.0f32];
    for pass in 0..5 {
        let step = 1i64 << pass;
        let next: Vec<([f32; 3], f32)> = (0..w * h).into_par_iter().map(|i| {
            let me = &f.aux[i];
            if !me[6].is_finite() {
                return (c[i], var[i]);
            }
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            let l0 = lum(c[i]);
            let (mut sum, mut sv, mut tw) = ([0.0f32; 3], 0.0f32, 0.0f32);
            for (dj, kj) in kern.iter().enumerate() {
                for (di, ki) in kern.iter().enumerate() {
                    let (qx, qy) = (x + (di as i64 - 2) * step, y + (dj as i64 - 2) * step);
                    if qx < 0 || qy < 0 || qx >= w as i64 || qy >= h as i64 {
                        continue;
                    }
                    let q = qy as usize * w + qx as usize;
                    let a = &f.aux[q];
                    if !a[6].is_finite() {
                        continue;
                    }
                    let nd = (me[3] * a[3] + me[4] * a[4] + me[5] * a[5]).max(0.0).powi(32);
                    let zd = (-(me[6] - a[6]).abs() / (me[6] * 0.02 * step as f32 + 1e-6)).exp();
                    let ld = (-(l0 - lum(c[q])).abs() / (4.0 * (var[i] + var[q]).max(0.0).sqrt() + 1e-4)).exp();
                    let wt = ki * kj * nd * zd * ld;
                    for k in 0..3 {
                        sum[k] += c[q][k] * wt;
                    }
                    sv += wt * wt * var[q];
                    tw += wt;
                }
            }
            if tw <= 1e-12 {
                (c[i], var[i])
            } else {
                ([sum[0] / tw, sum[1] / tw, sum[2] / tw], sv / (tw * tw))
            }
        }).collect();
        for (i, (cc, vv)) in next.into_iter().enumerate() {
            c[i] = cc;
            var[i] = vv;
        }
    }
    for i in 0..w * h {
        if f.aux[i][6].is_finite() {
            let a = alb(i);
            f.px[i] = [c[i][0] * a[0], c[i][1] * a[1], c[i][2] * a[2], f.px[i][3]];
        }
    }
}

fn blur_rows(img: &mut [[f32; 3]], w: usize, r: usize) {
    img.par_chunks_mut(w).for_each(|row| {
        for _ in 0..3 {
            let src = row.to_vec();
            let mut acc = [0.0f32; 3];
            let at = |i: i64| src[i.clamp(0, w as i64 - 1) as usize];
            for i in -(r as i64)..=(r as i64) {
                let p = at(i);
                (0..3).for_each(|k| acc[k] += p[k]);
            }
            let n = (2 * r + 1) as f32;
            for x in 0..w {
                row[x] = [acc[0] / n, acc[1] / n, acc[2] / n];
                let (add, sub) = (at(x as i64 + r as i64 + 1), at(x as i64 - r as i64));
                (0..3).for_each(|k| acc[k] += add[k] - sub[k]);
            }
        }
    });
}

fn transpose(img: &[[f32; 3]], w: usize, h: usize) -> Vec<[f32; 3]> {
    (0..w * h).map(|i| img[(i % h) * w + i / h]).collect()
}

fn blur(img: &[[f32; 3]], w: usize, h: usize, r: usize) -> Vec<[f32; 3]> {
    let mut a = img.to_vec();
    blur_rows(&mut a, w, r);
    let mut t = transpose(&a, w, h);
    blur_rows(&mut t, h, r);
    transpose(&t, h, w)
}

/// Light above 1 spreads into a near and a wide glow, `amount` strong.
pub fn bloom(f: &mut Frame, amount: f64) {
    let (w, h) = (f.w, f.h);
    let hot: Vec<[f32; 3]> = f.px.iter().map(|p| [(p[0] - 1.0).max(0.0), (p[1] - 1.0).max(0.0), (p[2] - 1.0).max(0.0)]).collect();
    let near = blur(&hot, w, h, (w.max(h) / 200).max(1));
    let wide = blur(&hot, w, h, (w.max(h) / 40).max(2));
    let k = amount as f32;
    for i in 0..w * h {
        for c in 0..3 {
            f.px[i][c] += (near[i][c] + wide[i][c]) * 0.5 * k;
        }
    }
}
