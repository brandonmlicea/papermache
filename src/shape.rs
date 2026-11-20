use serde::{Deserialize, Serialize};

fn white() -> [f64; 3] {
    [1.0; 3]
}
fn four() -> usize {
    4
}
fn one() -> f64 {
    1.0
}
fn sixty_four() -> usize {
    64
}
fn forty() -> f64 {
    40.0
}
fn center() -> String {
    "center".into()
}
fn is_zero(v: &f64) -> bool {
    *v == 0.0
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skip: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        smooth: Option<f64>,
        #[serde(default, skip_serializing_if = "is_none_usize")]
        subdivide: usize,
    },
    Lathe {
        profile: Vec<[f64; 2]>,
        #[serde(default = "sixty_four")]
        segments: usize,
        #[serde(default, skip_serializing_if = "is_none_usize")]
        ridges: usize,
        #[serde(default, skip_serializing_if = "is_zero")]
        ridge_depth: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ridge_band: Option<[f64; 2]>,
        #[serde(default, skip_serializing_if = "is_none_usize")]
        subdivide: usize,
        #[serde(default = "forty")]
        smooth: f64,
    },
    Extrude {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outline: Option<Vec<[f64; 2]>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<[f64; 2]>,
        #[serde(default, skip_serializing_if = "is_zero")]
        radius: f64,
        depth: f64,
        #[serde(default, skip_serializing_if = "is_zero")]
        bevel: f64,
        #[serde(default = "four")]
        segments: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wall: Option<f64>,
        #[serde(default, skip_serializing_if = "is_none_usize")]
        subdivide: usize,
        #[serde(default = "forty")]
        smooth: f64,
    },
    Backdrop {
        size: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        radius: Option<f64>,
    },
}

fn is_none_usize(v: &usize) -> bool {
    *v == 0
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
            Shape::Lathe { .. } => "lathe",
            Shape::Extrude { .. } => "extrude",
            Shape::Backdrop { .. } => "backdrop",
        }
    }
}
