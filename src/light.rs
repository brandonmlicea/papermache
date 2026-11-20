//! Lamps beside the sun and sky: point, spot and rectangular area lights, sampled for soft shadows.
use crate::math::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Lamp {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub kind: String,
    pub position: [f64; 3],
    pub look: [f64; 3],
    pub color: [f64; 3],
    pub strength: f64,
    pub radius: f64,
    pub size: [f64; 2],
    pub angle: f64,
    pub blend: f64,
    pub visible: bool,
}

impl Default for Lamp {
    fn default() -> Self {
        Lamp { name: String::new(), kind: "area".into(), position: [0.0, -200.0, 200.0], look: [0.0; 3], color: [1.0; 3], strength: 1.0, radius: 0.0, size: [100.0, 100.0], angle: 45.0, blend: 0.2, visible: true }
    }
}

/// A lamp ready to sample: where it is, which way it faces, its rectangle's axes and its light.
pub struct Live {
    p: V3,
    n: V3,
    u: V3,
    v: V3,
    hw: f64,
    hh: f64,
    area: bool,
    show: bool,
    r: f64,
    cone: (f64, f64),
    e: V3,
}

impl Lamp {
    pub fn live(&self) -> Live {
        let p = V3::from(self.position);
        let n = (V3::from(self.look) - p).norm();
        let n = if n.len() < 0.5 { v3(0.0, 0.0, -1.0) } else { n };
        let up = if n.z.abs() > 0.99 { v3(0.0, 1.0, 0.0) } else { v3(0.0, 0.0, 1.0) };
        let u = n.cross(up).norm();
        let half = (self.angle.clamp(0.1, 179.0) / 2.0).to_radians();
        Live {
            p,
            n,
            u,
            v: u.cross(n),
            hw: self.size[0].abs() / 2.0,
            hh: self.size[1].abs() / 2.0,
            area: self.kind == "area",
            show: self.visible && self.kind == "area",
            r: self.radius.max(0.0),
            cone: if self.kind == "spot" { (half.cos(), (half * (1.0 - self.blend.clamp(0.0, 1.0))).cos()) } else { (-2.0, -1.0) },
            e: V3::from(self.color) * self.strength * 1e4,
        }
    }
}

impl Live {
    pub fn shows(&self) -> bool {
        self.show
    }

    /// From point `p`: the direction to a random point of the lamp, its distance, and the light arriving
    /// (strength 1 lights like a sun of 1 at 100 mm).
    pub fn sample(&self, p: V3, rng: &mut Rng) -> Option<(V3, f64, V3)> {
        let q = if self.area {
            self.p + self.u * ((rng.f() * 2.0 - 1.0) * self.hw) + self.v * ((rng.f() * 2.0 - 1.0) * self.hh)
        } else {
            self.p + in_sphere(rng) * self.r
        };
        let l = q - p;
        let d2 = l.dot(l).max(1.0);
        let dist = d2.sqrt();
        let dir = l / dist;
        let c = -dir.dot(self.n);
        let k = if self.area {
            c
        } else {
            let (lo, hi) = self.cone;
            let t = ((c - lo) / (hi - lo).max(1e-9)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        (k > 0.0).then(|| (dir, dist, self.e * (k / d2)))
    }

    /// A reflected ray meeting the lit face of a visible area lamp before `tmax`: its distance and the lamp's radiance.
    /// The camera never sees lamps directly, as a studio keeps its lights out of frame.
    pub fn seen(&self, o: V3, d: V3, tmax: f64) -> Option<(f64, V3)> {
        let dn = d.dot(self.n);
        if !self.show || dn >= 0.0 {
            return None;
        }
        let t = (self.p - o).dot(self.n) / dn;
        if t <= 1e-6 || t >= tmax {
            return None;
        }
        let q = o + d * t - self.p;
        (q.dot(self.u).abs() <= self.hw && q.dot(self.v).abs() <= self.hh).then(|| (t, self.e / (4.0 * self.hw * self.hh).max(1e-9)))
    }
}
