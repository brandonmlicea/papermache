use crate::geom::Kind;
use crate::math::*;
use crate::scene::{parts, Scene};
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::fmt::Write as _;

pub type Faces = (Vec<V3>, Vec<[usize; 3]>);

pub fn load(path: &str) -> Result<Faces, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let lower = path.to_lowercase();
    if lower.ends_with(".obj") {
        Ok(obj(&String::from_utf8_lossy(&bytes)))
    } else if lower.ends_with(".stl") {
        stl(&bytes).ok_or_else(|| format!("{path}: not a readable STL"))
    } else {
        Err(format!("{path}: meshes are .obj or .stl"))
    }
}

fn obj(text: &str) -> Faces {
    let (mut vs, mut fs) = (vec![], vec![]);
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let c: Vec<f64> = it.take(3).filter_map(|x| x.parse().ok()).collect();
                if c.len() == 3 {
                    vs.push(v3(c[0], c[1], c[2]));
                }
            }
            Some("f") => {
                let n = vs.len() as i64;
                let idx: Vec<usize> = it
                    .filter_map(|t| t.split('/').next()?.parse::<i64>().ok())
                    .map(|i| (if i < 0 { n + i } else { i - 1 }) as usize)
                    .collect();
                for k in 1..idx.len().saturating_sub(1) {
                    fs.push([idx[0], idx[k], idx[k + 1]]);
                }
            }
            _ => {}
        }
    }
    (vs, fs)
}

fn stl(b: &[u8]) -> Option<Faces> {
    let (mut vs, mut fs) = (vec![], vec![]);
    if b.len() >= 84 {
        let n = u32::from_le_bytes(b[80..84].try_into().ok()?) as usize;
        if b.len() == 84 + n * 50 {
            for i in 0..n {
                let at = 84 + i * 50 + 12;
                let f = |k: usize| f32::from_le_bytes(b[at + k * 4..at + k * 4 + 4].try_into().unwrap()) as f64;
                for c in 0..3 {
                    vs.push(v3(f(c * 3), f(c * 3 + 1), f(c * 3 + 2)));
                }
                fs.push([i * 3, i * 3 + 1, i * 3 + 2]);
            }
            return Some((vs, fs));
        }
    }
    let text = std::str::from_utf8(b).ok()?;
    for line in text.lines() {
        let mut it = line.split_whitespace();
        if it.next() == Some("vertex") {
            let c: Vec<f64> = it.filter_map(|x| x.parse().ok()).collect();
            vs.push(v3(*c.first()?, *c.get(1)?, *c.get(2)?));
            if vs.len() % 3 == 0 {
                let i = vs.len() - 3;
                fs.push([i, i + 1, i + 2]);
            }
        }
    }
    (!fs.is_empty()).then_some((vs, fs))
}

fn grid(rows: usize, cols: usize, at: impl Fn(usize, usize) -> V3, out: &mut Vec<[V3; 3]>) {
    for i in 0..rows {
        for j in 0..cols {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            out.push([a, b, c]);
            out.push([a, c, d]);
        }
    }
}

fn axis_steps(h: f64, r: f64) -> Vec<f64> {
    if r <= 0.0 {
        return vec![-h, h];
    }
    let k = 6;
    let mut v: Vec<f64> = (0..=k).map(|i| -(h - r) - r * (i as f64 * FRAC_PI_2 / k as f64).cos()).collect();
    v.extend((0..=k).map(|i| (h - r) + r * (i as f64 * FRAC_PI_2 / k as f64).sin()));
    v.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    v
}

fn rbox(h: V3, r: f64, out: &mut Vec<[V3; 3]>) {
    let place = |q: V3| {
        if r <= 0.0 {
            return q;
        }
        let c = q.max(-(h - V3::splat(r))).min(h - V3::splat(r));
        let d = q - c;
        if d.len() < 1e-12 { q } else { c + d.norm() * r }
    };
    for axis in 0..3 {
        for side in [-1.0, 1.0] {
            let (u, w) = ((axis + 1) % 3, (axis + 2) % 3);
            let (su, sw) = (axis_steps(h.get(u), r), axis_steps(h.get(w), r));
            let at = |i: usize, j: usize| {
                let mut p = [0.0; 3];
                p[axis] = side * h.get(axis);
                p[u] = su[i];
                p[w] = sw[j];
                place(V3::from(p))
            };
            let start = out.len();
            grid(su.len() - 1, sw.len() - 1, at, out);
            if side < 0.0 {
                for t in &mut out[start..] {
                    t.swap(1, 2);
                }
            }
        }
    }
}

/// Triangles of one local shape, outward wound. Text has none; a picture is its row runs as boxes.
pub fn tessellate(k: &Kind) -> Vec<[V3; 3]> {
    let mut out = vec![];
    match k {
        Kind::Box(h) => rbox(*h, 0.0, &mut out),
        Kind::RBox(h, r) => rbox(*h, *r, &mut out),
        Kind::Cyl { r0, r1, h2 } => {
            let s = 48;
            let ring = |j: usize, z: f64, r: f64| {
                let a = TAU * j as f64 / s as f64;
                v3(r * a.cos(), r * a.sin(), z)
            };
            grid(1, s, |i, j| if i == 0 { ring(j, *h2, *r1) } else { ring(j, -h2, *r0) }, &mut out);
            for j in 0..s {
                if *r0 > 0.0 {
                    out.push([v3(0.0, 0.0, -h2), ring(j + 1, -h2, *r0), ring(j, -h2, *r0)]);
                }
                if *r1 > 0.0 {
                    out.push([v3(0.0, 0.0, *h2), ring(j, *h2, *r1), ring(j + 1, *h2, *r1)]);
                }
            }
        }
        Kind::Sphere(r) => grid(16, 32, |i, j| {
            let (th, ph) = (PI * i as f64 / 16.0, TAU * j as f64 / 32.0);
            v3(r * th.sin() * ph.cos(), r * th.sin() * ph.sin(), r * th.cos())
        }, &mut out),
        Kind::Torus(big, r) => grid(48, 24, |i, j| {
            let (u, w) = (TAU * i as f64 / 48.0, TAU * j as f64 / 24.0);
            let rr = big + r * w.cos();
            v3(rr * u.cos(), rr * u.sin(), r * w.sin())
        }, &mut out),
        Kind::Plane(w, h) => grid(1, 1, |i, j| v3(if i == 0 { -w } else { *w }, if j == 0 { -h } else { *h }, 0.0), &mut out),
        Kind::Text { .. } => {}
        Kind::Image { half, px } => {
            for (c, h) in px.runs(*half) {
                let mut b = vec![];
                rbox(h, 0.0, &mut b);
                out.extend(b.into_iter().map(|t| t.map(|p| p + c)));
            }
        }
        Kind::Tri { a, e1, e2, .. } => out.push([*a, *a + *e1, *a + *e2]),
    }
    out
}

/// Writes every visible non-text object (or those tagged `tag`) as .obj (+ .mtl) or binary .stl.
pub fn export(s: &Scene, path: &str, tag: Option<&str>) -> Result<String, String> {
    let mut groups: Vec<(String, String, Vec<[V3; 3]>)> = vec![];
    for o in s.objects.iter().filter(|o| !o.hidden && tag.is_none_or(|t| o.tags.iter().any(|x| x == t))) {
        let mut tris = vec![];
        for (a, k) in parts(o)? {
            let flip = a.det() < 0.0;
            for t in tessellate(&k) {
                let t = if matches!(k, Kind::Tri { .. }) { t } else { t.map(|p| a.apply(p)) };
                tris.push(if flip { [t[0], t[2], t[1]] } else { t });
            }
        }
        if !tris.is_empty() {
            groups.push((o.name.clone(), o.material.clone(), tris));
        }
    }
    let total: usize = groups.iter().map(|g| g.2.len()).sum();
    let err = |e: std::io::Error| format!("{path}: {e}");
    if path.to_lowercase().ends_with(".stl") {
        let mut b = vec![0u8; 80];
        b[..19].copy_from_slice(b"Paper Mache export\0");
        b.extend((total as u32).to_le_bytes());
        for t in groups.iter().flat_map(|g| g.2.iter()) {
            let n = (t[1] - t[0]).cross(t[2] - t[0]).norm();
            for v in [n, t[0], t[1], t[2]] {
                for c in v.arr() {
                    b.extend((c as f32).to_le_bytes());
                }
            }
            b.extend([0u8, 0]);
        }
        std::fs::write(path, b).map_err(err)?;
    } else {
        let mtl = std::path::Path::new(path).with_extension("mtl");
        let mtl_name = mtl.file_name().and_then(|n| n.to_str()).unwrap_or("model.mtl").to_string();
        let (mut o, mut m) = (format!("# Paper Mache\nmtllib {mtl_name}\n"), String::from("# Paper Mache\n"));
        for (name, mat) in &s.materials {
            let c = mat.color;
            let _ = writeln!(m, "newmtl {name}\nKd {} {} {}\nKs {k} {k} {k}\nNs {}\nd {}\nKe {} {} {}", c[0], c[1], c[2],
                ((1.0 - mat.roughness) * 900.0).max(1.0), mat.opacity, mat.emission[0], mat.emission[1], mat.emission[2], k = 0.04 + 0.5 * mat.metallic);
        }
        let mut base = 1;
        for (name, mat, tris) in &groups {
            let _ = writeln!(o, "o {}", name.replace(' ', "_"));
            if !mat.is_empty() {
                let _ = writeln!(o, "usemtl {mat}");
            }
            for t in tris {
                for p in t {
                    let _ = writeln!(o, "v {:.5} {:.5} {:.5}", p.x, p.y, p.z);
                }
            }
            for i in 0..tris.len() {
                let _ = writeln!(o, "f {} {} {}", base + i * 3, base + i * 3 + 1, base + i * 3 + 2);
            }
            base += tris.len() * 3;
        }
        std::fs::write(path, o).map_err(err)?;
        std::fs::write(&mtl, m).map_err(err)?;
    }
    Ok(format!("{total} triangles from {} objects to {path}", groups.len()))
}
