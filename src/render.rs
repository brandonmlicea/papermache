use crate::math::*;
use crate::scene::{Camera, Compiled, Material, Scene};
use rayon::prelude::*;
use serde_json::Value;
use std::f64::consts::PI;

const EPS: f64 = 1e-6;
const LIFT: f64 = 1e-4;

pub struct Cam {
    eye: V3,
    f: V3,
    r: V3,
    u: V3,
    ortho: bool,
    sx: f64,
    sy: f64,
    w: f64,
    h: f64,
}

impl Cam {
    pub fn new(c: &Camera, size: [u32; 2]) -> Cam {
        let eye = V3::from(c.eye);
        let f = (V3::from(c.look) - eye).norm();
        let mut up = c.up.map(V3::from).unwrap_or(v3(0.0, 0.0, 1.0));
        for alt in [v3(0.0, 1.0, 0.0), v3(1.0, 0.0, 0.0)] {
            if f.cross(up).len() < 1e-9 {
                up = alt;
            }
        }
        let r = f.cross(up).norm();
        let u = r.cross(f);
        let (w, h) = (size[0].max(1) as f64, size[1].max(1) as f64);
        let ortho = c.projection.starts_with("ortho");
        let half = if ortho {
            c.width / 2.0
        } else if let Some(fov) = c.fov {
            (fov.to_radians() / 2.0).tan()
        } else {
            18.0 / c.lens
        };
        let (sx, sy) = if w >= h { (half, half * h / w) } else { (half * w / h, half) };
        Cam { eye, f, r, u, ortho, sx, sy, w, h }
    }

    pub fn ray(&self, px: f64, py: f64) -> (V3, V3) {
        let x = (2.0 * px / self.w - 1.0) * self.sx;
        let y = (1.0 - 2.0 * py / self.h) * self.sy;
        if self.ortho {
            (self.eye + self.r * x + self.u * y, self.f)
        } else {
            (self.eye, (self.f + self.r * x + self.u * y).norm())
        }
    }

    pub fn project(&self, p: V3) -> [f64; 2] {
        let v = p - self.eye;
        let (mut x, mut y) = (v.dot(self.r), v.dot(self.u));
        if !self.ortho {
            let z = v.dot(self.f);
            x /= z;
            y /= z;
        }
        [(x / self.sx + 1.0) / 2.0 * self.w, (1.0 - y / self.sy) / 2.0 * self.h]
    }
}

struct Env<'a> {
    c: &'a Compiled,
    sun: V3,
    cos_max: f64,
    sun_e: f64,
    sky: V3,
    bg: Option<V3>,
}

fn ggx(n: V3, v: V3, l: V3, rough: f64, f0: V3, ndv: f64, ndl: f64) -> V3 {
    let h = (v + l).norm();
    let a = (rough * rough).max(0.02);
    let a2 = a * a;
    let ndh = n.dot(h).max(0.0);
    let dd = ndh * ndh * (a2 - 1.0) + 1.0;
    let d = a2 / (PI * dd * dd);
    let k = a / 2.0;
    let g = ndv / (ndv * (1.0 - k) + k) * ndl / (ndl * (1.0 - k) + k);
    let f = f0 + (V3::splat(1.0) - f0) * (1.0 - v.dot(h).max(0.0)).powi(5);
    f * (d * g / (4.0 * ndv * ndl)).min(60.0)
}

impl Env<'_> {
    fn mat(&self, prim: u32) -> &Material {
        &self.c.mats[self.c.prims[prim as usize].mat as usize]
    }

    fn transmit(&self, o: V3, d: V3) -> f64 {
        let mut t = 1.0;
        self.c.bvh.each(&self.c.prims, o, d, EPS, f64::INFINITY, &mut |i, _| {
            t *= 1.0 - self.mat(i).opacity.clamp(0.0, 1.0);
            t <= 1e-3
        });
        if t <= 1e-3 { 0.0 } else { t }
    }

    fn trace(&self, o: V3, d: V3, depth: u32, rng: &mut Rng, primary: bool) -> (V3, f64) {
        let Some((h, pi)) = self.c.bvh.closest(&self.c.prims, o, d, EPS, f64::INFINITY) else {
            return match (primary, self.bg) {
                (false, _) => (self.sky, 1.0),
                (true, Some(c)) => (c, 1.0),
                (true, None) => (V3::default(), 0.0),
            };
        };
        let own;
        let m = match h.col {
            Some(c) => {
                own = Material { color: c.arr(), ..self.mat(pi).clone() };
                &own
            }
            None => self.mat(pi),
        };
        let p = o + d * h.t;
        let n = if h.n.dot(d) > 0.0 { -h.n } else { h.n };
        let mut col = self.shade(p, n, d, m, depth, rng);
        let mut alpha = 1.0;
        let op = m.opacity.clamp(0.0, 1.0);
        if op < 1.0 && depth < 8 {
            let (bc, ba) = self.trace(p + d * LIFT, d, depth + 1, rng, primary);
            col = col * op + bc * (1.0 - op);
            alpha = op + (1.0 - op) * ba;
        }
        (col, alpha)
    }

    fn shade(&self, p: V3, n: V3, d: V3, m: &Material, depth: u32, rng: &mut Rng) -> V3 {
        let base = V3::from(m.color);
        let met = m.metallic.clamp(0.0, 1.0);
        let rough = m.roughness.clamp(0.02, 1.0);
        let diff = base * (1.0 - met);
        let f0 = V3::splat(0.04) * (1.0 - met) + base * met;
        let v = -d;
        let ndv = n.dot(v).max(1e-4);
        let po = p + n * LIFT;
        let mut out = V3::from(m.emission);
        if self.sun_e > 0.0 {
            let l = cone_dir(self.sun, self.cos_max, rng);
            let ndl = n.dot(l);
            if ndl > 0.0 {
                let vis = self.transmit(po, l);
                if vis > 0.0 {
                    out += (diff / PI + ggx(n, v, l, rough, f0, ndv, ndl)) * (self.sun_e * ndl * vis);
                }
            }
        }
        let w = cosine_dir(n, rng);
        let open = match self.c.bvh.closest(&self.c.prims, po, w, EPS, f64::INFINITY) {
            None => {
                out += diff * self.sky;
                true
            }
            Some((_, q)) => {
                let q = self.mat(q);
                out += diff * V3::from(q.color) * (1.0 - q.metallic) * self.sky * 0.35;
                false
            }
        };
        let fr = f0 + (V3::splat(1.0) - f0) * (1.0 - ndv).powi(5);
        if (met > 0.05 || rough < 0.3) && depth < 2 {
            let r = d - n * (2.0 * d.dot(n));
            let mut rr = (r + in_sphere(rng) * (rough * rough)).norm();
            if rr.dot(n) <= 0.0 {
                rr = r;
            }
            let (lc, _) = self.trace(po, rr, depth + 1, rng, false);
            out += fr * lc;
        } else if open {
            out += fr * self.sky;
        }
        out
    }
}

fn srgb(c: f64) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

/// RGBA8 pixels of the scene seen by its camera at `size`.
pub fn render(s: &Scene, c: &Compiled, size: [u32; 2], samples: u32) -> Vec<u8> {
    let cam = Cam::new(&s.camera, size);
    let l = &s.lights;
    let sky = V3::from(l.sky.color) * l.sky.strength;
    let bg = match &l.background {
        Some(Value::String(t)) if t == "transparent" => None,
        Some(Value::Array(a)) if a.len() >= 3 => Some(v3(a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0), a[2].as_f64().unwrap_or(0.0))),
        _ => Some(sky),
    };
    let env = Env {
        c,
        sun: V3::from(l.sun.direction).norm(),
        cos_max: (l.sun.angle.to_radians() / 2.0).cos(),
        sun_e: l.sun.strength,
        sky,
        bg,
    };
    let (w, h) = (size[0] as usize, size[1] as usize);
    let n = samples.max(1);
    let mut img = vec![0u8; w * h * 4];
    img.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let mut rng = Rng::new(((y as u64) << 32) ^ x as u64);
            let (ox, oy) = (rng.f(), rng.f());
            let (mut acc, mut a) = (V3::default(), 0.0);
            for i in 0..n {
                let jx = (ox + i as f64 * 0.754_877_666_246_692_7).fract();
                let jy = (oy + i as f64 * 0.569_840_290_998_053_3).fract();
                let (o, d) = cam.ray(x as f64 + jx, y as f64 + jy);
                let (col, al) = env.trace(o, d, 0, &mut rng, true);
                acc += col * al;
                a += al;
            }
            let c = if a > 0.0 { acc / a } else { V3::default() };
            let px = &mut row[x * 4..x * 4 + 4];
            px.copy_from_slice(&[srgb(c.x), srgb(c.y), srgb(c.z), (a / n as f64 * 255.0 + 0.5) as u8]);
        }
    });
    img
}

pub fn png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut e = png::Encoder::new(&mut buf, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().expect("png header");
        wr.write_image_data(rgba).expect("png data");
    }
    buf
}

/// Area-average downscale of RGBA8 pixels.
pub fn shrink(rgba: &[u8], w: u32, h: u32, nw: u32, nh: u32) -> Vec<u8> {
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let (x0, x1) = (x * w / nw, ((x + 1) * w / nw).max(x * w / nw + 1));
            let (y0, y1) = (y * h / nh, ((y + 1) * h / nh).max(y * h / nh + 1));
            let mut s = [0u32; 4];
            for yy in y0..y1.min(h) {
                for xx in x0..x1.min(w) {
                    for k in 0..4 {
                        s[k] += rgba[((yy * w + xx) * 4) as usize + k] as u32;
                    }
                }
            }
            let cnt = ((x1.min(w) - x0) * (y1.min(h) - y0)).max(1);
            for k in 0..4 {
                out[((y * nw + x) * 4) as usize + k] = (s[k] / cnt) as u8;
            }
        }
    }
    out
}

/// The size to render: the camera's, times scale, fitted inside max_size if given.
pub fn fit(size: [u32; 2], max: Option<u32>) -> [u32; 2] {
    match max {
        Some(m) if size[0].max(size[1]) > m => {
            let k = m as f64 / size[0].max(size[1]) as f64;
            [((size[0] as f64 * k).round() as u32).max(1), ((size[1] as f64 * k).round() as u32).max(1)]
        }
        _ => size,
    }
}
