use crate::sketch::{self, View};
use crate::theme::{self, Palette};
use cadet_core::kernel::EdgeRef;
use cadet_core::{Axis, Document, Kind, Op, Plane, chosen_regions};
use cadet_sketch::{self as cs, Sketch};
use egui::{Align2, FontId, Painter, PointerButton, Pos2, Response, RichText, Shape, Stroke};
use glam::{DVec2, DVec3};

pub enum Outcome {
    Ok,
    Cancel,
}

/// An extrude or revolve being created or edited, with a live preview.
pub struct FeatureEdit {
    pub index: Option<usize>,
    pub kind: Kind,
    picking_axis: bool,
    pub changed: bool,
    /// Switch between Join and Cut by drag direction until the user picks an operation.
    auto_op: bool,
    /// Distance offset between the grab point and the handle tip while dragging.
    drag: Option<f64>,
}

fn sketch_of<'a>(doc: &'a Document, kind: &Kind) -> Option<(&'a Plane, &'a Sketch)> {
    doc.sketch(kind.sketch_ref()?)
}

/// A point inside a region (not in its holes), for referencing it robustly.
fn interior(loops: &[cs::Loop], r: &cs::Region) -> Option<DVec2> {
    let poly = &loops[r.outer].poly;
    cs::triangulate(poly).iter().map(|t| (poly[t[0]] + poly[t[1]] + poly[t[2]]) / 3.0).find(|&p| r.contains(loops, p))
}

impl FeatureEdit {
    pub fn new(doc: &Document, revolve: bool) -> Option<Self> {
        let sketch = doc.features.iter().rev().find_map(|f| match &f.kind {
            Kind::Sketch { sketch, .. } if !cs::loops(sketch).is_empty() => Some(f.id),
            _ => None,
        })?;
        let on_face = doc.sketch(sketch).is_some_and(|(p, _)| p.basis == cadet_core::Basis::Face);
        let op = if on_face { Op::Join } else { Op::New };
        let kind = if revolve {
            Kind::Revolve { sketch, probes: vec![], axis: Axis::Y, angle: 360.0, op }
        } else {
            Kind::Extrude { sketch, probes: vec![], distance: 10.0, symmetric: false, op }
        };
        Some(Self { index: None, kind, picking_axis: false, changed: true, auto_op: on_face, drag: None })
    }

    pub fn round(chamfer: bool) -> Self {
        Self { index: None, kind: Kind::Round { edges: vec![], size: if chamfer { 1.0 } else { 2.0 }, chamfer }, picking_axis: false, changed: true, auto_op: false, drag: None }
    }

    pub fn is_round(&self) -> bool {
        matches!(self.kind, Kind::Round { .. })
    }

    /// Where the feature sits in the timeline.
    pub fn position(&self, doc: &Document) -> usize {
        self.index.unwrap_or(doc.features.len())
    }

    pub fn edit(doc: &Document, index: usize) -> Self {
        Self { index: Some(index), kind: doc.features[index].kind.clone(), picking_axis: false, changed: true, auto_op: false, drag: None }
    }

    /// The document as it would be with this feature applied.
    pub fn preview(&self, doc: &Document) -> Document {
        let mut d = doc.clone();
        match self.index {
            Some(i) => d.features[i].kind = self.kind.clone(),
            None => {
                d.add(self.kind.clone());
            }
        }
        d
    }

    pub fn commit(self, doc: &mut Document) {
        match self.index {
            Some(i) => doc.features[i].kind = self.kind,
            None => {
                let sketch = self.kind.sketch_ref();
                doc.add(self.kind);
                if let Some(i) = sketch.and_then(|id| doc.index_of(id)) {
                    doc.features[i].visible = false;
                }
            }
        }
    }

    fn probes(&mut self) -> &mut Vec<DVec2> {
        match &mut self.kind {
            Kind::Extrude { probes, .. } | Kind::Revolve { probes, .. } => probes,
            Kind::Sketch { .. } | Kind::Round { .. } => unreachable!(),
        }
    }

    /// Toggles a region or edge under the cursor, or picks the revolve axis.
    pub fn click(&mut self, doc: &Document, view: &View, pos: Pos2, edges: &[EdgeRef]) {
        if let Kind::Round { edges: picked, .. } = &mut self.kind {
            if let Some(e) = hit_edge(edges, view, pos) {
                match picked.iter().position(|&p| e.distance(p) < 1e-4) {
                    Some(i) => {
                        picked.remove(i);
                    }
                    None => picked.push(e.probe),
                }
                self.changed = true;
            }
            return;
        }
        if self.picking_axis {
            if let Some((plane, sk)) = sketch_of(doc, &self.kind)
                && let Some(e) = sketch::hit_line(sk, &View { plane: *plane, ..*view }, pos) {
                    if let Kind::Revolve { axis, .. } = &mut self.kind {
                        *axis = Axis::Line(e);
                    }
                    self.picking_axis = false;
                    self.changed = true;
                }
            return;
        }
        let candidates = doc.features.iter().filter_map(|f| match &f.kind {
            Kind::Sketch { plane, sketch } if f.visible || Some(f.id) == self.kind.sketch_ref() => Some((f.id, plane, sketch)),
            _ => None,
        });
        for (id, plane, sk) in candidates {
            let Some(q) = (View { plane: *plane, ..*view }).to_sketch(pos) else { continue };
            let loops = cs::loops(sk);
            let regions = cs::regions(&loops);
            let Some(r) = regions.iter().find(|r| r.contains(&loops, q)) else { continue };
            if Some(id) != self.kind.sketch_ref() {
                match &mut self.kind {
                    Kind::Extrude { sketch, probes, .. } | Kind::Revolve { sketch, probes, .. } => {
                        *sketch = id;
                        *probes = vec![q];
                    }
                    Kind::Sketch { .. } | Kind::Round { .. } => {}
                }
            } else {
                let all: Vec<DVec2> = chosen_regions(&loops, &regions, &[]).into_iter().filter_map(|r| interior(&loops, r)).collect();
                let probes = self.probes();
                if probes.is_empty() {
                    *probes = all;
                }
                if let Some(i) = probes.iter().position(|&p| r.contains(&loops, p)) {
                    probes.remove(i);
                } else {
                    probes.push(q);
                }
                if probes.is_empty() {
                    probes.push(q);
                }
            }
            self.changed = true;
            return;
        }
    }

    /// The drag handle for the feature being edited, if it has one.
    fn handle(&self, doc: &Document, edges: &[EdgeRef], center: DVec3) -> Option<Handle> {
        match &self.kind {
            Kind::Extrude { probes, distance, symmetric, .. } => {
                let (plane, c) = profile_center(doc, &self.kind, probes)?;
                Some(Handle::Line { base: plane.world(c), dir: plane.normal(), len: if *symmetric { distance * 0.5 } else { *distance }, scale: 1.0 })
            }
            Kind::Revolve { probes, axis, angle, .. } => {
                let (plane, c) = profile_center(doc, &self.kind, probes)?;
                let (o, d) = axis_line(doc, &self.kind, *axis)?;
                let from = plane.world(c);
                let on_axis = o + d * (from - o).dot(d);
                ((from - on_axis).length() > 1e-6).then_some(Handle::Arc { center: on_axis, axis: d, from, degrees: *angle })
            }
            Kind::Round { edges: picked, size, .. } => {
                let e = picked.first().and_then(|&q| edges.iter().find(|e| e.distance(q) < 1e-4))?;
                let tangent = (*e.points.last()? - e.points[0]).normalize_or_zero();
                let out = e.probe - center;
                let dir = (out - tangent * out.dot(tangent)).normalize_or(DVec3::Z);
                Some(Handle::Line { base: e.probe, dir, len: *size, scale: 4.0 })
            }
            Kind::Sketch { .. } => None,
        }
    }

    fn grip(h: &Handle) -> DVec3 {
        match *h {
            Handle::Line { base, dir, len, scale } => base + dir * len * scale,
            Handle::Arc { center, axis, from, degrees } => center + rotate(from - center, axis, degrees.to_radians()),
        }
    }

    fn handle_hot(&self, view: &View, h: &Handle, pos: Option<Pos2>) -> bool {
        let tip = view.screen(Self::grip(h).as_vec3());
        self.drag.is_some() || pos.is_some_and(|p| p.distance(tip) < 14.0)
    }

    /// Drags the feature's handle. Returns true while the pointer belongs to the handle.
    pub fn drag_handle(&mut self, ui: &egui::Ui, resp: &Response, view: &View, doc: &Document, edges: &[EdgeRef], center: DVec3) -> bool {
        let Some(h) = self.handle(doc, edges, center) else { return false };
        let pos = resp.hover_pos().or(resp.interact_pointer_pos());
        if self.handle_hot(view, &h, pos) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let value = |p: Pos2| {
            let (o, d) = view.ray(p);
            let (o, d) = (o.as_dvec3(), d.as_dvec3());
            match h {
                Handle::Line { base, dir, scale, .. } => closest_on_axis(base, dir, o, d).map(|t| t / scale),
                Handle::Arc { center, axis, from, .. } => {
                    let den = d.dot(axis);
                    (den.abs() > 1e-6).then(|| {
                        let q = o + d * ((center - o).dot(axis) / den) - center;
                        let u = (from - center).normalize();
                        let w = axis.cross(u);
                        q.dot(w).atan2(q.dot(u)).to_degrees().rem_euclid(360.0)
                    })
                }
            }
        };
        let current = match h {
            Handle::Line { len, .. } => len,
            Handle::Arc { degrees, .. } => degrees,
        };
        let pressed_at = ui.input(|i| i.pointer.press_origin());
        if resp.drag_started_by(PointerButton::Primary) && self.handle_hot(view, &h, pressed_at) {
            let offset = match h {
                Handle::Line { .. } => pressed_at.and_then(value).map(|t| current - t),
                Handle::Arc { .. } => Some(0.0),
            };
            self.drag = offset;
        }
        let Some(offset) = self.drag else { return false };
        if let Some(t) = pos.filter(|_| resp.dragged_by(PointerButton::Primary)).and_then(value) {
            let v = t + offset;
            match &mut self.kind {
                Kind::Extrude { distance, symmetric, op, .. } => {
                    let mut d = (v * if *symmetric { 2.0 } else { 1.0 } * 100.0).round() / 100.0;
                    if d.abs() < 0.01 {
                        d = 0.01_f64.copysign(*distance);
                    }
                    if d != *distance {
                        *distance = d;
                        if self.auto_op {
                            *op = if d < 0.0 { Op::Cut } else { Op::Join };
                        }
                        self.changed = true;
                    }
                }
                Kind::Revolve { angle, .. } => {
                    let a = if v.round() < 1.0 { 360.0 } else { v.round() };
                    if a != *angle {
                        *angle = a;
                        self.changed = true;
                    }
                }
                Kind::Round { size, .. } => {
                    let r = ((v * 100.0).round() / 100.0).max(0.01);
                    if r != *size {
                        *size = r;
                        self.changed = true;
                    }
                }
                Kind::Sketch { .. } => {}
            }
        }
        if !resp.dragged() {
            self.drag = None;
        }
        true
    }

    fn draw_handle(&self, painter: &Painter, view: &View, doc: &Document, p: &Palette, edges: &[EdgeRef], center: DVec3) {
        let Some(h) = self.handle(doc, edges, center) else { return };
        let s = |q: DVec3| view.screen(q.as_vec3());
        let tip = s(Self::grip(&h));
        let hot = self.handle_hot(view, &h, painter.ctx().pointer_hover_pos());
        let stroke = Stroke::new(2.0, p.accent);
        match h {
            Handle::Line { base, .. } => {
                let b = s(base);
                let along = (tip - b).normalized();
                painter.line_segment([b, tip], stroke);
                let side = along.rot90() * 6.0;
                painter.add(Shape::convex_polygon(vec![tip + along * 12.0, tip + side, tip - side], p.accent, Stroke::NONE));
            }
            Handle::Arc { center, axis, from, degrees } => {
                let pts = (0..=48).map(|i| s(center + rotate(from - center, axis, (degrees * i as f64 / 48.0).to_radians()))).collect();
                painter.add(Shape::line(pts, stroke));
                painter.add(Shape::dashed_line(&[s(center - axis * 1000.0), s(center + axis * 1000.0)], Stroke::new(1.0, p.accent.gamma_multiply(0.5)), 8.0, 5.0));
            }
        }
        painter.circle(tip, if hot { 7.0 } else { 5.5 }, if hot { p.accent } else { p.card }, stroke);
        let label = match &self.kind {
            Kind::Extrude { distance, op, .. } => format!("{} mm{}", sketch::fmt_len(*distance), if *op == Op::Cut { " · cut" } else { "" }),
            Kind::Revolve { angle, .. } => format!("{}°", sketch::fmt_len(*angle)),
            Kind::Round { size, chamfer, .. } => format!("{} {} mm", if *chamfer { "Distance" } else { "R" }, sketch::fmt_len(*size)),
            Kind::Sketch { .. } => String::new(),
        };
        painter.text(tip + egui::vec2(14.0, -14.0), Align2::LEFT_BOTTOM, label, FontId::proportional(13.0), p.text);
    }

    pub fn draw(&self, painter: &Painter, view: &View, doc: &Document, p: &Palette, edges: &[EdgeRef], center: DVec3) {
        self.draw_handle(painter, view, doc, p, edges, center);
        if let Kind::Round { edges: picked, .. } = &self.kind {
            let hover = painter.ctx().pointer_hover_pos().and_then(|pos| hit_edge(edges, view, pos));
            for e in edges {
                let on = picked.iter().any(|&q| e.distance(q) < 1e-4);
                let hot = hover.is_some_and(|h| std::ptr::eq(h, e));
                if on || hot {
                    let pts = e.points.iter().map(|q| view.screen(q.as_vec3())).collect();
                    painter.add(egui::Shape::line(pts, Stroke::new(if on { 3.0 } else { 2.0 }, p.accent.gamma_multiply(if on { 1.0 } else { 0.6 }))));
                }
            }
            return;
        }
        let Some((plane, sk)) = sketch_of(doc, &self.kind) else { return };
        let view = View { plane: *plane, ..*view };
        let loops = cs::loops(sk);
        let regions = cs::regions(&loops);
        let probes = match &self.kind {
            Kind::Extrude { probes, .. } | Kind::Revolve { probes, .. } => probes.as_slice(),
            Kind::Sketch { .. } | Kind::Round { .. } => &[],
        };
        let chosen = chosen_regions(&loops, &regions, probes);
        for r in &regions {
            let on = chosen.iter().any(|c| std::ptr::eq(*c, r));
            sketch::fill_poly(painter, &view, &loops[r.outer].poly, p.accent.gamma_multiply(if on { 0.35 } else { 0.08 }));
        }
        for (_, e) in sk.live() {
            let pts: Vec<Pos2> = cs::polyline(sk, e).into_iter().map(|q| view.to_screen(q)).collect();
            painter.add(egui::Shape::line(pts, Stroke::new(1.2, p.weak)));
        }
        if let Kind::Revolve { axis: Axis::Line(e), .. } = self.kind
            && let Some(cs::Entity::Line { a, b }) = sk.entity(e) {
                painter.add(egui::Shape::dashed_line(&[view.to_screen(sk.pt(a)), view.to_screen(sk.pt(b))], Stroke::new(2.0, p.accent), 8.0, 5.0));
            }
    }

    pub fn panel(&mut self, ui: &mut egui::Ui, doc: &Document, error: Option<&str>) -> Option<Outcome> {
        let p = theme::palette(ui.ctx());
        let title = match self.index {
            Some(i) => format!("Edit {}", doc.features[i].name),
            None => self.kind.label().to_string(),
        };
        ui.set_width(230.0);
        ui.label(RichText::new(title).heading());
        if let Kind::Round { edges, size, chamfer } = &mut self.kind {
            let n = edges.len();
            ui.label(RichText::new(format!("{n} edge{} · click edges to toggle", if n == 1 { "" } else { "s" })).color(p.weak).small());
            ui.add_space(4.0);
            let before = (*size, *chamfer);
            egui::Grid::new("round_grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Type");
                ui.horizontal(|ui| {
                    ui.selectable_value(chamfer, false, "Fillet");
                    ui.selectable_value(chamfer, true, "Chamfer");
                });
                ui.end_row();
                ui.label(if *chamfer { "Distance" } else { "Radius" });
                ui.add(egui::DragValue::new(size).speed(0.05).range(0.01..=1000.0).suffix(" mm").max_decimals(3));
                ui.end_row();
            });
            if before != (*size, *chamfer) {
                self.changed = true;
            }
            return buttons(ui, p, n > 0, error);
        }
        let count = match (&self.kind, sketch_of(doc, &self.kind)) {
            (Kind::Extrude { probes, .. } | Kind::Revolve { probes, .. }, Some((_, sk))) => {
                let loops = cs::loops(sk);
                chosen_regions(&loops, &cs::regions(&loops), probes).len()
            }
            _ => 0,
        };
        ui.label(RichText::new(format!("{count} profile{} · click regions to toggle", if count == 1 { "" } else { "s" })).color(p.weak).small());
        ui.add_space(4.0);
        let before = format!("{:?}", self.kind);
        let mut picking = self.picking_axis;
        let mut picked_op = false;
        egui::Grid::new("feature_grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| match &mut self.kind {
            Kind::Extrude { distance, symmetric, op, .. } => {
                ui.label("Distance");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(distance).speed(0.2).suffix(" mm").max_decimals(3));
                    if ui.small_button("Flip").clicked() {
                        *distance = -*distance;
                    }
                });
                ui.end_row();
                ui.label("");
                ui.checkbox(symmetric, "Symmetric");
                ui.end_row();
                picked_op |= op_row(ui, op);
            }
            Kind::Revolve { axis, angle, op, .. } => {
                ui.label("Axis");
                ui.horizontal(|ui| {
                    for (a, name) in [(Axis::X, "X"), (Axis::Y, "Y"), (Axis::Z, "Z")] {
                        ui.selectable_value(axis, a, name);
                    }
                    let line = matches!(axis, Axis::Line(_));
                    if ui.selectable_label(picking || line, if line { "Line" } else { "Pick line" }).on_hover_text("Click a line in the sketch").clicked() {
                        picking = !picking;
                    }
                });
                ui.end_row();
                ui.label("Angle");
                ui.add(egui::DragValue::new(angle).speed(1.0).range(1.0..=360.0).suffix("°"));
                ui.end_row();
                picked_op |= op_row(ui, op);
            }
            Kind::Sketch { .. } | Kind::Round { .. } => {}
        });
        self.picking_axis = picking;
        if picked_op {
            self.auto_op = false;
        }
        if format!("{:?}", self.kind) != before {
            self.changed = true;
        }
        buttons(ui, p, count > 0, error)
    }
}

fn buttons(ui: &mut egui::Ui, p: &Palette, ready: bool, error: Option<&str>) -> Option<Outcome> {
    if let Some(e) = error {
        ui.add_space(4.0);
        ui.label(RichText::new(e).color(p.error()).small());
    }
    ui.add_space(6.0);
    let mut out = None;
    ui.horizontal(|ui| {
        if ui.button("Cancel").clicked() {
            out = Some(Outcome::Cancel);
        }
        let ok = egui::Button::new(RichText::new("  OK  ").color(p.on_accent)).fill(p.accent);
        if ui.add_enabled(ready, ok).clicked() {
            out = Some(Outcome::Ok);
        }
    });
    out
}

/// The model edge nearest the cursor on screen.
pub fn hit_edge<'a>(edges: &'a [EdgeRef], view: &View, pos: Pos2) -> Option<&'a EdgeRef> {
    edges
        .iter()
        .map(|e| {
            let pts: Vec<Pos2> = e.points.iter().map(|q| view.screen(q.as_vec3())).collect();
            (pts.windows(2).map(|w| sketch::seg_dist(pos, w[0], w[1])).fold(f32::MAX, f32::min), e)
        })
        .filter(|d| d.0 < 7.0)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|d| d.1)
}

/// Returns true if the user picked an operation.
fn op_row(ui: &mut egui::Ui, op: &mut Op) -> bool {
    ui.label("Operation");
    let picked = ui
        .horizontal(|ui| {
            let a = ui.selectable_value(op, Op::New, "New body").clicked();
            let b = ui.selectable_value(op, Op::Join, "Join").clicked();
            let c = ui.selectable_value(op, Op::Cut, "Cut").clicked();
            a | b | c
        })
        .inner;
    ui.end_row();
    picked
}

enum Handle {
    /// An arrow from `base` along `dir`, drawn `scale` times `len` long.
    Line { base: DVec3, dir: DVec3, len: f64, scale: f64 },
    /// An arc around `axis` through `center`, from the point `from`, `degrees` long.
    Arc { center: DVec3, axis: DVec3, from: DVec3, degrees: f64 },
}

/// The chosen profiles' plane and their combined centroid.
fn profile_center<'a>(doc: &'a Document, kind: &Kind, probes: &[DVec2]) -> Option<(&'a Plane, DVec2)> {
    let (plane, sk) = sketch_of(doc, kind)?;
    let loops = cs::loops(sk);
    let regions = cs::regions(&loops);
    let chosen = chosen_regions(&loops, &regions, probes);
    (!chosen.is_empty()).then(|| (plane, chosen.iter().map(|r| centroid(&loops[r.outer].poly)).sum::<DVec2>() / chosen.len() as f64))
}

/// A revolve axis in world space: a point and a unit direction.
fn axis_line(doc: &Document, kind: &Kind, axis: Axis) -> Option<(DVec3, DVec3)> {
    Some(match axis {
        Axis::X => (DVec3::ZERO, DVec3::X),
        Axis::Y => (DVec3::ZERO, DVec3::Y),
        Axis::Z => (DVec3::ZERO, DVec3::Z),
        Axis::Line(e) => {
            let (plane, sk) = sketch_of(doc, kind)?;
            let Some(cs::Entity::Line { a, b }) = sk.entity(e) else { return None };
            let (a, b) = (plane.world(sk.pt(a)), plane.world(sk.pt(b)));
            (a, (b - a).normalize_or_zero())
        }
    })
}

/// Rotates v around a unit axis (Rodrigues).
fn rotate(v: DVec3, k: DVec3, angle: f64) -> DVec3 {
    let (s, c) = angle.sin_cos();
    v * c + k.cross(v) * s + k * k.dot(v) * (1.0 - c)
}

/// Area-weighted centroid of a simple polygon.
fn centroid(poly: &[DVec2]) -> DVec2 {
    let (mut a, mut c) = (0.0, DVec2::ZERO);
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        let w = p.perp_dot(q);
        a += w;
        c += (p + q) * w;
    }
    if a.abs() < 1e-12 { poly.iter().copied().sum::<DVec2>() / poly.len().max(1) as f64 } else { c / (3.0 * a) }
}

/// The parameter t along `base + t·dir` closest to a ray.
fn closest_on_axis(base: DVec3, dir: DVec3, origin: DVec3, ray: DVec3) -> Option<f64> {
    let w = origin - base;
    let (a, b, c) = (ray.dot(ray), ray.dot(dir), dir.dot(dir));
    let den = a * c - b * b;
    (den.abs() > 1e-9).then(|| (a * dir.dot(w) - b * ray.dot(w)) / den)
}
