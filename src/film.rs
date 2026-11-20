//! Films: the timeline rendered frame by frame, with motion blur from sub-frames, encoded to mp4 by ffmpeg.
use crate::anim;
use crate::render::{finish, png, render_hdr, Frame};
use crate::scene::{compile, Scene};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Instant;

pub struct Opts {
    pub dir: String,
    pub fps: f64,
    pub from: f64,
    pub to: Option<f64>,
    pub samples: Option<u32>,
    pub scale: f64,
    pub mp4: bool,
    pub resume: bool,
}

/// One frame at time `t`: the scene there, sized by `scale`, averaged over `render.blur` moments the shutter is open.
pub fn frame(s: &Scene, t: f64, scale: f64, samples: Option<u32>) -> Result<(Vec<u8>, [u32; 2]), String> {
    let steps = s.render.blur.max(1);
    let open = s.render.shutter.clamp(0.0, 1.0) / s.render.fps.max(1e-3);
    let mut acc: Option<(Frame, Scene)> = None;
    let mut size = [1, 1];
    for i in 0..steps {
        let at = if steps > 1 { t + open * (i as f64 + 0.5) / steps as f64 } else { t };
        let sc = anim::at(s, at)?;
        size = sc.camera.size.map(|v| ((v as f64 * scale).round() as u32).max(2));
        let n = samples.unwrap_or(sc.render.samples).div_ceil(steps).max(1);
        let f = render_hdr(&sc, &compile(&sc)?, size, n);
        match &mut acc {
            None => acc = Some((f, sc)),
            Some((a, _)) => {
                for (p, q) in a.px.iter_mut().zip(&f.px) {
                    (0..4).for_each(|k| p[k] += q[k]);
                }
                a.var.iter_mut().zip(&f.var).for_each(|(v, w)| *v += w);
                a.samples += f.samples;
            }
        }
    }
    let (mut f, sc) = acc.ok_or("no frame")?;
    let k = steps as f32;
    f.px.iter_mut().for_each(|p| p.iter_mut().for_each(|c| *c /= k));
    f.var.iter_mut().for_each(|v| *v /= k * k);
    Ok((finish(&sc, &mut f), size))
}

pub fn ffmpeg() -> bool {
    std::process::Command::new("ffmpeg").arg("-version").output().is_ok_and(|o| o.status.success())
}

/// `dir/frame_00001.png...` into an H.264 mp4, with `song` from `offset` seconds under it when given.
pub fn encode(dir: &str, fps: f64, out: &str, song: Option<(&str, f64)>) -> Result<(), String> {
    let mut cmd = std::process::Command::new("ffmpeg");
    cmd.args(["-v", "error", "-y", "-framerate", &fps.to_string(), "-i"]).arg(Path::new(dir).join("frame_%05d.png"));
    if let Some((s, off)) = song {
        cmd.args(["-ss", &format!("{off:.4}"), "-i", s]);
    }
    cmd.args(["-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "18", "-movflags", "+faststart"]);
    if song.is_some() {
        cmd.args(["-map", "0:v", "-map", "1:a", "-c:a", "aac", "-b:a", "192k", "-shortest"]);
    }
    let o = cmd.arg(out).output().map_err(|e| format!("ffmpeg: {e}"))?;
    if !o.status.success() {
        return Err(format!("ffmpeg: {}", String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(())
}

pub fn write_frame(dir: &str, k: usize, px: &[u8], size: [u32; 2]) -> Result<String, String> {
    let p = Path::new(dir).join(format!("frame_{:05}.png", k + 1));
    std::fs::write(&p, png(size[0], size[1], px)).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(p.to_string_lossy().into_owned())
}

/// Renders the timeline from `from` to `to` (its last key) into `dir`, then `dir/film.mp4` when asked and ffmpeg is here.
pub fn film(s: &Scene, o: &Opts, log: &mut dyn FnMut(usize, usize, u128)) -> Result<Value, String> {
    std::fs::create_dir_all(&o.dir).map_err(|e| format!("{}: {e}", o.dir))?;
    let fps = if o.fps > 0.0 { o.fps } else { s.render.fps };
    let to = o.to.unwrap_or_else(|| anim::length(s)).max(o.from + 1.0 / fps);
    let n = ((to - o.from) * fps - 1e-6).ceil().max(1.0) as usize;
    let mut s = s.clone();
    s.render.fps = fps;
    let start = Instant::now();
    let (mut size, mut done) = ([0, 0], 0);
    for k in 0..n {
        if o.resume && Path::new(&o.dir).join(format!("frame_{:05}.png", k + 1)).exists() {
            continue;
        }
        let t = Instant::now();
        let (px, sz) = frame(&s, o.from + k as f64 / fps, o.scale, o.samples)?;
        write_frame(&o.dir, k, &px, sz)?;
        size = sz;
        done += 1;
        log(k + 1, n, t.elapsed().as_millis());
    }
    let secs = start.elapsed().as_secs_f64();
    let mp4 = if o.mp4 && ffmpeg() {
        let out = Path::new(&o.dir).join("film.mp4").to_string_lossy().into_owned();
        encode(&o.dir, fps, &out, None)?;
        Some(out)
    } else {
        None
    };
    Ok(json!({"dir": o.dir, "frames": n, "rendered": done, "fps": fps, "from": o.from, "to": to, "size": size, "seconds": secs,
        "seconds_per_frame": if done > 0 { secs / done as f64 } else { 0.0 }, "mp4": mp4, "ffmpeg": ffmpeg()}))
}
