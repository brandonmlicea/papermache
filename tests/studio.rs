use papermache::anim::{self, ease, Key, Track};
use papermache::math::*;
use papermache::mcp::Server;
use papermache::model::{extrude, lathe, rounded_rect, Mesh};
use papermache::scene::{compile, Scene};
use papermache::subdiv::{subdivide, weld};
use papermache::{agentcut, cut, gltf};
use serde_json::{json, Value};

const GLASSES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/innerspeak-ad/glasses.glb");

fn volume(m: &Mesh) -> f64 {
    m.f.iter().map(|f| m.v[f[0] as usize].dot(m.face_normal(*f))).sum::<f64>() / 6.0
}

fn edges_shared_twice(m: &Mesh) -> bool {
    let w = weld(m);
    let mut count = std::collections::HashMap::new();
    for f in &w.f {
        for k in 0..3 {
            let (a, b) = (f[k], f[(k + 1) % 3]);
            *count.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    count.values().all(|&c| c == 2)
}

#[test]
fn lathe_turns_a_closed_cylinder_with_flutes() {
    let profile = [[0.0, 0.0], [5.0, 0.0], [5.0, 10.0], [0.0, 10.0]];
    let m = lathe(&profile, 256, 0, 0.0, None);
    let v = volume(&m);
    let exact = std::f64::consts::PI * 25.0 * 10.0;
    assert!(v > 0.0 && (v - exact).abs() / exact < 0.002, "a turned rectangle is a cylinder: {v} vs {exact}");
    assert!(edges_shared_twice(&m), "closed and watertight");
    let knurl = lathe(&profile, 64, 24, 0.4, None);
    let radii: Vec<f64> = knurl.v.iter().filter(|p| p.x.hypot(p.y) > 1.0).map(|p| p.x.hypot(p.y)).collect();
    let (lo, hi) = radii.iter().fold((f64::MAX, 0.0f64), |a, r| (a.0.min(*r), a.1.max(*r)));
    assert!((hi - 5.0).abs() < 1e-9 && (lo - 4.6).abs() < 0.02, "ridges cut 0.4 deep: {lo}..{hi}");
}

#[test]
fn extrude_makes_buttons_and_frames() {
    let solid = extrude(&rounded_rect([20.0, 10.0], 0.0, 4), 4.0, 0.0, 1, None);
    assert!((volume(&solid) - 800.0).abs() < 1e-6, "a 20 x 10 x 4 block: {}", volume(&solid));
    assert!(edges_shared_twice(&solid));
    let round = extrude(&rounded_rect([20.0, 10.0], 5.0, 16), 2.0, 0.5, 4, None);
    let pill = 2.0 * (10.0 * 10.0 + std::f64::consts::PI * 25.0);
    assert!(volume(&round) < pill && volume(&round) > pill * 0.9, "bevels trim a little off a pill");
    assert!(round.v.iter().all(|p| p.z.abs() <= 1.0 + 1e-9 && p.x.abs() <= 10.0 + 1e-9));
    let frame = extrude(&rounded_rect([20.0, 20.0], 0.0, 4), 2.0, 0.0, 1, Some(5.0));
    assert!((volume(&frame) - (400.0 - 100.0) * 2.0).abs() < 1e-6, "a 5 mm wall leaves a 10 x 10 hole");
    assert!(edges_shared_twice(&frame));
}

#[test]
fn loop_subdivision_quadruples_and_shrinks_toward_the_limit() {
    let cube = extrude(&rounded_rect([2.0, 2.0], 0.0, 4), 2.0, 0.0, 1, None);
    let w = weld(&cube);
    let s1 = subdivide(&cube, 1);
    let s2 = subdivide(&cube, 2);
    assert_eq!(s1.f.len(), w.f.len() * 4);
    assert_eq!(s2.f.len(), w.f.len() * 16);
    assert!(edges_shared_twice(&s2), "stays closed");
    let r = |m: &Mesh| m.v.iter().map(|p| p.len()).fold(0.0, f64::max);
    assert!(r(&s2) < r(&s1) && r(&s1) < 3f64.sqrt(), "corners round off");
    assert!(volume(&s2) > 3.0 && volume(&s2) < 8.0, "a rounded blob inside the cube: {}", volume(&s2));
    let smooth = s2.smooth(80.0);
    assert!(smooth.n.iter().all(|n| (n.len() - 1.0).abs() < 1e-9) && smooth.n.len() == smooth.f.len() * 3);
}

#[test]
fn glb_import_brings_nodes_materials_and_normals() {
    let all = gltf::load(GLASSES, None, &[]).unwrap();
    assert!(all.f.len() > 10_000 && all.n.len() == all.v.len(), "{} faces", all.f.len());
    assert_eq!(all.mats.len(), 11);
    let dot = gltf::load(GLASSES, Some("armL_dot1"), &[]).unwrap();
    let (name, m) = &dot.mats[0];
    assert_eq!(name, "dot");
    assert!(m.emission[2] > 1.3 && m.metallic == 0.0, "emissive strength carried: {:?}", m.emission);
    let b = dot.v.iter().fold(Aabb::EMPTY, |b, p| b.grow(*p));
    assert!(b.lo.x > 650.0 && b.hi.x < 660.0 && b.lo.z > 120.0, "Y-up metres to Z-up mm, placed by its parent: {:?}", b.to_json());
    let arm = gltf::load(GLASSES, Some("armL"), &["armL_dot0".into(), "armL_dot1".into(), "armL_dot2".into()]).unwrap();
    let with = gltf::load(GLASSES, Some("armL"), &[]).unwrap();
    assert_eq!(with.f.len() - arm.f.len(), 3 * dot.f.len(), "skip leaves the three dots out");
    let lens = gltf::load(GLASSES, Some("front_lens"), &[]).unwrap();
    assert!(lens.mats[0].1.opacity < 0.3, "transmission reads as see-through");
}

#[test]
fn keyframes_ease_spline_and_step() {
    assert_eq!(ease(None, 0.25), 0.25);
    assert!((ease(Some(&json!("inout")), 0.5) - 0.5).abs() < 1e-12 && ease(Some(&json!("inout")), 0.25) < 0.25);
    assert!(ease(Some(&json!("in")), 0.5) < 0.5 && ease(Some(&json!("out")), 0.5) > 0.5);
    let css = ease(Some(&json!([0.42, 0.0, 0.58, 1.0])), 0.5);
    assert!((css - 0.5).abs() < 1e-6, "a symmetric bezier passes the middle: {css}");
    let key = |t: f64, v: Value, e: Option<Value>| Key { t, v, ease: e };
    let tr = Track { path: "camera.eye".into(), keys: vec![key(0.0, json!([0, 0, 0]), None), key(2.0, json!([10, 20, 30]), None)], spline: false };
    assert_eq!(tr.at(1.0).unwrap(), json!([5.0, 10.0, 15.0]));
    assert_eq!(tr.at(-1.0).unwrap(), json!([0, 0, 0]));
    assert_eq!(tr.at(9.0).unwrap(), json!([10, 20, 30]));
    let step = Track { path: "x".into(), keys: vec![key(0.0, json!(1), Some(json!("step"))), key(1.0, json!(5), None)], spline: false };
    assert_eq!(step.at(0.99).unwrap(), json!(1.0));
    let sp = Track { path: "x".into(), keys: (0..4).map(|i| key(i as f64, json!(i * i), None)).collect(), spline: true };
    let mid = sp.at(1.5).unwrap().as_f64().unwrap();
    assert!((mid - 2.25).abs() < 0.2 && mid != 2.5, "a spline bends with its neighbours: {mid}");
    let s: Scene = serde_json::from_value(json!({
        "materials": {"dot": {"emission": [0, 0, 0]}},
        "objects": [{"name": "a", "shape": "sphere", "radius": 1, "tags": ["g"]}, {"name": "b", "shape": "box", "size": [1, 1, 1], "tags": ["g"]}, {"name": "c", "shape": "sphere", "radius": 1}],
        "lights": {"lamps": [{"name": "key", "strength": 1}]},
        "timeline": [
            {"path": "objects.tag:g.rotation", "keys": [{"t": 0, "v": [0, 0, 0]}, {"t": 1, "v": [0, 0, 90]}]},
            {"path": "materials.dot.emission", "keys": [{"t": 0, "v": [0, 0, 0]}, {"t": 1, "v": [0, 0, 4]}]},
            {"path": "lights.lamps.key.strength", "keys": [{"t": 0, "v": 1}, {"t": 1, "v": 3}]},
            {"path": "objects.c.hidden", "keys": [{"t": 0, "v": false, "ease": "step"}, {"t": 0.5, "v": true}]}
        ]})).unwrap();
    let half = anim::at(&s, 0.5).unwrap();
    assert_eq!(half.objects[0].rotation, [0.0, 0.0, 45.0]);
    assert_eq!(half.objects[1].rotation, [0.0, 0.0, 45.0]);
    assert_eq!(half.objects[2].rotation, [0.0, 0.0, 0.0], "only the tagged objects turn");
    assert!(half.objects[2].hidden && !anim::at(&s, 0.4).unwrap().objects[2].hidden);
    assert_eq!(half.materials["dot"].emission, [0.0, 0.0, 2.0]);
    assert_eq!(half.lights.lamps[0].strength, 2.0);
    assert_eq!(anim::length(&s), 1.0);
}

#[test]
fn explode_keys_parts_away_from_the_centre_and_back() {
    let mut s: Scene = serde_json::from_value(json!({"objects": [
        {"name": "l", "shape": "box", "size": [2, 2, 2], "position": [-5, 0, 0]}, {"name": "r", "shape": "box", "size": [2, 2, 2], "position": [5, 0, 0]}]})).unwrap();
    anim::explode(&mut s, &["l".into(), "r".into()], &Default::default(), 10.0, &[0.0, 1.0, 2.0, 3.0], None).unwrap();
    let apart = anim::at(&s, 1.5).unwrap();
    assert_eq!(apart.objects[0].position, [-15.0, 0.0, 0.0]);
    assert_eq!(apart.objects[1].position, [15.0, 0.0, 0.0]);
    assert_eq!(anim::at(&s, 3.0).unwrap().objects[1].position, [5.0, 0.0, 0.0]);
}

#[test]
fn shots_land_on_beats() {
    let shot = |beats: f64| cut::Shot { name: String::new(), scene: None, from: 0.0, to: None, beats, speed: None };
    let beats: Vec<f64> = (0..8).map(|k| 0.25 + k as f64 * 0.5).collect();
    let spans = cut::lay(&[shot(2.0), shot(4.0), shot(4.0)], &beats, 0.5, 1);
    assert_eq!(spans, vec![(0.0, 1.0), (1.0, 3.0), (3.0, 5.0)], "past the last beat it keeps the tempo");
}

#[test]
fn agentcut_requests_are_built_without_the_network() {
    let up = agentcut::upload("http://localhost:8791/", "k1", "ad film.mp4", agentcut::Media { duration: Some(8.0), width: Some(640), height: Some(360) }, vec![1, 2, 3]);
    assert_eq!(up.method, "PUT");
    assert_eq!(up.url, "http://localhost:8791/api/assets?name=ad%20film.mp4&duration=8.000&width=640&height=360");
    assert!(up.headers.contains(&("authorization".into(), "Bearer k1".into())) && up.headers.contains(&("content-type".into(), "video/mp4".into())));
    assert_eq!(up.body, vec![1, 2, 3]);
    assert_eq!(agentcut::mime("still.PNG"), "image/png");
    let call = agentcut::place("https://agentcut.example", "k1", "Launch", "a123", &json!("0:15"));
    assert_eq!(call.url, "https://agentcut.example/mcp");
    let body: Value = serde_json::from_slice(&call.body).unwrap();
    assert_eq!(body["method"], "tools/call");
    assert_eq!(body["params"]["name"], "edit_video");
    assert_eq!(body["params"]["arguments"], json!({"project": "Launch", "ops": [{"op": "add_clip", "asset": "a123", "start": "0:15"}]}));
    assert_eq!(agentcut::reply("event: message\ndata: {\"result\": 1}\n").unwrap()["result"], 1);
    let png = papermache::render::png(3, 2, &[0; 24]);
    let m = agentcut::measure("x.png", &png);
    assert_eq!((m.width, m.height, m.duration), (Some(3), Some(2), None));
}

#[test]
fn studio_renders_lamps_coat_patterns_dof_and_denoise() {
    let s: Scene = serde_json::from_value(json!({
        "materials": {"cyc": {"color": [0.5, 0.5, 0.5]}, "steel": {"color": [0.8, 0.8, 0.8], "metallic": 1, "roughness": 0.3, "pattern": {"kind": "spun"}},
            "ink": {"color": [0.02, 0.02, 0.02], "coat": 1}},
        "objects": [{"shape": "backdrop", "size": [300, 200, 100]}, {"name": "crown", "shape": "lathe", "profile": [[0, 0], [6, 0], [6, 6], [0, 6]], "ridges": 20, "ridge_depth": 0.3, "material": "steel"},
            {"shape": "extrude", "size": [20, 10], "radius": 3, "depth": 3, "bevel": 0.5, "position": [15, 0, 1.5], "material": "ink"}],
        "camera": {"eye": [0, -80, 40], "look": [0, 0, 3], "size": [64, 40], "fstop": 2, "focus_on": "crown"},
        "lights": {"sun": {"strength": 0}, "sky": {"strength": 0}, "background": [0, 0, 0], "lamps": [{"name": "key", "position": [0, -60, 80], "size": [60, 40], "strength": 2}]},
        "render": {"noise": 0.02, "denoise": true, "bloom": 0.2, "filmic": true}
    })).unwrap();
    let c = compile(&s).unwrap();
    let f = papermache::render::render_hdr(&s, &c, [64, 40], 16);
    assert!(f.samples < 64 * 40 * 16, "settled pixels stop early: {} samples", f.samples);
    let px = papermache::render::render(&s, &c, [64, 40], 16);
    let mean = px.chunks(4).map(|p| p[0] as f64).sum::<f64>() / (64.0 * 40.0);
    assert!(mean > 10.0, "the area lamp lights the studio: {mean}");
    let mut dark = s.clone();
    dark.lights.lamps.clear();
    let unlit = papermache::render::render(&dark, &compile(&dark).unwrap(), [64, 40], 4);
    assert!(unlit.chunks(4).all(|p| p[0] < 4), "no sun, sky or lamp: black");
}

#[test]
fn mcp_studio_tools_build_and_film_a_scene() {
    let mut srv = Server::default();
    let mut call = |name: &str, args: Value| -> Value {
        let r: Value = serde_json::from_str(&srv.handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}}).to_string()).unwrap()).unwrap();
        assert_eq!(r["result"]["isError"], false, "{name}: {r}");
        r["result"]["content"].clone()
    };
    let names: Vec<String> = serde_json::from_str::<Value>(&Server::default().handle(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap()).unwrap()["result"]["tools"]
        .as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    for t in ["lamp_set", "import", "import_info", "keyframe_set", "timeline", "explode", "film", "beats", "cut", "agentcut"] {
        assert!(names.contains(&t.to_string()), "{t} listed");
    }
    let info = call("import_info", json!({"file": GLASSES}));
    assert!(info[0]["text"].as_str().unwrap().contains("armL_dot2"));
    let added = call("import", json!({"file": GLASSES, "node": "armL", "split": true, "scale": [0.1, 0.1, 0.1], "tags": ["arm"]}));
    assert!(added[0]["text"].as_str().unwrap().contains("armL_battery"));
    call("lamp_set", json!({"name": "key", "kind": "area", "position": [0, -100, 100], "look": [60, 30, 10], "strength": 3}));
    call("keyframe_set", json!({"path": "materials.glow.emission", "keys": [{"t": 0, "v": [0, 0, 0]}, {"t": 0.1, "v": [0, 1, 4]}]}));
    call("explode", json!({"tag": "arm", "distance": 10, "times": [0, 0.1]}));
    call("camera_frame", json!({"tag": "arm"}));
    let dir = std::env::temp_dir().join("pm-film-test");
    let _ = std::fs::remove_dir_all(&dir);
    let r = call("film", json!({"dir": dir.to_str().unwrap(), "fps": 20, "to": 0.1, "samples": 1, "scale": 0.03, "mp4": false}));
    assert_eq!(r[0]["type"], "image");
    assert!(dir.join("frame_00002.png").exists() && !dir.join("frame_00003.png").exists());
}
