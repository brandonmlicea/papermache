use crate::geom::{Hit, Prim};
use crate::math::*;

pub struct Node {
    pub b: Aabb,
    pub a: u32,
    pub n: u32,
}

/// Binned-SAH bounding volume hierarchy; a node with n > 0 is a leaf over idx[a..a+n], else its children are a and a+1.
pub struct Bvh {
    pub nodes: Vec<Node>,
    pub idx: Vec<u32>,
}

const BINS: usize = 16;

impl Bvh {
    pub fn build(prims: &[Prim]) -> Bvh {
        let mut idx: Vec<u32> = (0..prims.len() as u32).collect();
        if prims.is_empty() {
            return Bvh { nodes: vec![], idx };
        }
        let cents: Vec<V3> = prims.iter().map(|p| p.bounds.centroid()).collect();
        let mut nodes = vec![Node { b: Aabb::EMPTY, a: 0, n: 0 }];
        split(&mut nodes, 0, 0, prims.len(), &mut idx, prims, &cents);
        Bvh { nodes, idx }
    }

    pub fn closest(&self, prims: &[Prim], o: V3, d: V3, tmin: f64, mut tmax: f64) -> Option<(Hit, u32)> {
        let mut best = None;
        self.walk(o, d, &mut tmax, &mut |i, tmax| {
            if let Some(h) = prims[i as usize].hit(o, d, tmin, *tmax) {
                *tmax = h.t;
                best = Some((h, i));
            }
            false
        });
        best
    }

    /// Calls f on every primitive the ray hits before tmax, in no particular order; f returns true to stop.
    pub fn each(&self, prims: &[Prim], o: V3, d: V3, tmin: f64, mut tmax: f64, f: &mut dyn FnMut(u32, &Hit) -> bool) {
        let limit = tmax;
        self.walk(o, d, &mut tmax, &mut |i, _| prims[i as usize].hit(o, d, tmin, limit).is_some_and(|h| f(i, &h)));
    }

    fn walk(&self, o: V3, d: V3, tmax: &mut f64, f: &mut dyn FnMut(u32, &mut f64) -> bool) {
        if self.nodes.is_empty() {
            return;
        }
        let inv = |c: f64| if c.abs() < 1e-300 { 1e300 } else { 1.0 / c };
        let inv = v3(inv(d.x), inv(d.y), inv(d.z));
        let mut stack = [0u32; 128];
        let mut sp = 1;
        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp] as usize];
            if node.b.hit(o, inv, *tmax).is_none() {
                continue;
            }
            if node.n > 0 {
                for k in node.a..node.a + node.n {
                    if f(self.idx[k as usize], tmax) {
                        return;
                    }
                }
                continue;
            }
            let (l, r) = (node.a, node.a + 1);
            let tl = self.nodes[l as usize].b.hit(o, inv, *tmax);
            let tr = self.nodes[r as usize].b.hit(o, inv, *tmax);
            let (near, far) = match (tl, tr) {
                (Some(a), Some(b)) if b < a => (Some(r), Some(l)),
                (Some(_), Some(_)) => (Some(l), Some(r)),
                (Some(_), None) => (Some(l), None),
                (None, Some(_)) => (Some(r), None),
                _ => (None, None),
            };
            for c in [far, near].into_iter().flatten() {
                if sp < stack.len() {
                    stack[sp] = c;
                    sp += 1;
                }
            }
        }
    }
}

fn split(nodes: &mut Vec<Node>, ni: usize, start: usize, end: usize, idx: &mut [u32], prims: &[Prim], cents: &[V3]) {
    let mut b = Aabb::EMPTY;
    let mut cb = Aabb::EMPTY;
    for &i in &idx[start..end] {
        b = b.union(prims[i as usize].bounds);
        cb = cb.grow(cents[i as usize]);
    }
    nodes[ni].b = b;
    let count = end - start;
    let leaf = |nodes: &mut Vec<Node>| {
        nodes[ni].a = start as u32;
        nodes[ni].n = count as u32;
    };
    if count <= 2 {
        return leaf(nodes);
    }
    let ext = cb.hi - cb.lo;
    let axis = if ext.x >= ext.y && ext.x >= ext.z { 0 } else if ext.y >= ext.z { 1 } else { 2 };
    let (lo, span) = (cb.lo.get(axis), ext.get(axis));
    let mut mid = start + count / 2;
    if span > 1e-12 {
        let bin = |c: V3| (((c.get(axis) - lo) / span * BINS as f64) as usize).min(BINS - 1);
        let mut boxes = [Aabb::EMPTY; BINS];
        let mut counts = [0usize; BINS];
        for &i in &idx[start..end] {
            let k = bin(cents[i as usize]);
            boxes[k] = boxes[k].union(prims[i as usize].bounds);
            counts[k] += 1;
        }
        let (mut best, mut cut) = (f64::INFINITY, 0);
        for s in 1..BINS {
            let (mut lb, mut rb, mut ln, mut rn) = (Aabb::EMPTY, Aabb::EMPTY, 0, 0);
            for k in 0..s {
                lb = lb.union(boxes[k]);
                ln += counts[k];
            }
            for k in s..BINS {
                rb = rb.union(boxes[k]);
                rn += counts[k];
            }
            let cost = lb.area() * ln as f64 + rb.area() * rn as f64;
            if ln > 0 && rn > 0 && cost < best {
                best = cost;
                cut = s;
            }
        }
        if count <= 4 && best >= b.area() * count as f64 {
            return leaf(nodes);
        }
        if cut > 0 {
            let (mut i, mut j) = (start, end);
            while i < j {
                if bin(cents[idx[i] as usize]) < cut {
                    i += 1;
                } else {
                    j -= 1;
                    idx.swap(i, j);
                }
            }
            mid = i;
        }
    } else if count <= 4 {
        return leaf(nodes);
    }
    if mid == start || mid == end {
        idx[start..end].sort_by(|a, b| cents[*a as usize].get(axis).total_cmp(&cents[*b as usize].get(axis)));
        mid = start + count / 2;
    }
    let l = nodes.len();
    nodes.push(Node { b: Aabb::EMPTY, a: 0, n: 0 });
    nodes.push(Node { b: Aabb::EMPTY, a: 0, n: 0 });
    nodes[ni].a = l as u32;
    nodes[ni].n = 0;
    split(nodes, l, start, mid, idx, prims, cents);
    split(nodes, l + 1, mid, end, idx, prims, cents);
}
