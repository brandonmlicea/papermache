//! From a scene to what the renderer meets: each shape's primitives, the materials they wear, the lamps, the BVH.
use crate::bvh::Bvh;
use crate::geom::{Kind, Prim, Shade};
use crate::light::Live;
use crate::math::*;
use crate::model::{self, Mesh};
use crate::scene::{resolve, Material, Object, Scene, Shape};
use crate::texture::{self, Tex};
use crate::{gltf, image, mesh, subdiv, text};
use std::collections::HashMap;
use std::sync::Arc;

fn finish(m: Mesh, subdivide: usize, smooth: Option<f64>) -> Mesh {
    let m = if subdivide > 0 { subdiv::subdivide(&m, subdivide) } else { m };
    match smooth {
        Some(a) => m.smooth(a),
        None if subdivide > 0 => m.smooth(60.0),
        None => m,
    }
}

/// The local mesh of a mesh, lathe, extrude or backdrop, built once per shape.
pub fn local_mesh(shape: &Shape) -> Result<Option<Arc<Mesh>>, String> {
    let key = serde_json::to_string(shape).unwrap_or_default();
    let m = match shape {
        Shape::Mesh { file, vertices, faces, node, skip, smooth, subdivide } => model::cached(key, || {
            let raw = match file {
                Some(f) if f.to_lowercase().ends_with(".glb") || f.to_lowercase().ends_with(".gltf") => gltf::load(&resolve(f), node.as_deref(), skip)?,
                Some(f) => {
                    let (v, f) = mesh::load(&resolve(f))?;
                    Mesh { v, f: f.iter().map(|t| t.map(|i| i as u32)).collect(), ..Mesh::default() }
                }
                None => Mesh {
                    v: vertices.clone().unwrap_or_default().into_iter().map(V3::from).collect(),
                    f: faces.clone().unwrap_or_default().iter().map(|t| t.map(|i| i as u32)).collect(),
                    ..Mesh::default()
                },
            };
            if let Some(f) = raw.f.iter().find(|f| f.iter().any(|&i| i as usize >= raw.v.len())) {
                return Err(format!("face {f:?} points past {} vertices", raw.v.len()));
            }
            Ok(finish(raw, *subdivide, *smooth))
        })?,
        Shape::Lathe { profile, segments, ridges, ridge_depth, ridge_band, subdivide, smooth } => {
            model::cached(key, || Ok(finish(model::lathe(profile, *segments, *ridges, *ridge_depth, *ridge_band), *subdivide, Some(*smooth))))?
        }
        Shape::Extrude { outline, size, radius, depth, bevel, segments, wall, subdivide, smooth } => model::cached(key, || {
            let line = match (outline, size) {
                (Some(o), _) if o.len() >= 3 => o.clone(),
                (_, Some(s)) => model::rounded_rect(*s, *radius, 8),
                _ => return Err("extrude needs an outline of 3 or more points, or a size [w, h]".into()),
            };
            Ok(finish(model::extrude(&line, *depth, *bevel, *segments, *wall), *subdivide, Some(*smooth)))
        })?,
        Shape::Backdrop { size, radius } => {
            model::cached(key, || Ok(model::backdrop(*size, radius.unwrap_or(size[1].min(size[2]) * 0.3)).smooth(40.0)))?
        }
        _ => return Ok(None),
    };
    Ok(Some(m))
}

/// A shape's pieces: each a local shape and the affine that places it in the world.
pub fn parts(o: &Object) -> Result<Vec<(Affine, Kind)>, String> {
    let at = Affine::compose(o.position, o.rotation, o.scale).then(&Affine::translate(-V3::from(o.pivot)));
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
    if let Some(m) = local_mesh(&o.shape).map_err(|e| format!("{}: {e}", o.name))? {
        let inv = at.inverse();
        let w: Vec<V3> = m.v.iter().map(|v| at.apply(*v)).collect();
        let flip = at.det() < 0.0;
        let sh = Arc::new(Shade {
            n: m.n.iter().map(|n| inv.tdir(*n).norm()).collect(),
            uv: m.uv.clone(),
            f: m.f.clone(),
            fm: m.fm.clone(),
            mats: m.mats.clone(),
            local: inv,
        });
        let mut out = Vec::with_capacity(m.f.len());
        for (i, f) in m.f.iter().enumerate() {
            let [a, b, c] = f.map(|k| w[k as usize]);
            let (b, c) = if flip { (c, b) } else { (b, c) };
            let (e1, e2) = (b - a, c - a);
            let n = e1.cross(e2).norm();
            if n.len() > 0.0 {
                out.push((Affine::ID, Kind::Tri { a, e1, e2, n, sh: Some((sh.clone(), i as u32 | (flip as u32) << 31)) }));
            }
        }
        return Ok(out);
    }
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
        _ => vec![],
    })
}

pub fn bounds(o: &Object) -> Aabb {
    parts(o).unwrap_or_default().iter().fold(Aabb::EMPTY, |b, (a, k)| {
        b.union(if matches!(k, Kind::Tri { .. }) { k.local_bounds() } else { k.local_bounds().transform(a) })
    })
}

/// A material ready to shade: its fields, its texture, and the screen frame showing now.
pub struct Mat {
    pub m: Material,
    pub tex: Option<Arc<Tex>>,
    pub screen: Option<Arc<Tex>>,
}

pub struct Compiled {
    pub prims: Vec<Prim>,
    pub mats: Vec<Mat>,
    pub bvh: Bvh,
    pub lamps: Vec<Live>,
}

fn ready(m: &Material, time: f64, strict: bool) -> Result<Mat, String> {
    let tex = match m.texture.as_deref() {
        Some(f) => match texture::load(f) {
            Ok(t) => Some(t),
            Err(e) if strict => return Err(e),
            Err(_) => None,
        },
        None => None,
    };
    let screen = match m.screen.as_ref().and_then(|s| s.frame(time)) {
        Some(f) => Some(texture::load(&f)?),
        None => None,
    };
    Ok(Mat { m: m.clone(), tex, screen })
}

pub fn compile(s: &Scene) -> Result<Compiled, String> {
    let mut mats = vec![ready(&Material::default(), s.time, true)?];
    let mut index = HashMap::new();
    for (n, m) in &s.materials {
        index.insert(n.clone(), mats.len() as u32);
        mats.push(ready(m, s.time, true).map_err(|e| format!("material {n}: {e}"))?);
    }
    let mut own: HashMap<(usize, u16), u32> = HashMap::new();
    let mut prims = vec![];
    for (oi, o) in s.objects.iter().enumerate().filter(|(_, o)| !o.hidden) {
        let set = index.get(&o.material).copied();
        for (a, k) in parts(o)? {
            let mi = match (&k, set) {
                (_, Some(m)) => m,
                (Kind::Tri { sh: Some((sh, i)), .. }, None) if !sh.mats.is_empty() => {
                    let slot = sh.fm.get((*i & 0x7fff_ffff) as usize).copied().unwrap_or(0);
                    let (name, m) = &sh.mats[slot as usize];
                    match index.get(name) {
                        Some(&m) => m,
                        None => *own.entry((Arc::as_ptr(sh) as usize, slot)).or_insert_with(|| {
                            mats.push(ready(m, s.time, false).unwrap_or(Mat { m: m.clone(), tex: None, screen: None }));
                            mats.len() as u32 - 1
                        }),
                    }
                }
                _ => 0,
            };
            prims.push(Prim::new(k, &a, mi, oi as u32));
        }
    }
    let bvh = Bvh::build(&prims);
    Ok(Compiled { prims, mats, bvh, lamps: s.lights.lamps.iter().filter(|l| l.strength > 0.0).map(|l| l.live()).collect() })
}
