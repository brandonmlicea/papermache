use crate::math::*;
use crate::scene::{bounds, Camera, Compiled, Scene};
use crate::shade::{luma, Env, EPS};
use rayon::prelude::*;
use serde_json::Value;
use std::f64::consts::TAU;

pub struct Cam {
    eye: V3,
    f: V3,
    r: V3,
    u: V3,
    ortho: bool,
    sx: f64,
    sy: f64,
    w: f64,
    h: f64,
    aperture: f64,
    focus: f64,
}

impl Cam {
    pub fn new(c: &Camera, size: [u32; 2]) -> Cam {
        let eye = V3::from(c.eye);
        let f = (V3::from(c.look) - eye).norm();
        let mut up = c.up.map(V3::from).unwrap_or(v3(0.0, 0.0, 1.0));
        for alt in [v3(0.0, 1.0, 0.0), v3(1.0, 0.0, 0.0)] {
            if f.cross(up).len() < 1e-9 {
                up = alt;
            }
        }
        let r = f.cross(up).norm();
        let u = r.cross(f);
        let (w, h) = (size[0].max(1) as f64, size[1].max(1) as f64);
        let ortho = c.projection.starts_with("ortho");
        let half = if ortho {
            c.width / 2.0
        } else if let Some(fov) = c.fov {
            (fov.to_radians() / 2.0).tan()
        } else {
            18.0 / c.lens
        };
        let (sx, sy) = if w >= h { (half, half * h / w) } else { (half * w / h, half) };
        let aperture = if ortho { 0.0 } else { c.fstop.filter(|s| *s > 0.0).map_or(0.0, |s| 18.0 / half / s / 2.0) };
        let focus = c.focus.unwrap_or((V3::from(c.look) - eye).len()).max(1e-3);
        Cam { eye, f, r, u, ortho, sx, sy, w, h, aperture, focus }
    }

    /// A ray through a thin lens: sharp at the focus distance, softer away from it as the f-stop opens.
    pub fn lens_ray(&self, px: f64, py: f64, rng: &mut Rng) -> (V3, V3) {
        let (o, d) = self.ray(px, py);
        if self.aperture <= 0.0 {
            return (o, d);
        }
        let at = o + d * (self.focus / d.dot(self.f));
        let (r, a) = (rng.f().sqrt() * self.aperture, rng.f() * TAU);
        let o = o + self.r * (r * a.cos()) + self.u * (r * a.sin());
        (o, (at - o).norm())
    }

    pub fn ray(&self, px: f64, py: f64) -> (V3, V3) {
        let x = (2.0 * px / self.w - 1.0) * self.sx;
        let y = (1.0 - 2.0 * py / self.h) * self.sy;
        if self.ortho {
            (self.eye + self.r * x + self.u * y, self.f)
        } else {
            (self.eye, (self.f + self.r * x + self.u * y).norm())
        }
    }

    pub fn project(&self, p: V3) -> [f64; 2] {
        let v = p - self.eye;
        let (mut x, mut y) = (v.dot(self.r), v.dot(self.u));
        if !self.ortho {
            let z = v.dot(self.f);
            x /= z;
            y /= z;
        }
        [(x / self.sx + 1.0) / 2.0 * self.w, (1.0 - y / self.sy) / 2.0 * self.h]
    }
}

/// Linear colour and alpha of every pixel, with what the denoiser leans on: each pixel's variance and the
/// albedo, normal and depth of what its centre sees.
pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[f32; 4]>,
    pub var: Vec<f32>,
    pub aux: Vec<[f32; 7]>,
    pub samples: u64,
}

/// The camera's focus distance: `focus`, or the distance along the view to `focus_on`'s centre, or to the look point.
pub fn focus(s: &Scene) -> Option<f64> {
    let c = &s.camera;
    let f = (V3::from(c.look) - V3::from(c.eye)).norm();
    match &c.focus_on {
        Some(n) => s.objects.iter().find(|o| &o.name == n).map(|o| (bounds(o).centroid() - V3::from(c.eye)).dot(f)),
        None => c.focus,
    }
}

/// The scene seen by its camera at `size`, in linear light: up to `samples` a pixel, fewer where it has settled.
pub fn render_hdr(s: &Scene, c: &Compiled, size: [u32; 2], samples: u32) -> Frame {
    let mut camera = s.camera.clone();
    camera.focus = focus(s);
    let cam = Cam::new(&camera, size);
    let l = &s.lights;
    let sky = V3::from(l.sky.color) * l.sky.strength;
    let bg = match &l.background {
        Some(Value::String(t)) if t == "transparent" => None,
        Some(Value::Array(a)) if a.len() >= 3 => Some(v3(a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0), a[2].as_f64().unwrap_or(0.0))),
        _ => Some(sky),
    };
    let env = Env { c, sun: V3::from(l.sun.direction).norm(), cos_max: (l.sun.angle.to_radians() / 2.0).cos(), sun_e: l.sun.strength, sky, bg };
    let (w, h) = (size[0] as usize, size[1] as usize);
    let n = samples.max(1);
    let tol = s.render.noise.max(0.0);
    let least = if tol > 0.0 { (n / 4).clamp(4, n) } else { n };
    let settled = |p: &Px| p.k > 1 && (p.m2 / (p.k * (p.k - 1)) as f64).sqrt() <= tol * 2.0 * (p.mean.max(0.0) + 1e-3).sqrt();
    let run = |p: &mut Px, x: usize, y: usize, upto: u32, floor: u32| {
        while p.k < upto {
            let jx = (p.ox + p.k as f64 * 0.754_877_666_246_692_7).fract();
            let jy = (p.oy + p.k as f64 * 0.569_840_290_998_053_3).fract();
            let (o, d) = cam.lens_ray(x as f64 + jx, y as f64 + jy, &mut p.rng);
            let (col, al) = env.trace(o, d, 0, &mut p.rng, true);
            p.acc += col * al;
            p.a += al;
            p.k += 1;
            let y1 = luma(col * al);
            let dl = y1 - p.mean;
            p.mean += dl / p.k as f64;
            p.m2 += dl * (y1 - p.mean);
            if p.k >= floor && p.k % 4 == 0 && settled(p) {
                break;
            }
        }
    };
    let mut px: Vec<Px> = (0..w * h).map(|i| {
        let mut rng = Rng::new((((i / w) as u64) << 32) ^ (i % w) as u64);
        Px { ox: rng.f(), oy: rng.f(), rng, acc: V3::default(), a: 0.0, mean: 0.0, m2: 0.0, k: 0 }
    }).collect();
    px.par_chunks_mut(w).enumerate().for_each(|(y, row)| row.iter_mut().enumerate().for_each(|(x, p)| run(p, x, y, least, u32::MAX)));
    if least < n {
        let rough: Vec<bool> = px.iter().map(|p| !settled(p)).collect();
        let near = |x: usize, y: usize| (y.saturating_sub(1)..(y + 2).min(h)).any(|j| (x.saturating_sub(1)..(x + 2).min(w)).any(|i| rough[j * w + i]));
        px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            row.iter_mut().enumerate().filter(|(x, _)| near(*x, y)).for_each(|(x, p)| run(p, x, y, n, (least * 2).min(n)))
        });
    }
    let aux: Vec<[f32; 7]> = if !s.render.denoise {
        vec![]
    } else {
        (0..w * h).into_par_iter().map(|i| {
            let (o, d) = cam.ray((i % w) as f64 + 0.5, (i / w) as f64 + 0.5);
            match c.bvh.closest(&c.prims, o, d, EPS, f64::INFINITY) {
                Some((hit, pi)) => {
                    let (nn, _, m) = env.surface(&hit, pi, o + d * hit.t);
                    let nn = if nn.dot(d) > 0.0 { -nn } else { nn };
                    let alb = m.color + m.emission;
                    [alb.x as f32, alb.y as f32, alb.z as f32, nn.x as f32, nn.y as f32, nn.z as f32, hit.t as f32]
                }
                None => [1.0, 1.0, 1.0, 0.0, 0.0, 0.0, f32::INFINITY],
            }
        }).collect()
    };
    Frame {
        w,
        h,
        px: px.iter().map(|p| {
            let col = if p.a > 0.0 { p.acc / p.a } else { V3::default() };
            [col.x as f32, col.y as f32, col.z as f32, (p.a / p.k.max(1) as f64) as f32]
        }).collect(),
        var: px.iter().map(|p| if p.k > 1 { (p.m2 / (p.k * (p.k - 1)) as f64) as f32 } else { 0.0 }).collect(),
        aux,
        samples: px.iter().map(|p| p.k as u64).sum(),
    }
}

/// One pixel's running sums while it samples.
struct Px {
    rng: Rng,
    ox: f64,
    oy: f64,
    acc: V3,
    a: f64,
    mean: f64,
    m2: f64,
    k: u32,
}

/// RGBA8 pixels of the scene seen by its camera at `size`, denoised, bloomed and toned as the scene asks.
pub fn render(s: &Scene, c: &Compiled, size: [u32; 2], samples: u32) -> Vec<u8> {
    let mut f = render_hdr(s, c, size, samples);
    finish(s, &mut f)
}

/// The scene's post steps on a frame, then 8-bit sRGB.
pub fn finish(s: &Scene, f: &mut Frame) -> Vec<u8> {
    if s.render.denoise && f.aux.len() == f.px.len() {
        crate::post::denoise(f);
    }
    if s.render.bloom > 0.0 {
        crate::post::bloom(f, s.render.bloom);
    }
    let tone = |c: f32| -> f64 {
        let c = c.max(0.0) as f64;
        if s.render.filmic { (c * (2.51 * c + 0.03) / (c * (2.43 * c + 0.59) + 0.14)).clamp(0.0, 1.0) } else { c }
    };
    f.px.iter().flat_map(|p| [srgb(tone(p[0])), srgb(tone(p[1])), srgb(tone(p[2])), (p[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8]).collect()
}

fn srgb(c: f64) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

pub fn png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut e = png::Encoder::new(&mut buf, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().expect("png header");
        wr.write_image_data(rgba).expect("png data");
    }
    buf
}

/// Area-average downscale of RGBA8 pixels.
pub fn shrink(rgba: &[u8], w: u32, h: u32, nw: u32, nh: u32) -> Vec<u8> {
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let (x0, x1) = (x * w / nw, ((x + 1) * w / nw).max(x * w / nw + 1));
            let (y0, y1) = (y * h / nh, ((y + 1) * h / nh).max(y * h / nh + 1));
            let mut s = [0u32; 4];
            for yy in y0..y1.min(h) {
                for xx in x0..x1.min(w) {
                    for k in 0..4 {
                        s[k] += rgba[((yy * w + xx) * 4) as usize + k] as u32;
                    }
                }
            }
            let cnt = ((x1.min(w) - x0) * (y1.min(h) - y0)).max(1);
            for k in 0..4 {
                out[((y * nw + x) * 4) as usize + k] = (s[k] / cnt) as u8;
            }
        }
    }
    out
}

/// The size to render: the camera's, times scale, fitted inside max_size if given.
pub fn fit(size: [u32; 2], max: Option<u32>) -> [u32; 2] {
    match max {
        Some(m) if size[0].max(size[1]) > m => {
            let k = m as f64 / size[0].max(size[1]) as f64;
            [((size[0] as f64 * k).round() as u32).max(1), ((size[1] as f64 * k).round() as u32).max(1)]
        }
        _ => size,
    }
}
