//! Surfaces: image textures on UVs, procedural patterns in object space, and screens that play a frame sequence.
use crate::math::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub struct Tex {
    w: usize,
    h: usize,
    px: Vec<[f32; 3]>,
}

fn decode(key: &str) -> Result<Tex, String> {
    let bytes = match key.rsplit_once("#texture") {
        Some((file, i)) => crate::gltf::texture_bytes(file, i.parse().map_err(|_| format!("{key}: texture index"))?)?,
        None => std::fs::read(key).map_err(|e| format!("{key}: {e}"))?,
    };
    if !bytes.starts_with(b"\x89PNG") {
        return Err(format!("{key}: textures are PNG"));
    }
    let (w, h, px) = crate::image::decode_bytes(&bytes, key)?;
    let lin = |c: u8| crate::image::linear(c);
    Ok(Tex { w, h, px: px.iter().map(|p| [lin(p[0]), lin(p[1]), lin(p[2])]).collect() })
}

/// A PNG file (or `model.glb#textureN`) as linear colour, kept for reuse.
pub fn load(file: &str) -> Result<Arc<Tex>, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Tex>>>> = OnceLock::new();
    let key = crate::scene::resolve(file);
    let cache = CACHE.get_or_init(Default::default);
    if let Some(t) = cache.lock().unwrap().get(&key) {
        return Ok(t.clone());
    }
    let t = Arc::new(decode(&key)?);
    let mut c = cache.lock().unwrap();
    if c.len() >= 32 {
        c.clear();
    }
    c.insert(key, t.clone());
    Ok(t)
}

impl Tex {
    /// Bilinear, repeating; v runs up the picture.
    pub fn sample(&self, uv: [f64; 2]) -> V3 {
        let x = uv[0].rem_euclid(1.0) * self.w as f64 - 0.5;
        let y = (1.0 - uv[1].rem_euclid(1.0)) * self.h as f64 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |i: f64, j: f64| {
            let p = self.px[(j as i64).rem_euclid(self.h as i64) as usize * self.w + (i as i64).rem_euclid(self.w as i64) as usize];
            v3(p[0] as f64, p[1] as f64, p[2] as f64)
        };
        (at(x0, y0) * (1.0 - fx) + at(x0 + 1.0, y0) * fx) * (1.0 - fy) + (at(x0, y0 + 1.0) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0) * fx) * fy
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Pattern {
    pub kind: String,
    pub scale: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<[f64; 3]>,
    pub amount: f64,
    pub axis: [f64; 3],
    pub range: [f64; 2],
}

impl Default for Pattern {
    fn default() -> Self {
        Pattern { kind: "brushed".into(), scale: 1.0, color: None, amount: 0.25, axis: [1.0, 0.0, 0.0], range: [-10.0, 10.0] }
    }
}

fn hash(x: i64, y: i64, z: i64) -> f64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ (z as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    h = (h ^ (h >> 31)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    ((h ^ (h >> 29)) >> 11) as f64 / (1u64 << 53) as f64
}

/// Smooth value noise in 0..1.
pub fn noise(p: V3) -> f64 {
    let (x0, y0, z0) = (p.x.floor(), p.y.floor(), p.z.floor());
    let s = |t: f64| t * t * (3.0 - 2.0 * t);
    let (fx, fy, fz) = (s(p.x - x0), s(p.y - y0), s(p.z - z0));
    let (i, j, k) = (x0 as i64, y0 as i64, z0 as i64);
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let c = |a: i64, b: i64, d: i64| hash(i + a, j + b, k + d);
    lerp(
        lerp(lerp(c(0, 0, 0), c(1, 0, 0), fx), lerp(c(0, 1, 0), c(1, 1, 0), fx), fy),
        lerp(lerp(c(0, 0, 1), c(1, 0, 1), fx), lerp(c(0, 1, 1), c(1, 1, 1), fx), fy),
        fz,
    )
}

fn fbm(p: V3) -> f64 {
    (0..4).map(|o| noise(p * (1u32 << o) as f64 + V3::splat(o as f64 * 17.3)) / (1u32 << o) as f64).sum::<f64>() / 1.875
}

impl Pattern {
    /// Colour and roughness at local point `p` over a base colour and roughness.
    pub fn at(&self, p: V3, base: V3, rough: f64) -> (V3, f64) {
        let s = self.scale.max(1e-6);
        let other = self.color.map(V3::from).unwrap_or(base * (1.0 - self.amount).max(0.0));
        let (k, r) = match self.kind.as_str() {
            "spun" => {
                let r = p.x.hypot(p.y) / s;
                let k = fbm(v3(r, p.y.atan2(p.x) * 2.0, 0.0));
                (k, rough * (1.0 + self.amount * (k - 0.5)))
            }
            "noise" => (fbm(p / s), rough),
            "gradient" => {
                let t = ((p.dot(V3::from(self.axis).norm()) - self.range[0]) / (self.range[1] - self.range[0]).max(1e-9)).clamp(0.0, 1.0);
                (t * t * (3.0 - 2.0 * t), rough)
            }
            "checker" => ((((p.x / s).floor() + (p.y / s).floor() + (p.z / s).floor()) as i64).rem_euclid(2) as f64, rough),
            _ => {
                let a = V3::from(self.axis).norm();
                let along = p.dot(a);
                let across = p - a * along;
                let k = fbm(across / s + a * (along * 0.02 / s));
                (k, rough * (1.0 + self.amount * (k - 0.5)))
            }
        };
        (base + (other - base) * k, r.clamp(0.02, 1.0))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Screen {
    pub file: String,
    pub fps: f64,
    pub start: f64,
    pub first: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i64>,
    pub strength: f64,
}

impl Default for Screen {
    fn default() -> Self {
        Screen { file: String::new(), fps: 30.0, start: 0.0, first: 1, count: None, strength: 1.0 }
    }
}

impl Screen {
    /// The picture showing at `time`: the file itself, or for `name%04d.png` the frame then, looping over `count`.
    pub fn frame(&self, time: f64) -> Option<String> {
        if time < self.start {
            return None;
        }
        let Some((head, tail)) = self.file.split_once('%') else { return Some(self.file.clone()) };
        let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        let rest = tail[digits.len()..].strip_prefix('d')?;
        let mut n = ((time - self.start) * self.fps + 1e-6).floor() as i64;
        if let Some(c) = self.count.filter(|c| *c > 0) {
            n = n.rem_euclid(c);
        }
        let width = digits.parse::<usize>().unwrap_or(0);
        let file = format!("{head}{:0width$}{rest}", n + self.first);
        std::path::Path::new(&crate::scene::resolve(&file)).exists().then_some(file)
    }
}
