use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

static FONT: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");
const PX: f32 = 128.0;
const PAD: f32 = 2.0;

/// A string's coverage mask in em units, y up: `m0..m1` is the area the mask covers, `i0..i1` the ink.
pub struct Mask {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
    pub m0: (f64, f64),
    pub m1: (f64, f64),
    pub i0: (f64, f64),
    pub i1: (f64, f64),
}

impl Mask {
    /// Bilinear coverage at pixel coordinates (x right, y down), 0..1.
    pub fn sample(&self, x: f64, y: f64) -> f64 {
        let (x, y) = (x - 0.5, y - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |i: f64, j: f64| -> f64 {
            if i < 0.0 || j < 0.0 || i >= self.w as f64 || j >= self.h as f64 {
                0.0
            } else {
                self.data[j as usize * self.w + i as usize] as f64 / 255.0
            }
        };
        let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1.0, y0) * fx;
        let bot = at(x0, y0 + 1.0) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0) * fx;
        top * (1.0 - fy) + bot * fy
    }

    /// The quad (lo, hi) in local units for a text of em size `size` and an alignment.
    pub fn quad(&self, size: f64, align: &str) -> ((f64, f64), (f64, f64)) {
        let dx = match align {
            "left" => -self.i0.0,
            "right" => -self.i1.0,
            _ => -(self.i0.0 + self.i1.0) / 2.0,
        };
        let dy = -(self.i0.1 + self.i1.1) / 2.0;
        (((self.m0.0 + dx) * size, (self.m0.1 + dy) * size), ((self.m1.0 + dx) * size, (self.m1.1 + dy) * size))
    }
}

pub fn font() -> FontRef<'static> {
    FontRef::try_from_slice(FONT).expect("embedded DejaVu Sans Bold")
}

pub fn mask(text: &str) -> Option<Arc<Mask>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<Mask>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(m) = cache.lock().unwrap().get(text) {
        return m.clone();
    }
    let m = build(text).map(Arc::new);
    cache.lock().unwrap().insert(text.to_string(), m.clone());
    m
}

fn build(text: &str) -> Option<Mask> {
    let font = font();
    let scale = PxScale::from(PX * font.height_unscaled() / font.units_per_em()?);
    let sf = font.as_scaled(scale);
    let (mut caret, mut prev, mut outs) = (0.0f32, None, vec![]);
    for c in text.chars().filter(|c| !c.is_control()) {
        let g = font.glyph_id(c);
        if let Some(p) = prev {
            caret += sf.kern(p, g);
        }
        let glyph = g.with_scale_and_position(scale, point(caret, sf.ascent()));
        caret += sf.h_advance(g);
        prev = Some(g);
        if let Some(o) = font.outline_glyph(glyph) {
            outs.push(o);
        }
    }
    let first = outs.first()?.px_bounds();
    let (mut x0, mut y0, mut x1, mut y1) = (first.min.x, first.min.y, first.max.x, first.max.y);
    for o in &outs {
        let b = o.px_bounds();
        (x0, y0, x1, y1) = (x0.min(b.min.x), y0.min(b.min.y), x1.max(b.max.x), y1.max(b.max.y));
    }
    let (w, h) = ((x1 - x0 + 2.0 * PAD).ceil() as usize, (y1 - y0 + 2.0 * PAD).ceil() as usize);
    let mut data = vec![0u8; w * h];
    for o in &outs {
        let b = o.px_bounds();
        let (ox, oy) = ((b.min.x - x0 + PAD).round() as i64, (b.min.y - y0 + PAD).round() as i64);
        o.draw(|gx, gy, c| {
            let (x, y) = (ox + gx as i64, oy + gy as i64);
            if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                let i = y as usize * w + x as usize;
                data[i] = data[i].max((c.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        });
    }
    let em = |v: f32| (v / PX) as f64;
    let top = -em(y0 - PAD);
    Some(Mask {
        w,
        h,
        data,
        m0: (em(x0 - PAD), top - h as f64 / PX as f64),
        m1: (em(x0 - PAD) + w as f64 / PX as f64, top),
        i0: (em(x0), -em(y1)),
        i1: (em(x1), -em(y0)),
    })
}
