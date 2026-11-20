use crate::math::*;
use crate::render::{fit, png, render, shrink, Cam};
use crate::scene::{bounds, compile, parts, Material, Object, Scene};
use crate::tools::tools;
use base64::Engine;
use serde_json::{json, Map, Value};
use std::io::{BufRead, Write};

const INSTRUCTIONS: &str = "Paper Mache is a 3D modeling, animation and rendering scene you build with tools. Units are millimetres, Z is up, rotations are degrees (Blender XYZ euler). Start with scene_info (or scene_load a saved scene), add objects with object_add (box with a bevel radius, cylinder/cone, sphere, torus, rod, tube, plane, text, image (a .png raised into a solid), lathe (a turned profile, ridges for knurled crowns), extrude (an outline or rounded rectangle with depth, bevel and wall: buttons, bezels, frames), backdrop (a studio sweep), mesh from .obj/.stl/.glb with smooth and subdivide), or import a .glb part by part (import_info lists its nodes). Give them materials with material_set (coat, texture, pattern brushed/spun/noise/gradient/checker, screen playing a frame sequence), light with lights_set and lamp_set (area, spot, point), aim with camera_frame or camera_set (fstop and focus_on for depth of field), animate with keyframe_set and explode, look with render (time picks the moment; keep max_size about 600-800 while iterating), then film the timeline, find a song's beats and cut shots on them. Save with scene_save and export .obj/.stl with export.";

pub struct Server {
    pub scene: Scene,
}

impl Default for Server {
    fn default() -> Self {
        Server { scene: Scene::default() }
    }
}

pub fn run_stdio() {
    let mut srv = Server::default();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = srv.handle(&line) {
            let _ = writeln!(out, "{resp}");
            let _ = out.flush();
        }
    }
}

fn text(v: Value) -> Vec<Value> {
    vec![json!({"type": "text", "text": if let Value::String(s) = v { s } else { serde_json::to_string_pretty(&v).unwrap() }})]
}

/// Deep merge: objects merge key by key, everything else replaces.
pub fn merge(a: &mut Value, b: &Value) {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                merge(a.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (a, b) => *a = b.clone(),
    }
}

fn s<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str)
}

fn v3a(a: &Value, k: &str) -> Option<V3> {
    let x = a.get(k)?.as_array()?;
    Some(v3(x.first()?.as_f64()?, x.get(1)?.as_f64()?, x.get(2)?.as_f64()?))
}

impl Server {
    pub fn handle(&mut self, line: &str) -> Option<String> {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}")}}).to_string()),
        };
        let id = v.get("id")?.clone();
        let params = v.get("params").cloned().unwrap_or(json!({}));
        let result = match s(&v, "method").unwrap_or("") {
            "initialize" => json!({
                "protocolVersion": s(&params, "protocolVersion").unwrap_or("2025-06-18"),
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "paper-mache", "title": "Paper Mache", "version": env!("CARGO_PKG_VERSION")},
                "instructions": INSTRUCTIONS
            }),
            "ping" => json!({}),
            "tools/list" => json!({"tools": tools()}),
            "tools/call" => {
                let name = s(&params, "name").unwrap_or("").to_string();
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                match self.call(&name, &args) {
                    Ok(content) => json!({"content": content, "isError": false}),
                    Err(e) => json!({"content": text(json!(e)), "isError": true}),
                }
            }
            m => return Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("no method {m}")}}).to_string()),
        };
        Some(json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string())
    }

    fn targets(&self, a: &Value) -> Result<Vec<usize>, String> {
        let hit: Vec<usize> = match (s(a, "name"), s(a, "tag")) {
            (Some(n), _) => self.scene.objects.iter().position(|o| o.name == n).into_iter().collect(),
            (None, Some(t)) => (0..self.scene.objects.len()).filter(|&i| self.scene.objects[i].tags.iter().any(|x| x == t)).collect(),
            _ => return Err("give name or tag".into()),
        };
        if hit.is_empty() {
            return Err(format!("no object with {}", a.get("name").or(a.get("tag")).unwrap_or(&Value::Null)));
        }
        Ok(hit)
    }

    fn add(&mut self, v: &Value) -> Result<String, String> {
        let o: Object = serde_json::from_value(v.clone()).map_err(|e| format!("object {v}: {e}"))?;
        parts(&o)?;
        self.scene.objects.push(o);
        self.scene.name_all();
        Ok(self.scene.objects.last().unwrap().name.clone())
    }

    pub fn call(&mut self, name: &str, a: &Value) -> Result<Vec<Value>, String> {
        let sc = &mut self.scene;
        Ok(text(match name {
            "scene_info" => info(sc, s(a, "tag")),
            "scene_clear" => {
                *sc = Scene::default();
                json!("cleared")
            }
            "scene_load" => {
                *sc = Scene::load(s(a, "path").ok_or("path")?)?;
                json!(format!("loaded {} objects, {} materials", sc.objects.len(), sc.materials.len()))
            }
            "scene_save" => {
                let p = s(a, "path").ok_or("path")?;
                std::fs::write(p, serde_json::to_string_pretty(sc).unwrap()).map_err(|e| format!("{p}: {e}"))?;
                json!(format!("saved {} objects to {p}", sc.objects.len()))
            }
            "object_add" => {
                let list = match (a.get("objects"), a.get("object")) {
                    (Some(Value::Array(l)), _) => l.clone(),
                    (_, Some(o)) => vec![o.clone()],
                    _ => vec![a.clone()],
                };
                let names = list.iter().map(|o| self.add(o)).collect::<Result<Vec<_>, _>>()?;
                json!({"added": names})
            }
            "object_update" => {
                let idx = self.targets(a)?;
                let sc = &mut self.scene;
                let mut patch = a.clone();
                let p = patch.as_object_mut().unwrap();
                for k in ["name", "tag", "new_name"] {
                    p.remove(k);
                }
                for &i in &idx {
                    let mut cur = serde_json::to_value(&sc.objects[i]).unwrap();
                    merge(&mut cur, &patch);
                    if let Some(n) = s(a, "new_name") {
                        cur["name"] = json!(n);
                    }
                    let o: Object = serde_json::from_value(cur).map_err(|e| e.to_string())?;
                    parts(&o)?;
                    sc.objects[i] = o;
                }
                sc.name_all();
                json!(format!("updated {}", idx.len()))
            }
            "object_remove" => {
                let idx = self.targets(a)?;
                let sc = &mut self.scene;
                let mut k = 0;
                sc.objects.retain(|_| {
                    k += 1;
                    !idx.contains(&(k - 1))
                });
                json!(format!("removed {}", idx.len()))
            }
            "object_array" => {
                let i = *self.targets(&json!({"name": s(a, "name").ok_or("name")?}))?.first().unwrap();
                let count = a.get("count").and_then(Value::as_u64).ok_or("count")?.min(10_000) as usize;
                let off = v3a(a, "offset").unwrap_or_default();
                let rot = v3a(a, "rotate").unwrap_or_default();
                let src = self.scene.objects[i].clone();
                let mut names = vec![];
                for k in 1..=count {
                    let mut o = src.clone();
                    o.name = format!("{}.{k}", src.name);
                    o.position = (V3::from(o.position) + off * k as f64).arr();
                    o.rotation = (V3::from(o.rotation) + rot * k as f64).arr();
                    self.scene.objects.push(o);
                    names.push(self.scene.objects.last().unwrap().name.clone());
                }
                self.scene.name_all();
                json!({"added": names})
            }
            "material_set" => {
                let n = s(a, "name").ok_or("name")?.to_string();
                let mut cur = serde_json::to_value(sc.materials.get(&n).cloned().unwrap_or_default()).unwrap();
                let mut patch = a.clone();
                patch.as_object_mut().unwrap().remove("name");
                merge(&mut cur, &patch);
                sc.materials.insert(n.clone(), serde_json::from_value::<Material>(cur).map_err(|e| e.to_string())?);
                json!(format!("material {n}"))
            }
            "camera_set" => {
                let mut cur = serde_json::to_value(&sc.camera).unwrap();
                merge(&mut cur, a);
                sc.camera = serde_json::from_value(cur).map_err(|e| e.to_string())?;
                serde_json::to_value(&sc.camera).unwrap()
            }
            "camera_frame" => frame(sc, a)?,
            "lights_set" => {
                let mut cur = serde_json::to_value(&sc.lights).unwrap();
                merge(&mut cur, a);
                sc.lights = serde_json::from_value(cur).map_err(|e| e.to_string())?;
                serde_json::to_value(&sc.lights).unwrap()
            }
            "render" => return self.render(a),
            "project" => {
                let cam = Cam::new(&sc.camera, sc.camera.size);
                let pts = a.get("points").and_then(Value::as_array).ok_or("points")?;
                json!(pts.iter().map(|p| v3a(&json!({"p": p}), "p").map(|p| cam.project(p))).collect::<Vec<_>>())
            }
            "export" => json!(crate::mesh::export(sc, s(a, "path").ok_or("path")?, s(a, "tag"))?),
            _ => return crate::studio::call(&mut self.scene, name, a).unwrap_or(Err(format!("unknown tool {name}"))),
        }))
    }

    fn render(&mut self, a: &Value) -> Result<Vec<Value>, String> {
        let sc = &mut self.scene;
        if let Some(sz) = a.get("size") {
            sc.camera.size = serde_json::from_value(sz.clone()).map_err(|e| format!("size: {e}"))?;
        }
        let samples = a.get("samples").and_then(Value::as_u64).unwrap_or(8) as u32;
        let max = a.get("max_size").and_then(Value::as_u64).unwrap_or(800) as u32;
        let path = s(a, "path");
        let full = sc.camera.size;
        let size = if path.is_some() { full } else { fit(full, Some(max)) };
        let t = std::time::Instant::now();
        let sc = &crate::anim::at(sc, a.get("time").and_then(Value::as_f64).unwrap_or(0.0))?;
        let c = compile(sc)?;
        let px = render(sc, &c, size, samples);
        if let Some(p) = path {
            std::fs::write(p, png(size[0], size[1], &px)).map_err(|e| format!("{p}: {e}"))?;
        }
        let small = fit(size, Some(max));
        let shown = if small == size { px } else { shrink(&px, size[0], size[1], small[0], small[1]) };
        let cam = Cam::new(&sc.camera, full);
        let points: Map<String, Value> = sc.points.iter().map(|(k, p)| (k.clone(), json!(cam.project(V3::from(*p))))).collect();
        let note = json!({"file": path, "size": size, "shown": small, "ms": t.elapsed().as_millis() as u64, "primitives": c.prims.len(), "points": points});
        Ok(vec![
            json!({"type": "image", "data": base64::engine::general_purpose::STANDARD.encode(png(small[0], small[1], &shown)), "mimeType": "image/png"}),
            json!({"type": "text", "text": note.to_string()}),
        ])
    }
}

pub fn info(sc: &Scene, tag: Option<&str>) -> Value {
    let pick: Vec<&Object> = sc.objects.iter().filter(|o| tag.is_none_or(|t| o.tags.iter().any(|x| x == t))).collect();
    let mut all = Aabb::EMPTY;
    let mut shapes: std::collections::BTreeMap<&str, usize> = Default::default();
    let mut list = vec![];
    for o in &pick {
        let b = bounds(o);
        if !o.hidden && !b.is_empty() {
            all = all.union(b);
        }
        *shapes.entry(o.shape.name()).or_default() += 1;
        if list.len() < 300 {
            list.push(json!({"name": o.name, "shape": o.shape.name(), "material": o.material, "tags": o.tags, "hidden": o.hidden,
                "bounds": if b.is_empty() { Value::Null } else { json!(b.to_json()) }}));
        }
    }
    json!({
        "objects": pick.len(), "shapes": shapes, "bounds": if all.is_empty() { Value::Null } else { json!(all.to_json()) },
        "materials": sc.materials, "camera": sc.camera, "lights": sc.lights,
        "list": list, "note": if pick.len() > 300 { format!("first 300 of {}; filter with tag", pick.len()) } else { String::new() }
    })
}

fn frame(sc: &mut Scene, a: &Value) -> Result<Value, String> {
    let tag = s(a, "tag");
    let b = sc.objects.iter().filter(|o| !o.hidden && tag.is_none_or(|t| o.tags.iter().any(|x| x == t))).map(bounds).fold(Aabb::EMPTY, Aabb::union);
    if b.is_empty() {
        return Err("nothing to frame".into());
    }
    let dir = v3a(a, "direction").unwrap_or(v3(1.0, -1.2, 0.9)).norm();
    let margin = a.get("margin").and_then(Value::as_f64).unwrap_or(1.1);
    if let Some(p) = s(a, "projection") {
        sc.camera.projection = p.into();
    }
    let c = b.centroid();
    let r = ((b.hi - b.lo).len() / 2.0).max(1e-3) * margin;
    let [w, h] = sc.camera.size.map(|v| v.max(1) as f64);
    let aspect = w.max(h) / w.min(h);
    let cam = &mut sc.camera;
    let dist = if cam.projection.starts_with("ortho") {
        cam.width = 2.0 * r * aspect;
        r * 4.0 + 10.0
    } else {
        let half = cam.fov.map(|f| (f.to_radians() / 2.0).tan()).unwrap_or(18.0 / cam.lens) / aspect;
        r / half.atan().sin()
    };
    cam.eye = (c + dir * dist).arr();
    cam.look = c.arr();
    Ok(serde_json::to_value(&sc.camera).unwrap())
}
