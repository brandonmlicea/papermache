//! glTF 2.0 in (.glb or .gltf): meshes placed by their nodes, with materials and base colour textures, turned from
//! Y-up metres into Z-up millimetres.
use crate::math::*;
use crate::model::Mesh;
use crate::scene::Material;
use base64::Engine;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub struct Doc {
    pub json: Value,
    bufs: Vec<Vec<u8>>,
}

fn u32le(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
}

fn uri_bytes(uri: &str, dir: &std::path::Path) -> Result<Vec<u8>, String> {
    match uri.strip_prefix("data:").and_then(|d| d.split_once(";base64,")) {
        Some((_, data)) => base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| e.to_string()),
        None => std::fs::read(dir.join(uri)).map_err(|e| format!("{uri}: {e}")),
    }
}

fn read(path: &str) -> Result<Doc, String> {
    let b = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let bad = || format!("{path}: not a readable glTF");
    let (json, bin) = if b.starts_with(b"glTF") {
        let n = u32le(&b, 12).ok_or_else(bad)?;
        let json: Value = serde_json::from_slice(b.get(20..20 + n).ok_or_else(bad)?).map_err(|e| format!("{path}: {e}"))?;
        let at = 20 + n;
        let bin = u32le(&b, at).and_then(|m| b.get(at + 8..at + 8 + m)).map(|s| s.to_vec());
        (json, bin)
    } else {
        (serde_json::from_slice(&b).map_err(|e| format!("{path}: {e}"))?, None)
    };
    let dir = std::path::Path::new(path).parent().unwrap_or(std::path::Path::new("."));
    let mut bufs = vec![];
    for buf in json["buffers"].as_array().cloned().unwrap_or_default() {
        bufs.push(match buf["uri"].as_str() {
            Some(u) => uri_bytes(u, dir)?,
            None => bin.clone().ok_or_else(bad)?,
        });
    }
    Ok(Doc { json, bufs })
}

pub fn doc(path: &str) -> Result<Arc<Doc>, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Doc>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(d) = cache.lock().unwrap().get(path) {
        return Ok(d.clone());
    }
    let d = Arc::new(read(path)?);
    cache.lock().unwrap().insert(path.to_string(), d.clone());
    Ok(d)
}

impl Doc {
    fn view(&self, i: usize) -> Option<&[u8]> {
        let v = &self.json["bufferViews"][i];
        let off = v["byteOffset"].as_u64().unwrap_or(0) as usize;
        self.bufs.get(v["buffer"].as_u64()? as usize)?.get(off..off + v["byteLength"].as_u64()? as usize)
    }

    /// An accessor's elements, each `comps` numbers long.
    fn accessor(&self, i: usize) -> Result<(Vec<f64>, usize), String> {
        let a = &self.json["accessors"][i];
        let comps = match a["type"].as_str().unwrap_or("") {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT4" => 16,
            t => return Err(format!("accessor {i}: type {t}")),
        };
        let count = a["count"].as_u64().unwrap_or(0) as usize;
        let ct = a["componentType"].as_u64().unwrap_or(5126);
        let size = match ct {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            _ => 4,
        };
        let Some(bv) = a["bufferView"].as_u64() else { return Ok((vec![0.0; count * comps], comps)) };
        let data = self.view(bv as usize).ok_or(format!("accessor {i}: view out of range"))?;
        let stride = self.json["bufferViews"][bv as usize]["byteStride"].as_u64().map(|s| s as usize).unwrap_or(comps * size);
        let base = a["byteOffset"].as_u64().unwrap_or(0) as usize;
        let norm = a["normalized"].as_bool().unwrap_or(false);
        let mut out = Vec::with_capacity(count * comps);
        for e in 0..count {
            for c in 0..comps {
                let at = base + e * stride + c * size;
                let s = data.get(at..at + size).ok_or(format!("accessor {i}: past its view"))?;
                let (v, max) = match ct {
                    5120 => (s[0] as i8 as f64, 127.0),
                    5121 => (s[0] as f64, 255.0),
                    5122 => (i16::from_le_bytes([s[0], s[1]]) as f64, 32767.0),
                    5123 => (u16::from_le_bytes([s[0], s[1]]) as f64, 65535.0),
                    5125 => (u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64, 1.0),
                    _ => (f32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64, 1.0),
                };
                out.push(if norm { (v / max).max(-1.0) } else { v });
            }
        }
        Ok((out, comps))
    }

    fn local(&self, n: &Value) -> Affine {
        let f = |k: &str, d: &[f64]| -> Vec<f64> { n[k].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or(d.to_vec()) };
        if let Some(m) = n["matrix"].as_array().filter(|m| m.len() == 16) {
            let m: Vec<f64> = m.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect();
            return Affine { m: [[m[0], m[4], m[8]], [m[1], m[5], m[9]], [m[2], m[6], m[10]]], t: v3(m[12], m[13], m[14]) };
        }
        let (t, q, s) = (f("translation", &[0.0; 3]), f("rotation", &[0.0, 0.0, 0.0, 1.0]), f("scale", &[1.0; 3]));
        let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
        let r = [
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
            [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
            [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
        ];
        let mut m = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] = r[i][j] * s[j];
            }
        }
        Affine { m, t: v3(t[0], t[1], t[2]) }
    }

    /// Every node under `pick` (or every node) that is not under a `skip` name, with its world placement in Z-up mm.
    fn walk(&self, pick: Option<&str>, skip: &[String]) -> Vec<(usize, Affine)> {
        let nodes = self.json["nodes"].as_array().cloned().unwrap_or_default();
        let roots: Vec<usize> = match self.json["scenes"][self.json["scene"].as_u64().unwrap_or(0) as usize]["nodes"].as_array() {
            Some(r) => r.iter().filter_map(|v| v.as_u64().map(|v| v as usize)).collect(),
            None => {
                let kids: Vec<u64> = nodes.iter().flat_map(|n| n["children"].as_array().cloned().unwrap_or_default()).filter_map(|v| v.as_u64()).collect();
                (0..nodes.len()).filter(|i| !kids.contains(&(*i as u64))).collect()
            }
        };
        let up = Affine { m: [[1000.0, 0.0, 0.0], [0.0, 0.0, -1000.0], [0.0, 1000.0, 0.0]], t: V3::default() };
        let mut out = vec![];
        let mut stack: Vec<(usize, Affine, bool)> = roots.into_iter().map(|r| (r, up, pick.is_none())).collect();
        while let Some((i, parent, inside)) = stack.pop() {
            let Some(n) = nodes.get(i) else { continue };
            let name = n["name"].as_str().unwrap_or("");
            if skip.iter().any(|s| s == name) {
                continue;
            }
            let world = parent.then(&self.local(n));
            let inside = inside || pick == Some(name);
            if inside {
                out.push((i, world));
            }
            for c in n["children"].as_array().cloned().unwrap_or_default() {
                if let Some(c) = c.as_u64() {
                    stack.push((c as usize, world, inside));
                }
            }
        }
        out
    }
}

fn material(m: &Value, path: &str) -> Material {
    let pbr = &m["pbrMetallicRoughness"];
    let ext = &m["extensions"];
    let num = |v: &Value, d: f64| v.as_f64().unwrap_or(d);
    let bc: Vec<f64> = pbr["baseColorFactor"].as_array().map(|a| a.iter().map(|x| num(x, 1.0)).collect()).unwrap_or(vec![1.0; 4]);
    let glow = num(&ext["KHR_materials_emissive_strength"]["emissiveStrength"], 1.0);
    let e: Vec<f64> = m["emissiveFactor"].as_array().map(|a| a.iter().map(|x| num(x, 0.0) * glow).collect()).unwrap_or(vec![0.0; 3]);
    let through = num(&ext["KHR_materials_transmission"]["transmissionFactor"], 0.0);
    let alpha = if m["alphaMode"].as_str() == Some("BLEND") { bc.get(3).copied().unwrap_or(1.0) } else { 1.0 };
    let texture = pbr["baseColorTexture"]["index"].as_u64().map(|_| (pbr["baseColorTexture"]["index"].clone(), path));
    Material {
        color: [bc[0], bc[1], bc[2]].map(|c| c * (1.0 - 0.9 * through)),
        metallic: num(&pbr["metallicFactor"], 1.0),
        roughness: num(&pbr["roughnessFactor"], 1.0),
        emission: [e[0], e[1], e[2]],
        opacity: alpha * (1.0 - 0.85 * through),
        coat: num(&ext["KHR_materials_clearcoat"]["clearcoatFactor"], 0.0),
        coat_roughness: num(&ext["KHR_materials_clearcoat"]["clearcoatRoughnessFactor"], 0.0),
        texture: texture.map(|(i, p)| format!("{p}#texture{i}")),
        ..Material::default()
    }
}

/// The meshes under node `pick` (or all), less the `skip` subtrees, as one mesh with a material slot per glTF material.
pub fn load(path: &str, pick: Option<&str>, skip: &[String]) -> Result<Mesh, String> {
    let d = doc(path)?;
    let j = &d.json;
    let mut m = Mesh::default();
    let mut slots: HashMap<i64, u16> = HashMap::new();
    let mut flat = false;
    for (ni, world) in d.walk(pick, skip) {
        let Some(mi) = j["nodes"][ni]["mesh"].as_u64() else { continue };
        let normal = world.inverse();
        let flip = world.det() < 0.0;
        for p in j["meshes"][mi as usize]["primitives"].as_array().cloned().unwrap_or_default() {
            if p["mode"].as_u64().unwrap_or(4) != 4 {
                continue;
            }
            let at = |k: &str| p["attributes"][k].as_u64().map(|i| d.accessor(i as usize)).transpose();
            let Some((pos, _)) = at("POSITION")? else { continue };
            let nrm = at("NORMAL")?;
            let uv = at("TEXCOORD_0")?;
            let count = pos.len() / 3;
            let idx: Vec<u32> = match p["indices"].as_u64() {
                Some(i) => d.accessor(i as usize)?.0.iter().map(|&x| x as u32).collect(),
                None => (0..count as u32).collect(),
            };
            let gm = p["material"].as_i64().unwrap_or(-1);
            let slot = *slots.entry(gm).or_insert_with(|| {
                let mat = &j["materials"][gm.max(0) as usize];
                let name = if gm < 0 { "default".to_string() } else { mat["name"].as_str().map(String::from).unwrap_or(format!("material{gm}")) };
                m.mats.push((name, if gm < 0 { Material::default() } else { material(mat, path) }));
                m.mats.len() as u16 - 1
            });
            let base = m.v.len() as u32;
            for k in 0..count {
                m.v.push(world.apply(v3(pos[k * 3], pos[k * 3 + 1], pos[k * 3 + 2])));
                match &nrm {
                    Some((n, _)) => m.n.push(normal.tdir(v3(n[k * 3], n[k * 3 + 1], n[k * 3 + 2])).norm()),
                    None => flat = true,
                }
                m.uv.push(uv.as_ref().map_or([0.0, 0.0], |(u, _)| [u[k * 2], 1.0 - u[k * 2 + 1]]));
            }
            for t in idx.chunks(3).filter(|t| t.len() == 3 && t.iter().all(|&i| (i as usize) < count)) {
                m.f.push(if flip { [base + t[0], base + t[2], base + t[1]] } else { [base + t[0], base + t[1], base + t[2]] });
                m.fm.push(slot);
            }
        }
    }
    if m.f.is_empty() {
        return Err(format!("{path}: no triangles{}", pick.map(|p| format!(" under node {p}")).unwrap_or_default()));
    }
    if flat {
        m.n.clear();
    }
    Ok(m)
}

/// The bytes of the image a material's `file#textureN` names.
pub fn texture_bytes(path: &str, tex: usize) -> Result<Vec<u8>, String> {
    let d = doc(path)?;
    let src = d.json["textures"][tex]["source"].as_u64().ok_or(format!("{path}: texture {tex}"))? as usize;
    let img = &d.json["images"][src];
    match (img["bufferView"].as_u64(), img["uri"].as_str()) {
        (Some(v), _) => Ok(d.view(v as usize).ok_or(format!("{path}: image {src}"))?.to_vec()),
        (None, Some(u)) => uri_bytes(u, std::path::Path::new(path).parent().unwrap_or(std::path::Path::new("."))),
        _ => Err(format!("{path}: image {src} has no data")),
    }
}

/// Every node: name, parent, whether it holds a mesh, and its world bounds in Z-up mm; and the material names.
pub fn outline(path: &str) -> Result<Value, String> {
    let d = doc(path)?;
    let nodes = d.json["nodes"].as_array().cloned().unwrap_or_default();
    let mut parent = vec![Value::Null; nodes.len()];
    for (i, n) in nodes.iter().enumerate() {
        for c in n["children"].as_array().cloned().unwrap_or_default() {
            if let Some(slot) = c.as_u64().and_then(|c| parent.get_mut(c as usize)) {
                *slot = nodes[i]["name"].clone();
            }
        }
    }
    let list: Vec<Value> = nodes.iter().enumerate().map(|(i, n)| {
        let name = n["name"].as_str().unwrap_or("");
        let b = load(path, Some(name), &[]).ok().map(|m| m.v.iter().fold(Aabb::EMPTY, |b, p| b.grow(*p)).to_json());
        json!({"name": name, "parent": parent[i], "mesh": n["mesh"].is_u64(), "bounds": b})
    }).collect();
    let mats: Vec<Value> = d.json["materials"].as_array().cloned().unwrap_or_default().iter().map(|m| m["name"].clone()).collect();
    Ok(json!({"nodes": list, "materials": mats}))
}
