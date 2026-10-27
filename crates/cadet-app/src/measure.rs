use crate::sketch::{View, fmt_len, seg_dist};
use crate::snap::{self, Marker};
use crate::theme::{self, Palette};
use cadet_core::kernel::EdgeRef;
use egui::{FontId, Painter, Pos2, RichText, Stroke, vec2};
use glam::DVec3;

#[derive(Default)]
pub struct Measure {
    points: Vec<DVec3>,
    hover: Option<(DVec3, Marker, Option<usize>)>,
    /// Latest ray hit on the model, filled in asynchronously by the kernel.
    pub face_hit: Option<DVec3>,
}

impl Measure {
    /// Snaps to vertex > circle center > division point > edge > face.
    pub fn hover(&mut self, view: &View, pos: Option<Pos2>, edges: &[EdgeRef], n: u32) {
        let Some(pos) = pos else { return self.hover = None };
        let s = |q: DVec3| view.screen(q.as_vec3());
        let nearest = |it: Vec<(DVec3, Marker, Option<usize>)>, r: f32| {
            it.into_iter().map(|c| (s(c.0).distance(pos), c)).filter(|x| x.0 < r).min_by(|a, b| a.0.total_cmp(&b.0)).map(|x| x.1)
        };
        let vertices = edges
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.closed())
            .flat_map(|(i, e)| [(e.points[0], Marker::Point, Some(i)), (*e.points.last().unwrap(), Marker::Point, Some(i))])
            .collect();
        let centers = edges.iter().enumerate().filter_map(|(i, e)| e.circle.map(|(c, _)| (c, Marker::Center, Some(i)))).collect();
        let near_edge = |e: &EdgeRef| e.points.windows(2).map(|w| seg_dist(pos, s(w[0]), s(w[1]))).fold(f32::MAX, f32::min) < 16.0;
        let divisions = edges
            .iter()
            .enumerate()
            .filter(|(_, e)| near_edge(e))
            .flat_map(|(i, e)| snap::divide(&e.points, n, e.closed()).into_iter().map(move |(k, q)| (q, Marker::Division(k.max(1), n), Some(i))))
            .collect();
        if let Some(h) = nearest(vertices, 10.0).or_else(|| nearest(centers, 10.0)).or_else(|| nearest(divisions, 9.0)) {
            return self.hover = Some(h);
        }
        let mut best: Option<(f32, DVec3, usize)> = None;
        for (i, e) in edges.iter().enumerate() {
            for w in e.points.windows(2) {
                let (a, b) = (s(w[0]), s(w[1]));
                let d = seg_dist(pos, a, b);
                if d < 7.0 && best.is_none_or(|b| d < b.0) {
                    let t = ((pos - a).dot(b - a) / (b - a).length_sq().max(1e-6)).clamp(0.0, 1.0);
                    best = Some((d, w[0].lerp(w[1], t as f64), i));
                }
            }
        }
        self.hover = match best {
            Some((_, q, i)) => Some((q, Marker::Curve, Some(i))),
            None => self.face_hit.map(|q| (q, Marker::Face, None)),
        };
    }

    pub fn click(&mut self) {
        if let Some((q, ..)) = self.hover {
            if self.points.len() == 2 {
                self.points.clear();
            }
            self.points.push(q);
        }
    }

    pub fn draw(&self, painter: &Painter, view: &View, p: &Palette) {
        let s = |q: DVec3| view.screen(q.as_vec3());
        if let [a, b] = self.points[..] {
            painter.line_segment([s(a), s(b)], Stroke::new(2.0, p.accent));
            let label = format!("{} mm", fmt_len(a.distance(b)));
            let mid = s(a).lerp(s(b), 0.5);
            let galley = painter.layout_no_wrap(label, FontId::proportional(13.0), p.text);
            let rect = egui::Rect::from_center_size(mid - vec2(0.0, 16.0), galley.size() + vec2(12.0, 6.0));
            painter.rect(rect, 5, p.card, Stroke::new(1.0, p.accent), egui::StrokeKind::Inside);
            painter.galley(rect.center() - galley.size() * 0.5, galley, p.text);
        }
        for &q in &self.points {
            painter.circle(s(q), 5.0, p.accent, Stroke::new(1.5, p.bg));
        }
        if let Some((q, marker, _)) = self.hover {
            snap::draw(painter, s(q), marker, false, p);
        }
    }

    pub fn panel(&self, ui: &mut egui::Ui, edges: &[EdgeRef], props: Option<&cadet_core::kernel::Props>) {
        let p = theme::palette(ui.ctx());
        ui.set_width(230.0);
        ui.label(RichText::new("Measure").heading());
        let row = |ui: &mut egui::Ui, k: &str, v: String| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(k).color(p.weak));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(v));
            });
        };
        if let Some(pr) = props {
            let d = pr.max - pr.min;
            row(ui, "Model size", format!("{} × {} × {} mm", fmt_len(d.x), fmt_len(d.y), fmt_len(d.z)));
            row(ui, "Volume", format!("{} cm³", fmt_len(pr.volume / 1000.0)));
            row(ui, "Surface", format!("{} cm²", fmt_len(pr.area / 100.0)));
        }
        if let Some((_, _, Some(i))) = self.hover {
            let e = &edges[i];
            row(ui, "Edge length", format!("{} mm", fmt_len(e.length())));
            if let Some((_, r)) = e.circle {
                row(ui, if e.closed() { "Diameter" } else { "Arc diameter" }, format!("Ø {} mm", fmt_len(2.0 * r)));
            }
        }
        ui.separator();
        match self.points[..] {
            [a, b] => {
                let d = b - a;
                row(ui, "Distance", format!("{} mm", fmt_len(d.length())));
                row(ui, "ΔX · ΔY · ΔZ", format!("{} · {} · {}", fmt_len(d.x.abs()), fmt_len(d.y.abs()), fmt_len(d.z.abs())));
            }
            [_] => {
                ui.label(RichText::new("Click a second point").color(p.weak).small());
            }
            _ => {
                ui.label(RichText::new("Click vertices, edges or faces · Esc to close").color(p.weak).small());
            }
        }
    }
}
