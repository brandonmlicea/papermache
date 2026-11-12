use papermache::geom::{Kind, Prim};
use papermache::math::*;
use papermache::mcp::Server;
use papermache::render::Cam;
use papermache::scene::{compile, parts, Camera, Object, Scene};
use serde_json::{json, Value};

fn prim(o: Value) -> Prim {
    let o: Object = serde_json::from_value(o).unwrap();
    let (a, k) = parts(&o).unwrap().remove(0);
    Prim::new(k, &a, 0, 0)
}

fn down(p: &Prim, x: f64, y: f64) -> Option<f64> {
    p.hit(v3(x, y, 100.0), v3(0.0, 0.0, -1.0), 1e-6, f64::INFINITY).map(|h| 100.0 - h.t)
}

#[test]
fn box_hits_its_top_and_turns() {
    let b = prim(json!({"shape": "box", "size": [10, 4, 2], "position": [0, 0, 1], "rotation": [0, 0, 90]}));
    assert!((down(&b, 0.0, 4.0).unwrap() - 2.0).abs() < 1e-9, "turned 90 degrees, the 10 mm side runs along y");
    assert!(down(&b, 4.0, 0.0).is_none());
    let h = b.hit(v3(-50.0, 0.0, 1.0), v3(1.0, 0.0, 0.0), 1e-6, f64::INFINITY).unwrap();
    assert!((h.t - 48.0).abs() < 1e-9 && (h.n.x + 1.0).abs() < 1e-9);
}

#[test]
fn rounded_box_and_torus() {
    let r = prim(json!({"shape": "box", "size": [10, 10, 10], "radius": 2}));
    assert!((down(&r, 0.0, 0.0).unwrap() - 5.0).abs() < 1e-3);
    let corner = down(&r, 4.9, 4.9);
    assert!(corner.is_none_or(|z| z < 4.0), "the bevel cuts the corner");
    let t = prim(json!({"shape": "torus", "major": 10, "minor": 2}));
    assert!((down(&t, 10.0, 0.0).unwrap() - 2.0).abs() < 1e-3);
    assert!(down(&t, 0.0, 0.0).is_none());
}

#[test]
fn cylinder_cone_and_ellipsoid() {
    let c = prim(json!({"shape": "cylinder", "radius": 5, "height": 10, "position": [0, 0, 5]}));
    assert!((down(&c, 3.0, 0.0).unwrap() - 10.0).abs() < 1e-9);
    assert!(down(&c, 5.5, 0.0).is_none());
    let side = c.hit(v3(-20.0, 0.0, 5.0), v3(1.0, 0.0, 0.0), 1e-6, f64::INFINITY).unwrap();
    assert!((side.t - 15.0).abs() < 1e-9 && (side.n.x + 1.0).abs() < 1e-9);
    let cone = prim(json!({"shape": "cylinder", "radius": 4, "radius_top": 0, "height": 8}));
    let s = cone.hit(v3(-20.0, 0.0, 0.0), v3(1.0, 0.0, 0.0), 1e-6, f64::INFINITY).unwrap();
    assert!((s.t - 18.0).abs() < 1e-9, "half way up a cone of radius 4 the radius is 2");
    let e = prim(json!({"shape": "sphere", "radius": 2, "scale": [1, 1, 3]}));
    assert!((down(&e, 0.0, 0.0).unwrap() - 6.0).abs() < 1e-9);
    let rod = prim(json!({"shape": "rod", "from": [0, 0, 0], "to": [10, 0, 0], "radius": 1}));
    assert!((down(&rod, 5.0, 0.0).unwrap() - 1.0).abs() < 1e-9);
}

#[test]
fn text_is_cut_out_of_its_quad() {
    let o: Object = serde_json::from_value(json!({"shape": "text", "text": "I", "size": 10})).unwrap();
    let (a, k) = parts(&o).unwrap().remove(0);
    let Kind::Text { lo, hi, .. } = &k else { panic!("text") };
    assert!(lo.0 < 0.0 && hi.0 > 0.0 && lo.1 < 0.0 && hi.1 > 0.0, "centred on its ink");
    let t = Prim::new(k.clone(), &a, 0, 0);
    assert!(down(&t, 0.0, 0.0).is_some(), "the stem of the I");
    assert!(down(&t, hi.0 * 0.95, 0.0).is_none(), "beside the stem is empty");
    let none: Object = serde_json::from_value(json!({"shape": "text", "text": "   "})).unwrap();
    assert!(parts(&none).unwrap().is_empty());
}

#[test]
fn projection_centres_the_look_point() {
    for projection in ["perspective", "orthographic"] {
        let c = Camera { eye: [30.0, -80.0, 50.0], look: [3.0, 4.0, 5.0], projection: projection.into(), size: [640, 480], ..Camera::default() };
        let cam = Cam::new(&c, c.size);
        let p = cam.project(v3(3.0, 4.0, 5.0));
        assert!((p[0] - 320.0).abs() < 1e-6 && (p[1] - 240.0).abs() < 1e-6, "{projection}: {p:?}");
        let (o, d) = cam.ray(100.0, 70.0);
        let back = cam.project(o + d * 40.0);
        assert!((back[0] - 100.0).abs() < 1e-6 && (back[1] - 70.0).abs() < 1e-6, "{projection}: ray and project agree");
    }
}

#[test]
fn straight_down_ortho_has_x_right_y_up() {
    let c = Camera { eye: [0.0, 0.0, 400.0], look: [0.0; 3], projection: "orthographic".into(), width: 100.0, size: [1000, 500], ..Camera::default() };
    let cam = Cam::new(&c, c.size);
    let p = cam.project(v3(10.0, 0.0, 0.0));
    let q = cam.project(v3(0.0, 10.0, 0.0));
    assert!((p[0] - 600.0).abs() < 1e-6 && (p[1] - 250.0).abs() < 1e-6, "+x is right: {p:?}");
    assert!((q[0] - 500.0).abs() < 1e-6 && (q[1] - 150.0).abs() < 1e-6, "+y is up: {q:?}");
}

#[test]
fn renders_a_lit_box_on_a_transparent_background() {
    let s: Scene = serde_json::from_value(json!({
        "materials": {"red": {"color": [0.8, 0.1, 0.1]}},
        "objects": [{"shape": "box", "size": [20, 20, 20], "material": "red"}],
        "camera": {"eye": [0, 0, 100], "look": [0, 0, 0], "projection": "orthographic", "width": 60, "size": [60, 40]},
        "lights": {"background": "transparent"}
    })).unwrap();
    let c = compile(&s).unwrap();
    let px = papermache::render::render(&s, &c, [60, 40], 4);
    let at = |x: usize, y: usize| &px[(y * 60 + x) * 4..(y * 60 + x) * 4 + 4];
    assert_eq!(at(30, 20)[3], 255);
    assert!(at(30, 20)[0] > at(30, 20)[1] * 2, "red where the box is");
    assert_eq!(at(2, 2)[3], 0, "nothing behind: transparent");
}

#[test]
fn mcp_roundtrip_renders_a_png() {
    let mut srv = Server::default();
    let call = |srv: &mut Server, id: u64, method: &str, params: Value| -> Value {
        serde_json::from_str(&srv.handle(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()).unwrap()).unwrap()
    };
    let init = call(&mut srv, 1, "initialize", json!({"protocolVersion": "2025-06-18"}));
    assert_eq!(init["result"]["serverInfo"]["name"], "paper-mache");
    assert!(srv.handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string()).is_none());
    let list = call(&mut srv, 2, "tools/list", json!({}));
    assert!(list["result"]["tools"].as_array().unwrap().len() >= 15);
    let add = call(&mut srv, 3, "tools/call", json!({"name": "object_add", "arguments": {"objects": [
        {"name": "floor", "shape": "plane", "size": [100, 100]}, {"shape": "sphere", "radius": 10, "position": [0, 0, 10], "tags": ["ball"]}]}}));
    assert_eq!(add["result"]["isError"], false, "{add}");
    let frame = call(&mut srv, 4, "tools/call", json!({"name": "camera_frame", "arguments": {"tag": "ball"}}));
    assert_eq!(frame["result"]["isError"], false, "{frame}");
    let r = call(&mut srv, 5, "tools/call", json!({"name": "render", "arguments": {"max_size": 64, "samples": 2}}));
    let img = &r["result"]["content"][0];
    assert_eq!(img["type"], "image");
    assert_eq!(img["mimeType"], "image/png");
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(img["data"].as_str().unwrap()).unwrap();
    assert_eq!(&bytes[1..4], b"PNG");
    let bad = call(&mut srv, 6, "tools/call", json!({"name": "object_remove", "arguments": {"name": "nope"}}));
    assert_eq!(bad["result"]["isError"], true);
    let unknown = call(&mut srv, 7, "nope", json!({}));
    assert_eq!(unknown["error"]["code"], -32601);
}

#[test]
fn export_writes_obj_and_stl() {
    let dir = std::env::temp_dir();
    let s: Scene = serde_json::from_value(json!({"objects": [
        {"shape": "box", "size": [2, 2, 2], "radius": 0.3}, {"shape": "cylinder", "radius": 1, "height": 2}, {"shape": "torus", "major": 3, "minor": 0.5},
        {"shape": "text", "text": "skip"}]})).unwrap();
    let stl = dir.join("pm-test.stl");
    papermache::mesh::export(&s, stl.to_str().unwrap(), None).unwrap();
    let b = std::fs::read(&stl).unwrap();
    let n = u32::from_le_bytes(b[80..84].try_into().unwrap()) as usize;
    assert_eq!(b.len(), 84 + 50 * n);
    let obj = dir.join("pm-test.obj");
    let msg = papermache::mesh::export(&s, obj.to_str().unwrap(), None).unwrap();
    assert!(msg.contains("from 3 objects"), "{msg}");
    let back = papermache::mesh::load(obj.to_str().unwrap()).unwrap();
    assert_eq!(back.1.len(), n);
}

#[test]
fn image_raises_its_pixels_in_their_own_colours() {
    let path = std::env::temp_dir().join("papermache-test-4x2.png");
    let mut e = png::Encoder::new(std::fs::File::create(&path).unwrap(), 4, 2);
    e.set_color(png::ColorType::Rgb);
    let (r, w, k) = ([255u8, 0, 0], [255u8, 255, 255], [0u8, 0, 0]);
    let rows = [r, r, w, w, k, w, w, k].concat();
    e.write_header().unwrap().write_image_data(&rows).unwrap();
    let p = prim(json!({"shape": "image", "file": path.to_str().unwrap(), "width": 40, "depth": 3, "detail": 1}));
    let at = |x: f64, y: f64| p.hit(v3(x, y, 100.0), v3(0.0, 0.0, -1.0), 1e-6, f64::INFINITY);
    let red = at(-15.0, 5.0).expect("top-left pixel is red and raised");
    assert!((100.0 - red.t - 1.5).abs() < 1e-9 && red.n.z > 0.99, "its top is at depth / 2");
    assert!((red.col.unwrap().x - 1.0).abs() < 1e-6 && red.col.unwrap().y.abs() < 1e-6, "in the pixel's own red");
    assert!(at(5.0, 5.0).is_none() && at(-5.0, -5.0).is_none(), "white pixels, the key, are empty");
    assert!(at(15.0, -5.0).unwrap().col.unwrap().len() < 1e-6, "the black pixel is black");
    let side = p.hit(v3(-50.0, 5.0, 0.0), v3(1.0, 0.0, 0.0), 1e-6, f64::INFINITY).unwrap();
    assert!((side.t - 30.0).abs() < 1e-9 && side.n.x < -0.99, "a ray from the side meets the image's left wall");
    let _ = std::fs::remove_file(path);
}
