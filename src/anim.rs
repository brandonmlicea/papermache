//! The timeline: keyframed tracks on any field of the scene, eased or splined, and the scene at any time.
use crate::math::*;
use crate::scene::{bounds, Scene};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// A field's path, `objects.<name>.position` (or `objects.tag:<t>.rotation`), `camera.eye`, `materials.<name>.emission`, `lights.lamps.<name>.strength`,
/// with keys sorted by time; `spline` runs a smooth curve through them instead of straight lines.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub path: String,
    pub keys: Vec<Key>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub spline: bool,
}

/// A value at time `t` (seconds); `ease` shapes the way to the next key: linear, in, out, inout, step, or a
/// cubic bezier [x1, y1, x2, y2] as in CSS.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Key {
    pub t: f64,
    pub v: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ease: Option<Value>,
}

fn bezier(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    let c = |a: f64, b: f64, s: f64| 3.0 * a * s * (1.0 - s).powi(2) + 3.0 * b * s * s * (1.0 - s) + s.powi(3);
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..40 {
        let m = (lo + hi) / 2.0;
        if c(x1, x2, m) < x { lo = m } else { hi = m }
    }
    c(y1, y2, (lo + hi) / 2.0)
}

/// Eased progress 0..1 for raw progress `x`.
pub fn ease(e: Option<&Value>, x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    match e {
        Some(Value::String(s)) => match s.as_str() {
            "in" => x * x * x,
            "out" => 1.0 - (1.0 - x).powi(3),
            "inout" | "smooth" => if x < 0.5 { 4.0 * x * x * x } else { 1.0 - (-2.0 * x + 2.0).powi(3) / 2.0 },
            "step" => if x < 1.0 { 0.0 } else { 1.0 },
            _ => x,
        },
        Some(Value::Array(a)) if a.len() == 4 => {
            let f = |i: usize| a[i].as_f64().unwrap_or(0.0);
            bezier(f(0), f(1), f(2), f(3), x)
        }
        _ => x,
    }
}

fn nums(v: &Value) -> Option<Vec<f64>> {
    match v {
        Value::Number(n) => Some(vec![n.as_f64()?]),
        Value::Array(a) => a.iter().map(Value::as_f64).collect(),
        _ => None,
    }
}

fn shaped(like: &Value, x: Vec<f64>) -> Value {
    if like.is_number() { json!(x[0]) } else { json!(x) }
}

impl Track {
    pub fn at(&self, t: f64) -> Option<Value> {
        let k = &self.keys;
        let first = k.first()?;
        if k.len() == 1 || t <= first.t {
            return Some(first.v.clone());
        }
        let i = k.windows(2).position(|w| t < w[1].t).unwrap_or(k.len() - 1);
        if i == k.len() - 1 {
            return Some(k[i].v.clone());
        }
        let (a, b) = (&k[i], &k[i + 1]);
        let s = ease(a.ease.as_ref(), (t - a.t) / (b.t - a.t).max(1e-12));
        let (Some(p1), Some(p2)) = (nums(&a.v), nums(&b.v)) else { return Some(if s < 1.0 { a.v.clone() } else { b.v.clone() }) };
        if p1.len() != p2.len() {
            return Some(a.v.clone());
        }
        if !self.spline {
            return Some(shaped(&a.v, p1.iter().zip(&p2).map(|(x, y)| x + (y - x) * s).collect()));
        }
        let p0 = i.checked_sub(1).and_then(|j| nums(&k[j].v)).filter(|v| v.len() == p1.len()).unwrap_or(p1.clone());
        let p3 = k.get(i + 2).and_then(|n| nums(&n.v)).filter(|v| v.len() == p1.len()).unwrap_or(p2.clone());
        let (s2, s3) = (s * s, s * s * s);
        Some(shaped(&a.v, (0..p1.len()).map(|j| {
            0.5 * (2.0 * p1[j] + (p2[j] - p0[j]) * s + (2.0 * p0[j] - 5.0 * p1[j] + 4.0 * p2[j] - p3[j]) * s2 + (3.0 * p1[j] - p0[j] - 3.0 * p2[j] + p3[j]) * s3)
        }).collect()))
    }

    /// Adds keys, replacing any at the same time, and keeps them in order.
    pub fn put(&mut self, keys: Vec<Key>) {
        for key in keys {
            self.keys.retain(|k| (k.t - key.t).abs() > 1e-9);
            self.keys.push(key);
        }
        self.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
    }
}

/// Sets the value at a dotted path; a segment picks a map key, an array index, the array item with that `name`, or
/// with `tag:<t>` every item with that tag.
pub fn set(root: &mut Value, path: &str, v: Value) -> Result<(), String> {
    let mut cur = root;
    let segs: Vec<&str> = path.split('.').collect();
    for (i, seg) in segs.iter().enumerate() {
        let last = i == segs.len() - 1;
        cur = match cur {
            Value::Array(a) if seg.starts_with("tag:") => {
                let rest = segs[i + 1..].join(".");
                for o in a.iter_mut().filter(|o| o.get("tags").and_then(Value::as_array).is_some_and(|t| t.iter().any(|x| x.as_str() == Some(&seg[4..])))) {
                    set(o, &rest, v.clone())?;
                }
                return Ok(());
            }
            Value::Array(a) => {
                let at = match seg.parse::<usize>() {
                    Ok(n) if n < a.len() => n,
                    _ => a.iter().position(|o| o.get("name").and_then(Value::as_str) == Some(seg)).ok_or(format!("{path}: no {seg}"))?,
                };
                &mut a[at]
            }
            Value::Object(m) => {
                if !m.contains_key(*seg) && !last {
                    m.insert(seg.to_string(), json!({}));
                }
                m.entry(seg.to_string()).or_insert(Value::Null)
            }
            _ => return Err(format!("{path}: {seg} is inside a value that is not a map or list")),
        };
        if last {
            *cur = v;
            return Ok(());
        }
    }
    Ok(())
}

/// The scene at time `t`, every track applied.
pub fn at(s: &Scene, t: f64) -> Result<Scene, String> {
    let mut out = if s.timeline.is_empty() {
        s.clone()
    } else {
        let mut v = serde_json::to_value(s).map_err(|e| e.to_string())?;
        for tr in &s.timeline {
            if let Some(val) = tr.at(t) {
                set(&mut v, &tr.path, val)?;
            }
        }
        serde_json::from_value(v).map_err(|e| format!("timeline at {t}s: {e}"))?
    };
    out.time = t;
    Ok(out)
}

/// The last key's time.
pub fn length(s: &Scene) -> f64 {
    s.timeline.iter().flat_map(|t| t.keys.iter().map(|k| k.t)).fold(0.0, f64::max)
}

pub fn track<'a>(s: &'a mut Scene, path: &str) -> &'a mut Track {
    match s.timeline.iter().position(|t| t.path == path) {
        Some(i) => &mut s.timeline[i],
        None => {
            s.timeline.push(Track { path: path.into(), keys: vec![], spline: false });
            s.timeline.last_mut().unwrap()
        }
    }
}

/// Keys each named part from where it sits at `times[0]` out to `distance` mm from the assembly's centre (or its own
/// offset) at `times[1]`, holding until `times[2]` and back home at `times[3]` when given.
pub fn explode(s: &mut Scene, names: &[String], offsets: &serde_json::Map<String, Value>, distance: f64, times: &[f64], ease_by: Option<Value>) -> Result<Vec<String>, String> {
    let picked: Vec<_> = names.iter().map(|n| s.objects.iter().find(|o| &o.name == n).cloned().ok_or(format!("no object {n}"))).collect::<Result<_, _>>()?;
    let mid = picked.iter().map(bounds).fold(Aabb::EMPTY, Aabb::union).centroid();
    if times.len() < 2 {
        return Err("times: [together, apart, hold?, back?]".into());
    }
    for o in &picked {
        let home = V3::from(o.position);
        let off = match offsets.get(&o.name).and_then(nums) {
            Some(v) if v.len() == 3 => v3(v[0], v[1], v[2]),
            _ => {
                let d = bounds(o).centroid() - mid;
                (if d.len() < 1e-9 { v3(0.0, 0.0, 1.0) } else { d.norm() }) * distance
            }
        };
        let apart = json!((home + off).arr());
        let mut keys = vec![Key { t: times[0], v: json!(home.arr()), ease: ease_by.clone() }, Key { t: times[1], v: apart.clone(), ease: ease_by.clone() }];
        if let Some(&h) = times.get(2) {
            keys.push(Key { t: h, v: apart, ease: ease_by.clone() });
        }
        if let Some(&b) = times.get(3) {
            keys.push(Key { t: b, v: json!(home.arr()), ease: None });
        }
        track(s, &format!("objects.{}.position", o.name)).put(keys);
    }
    Ok(picked.iter().map(|o| o.name.clone()).collect())
}
