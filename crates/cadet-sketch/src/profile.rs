use crate::{Constraint, E, Entity, P, Sketch};
use glam::DVec2;
use std::collections::HashMap;
use std::f64::consts::TAU;

fn arc(c: DVec2, a: DVec2, b: DVec2) -> Vec<DVec2> {
    let (r, a0) = ((a - c).length(), (a - c).to_angle());
    let mut sweep = ((b - c).to_angle() - a0).rem_euclid(TAU);
    if sweep < 1e-9 {
        sweep = TAU;
    }
    let n = ((sweep / TAU * 96.0).ceil() as usize).max(2);
    (0..=n).map(|i| c + DVec2::from_angle(a0 + sweep * i as f64 / n as f64) * r).collect()
}

/// Points along an entity, for drawing and picking.
pub fn polyline(sk: &Sketch, e: Entity) -> Vec<DVec2> {
    match e {
        Entity::Line { a, b } => vec![sk.pt(a), sk.pt(b)],
        Entity::Circle { c, r } => {
            let (c, r) = (sk.pt(c), sk.params[r]);
            (0..=96).map(|i| c + DVec2::from_angle(TAU * i as f64 / 96.0) * r).collect()
        }
        Entity::Arc { c, a, b } => arc(sk.pt(c), sk.pt(a), sk.pt(b)),
    }
}

/// Exact boundary pieces of a loop, in traversal order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Line(DVec2, DVec2),
    /// Start, a point midway along the arc, end.
    Arc(DVec2, DVec2, DVec2),
    Circle(DVec2, f64),
}

#[derive(Clone, Debug)]
pub struct Loop {
    pub segs: Vec<Seg>,
    pub poly: Vec<DVec2>,
}

/// A fillable area: an outer loop minus the loops directly inside it.
#[derive(Clone, Debug)]
pub struct Region {
    pub outer: usize,
    pub holes: Vec<usize>,
    /// Nesting depth of the outer loop; even depths are the natural "solid" regions.
    pub depth: usize,
}

fn arc_mid(c: DVec2, a: DVec2, b: DVec2) -> DVec2 {
    let a0 = (a - c).to_angle();
    let mut sweep = ((b - c).to_angle() - a0).rem_euclid(TAU);
    if sweep < 1e-9 {
        sweep = TAU;
    }
    c + DVec2::from_angle(a0 + sweep * 0.5) * (a - c).length()
}

pub fn contains(poly: &[DVec2], p: DVec2) -> bool {
    let mut inside = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

fn area(poly: &[DVec2]) -> f64 {
    (0..poly.len()).map(|i| poly[i].perp_dot(poly[(i + 1) % poly.len()])).sum::<f64>().abs() * 0.5
}

/// One region per loop, minus the loops directly inside it.
pub fn regions(loops: &[Loop]) -> Vec<Region> {
    let parent: Vec<Option<usize>> = (0..loops.len())
        .map(|i| {
            (0..loops.len())
                .filter(|&j| j != i && area(&loops[j].poly) > area(&loops[i].poly) && contains(&loops[j].poly, loops[i].poly[0]))
                .min_by(|&a, &b| area(&loops[a].poly).total_cmp(&area(&loops[b].poly)))
        })
        .collect();
    let depth = |mut i: usize| {
        let mut d = 0;
        while let Some(p) = parent[i] {
            d += 1;
            i = p;
        }
        d
    };
    (0..loops.len()).map(|i| Region { outer: i, holes: (0..loops.len()).filter(|&j| parent[j] == Some(i)).collect(), depth: depth(i) }).collect()
}

impl Region {
    pub fn contains(&self, loops: &[Loop], p: DVec2) -> bool {
        contains(&loops[self.outer].poly, p) && !self.holes.iter().any(|&h| contains(&loops[h].poly, p))
    }
}

pub fn profiles(sk: &Sketch) -> Vec<Vec<DVec2>> {
    loops(sk).into_iter().map(|l| l.poly).collect()
}

/// Closed loops: circles, plus chains of lines/arcs whose every node has degree 2.
pub fn loops(sk: &Sketch) -> Vec<Loop> {
    let mut parent: HashMap<P, P> = HashMap::new();
    fn root(parent: &mut HashMap<P, P>, p: P) -> P {
        let q = *parent.get(&p).unwrap_or(&p);
        if q == p {
            p
        } else {
            let r = root(parent, q);
            parent.insert(p, r);
            r
        }
    }
    for c in &sk.constraints {
        if let Constraint::Coincident(a, b) = *c {
            let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
            parent.insert(ra, rb);
        }
    }

    let mut loops = vec![];
    let mut edges: Vec<(E, P, P)> = vec![];
    for (i, e) in sk.live().filter(|(i, _)| !sk.is_construction(*i)) {
        match e {
            Entity::Circle { c, r } => loops.push(Loop { segs: vec![Seg::Circle(sk.pt(c), sk.params[r])], poly: polyline(sk, e) }),
            Entity::Line { a, b } | Entity::Arc { a, b, .. } => edges.push((i, root(&mut parent, a), root(&mut parent, b))),
        }
    }
    let mut adj: HashMap<P, Vec<usize>> = HashMap::new();
    for (k, &(_, a, b)) in edges.iter().enumerate() {
        adj.entry(a).or_default().push(k);
        adj.entry(b).or_default().push(k);
    }
    let mut seen = vec![false; edges.len()];
    for start in 0..edges.len() {
        if seen[start] {
            continue;
        }
        let (mut poly, mut segs) = (vec![], vec![]);
        let (mut k, mut node) = (start, edges[start].1);
        let closed = loop {
            seen[k] = true;
            let (e, a, b) = edges[k];
            let forward = node == a;
            let ent = sk.entity(e).unwrap();
            let mut pts = polyline(sk, ent);
            if !forward {
                pts.reverse();
            }
            let (s0, s1) = (pts[0], *pts.last().unwrap());
            segs.push(match ent {
                Entity::Arc { c, a, b } => Seg::Arc(s0, arc_mid(sk.pt(c), sk.pt(a), sk.pt(b)), s1),
                _ => Seg::Line(s0, s1),
            });
            pts.pop();
            poly.extend(pts);
            node = if forward { b } else { a };
            let next = &adj[&node];
            if next.len() != 2 {
                break false;
            }
            k = if next[0] == k { next[1] } else { next[0] };
            if k == start {
                break true;
            }
            if seen[k] {
                break false;
            }
        };
        if closed && poly.len() >= 3 {
            loops.push(Loop { segs, poly });
        }
    }
    loops
}

/// Ear-clipping triangulation of a simple polygon.
pub fn triangulate(poly: &[DVec2]) -> Vec<[usize; 3]> {
    let area: f64 = (0..poly.len()).map(|i| poly[i].perp_dot(poly[(i + 1) % poly.len()])).sum();
    let mut idx: Vec<usize> = (0..poly.len()).collect();
    if area < 0.0 {
        idx.reverse();
    }
    let mut tris = vec![];
    let mut guard = 0;
    while idx.len() > 3 && guard < poly.len() * poly.len() {
        guard += 1;
        let n = idx.len();
        let ear = (0..n).find(|&i| {
            let (a, b, c) = (poly[idx[(i + n - 1) % n]], poly[idx[i]], poly[idx[(i + 1) % n]]);
            (b - a).perp_dot(c - b) > 1e-12
                && idx.iter().all(|&j| {
                    let p = poly[j];
                    p == a || p == b || p == c || !in_triangle(p, a, b, c)
                })
        });
        let Some(i) = ear else { break };
        tris.push([idx[(i + n - 1) % n], idx[i], idx[(i + 1) % n]]);
        idx.remove(i);
    }
    if idx.len() == 3 {
        tris.push([idx[0], idx[1], idx[2]]);
    }
    tris
}

fn in_triangle(p: DVec2, a: DVec2, b: DVec2, c: DVec2) -> bool {
    let (d1, d2, d3) = ((b - a).perp_dot(p - a), (c - b).perp_dot(p - b), (a - c).perp_dot(p - c));
    d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0
}
