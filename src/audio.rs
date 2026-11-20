//! Music in, beats out: decoding, an onset envelope from spectral flux, tempo by autocorrelation, beats by dynamic programming.
use serde::Serialize;
use std::f64::consts::{PI, TAU};

#[derive(Clone, Debug, Serialize)]
pub struct Beats {
    pub bpm: f64,
    pub beats: Vec<f64>,
    pub downbeats: Vec<f64>,
    pub onsets: Vec<f64>,
    pub duration: f64,
}

fn read_wav(path: &str) -> Result<(Vec<f32>, u32), String> {
    let err = |e: hound::Error| format!("{path}: {e}");
    let mut r = hound::WavReader::open(path).map_err(err)?;
    let spec = r.spec();
    let ch = spec.channels.max(1) as usize;
    let all: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>().map_err(err)?,
        hound::SampleFormat::Int => {
            let k = 1.0 / (1u64 << (spec.bits_per_sample.max(1) - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 * k)).collect::<Result<_, _>>().map_err(err)?
        }
    };
    Ok((all.chunks(ch).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect(), spec.sample_rate))
}

/// Mono samples in -1..1 and their rate: .wav directly, anything else through ffmpeg.
pub fn decode(path: &str) -> Result<(Vec<f32>, u32), String> {
    if path.to_lowercase().ends_with(".wav") {
        return read_wav(path);
    }
    if !std::path::Path::new(path).exists() {
        return Err(format!("{path}: no such file"));
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!("papermache-{}-{nanos}.wav", std::process::id()));
    let out = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-i", path, "-ac", "1", "-ar", "22050", "-f", "wav"])
        .arg(&tmp)
        .output()
        .map_err(|e| format!("{path}: decoding needs ffmpeg on PATH ({e})"))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{path}: ffmpeg: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let r = read_wav(&tmp.to_string_lossy());
    let _ = std::fs::remove_file(&tmp);
    r
}

/// Decodes a song and finds its tempo, beats, downbeats and onsets.
pub fn beats(path: &str) -> Result<Beats, String> {
    let (s, rate) = decode(path)?;
    Ok(analyze(&s, rate))
}

fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let a = -TAU / len as f64;
        for s in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (wr, wi) = ((a * k as f64).cos(), (a * k as f64).sin());
                let (p, q) = (s + k, s + k + len / 2);
                let (xr, xi) = (re[q] * wr - im[q] * wi, re[q] * wi + im[q] * wr);
                re[q] = re[p] - xr;
                im[q] = im[p] - xi;
                re[p] += xr;
                im[p] += xi;
            }
        }
        len <<= 1;
    }
}

/// Onset strength a frame: log-compressed spectral flux, minus its local mean, in units of its spread.
fn envelope(x: &[f64], n: usize, hop: usize, fr: f64) -> Vec<f64> {
    let win: Vec<f64> = (0..n).map(|i| 0.5 - 0.5 * (TAU * i as f64 / n as f64).cos()).collect();
    let frames = if x.len() >= n { (x.len() - n) / hop + 1 } else { 0 };
    let mut prev = vec![0.0; n / 2 + 1];
    let mut flux = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut re: Vec<f64> = (0..n).map(|i| x[f * hop + i] * win[i]).collect();
        let mut im = vec![0.0; n];
        fft(&mut re, &mut im);
        let mut sum = 0.0;
        for k in 0..=n / 2 {
            let m = (1.0 + 100.0 * re[k].hypot(im[k])).ln();
            if f > 0 {
                sum += (m - prev[k]).max(0.0);
            }
            prev[k] = m;
        }
        flux.push(sum);
    }
    let w = (fr * 0.5) as usize;
    let mut acc = vec![0.0; flux.len() + 1];
    for (i, v) in flux.iter().enumerate() {
        acc[i + 1] = acc[i] + v;
    }
    let mut env: Vec<f64> = (0..flux.len()).map(|i| {
        let (a, b) = (i.saturating_sub(w), (i + w + 1).min(flux.len()));
        flux[i] - (acc[b] - acc[a]) / (b - a) as f64
    }).collect();
    let sd = (env.iter().map(|v| v * v).sum::<f64>() / env.len().max(1) as f64).sqrt();
    if sd > 1e-12 {
        env.iter_mut().for_each(|v| *v /= sd);
    }
    env
}

/// The beat period in frames: the autocorrelation peak for 60-200 BPM under a prior around 120 BPM.
fn period(env: &[f64], fr: f64) -> f64 {
    let (lo, hi) = (((60.0 * fr / 200.0).floor() as usize).max(2), ((60.0 * fr / 60.0).ceil() as usize).min(env.len().saturating_sub(2)));
    if hi <= lo + 2 {
        return 60.0 * fr / 120.0;
    }
    let score = |lag: usize| {
        let r: f64 = env.iter().zip(&env[lag..]).map(|(a, b)| a * b).sum::<f64>() / (env.len() - lag) as f64;
        let bpm = 60.0 * fr / lag as f64;
        r * (-0.5 * (bpm / 120.0).log2().powi(2)).exp()
    };
    let s: Vec<f64> = (lo - 1..=hi + 1).map(score).collect();
    let best = (1..s.len() - 1).max_by(|&a, &b| s[a].total_cmp(&s[b])).unwrap();
    let (a, b, c) = (s[best - 1], s[best], s[best + 1]);
    let den = a - 2.0 * b + c;
    let shift = if den.abs() > 1e-12 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
    (lo - 1 + best) as f64 + shift
}

/// Beat frames by Ellis's dynamic programming: each beat rewards onset strength and a gap near the period.
fn track(env: &[f64], p: f64) -> Vec<usize> {
    let n = env.len();
    if n == 0 {
        return vec![];
    }
    let tight = 100.0;
    let mut score = env.to_vec();
    let mut back = vec![usize::MAX; n];
    for i in 0..n {
        let (a, b) = ((i as f64 - 2.0 * p).round().max(0.0) as usize, (i as f64 - p / 2.0).round());
        if b < 0.0 {
            continue;
        }
        let mut best = f64::NEG_INFINITY;
        for j in a..=(b as usize).min(i.saturating_sub(1)) {
            let v = score[j] - tight * ((i - j) as f64 / p).ln().powi(2);
            if v > best {
                best = v;
                back[i] = j;
            }
        }
        if best.is_finite() {
            score[i] = env[i] + best;
        }
    }
    let tail = n.saturating_sub(p.round() as usize);
    let mut i = (tail..n).max_by(|&a, &b| score[a].total_cmp(&score[b])).unwrap_or(n - 1);
    let mut out = vec![i];
    while back[i] != usize::MAX {
        i = back[i];
        out.push(i);
    }
    out.reverse();
    out
}

/// Tempo, beats, downbeats and onsets of mono samples at `rate`.
pub fn analyze(samples: &[f32], rate: u32) -> Beats {
    let rate = rate.max(1) as f64;
    let duration = samples.len() as f64 / rate;
    let k = if rate > 32000.0 { (rate / 22050.0).round().max(1.0) as usize } else { 1 };
    let x: Vec<f64> = samples.chunks(k).map(|c| c.iter().map(|&v| v as f64).sum::<f64>() / c.len() as f64).collect();
    let sr = rate / k as f64;
    let n = if sr > 16000.0 { 1024 } else { 512 };
    let hop = n / 4;
    let fr = sr / hop as f64;
    let env = envelope(&x, n, hop, fr);
    let at = |f: usize| (f * hop + n / 2) as f64 / sr;
    let gap = ((0.05 * fr).ceil() as usize).max(1);
    let mut onsets: Vec<usize> = vec![];
    for i in 0..env.len() {
        let (a, b) = (i.saturating_sub(gap), (i + gap + 1).min(env.len()));
        if env[i] > 1.0 && env[a..b].iter().all(|&v| v <= env[i]) && onsets.last().is_none_or(|&l| i - l >= gap) {
            onsets.push(i);
        }
    }
    let p = period(&env, fr);
    let mut frames = track(&env, p);
    if let (Some(&a), Some(&b)) = (onsets.first(), onsets.last()) {
        frames.retain(|&f| f + gap >= a && f as f64 <= b as f64 + p);
    }
    let phase = (0..4.min(frames.len().max(1)))
        .max_by(|&a, &b| {
            let sum = |ph: usize| frames.iter().skip(ph).step_by(4).map(|&f| env[f]).sum::<f64>();
            sum(a).total_cmp(&sum(b))
        })
        .unwrap_or(0);
    Beats {
        bpm: if env.is_empty() { 0.0 } else { 60.0 * fr / p },
        beats: frames.iter().map(|&f| at(f)).collect(),
        downbeats: frames.iter().skip(phase).step_by(4).map(|&f| at(f)).collect(),
        onsets: onsets.into_iter().map(at).collect(),
        duration,
    }
}

/// A test click track: a 5 ms 1 kHz burst on every beat from 0.5 s, every 4th one louder, over faint noise.
pub fn click_track(bpm: f64, seconds: f64, rate: u32) -> Vec<f32> {
    let sr = rate as f64;
    let len = (seconds * sr) as usize;
    let mut seed: u32 = 0x1234_5678;
    let mut out: Vec<f32> = (0..len).map(|_| {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 0.004
    }).collect();
    let step = 60.0 / bpm;
    let burst = (0.005 * sr) as usize;
    let mut k = 0;
    loop {
        let t = 0.5 + k as f64 * step;
        let start = (t * sr).round() as usize;
        if start >= len {
            break;
        }
        let amp = if k % 4 == 0 { 0.9 } else { 0.4 };
        for i in 0..burst.min(len - start) {
            let s = i as f64 / sr;
            out[start + i] += (amp * (2.0 * PI * 1000.0 * s).sin() * (-s / 0.0015).exp()) as f32;
        }
        k += 1;
    }
    out
}
