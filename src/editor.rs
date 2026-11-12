//! The editor: a local page that edits one live scene by hand, drawn by Paper Mache's own renderer, while the same
//! scene answers MCP at /mcp, so a person and a coding agent can work on it together.
use crate::mcp::Server;
use crate::render::{png, render, Cam};
use crate::scene::{bounds, compile, resolve, set_base, Scene};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Method, Request, Response, Server as Http};

const PAGE: &str = include_str!("editor.html");

struct State {
    srv: Server,
    path: Option<String>,
    version: u64,
}

type Shared = Arc<Mutex<State>>;

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

fn reply(req: Request, status: u16, kind: &str, body: Vec<u8>) {
    let _ = req.respond(Response::from_data(body).with_status_code(status).with_header(header("Content-Type", kind)).with_header(header("Cache-Control", "no-store")));
}

fn ok(req: Request, v: Value) {
    reply(req, 200, "application/json", v.to_string().into_bytes());
}

fn fail(req: Request, e: impl ToString) {
    reply(req, 400, "application/json", json!({"error": e.to_string()}).to_string().into_bytes());
}

fn field<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

/// Pages from other sites may not drive the editor: a browser request must come from this page itself.
fn trusted(req: &Request, port: u16) -> bool {
    let get = |k: &str| req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(k)).map(|h| h.value.as_str().to_string());
    let host_ok = get("Host").is_none_or(|h| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}"));
    let origin_ok = get("Origin").is_none_or(|o| o == format!("http://127.0.0.1:{port}") || o == format!("http://localhost:{port}"));
    host_ok && origin_ok
}

fn query(url: &str, k: &str) -> Option<f64> {
    url.split_once('?')?.1.split('&').find_map(|p| p.strip_prefix(&format!("{k}=")).and_then(|v| v.parse().ok()))
}

fn handle(st: &Shared, mut req: Request, port: u16) {
    if !trusted(&req, port) {
        return reply(req, 403, "text/plain", b"forbidden".to_vec());
    }
    let url = req.url().to_string();
    let route = url.split('?').next().unwrap_or("").to_string();
    let mut body = vec![];
    let _ = req.as_reader().read_to_end(&mut body);
    let json_body = || serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    match (req.method().clone(), route.as_str()) {
        (Method::Get, "/") => reply(req, 200, "text/html; charset=utf-8", PAGE.as_bytes().to_vec()),
        (Method::Get, "/api/version") => {
            let v = st.lock().unwrap().version;
            ok(req, json!({"version": v}))
        }
        (Method::Get, "/api/scene") => {
            let s = st.lock().unwrap();
            ok(req, json!({"version": s.version, "path": s.path, "scene": s.srv.scene}))
        }
        (Method::Post, "/api/scene") => match serde_json::from_slice::<Scene>(&body) {
            Ok(mut sc) => {
                sc.name_all();
                if let Err(e) = compile(&sc) {
                    return fail(req, e);
                }
                let mut s = st.lock().unwrap();
                s.srv.scene = sc;
                s.version += 1;
                let v = s.version;
                drop(s);
                ok(req, json!({"version": v}))
            }
            Err(e) => fail(req, e),
        },
        (Method::Get, "/api/bounds") => {
            let sc = st.lock().unwrap().srv.scene.clone();
            ok(req, json!(sc.objects.iter().map(|o| bounds(o).to_json()).collect::<Vec<_>>()))
        }
        (Method::Get, "/api/render") => {
            let sc = st.lock().unwrap().srv.scene.clone();
            let size = [query(&url, "w").unwrap_or(800.0).clamp(16.0, 8000.0) as u32, query(&url, "h").unwrap_or(500.0).clamp(16.0, 8000.0) as u32];
            let samples = query(&url, "s").unwrap_or(4.0).clamp(1.0, 4096.0) as u32;
            let t = std::time::Instant::now();
            match compile(&sc) {
                Ok(c) => {
                    let img = png(size[0], size[1], &render(&sc, &c, size, samples));
                    let ms = t.elapsed().as_millis().to_string();
                    let _ = req.respond(Response::from_data(img).with_header(header("Content-Type", "image/png")).with_header(header("X-Ms", &ms)).with_header(header("Cache-Control", "no-store")));
                }
                Err(e) => fail(req, e),
            }
        }
        (Method::Post, "/api/pick") => {
            let a = json_body();
            let sc = st.lock().unwrap().srv.scene.clone();
            let n = |k: &str| a.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            let size = [n("w").max(1.0) as u32, n("h").max(1.0) as u32];
            let hit = compile(&sc).ok().and_then(|c| {
                let (o, d) = Cam::new(&sc.camera, size).ray(n("x"), n("y"));
                c.bvh.closest(&c.prims, o, d, 1e-6, f64::INFINITY).map(|(_, p)| c.prims[p as usize].obj)
            });
            ok(req, json!({"index": hit}))
        }
        (Method::Post, "/api/open") => {
            let a = json_body();
            let Some(p) = field(&a, "path").map(String::from) else { return fail(req, "path") };
            match Scene::load(&p) {
                Ok(sc) => {
                    let mut s = st.lock().unwrap();
                    s.srv.scene = sc;
                    s.path = Some(p);
                    s.version += 1;
                    let v = s.version;
                    drop(s);
                    ok(req, json!({"version": v}))
                }
                Err(e) => fail(req, e),
            }
        }
        (Method::Post, "/api/save") => {
            let a = json_body();
            let mut s = st.lock().unwrap();
            let Some(p) = field(&a, "path").map(String::from).or(s.path.clone()) else { return fail(req, "give a path to save to") };
            match std::fs::write(&p, serde_json::to_string_pretty(&s.srv.scene).unwrap()) {
                Ok(()) => {
                    set_base(&p);
                    s.path = Some(p.clone());
                    drop(s);
                    ok(req, json!({"saved": p}))
                }
                Err(e) => fail(req, format!("{p}: {e}")),
            }
        }
        (Method::Post, "/api/export") => {
            let a = json_body();
            let sc = st.lock().unwrap().srv.scene.clone();
            match field(&a, "path") {
                Some(p) => match crate::mesh::export(&sc, p, field(&a, "tag")) {
                    Ok(t) => ok(req, json!({"done": t})),
                    Err(e) => fail(req, e),
                },
                None => fail(req, "path"),
            }
        }
        (Method::Post, "/api/upload") => {
            let name = url.split_once("name=").map(|(_, n)| n.split('&').next().unwrap_or("")).unwrap_or("");
            let name: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c)).collect();
            if !name.to_ascii_lowercase().ends_with(".png") || body.is_empty() {
                return fail(req, "upload a .png");
            }
            let dir = st.lock().unwrap().path.as_ref().and_then(|p| std::path::Path::new(p).parent().map(|d| d.to_path_buf())).unwrap_or_else(|| ".".into());
            let at = dir.join(&name);
            match std::fs::write(&at, &body) {
                Ok(()) => ok(req, json!({"file": resolve(&at.to_string_lossy())})),
                Err(e) => fail(req, e),
            }
        }
        (Method::Post, "/mcp") => {
            let line = String::from_utf8_lossy(&body).to_string();
            let mut s = st.lock().unwrap();
            let out = s.srv.handle(&line);
            if line.contains("tools/call") {
                s.version += 1;
            }
            drop(s);
            match out {
                Some(o) => reply(req, 200, "application/json", o.into_bytes()),
                None => reply(req, 202, "application/json", vec![]),
            }
        }
        _ => reply(req, 404, "text/plain", b"not found".to_vec()),
    }
}

/// Serves the editor on 127.0.0.1 (7820, or the next free port) and opens it in the browser.
pub fn run(path: Option<String>, port: u16, open: bool) -> Result<(), String> {
    let mut srv = Server::default();
    if let Some(p) = &path {
        srv.scene = Scene::load(p)?;
    }
    let st: Shared = Arc::new(Mutex::new(State { srv, path, version: 1 }));
    let (http, port) = (port..port + 20).find_map(|p| Http::http(("127.0.0.1", p)).ok().map(|h| (h, p))).ok_or("no free port from 7820")?;
    let url = format!("http://127.0.0.1:{port}/");
    println!("Paper Mache editor: {url}  (MCP for agents: {url}mcp)  Ctrl+C stops it");
    if open {
        let _ = if cfg!(windows) {
            std::process::Command::new("cmd").args(["/C", "start", "", &url]).spawn()
        } else if cfg!(target_os = "macos") {
            std::process::Command::new("open").arg(&url).spawn()
        } else {
            std::process::Command::new("xdg-open").arg(&url).spawn()
        };
    }
    for req in http.incoming_requests() {
        let st = st.clone();
        std::thread::spawn(move || handle(&st, req, port));
    }
    Ok(())
}
