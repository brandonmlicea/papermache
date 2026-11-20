use crate::math::*;
use crate::image::Pixels;
use crate::scene::Material;
use crate::text::Mask;
use std::sync::Arc;

/// A mesh's per-vertex normals and UVs in world space, its faces' materials, and the way back to its own space.
pub struct Shade {
    pub n: Vec<V3>,
    pub uv: Vec<[f64; 2]>,
    pub f: Vec<[u32; 3]>,
    pub fm: Vec<u16>,
    pub mats: Vec<(String, Material)>,
    pub local: Affine,
}

/// A shape in its own local space; `Tri` alone is stored in world space.
#[derive(Clone)]
pub enum Kind {
    Box(V3),
    RBox(V3, f64),
    Cyl { r0: f64, r1: f64, h2: f64 },
    Sphere(f64),
    Torus(f64, f64),
    Plane(f64, f64),
    Text { lo: (f64, f64), hi: (f64, f64), mask: Arc<Mask> },
    Image { half: V3, px: Arc<Pixels> },
    Tri { a: V3, e1: V3, e2: V3, n: V3, sh: Option<(Arc<Shade>, u32)> },
}

pub struct Prim {
    pub kind: Kind,
    pub inv: Affine,
    pub mat: u32,
    pub obj: u32,
    pub bounds: Aabb,
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub t: f64,
    pub n: V3,
    pub col: Option<V3>,
    pub uv: [f64; 2],
}

impl Kind {
    pub fn local_bounds(&self) -> Aabb {
        let b = |x: f64, y: f64, z: f64| Aabb::new(v3(-x, -y, -z), v3(x, y, z));
        match self {
            Kind::Box(h) | Kind::RBox(h, _) | Kind::Image { half: h, .. } => Aabb::new(-*h, *h),
            Kind::Cyl { r0, r1, h2 } => b(r0.max(*r1), r0.max(*r1), *h2),
            Kind::Sphere(r) => b(*r, *r, *r),
            Kind::Torus(big, r) => b(big + r, big + r, *r),
            Kind::Plane(w, h) => b(*w, *h, 0.0),
            Kind::Text { lo, hi, .. } => Aabb::new(v3(lo.0, lo.1, 0.0), v3(hi.0, hi.1, 0.0)),
            Kind::Tri { a, e1, e2, .. } => Aabb::new(*a, *a).grow(*a + *e1).grow(*a + *e2),
        }
    }
}

impl Prim {
    pub fn new(kind: Kind, world: &Affine, mat: u32, obj: u32) -> Prim {
        let lb = kind.local_bounds();
        let bounds = if matches!(kind, Kind::Tri { .. }) { lb } else { lb.transform(world) };
        let pad = 1e-6 * (1.0 + (bounds.hi - bounds.lo).max_c());
        Prim { kind, inv: world.inverse(), mat, obj, bounds: bounds.pad(pad) }
    }

    pub fn hit(&self, o: V3, d: V3, tmin: f64, tmax: f64) -> Option<Hit> {
        if let Kind::Tri { a, e1, e2, n, .. } = &self.kind {
            return tri(o, d, *a, *e1, *e2, tmin, tmax).map(|(t, u, v)| Hit { t, n: *n, col: None, uv: [u, v] });
        }
        let (lo, ld) = (self.inv.apply(o), self.inv.dir(d));
        if let Kind::Image { half, px } = &self.kind {
            return px.hit(lo, ld, *half, tmin, tmax).map(|(t, n, c)| Hit { t, n: self.inv.tdir(n).norm(), col: Some(c), uv: [0.0; 2] });
        }
        let (t, n) = match &self.kind {
            Kind::Box(h) => hit_box(lo, ld, *h, tmin, tmax),
            Kind::Cyl { r0, r1, h2 } => hit_cyl(lo, ld, *r0, *r1, *h2, tmin, tmax),
            Kind::Sphere(r) => hit_sphere(lo, ld, *r, tmin, tmax),
            Kind::Plane(w, h) => hit_plane(lo, ld, tmin, tmax).filter(|(t, _)| {
                let p = lo + ld * *t;
                p.x.abs() <= *w && p.y.abs() <= *h
            }),
            Kind::Text { lo: a, hi: b, mask } => hit_plane(lo, ld, tmin, tmax).filter(|(t, _)| {
                let p = lo + ld * *t;
                p.x >= a.0 && p.x <= b.0 && p.y >= a.1 && p.y <= b.1
                    && mask.sample((p.x - a.0) / (b.0 - a.0) * mask.w as f64, (b.1 - p.y) / (b.1 - a.1) * mask.h as f64) >= 0.5
            }),
            Kind::RBox(h, r) => {
                let (h, r) = (*h, *r);
                march(lo, ld, h, tmin, tmax, &|p: V3| {
                    let q = p.abs() - h + V3::splat(r);
                    q.max(V3::default()).len() + q.max_c().min(0.0) - r
                })
            }
            Kind::Torus(big, r) => {
                let (big, r) = (*big, *r);
                march(lo, ld, v3(big + r, big + r, r), tmin, tmax, &|p: V3| ((p.x * p.x + p.y * p.y).sqrt() - big).hypot(p.z) - r)
            }
            Kind::Tri { .. } | Kind::Image { .. } => unreachable!(),
        }?;
        Some(Hit { t, n: self.inv.tdir(n).norm(), col: None, uv: [0.0; 2] })
    }
}

fn slab(o: V3, d: V3, lo: V3, hi: V3) -> Option<(f64, f64)> {
    let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
    for i in 0..3 {
        let (oi, di) = (o.get(i), d.get(i));
        if di.abs() < 1e-300 {
            if oi < lo.get(i) || oi > hi.get(i) {
                return None;
            }
            continue;
        }
        let (mut a, mut b) = ((lo.get(i) - oi) / di, (hi.get(i) - oi) / di);
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        t0 = t0.max(a);
        t1 = t1.min(b);
        if t0 > t1 {
            return None;
        }
    }
    Some((t0, t1))
}

fn pick(t0: f64, t1: f64, tmin: f64, tmax: f64) -> Option<f64> {
    let t = if t0 > tmin { t0 } else if t1 > tmin { t1 } else { return None };
    (t < tmax).then_some(t)
}

pub fn hit_box(o: V3, d: V3, h: V3, tmin: f64, tmax: f64) -> Option<(f64, V3)> {
    let (t0, t1) = slab(o, d, -h, h)?;
    let t = pick(t0, t1, tmin, tmax)?;
    let p = o + d * t;
    let q = [p.x / h.x.max(1e-12), p.y / h.y.max(1e-12), p.z / h.z.max(1e-12)];
    let i = (0..3).max_by(|&a, &b| q[a].abs().total_cmp(&q[b].abs())).unwrap();
    let mut n = [0.0; 3];
    n[i] = q[i].signum();
    Some((t, V3::from(n)))
}

fn take(best: &mut Option<(f64, V3)>, t: f64, n: V3, tmin: f64, tmax: f64) {
    if t > tmin && t < tmax && best.is_none_or(|(b, _)| t < b) {
        *best = Some((t, n));
    }
}

pub fn hit_cyl(o: V3, d: V3, r0: f64, r1: f64, h2: f64, tmin: f64, tmax: f64) -> Option<(f64, V3)> {
    let b = (r1 - r0) / (2.0 * h2);
    let a0 = r0 + b * h2;
    let k = a0 + b * o.z;
    let qa = d.x * d.x + d.y * d.y - b * b * d.z * d.z;
    let qb = o.x * d.x + o.y * d.y - b * k * d.z;
    let qc = o.x * o.x + o.y * o.y - k * k;
    let mut best = None;
    if qa.abs() > 1e-300 {
        let disc = qb * qb - qa * qc;
        if disc >= 0.0 {
            let s = disc.sqrt();
            for t in [(-qb - s) / qa, (-qb + s) / qa] {
                let p = o + d * t;
                let rz = a0 + b * p.z;
                if p.z.abs() <= h2 && rz >= 0.0 {
                    take(&mut best, t, v3(p.x, p.y, -b * rz), tmin, tmax);
                }
            }
        }
    }
    if d.z.abs() > 1e-300 {
        for (z, r, nz) in [(-h2, r0, -1.0), (h2, r1, 1.0)] {
            let t = (z - o.z) / d.z;
            let p = o + d * t;
            if r > 0.0 && p.x * p.x + p.y * p.y <= r * r {
                take(&mut best, t, v3(0.0, 0.0, nz), tmin, tmax);
            }
        }
    }
    best
}

pub fn hit_sphere(o: V3, d: V3, r: f64, tmin: f64, tmax: f64) -> Option<(f64, V3)> {
    let (a, b, c) = (d.dot(d), o.dot(d), o.dot(o) - r * r);
    let disc = b * b - a * c;
    if disc < 0.0 {
        return None;
    }
    let s = disc.sqrt();
    let t = pick((-b - s) / a, (-b + s) / a, tmin, tmax)?;
    Some((t, o + d * t))
}

fn hit_plane(o: V3, d: V3, tmin: f64, tmax: f64) -> Option<(f64, V3)> {
    if d.z.abs() < 1e-300 {
        return None;
    }
    let t = -o.z / d.z;
    (t > tmin && t < tmax).then_some((t, v3(0.0, 0.0, 1.0)))
}

fn tri(o: V3, d: V3, a: V3, e1: V3, e2: V3, tmin: f64, tmax: f64) -> Option<(f64, f64, f64)> {
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-300 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > tmin && t < tmax).then_some((t, u, v))
}

/// Sphere tracing a signed distance inside the shape's bounds, for bevels and tori.
fn march(o: V3, d: V3, half: V3, tmin: f64, tmax: f64, sdf: &dyn Fn(V3) -> f64) -> Option<(f64, V3)> {
    let scale = half.max_c();
    let eps = scale * 2e-6 + 1e-9;
    let (t0, t1) = slab(o, d, -half * 1.001, half * 1.001)?;
    let dl = d.len();
    let dn = d / dl;
    let mut s = t0.max(tmin) * dl;
    let end = t1.min(tmax) * dl;
    let mut leaving = sdf(o + dn * s) < eps;
    for _ in 0..400 {
        if s > end {
            return None;
        }
        let dist = sdf(o + dn * s);
        if leaving {
            if dist > eps {
                leaving = false;
            }
            s += dist.abs().max(eps * 4.0);
            continue;
        }
        if dist < eps {
            let p = o + dn * s;
            let h = scale * 1e-4 + 1e-9;
            let g = |a: V3| sdf(p + a) - sdf(p - a);
            let n = v3(g(v3(h, 0.0, 0.0)), g(v3(0.0, h, 0.0)), g(v3(0.0, 0.0, h))).norm();
            return Some((s / dl, n));
        }
        s += dist;
    }
    None
}
