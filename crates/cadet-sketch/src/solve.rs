use crate::{Constraint, E, Entity, Sketch};
use glam::DVec2;

const TOL: f64 = 1e-9;

#[derive(Clone, Copy)]
enum Eq {
    C(Constraint),
    ArcRadius(E),
}

struct System<'a> {
    sk: &'a Sketch,
    eqs: Vec<Eq>,
    free: Vec<usize>,
    col: Vec<Option<usize>>,
    scale: Vec<f64>,
}

/// Solves the sketch in place. `drag` moves params to targets; those params become
/// expensive to move, so the rest of the sketch follows them where constraints allow.
/// Returns false and leaves params unchanged if the constraints can't be satisfied.
pub fn solve(sk: &mut Sketch, drag: &[(usize, f64)]) -> bool {
    let backup = sk.params.clone();
    let free: Vec<bool> = system(sk, &[]).col.iter().map(Option::is_some).collect();
    let drag: Vec<_> = drag.iter().filter(|d| free[d.0]).copied().collect();
    for &(i, v) in &drag {
        sk.params[i] = v;
    }
    let heavy: Vec<usize> = drag.iter().map(|d| d.0).collect();
    let mut x = sk.params.clone();
    if system(sk, &heavy).split().iter().all(|part| part.minimize(&mut x)) {
        sk.params = x;
        true
    } else {
        sk.params = backup;
        false
    }
}

fn system<'a>(sk: &'a Sketch, heavy: &[usize]) -> System<'a> {
    let mut fixed = vec![false; sk.params.len()];
    fixed[Sketch::ORIGIN] = true;
    fixed[Sketch::ORIGIN + 1] = true;
    let mut eqs = vec![];
    for &c in &sk.constraints {
        match c {
            Constraint::Fix(p) => {
                fixed[p] = true;
                fixed[p + 1] = true;
            }
            c => eqs.push(Eq::C(c)),
        }
    }
    for (e, ent) in sk.live() {
        if let Entity::Arc { .. } = ent {
            eqs.push(Eq::ArcRadius(e));
        }
    }
    let mut used = vec![false; sk.params.len()];
    for &p in &sk.points {
        used[p] = true;
        used[p + 1] = true;
    }
    for (_, e) in sk.live() {
        if let Entity::Circle { r, .. } = e {
            used[r] = true;
        }
    }
    let free: Vec<usize> = (0..sk.params.len()).filter(|&i| used[i] && !fixed[i]).collect();
    let mut col = vec![None; sk.params.len()];
    for (j, &i) in free.iter().enumerate() {
        col[i] = Some(j);
    }
    let scale = free.iter().map(|i| if heavy.contains(i) { 0.01 } else { 1.0 }).collect();
    System { sk, eqs, free, col, scale }
}

impl<'a> System<'a> {
    /// Independent subsystems: equations connected through shared free params.
    fn split(&self) -> Vec<System<'a>> {
        let n = self.free.len();
        let mut parent: Vec<usize> = (0..=n).collect();
        fn root(p: &mut [usize], i: usize) -> usize {
            let mut r = i;
            while p[r] != r {
                r = p[r];
            }
            p[i] = r;
            r
        }
        let eq_cols: Vec<Vec<usize>> = self.eqs.iter().map(|e| deps(self.sk, e).into_iter().filter_map(|i| self.col[i]).collect()).collect();
        for cols in &eq_cols {
            for w in cols.windows(2) {
                let (a, b) = (root(&mut parent, w[0]), root(&mut parent, w[1]));
                parent[a] = b;
            }
        }
        let mut groups: std::collections::BTreeMap<usize, (Vec<Eq>, Vec<usize>)> = Default::default();
        for (eq, cols) in self.eqs.iter().zip(&eq_cols) {
            let g = cols.first().map_or(n, |&c| root(&mut parent, c));
            groups.entry(g).or_default().0.push(*eq);
        }
        for c in 0..n {
            let g = root(&mut parent, c);
            groups.entry(g).or_default().1.push(c);
        }
        groups
            .into_values()
            .map(|(eqs, cols)| {
                let free: Vec<usize> = cols.iter().map(|&c| self.free[c]).collect();
                let mut col = vec![None; self.col.len()];
                for (j, &i) in free.iter().enumerate() {
                    col[i] = Some(j);
                }
                let scale = cols.iter().map(|&c| self.scale[c]).collect();
                System { sk: self.sk, eqs, free, col, scale }
            })
            .collect()
    }

    fn residuals(&self, x: &[f64]) -> Vec<f64> {
        let mut r = Vec::with_capacity(self.eqs.len() * 2);
        for eq in &self.eqs {
            eval(self.sk, eq, x, &mut r);
        }
        r
    }

    fn jacobian(&self, x: &mut [f64], m: usize) -> Vec<f64> {
        let n = self.free.len();
        let mut j = vec![0.0; m * n];
        let mut row = 0;
        let (mut lo, mut hi) = (Vec::new(), Vec::new());
        for eq in &self.eqs {
            lo.clear();
            eval(self.sk, eq, x, &mut lo);
            let rows = lo.len();
            for i in deps(self.sk, eq) {
                let Some(c) = self.col[i] else { continue };
                let h = 1e-7 * x[i].abs().max(1.0);
                let x0 = x[i];
                x[i] = x0 + h;
                hi.clear();
                eval(self.sk, eq, x, &mut hi);
                x[i] = x0 - h;
                lo.clear();
                eval(self.sk, eq, x, &mut lo);
                x[i] = x0;
                for k in 0..rows {
                    j[(row + k) * n + c] = (hi[k] - lo[k]) / (2.0 * h);
                }
            }
            row += rows;
        }
        j
    }

    /// Damped Gauss-Newton. Uses the min-norm form Jᵀ(JJᵀ+λI)⁻¹ when under-determined,
    /// so free geometry moves as little as possible, else (JᵀJ+λI)⁻¹Jᵀ.
    fn minimize(&self, x: &mut Vec<f64>) -> bool {
        let n = self.free.len();
        let mut r = self.residuals(x);
        let m = r.len();
        let mut err = norm2(&r);
        let mut lambda = 1e-9;
        let wide = m <= n;
        let k = if wide { m } else { n };
        for _ in 0..100 {
            if max_abs(&r) < TOL || n == 0 {
                break;
            }
            let mut j = self.jacobian(x, m);
            for (i, v) in j.iter_mut().enumerate() {
                *v *= self.scale[i % n];
            }
            let at = |p: usize, q: usize| j[p * n + q];
            let mut a = vec![0.0; k * k];
            for p in 0..k {
                for q in 0..=p {
                    let s: f64 = if wide { (0..n).map(|i| at(p, i) * at(q, i)).sum() } else { (0..m).map(|i| at(i, p) * at(i, q)).sum() };
                    a[p * k + q] = s;
                    a[q * k + p] = s;
                }
            }
            let rhs: Vec<f64> = if wide { r.iter().map(|v| -v).collect() } else { (0..n).map(|q| -(0..m).map(|i| at(i, q) * r[i]).sum::<f64>()).collect() };
            let improved = loop {
                let mut damped = a.clone();
                for p in 0..k {
                    damped[p * k + p] += lambda * (1.0 + a[p * k + p]);
                }
                let mut y = rhs.clone();
                if cholesky_solve(&mut damped, &mut y, k) {
                    let mut trial = x.clone();
                    for (q, &i) in self.free.iter().enumerate() {
                        trial[i] += self.scale[q] * if wide { (0..m).map(|p| at(p, q) * y[p]).sum::<f64>() } else { y[q] };
                    }
                    let rt = self.residuals(&trial);
                    let et = norm2(&rt);
                    if et < err {
                        let gain = err - et;
                        *x = trial;
                        r = rt;
                        err = et;
                        lambda = (lambda * 0.1).max(1e-15);
                        break gain > 1e-24;
                    }
                }
                lambda *= 10.0;
                if lambda > 1e8 {
                    break false;
                }
            };
            if !improved {
                break;
            }
        }
        max_abs(&r) < TOL
    }
}

pub struct Analysis {
    pub dof: usize,
    /// Per param: true if no remaining freedom can move it.
    pub determined: Vec<bool>,
}

pub fn analyze(sk: &Sketch) -> Analysis {
    let mut a = Analysis { dof: 0, determined: vec![true; sk.params.len()] };
    for sys in system(sk, &[]).split() {
        sys.rank_into(&mut a);
    }
    a
}

impl System<'_> {
    fn rank_into(&self, out: &mut Analysis) {
        let mut x = self.sk.params.clone();
        let m = self.residuals(&x).len();
        let n = self.free.len();
        let mut j = self.jacobian(&mut x, m);
        let mut pivots = vec![];
        for c in 0..n {
            let row = pivots.len();
            let Some(p) = (row..m).max_by(|&a, &b| j[a * n + c].abs().total_cmp(&j[b * n + c].abs())) else { break };
            if j[p * n + c].abs() < 1e-8 {
                continue;
            }
            for k in 0..n {
                j.swap(row * n + k, p * n + k);
            }
            let d = j[row * n + c];
            for k in 0..n {
                j[row * n + k] /= d;
            }
            for q in 0..m {
                let f = j[q * n + c];
                if q != row && f != 0.0 {
                    for k in 0..n {
                        j[q * n + k] -= f * j[row * n + k];
                    }
                }
            }
            pivots.push(c);
        }
        for &i in &self.free {
            out.determined[i] = false;
        }
        for (r, &c) in pivots.iter().enumerate() {
            out.determined[self.free[c]] = (0..n).filter(|k| !pivots.contains(k)).all(|k| j[r * n + k].abs() < 1e-8);
        }
        out.dof += n - pivots.len();
    }
}

fn deps(sk: &Sketch, eq: &Eq) -> Vec<usize> {
    let pt = |p: usize| [p, p + 1];
    let ent = |e: E| match sk.entity(e) {
        Some(Entity::Line { a, b }) => vec![a, a + 1, b, b + 1],
        Some(Entity::Circle { c, r }) => vec![c, c + 1, r],
        Some(Entity::Arc { c, a, b }) => vec![c, c + 1, a, a + 1, b, b + 1],
        None => vec![],
    };
    use Constraint::*;
    match *eq {
        Eq::ArcRadius(e) => ent(e),
        Eq::C(c) => match c {
            Coincident(a, b) | Horizontal(a, b) | Vertical(a, b) | Distance(a, b, _) => [pt(a), pt(b)].concat(),
            Parallel(a, b) | Perpendicular(a, b) | Equal(a, b) | Tangent(a, b) => [ent(a), ent(b)].concat(),
            OnCurve(p, e) | Fraction(p, e, _) => [pt(p).to_vec(), ent(e)].concat(),
            Radius(e, _) => ent(e),
            Fix(_) => vec![],
        },
    }
}

fn eval(sk: &Sketch, eq: &Eq, x: &[f64], out: &mut Vec<f64>) {
    let pt = |p: usize| DVec2::new(x[p], x[p + 1]);
    let dir = |e: E| match sk.entity(e) {
        Some(Entity::Line { a, b }) => (pt(b) - pt(a)).normalize_or_zero(),
        _ => DVec2::ZERO,
    };
    let size = |e: E| match sk.entity(e) {
        Some(Entity::Line { a, b }) => pt(a).distance(pt(b)),
        Some(Entity::Circle { r, .. }) => x[r],
        Some(Entity::Arc { c, a, .. }) => pt(a).distance(pt(c)),
        None => 0.0,
    };
    let center = |e: E| match sk.entity(e) {
        Some(Entity::Circle { c, .. } | Entity::Arc { c, .. }) => Some(pt(c)),
        _ => None,
    };
    let line = |e: E| match sk.entity(e) {
        Some(Entity::Line { a, .. }) => Some(pt(a)),
        _ => None,
    };
    use Constraint::*;
    match *eq {
        Eq::ArcRadius(e) => {
            if let Some(Entity::Arc { c, a, b }) = sk.entity(e) {
                out.push(pt(a).distance(pt(c)) - pt(b).distance(pt(c)));
            }
        }
        Eq::C(c) => match c {
            Coincident(a, b) => out.extend((pt(a) - pt(b)).to_array()),
            Horizontal(a, b) => out.push(x[a + 1] - x[b + 1]),
            Vertical(a, b) => out.push(x[a] - x[b]),
            Parallel(a, b) => out.push(dir(a).perp_dot(dir(b))),
            Perpendicular(a, b) => out.push(dir(a).dot(dir(b))),
            Equal(a, b) => out.push(size(a) - size(b)),
            Distance(a, b, d) => out.push(pt(a).distance(pt(b)) - d),
            Radius(e, v) => out.push(size(e) - v),
            OnCurve(p, e) => match (line(e), center(e)) {
                (Some(a), _) => out.push(dir(e).perp_dot(pt(p) - a)),
                (_, Some(c)) => out.push(pt(p).distance(c) - size(e)),
                _ => out.push(0.0),
            },
            Tangent(a, b) => {
                let (l, k) = if line(a).is_some() { (a, b) } else { (b, a) };
                match (line(l), center(l), center(k)) {
                    (Some(p0), _, Some(c)) => out.push(dir(l).perp_dot(c - p0).abs() - size(k)),
                    (_, Some(c1), Some(c2)) => {
                        let d = c1.distance(c2);
                        let (r1, r2) = (size(l), size(k));
                        out.push((d - (r1 + r2)).abs().min((d - (r1 - r2).abs()).abs()));
                    }
                    _ => out.push(0.0),
                }
            }
            Fraction(p, e, t) => match sk.entity(e) {
                Some(Entity::Line { a, b }) => out.extend((pt(p) - pt(a).lerp(pt(b), t)).to_array()),
                _ => out.extend([0.0, 0.0]),
            },
            Fix(_) => {}
        },
    }
}

fn norm2(r: &[f64]) -> f64 {
    r.iter().map(|v| v * v).sum()
}

fn max_abs(r: &[f64]) -> f64 {
    r.iter().fold(0.0, |m, v| m.max(v.abs()))
}

fn cholesky_solve(a: &mut [f64], b: &mut [f64], n: usize) -> bool {
    for i in 0..n {
        for j in 0..=i {
            let s = a[i * n + j] - (0..j).map(|k| a[i * n + k] * a[j * n + k]).sum::<f64>();
            if i == j {
                if s <= 0.0 {
                    return false;
                }
                a[i * n + i] = s.sqrt();
            } else {
                a[i * n + j] = s / a[j * n + j];
            }
        }
    }
    for i in 0..n {
        b[i] = (b[i] - (0..i).map(|k| a[i * n + k] * b[k]).sum::<f64>()) / a[i * n + i];
    }
    for i in (0..n).rev() {
        b[i] = (b[i] - (i + 1..n).map(|k| a[k * n + i] * b[k]).sum::<f64>()) / a[i * n + i];
    }
    true
}
