//! What one ray sees: the surface it meets after textures and patterns, lit by the sun, sky and lamps, with
//! reflections through a base and a clear coat.
use crate::build::Compiled;
use crate::geom::{Hit, Kind};
use crate::math::*;
use std::f64::consts::{PI, TAU};

pub const EPS: f64 = 1e-6;
const LIFT: f64 = 1e-4;

/// A material at one point, after its texture, pattern and screen.
#[derive(Clone, Copy)]
pub struct Look {
    pub color: V3,
    pub metallic: f64,
    pub rough: f64,
    pub emission: V3,
    pub opacity: f64,
    pub coat: f64,
    pub coat_rough: f64,
}

pub struct Env<'a> {
    pub c: &'a Compiled,
    pub sun: V3,
    pub cos_max: f64,
    pub sun_e: f64,
    pub sky: V3,
    pub bg: Option<V3>,
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

fn uv_of(k: &Kind, p: V3) -> [f64; 2] {
    let around = |p: V3| p.y.atan2(p.x) / TAU + 0.5;
    match k {
        Kind::Plane(w, h) => [(p.x / w + 1.0) / 2.0, (p.y / h + 1.0) / 2.0],
        Kind::Box(h) | Kind::RBox(h, _) => {
            let q = v3(p.x / h.x, p.y / h.y, p.z / h.z);
            let (a, b) = if q.z.abs() >= q.x.abs().max(q.y.abs()) { (q.x, q.y) } else if q.y.abs() >= q.x.abs() { (q.x, q.z) } else { (q.y, q.z) };
            [(a + 1.0) / 2.0, (b + 1.0) / 2.0]
        }
        Kind::Cyl { h2, .. } => [around(p), (p.z / h2 + 1.0) / 2.0],
        Kind::Sphere(r) => [around(p), 1.0 - (p.z / r).clamp(-1.0, 1.0).acos() / PI],
        Kind::Text { lo, hi, .. } => [(p.x - lo.0) / (hi.0 - lo.0), (p.y - lo.1) / (hi.1 - lo.1)],
        _ => [around(p), 0.5],
    }
}

pub fn luma(c: V3) -> f64 {
    0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z
}

impl Env<'_> {
    fn opacity(&self, prim: u32) -> f64 {
        self.c.mats[self.c.prims[prim as usize].mat as usize].m.opacity
    }

    fn transmit(&self, o: V3, d: V3, tmax: f64) -> f64 {
        let mut t = 1.0;
        self.c.bvh.each(&self.c.prims, o, d, EPS, tmax, &mut |i, _| {
            t *= 1.0 - self.opacity(i).clamp(0.0, 1.0);
            t <= 1e-3
        });
        if t <= 1e-3 { 0.0 } else { t }
    }

    /// The shading normal (smooth on meshes), the geometric normal, and the look of what a ray hit.
    pub fn surface(&self, h: &Hit, pi: u32, p: V3) -> (V3, V3, Look) {
        let prim = &self.c.prims[pi as usize];
        let mat = &self.c.mats[prim.mat as usize];
        let (n, uv, local) = match &prim.kind {
            Kind::Tri { sh: Some((sh, i)), .. } => {
                let f = sh.f[(i & 0x7fff_ffff) as usize];
                let c = if i >> 31 == 1 { [f[0], f[2], f[1]] } else { f };
                let w = [1.0 - h.uv[0] - h.uv[1], h.uv[0], h.uv[1]];
                let n = if sh.n.is_empty() { h.n } else { (0..3).fold(V3::default(), |s, k| s + sh.n[c[k] as usize] * w[k]).norm() };
                let uv = if sh.uv.is_empty() { [0.0; 2] } else { (0..3).fold([0.0; 2], |s, k| [s[0] + sh.uv[c[k] as usize][0] * w[k], s[1] + sh.uv[c[k] as usize][1] * w[k]]) };
                (n, uv, sh.local.apply(p))
            }
            Kind::Tri { .. } => (h.n, [0.0; 2], p),
            k => {
                let lp = prim.inv.apply(p);
                (h.n, uv_of(k, lp), lp)
            }
        };
        let m = &mat.m;
        let mut color = h.col.unwrap_or(V3::from(m.color));
        let uv = [uv[0] * m.repeat[0], uv[1] * m.repeat[1]];
        if let Some(t) = &mat.tex {
            color = color * t.sample(uv);
        }
        let mut rough = m.roughness;
        if let Some(pt) = &m.pattern {
            (color, rough) = pt.at(local, color, rough);
        }
        let mut emission = V3::from(m.emission);
        if let (Some(t), Some(s)) = (&mat.screen, &m.screen) {
            emission += t.sample(uv) * s.strength;
        }
        let look = Look { color, metallic: m.metallic, rough, emission, opacity: m.opacity, coat: m.coat.clamp(0.0, 1.0), coat_rough: m.coat_roughness };
        (n, h.n, look)
    }

    pub fn trace(&self, o: V3, d: V3, depth: u32, rng: &mut Rng, primary: bool) -> (V3, f64) {
        let hit = self.c.bvh.closest(&self.c.prims, o, d, EPS, f64::INFINITY);
        let mut near = hit.map_or(f64::INFINITY, |(h, _)| h.t);
        let mut lamp = None;
        for l in self.c.lamps.iter().filter(|_| !primary || depth > 0) {
            if let Some((t, e)) = l.seen(o, d, near) {
                near = t;
                lamp = Some(e);
            }
        }
        if let Some(e) = lamp {
            return (e.min(V3::splat(40.0)), 1.0);
        }
        let Some((h, pi)) = hit else {
            return match (primary, self.bg) {
                (false, _) => (self.sky, 1.0),
                (true, Some(c)) => (c, 1.0),
                (true, None) => (V3::default(), 0.0),
            };
        };
        let p = o + d * h.t;
        let (n, ng, m) = self.surface(&h, pi, p);
        let ng = if ng.dot(d) > 0.0 { -ng } else { ng };
        let n = if n.dot(ng) < 0.0 { -n } else { n };
        let mut col = self.shade(p, n, ng, d, &m, depth, rng);
        let mut alpha = 1.0;
        let op = m.opacity.clamp(0.0, 1.0);
        if op < 1.0 && depth < 8 {
            let (bc, ba) = self.trace(p + d * LIFT, d, depth + 1, rng, primary);
            col = col * op + bc * (1.0 - op);
            alpha = op + (1.0 - op) * ba;
        }
        (col, alpha)
    }

    fn shade(&self, p: V3, n: V3, ng: V3, d: V3, m: &Look, depth: u32, rng: &mut Rng) -> V3 {
        let met = m.metallic.clamp(0.0, 1.0);
        let rough = m.rough.clamp(0.02, 1.0);
        let diff = m.color * (1.0 - met);
        let f0 = V3::splat(0.04) * (1.0 - met) + m.color * met;
        let v = -d;
        let ndv = n.dot(v).max(1e-4);
        let po = p + ng * LIFT;
        let mut out = m.emission;
        let shiny = met > 0.05 || rough < 0.3;
        let gloss = (shiny || m.coat > 0.0) && depth < 2;
        let cf = m.coat * (0.04 + 0.96 * (1.0 - ndv).powi(5));
        let brdf = |l: V3, ndl: f64, spec: bool| {
            let mut f = diff / PI * (1.0 - cf);
            if spec {
                f += ggx(n, v, l, rough, f0, ndv, ndl) * (1.0 - cf);
                if m.coat > 0.0 {
                    f += ggx(n, v, l, m.coat_rough.clamp(0.02, 1.0), V3::splat(0.04), ndv, ndl) * m.coat;
                }
            }
            f
        };
        if self.sun_e > 0.0 {
            let l = cone_dir(self.sun, self.cos_max, rng);
            let ndl = n.dot(l);
            if ndl > 0.0 && ng.dot(l) > 0.0 {
                let vis = self.transmit(po, l, f64::INFINITY);
                if vis > 0.0 {
                    out += brdf(l, ndl, true) * (self.sun_e * ndl * vis);
                }
            }
        }
        for lamp in &self.c.lamps {
            if let Some((l, dist, e)) = lamp.sample(po, rng) {
                let ndl = n.dot(l);
                if ndl > 0.0 && ng.dot(l) > 0.0 {
                    let vis = self.transmit(po, l, dist * (1.0 - 1e-6));
                    if vis > 0.0 {
                        out += brdf(l, ndl, !(gloss && lamp.shows())) * e * (ndl * vis);
                    }
                }
            }
        }
        let w = cosine_dir(n, rng);
        let open = match self.c.bvh.closest(&self.c.prims, po, w, EPS, f64::INFINITY) {
            None => {
                out += diff * self.sky * (1.0 - cf);
                true
            }
            Some((_, q)) => {
                let q = &self.c.mats[self.c.prims[q as usize].mat as usize].m;
                out += diff * V3::from(q.color) * (1.0 - q.metallic) * self.sky * 0.35 * (1.0 - cf);
                false
            }
        };
        let fr = f0 + (V3::splat(1.0) - f0) * (1.0 - ndv).powi(5);
        if gloss {
            let bw = if shiny { luma(fr) * (1.0 - cf) } else { 0.0 };
            let pc = cf / (cf + bw).max(1e-9);
            let coat = rng.f() < pc;
            let spread = if coat { m.coat_rough.clamp(0.0, 1.0).powi(2) } else { rough * rough };
            let r = d - n * (2.0 * d.dot(n));
            let mut rr = (r + in_sphere(rng) * spread).norm();
            if rr.dot(ng) <= 0.0 {
                rr = r;
            }
            if rr.dot(ng) > 0.0 {
                let (lc, _) = self.trace(po, rr, depth + 1, rng, false);
                out += if coat { lc * (cf / pc) } else { fr * lc * ((1.0 - cf) / (1.0 - pc).max(1e-9)) };
            }
        } else if open {
            out += fr * self.sky;
        }
        out
    }
}
