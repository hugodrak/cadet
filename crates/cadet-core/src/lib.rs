//! The document: a timeline of features, evaluated into bodies.

use cadet_kernel::{self as k, Body, Frame, Tessellation};
use cadet_sketch::{self as cs, E, Entity, Sketch};
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub use cadet_kernel::{self as kernel, Op};

pub mod worker;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Basis {
    XY,
    XZ,
    YZ,
    Face,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    pub basis: Basis,
    pub o: DVec3,
    pub u: DVec3,
    pub v: DVec3,
}

impl Plane {
    pub const XY: Plane = Plane { basis: Basis::XY, o: DVec3::ZERO, u: DVec3::X, v: DVec3::Y };
    pub const XZ: Plane = Plane { basis: Basis::XZ, o: DVec3::ZERO, u: DVec3::X, v: DVec3::Z };
    pub const YZ: Plane = Plane { basis: Basis::YZ, o: DVec3::ZERO, u: DVec3::Y, v: DVec3::Z };
    pub const STANDARD: [Plane; 3] = [Self::XY, Self::XZ, Self::YZ];

    pub fn from_frame(f: Frame) -> Self {
        Plane { basis: Basis::Face, o: f.o, u: f.u, v: f.v }
    }

    pub fn frame(&self) -> Frame {
        Frame { o: self.o, u: self.u, v: self.v }
    }

    pub fn name(&self) -> &'static str {
        match self.basis {
            Basis::XY => "XY",
            Basis::XZ => "XZ",
            Basis::YZ => "YZ",
            Basis::Face => "Face",
        }
    }

    pub fn normal(&self) -> DVec3 {
        self.u.cross(self.v)
    }

    pub fn world(&self, q: DVec2) -> DVec3 {
        self.o + self.u * q.x + self.v * q.y
    }

    /// Ray intersection in sketch coordinates, with the ray parameter.
    pub fn hit(&self, origin: DVec3, dir: DVec3) -> Option<(f64, DVec2)> {
        let n = self.normal();
        let den = dir.dot(n);
        let t = (self.o - origin).dot(n) / den;
        (den.abs() > 1e-9 && t > 0.0).then(|| {
            let d = origin + dir * t - self.o;
            (t, DVec2::new(d.dot(self.u), d.dot(self.v)))
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Axis {
    X,
    Y,
    Z,
    Line(E),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Kind {
    Sketch { plane: Plane, sketch: Sketch },
    /// `probes` are points inside the chosen regions; empty means every outer region.
    Extrude { sketch: u32, probes: Vec<DVec2>, distance: f64, symmetric: bool, op: Op },
    Revolve { sketch: u32, probes: Vec<DVec2>, axis: Axis, angle: f64, op: Op },
    /// Fillet (or chamfer) of the edges passing through `edges`.
    Round { edges: Vec<DVec3>, size: f64, chamfer: bool },
}

impl Kind {
    pub fn label(&self) -> &'static str {
        match self {
            Kind::Sketch { .. } => "Sketch",
            Kind::Extrude { .. } => "Extrude",
            Kind::Revolve { .. } => "Revolve",
            Kind::Round { chamfer: false, .. } => "Fillet",
            Kind::Round { chamfer: true, .. } => "Chamfer",
        }
    }

    pub fn sketch_ref(&self) -> Option<u32> {
        match self {
            Kind::Extrude { sketch, .. } | Kind::Revolve { sketch, .. } => Some(*sketch),
            Kind::Sketch { .. } | Kind::Round { .. } => None,
        }
    }

    /// Changes whenever the feature's definition does.
    pub fn fingerprint(&self) -> String {
        format!("{self:?}")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Feature {
    pub id: u32,
    pub name: String,
    pub visible: bool,
    pub kind: Kind,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Document {
    pub features: Vec<Feature>,
    next_id: u32,
}

#[derive(Serialize, Deserialize)]
struct FileFormat {
    format: String,
    version: u32,
    units: String,
    #[serde(flatten)]
    doc: Document,
}

impl Document {
    pub fn add(&mut self, kind: Kind) -> usize {
        let n = self.features.iter().filter(|f| f.kind.label() == kind.label()).count() + 1;
        self.next_id += 1;
        self.features.push(Feature { id: self.next_id, name: format!("{}{n}", kind.label()), visible: true, kind });
        self.features.len() - 1
    }

    pub fn index_of(&self, id: u32) -> Option<usize> {
        self.features.iter().position(|f| f.id == id)
    }

    pub fn sketch(&self, id: u32) -> Option<(&Plane, &Sketch)> {
        match &self.features[self.index_of(id)?].kind {
            Kind::Sketch { plane, sketch } => Some((plane, sketch)),
            _ => None,
        }
    }

    /// Removes a feature and everything that depends on it. Returns how many were removed.
    pub fn remove(&mut self, i: usize) -> usize {
        let id = self.features[i].id;
        let before = self.features.len();
        self.features.retain(|f| f.id != id && f.kind.sketch_ref() != Some(id));
        before - self.features.len()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let file = FileFormat { format: "cadet".into(), version: 1, units: "mm".into(), doc: self.clone() };
        std::fs::write(path, serde_json::to_string_pretty(&file)?)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let file: FileFormat = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        if file.format != "cadet" || file.version > 1 {
            return Err(format!("Unsupported file (format '{}' v{})", file.format, file.version));
        }
        Ok(file.doc)
    }
}

/// Regions of a sketch picked by probe points; without probes, every outer (even-depth) region.
pub fn chosen_regions<'a>(loops: &[cs::Loop], regions: &'a [cs::Region], probes: &[DVec2]) -> Vec<&'a cs::Region> {
    regions.iter().filter(|r| if probes.is_empty() { r.depth % 2 == 0 } else { probes.iter().any(|&p| r.contains(loops, p)) }).collect()
}

#[derive(Default)]
pub struct Model {
    pub bodies: Vec<Body>,
    pub mesh: Tessellation,
    pub errors: Vec<(u32, String)>,
    pub millis: f64,
}

/// Evaluates the first `upto` features.
pub fn evaluate(doc: &Document, upto: usize) -> Model {
    evaluate_with(doc, upto, &[], |_, _| {})
}

/// Evaluates, skipping features by id. `progress` sees each feature index and the bodies before it runs.
pub fn evaluate_with(doc: &Document, upto: usize, skip: &[u32], mut progress: impl FnMut(usize, &[Body])) -> Model {
    let t0 = std::time::Instant::now();
    let mut m = Model::default();
    for (i, f) in doc.features.iter().enumerate().take(upto) {
        if skip.contains(&f.id) {
            m.errors.push((f.id, "This feature crashed the geometry kernel, so it was skipped".into()));
            continue;
        }
        progress(i, &m.bodies);
        if let Err(e) = apply(doc, i, f, &mut m.bodies) {
            m.errors.push((f.id, e));
        }
    }
    m.mesh = k::tessellate(&m.bodies, 0.02);
    m.millis = t0.elapsed().as_secs_f64() * 1000.0;
    m
}

fn apply(doc: &Document, i: usize, f: &Feature, bodies: &mut Vec<Body>) -> Result<(), String> {
    let (sketch_id, probes, op) = match &f.kind {
        Kind::Sketch { .. } => return Ok(()),
        Kind::Round { edges, size, chamfer } => {
            if *size <= 0.0 {
                return Err("Size must be positive".into());
            }
            return k::round(bodies, edges, *size, *chamfer);
        }
        Kind::Extrude { sketch, probes, op, .. } | Kind::Revolve { sketch, probes, op, .. } => (*sketch, probes, *op),
    };
    let (plane, sk) = doc.sketch(sketch_id).filter(|_| doc.index_of(sketch_id) < Some(i)).ok_or("Its sketch is missing")?;
    let loops = cs::loops(sk);
    let regions = cs::regions(&loops);
    let chosen = chosen_regions(&loops, &regions, probes);
    if chosen.is_empty() {
        return Err("No closed profile selected".into());
    }
    let faces = k::faces(&plane.frame(), &loops, &chosen)?;
    let tool = match f.kind {
        Kind::Extrude { distance, symmetric, .. } => k::extrude(&faces, plane.normal() * distance, symmetric),
        Kind::Revolve { axis, angle, .. } => {
            let (o, d) = match axis {
                Axis::X => (DVec3::ZERO, DVec3::X),
                Axis::Y => (DVec3::ZERO, DVec3::Y),
                Axis::Z => (DVec3::ZERO, DVec3::Z),
                Axis::Line(e) => match sk.entity(e) {
                    Some(Entity::Line { a, b }) => (plane.world(sk.pt(a)), plane.world(sk.pt(b)) - plane.world(sk.pt(a))),
                    _ => return Err("The revolve axis line was deleted".into()),
                },
            };
            k::revolve(&faces, o, d.normalize(), angle)
        }
        Kind::Sketch { .. } | Kind::Round { .. } => unreachable!(),
    }?;
    k::combine(bodies, tool, op)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_roundtrip() {
        let mut doc = Document::default();
        let mut sk = Sketch::new();
        let p = [(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (0.0, 10.0)].map(|(x, y)| sk.add_point(DVec2::new(x, y)));
        for i in 0..4 {
            sk.add_line(p[i], p[(i + 1) % 4]);
        }
        let s = doc.add(Kind::Sketch { plane: Plane::XY, sketch: sk });
        let sid = doc.features[s].id;
        doc.add(Kind::Extrude { sketch: sid, probes: vec![], distance: 5.0, symmetric: false, op: Op::New });
        doc.add(Kind::Revolve { sketch: sid, probes: vec![DVec2::new(1.0, 1.0)], axis: Axis::Y, angle: 90.0, op: Op::Join });
        doc.add(Kind::Round { edges: vec![DVec3::new(10.0, 0.0, 5.0)], size: 1.0, chamfer: false });
        let m = evaluate(&doc, doc.features.len());
        assert!(m.errors.is_empty(), "{:?}", m.errors);
        assert_eq!(m.bodies.len(), 1);
        eprintln!("4 features in {:.1} ms", m.millis);

        let path = std::env::temp_dir().join("cadet-roundtrip.cadet");
        doc.save(&path).unwrap();
        let back = Document::load(&path).unwrap();
        assert_eq!(back.features.len(), 4);
        assert_eq!(back.features[3].name, "Fillet1");
        assert_eq!(evaluate(&back, 4).mesh.indices.len(), m.mesh.indices.len());

        let mut doc = back;
        assert_eq!(doc.remove(0), 3);
    }
}
