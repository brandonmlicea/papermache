//! MCP tools for the studio: lamps, the timeline, explode, glTF import, films, beats and the cut.
use crate::anim::{self, Key};
use crate::light::Lamp;
use crate::mcp::merge;
use crate::render::shrink;
use crate::scene::{Object, Scene, Shape};
use crate::{audio, cut, film, gltf};
use base64::Engine;
use serde_json::{json, Value};

fn s<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str)
}

fn f(a: &Value, k: &str) -> Option<f64> {
    a.get(k).and_then(Value::as_f64)
}

fn names(sc: &Scene, a: &Value) -> Vec<String> {
    let mut out: Vec<String> = a.get("names").and_then(Value::as_array).map(|l| l.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    if let Some(t) = s(a, "tag") {
        out.extend(sc.objects.iter().filter(|o| o.tags.iter().any(|x| x == t)).map(|o| o.name.clone()));
    }
    out
}

fn thumb(path: &str, max: u32) -> Option<Value> {
    let (w, h, px) = crate::image::decode_bytes(&std::fs::read(path).ok()?, path).ok()?;
    let flat: Vec<u8> = px.concat();
    let small = crate::render::fit([w as u32, h as u32], Some(max));
    let img = shrink(&flat, w as u32, h as u32, small[0], small[1]);
    Some(json!({"type": "image", "data": base64::engine::general_purpose::STANDARD.encode(crate::render::png(small[0], small[1], &img)), "mimeType": "image/png"}))
}

/// Runs a studio tool on the scene; None when the name is not one of them.
pub fn call(sc: &mut Scene, name: &str, a: &Value) -> Option<Result<Vec<Value>, String>> {
    let text = |v: Value| vec![json!({"type": "text", "text": if let Value::String(s) = v { s } else { v.to_string() }})];
    let r = (|| -> Result<Vec<Value>, String> {
        Ok(text(match name {
            "lamp_set" => {
                let n = s(a, "name").ok_or("name")?.to_string();
                let lamps = &mut sc.lights.lamps;
                if a.get("remove").and_then(Value::as_bool) == Some(true) {
                    lamps.retain(|l| l.name != n);
                    return Ok(text(json!(format!("removed lamp {n}"))));
                }
                let at = lamps.iter().position(|l| l.name == n);
                let mut cur = serde_json::to_value(at.map(|i| lamps[i].clone()).unwrap_or(Lamp { name: n.clone(), ..Lamp::default() })).unwrap();
                merge(&mut cur, a);
                let lamp: Lamp = serde_json::from_value(cur).map_err(|e| e.to_string())?;
                match at {
                    Some(i) => lamps[i] = lamp,
                    None => lamps.push(lamp),
                }
                json!({"lamps": lamps})
            }
            "keyframe_set" => {
                let path = s(a, "path").ok_or("path")?.to_string();
                let keys: Vec<Key> = match a.get("keys") {
                    Some(k) => serde_json::from_value(k.clone()).map_err(|e| format!("keys: {e}"))?,
                    None => vec![Key { t: f(a, "t").ok_or("t or keys")?, v: a.get("value").cloned().ok_or("value")?, ease: a.get("ease").cloned() }],
                };
                let before = sc.timeline.clone();
                let tr = anim::track(sc, &path);
                tr.put(keys);
                if let Some(sp) = a.get("spline").and_then(Value::as_bool) {
                    tr.spline = sp;
                }
                let probe = tr.keys.first().map(|k| k.t).unwrap_or(0.0);
                if let Err(e) = anim::at(sc, probe) {
                    sc.timeline = before;
                    return Err(e);
                }
                json!({"path": path, "keys": anim::track(sc, &path).keys.len(), "length": anim::length(sc)})
            }
            "timeline" => {
                match s(a, "clear") {
                    Some("all") => sc.timeline.clear(),
                    Some(p) => sc.timeline.retain(|t| t.path != p),
                    None => {}
                }
                json!({"length": anim::length(sc), "tracks": sc.timeline.iter().map(|t| json!({"path": t.path, "keys": t.keys.len(),
                    "from": t.keys.first().map(|k| k.t), "to": t.keys.last().map(|k| k.t), "spline": t.spline})).collect::<Vec<_>>()})
            }
            "explode" => {
                let list = names(sc, a);
                let times: Vec<f64> = a.get("times").and_then(Value::as_array).map(|t| t.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
                let offsets = a.get("offsets").and_then(Value::as_object).cloned().unwrap_or_default();
                let done = anim::explode(sc, &list, &offsets, f(a, "distance").unwrap_or(30.0), &times, a.get("ease").cloned().or(Some(json!("inout"))))?;
                json!({"exploded": done, "times": times})
            }
            "import_info" => gltf::outline(&crate::scene::resolve(s(a, "file").ok_or("file")?))?,
            "import" => {
                let file = s(a, "file").ok_or("file")?.to_string();
                let node = s(a, "node").map(String::from);
                let mut picks = vec![node.clone()];
                if a.get("split").and_then(Value::as_bool) == Some(true) {
                    let o = gltf::outline(&crate::scene::resolve(&file))?;
                    let under = |n: &Value| node.is_none() || n["parent"].as_str() == node.as_deref() || n["name"].as_str() == node.as_deref();
                    picks = o["nodes"].as_array().unwrap().iter().filter(|n| n["mesh"].as_bool() == Some(true) && under(n)).map(|n| n["name"].as_str().map(String::from)).collect();
                }
                let mut added = vec![];
                for p in picks {
                    let mut v = json!({"shape": "mesh", "file": file, "node": p, "name": p.clone().or(s(a, "name").map(String::from)).unwrap_or("model".into())});
                    let mut extra = a.clone();
                    let m = extra.as_object_mut().unwrap();
                    for k in ["file", "node", "split", "name"] {
                        m.remove(k);
                    }
                    merge(&mut v, &extra);
                    let o: Object = serde_json::from_value(v).map_err(|e| e.to_string())?;
                    if !matches!(o.shape, Shape::Mesh { .. }) {
                        return Err("import makes meshes".into());
                    }
                    crate::scene::parts(&o)?;
                    sc.objects.push(o);
                    sc.name_all();
                    added.push(sc.objects.last().unwrap().name.clone());
                }
                json!({"added": added})
            }
            "film" => {
                let o = film::Opts {
                    dir: s(a, "dir").ok_or("dir")?.into(),
                    fps: f(a, "fps").unwrap_or(sc.render.fps),
                    from: f(a, "from").unwrap_or(0.0),
                    to: f(a, "to"),
                    samples: f(a, "samples").map(|v| v as u32),
                    scale: f(a, "scale").unwrap_or(1.0),
                    mp4: a.get("mp4").and_then(Value::as_bool).unwrap_or(true),
                    resume: a.get("resume").and_then(Value::as_bool).unwrap_or(false),
                };
                let r = film::film(sc, &o, &mut |_, _, _| {})?;
                let mid = (r["frames"].as_u64().unwrap_or(1) / 2).max(1);
                let shown = thumb(&std::path::Path::new(&o.dir).join(format!("frame_{mid:05}.png")).to_string_lossy(), 480);
                return Ok(shown.into_iter().chain(text(r)).collect());
            }
            "beats" => serde_json::to_value(audio::beats(&crate::scene::resolve(s(a, "file").ok_or("file")?))?).unwrap(),
            "cut" => {
                let c: cut::Cut = match a.get("cut") {
                    Some(Value::String(p)) => {
                        crate::scene::set_base(p);
                        serde_json::from_str(&std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?).map_err(|e| e.to_string())?
                    }
                    Some(v) => serde_json::from_value(v.clone()).map_err(|e| e.to_string())?,
                    None => return Err("cut: a cut object or file".into()),
                };
                let out = s(a, "out").ok_or("out")?.to_string();
                let dir = s(a, "frames").map(String::from).unwrap_or(format!("{out}.frames"));
                let here = match &c.scene {
                    Some(p) => Scene::load(&crate::scene::resolve(p))?,
                    None => sc.clone(),
                };
                cut::cut(&c, &here, &dir, &out, f(a, "samples").map(|v| v as u32), f(a, "scale").unwrap_or(1.0), &mut |_, _, _| {})?
            }
            "agentcut" => {
                let start = a.get("start").cloned().unwrap_or(json!(0));
                crate::agentcut::send_file(s(a, "file").ok_or("file")?, s(a, "url"), s(a, "key"), s(a, "project").unwrap_or("Paper Mache"), &start)?
            }
            _ => return Err(String::new()),
        }))
    })();
    match r {
        Err(e) if e.is_empty() => None,
        r => Some(r),
    }
}
