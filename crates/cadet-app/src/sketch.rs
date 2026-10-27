use crate::commands::Action;
use crate::theme::{self, Palette};
use crate::icons;
use crate::snap::{self, Marker};
use cadet_core::Plane;
use cadet_render::Camera;
use cadet_sketch::{self as cs, Constraint, E, Entity, P, Sketch};
use egui::{Align2, Color32, FontId, Painter, PointerButton, Pos2, Rect, Response, Shape, Stroke, Vec2, vec2};
use glam::{DVec2, Vec3};
use std::f32::consts::FRAC_PI_2;

pub fn plane_view(p: &Plane) -> (f32, f32) {
    let n = p.normal().as_vec3();
    if n.z.abs() > 0.999 {
        return (-FRAC_PI_2, n.z.signum() * (FRAC_PI_2 - 1e-3));
    }
    (n.y.atan2(n.x), n.z.asin())
}

/// Indices into [X, Y, Z] axis colors for the plane's u and v directions.
pub fn plane_axes(p: &Plane) -> (usize, usize) {
    let dominant = |a: glam::DVec3| (0..3).max_by(|&i, &j| a.abs()[i].total_cmp(&a.abs()[j])).unwrap();
    (dominant(p.u), dominant(p.v))
}

pub struct View {
    pub rect: Rect,
    pub camera: Camera,
    pub plane: Plane,
}

impl View {
    fn aspect(&self) -> f32 {
        self.rect.width() / self.rect.height().max(1.0)
    }

    pub fn screen(&self, w: Vec3) -> Pos2 {
        let n = self.camera.project(w, self.aspect()).unwrap_or(glam::Vec2::splat(-10.0));
        self.rect.min + vec2((n.x + 1.0) * 0.5 * self.rect.width(), (1.0 - n.y) * 0.5 * self.rect.height())
    }

    pub fn world(&self, q: DVec2) -> Vec3 {
        self.plane.world(q).as_vec3()
    }

    pub fn ray(&self, p: Pos2) -> (Vec3, Vec3) {
        let r = self.rect;
        let ndc = glam::Vec2::new((p.x - r.min.x) / r.width() * 2.0 - 1.0, 1.0 - (p.y - r.min.y) / r.height() * 2.0);
        self.camera.ray(ndc, self.aspect())
    }

    pub fn to_screen(&self, q: DVec2) -> Pos2 {
        self.screen(self.world(q))
    }

    pub fn to_sketch(&self, p: Pos2) -> Option<DVec2> {
        let (o, d) = self.ray(p);
        self.plane.hit(o.as_dvec3(), d.as_dvec3()).map(|h| h.1)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tool {
    Select,
    Line,
    Rect,
    Circle,
    Arc,
    Dimension,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Sel {
    Point(P),
    Entity(E),
    Constraint(usize),
}

#[derive(Clone, Copy)]
enum Pick {
    Point(P),
    Free(DVec2),
    On(E, DVec2),
    /// At fraction t along a line; becomes a fraction constraint.
    Fraction(E, f64, DVec2),
    /// A point of the existing model; becomes a fixed point.
    Fixed(DVec2),
    /// On centerlines: same y as the first point and/or same x as the second.
    Aligned(DVec2, Option<P>, Option<P>),
}

/// A horizontal (y = c) or vertical (x = c) centerline through a source point, over a range.
/// Circle center lines are drawn ±1.25 r but snap along their whole length (alignment inference).
struct Guide {
    at: f64,
    source: P,
    range: (f64, f64),
}

/// The sketch axes and the center marks of circles and arcs: (horizontal, vertical).
fn guides(sk: &Sketch) -> (Vec<Guide>, Vec<Guide>) {
    let all = (f64::MIN, f64::MAX);
    let (mut h, mut v) = (vec![Guide { at: 0.0, source: Sketch::ORIGIN, range: all }], vec![Guide { at: 0.0, source: Sketch::ORIGIN, range: all }]);
    for (_, ent) in sk.live() {
        if let Entity::Circle { c, .. } | Entity::Arc { c, .. } = ent {
            let p = sk.pt(c);
            h.push(Guide { at: p.y, source: c, range: all });
            v.push(Guide { at: p.x, source: c, range: all });
        }
    }
    (h, v)
}

/// A snap target from the existing model, projected into the sketch plane.
#[derive(Clone, Copy)]
pub struct ModelSnap {
    pub at: DVec2,
    pub marker: Marker,
}

struct Drag {
    points: Vec<(P, DVec2)>,
    radius: Option<(usize, P)>,
    before: Sketch,
}

struct DimEdit {
    c: usize,
    text: String,
    pos: Pos2,
    focus: bool,
}

pub struct Editor {
    pub index: usize,
    pub tool: Tool,
    pending: Vec<Pick>,
    chain_start: Option<P>,
    selection: Vec<Sel>,
    hover: Option<Sel>,
    drag: Option<Drag>,
    undo: Vec<Sketch>,
    redo: Vec<Sketch>,
    dim_edit: Option<DimEdit>,
    analysis: cs::Analysis,
    badges: Vec<(Rect, usize)>,
    cursor: Option<DVec2>,
    snap: Option<(Marker, bool, Pick)>,
    pub toast: Option<String>,
    /// How curves are divided for snapping (2 = midpoints).
    pub divisions: u32,
    pub model_snaps: Vec<ModelSnap>,
}

const SNAP_PX: f32 = 9.0;
const HV_TOL: f64 = 0.05;

impl Editor {
    pub fn new(index: usize, sk: &Sketch) -> Self {
        Self {
            index,
            tool: Tool::Line,
            pending: vec![],
            chain_start: None,
            selection: vec![],
            hover: None,
            drag: None,
            undo: vec![],
            redo: vec![],
            dim_edit: None,
            analysis: cs::analyze(sk),
            badges: vec![],
            cursor: None,
            snap: None,
            toast: None,
            divisions: 2,
            model_snaps: vec![],
        }
    }

    pub fn dof(&self) -> usize {
        self.analysis.dof
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.pending.clear();
        self.chain_start = None;
    }

    pub fn hint(&self) -> &'static str {
        match (self.tool, self.pending.len()) {
            (Tool::Select, _) => "Click to select · Shift-click to add · drag to move",
            (Tool::Line, 0) => "Click to start a line",
            (Tool::Line, _) => "Click the next point · click the start to close · Esc to stop",
            (Tool::Rect, 0) => "Click the first corner",
            (Tool::Rect, _) => "Click the opposite corner",
            (Tool::Circle, 0) => "Click the center",
            (Tool::Circle, _) => "Click to set the radius",
            (Tool::Arc, 0) => "Click the center",
            (Tool::Arc, 1) => "Click the start point",
            (Tool::Arc, _) => "Click the end point (counter-clockwise)",
            (Tool::Dimension, 0) => "Click a line, circle, arc or point",
            (Tool::Dimension, _) => "Click a second point",
        }
    }

    fn checkpoint(&mut self, sk: &Sketch) {
        self.undo.push(sk.clone());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Solves after an edit; rolls back to the last checkpoint if it can't be satisfied.
    fn commit(&mut self, sk: &mut Sketch) -> bool {
        let ok = cs::solve(sk, &[]);
        if !ok {
            if let Some(prev) = self.undo.pop() {
                *sk = prev;
            }
            self.toast = Some("That would over-constrain the sketch".into());
        }
        self.analysis = cs::analyze(sk);
        ok
    }

    pub fn undo(&mut self, sk: &mut Sketch) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(sk, prev));
            self.reset(sk);
        }
    }

    pub fn redo(&mut self, sk: &mut Sketch) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(sk, next));
            self.reset(sk);
        }
    }

    fn reset(&mut self, sk: &Sketch) {
        self.pending.clear();
        self.chain_start = None;
        self.selection.clear();
        self.dim_edit = None;
        self.analysis = cs::analyze(sk);
    }

    pub fn escape(&mut self) {
        if self.dim_edit.take().is_some() {
        } else if !self.pending.is_empty() {
            self.pending.clear();
            self.chain_start = None;
        } else if self.tool != Tool::Select {
            self.tool = Tool::Select;
        } else {
            self.selection.clear();
        }
    }

    /// Toggles the selected curves between normal and construction (centerline) geometry.
    pub fn toggle_construction(&mut self, sk: &mut Sketch) {
        let picked: Vec<E> = self.selection.iter().filter_map(|s| if let Sel::Entity(e) = s { Some(*e) } else { None }).collect();
        if picked.is_empty() {
            self.toast = Some("Select lines, circles or arcs first".into());
            return;
        }
        self.checkpoint(sk);
        for e in picked {
            sk.toggle_construction(e);
        }
        self.commit(sk);
    }

    pub fn delete(&mut self, sk: &mut Sketch) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint(sk);
        let mut cons: Vec<usize> = self.selection.iter().filter_map(|s| if let Sel::Constraint(c) = s { Some(*c) } else { None }).collect();
        cons.sort_unstable_by(|a, b| b.cmp(a));
        for c in cons {
            sk.constraints.remove(c);
        }
        for s in std::mem::take(&mut self.selection) {
            match s {
                Sel::Entity(e) => sk.remove_entity(e),
                Sel::Point(p) => {
                    for e in sk.point_used_by(p).collect::<Vec<_>>() {
                        sk.remove_entity(e);
                    }
                }
                Sel::Constraint(_) => {}
            }
        }
        self.commit(sk);
    }

    pub fn constrain(&mut self, sk: &mut Sketch, action: Action) {
        use Constraint as C;
        let pts: Vec<P> = self.selection.iter().filter_map(|s| if let Sel::Point(p) = s { Some(*p) } else { None }).collect();
        let ents: Vec<(E, Entity)> = self.selection.iter().filter_map(|s| if let Sel::Entity(e) = s { sk.entity(*e).map(|x| (*e, x)) } else { None }).collect();
        let lines: Vec<(P, P)> = ents.iter().filter_map(|(_, e)| if let Entity::Line { a, b } = e { Some((*a, *b)) } else { None }).collect();
        let is_line = |e: &Entity| matches!(e, Entity::Line { .. });
        let two = ents.len() == 2 && pts.is_empty();
        let (new, need): (Vec<C>, &str) = match action {
            Action::Horizontal | Action::Vertical => {
                let mk = |a, b| if action == Action::Horizontal { C::Horizontal(a, b) } else { C::Vertical(a, b) };
                let v = if !lines.is_empty() && pts.is_empty() {
                    lines.iter().map(|&(a, b)| mk(a, b)).collect()
                } else if pts.len() == 2 && ents.is_empty() {
                    vec![mk(pts[0], pts[1])]
                } else {
                    vec![]
                };
                (v, "lines or two points")
            }
            Action::Coincident => match (pts.len(), ents.len()) {
                (2, 0) => (vec![C::Coincident(pts[0], pts[1])], ""),
                (1, 1) => (vec![C::OnCurve(pts[0], ents[0].0)], ""),
                _ => (vec![], "two points, or a point and a curve"),
            },
            Action::Parallel | Action::Perpendicular if two && lines.len() == 2 => {
                let (a, b) = (ents[0].0, ents[1].0);
                (vec![if action == Action::Parallel { C::Parallel(a, b) } else { C::Perpendicular(a, b) }], "")
            }
            Action::Parallel | Action::Perpendicular => (vec![], "two lines"),
            Action::Equal if two && is_line(&ents[0].1) == is_line(&ents[1].1) => (vec![C::Equal(ents[0].0, ents[1].0)], ""),
            Action::Equal => (vec![], "two lines or two circles/arcs"),
            Action::Tangent if two && !(is_line(&ents[0].1) && is_line(&ents[1].1)) => (vec![C::Tangent(ents[0].0, ents[1].0)], ""),
            Action::Tangent => (vec![], "a line and a circle/arc, or two circles/arcs"),
            Action::Fix => {
                let mut all = pts.clone();
                all.extend(ents.iter().flat_map(|(_, e)| Sketch::entity_points(*e)));
                all.dedup();
                (all.into_iter().map(C::Fix).collect(), "points or curves")
            }
            _ => (vec![], ""),
        };
        let new: Vec<C> = new.into_iter().filter(|c| !sk.constraints.contains(c)).collect();
        if new.is_empty() {
            if !need.is_empty() {
                self.toast = Some(format!("Select {need} first"));
            }
            return;
        }
        self.checkpoint(sk);
        sk.constraints.extend(new);
        if self.commit(sk) {
            self.selection.clear();
        }
    }

    fn hit(&self, sk: &Sketch, view: &View, pos: Pos2) -> Option<Sel> {
        if let Some(&(_, c)) = self.badges.iter().find(|(r, _)| r.expand(2.0).contains(pos)) {
            return Some(Sel::Constraint(c));
        }
        let skip = self.drag.as_ref().map(|d| d.points.iter().map(|p| p.0).collect::<Vec<_>>()).unwrap_or_default();
        let near = |q: DVec2| view.to_screen(q).distance(pos);
        let point = sk.points.iter().filter(|p| !skip.contains(p)).map(|&p| (near(sk.pt(p)), p)).filter(|d| d.0 < SNAP_PX).min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, p)) = point {
            return Some(Sel::Point(p));
        }
        sk.live()
            .filter(|(_, e)| !Sketch::entity_points(*e).iter().any(|p| skip.contains(p)))
            .map(|(i, e)| {
                let pts: Vec<Pos2> = cs::polyline(sk, e).into_iter().map(|q| view.to_screen(q)).collect();
                (pts.windows(2).map(|w| seg_dist(pos, w[0], w[1])).fold(f32::MAX, f32::min), i)
            })
            .filter(|d| d.0 < 6.0)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, i)| Sel::Entity(i))
    }

    fn pick(&self, view: &View, pos: Pos2) -> Option<Pick> {
        let q = view.to_sketch(pos)?;
        Some(match self.hover {
            Some(Sel::Point(p)) => Pick::Point(p),
            Some(Sel::Entity(e)) => Pick::On(e, q),
            _ => Pick::Free(q),
        })
    }

    fn pick_pos(sk: &Sketch, p: Pick) -> DVec2 {
        match p {
            Pick::Point(p) => sk.pt(p),
            Pick::Free(q) | Pick::On(_, q) | Pick::Fraction(_, _, q) | Pick::Fixed(q) | Pick::Aligned(q, ..) => q,
        }
    }

    /// Division points of an entity: (k, position, fraction along a line).
    fn divisions_of(&self, sk: &Sketch, e: Entity) -> Vec<(u32, DVec2, f64)> {
        let n = self.divisions;
        if n < 2 {
            return vec![];
        }
        match e {
            Entity::Line { a, b } => (1..n).map(|k| (k, sk.pt(a).lerp(sk.pt(b), k as f64 / n as f64), k as f64 / n as f64)).collect(),
            Entity::Circle { c, r } => (0..n).map(|k| (k, sk.pt(c) + DVec2::from_angle(std::f64::consts::TAU * k as f64 / n as f64) * sk.params[r], 0.0)).collect(),
            Entity::Arc { c, a, b } => {
                let (c, a, b) = (sk.pt(c), sk.pt(a), sk.pt(b));
                let a0 = (a - c).to_angle();
                let sweep = ((b - c).to_angle() - a0).rem_euclid(std::f64::consts::TAU);
                (1..n).map(|k| (k, c + DVec2::from_angle(a0 + sweep * k as f64 / n as f64) * a.distance(c), 0.0)).collect()
            }
        }
    }

    fn curve_dist(sk: &Sketch, view: &View, e: Entity, pos: Pos2) -> f32 {
        let pts: Vec<Pos2> = cs::polyline(sk, e).into_iter().map(|q| view.to_screen(q)).collect();
        pts.windows(2).map(|w| seg_dist(pos, w[0], w[1])).fold(f32::MAX, f32::min)
    }

    /// The best snap under the cursor while drawing:
    /// endpoint/center > division point > model geometry > on curve > free.
    fn snap(&self, sk: &Sketch, view: &View, pos: Pos2) -> Option<(Marker, bool, Pick)> {
        let q = view.to_sketch(pos)?;
        let d = |a: DVec2| view.to_screen(a).distance(pos);
        let nearest = |it: &mut dyn Iterator<Item = (DVec2, Marker, bool, Pick)>| it.map(|c| (d(c.0), c)).filter(|x| x.0 < SNAP_PX).min_by(|a, b| a.0.total_cmp(&b.0)).map(|x| x.1);
        let centers: Vec<P> = sk.live().filter_map(|(_, e)| match e {
            Entity::Circle { c, .. } | Entity::Arc { c, .. } => Some(c),
            _ => None,
        }).collect();
        let points = sk.points.iter().map(|&p| (sk.pt(p), if centers.contains(&p) { Marker::Center } else { Marker::Point }, false, Pick::Point(p)));
        if let Some(c) = nearest(&mut points.into_iter()) {
            return Some((c.1, c.2, c.3));
        }
        let mut divs = vec![];
        for (i, e) in sk.live().filter(|(_, e)| Self::curve_dist(sk, view, *e, pos) < 16.0) {
            for (k, at, t) in self.divisions_of(sk, e) {
                let pick = if matches!(e, Entity::Line { .. }) { Pick::Fraction(i, t, at) } else { Pick::On(i, at) };
                divs.push((at, Marker::Division(if matches!(e, Entity::Circle { .. }) { k.max(1) } else { k }, self.divisions), false, pick));
            }
        }
        if let Some(c) = nearest(&mut divs.into_iter()) {
            return Some((c.1, c.2, c.3));
        }
        if let Some(c) = nearest(&mut self.model_snaps.iter().map(|m| (m.at, m.marker, true, Pick::Fixed(m.at)))) {
            return Some((c.1, c.2, c.3));
        }
        let (hs, vs) = guides(sk);
        let within = |g: &Guide, x: f64| (g.range.0..=g.range.1).contains(&x);
        let mut crosses = vec![];
        for h in &hs {
            for v in vs.iter().filter(|v| within(h, v.at) && within(v, h.at) && !(h.source == v.source)) {
                crosses.push((DVec2::new(v.at, h.at), Marker::Cross, false, Pick::Aligned(DVec2::new(v.at, h.at), Some(h.source), Some(v.source))));
            }
        }
        if let Some(c) = nearest(&mut crosses.into_iter()) {
            return Some((c.1, c.2, c.3));
        }
        let on_h = hs.iter().filter(|g| within(g, q.x)).map(|g| (DVec2::new(q.x, g.at), Marker::Guide, false, Pick::Aligned(DVec2::new(q.x, g.at), Some(g.source), None)));
        let on_v = vs.iter().filter(|g| within(g, q.y)).map(|g| (DVec2::new(g.at, q.y), Marker::Guide, false, Pick::Aligned(DVec2::new(g.at, q.y), None, Some(g.source))));
        let near_guide = |c: &(DVec2, Marker, bool, Pick)| d(c.0) < 7.0;
        let curve_hover = matches!(self.hover, Some(Sel::Entity(_)));
        if let Some(c) = on_h.chain(on_v).filter(near_guide).min_by(|a, b| d(a.0).total_cmp(&d(b.0))).filter(|_| !curve_hover) {
            return Some((c.1, c.2, c.3));
        }
        Some(match self.hover {
            Some(Sel::Entity(e)) => (Marker::Curve, false, Pick::On(e, q)),
            _ => (Marker::Point, false, Pick::Free(q)),
        })
    }

    fn materialize(sk: &mut Sketch, p: Pick) -> P {
        match p {
            Pick::Point(p) => p,
            Pick::Free(q) => sk.add_point(q),
            Pick::On(e, q) => {
                let p = sk.add_point(q);
                sk.constraints.push(Constraint::OnCurve(p, e));
                p
            }
            Pick::Fraction(e, t, q) => {
                let p = sk.add_point(q);
                sk.constraints.push(Constraint::Fraction(p, e, t));
                p
            }
            Pick::Fixed(q) => {
                let p = sk.add_point(q);
                sk.constraints.push(Constraint::Fix(p));
                p
            }
            Pick::Aligned(q, h, v) => {
                let p = sk.add_point(q);
                sk.constraints.extend(h.map(|h| Constraint::Horizontal(h, p)));
                sk.constraints.extend(v.map(|v| Constraint::Vertical(v, p)));
                p
            }
        }
    }

    fn auto_hv(sk: &Sketch, a: P, b: P) -> Option<Constraint> {
        let d = sk.pt(b) - sk.pt(a);
        if d.y.abs() <= d.x.abs() * HV_TOL {
            Some(Constraint::Horizontal(a, b))
        } else if d.x.abs() <= d.y.abs() * HV_TOL {
            Some(Constraint::Vertical(a, b))
        } else {
            None
        }
    }

    /// Handles viewport input. Returns true if the primary drag was consumed.
    pub fn input(&mut self, ui: &egui::Ui, resp: &Response, view: &View, sk: &mut Sketch) -> bool {
        let pos = resp.hover_pos().or(resp.interact_pointer_pos());
        self.cursor = pos.and_then(|p| view.to_sketch(p));
        if self.drag.is_none() {
            self.hover = pos.and_then(|p| self.hit(sk, view, p));
        }
        let drawing = matches!(self.tool, Tool::Line | Tool::Rect | Tool::Circle | Tool::Arc);
        self.snap = if drawing { pos.and_then(|p| self.snap(sk, view, p)) } else { None };
        let typing = ui.ctx().egui_wants_keyboard_input();
        if !typing && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete) || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)) {
            self.delete(sk);
        }
        if self.tool != Tool::Select && resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        } else if self.hover.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        if let Some(pos) = pos.filter(|_| resp.clicked_by(PointerButton::Primary)) {
            let shift = ui.input(|i| i.modifiers.shift);
            self.click(sk, view, pos, shift);
        }
        if resp.clicked_by(PointerButton::Secondary) {
            self.escape();
        }
        if resp.double_clicked()
            && let Some(Sel::Constraint(c)) = self.hover {
                self.edit_dim(sk, c);
            }

        if self.tool == Tool::Select && resp.drag_started_by(PointerButton::Primary)
            && let (Some(h), Some(grab)) = (self.hover, self.cursor) {
                self.start_drag(sk, h, grab);
            }
        let Some(drag) = &self.drag else { return false };
        if let Some(cur) = self.cursor.filter(|_| resp.dragged_by(PointerButton::Primary)) {
            let mut targets: Vec<(usize, f64)> = drag.points.iter().flat_map(|&(p, off)| [(p, (cur + off).x), (p + 1, (cur + off).y)]).collect();
            if let Some((r, c)) = drag.radius {
                targets.push((r, cur.distance(sk.pt(c))));
            }
            cs::solve(sk, &targets);
        }
        if !resp.dragged() {
            let d = self.drag.take().unwrap();
            if d.before.params != sk.params {
                self.undo.push(d.before);
                self.redo.clear();
            }
            self.analysis = cs::analyze(sk);
        }
        true
    }

    fn start_drag(&mut self, sk: &Sketch, h: Sel, grab: DVec2) {
        let (pts, radius) = match h {
            Sel::Point(p) if p != Sketch::ORIGIN => (vec![p], None),
            Sel::Entity(e) => match sk.entity(e) {
                Some(Entity::Circle { c, r }) => (vec![], Some((r, c))),
                Some(e) => (Sketch::entity_points(e), None),
                None => return,
            },
            _ => return,
        };
        self.drag = Some(Drag { points: pts.into_iter().map(|p| (p, sk.pt(p) - grab)).collect(), radius, before: sk.clone() });
    }

    fn click(&mut self, sk: &mut Sketch, view: &View, pos: Pos2, shift: bool) {
        if self.tool == Tool::Select {
            match self.hover {
                Some(h) if shift => {
                    if let Some(i) = self.selection.iter().position(|s| *s == h) {
                        self.selection.remove(i);
                    } else {
                        self.selection.push(h);
                    }
                }
                Some(h) => self.selection = vec![h],
                None => self.selection.clear(),
            }
            return;
        }
        let Some(pick) = self.snap.map(|s| s.2).or_else(|| self.pick(view, pos)) else { return };
        if self.tool == Tool::Dimension {
            self.dimension_click(sk, pick);
            return;
        }
        let need = if self.tool == Tool::Arc { 2 } else { 1 };
        if self.pending.len() < need {
            self.pending.push(pick);
            return;
        }
        self.checkpoint(sk);
        let q = Self::pick_pos(sk, pick);
        match self.tool {
            Tool::Line => {
                let prev = Self::materialize(sk, self.pending[0]);
                if matches!(pick, Pick::Point(p) if p == prev) || sk.pt(prev).distance(q) < 1e-9 {
                    self.undo.pop();
                    return;
                }
                let p = Self::materialize(sk, pick);
                sk.add_line(prev, p);
                if let Some(c) = Self::auto_hv(sk, prev, p) {
                    sk.constraints.push(c);
                    if !cs::solve(sk, &[]) {
                        sk.constraints.pop();
                    }
                }
                let start = *self.chain_start.get_or_insert(prev);
                if p == start {
                    self.pending.clear();
                    self.chain_start = None;
                } else {
                    self.pending = vec![Pick::Point(p)];
                }
            }
            Tool::Rect => {
                let first = self.pending.remove(0);
                let a = Self::pick_pos(sk, first);
                if (a.x - q.x).abs() < 1e-6 || (a.y - q.y).abs() < 1e-6 {
                    self.undo.pop();
                    return;
                }
                let p1 = Self::materialize(sk, first);
                let p3 = Self::materialize(sk, pick);
                let p2 = sk.add_point(DVec2::new(q.x, a.y));
                let p4 = sk.add_point(DVec2::new(a.x, q.y));
                for (x, y) in [(p1, p2), (p2, p3), (p3, p4), (p4, p1)] {
                    sk.add_line(x, y);
                }
                use Constraint as C;
                sk.constraints.extend([C::Horizontal(p1, p2), C::Vertical(p2, p3), C::Horizontal(p3, p4), C::Vertical(p4, p1)]);
            }
            Tool::Circle => {
                let c = Self::materialize(sk, self.pending[0]);
                self.pending.clear();
                let r = sk.pt(c).distance(q);
                if r < 1e-6 {
                    self.undo.pop();
                    return;
                }
                let e = sk.add_circle(c, r);
                if let Pick::Point(p) = pick {
                    sk.constraints.push(Constraint::OnCurve(p, e));
                }
            }
            Tool::Arc => {
                let c = Self::materialize(sk, self.pending[0]);
                let a = Self::materialize(sk, self.pending[1]);
                self.pending.clear();
                let (cp, r) = (sk.pt(c), sk.pt(a).distance(sk.pt(c)));
                let b = match pick {
                    Pick::Point(p) => p,
                    _ => sk.add_point(cp + (q - cp).normalize_or(DVec2::X) * r),
                };
                if r < 1e-6 || a == b {
                    self.undo.pop();
                    return;
                }
                sk.add_arc(c, a, b);
            }
            _ => {}
        }
        self.commit(sk);
    }

    fn dimension_click(&mut self, sk: &mut Sketch, pick: Pick) {
        let c = match (self.hover, self.pending.first()) {
            (Some(Sel::Point(p)), Some(&Pick::Point(q))) if p != q => Constraint::Distance(q, p, sk.pt(p).distance(sk.pt(q))),
            (Some(Sel::Point(_)), _) => {
                self.pending = vec![pick];
                return;
            }
            (Some(Sel::Entity(e)), _) => match sk.entity(e) {
                Some(Entity::Line { a, b }) => Constraint::Distance(a, b, sk.pt(a).distance(sk.pt(b))),
                Some(_) => Constraint::Radius(e, sk.radius(e)),
                None => return,
            },
            _ => return,
        };
        self.pending.clear();
        if let Some(i) = sk.constraints.iter().position(|x| x.value().is_some() && std::mem::discriminant(x) == std::mem::discriminant(&c) && same_target(x, &c)) {
            self.edit_dim(sk, i);
            return;
        }
        self.checkpoint(sk);
        sk.constraints.push(c);
        if self.commit(sk) {
            self.edit_dim(sk, sk.constraints.len() - 1);
        }
    }

    fn edit_dim(&mut self, sk: &Sketch, c: usize) {
        if let Some(v) = sk.constraints.get(c).and_then(|c| c.value()) {
            self.dim_edit = Some(DimEdit { c, text: fmt_len(v), pos: Pos2::ZERO, focus: true });
        }
    }

    fn apply_dim(&mut self, sk: &mut Sketch) {
        let Some(d) = self.dim_edit.take() else { return };
        let Ok(v) = d.text.trim().trim_end_matches("mm").trim().parse::<f64>() else {
            self.toast = Some(format!("'{}' isn't a number", d.text));
            return;
        };
        if v <= 0.0 {
            self.toast = Some("Dimensions must be positive".into());
            return;
        }
        self.checkpoint(sk);
        sk.constraints[d.c].set_value(v);
        self.commit(sk);
    }

    pub fn draw(&mut self, ui: &mut egui::Ui, painter: &Painter, view: &View, sk_mut: &mut Sketch) {
        let p = theme::palette(ui.ctx());
        let sk = &*sk_mut;
        fill_profiles(painter, view, sk, p.accent.gamma_multiply(0.12));

        let det = |i: usize| self.analysis.determined.get(i).copied().unwrap_or(false);
        let color_of = |sel: Sel, determined: bool| {
            if self.selection.contains(&sel) {
                p.accent
            } else if self.hover == Some(sel) {
                p.accent.gamma_multiply(0.8)
            } else if determined {
                p.text
            } else {
                p.free
            }
        };
        for (i, e) in sk.live() {
            let mut params: Vec<usize> = Sketch::entity_points(e).iter().flat_map(|&q| [q, q + 1]).collect();
            if let Entity::Circle { r, .. } = e {
                params.push(r);
            }
            let col = color_of(Sel::Entity(i), params.iter().all(|&q| det(q)));
            let w = if self.hover == Some(Sel::Entity(i)) { 2.5 } else { 1.6 };
            let pts: Vec<Pos2> = cs::polyline(sk, e).into_iter().map(|q| view.to_screen(q)).collect();
            curve(painter, pts, Stroke::new(w, col), sk.is_construction(i));
        }
        center_marks(painter, view, sk, p);
        for &q in &sk.points {
            let pos = view.to_screen(sk.pt(q));
            if q == Sketch::ORIGIN {
                painter.circle(pos, 4.0, p.bg, Stroke::new(1.5, p.accent));
                continue;
            }
            let hot = self.hover == Some(Sel::Point(q));
            painter.circle_filled(pos, if hot { 4.5 } else { 3.0 }, color_of(Sel::Point(q), det(q) && det(q + 1)));
        }
        self.draw_constraints(painter, view, sk, p);
        self.draw_preview(painter, view, sk, p);

        if let Some(d) = &mut self.dim_edit {
            let mut done = false;
            let mut cancel = false;
            egui::Area::new(egui::Id::unique("dim_edit")).fixed_pos(d.pos - vec2(40.0, 13.0)).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).inner_margin(2.0).show(ui, |ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut d.text).desired_width(76.0).align(Align2::CENTER_CENTER));
                    if d.focus {
                        r.request_focus();
                        d.focus = false;
                    }
                    done = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    cancel = r.lost_focus() && !done;
                });
            });
            if done {
                self.apply_dim(sk_mut);
            } else if cancel {
                self.dim_edit = None;
            }
        }
    }

    fn draw_constraints(&mut self, painter: &Painter, view: &View, sk: &Sketch, p: &Palette) {
        self.badges.clear();
        let mut stack: std::collections::HashMap<(i32, i32), f32> = Default::default();
        let font = FontId::proportional(11.0);
        for (ci, c) in sk.constraints.iter().enumerate() {
            let sel = self.selection.contains(&Sel::Constraint(ci));
            let hot = self.hover == Some(Sel::Constraint(ci));
            if let Some(v) = c.value() {
                let (anchor, text) = match *c {
                    Constraint::Distance(a, b, _) => {
                        let (sa, sb) = (view.to_screen(sk.pt(a)), view.to_screen(sk.pt(b)));
                        let n = (sb - sa).normalized().rot90() * -18.0;
                        let line = Stroke::new(1.0, p.weak);
                        painter.line_segment([sa, sa + n * 1.2], line);
                        painter.line_segment([sb, sb + n * 1.2], line);
                        painter.line_segment([sa + n, sb + n], line);
                        (sa.lerp(sb, 0.5) + n, fmt_len(v))
                    }
                    Constraint::Radius(e, _) => {
                        let Some(center) = (match sk.entity(e) {
                            Some(Entity::Circle { c, .. } | Entity::Arc { c, .. }) => Some(sk.pt(c)),
                            _ => None,
                        }) else { continue };
                        let edge = center + DVec2::splat(std::f64::consts::FRAC_1_SQRT_2) * v;
                        let (sc, se) = (view.to_screen(center), view.to_screen(edge));
                        painter.line_segment([sc, se], Stroke::new(1.0, p.weak));
                        (se + (se - sc).normalized() * 16.0, format!("R{}", fmt_len(v)))
                    }
                    _ => continue,
                };
                let galley = painter.layout_no_wrap(text, font.clone(), p.text);
                let rect = Rect::from_center_size(anchor, galley.size() + vec2(10.0, 4.0));
                painter.rect(rect, 4, if sel { p.accent.gamma_multiply(0.4) } else { p.card }, Stroke::new(1.0, if hot { p.accent } else { p.line }), egui::StrokeKind::Inside);
                painter.galley(rect.center() - galley.size() * 0.5, galley, p.text);
                if let Some(d) = self.dim_edit.as_mut().filter(|d| d.c == ci) {
                    d.pos = anchor;
                }
                self.badges.push((rect, ci));
                continue;
            }
            let (glyph, anchors) = badge(sk, c, view);
            for (pos, dir) in anchors {
                let key = ((pos.x / 6.0) as i32, (pos.y / 6.0) as i32);
                let k = stack.entry(key).or_insert(0.0);
                let at = pos + dir * (14.0 + *k);
                *k += 18.0;
                let rect = Rect::from_center_size(at, vec2(16.0, 16.0));
                painter.rect(rect, 4, if sel { p.accent } else { p.card }, Stroke::new(1.0, if hot { p.accent } else { p.line }), egui::StrokeKind::Inside);
                icons::paint(painter, at, glyph, 11.0, if sel { p.on_accent } else { p.weak });
                self.badges.push((rect, ci));
            }
        }
    }

    fn draw_preview(&self, painter: &Painter, view: &View, sk: &Sketch, p: &Palette) {
        let Some(mut cur) = self.cursor else { return };
        if let Some((marker, model, pick)) = self.snap {
            let pos = view.to_screen(cur);
            for (_, e) in sk.live().filter(|(_, e)| Self::curve_dist(sk, view, *e, pos) < 16.0) {
                for (_, at, _) in self.divisions_of(sk, e) {
                    snap::tick(painter, view.to_screen(at), p);
                }
            }
            for m in self.model_snaps.iter().filter(|m| view.to_screen(m.at).distance(pos) < 80.0) {
                snap::tick(painter, view.to_screen(m.at), p);
            }
            cur = Self::pick_pos(sk, pick);
            if let Pick::Aligned(q, h, v) = pick {
                let guide = Stroke::new(1.0, p.accent.gamma_multiply(0.5));
                if let Some(h) = h {
                    curve(painter, vec![view.to_screen(DVec2::new(sk.pt(h).x, q.y)), view.to_screen(q)], guide, true);
                }
                if let Some(v) = v {
                    curve(painter, vec![view.to_screen(DVec2::new(q.x, sk.pt(v).y)), view.to_screen(q)], guide, true);
                }
            }
            if !matches!(pick, Pick::Free(_)) {
                snap::draw(painter, view.to_screen(cur), marker, model, p);
            }
        }
        let stroke = Stroke::new(1.5, p.accent.gamma_multiply(0.9));
        let s = |q: DVec2| view.to_screen(q);
        let pend: Vec<DVec2> = self.pending.iter().map(|&k| Self::pick_pos(sk, k)).collect();
        match (self.tool, pend.as_slice()) {
            (Tool::Line, [a]) => {
                painter.line_segment([s(*a), s(cur)], stroke);
                let d = cur - *a;
                let hint = if d.y.abs() <= d.x.abs() * HV_TOL { "H" } else if d.x.abs() <= d.y.abs() * HV_TOL { "V" } else { "" };
                let label = format!("{}  {}", fmt_len(d.length()), hint);
                painter.text(s(cur) + vec2(14.0, 14.0), Align2::LEFT_TOP, label.trim(), FontId::proportional(11.0), p.weak);
            }
            (Tool::Rect, [a]) => {
                let c = [*a, DVec2::new(cur.x, a.y), cur, DVec2::new(a.x, cur.y)].map(s);
                painter.add(Shape::closed_line(c.to_vec(), stroke));
                let d = (cur - *a).abs();
                painter.text(s(cur) + vec2(14.0, 14.0), Align2::LEFT_TOP, format!("{} × {}", fmt_len(d.x), fmt_len(d.y)), FontId::proportional(11.0), p.weak);
            }
            (Tool::Circle, [c]) => {
                let r = c.distance(cur);
                let pts = (0..=64).map(|i| s(*c + DVec2::from_angle(i as f64 / 64.0 * std::f64::consts::TAU) * r)).collect();
                painter.add(Shape::line(pts, stroke));
                painter.text(s(cur) + vec2(14.0, 14.0), Align2::LEFT_TOP, format!("R{}", fmt_len(r)), FontId::proportional(11.0), p.weak);
            }
            (Tool::Arc, [c]) => {
                painter.add(Shape::dashed_line(&[s(*c), s(cur)], stroke, 4.0, 4.0));
            }
            (Tool::Arc, [c, a]) => {
                let r = c.distance(*a);
                let b = *c + (cur - *c).normalize_or(DVec2::X) * r;
                let (a0, mut sweep) = ((*a - *c).to_angle(), ((b - *c).to_angle() - (*a - *c).to_angle()).rem_euclid(std::f64::consts::TAU));
                if sweep < 1e-6 {
                    sweep = std::f64::consts::TAU;
                }
                let pts = (0..=64).map(|i| s(*c + DVec2::from_angle(a0 + sweep * i as f64 / 64.0) * r)).collect();
                painter.add(Shape::line(pts, stroke));
            }
            _ => {}
        }
    }
}

fn same_target(a: &Constraint, b: &Constraint) -> bool {
    match (*a, *b) {
        (Constraint::Distance(a1, b1, _), Constraint::Distance(a2, b2, _)) => (a1, b1) == (a2, b2) || (a1, b1) == (b2, a2),
        (Constraint::Radius(e1, _), Constraint::Radius(e2, _)) => e1 == e2,
        _ => false,
    }
}

fn badge(sk: &Sketch, c: &Constraint, view: &View) -> (&'static str, Vec<(Pos2, Vec2)>) {
    let at_ent = |e: E| -> (Pos2, Vec2) {
        match sk.entity(e) {
            Some(Entity::Line { a, b }) => {
                let (sa, sb) = (view.to_screen(sk.pt(a)), view.to_screen(sk.pt(b)));
                (sa.lerp(sb, 0.5), (sb - sa).normalized().rot90())
            }
            Some(ent) => {
                let pts = cs::polyline(sk, ent);
                let mid = view.to_screen(pts[pts.len() / 2]);
                let c = match ent {
                    Entity::Circle { c, .. } | Entity::Arc { c, .. } => view.to_screen(sk.pt(c)),
                    _ => mid,
                };
                (mid, (mid - c).normalized())
            }
            None => (Pos2::ZERO, Vec2::ZERO),
        }
    };
    let at_pts = |a: P, b: P| {
        let (sa, sb) = (view.to_screen(sk.pt(a)), view.to_screen(sk.pt(b)));
        let line = sk.live().find(|(_, e)| matches!(e, Entity::Line { a: x, b: y } if (*x, *y) == (a, b) || (*x, *y) == (b, a)));
        match line {
            Some((e, _)) => at_ent(e),
            None => (sa.lerp(sb, 0.5), vec2(0.7, -0.7)),
        }
    };
    let corner = vec2(0.7, -0.7);
    use Constraint::*;
    match *c {
        Horizontal(a, b) => ("H", vec![at_pts(a, b)]),
        Vertical(a, b) => ("V", vec![at_pts(a, b)]),
        Parallel(a, b) => (icons::PARALLEL, vec![at_ent(a), at_ent(b)]),
        Perpendicular(a, b) => (icons::PERPENDICULAR, vec![at_ent(a), at_ent(b)]),
        Equal(a, b) => (icons::EQUALS, vec![at_ent(a), at_ent(b)]),
        Tangent(a, b) => ("T", vec![at_ent(a), at_ent(b)]),
        Coincident(a, _) => (icons::DOT, vec![(view.to_screen(sk.pt(a)), corner)]),
        OnCurve(a, _) => (icons::DOT, vec![(view.to_screen(sk.pt(a)), corner)]),
        Fix(a) => (icons::LOCK, vec![(view.to_screen(sk.pt(a)), corner)]),
        Fraction(a, _, _) => ("÷", vec![(view.to_screen(sk.pt(a)), corner)]),
        Distance(..) | Radius(..) => ("", vec![]),
    }
}

fn plane_quad(view: &View, plane: &Plane) -> (f64, [Pos2; 4]) {
    let s = view.camera.dist as f64 * 0.22;
    let q = [DVec2::ZERO, DVec2::new(s, 0.0), DVec2::splat(s), DVec2::new(0.0, s)];
    (s, q.map(|q| view.screen(plane.world(q).as_vec3())))
}

/// The nearest standard plane quad under the cursor, with its ray distance.
pub fn pick_plane(view: &View, pos: Pos2) -> Option<(f64, usize)> {
    let (o, d) = view.ray(pos);
    Plane::STANDARD
        .iter()
        .enumerate()
        .filter_map(|(i, pl)| {
            let s = plane_quad(view, pl).0;
            pl.hit(o.as_dvec3(), d.as_dvec3()).filter(|(_, q)| (0.0..=s).contains(&q.x) && (0.0..=s).contains(&q.y)).map(|(t, _)| (t, i))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

pub fn draw_planes(painter: &Painter, view: &View, hover: Option<usize>, p: &Palette) {
    let colors = [Color32::from_rgb(0x3e, 0x8e, 0xf7), Color32::from_rgb(0x46, 0xa7, 0x58), Color32::from_rgb(0xe5, 0x48, 0x4d)];
    for (i, pl) in Plane::STANDARD.iter().enumerate() {
        let (_, quad) = plane_quad(view, pl);
        let hot = hover == Some(i);
        let c = colors[i];
        painter.add(Shape::convex_polygon(quad.to_vec(), c.gamma_multiply(if hot { 0.45 } else { 0.16 }), Stroke::new(if hot { 2.0 } else { 1.0 }, c)));
        let center = quad.iter().fold(Vec2::ZERO, |a, q| a + q.to_vec2()) / 4.0;
        painter.text(center.to_pos2(), Align2::CENTER_CENTER, pl.name(), FontId::new(13.0, egui::FontFamily::Name("bold".into())), if hot { p.text } else { p.weak });
    }
}

/// Outlines a face plane around a hit point.
pub fn draw_face_hint(painter: &Painter, view: &View, plane: &Plane, at: glam::DVec3, p: &Palette) {
    let s = view.camera.dist as f64 * 0.06;
    let c = DVec2::new((at - plane.o).dot(plane.u), (at - plane.o).dot(plane.v));
    let quad: Vec<Pos2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].iter().map(|&(x, y)| view.to_screen(c + DVec2::new(x, y) * s)).collect();
    painter.add(Shape::convex_polygon(quad, p.accent.gamma_multiply(0.3), Stroke::new(2.0, p.accent)));
}

/// The closest line entity to a screen point.
pub fn hit_line(sk: &Sketch, view: &View, pos: Pos2) -> Option<E> {
    sk.live()
        .filter_map(|(i, e)| match e {
            Entity::Line { a, b } => Some((seg_dist(pos, view.to_screen(sk.pt(a)), view.to_screen(sk.pt(b))), i)),
            _ => None,
        })
        .filter(|d| d.0 < 8.0)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|d| d.1)
}

pub fn fill_profiles(painter: &Painter, view: &View, sk: &Sketch, color: Color32) {
    for poly in cs::profiles(sk) {
        fill_poly(painter, view, &poly, color);
    }
}

pub fn fill_poly(painter: &Painter, view: &View, poly: &[DVec2], color: Color32) {
    let mut mesh = egui::Mesh::default();
    for &q in poly {
        mesh.colored_vertex(view.to_screen(q), color);
    }
    for t in cs::triangulate(poly) {
        mesh.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
    }
    painter.add(mesh);
}

pub fn draw_passive(painter: &Painter, view: &View, sk: &Sketch, p: &Palette) {
    fill_profiles(painter, view, sk, p.accent.gamma_multiply(0.07));
    for (i, e) in sk.live() {
        let pts: Vec<Pos2> = cs::polyline(sk, e).into_iter().map(|q| view.to_screen(q)).collect();
        curve(painter, pts, Stroke::new(1.2, p.weak), sk.is_construction(i));
    }
}

/// A solid curve, or dash-dot for construction geometry.
fn curve(painter: &Painter, pts: Vec<Pos2>, stroke: Stroke, construction: bool) {
    if construction {
        painter.extend(Shape::dashed_line_with_offset(&pts, Stroke::new(stroke.width.min(1.4), stroke.color), &[10.0, 2.0], &[3.0, 2.0], 0.0));
    } else {
        painter.add(Shape::line(pts, stroke));
    }
}

/// Dash-dot centerlines through the centers of circles and arcs.
fn center_marks(painter: &Painter, view: &View, sk: &Sketch, p: &Palette) {
    let stroke = Stroke::new(1.0, p.weak.gamma_multiply(0.8));
    for (e, ent) in sk.live() {
        if let Entity::Circle { c, .. } | Entity::Arc { c, .. } = ent {
            let (c, r) = (sk.pt(c), sk.radius(e) * 1.25);
            for d in [DVec2::X, DVec2::Y] {
                curve(painter, vec![view.to_screen(c - d * r), view.to_screen(c + d * r)], stroke, true);
            }
        }
    }
}

pub fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

pub fn fmt_len(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}
