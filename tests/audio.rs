use papermache::audio::{analyze, beats, click_track, Beats};

const RATE: u32 = 22050;

fn clicks(bpm: f64, seconds: f64) -> Vec<f64> {
    (0..).map(|k| 0.5 + k as f64 * 60.0 / bpm).take_while(|&t| t < seconds).collect()
}

fn near(t: f64, clicks: &[f64]) -> (usize, f64) {
    clicks.iter().enumerate().map(|(i, c)| (i, (t - c).abs())).min_by(|a, b| a.1.total_cmp(&b.1)).unwrap()
}

fn check(b: &Beats, bpm: f64, seconds: f64) {
    let step = 60.0 / bpm;
    assert!((b.bpm - bpm).abs() < 1.5, "tempo {} for {bpm}", b.bpm);
    let gaps: Vec<f64> = b.beats.windows(2).map(|w| w[1] - w[0]).collect();
    let mean_err = gaps.iter().map(|g| (g - step).abs()).sum::<f64>() / gaps.len() as f64;
    assert!(mean_err < 0.025, "gaps off by {mean_err} s on average");
    let c = clicks(bpm, seconds);
    let late: Vec<f64> = b.beats.iter().filter(|&&t| t > 2.0).map(|&t| near(t, &c).1).collect();
    assert!(late.len() as f64 >= (seconds - 3.0) / step, "{} beats after 2 s", late.len());
    let worst = late.iter().cloned().fold(0.0, f64::max);
    assert!(worst < 0.03, "a beat {worst} s from its click");
    println!("{bpm} BPM: found {:.3}, {} beats, mean gap error {:.4} s, worst beat {:.4} s, mean beat {:.4} s, {} onsets",
        b.bpm, b.beats.len(), mean_err, worst, late.iter().sum::<f64>() / late.len() as f64, b.onsets.len());
}

#[test]
fn finds_120_bpm() {
    check(&analyze(&click_track(120.0, 20.0, RATE), RATE), 120.0, 20.0);
}

#[test]
fn finds_93_bpm() {
    check(&analyze(&click_track(93.0, 20.0, RATE), RATE), 93.0, 20.0);
}

#[test]
fn downbeats_fall_on_the_accents() {
    for bpm in [120.0, 93.0] {
        let b = analyze(&click_track(bpm, 20.0, RATE), RATE);
        let c = clicks(bpm, 20.0);
        assert!(b.downbeats.len() >= 4);
        for &t in b.downbeats.iter().filter(|&&t| t > 2.0) {
            let (i, d) = near(t, &c);
            assert!(d < 0.03 && i % 4 == 0, "{bpm} BPM: downbeat {t} is click {i}");
        }
        assert!(b.onsets.len() + 2 >= c.len() && b.onsets.len() <= c.len() + 2, "{} onsets for {} clicks", b.onsets.len(), c.len());
    }
}

#[test]
fn reads_a_wav_back() {
    let path = std::env::temp_dir().join("papermache-clicks.wav");
    let spec = hound::WavSpec { channels: 2, sample_rate: 44100, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(&path, spec).unwrap();
    for s in click_track(120.0, 12.0, 44100) {
        let v = (s * 32767.0) as i16;
        w.write_sample(v).unwrap();
        w.write_sample(v).unwrap();
    }
    w.finalize().unwrap();
    let b = beats(path.to_str().unwrap()).unwrap();
    assert!((b.duration - 12.0).abs() < 0.01, "{}", b.duration);
    check(&b, 120.0, 12.0);
    let _ = std::fs::remove_file(path);
    assert!(beats("no-such-song.mp3").is_err());
}
