use papermache::math::V3;
use papermache::render::{png, Cam};
use papermache::scene::Scene;
use papermache::{anim, audio, cut, film};
use serde_json::{json, Map, Value};

const USAGE: &str = "Paper Mache: 3d model design, via mcp (rust)

  papermache                      the editor in your browser (also: papermache edit [scene.json] [--port N] [--no-open])
  papermache render <scene.json> -o <out.png> [--samples N] [--scale F] [--time S]
  papermache film <scene.json> -o <dir> [--fps N] [--from S] [--to S] [--samples N] [--scale F] [--no-mp4] [--resume]
  papermache beats <song.wav|.mp3|.flac>
  papermache cut <cut.json> -o <out.mp4> [--samples N] [--scale F] [--frames dir] [--resume]
  papermache agentcut <film.mp4|still.png> --project NAME [--start S] [--url U] [--key K]
  papermache export <scene.json> -o <model.obj|model.stl> [--tag T]
  papermache info <scene.json|model.glb>
  papermache mcp                  MCP server on stdin/stdout
  papermache --version";

fn flag(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

fn num<T: std::str::FromStr>(args: &[String], k: &str) -> Result<Option<T>, String> {
    flag(args, k).map(|v| v.parse().map_err(|_| format!("{k} needs a number"))).transpose()
}

fn progress(k: usize, n: usize, ms: u128) {
    if k % 10 == 0 || k == n {
        eprintln!("frame {k}/{n} {ms} ms");
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let file = || args.get(1).cloned().ok_or(USAGE.to_string());
    let scene = || Scene::load(&file()?);
    let out = || flag(args, "-o").ok_or(format!("{cmd} needs -o"));
    let scale = num::<f64>(args, "--scale")?.unwrap_or(1.0);
    let samples = num::<u32>(args, "--samples")?;
    match cmd {
        "" | "edit" => {
            let file = args.get(1).filter(|a| !a.starts_with("--")).cloned();
            let port = flag(args, "--port").and_then(|p| p.parse().ok()).unwrap_or(7820);
            papermache::editor::run(file, port, !args.iter().any(|a| a == "--no-open"))?
        }
        "mcp" => papermache::mcp::run_stdio(),
        "--version" | "-V" => println!("papermache {}", env!("CARGO_PKG_VERSION")),
        "render" => {
            let s = scene()?;
            let out = out()?;
            let time = num::<f64>(args, "--time")?.unwrap_or(0.0);
            let t = std::time::Instant::now();
            let (px, size) = film::frame(&s, time, scale, samples)?;
            std::fs::write(&out, png(size[0], size[1], &px)).map_err(|e| format!("{out}: {e}"))?;
            let at = anim::at(&s, time)?;
            let cam = Cam::new(&at.camera, size);
            let points: Map<String, Value> = at.points.iter().map(|(n, p)| (n.clone(), json!(cam.project(V3::from(*p))))).collect();
            println!("{}", json!({"file": out, "size": size, "ms": t.elapsed().as_millis() as u64, "objects": s.objects.len(), "time": time, "points": points}));
        }
        "film" => {
            let s = scene()?;
            let o = film::Opts {
                dir: out()?,
                fps: num(args, "--fps")?.unwrap_or(s.render.fps),
                from: num(args, "--from")?.unwrap_or(0.0),
                to: num(args, "--to")?,
                samples,
                scale,
                mp4: !args.iter().any(|a| a == "--no-mp4"),
                resume: args.iter().any(|a| a == "--resume"),
            };
            println!("{}", film::film(&s, &o, &mut progress)?);
        }
        "beats" => println!("{}", serde_json::to_string(&audio::beats(&file()?)?).unwrap()),
        "cut" => {
            let path = file()?;
            papermache::scene::set_base(&path);
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
            let mut c: cut::Cut = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
            c.resume |= args.iter().any(|a| a == "--resume");
            let here = match &c.scene {
                Some(p) => Scene::load(&papermache::scene::resolve(p))?,
                None => Scene::default(),
            };
            let out = out()?;
            let frames = flag(args, "--frames").unwrap_or_else(|| format!("{out}.frames"));
            println!("{}", cut::cut(&c, &here, &frames, &out, samples, scale, &mut progress)?);
        }
        "agentcut" => {
            let start = flag(args, "--start").map(|v| v.parse::<f64>().map(|n| json!(n)).unwrap_or(json!(v))).unwrap_or(json!(0));
            let project = flag(args, "--project").unwrap_or("Paper Mache".into());
            println!("{}", papermache::agentcut::send_file(&file()?, flag(args, "--url").as_deref(), flag(args, "--key").as_deref(), &project, &start)?);
        }
        "export" => {
            let s = scene()?;
            println!("{}", papermache::mesh::export(&s, &out()?, flag(args, "--tag").as_deref())?);
        }
        "info" => {
            let f = file()?;
            if f.to_lowercase().ends_with(".glb") || f.to_lowercase().ends_with(".gltf") {
                println!("{}", serde_json::to_string_pretty(&papermache::gltf::outline(&f)?).unwrap());
                return Ok(());
            }
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
