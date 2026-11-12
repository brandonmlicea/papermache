use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub const fn v3(x: f64, y: f64, z: f64) -> V3 {
    V3 { x, y, z }
}

impl V3 {
    pub fn from(a: [f64; 3]) -> V3 {
        v3(a[0], a[1], a[2])
    }
    pub fn arr(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
    pub fn splat(s: f64) -> V3 {
        v3(s, s, s)
    }
    pub fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: V3) -> V3 {
        v3(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }
    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn norm(self) -> V3 {
        let l = self.len();
        if l > 0.0 { self / l } else { self }
    }
    pub fn min(self, o: V3) -> V3 {
        v3(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }
    pub fn max(self, o: V3) -> V3 {
        v3(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }
    pub fn abs(self) -> V3 {
        v3(self.x.abs(), self.y.abs(), self.z.abs())
    }
    pub fn get(self, i: usize) -> f64 {
        match i {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }
    pub fn max_c(self) -> f64 {
        self.x.max(self.y).max(self.z)
    }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl AddAssign for V3 {
    fn add_assign(&mut self, o: V3) {
        *self = *self + o;
    }
}
impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        v3(-self.x, -self.y, -self.z)
    }
}
impl Mul<f64> for V3 {
    type Output = V3;
    fn mul(self, s: f64) -> V3 {
        v3(self.x * s, self.y * s, self.z * s)
    }
}
impl Mul<V3> for V3 {
    type Output = V3;
    fn mul(self, o: V3) -> V3 {
        v3(self.x * o.x, self.y * o.y, self.z * o.z)
    }
}
impl Div<f64> for V3 {
    type Output = V3;
    fn div(self, s: f64) -> V3 {
        v3(self.x / s, self.y / s, self.z / s)
    }
}

/// p' = m · p + t
#[derive(Clone, Copy, Debug)]
pub struct Affine {
    pub m: [[f64; 3]; 3],
    pub t: V3,
}

type M3 = [[f64; 3]; 3];

fn mul3(a: &M3, b: &M3) -> M3 {
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

/// Blender's XYZ euler: Rz · Ry · Rx, degrees.
pub fn rot_xyz(deg: [f64; 3]) -> M3 {
    let [a, b, c] = deg.map(f64::to_radians);
    let rx = [[1.0, 0.0, 0.0], [0.0, a.cos(), -a.sin()], [0.0, a.sin(), a.cos()]];
    let ry = [[b.cos(), 0.0, b.sin()], [0.0, 1.0, 0.0], [-b.sin(), 0.0, b.cos()]];
    let rz = [[c.cos(), -c.sin(), 0.0], [c.sin(), c.cos(), 0.0], [0.0, 0.0, 1.0]];
    mul3(&rz, &mul3(&ry, &rx))
}

impl Affine {
    pub const ID: Affine = Affine { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: v3(0.0, 0.0, 0.0) };

    pub fn compose(pos: [f64; 3], rot: [f64; 3], scale: [f64; 3]) -> Affine {
        let r = rot_xyz(rot);
        let s = scale.map(|s| if s.abs() < 1e-9 { 1e-9 } else { s });
        let mut m = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] = r[i][j] * s[j];
            }
        }
        Affine { m, t: V3::from(pos) }
    }
    /// Columns are the images of local x, y and z.
    pub fn frame(origin: V3, x: V3, y: V3, z: V3) -> Affine {
        Affine { m: [[x.x, y.x, z.x], [x.y, y.y, z.y], [x.z, y.z, z.z]], t: origin }
    }
    pub fn translate(t: V3) -> Affine {
        Affine { t, ..Affine::ID }
    }
    pub fn dir(&self, d: V3) -> V3 {
        let m = &self.m;
        v3(m[0][0] * d.x + m[0][1] * d.y + m[0][2] * d.z, m[1][0] * d.x + m[1][1] * d.y + m[1][2] * d.z, m[2][0] * d.x + m[2][1] * d.y + m[2][2] * d.z)
    }
    pub fn tdir(&self, d: V3) -> V3 {
        let m = &self.m;
        v3(m[0][0] * d.x + m[1][0] * d.y + m[2][0] * d.z, m[0][1] * d.x + m[1][1] * d.y + m[2][1] * d.z, m[0][2] * d.x + m[1][2] * d.y + m[2][2] * d.z)
    }
    pub fn apply(&self, p: V3) -> V3 {
        self.dir(p) + self.t
    }
    /// self ∘ o: apply o, then self.
    pub fn then(&self, o: &Affine) -> Affine {
        Affine { m: mul3(&self.m, &o.m), t: self.apply(o.t) }
    }
    pub fn det(&self) -> f64 {
        let m = &self.m;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }
    pub fn inverse(&self) -> Affine {
        let m = &self.m;
        let mut d = self.det();
        if d.abs() < 1e-300 {
            d = 1e-300;
        }
        let i = [
            [(m[1][1] * m[2][2] - m[1][2] * m[2][1]) / d, (m[0][2] * m[2][1] - m[0][1] * m[2][2]) / d, (m[0][1] * m[1][2] - m[0][2] * m[1][1]) / d],
            [(m[1][2] * m[2][0] - m[1][0] * m[2][2]) / d, (m[0][0] * m[2][2] - m[0][2] * m[2][0]) / d, (m[0][2] * m[1][0] - m[0][0] * m[1][2]) / d],
            [(m[1][0] * m[2][1] - m[1][1] * m[2][0]) / d, (m[0][1] * m[2][0] - m[0][0] * m[2][1]) / d, (m[0][0] * m[1][1] - m[0][1] * m[1][0]) / d],
        ];
        let inv = Affine { m: i, t: V3::default() };
        Affine { t: -inv.dir(self.t), ..inv }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub lo: V3,
    pub hi: V3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb { lo: V3::splat_const(f64::INFINITY), hi: V3::splat_const(f64::NEG_INFINITY) };

    pub fn new(lo: V3, hi: V3) -> Aabb {
        Aabb { lo, hi }
    }
    pub fn union(self, o: Aabb) -> Aabb {
        Aabb { lo: self.lo.min(o.lo), hi: self.hi.max(o.hi) }
    }
    pub fn grow(self, p: V3) -> Aabb {
        Aabb { lo: self.lo.min(p), hi: self.hi.max(p) }
    }
    pub fn pad(self, e: f64) -> Aabb {
        Aabb { lo: self.lo - V3::splat(e), hi: self.hi + V3::splat(e) }
    }
    pub fn is_empty(&self) -> bool {
        self.lo.x > self.hi.x
    }
    pub fn centroid(&self) -> V3 {
        (self.lo + self.hi) * 0.5
    }
    pub fn area(&self) -> f64 {
        if self.is_empty() {
            return 0.0;
        }
        let d = self.hi - self.lo;
        2.0 * (d.x * d.y + d.y * d.z + d.z * d.x)
    }
    pub fn transform(&self, a: &Affine) -> Aabb {
        let mut b = Aabb::EMPTY;
        for i in 0..8 {
            let c = v3(if i & 1 == 0 { self.lo.x } else { self.hi.x }, if i & 2 == 0 { self.lo.y } else { self.hi.y }, if i & 4 == 0 { self.lo.z } else { self.hi.z });
            b = b.grow(a.apply(c));
        }
        b
    }
    pub fn hit(&self, o: V3, inv: V3, tmax: f64) -> Option<f64> {
        let (a, b) = ((self.lo - o) * inv, (self.hi - o) * inv);
        let t0 = a.min(b).max_c().max(0.0);
        let hi = a.max(b);
        let t1 = hi.x.min(hi.y).min(hi.z).min(tmax);
        (t0 <= t1).then_some(t0)
    }
    pub fn to_json(&self) -> [[f64; 3]; 2] {
        [self.lo.arr(), self.hi.arr()]
    }
}

impl V3 {
    pub const fn splat_const(s: f64) -> V3 {
        v3(s, s, s)
    }
}

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }
    pub fn f(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

pub fn onb(n: V3) -> (V3, V3) {
    let a = if n.x.abs() > 0.9 { v3(0.0, 1.0, 0.0) } else { v3(1.0, 0.0, 0.0) };
    let t = n.cross(a).norm();
    (t, n.cross(t))
}

pub fn cosine_dir(n: V3, rng: &mut Rng) -> V3 {
    let (t, b) = onb(n);
    let (r, phi) = (rng.f().sqrt(), rng.f() * std::f64::consts::TAU);
    (t * (r * phi.cos()) + b * (r * phi.sin()) + n * (1.0 - r * r).max(0.0).sqrt()).norm()
}

pub fn cone_dir(axis: V3, cos_max: f64, rng: &mut Rng) -> V3 {
    let (t, b) = onb(axis);
    let c = 1.0 - rng.f() * (1.0 - cos_max);
    let s = (1.0 - c * c).max(0.0).sqrt();
    let phi = rng.f() * std::f64::consts::TAU;
    (t * (s * phi.cos()) + b * (s * phi.sin()) + axis * c).norm()
}

pub fn in_sphere(rng: &mut Rng) -> V3 {
    loop {
        let p = v3(rng.f() * 2.0 - 1.0, rng.f() * 2.0 - 1.0, rng.f() * 2.0 - 1.0);
        if p.dot(p) <= 1.0 {
            return p;
        }
    }
}
