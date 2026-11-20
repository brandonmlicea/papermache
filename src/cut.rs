//! The edit: named shots laid end to end on the song's beats, rendered as one film and muxed with the song.
use crate::audio;
use crate::film::{encode, ffmpeg, frame, write_frame};
use crate::scene::Scene;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// A shot: `scene` (a file, or the scene at hand) played from `from` for `beats` beats; `to` retimes it to end
/// there, `speed` plays it faster or slower.
#[derive(Clone, Debug, Deserialize)]
pub struct Shot {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub scene: Option<String>,
    #[serde(default)]
    pub from: f64,
    #[serde(default)]
    pub to: Option<f64>,
    #[serde(default = "four")]
    pub beats: f64,
    #[serde(default)]
    pub speed: Option<f64>,
}

fn four() -> f64 {
    4.0
}

#[derive(Clone, Debug, Deserialize)]
pub struct Cut {
    #[serde(default)]
    pub scene: Option<String>,
    #[serde(default)]
    pub song: Option<String>,
    #[serde(default)]
    pub bpm: Option<f64>,
    #[serde(default)]
    pub start_beat: usize,
    #[serde(default)]
    pub fps: Option<f64>,
    #[serde(default)]
    pub resume: bool,
    pub shots: Vec<Shot>,
}

/// Where each shot starts and ends in the film, from the beats (song seconds) and the first beat used.
pub fn lay(shots: &[Shot], beats: &[f64], period: f64, start: usize) -> Vec<(f64, f64)> {
    let at = |b: f64| -> f64 {
        let i = b.floor() as usize;
        let f = b - b.floor();
        let t = |k: usize| beats.get(k).copied().unwrap_or_else(|| beats.last().map_or(0.0, |l| l + (k + 1 - beats.len()) as f64 * period));
        t(i) + (t(i + 1) - t(i)) * f
    };
    let zero = at(start as f64);
    let mut b = start as f64;
    shots.iter().map(|s| {
        let a = at(b) - zero;
        b += s.beats.max(0.0);
        (a, at(b) - zero)
    }).collect()
}

/// Renders the cut into `dir` and muxes `out`; returns the edit it made.
pub fn cut(c: &Cut, here: &Scene, dir: &str, out: &str, samples: Option<u32>, scale: f64, log: &mut dyn FnMut(usize, usize, u128)) -> Result<Value, String> {
    let song = c.song.as_ref().map(|s| crate::scene::resolve(s));
    let (beats, bpm) = match (&song, c.bpm) {
        (_, Some(bpm)) => ((0..10_000).map(|k| k as f64 * 60.0 / bpm).collect::<Vec<_>>(), bpm),
        (Some(song), None) => {
            let b = audio::beats(song)?;
            (b.beats, b.bpm)
        }
        (None, None) => return Err("a cut needs a song or a bpm".into()),
    };
    if beats.is_empty() {
        return Err("no beats found in the song".into());
    }
    let period = 60.0 / bpm.max(1.0);
    let spans = lay(&c.shots, &beats, period, c.start_beat);
    let offset = beats.get(c.start_beat).copied().unwrap_or(0.0);
    let fps = c.fps.unwrap_or(here.render.fps);
    let total = spans.last().map_or(0.0, |s| s.1);
    let n = (total * fps).round().max(1.0) as usize;
    let mut scenes: HashMap<String, Scene> = HashMap::new();
    for s in &c.shots {
        if let Some(p) = &s.scene {
            if !scenes.contains_key(p) {
                scenes.insert(p.clone(), Scene::load(&crate::scene::resolve(p))?);
            }
        }
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}"))?;
    let start = std::time::Instant::now();
    for k in 0..n {
        if c.resume && std::path::Path::new(dir).join(format!("frame_{:05}.png", k + 1)).exists() {
            continue;
        }
        let t = k as f64 / fps;
        let i = spans.iter().position(|s| t < s.1).unwrap_or(spans.len() - 1);
        let (shot, (a, b)) = (&c.shots[i], spans[i]);
        let speed = match (shot.to, shot.speed) {
            (Some(to), _) => (to - shot.from) / (b - a).max(1e-9),
            (None, Some(sp)) => sp,
            _ => 1.0,
        };
        let mut sc = shot.scene.as_ref().map_or_else(|| here.clone(), |p| scenes[p].clone());
        sc.render.fps = fps;
        let tick = std::time::Instant::now();
        let (px, size) = frame(&sc, shot.from + (t - a) * speed, scale, samples)?;
        write_frame(dir, k, &px, size)?;
        log(k + 1, n, tick.elapsed().as_millis());
    }
    if ffmpeg() {
        encode(dir, fps, out, song.as_deref().map(|s| (s, offset)))?;
    }
    let edit: Vec<Value> = c.shots.iter().zip(&spans).map(|(s, (a, b))| json!({"name": s.name, "start": a, "end": b, "beats": s.beats})).collect();
    Ok(json!({"mp4": if ffmpeg() { Some(out) } else { None }, "frames": n, "fps": fps, "bpm": bpm, "song_from": offset, "seconds": total,
        "render_seconds": start.elapsed().as_secs_f64(), "shots": edit}))
}
