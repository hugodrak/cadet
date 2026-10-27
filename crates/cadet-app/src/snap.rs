//! Snap markers and the geometry shared by sketching and measuring.

use crate::theme::Palette;
use egui::{Align2, FontId, Painter, Pos2, Shape, Stroke, vec2};
use glam::DVec3;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Marker {
    Point,
    Center,
    /// k of n along a curve.
    Division(u32, u32),
    Curve,
    Face,
    /// On a centerline (sketch axis or a circle's center mark).
    Guide,
    /// Where two centerlines cross.
    Cross,
}

impl Marker {
    pub fn label(self) -> String {
        match self {
            Marker::Point => "Endpoint".into(),
            Marker::Center => "Center".into(),
            Marker::Division(1, 2) => "Midpoint".into(),
            Marker::Division(k, n) => format!("{k}/{n}"),
            Marker::Curve => "On curve".into(),
            Marker::Face => "On face".into(),
            Marker::Guide => "On centerline".into(),
            Marker::Cross => "Centerline intersection".into(),
        }
    }
}

/// What ⌘1 … ⌘9 set, for messages.
pub fn division_name(n: u32) -> String {
    match n {
        1 => "endpoints only".into(),
        2 => "midpoints".into(),
        3 => "thirds".into(),
        4 => "quarters".into(),
        n => format!("1/{n} points"),
    }
}

pub fn draw(painter: &Painter, at: Pos2, m: Marker, model: bool, p: &Palette) {
    let s = Stroke::new(1.6, p.accent);
    match m {
        Marker::Point => {
            painter.rect_stroke(egui::Rect::from_center_size(at, vec2(10.0, 10.0)), 1, s, egui::StrokeKind::Middle);
        }
        Marker::Center => {
            painter.circle_stroke(at, 7.0, s);
            painter.line_segment([at - vec2(4.0, 0.0), at + vec2(4.0, 0.0)], s);
            painter.line_segment([at - vec2(0.0, 4.0), at + vec2(0.0, 4.0)], s);
        }
        Marker::Division(..) => {
            let d = [vec2(0.0, -7.0), vec2(7.0, 0.0), vec2(0.0, 7.0), vec2(-7.0, 0.0)].map(|o| at + o);
            painter.add(Shape::closed_line(d.to_vec(), s));
        }
        Marker::Curve => {
            painter.line_segment([at - vec2(5.0, 5.0), at + vec2(5.0, 5.0)], s);
            painter.line_segment([at - vec2(5.0, -5.0), at + vec2(5.0, -5.0)], s);
        }
        Marker::Face | Marker::Guide => {
            painter.circle_stroke(at, 4.0, s);
        }
        Marker::Cross => {
            painter.circle_stroke(at, 6.0, s);
            painter.circle_filled(at, 2.0, p.accent);
        }
    }
    let text = if model { format!("{} · model", m.label()) } else { m.label() };
    painter.text(at + vec2(12.0, -12.0), Align2::LEFT_BOTTOM, text, FontId::proportional(11.0), p.accent);
}

/// A faint tick showing where a snap point is.
pub fn tick(painter: &Painter, at: Pos2, p: &Palette) {
    painter.circle(at, 2.5, p.bg, Stroke::new(1.2, p.accent.gamma_multiply(0.7)));
}

/// Points dividing a polyline into n equal lengths (interior points; all n for closed curves).
pub fn divide(points: &[DVec3], n: u32, closed: bool) -> Vec<(u32, DVec3)> {
    if n < 2 || points.len() < 2 {
        return vec![];
    }
    let total: f64 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
    let ks = if closed { 0..n } else { 1..n };
    ks.filter_map(|k| {
        let mut left = total * k as f64 / n as f64;
        for w in points.windows(2) {
            let len = w[0].distance(w[1]);
            if left <= len {
                return Some((k, w[0].lerp(w[1], if len > 0.0 { left / len } else { 0.0 })));
            }
            left -= len;
        }
        None
    })
    .collect()
}
