//! Solid modeling on top of OpenCascade, through our own exception-safe C++ layer (occt.cpp).
//! OCCT failures come back as `Err`; no OCCT types leak out of this crate.

mod ffi;

use cadet_sketch::{Loop, Region, Seg};
use cxx::UniquePtr;
use ffi::occt as occ;
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    New,
    Join,
    Cut,
}

/// A sketch plane in world space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub o: DVec3,
    pub u: DVec3,
    pub v: DVec3,
}

impl Frame {
    pub fn at(&self, q: DVec2) -> DVec3 {
        self.o + self.u * q.x + self.v * q.y
    }

    pub fn normal(&self) -> DVec3 {
        self.u.cross(self.v)
    }
}

type Shape = UniquePtr<occ::TopoDS_Shape>;
pub type Result<T> = std::result::Result<T, String>;

pub struct Body(Shape);
pub struct Face(Shape);

fn err(e: cxx::Exception) -> String {
    e.what().to_string()
}

fn wire(f: &Frame, l: &Loop) -> Result<Shape> {
    let p3 = |q: DVec2| f.at(q).to_array();
    let segs: Vec<occ::Seg> = l
        .segs
        .iter()
        .map(|s| {
            let (kind, pts, r) = match *s {
                Seg::Line(a, b) => (0, [p3(a), p3(b), [0.0; 3]], 0.0),
                Seg::Arc(a, m, b) => (1, [p3(a), p3(m), p3(b)], 0.0),
                Seg::Circle(c, r) => (2, [p3(c), f.normal().to_array(), [0.0; 3]], r),
            };
            occ::Seg { kind, p: pts.concat().try_into().unwrap(), r }
        })
        .collect();
    occ::wire(&segs).map_err(err)
}

/// Planar faces for the given regions; holes are cut out with a boolean so loop winding doesn't matter.
pub fn faces(f: &Frame, loops: &[Loop], regions: &[&Region]) -> Result<Vec<Face>> {
    regions
        .iter()
        .map(|r| {
            let mut face = occ::face(&*wire(f, &loops[r.outer])?).map_err(err)?;
            for &h in &r.holes {
                let hole = occ::face(&*wire(f, &loops[h])?).map_err(err)?;
                face = occ::cut(&face, &hole).map_err(err)?;
            }
            Ok(Face(face))
        })
        .collect()
}

fn union_all(shapes: Vec<Shape>) -> Result<Body> {
    let mut it = shapes.into_iter();
    let first = it.next().ok_or("Nothing to build")?;
    it.try_fold(first, |acc, s| occ::fuse(&acc, &s).map_err(err)).map(Body)
}

pub fn extrude(faces: &[Face], dir: DVec3, symmetric: bool) -> Result<Body> {
    let solids = faces.iter().map(|f| {
        let s = occ::prism(&f.0, dir.x, dir.y, dir.z).map_err(err)?;
        if symmetric { occ::translated(&s, -dir.x * 0.5, -dir.y * 0.5, -dir.z * 0.5).map_err(err) } else { Ok(s) }
    });
    union_all(solids.collect::<Result<_>>()?)
}

pub fn revolve(faces: &[Face], origin: DVec3, axis: DVec3, degrees: f64) -> Result<Body> {
    let angle = degrees.clamp(-360.0, 360.0).to_radians();
    let solids = faces.iter().map(|f| occ::revol(&f.0, &origin.to_array(), &axis.to_array(), angle).map_err(err));
    union_all(solids.collect::<Result<_>>()?)
}

/// Applies a tool body to the current bodies.
pub fn combine(bodies: &mut Vec<Body>, tool: Body, op: Op) -> Result<()> {
    match op {
        Op::New => bodies.push(tool),
        Op::Join => {
            let all = std::mem::take(bodies).into_iter().map(|b| b.0).chain([tool.0]).collect();
            bodies.push(union_all(all)?);
        }
        Op::Cut if bodies.is_empty() => return Err("Nothing to cut".into()),
        Op::Cut => {
            for b in bodies.iter_mut() {
                b.0 = occ::cut(&b.0, &tool.0).map_err(err)?;
            }
        }
    }
    Ok(())
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Tessellation {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Feature edges as a line list.
    pub edges: Vec<[f32; 3]>,
}

/// A model edge as a polyline, with a point on it that identifies it later.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeRef {
    pub points: Vec<DVec3>,
    pub probe: DVec3,
    /// Exact curve length (the polyline is only for display and picking).
    pub length: f64,
    /// Exact center and radius when the edge is a circle or arc.
    pub circle: Option<(DVec3, f64)>,
}

impl EdgeRef {
    pub fn length(&self) -> f64 {
        self.length
    }

    pub fn distance(&self, p: DVec3) -> f64 {
        self.points.windows(2).map(|w| seg_distance(p, w[0], w[1])).fold(f64::MAX, f64::min)
    }

    pub fn closed(&self) -> bool {
        self.points.len() > 2 && self.points[0].distance(*self.points.last().unwrap()) < 1e-6
    }
}

fn seg_distance(p: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-18)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// The point halfway along a polyline, so a probe never sits on a shared vertex.
fn midpoint(points: &[DVec3]) -> Option<DVec3> {
    let total: f64 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
    let mut left = total * 0.5;
    for w in points.windows(2) {
        let len = w[0].distance(w[1]);
        if left <= len {
            return Some(w[0].lerp(w[1], if len > 0.0 { left / len } else { 0.0 }));
        }
        left -= len;
    }
    points.last().copied()
}

/// Each edge of a body once, indexed as OCCT's edge map; degenerate edges are None.
fn body_edges(b: &Body) -> Vec<Option<EdgeRef>> {
    let Ok(lines) = occ::edge_lines(&b.0, 0.002) else { return vec![] };
    let mut at = 0;
    lines
        .counts
        .iter()
        .zip(&lines.info)
        .map(|(&n, info)| {
            let points: Vec<DVec3> = lines.xyz[at * 3..(at + n as usize) * 3].chunks(3).map(DVec3::from_slice).collect();
            at += n as usize;
            let probe = midpoint(&points)?;
            let circle = info.circle.then(|| (DVec3::from_array(info.center), info.radius));
            (points.len() >= 2).then_some(EdgeRef { points, probe, length: info.length, circle })
        })
        .collect()
}

pub fn edges(bodies: &[Body]) -> Vec<EdgeRef> {
    bodies.iter().flat_map(|b| body_edges(b).into_iter().flatten()).collect()
}

/// Fillets or chamfers the edges identified by probe points. A probe that no longer sits on an
/// edge (because upstream geometry changed) falls back to the nearest edge.
pub fn round(bodies: &mut [Body], probes: &[DVec3], size: f64, chamfer: bool) -> Result<()> {
    let all: Vec<Vec<Option<EdgeRef>>> = bodies.iter().map(body_edges).collect();
    let mut picked: Vec<Vec<u32>> = vec![vec![]; bodies.len()];
    for &p in probes {
        let nearest = all
            .iter()
            .enumerate()
            .flat_map(|(bi, es)| es.iter().enumerate().filter_map(move |(ei, e)| e.as_ref().map(|e| (bi, ei, e))))
            .min_by(|a, b| a.2.distance(p).total_cmp(&b.2.distance(p)));
        let Some((bi, ei, e)) = nearest else { return Err("There are no edges to round".into()) };
        if e.distance(p) > 2.0 + 0.25 * e.length() {
            return Err("An edge it rounds no longer exists".into());
        }
        if !picked[bi].contains(&(ei as u32)) {
            picked[bi].push(ei as u32);
        }
    }
    for (bi, idx) in picked.iter().enumerate().filter(|(_, idx)| !idx.is_empty()) {
        bodies[bi].0 = occ::round_edges(&bodies[bi].0, idx, size, chamfer).map_err(err)?;
    }
    Ok(())
}

pub fn tessellate(bodies: &[Body], tolerance: f64) -> Tessellation {
    let mut t = Tessellation::default();
    for b in bodies {
        let Ok(m) = occ::mesh(&b.0, tolerance) else { continue };
        let base = t.positions.len() as u32;
        t.positions.extend(m.pos.chunks(3).map(|c| [c[0], c[1], c[2]]));
        t.normals.extend(m.nrm.chunks(3).map(|c| [c[0], c[1], c[2]]));
        t.indices.extend(m.idx.iter().map(|&i| base + i));
        for e in body_edges(b).into_iter().flatten() {
            for w in e.points.windows(2) {
                t.edges.extend([w[0].as_vec3().to_array(), w[1].as_vec3().to_array()]);
            }
        }
    }
    t
}

/// Exact size and mass properties of the model.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Props {
    pub min: DVec3,
    pub max: DVec3,
    pub volume: f64,
    pub area: f64,
}

pub fn props(bodies: &[Body]) -> Option<Props> {
    let c = compound(bodies).ok()?;
    let p = occ::props(&c).ok()?;
    Some(Props { min: DVec3::from_array(p.min), max: DVec3::from_array(p.max), volume: p.volume, area: p.area })
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Hit {
    pub t: f64,
    pub point: DVec3,
    /// Present when the face is planar.
    pub plane: Option<Frame>,
}

/// The nearest face along a ray. Planar faces also give a sketch frame aligned with world axes where possible.
pub fn ray(bodies: &[Body], origin: DVec3, dir: DVec3) -> Option<Hit> {
    bodies.iter().filter_map(|b| occ::ray(&b.0, &origin.to_array(), &dir.to_array()).ok().filter(|h| h.hit)).min_by(|a, b| a.t.total_cmp(&b.t)).map(|h| {
        let (p, n) = (DVec3::from_array(h.p), DVec3::from_array(h.n));
        let plane = h.planar.then(|| {
            let axes = [DVec3::Z, DVec3::Y, DVec3::X];
            let u = axes.iter().map(|a| *a - n * a.dot(n)).max_by(|a, b| a.length().total_cmp(&b.length())).unwrap().normalize();
            Frame { o: n * p.dot(n), u, v: n.cross(u) }
        });
        Hit { t: h.t, point: p, plane }
    })
}

unsafe extern "C" {
    fn _exit(code: i32) -> !;
}

/// Ends the process without running C++ static destructors. OCCT's are order-sensitive and
/// segfault at exit once the STEP writer has been used, so the kernel process leaves this way.
pub fn exit_now(code: i32) -> ! {
    unsafe { _exit(code) }
}

fn compound(bodies: &[Body]) -> Result<Shape> {
    if bodies.is_empty() {
        return Err("Nothing to export".into());
    }
    let mut c = occ::compound();
    for b in bodies {
        occ::compound_add(c.pin_mut(), &b.0).map_err(err)?;
    }
    Ok(c)
}

/// `tolerance`: the largest allowed distance between the mesh and the true surface, in mm.
pub fn write_stl(bodies: &[Body], path: &Path, tolerance: f64) -> Result<()> {
    occ::write_stl(&*compound(bodies)?, &path.to_string_lossy(), tolerance).map_err(err)
}

pub fn write_step(bodies: &[Body], path: &Path) -> Result<()> {
    occ::write_step(&*compound(bodies)?, &path.to_string_lossy()).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadet_sketch::{Constraint, Sketch, loops, regions};

    fn plate_with_hole() -> (Vec<Loop>, Vec<Region>) {
        let mut sk = Sketch::new();
        let p = [(0.0, 0.0), (40.0, 0.0), (40.0, 30.0), (0.0, 30.0)].map(|(x, y)| sk.add_point(DVec2::new(x, y)));
        for i in 0..4 {
            sk.add_line(p[i], p[(i + 1) % 4]);
        }
        let c = sk.add_point(DVec2::new(20.0, 15.0));
        sk.add_circle(c, 5.0);
        sk.constraints.push(Constraint::Fix(c));
        let l = loops(&sk);
        let r = regions(&l).into_iter().filter(|r| r.depth == 0).collect();
        (l, r)
    }

    const XY: Frame = Frame { o: DVec3::ZERO, u: DVec3::X, v: DVec3::Y };

    #[test]
    fn extrude_cut_tessellate_export() {
        let (l, r) = plate_with_hole();
        assert_eq!(r.len(), 1);
        let t0 = std::time::Instant::now();
        let f = faces(&XY, &l, &r.iter().collect::<Vec<_>>()).unwrap();
        let mut bodies = vec![extrude(&f, DVec3::Z * 10.0, false).unwrap()];
        let tool = extrude(&f, DVec3::Z * 4.0, true).unwrap();
        combine(&mut bodies, tool, Op::Join).unwrap();
        let t = tessellate(&bodies, 0.05);
        eprintln!("plate: {} tris, {} edge segs in {:?}", t.indices.len() / 3, t.edges.len() / 2, t0.elapsed());
        assert!(t.indices.len() > 30);
        let plane = |o, d| ray(&bodies, o, d).and_then(|h| h.plane.map(|f| (h.t, f)));
        let hit = plane(DVec3::new(5.0, 5.0, 50.0), -DVec3::Z).unwrap();
        assert!((hit.1.o.z - 10.0).abs() < 1e-6, "{:?}", hit);
        assert_eq!((hit.1.u, hit.1.v), (DVec3::X, DVec3::Y));
        assert!(plane(DVec3::new(20.0, 50.0, 5.0), -DVec3::Y).is_some_and(|h| h.1.o.y.abs() > 29.0));
        let wall = ray(&bodies, DVec3::new(15.5, 15.0, 5.0), DVec3::X).unwrap();
        assert!(wall.plane.is_none() && (wall.t - 9.5).abs() < 1e-6, "{wall:?}");

        let es = edges(&bodies);
        assert_eq!(es.iter().filter(|e| e.closed()).count(), 2, "hole rims");
        assert_eq!(es.iter().filter(|e| e.points.len() == 2).count(), 13, "box edges + hole seam");
        for e in &es {
            let near = es.iter().filter(|o| o.distance(e.probe) < 1e-4).count();
            assert_eq!(near, 1, "a probe must identify exactly one edge");
        }
        let top: Vec<DVec3> = es.iter().filter(|e| e.points.len() == 2 && e.points.iter().all(|p| p.z > 9.9)).map(|e| e.probe).collect();
        assert_eq!(top.len(), 4);
        round(&mut bodies, &top, 2.0, false).unwrap();
        assert!(edges(&bodies).len() > es.len());

        let mut taller = vec![extrude(&f, DVec3::Z * 12.0, false).unwrap()];
        round(&mut taller, &top[..1], 1.0, true).unwrap();
        assert!(edges(&taller).len() > 15, "the old probe found the moved top edge");
        let dir = std::env::temp_dir();
        write_stl(&bodies, &dir.join("cadet-test.stl"), 0.01).unwrap();
        assert!(std::fs::metadata(dir.join("cadet-test.stl")).unwrap().len() > 1000);
    }

    #[test]
    fn revolve_and_cut_empty() {
        let (l, r) = plate_with_hole();
        let f = faces(&XY, &l, &r.iter().collect::<Vec<_>>()).unwrap();
        let ring = revolve(&f, DVec3::new(-10.0, 0.0, 0.0), DVec3::Y, 360.0).unwrap();
        let too_big = &mut [extrude(&f, DVec3::Z * 10.0, false).unwrap()];
        let top: Vec<DVec3> = edges(too_big).into_iter().filter(|e| e.points.iter().all(|p| p.z > 9.9)).map(|e| e.probe).collect();
        let e = round(too_big, &top, 50.0, false).unwrap_err();
        assert!(e.contains("fillet"), "{e}");
        let mut bodies = vec![];
        assert!(combine(&mut bodies, extrude(&f, DVec3::Z, false).unwrap(), Op::Cut).is_err());
        combine(&mut bodies, ring, Op::New).unwrap();
        assert!(tessellate(&bodies, 0.1).indices.len() > 100);
    }

    /// STEP export runs in a child process that leaves via `exit_now`, like the kernel worker does.
    #[test]
    fn step_export() {
        let path = std::env::temp_dir().join("cadet-test.step");
        if std::env::var_os("CADET_STEP_CHILD").is_some() {
            let (l, r) = plate_with_hole();
            let f = faces(&XY, &l, &r.iter().collect::<Vec<_>>()).unwrap();
            let ok = write_step(&[extrude(&f, DVec3::Z * 10.0, false).unwrap()], &path).is_ok();
            exit_now(if ok { 0 } else { 1 });
        }
        let _ = std::fs::remove_file(&path);
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["tests::step_export", "--exact", "--quiet"])
            .env("CADET_STEP_CHILD", "1")
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "{status:?}");
        assert!(std::fs::read_to_string(&path).unwrap().contains("ISO-10303-21"));
    }

    /// Reads a binary STL: bounding box, enclosed volume (divergence theorem) and triangle count.
    fn read_stl(path: &std::path::Path) -> (DVec3, DVec3, f64, usize) {
        let b = std::fs::read(path).unwrap();
        let n = u32::from_le_bytes(b[80..84].try_into().unwrap()) as usize;
        assert_eq!(b.len(), 84 + n * 50, "binary STL size");
        let (mut lo, mut hi, mut vol) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN), 0.0);
        for t in 0..n {
            let f = |k: usize| f32::from_le_bytes(b[84 + t * 50 + k * 4..][..4].try_into().unwrap()) as f64;
            let v = [1, 2, 3].map(|i| DVec3::new(f(i * 3), f(i * 3 + 1), f(i * 3 + 2)));
            for p in v {
                (lo, hi) = (lo.min(p), hi.max(p));
            }
            vol += v[0].dot(v[1].cross(v[2])) / 6.0;
        }
        (lo, hi, vol, n)
    }

    /// The exported STL must match the model: same size to well under printer resolution, same volume.
    #[test]
    fn stl_matches_model() {
        let dir = std::env::temp_dir();
        let (l, r) = plate_with_hole();
        let f = faces(&XY, &l, &r.iter().collect::<Vec<_>>()).unwrap();
        let mut plate = vec![extrude(&f, DVec3::Z * 10.0, false).unwrap()];
        let exact = props(&plate).unwrap();
        let analytic = 40.0 * 30.0 * 10.0 - std::f64::consts::PI * 25.0 * 10.0;
        assert!((exact.volume - analytic).abs() < 1e-6, "{} vs {analytic}", exact.volume);
        assert!(exact.min.distance(DVec3::ZERO) < 1e-6 && exact.max.distance(DVec3::new(40.0, 30.0, 10.0)) < 1e-6, "{exact:?}");

        let hole = edges(&plate).into_iter().find(|e| e.circle.is_some()).unwrap();
        let (c, radius) = hole.circle.unwrap();
        assert!((radius - 5.0).abs() < 1e-9 && c.truncate().distance(glam::DVec2::new(20.0, 15.0)) < 1e-9, "{c:?} {radius}");
        assert!((hole.length() - std::f64::consts::TAU * 5.0).abs() < 1e-9, "exact arc length {}", hole.length());

        let path = dir.join("cadet-accuracy-plate.stl");
        write_stl(&plate, &path, 0.01).unwrap();
        let (lo, hi, vol, n) = read_stl(&path);
        eprintln!(
            "plate STL: {n} tris, {} bytes, size {:?}, volume {vol:.4} vs exact {:.4} ({:+.4}%)",
            84 + n * 50,
            hi - lo,
            exact.volume,
            (vol / exact.volume - 1.0) * 100.0
        );
        assert!(lo.distance(DVec3::ZERO) < 1e-4 && hi.distance(DVec3::new(40.0, 30.0, 10.0)) < 1e-4, "STL box {lo:?} {hi:?}");
        assert!((vol / exact.volume - 1.0).abs() < 1e-4, "STL volume {vol} vs {}", exact.volume);

        let top: Vec<DVec3> = edges(&plate).into_iter().filter(|e| e.points.len() == 2 && e.points.iter().all(|p| p.z > 9.9)).map(|e| e.probe).collect();
        round(&mut plate, &top, 2.0, false).unwrap();
        let exact = props(&plate).unwrap();
        write_stl(&plate, &path, 0.01).unwrap();
        let (lo, hi, vol, n) = read_stl(&path);
        eprintln!("filleted STL: {n} tris, size {:?}, volume {vol:.4} vs exact {:.4} ({:+.4}%)", hi - lo, exact.volume, (vol / exact.volume - 1.0) * 100.0);
        assert!(lo.distance(exact.min) < 1e-4 && hi.distance(exact.max) < 1e-4, "filleted STL box {lo:?} {hi:?} vs {exact:?}");
        assert!((vol / exact.volume - 1.0).abs() < 1e-3, "filleted STL volume {vol} vs {}", exact.volume);

        let mut sk = cadet_sketch::Sketch::new();
        let c = sk.add_point(DVec2::ZERO);
        sk.add_circle(c, 10.0);
        let ls = cadet_sketch::loops(&sk);
        let rs = cadet_sketch::regions(&ls);
        let disc = vec![extrude(&faces(&XY, &ls, &rs.iter().collect::<Vec<_>>()).unwrap(), DVec3::Z * 5.0, false).unwrap()];
        let exact = props(&disc).unwrap();
        assert!((exact.max.x - 10.0).abs() < 1e-6 && (exact.min.y + 10.0).abs() < 1e-6, "exact cylinder box {exact:?}");
        write_stl(&disc, &path, 0.01).unwrap();
        let (lo, hi, vol, n) = read_stl(&path);
        let analytic = std::f64::consts::PI * 100.0 * 5.0;
        eprintln!("cylinder Ø20 STL: {n} tris, size {:?}, volume {vol:.4} vs exact {analytic:.4} ({:+.4}%)", hi - lo, (vol / analytic - 1.0) * 100.0);
        assert!((hi.x - 10.0).abs() < 0.01 && (lo.y + 10.0).abs() < 0.01, "STL cylinder within the 0.01 mm tolerance: {lo:?} {hi:?}");
        assert!((vol / analytic - 1.0).abs() < 1e-3, "cylinder volume {vol} vs {analytic}");
    }
}
