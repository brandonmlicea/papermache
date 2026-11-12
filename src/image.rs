//! A picture raised into a solid: every pixel that is not the key colour becomes a column, in that pixel's own colour.
use crate::math::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub struct Pixels {
    pub w: usize,
    pub h: usize,
    pub on: Vec<bool>,
    pub rgb: Vec<[f32; 3]>,
    pub aspect: f64,
    cov: Vec<f32>,
    cw: usize,
    ch: usize,
}

fn linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn decode(path: &str) -> Result<(usize, usize, Vec<[u8; 4]>), String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(f));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut r = dec.read_info().map_err(|e| format!("{path}: {e}"))?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).map_err(|e| format!("{path}: {e}"))?;
    let px: Vec<[u8; 4]> = match info.color_type {
        png::ColorType::Rgba => buf.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect(),
        png::ColorType::Rgb => buf.chunks(3).map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks(2).map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(format!("{path}: indexed PNG not expanded")),
    };
    Ok((info.width as usize, info.height as usize, px[..info.width as usize * info.height as usize].to_vec()))
}

/// The picture at `detail` cells a pixel. A pixel's coverage is its alpha times how far it is from `key` against the
/// farthest pixel within two in the same colour direction, so it reads a blend of `key` and one colour, smoothed bilinearly and cut at one half. A solid pixel keeps its own colour; an anti-aliased
/// edge takes its nearest solid neighbour's, so edges neither thicken the shape nor tint its walls.
fn build(path: &str, key: [f64; 3], detail: usize) -> Result<Pixels, String> {
    let (w, h, px) = decode(path)?;
    let k = key.map(|c| (c * 255.0) as f64);
    let off: Vec<V3> = px.iter().map(|p| v3(p[0] as f64 - k[0], p[1] as f64 - k[1], p[2] as f64 - k[2]) / 255.0).collect();
    let alpha: Vec<f64> = px.iter().map(|p| p[3] as f64 / 255.0).collect();
    let near = |i: usize, j: usize, r: i64| -> Vec<usize> {
        let (i, j) = (i as i64, j as i64);
        (-r..=r).flat_map(|dj| (-r..=r).map(move |di| (i + di, j + dj)))
            .filter(|&(a, b)| a >= 0 && b >= 0 && (a as usize) < w && (b as usize) < h)
            .map(|(a, b)| b as usize * w + a as usize).collect()
    };
    let cov: Vec<f64> = (0..w * h).map(|q| {
        let own = off[q].len();
        if own < 0.08 || alpha[q] == 0.0 {
            return 0.0;
        }
        let u = off[q] / own;
        let full = near(q % w, q / w, 2).into_iter().map(|n| alpha[n] * off[n].dot(u)).fold(own, f64::max);
        alpha[q] * own / full
    }).collect();
    let at = |i: i64, j: i64| cov[j.clamp(0, h as i64 - 1) as usize * w + i.clamp(0, w as i64 - 1) as usize];
    let d = detail.max(1);
    let (gw, gh) = (w * d, h * d);
    let mut on = vec![false; gw * gh];
    let mut rgb = vec![[0.0; 3]; gw * gh];
    for j in 0..gh {
        for i in 0..gw {
            let (x, y) = ((i as f64 + 0.5) / d as f64 - 0.5, (j as f64 + 0.5) / d as f64 - 0.5);
            let (x0, y0) = (x.floor(), y.floor());
            let (fx, fy) = (x - x0, y - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            let c = (at(x0, y0) * (1.0 - fx) + at(x0 + 1, y0) * fx) * (1.0 - fy) + (at(x0, y0 + 1) * (1.0 - fx) + at(x0 + 1, y0 + 1) * fx) * fy;
            if c < 0.5 {
                continue;
            }
            let (si, sj) = (i / d, j / d);
            let gap = |q: usize| (q % w).abs_diff(si).pow(2) + (q / w).abs_diff(sj).pow(2);
            let best = near(si, sj, 2).into_iter().filter(|&q| cov[q] >= 0.85).min_by_key(|&q| gap(q)).unwrap_or(sj * w + si);
            let p = px[best];
            on[j * gw + i] = true;
            rgb[j * gw + i] = [linear(p[0]), linear(p[1]), linear(p[2])];
        }
    }
    Ok(Pixels { w: gw, h: gh, on, rgb, aspect: h as f64 / w as f64, cov: cov.iter().map(|&c| c as f32).collect(), cw: w, ch: h })
}

pub fn load(path: &str, key: [f64; 3], detail: usize) -> Result<Arc<Pixels>, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Pixels>>>> = OnceLock::new();
    let id = format!("{path}|{key:?}|{detail}");
    let cache = CACHE.get_or_init(Default::default);
    if let Some(p) = cache.lock().unwrap().get(&id) {
        return Ok(p.clone());
    }
    let p = Arc::new(build(path, key, detail)?);
    cache.lock().unwrap().insert(id, p.clone());
    Ok(p)
}

impl Pixels {
    fn coverage(&self, x: f64, y: f64) -> f64 {
        let (x, y) = (x - 0.5, y - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |i: f64, j: f64| self.cov[(j as i64).clamp(0, self.ch as i64 - 1) as usize * self.cw + (i as i64).clamp(0, self.cw as i64 - 1) as usize] as f64;
        (at(x0, y0) * (1.0 - fx) + at(x0 + 1.0, y0) * fx) * (1.0 - fy) + (at(x0, y0 + 1.0) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0) * fx) * fy
    }

    /// The outward normal of a wall at local (x, y): down the smoothed coverage, so curved letters shade smoothly.
    fn wall(&self, x: f64, y: f64, half: V3, fallback: V3) -> V3 {
        let (u, v) = ((x + half.x) / (2.0 * half.x) * self.cw as f64, (half.y - y) / (2.0 * half.y) * self.ch as f64);
        let e = 0.75;
        let gu = self.coverage(u + e, v) - self.coverage(u - e, v);
        let gv = self.coverage(u, v + e) - self.coverage(u, v - e);
        let n = v3(-gu, gv, 0.0);
        if n.len() < 1e-3 { fallback } else { n.norm() }
    }

    fn cell(&self, i: i64, j: i64) -> bool {
        i >= 0 && j >= 0 && (i as usize) < self.w && (j as usize) < self.h && self.on[j as usize * self.w + i as usize]
    }

    /// The first column a ray meets inside the slab |x| <= hx, |y| <= hy, |z| <= hz (row 0 at +y), walking the grid.
    pub fn hit(&self, o: V3, d: V3, half: V3, tmin: f64, tmax: f64) -> Option<(f64, V3, V3)> {
        let (mut t0, mut t1, mut n0, mut inside) = (tmin, tmax, V3::default(), true);
        for a in 0..3 {
            let (oa, da, ha) = (o.get(a), d.get(a), half.get(a));
            if da.abs() < 1e-300 {
                if oa.abs() > ha {
                    return None;
                }
                continue;
            }
            let (ta, tb) = ((-ha - oa) / da, (ha - oa) / da);
            let (lo, hi) = if ta < tb { (ta, tb) } else { (tb, ta) };
            if lo > t0 {
                t0 = lo;
                let mut n = [0.0; 3];
                n[a] = -da.signum();
                n0 = V3::from(n);
                inside = false;
            }
            t1 = t1.min(hi);
            if t0 > t1 {
                return None;
            }
        }
        let (cw, ch) = (2.0 * half.x / self.w as f64, 2.0 * half.y / self.h as f64);
        let gx = |x: f64| (x + half.x) / cw;
        let gy = |y: f64| (half.y - y) / ch;
        let p = o + d * t0;
        let (mut i, mut j) = ((gx(p.x).floor() as i64).clamp(0, self.w as i64 - 1), (gy(p.y).floor() as i64).clamp(0, self.h as i64 - 1));
        let (si, sj) = (if d.x > 0.0 { 1 } else { -1 }, if d.y < 0.0 { 1 } else { -1 });
        let next = |c: i64, s: i64, size: f64, lo: f64, dir: f64, org: f64| -> f64 {
            if dir.abs() < 1e-300 {
                return f64::INFINITY;
            }
            let edge = lo + (c + (s > 0) as i64) as f64 * size;
            (edge - org) / dir
        };
        let mut tx = next(i, si, cw, -half.x, d.x, o.x);
        let mut ty = next(j, sj, -ch, half.y, d.y, o.y);
        let (dtx, dty) = ((cw / d.x).abs(), (ch / d.y).abs());
        let (mut t, mut n) = (t0, n0);
        let mut first = true;
        while t <= t1 {
            if self.cell(i, j) && !(first && inside) {
                let c = self.rgb[j as usize * self.w + i as usize];
                let p = o + d * t;
                let n = if n.z == 0.0 && !(first && n0.z == 0.0 && (p.x.abs() >= half.x * 0.999 || p.y.abs() >= half.y * 0.999)) { self.wall(p.x, p.y, half, n) } else { n };
                return Some((t, n, v3(c[0] as f64, c[1] as f64, c[2] as f64)));
            }
            first = false;
            if tx < ty {
                t = tx;
                tx += dtx;
                i += si;
                n = v3(-si as f64, 0.0, 0.0);
            } else {
                t = ty;
                ty += dty;
                j += sj;
                n = v3(0.0, sj as f64, 0.0);
            }
            if i < 0 || j < 0 || i as usize >= self.w || j as usize >= self.h {
                return None;
            }
        }
        None
    }

    /// Row runs of solid cells as boxes (centre, half size) in the slab, for export.
    pub fn runs(&self, half: V3) -> Vec<(V3, V3)> {
        let (cw, ch) = (2.0 * half.x / self.w as f64, 2.0 * half.y / self.h as f64);
        let mut out = vec![];
        for j in 0..self.h {
            let mut i = 0;
            while i < self.w {
                if !self.on[j * self.w + i] {
                    i += 1;
                    continue;
                }
                let s = i;
                while i < self.w && self.on[j * self.w + i] {
                    i += 1;
                }
                let x = -half.x + (s + i) as f64 * cw / 2.0;
                out.push((v3(x, half.y - (j as f64 + 0.5) * ch, 0.0), v3((i - s) as f64 * cw / 2.0, ch / 2.0, half.z)));
            }
        }
        out
    }
}
