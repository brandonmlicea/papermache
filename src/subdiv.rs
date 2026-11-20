//! Loop subdivision: every triangle becomes four and the surface relaxes toward its smooth limit.
use crate::math::*;
use crate::model::Mesh;
use std::collections::HashMap;

/// Vertices at the same place become one, so faces share edges.
pub fn weld(m: &Mesh) -> Mesh {
    let span = m.v.iter().fold(Aabb::EMPTY, |b, p| b.grow(*p));
    let q = ((span.hi - span.lo).len() * 1e-7).max(1e-12);
    let mut at: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut out = Mesh { mats: m.mats.clone(), fm: m.fm.clone(), ..Mesh::default() };
    let map: Vec<u32> = m.v.iter().enumerate().map(|(i, p)| {
        *at.entry(((p.x / q).round() as i64, (p.y / q).round() as i64, (p.z / q).round() as i64)).or_insert_with(|| {
            out.v.push(*p);
            out.uv.push(m.uv.get(i).copied().unwrap_or([0.0, 0.0]));
            out.v.len() as u32 - 1
        })
    }).collect();
    out.f = m.f.iter().map(|f| f.map(|i| map[i as usize])).filter(|f| f[0] != f[1] && f[1] != f[2] && f[0] != f[2]).collect();
    if out.fm.len() != out.f.len() {
        out.fm.clear();
    }
    out
}

fn once(m: &Mesh) -> Mesh {
    let nv = m.v.len();
    let mut edges: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
    for f in &m.f {
        for k in 0..3 {
            let (a, b, c) = (f[k], f[(k + 1) % 3], f[(k + 2) % 3]);
            edges.entry((a.min(b), a.max(b))).or_default().push(c);
        }
    }
    let mut ring: Vec<Vec<u32>> = vec![vec![]; nv];
    let mut rim: Vec<Vec<u32>> = vec![vec![]; nv];
    for (&(a, b), opp) in &edges {
        ring[a as usize].push(b);
        ring[b as usize].push(a);
        if opp.len() == 1 {
            rim[a as usize].push(b);
            rim[b as usize].push(a);
        }
    }
    let mut out = Mesh { mats: m.mats.clone(), ..Mesh::default() };
    let uv = |i: u32| m.uv.get(i as usize).copied().unwrap_or([0.0, 0.0]);
    for i in 0..nv {
        let p = m.v[i];
        let moved = if !rim[i].is_empty() {
            if rim[i].len() == 2 { p * 0.75 + (m.v[rim[i][0] as usize] + m.v[rim[i][1] as usize]) * 0.125 } else { p }
        } else {
            let k = ring[i].len() as f64;
            let beta = if k > 3.0 { 3.0 / (8.0 * k) } else { 3.0 / 16.0 };
            p * (1.0 - k * beta) + ring[i].iter().fold(V3::default(), |s, &j| s + m.v[j as usize]) * beta
        };
        out.v.push(moved);
        out.uv.push(uv(i as u32));
    }
    let mut mid: HashMap<(u32, u32), u32> = HashMap::new();
    for (&(a, b), opp) in &edges {
        let (pa, pb) = (m.v[a as usize], m.v[b as usize]);
        let p = if opp.len() == 2 { (pa + pb) * 0.375 + (m.v[opp[0] as usize] + m.v[opp[1] as usize]) * 0.125 } else { (pa + pb) * 0.5 };
        out.v.push(p);
        let (ua, ub) = (uv(a), uv(b));
        out.uv.push([(ua[0] + ub[0]) / 2.0, (ua[1] + ub[1]) / 2.0]);
        mid.insert((a, b), out.v.len() as u32 - 1);
    }
    let e = |a: u32, b: u32| mid[&(a.min(b), a.max(b))];
    for (i, f) in m.f.iter().enumerate() {
        let [a, b, c] = *f;
        let (ab, bc, ca) = (e(a, b), e(b, c), e(c, a));
        out.f.extend([[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
        if let Some(&s) = m.fm.get(i) {
            out.fm.extend([s; 4]);
        }
    }
    out
}

/// `levels` rounds of Loop subdivision; each multiplies the faces by four.
pub fn subdivide(m: &Mesh, levels: usize) -> Mesh {
    let mut out = weld(m);
    for _ in 0..levels.min(5) {
        out = once(&out);
    }
    out
}
