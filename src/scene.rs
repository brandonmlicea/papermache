use crate::anim::Track;
use crate::light::Lamp;
use crate::texture::{Pattern, Screen};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

pub use crate::build::{bounds, compile, parts, Compiled};
pub use crate::shape::Shape;

fn zero3() -> [f64; 3] {
    [0.0; 3]
}
fn one3() -> [f64; 3] {
    [1.0; 3]
}

fn is_origin(v: &[f64; 3]) -> bool {
    *v == [0.0; 3]
}
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}
fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Material {
    pub color: [f64; 3],
    pub metallic: f64,
    pub roughness: f64,
    pub emission: [f64; 3],
    pub opacity: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub coat: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub coat_roughness: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
    #[serde(skip_serializing_if = "is_one2")]
    pub repeat: [f64; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<Pattern>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen: Option<Screen>,
}

fn is_one2(v: &[f64; 2]) -> bool {
    *v == [1.0, 1.0]
}

impl Default for Material {
    fn default() -> Self {
        Material { color: [0.8; 3], metallic: 0.0, roughness: 0.5, emission: [0.0; 3], opacity: 1.0, coat: 0.0, coat_roughness: 0.03, texture: None, repeat: [1.0, 1.0], pattern: None, screen: None }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Object {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(flatten)]
    pub shape: Shape,
    #[serde(default = "zero3")]
    pub position: [f64; 3],
    #[serde(default = "zero3")]
    pub rotation: [f64; 3],
    #[serde(default = "one3")]
    pub scale: [f64; 3],
    #[serde(default = "zero3", skip_serializing_if = "is_origin")]
    pub pivot: [f64; 3],
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub material: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Camera {
    pub eye: [f64; 3],
    pub look: [f64; 3],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub up: Option<[f64; 3]>,
    pub projection: String,
    pub lens: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fov: Option<f64>,
    pub width: f64,
    pub size: [u32; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fstop: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus_on: Option<String>,
}

impl Default for Camera {
    fn default() -> Self {
        Camera { eye: [120.0, -160.0, 110.0], look: [0.0; 3], up: None, projection: "perspective".into(), lens: 50.0, fov: None, width: 100.0, size: [1600, 1200], fstop: None, focus: None, focus_on: None }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Sun {
    pub direction: [f64; 3],
    pub angle: f64,
    pub strength: f64,
}

impl Default for Sun {
    fn default() -> Self {
        Sun { direction: [0.35, -0.45, 0.82], angle: 10.0, strength: 1.7 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Sky {
    pub color: [f64; 3],
    pub strength: f64,
}

impl Default for Sky {
    fn default() -> Self {
        Sky { color: [1.0; 3], strength: 0.7 }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Lights {
    pub sun: Sun,
    pub sky: Sky,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lamps: Vec<Lamp>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderOpts {
    pub samples: u32,
    pub noise: f64,
    #[serde(skip_serializing_if = "is_false")]
    pub denoise: bool,
    #[serde(skip_serializing_if = "is_zero")]
    pub bloom: f64,
    #[serde(skip_serializing_if = "is_false")]
    pub filmic: bool,
    pub fps: f64,
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub blur: u32,
    pub shutter: f64,
}

fn is_zero_u32(v: &u32) -> bool {
    *v <= 1
}

impl Default for RenderOpts {
    fn default() -> Self {
        RenderOpts { samples: 16, noise: 0.0, denoise: false, bloom: 0.0, filmic: false, fps: 30.0, blur: 0, shutter: 0.5 }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Scene {
    pub materials: BTreeMap<String, Material>,
    pub objects: Vec<Object>,
    pub camera: Camera,
    pub lights: Lights,
    pub render: RenderOpts,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub points: BTreeMap<String, [f64; 3]>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub timeline: Vec<Track>,
    #[serde(skip_serializing_if = "is_zero")]
    pub time: f64,
}

static BASE: std::sync::RwLock<Option<std::path::PathBuf>> = std::sync::RwLock::new(None);

/// A shape's `file`: as given if it exists, else beside the scene file last loaded or saved.
pub fn resolve(file: &str) -> String {
    let p = std::path::Path::new(file);
    if p.is_absolute() || p.exists() {
        return file.to_string();
    }
    match BASE.read().unwrap().as_ref().map(|b| b.join(p)) {
        Some(q) if q.exists() => q.to_string_lossy().into_owned(),
        _ => file.to_string(),
    }
}

/// Relative files in scenes are read beside this scene file from now on.
pub fn set_base(scene_path: &str) {
    let dir = std::path::Path::new(scene_path).parent().map(|d| d.to_path_buf());
    *BASE.write().unwrap() = dir.map(|d| if d.as_os_str().is_empty() { ".".into() } else { d });
}

impl Scene {
    pub fn load(path: &str) -> Result<Scene, String> {
        set_base(path);
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let mut s: Scene = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
        s.name_all();
        Ok(s)
    }

    /// Every object gets a unique name: its own, or obj<N>.
    pub fn name_all(&mut self) {
        let mut seen = HashSet::new();
        let mut n = 0;
        for o in &mut self.objects {
            if o.name.is_empty() || seen.contains(&o.name) {
                let base = if o.name.is_empty() { "obj".to_string() } else { format!("{}.", o.name) };
                loop {
                    n += 1;
                    let cand = format!("{base}{n}");
                    if !seen.contains(&cand) {
                        o.name = cand;
                        break;
                    }
                }
            }
            seen.insert(o.name.clone());
        }
    }
}
