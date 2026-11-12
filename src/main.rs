use papermache::math::V3;
use papermache::render::{png, render, Cam};
use papermache::scene::{compile, Scene};
use serde_json::{json, Map, Value};

const USAGE: &str = "Paper Mache: 3d model design, via mcp (rust)

  papermache                      the editor in your browser (also: papermache edit [scene.json] [--port N] [--no-open])
  papermache render <scene.json> -o <out.png> [--samples N] [--scale F]
  papermache export <scene.json> -o <model.obj|model.stl> [--tag T]
  papermache info <scene.json>
  papermache mcp                  MCP server on stdin/stdout
  papermache --version";

fn flag(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

fn run(args: &[String]) -> Result<(), String> {
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let scene = || Scene::load(args.get(1).ok_or(USAGE)?);
    match cmd {
        "" | "edit" => {
            let file = args.get(1).filter(|a| !a.starts_with("--")).cloned();
            let port = flag(args, "--port").and_then(|p| p.parse().ok()).unwrap_or(7820);
            papermache::editor::run(file, port, !args.iter().any(|a| a == "--no-open"))?
        }
        "mcp" => papermache::mcp::run_stdio(),
        "--version" | "-V" => println!("papermache {}", env!("CARGO_PKG_VERSION")),
        "render" => {
            let mut s = scene()?;
            let out = flag(args, "-o").ok_or("render needs -o <out.png>")?;
            let samples = flag(args, "--samples").map(|v| v.parse().map_err(|_| "--samples N")).transpose()?.unwrap_or(s.render.samples);
            let k: f64 = flag(args, "--scale").map(|v| v.parse().map_err(|_| "--scale F")).transpose()?.unwrap_or(1.0);
            s.camera.size = s.camera.size.map(|v| ((v as f64 * k).round() as u32).max(1));
            let t = std::time::Instant::now();
            let c = compile(&s)?;
            let px = render(&s, &c, s.camera.size, samples);
            std::fs::write(&out, png(s.camera.size[0], s.camera.size[1], &px)).map_err(|e| format!("{out}: {e}"))?;
            let cam = Cam::new(&s.camera, s.camera.size);
            let points: Map<String, Value> = s.points.iter().map(|(n, p)| (n.clone(), json!(cam.project(V3::from(*p))))).collect();
            println!("{}", json!({"file": out, "size": s.camera.size, "ms": t.elapsed().as_millis() as u64, "objects": s.objects.len(), "primitives": c.prims.len(), "points": points}));
        }
        "export" => {
            let s = scene()?;
            let out = flag(args, "-o").ok_or("export needs -o <model.obj|.stl>")?;
            println!("{}", papermache::mesh::export(&s, &out, flag(args, "--tag").as_deref())?);
        }
        "info" => {
            let mut v = papermache::mcp::info(&scene()?, None);
            v.as_object_mut().unwrap().remove("list");
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
