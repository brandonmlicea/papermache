//! Meshes built from profiles: lathe, extrude, the backdrop sweep, and smooth normals.
use crate::math::*;
use crate::scene::Material;
use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, TAU};
use std::sync::{Arc, Mutex, OnceLock};

/// Triangles in local space; `n` and `uv` are per vertex when present, `fm` picks each face's slot in `mats`.
#[derive(Clone, Default)]
pub struct Mesh {
    pub v: Vec<V3>,
    pub f: Vec<[u32; 3]>,
    pub n: Vec<V3>,
    pub uv: Vec<[f64; 2]>,
    pub fm: Vec<u16>,
    pub mats: Vec<(String, Material)>,
}

impl Mesh {
    fn put(&mut self, p: V3, uv: [f64; 2]) -> u32 {
        self.v.push(p);
        self.uv.push(uv);
        self.v.len() as u32 - 1
    }

    fn tri(&mut self, a: u32, b: u32, c: u32) {
        if a != b && b != c && a != c {
            self.f.push([a, b, c]);
        }
    }

    fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.tri(a, b, c);
        self.tri(a, c, d);
    }

    pub fn face_normal(&self, f: [u32; 3]) -> V3 {
        let [a, b, c] = f.map(|i| self.v[i as usize]);
        (b - a).cross(c - a)
    }

    /// Flips every face when the closed mesh's signed volume is negative.
    pub fn outward(mut self) -> Mesh {
        let vol: f64 = self.f.iter().map(|f| self.v[f[0] as usize].dot(self.face_normal(*f))).sum();
        if vol < 0.0 {
            self.f.iter_mut().for_each(|f| f.swap(1, 2));
        }
        self
    }

    /// Per-corner normals: each corner averages the area-weighted normals of the faces at its position that turn
    /// less than `angle` degrees from its own face. Vertices are split per corner.
    pub fn smooth(&self, angle: f64) -> Mesh {
        let fns: Vec<V3> = self.f.iter().map(|f| self.face_normal(*f)).collect();
        let span = self.v.iter().fold(Aabb::EMPTY, |b, p| b.grow(*p));
        let q = ((span.hi - span.lo).len() * 1e-6).max(1e-12);
        let key = |p: V3| ((p.x / q).round() as i64, (p.y / q).round() as i64, (p.z / q).round() as i64);
        let mut at: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
        for (i, f) in self.f.iter().enumerate() {
            for &k in f {
                at.entry(key(self.v[k as usize])).or_default().push(i);
            }
        }
        let cos = angle.to_radians().cos();
        let mut m = Mesh { mats: self.mats.clone(), fm: self.fm.clone(), ..Mesh::default() };
        for (i, f) in self.f.iter().enumerate() {
            let own = fns[i].norm();
            for &k in f {
                let p = self.v[k as usize];
                let mut n = V3::default();
                for &j in &at[&key(p)] {
                    if fns[j].norm().dot(own) >= cos {
                        n += fns[j];
                    }
                }
                m.v.push(p);
                m.n.push(if n.len() > 0.0 { n.norm() } else { own });
                m.uv.push(self.uv.get(k as usize).copied().unwrap_or([0.0, 0.0]));
            }
            let b = i as u32 * 3;
            m.f.push([b, b + 1, b + 2]);
        }
        m
    }
}

/// A profile of [r, z] points turned around Z. `ridges` flutes cut `depth` deep into every point at least 90% of
/// the widest radius (or inside `band` [z0, z1]): knurled crowns, dials, bottle caps.
pub fn lathe(profile: &[[f64; 2]], segments: usize, ridges: usize, depth: f64, band: Option<[f64; 2]>) -> Mesh {
    let seg = segments.max(3).max(ridges * 8);
    let rmax = profile.iter().map(|p| p[0]).fold(0.0, f64::max);
    let mut len = vec![0.0];
    for w in profile.windows(2) {
        len.push(len.last().unwrap() + (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]));
    }
    let total = len.last().copied().unwrap_or(1.0).max(1e-12);
    let mut m = Mesh::default();
    let mut rings = vec![];
    for (i, p) in profile.iter().enumerate() {
        let v = len[i] / total;
        if p[0] <= 1e-9 {
            let c = m.put(v3(0.0, 0.0, p[1]), [0.5, v]);
            rings.push(vec![c; seg]);
            continue;
        }
        let cut = ridges > 0 && band.map_or(p[0] >= 0.9 * rmax, |b| p[1] >= b[0] && p[1] <= b[1]);
        rings.push((0..seg).map(|j| {
            let th = TAU * j as f64 / seg as f64;
            let r = if cut { p[0] - depth * (ridges as f64 * th / 2.0).sin().abs() } else { p[0] };
            m.put(v3(r * th.cos(), r * th.sin(), p[1]), [j as f64 / seg as f64, v])
        }).collect::<Vec<_>>());
    }
    for w in rings.windows(2) {
        for j in 0..seg {
            let k = (j + 1) % seg;
            m.quad(w[0][j], w[0][k], w[1][k], w[1][j]);
        }
    }
    let closed = profile.first().is_some_and(|p| p[0] <= 1e-9) && profile.last().is_some_and(|p| p[0] <= 1e-9);
    if closed { m.outward() } else { m }
}

fn ccw(mut p: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    p.dedup_by(|a, b| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9);
    if p.len() > 1 && (p[0][0] - p[p.len() - 1][0]).hypot(p[0][1] - p[p.len() - 1][1]) < 1e-9 {
        p.pop();
    }
    let area: f64 = (0..p.len()).map(|i| {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        a[0] * b[1] - b[0] * a[1]
    }).sum();
    if area < 0.0 {
        p.reverse();
    }
    p
}

/// The outline moved `s` inward along each corner's mitre.
fn inset(p: &[[f64; 2]], s: f64) -> Vec<[f64; 2]> {
    let n = p.len();
    let normal = |a: [f64; 2], b: [f64; 2]| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = dx.hypot(dy).max(1e-12);
        [dy / l, -dx / l]
    };
    (0..n).map(|i| {
        let (a, b) = (normal(p[(i + n - 1) % n], p[i]), normal(p[i], p[(i + 1) % n]));
        let k = 1.0 + a[0] * b[0] + a[1] * b[1];
        let k = s / k.max(0.25);
        [p[i][0] - (a[0] + b[0]) * k, p[i][1] - (a[1] + b[1]) * k]
    }).collect()
}

/// Ear clipping of a simple counter-clockwise polygon.
pub fn ears(p: &[[f64; 2]]) -> Vec<[usize; 3]> {
    let cross = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let mut idx: Vec<usize> = (0..p.len()).collect();
    let mut out = vec![];
    while idx.len() > 3 {
        let n = idx.len();
        let ear = (0..n).find(|&i| {
            let (a, b, c) = (idx[(i + n - 1) % n], idx[i], idx[(i + 1) % n]);
            cross(p[a], p[b], p[c]) > 1e-12
                && !idx.iter().any(|&q| q != a && q != b && q != c && cross(p[a], p[b], p[q]) >= 0.0 && cross(p[b], p[c], p[q]) >= 0.0 && cross(p[c], p[a], p[q]) >= 0.0)
        }).unwrap_or(0);
        out.push([idx[(ear + n - 1) % n], idx[ear], idx[(ear + 1) % n]]);
        idx.remove(ear);
    }
    if idx.len() == 3 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    out
}

/// A rounded rectangle, counter-clockwise, `segments` points a corner.
pub fn rounded_rect(size: [f64; 2], radius: f64, segments: usize) -> Vec<[f64; 2]> {
    let (w, h) = (size[0] / 2.0, size[1] / 2.0);
    let r = radius.clamp(0.0, w.min(h));
    let k = if r > 0.0 { segments.max(1) } else { 0 };
    let mut out = vec![];
    for (i, (cx, cy)) in [(w - r, -h + r), (w - r, h - r), (-w + r, h - r), (-w + r, -h + r)].into_iter().enumerate() {
        for s in 0..=k {
            let a = (i as f64 - 1.0) * FRAC_PI_2 + FRAC_PI_2 * s as f64 / k.max(1) as f64;
            out.push([cx + r * a.cos(), cy + r * a.sin()]);
        }
    }
    ccw(out)
}

fn arc(c: [f64; 2], r: f64, a0: f64, a1: f64, k: usize) -> Vec<[f64; 2]> {
    if r <= 0.0 {
        return vec![[c[0] + r * a0.to_radians().cos(), c[1] + r * a0.to_radians().sin()]];
    }
    (0..=k).map(|i| {
        let a = (a0 + (a1 - a0) * i as f64 / k as f64).to_radians();
        [c[0] + r * a.cos(), c[1] + r * a.sin()]
    }).collect()
}

/// An outline pushed `depth` along Z, centred, its edges rounded by `bevel`; `wall` hollows it into a frame.
pub fn extrude(outline: &[[f64; 2]], depth: f64, bevel: f64, segments: usize, wall: Option<f64>) -> Mesh {
    let p = ccw(outline.to_vec());
    let h = depth / 2.0;
    let w = wall.filter(|w| *w > 0.0);
    let b = bevel.clamp(0.0, h.min(w.map_or(f64::INFINITY, |w| w / 2.0)));
    let k = segments.max(1);
    let mut sec = arc([b, -h + b], b, -90.0, -180.0, k);
    sec.extend(arc([b, h - b], b, 180.0, 90.0, k));
    if let Some(w) = w {
        sec.extend(arc([w - b, h - b], b, 90.0, 0.0, k));
        sec.extend(arc([w - b, -h + b], b, 0.0, -90.0, k));
    }
    sec.dedup_by(|a, c| (a[0] - c[0]).hypot(a[1] - c[1]) < 1e-12);
    let span = p.iter().fold((f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY), |s, q| (s.0.min(q[0]), s.1.max(q[0]), s.2.min(q[1]), s.3.max(q[1])));
    let uv = |q: [f64; 2]| [(q[0] - span.0) / (span.1 - span.0).max(1e-12), (q[1] - span.2) / (span.3 - span.2).max(1e-12)];
    let mut m = Mesh::default();
    let rings: Vec<Vec<u32>> = sec.iter().map(|s| inset(&p, s[0]).into_iter().map(|q| m.put(v3(q[0], q[1], s[1]), uv(q))).collect()).collect();
    let n = p.len();
    let pairs = if w.is_some() { rings.len() } else { rings.len() - 1 };
    for i in 0..pairs {
        let (r0, r1) = (&rings[i], &rings[(i + 1) % rings.len()]);
        for j in 0..n {
            m.quad(r0[j], r0[(j + 1) % n], r1[(j + 1) % n], r1[j]);
        }
    }
    if w.is_none() {
        let tris = ears(&inset(&p, b));
        let (bottom, top) = (rings[0].clone(), rings[rings.len() - 1].clone());
        for t in tris {
            m.tri(top[t[0]], top[t[1]], top[t[2]]);
            m.tri(bottom[t[0]], bottom[t[2]], bottom[t[1]]);
        }
    }
    m.outward()
}

/// A studio cyclorama: a floor `size[1]` deep that sweeps up through `radius` into a back wall `size[2]` high, `size[0]` wide,
/// the floor at z 0 centred on the origin, the wall at +Y.
pub fn backdrop(size: [f64; 3], radius: f64) -> Mesh {
    let (w, d, h) = (size[0] / 2.0, size[1] / 2.0, size[2]);
    let r = radius.clamp(0.0, (2.0 * d).min(h));
    let mut prof = vec![[-d, 0.0]];
    prof.extend(arc([d - r, r], r, -90.0, 0.0, 24));
    prof.push([d, h]);
    prof.dedup_by(|a, c| (a[0] - c[0]).hypot(a[1] - c[1]) < 1e-12);
    let mut len = 0.0;
    let mut m = Mesh::default();
    let mut last = prof[0];
    let total = prof.windows(2).map(|q| (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1])).sum::<f64>().max(1e-12);
    let mut rows = vec![];
    for q in &prof {
        len += (q[0] - last[0]).hypot(q[1] - last[1]);
        last = *q;
        rows.push([m.put(v3(-w, q[0], q[1]), [0.0, len / total]), m.put(v3(w, q[0], q[1]), [1.0, len / total])]);
    }
    for r in rows.windows(2) {
        m.quad(r[0][0], r[0][1], r[1][1], r[1][0]);
    }
    m
}

/// Meshes built from a shape's JSON, kept so a film builds each one once.
pub fn cached(key: String, make: impl FnOnce() -> Result<Mesh, String>) -> Result<Arc<Mesh>, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Mesh>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(m) = cache.lock().unwrap().get(&key) {
        return Ok(m.clone());
    }
    let m = Arc::new(make()?);
    let mut c = cache.lock().unwrap();
    if c.len() > 256 {
        c.clear();
    }
    c.insert(key, m.clone());
    Ok(m)
}
