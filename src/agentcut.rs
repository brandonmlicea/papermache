//! Sending a render to AgentCut: the file uploaded as an asset, then placed on a project's timeline over its MCP.
use serde_json::{json, Value};

/// One HTTP request, built apart from sending it.
#[derive(Debug, Clone)]
pub struct Req {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Where the file sits: its length in seconds (none for a picture) and its size in pixels.
#[derive(Debug, Clone, Copy, Default)]
pub struct Media {
    pub duration: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

fn escape(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn auth(key: &str) -> (String, String) {
    ("authorization".into(), format!("Bearer {key}"))
}

pub fn mime(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("").to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        _ => "video/mp4",
    }
}

pub fn upload(url: &str, key: &str, name: &str, m: Media, body: Vec<u8>) -> Req {
    let mut q = format!("name={}", escape(name));
    for (k, v) in [("duration", m.duration.map(|d| format!("{d:.3}"))), ("width", m.width.map(|w| w.to_string())), ("height", m.height.map(|h| h.to_string()))] {
        if let Some(v) = v {
            q += &format!("&{k}={v}");
        }
    }
    Req { method: "PUT", url: format!("{}/api/assets?{q}", url.trim_end_matches('/')), headers: vec![auth(key), ("content-type".into(), mime(name).into())], body }
}

/// `start` is seconds or AgentCut's own "0:15".
pub fn place(url: &str, key: &str, project: &str, asset: &str, start: &Value) -> Req {
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "edit_video",
        "arguments": {"project": project, "ops": [{"op": "add_clip", "asset": asset, "start": start}]}}});
    Req {
        method: "POST",
        url: format!("{}/mcp", url.trim_end_matches('/')),
        headers: vec![auth(key), ("content-type".into(), "application/json".into()), ("accept".into(), "application/json, text/event-stream".into())],
        body: call.to_string().into_bytes(),
    }
}

/// The JSON a reply carries, plain or as the last `data:` line of an event stream.
pub fn reply(text: &str) -> Result<Value, String> {
    let body = text.lines().rev().find_map(|l| l.strip_prefix("data:")).unwrap_or(text).trim();
    serde_json::from_str(body).map_err(|e| format!("AgentCut replied {}: {e}", text.chars().take(200).collect::<String>()))
}

fn send(r: &Req) -> Result<Value, String> {
    let mut q = ureq::request(r.method, &r.url);
    for (k, v) in &r.headers {
        q = q.set(k, v);
    }
    let text = match q.send_bytes(&r.body) {
        Ok(res) => res.into_string().map_err(|e| e.to_string())?,
        Err(ureq::Error::Status(code, res)) => return Err(format!("AgentCut {} {}: {code} {}", r.method, r.url.split('?').next().unwrap_or(""), res.into_string().unwrap_or_default())),
        Err(e) => return Err(format!("AgentCut {}: {e}", r.url)),
    };
    reply(&text)
}

/// Seconds and pixels of a png from its header, of a video from ffprobe when it is on PATH.
pub fn measure(path: &str, bytes: &[u8]) -> Media {
    if bytes.starts_with(b"\x89PNG") && bytes.len() >= 24 {
        let be = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        return Media { duration: None, width: Some(be(16)), height: Some(be(20)) };
    }
    let out = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height:format=duration", "-of", "json", path])
        .output();
    let Ok(v) = out.map(|o| serde_json::from_slice::<Value>(&o.stdout).unwrap_or(Value::Null)) else { return Media::default() };
    let s = &v["streams"][0];
    Media {
        duration: v["format"]["duration"].as_str().and_then(|d| d.parse().ok()),
        width: s["width"].as_u64().map(|w| w as u32),
        height: s["height"].as_u64().map(|h| h as u32),
    }
}

/// Uploads `file` and adds it to `project` at `start`; the URL and key come from the arguments or AGENTCUT_URL and AGENTCUT_KEY.
pub fn send_file(file: &str, url: Option<&str>, key: Option<&str>, project: &str, start: &Value) -> Result<Value, String> {
    let url = url.map(String::from).or_else(|| std::env::var("AGENTCUT_URL").ok()).unwrap_or("https://agentcut.brandonlicea61.workers.dev".into());
    let key = key.map(String::from).or_else(|| std::env::var("AGENTCUT_KEY").ok()).ok_or("an AgentCut key: --key or AGENTCUT_KEY")?;
    let bytes = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
    let m = measure(file, &bytes);
    let name = std::path::Path::new(file).file_name().and_then(|n| n.to_str()).unwrap_or("render.mp4").to_string();
    let asset = send(&upload(&url, &key, &name, m, bytes))?;
    let id = asset["id"].as_str().ok_or(format!("AgentCut gave no asset id: {asset}"))?.to_string();
    let placed = send(&place(&url, &key, project, &id, start))?;
    if let Some(e) = placed.get("error") {
        return Err(format!("AgentCut edit_video: {e}"));
    }
    Ok(json!({"asset": id, "project": project, "url": url, "result": placed["result"]}))
}
