use crate::bvh::Bvh;
use crate::geom::{Kind, Prim};
use crate::math::*;
use crate::{image, mesh, text};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

fn zero3() -> [f64; 3] {
    [0.0; 3]
}
fn one3() -> [f64; 3] {
    [1.0; 3]
}
fn white() -> [f64; 3] {
    [1.0; 3]
}

fn four() -> usize {
    4
}

fn one() -> f64 {
    1.0
}
fn center() -> String {
    "center".into()
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
}

impl Default for Material {
    fn default() -> Self {
        Material { color: [0.8; 3], metallic: 0.0, roughness: 0.5, emission: [0.0; 3], opacity: 1.0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "lowercase")]
pub enum Shape {
    Box {
        size: [f64; 3],
        #[serde(default, skip_serializing_if = "is_zero")]
        radius: f64,
    },
    Cylinder {
        radius: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        radius_top: Option<f64>,
        height: f64,
    },
    Sphere {
        radius: f64,
    },
    Torus {
        major: f64,
        minor: f64,
    },
    Rod {
        from: [f64; 3],
        to: [f64; 3],
        radius: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        radius_top: Option<f64>,
    },
    Tube {
        points: Vec<[f64; 3]>,
        radius: f64,
    },
    Plane {
        size: [f64; 2],
    },
    Text {
        text: String,
        #[serde(default = "one")]
        size: f64,
        #[serde(default = "center")]
        align: String,
    },
    Image {
        file: String,
        width: f64,
        #[serde(default = "one")]
        depth: f64,
        #[serde(default = "white")]
        key: [f64; 3],
        #[serde(default = "four")]
        detail: usize,
    },
    Mesh {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        vertices: Option<Vec<[f64; 3]>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        faces: Option<Vec<[usize; 3]>>,
    },
}

impl Shape {
    pub fn name(&self) -> &'static str {
        match self {
            Shape::Box { .. } => "box",
            Shape::Cylinder { .. } => "cylinder",
            Shape::Sphere { .. } => "sphere",
            Shape::Torus { .. } => "torus",
            Shape::Rod { .. } => "rod",
            Shape::Tube { .. } => "tube",
            Shape::Plane { .. } => "plane",
            Shape::Text { .. } => "text",
            Shape::Image { .. } => "image",
            Shape::Mesh { .. } => "mesh",
        }
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
}

impl Default for Camera {
    fn default() -> Self {
        Camera { eye: [120.0, -160.0, 110.0], look: [0.0; 3], up: None, projection: "perspective".into(), lens: 50.0, fov: None, width: 100.0, size: [1600, 1200] }
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderOpts {
    pub samples: u32,
}

impl Default for RenderOpts {
    fn default() -> Self {
        RenderOpts { samples: 16 }
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

/// A shape's pieces: each a local shape and the affine that places it in the world.
pub fn parts(o: &Object) -> Result<Vec<(Affine, Kind)>, String> {
    let at = Affine::compose(o.position, o.rotation, o.scale);
    let shift = V3::from(o.position);
    let rod = |a: V3, b: V3, r0: f64, r1: f64| -> Option<(Affine, Kind)> {
        let ax = b - a;
        let l = ax.len();
        if l < 1e-12 {
            return None;
        }
        let z = ax / l;
        let (x, y) = onb(z);
        let (x, y) = if x.cross(y).dot(z) < 0.0 { (y, x) } else { (x, y) };
        Some((Affine::frame((a + b) * 0.5 + shift, x, y, z), Kind::Cyl { r0, r1, h2: l / 2.0 }))
    };
    Ok(match &o.shape {
        Shape::Box { size, radius } => {
            let h = V3::from(*size) * 0.5;
            let r = radius.min(h.x.min(h.y).min(h.z));
            vec![(at, if r > 0.0 { Kind::RBox(h, r) } else { Kind::Box(h) })]
        }
        Shape::Cylinder { radius, radius_top, height } => vec![(at, Kind::Cyl { r0: *radius, r1: radius_top.unwrap_or(*radius), h2: height / 2.0 })],
        Shape::Sphere { radius } => vec![(at, Kind::Sphere(*radius))],
        Shape::Torus { major, minor } => vec![(at, Kind::Torus(*major, *minor))],
        Shape::Rod { from, to, radius, radius_top } => rod(V3::from(*from), V3::from(*to), *radius, radius_top.unwrap_or(*radius)).into_iter().collect(),
        Shape::Tube { points, radius } => {
            let p: Vec<V3> = points.iter().map(|q| V3::from(*q)).collect();
            let mut out: Vec<_> = p.windows(2).filter_map(|w| rod(w[0], w[1], *radius, *radius)).collect();
            out.extend(p.iter().map(|q| (Affine::translate(*q + shift), Kind::Sphere(*radius))));
            out
        }
        Shape::Plane { size } => vec![(at, Kind::Plane(size[0] / 2.0, size[1] / 2.0))],
        Shape::Text { text: t, size, align } => match text::mask(t) {
            Some(mask) => {
                let (lo, hi) = mask.quad(*size, align);
                vec![(at, Kind::Text { lo, hi, mask })]
            }
            None => vec![],
        },
        Shape::Image { file, width, depth, key, detail } => {
            let px = image::load(&resolve(file), *key, *detail)?;
            vec![(at, Kind::Image { half: v3(width / 2.0, width * px.aspect / 2.0, depth / 2.0), px })]
        }
        Shape::Mesh { file, vertices, faces } => {
            let (vs, fs) = match file {
                Some(f) => mesh::load(&resolve(f))?,
                None => (vertices.clone().unwrap_or_default().into_iter().map(V3::from).collect(), faces.clone().unwrap_or_default()),
            };
            let w: Vec<V3> = vs.iter().map(|v| at.apply(*v)).collect();
            let mut out = vec![];
            for f in fs {
                if f.iter().any(|&i| i >= w.len()) {
                    return Err(format!("{}: face {:?} points past {} vertices", o.name, f, w.len()));
                }
                let (a, e1, e2) = (w[f[0]], w[f[1]] - w[f[0]], w[f[2]] - w[f[0]]);
                let n = e1.cross(e2).norm();
                if n.len() > 0.0 {
                    out.push((Affine::ID, Kind::Tri { a, e1, e2, n }));
                }
            }
            out
        }
    })
}

pub fn bounds(o: &Object) -> Aabb {
    parts(o).unwrap_or_default().iter().fold(Aabb::EMPTY, |b, (a, k)| {
        b.union(if matches!(k, Kind::Tri { .. }) { k.local_bounds() } else { k.local_bounds().transform(a) })
    })
}

pub struct Compiled {
    pub prims: Vec<Prim>,
    pub mats: Vec<Material>,
    pub bvh: Bvh,
}

pub fn compile(s: &Scene) -> Result<Compiled, String> {
    let mut mats = vec![Material::default()];
    let mut index = HashMap::new();
    for (n, m) in &s.materials {
        index.insert(n.as_str(), mats.len() as u32);
        mats.push(m.clone());
    }
    let mut prims = vec![];
    for (oi, o) in s.objects.iter().enumerate().filter(|(_, o)| !o.hidden) {
        let mi = *index.get(o.material.as_str()).unwrap_or(&0);
        for (a, k) in parts(o)? {
            prims.push(Prim::new(k, &a, mi, oi as u32));
        }
    }
    let bvh = Bvh::build(&prims);
    Ok(Compiled { prims, mats, bvh })
}
