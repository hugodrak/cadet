pub const CUBE: &str = "\u{e1da}";
pub const PENCIL: &str = "\u{e3b4}";
pub const EXTRUDE: &str = "\u{e522}";
pub const REVOLVE: &str = "\u{e094}";
pub const FILLET: &str = "\u{eb00}";
pub const CHAMFER: &str = "\u{e6d0}";
pub const RULER: &str = "\u{e6b8}";
pub const EXPORT: &str = "\u{eaf0}";
pub const IMPORT: &str = "\u{e20c}";
pub const NEW: &str = "\u{e236}";
pub const OPEN: &str = "\u{e256}";
pub const SAVE: &str = "\u{e248}";
pub const BODY: &str = "\u{e1da}";
pub const COMMAND: &str = "\u{e1c4}";
pub const TERMINAL: &str = "\u{eae8}";
pub const PLUGIN: &str = "\u{e596}";
pub const CLOSE: &str = "\u{e4f6}";
pub const SETTINGS: &str = "\u{e270}";
pub const DASHED: &str = "\u{e6d4}";
pub const SUN: &str = "\u{e472}";
pub const MOON: &str = "\u{e330}";
pub const SEARCH: &str = "\u{e30c}";
pub const HOME: &str = "\u{e2c2}";
pub const EYE: &str = "\u{e220}";
pub const EYE_OFF: &str = "\u{e224}";
pub const STACK: &str = "\u{e466}";
pub const GRID: &str = "\u{e296}";
pub const UNDO: &str = "\u{e08a}";
pub const REDO: &str = "\u{e08c}";
pub const EDGES: &str = "\u{ec7c}";
pub const CURSOR: &str = "\u{e1dc}";
pub const LINE: &str = "\u{e6d2}";
pub const RECT: &str = "\u{e3f0}";
pub const CIRCLE: &str = "\u{e18a}";
pub const ARC: &str = "\u{eb44}";
pub const DIMENSION: &str = "\u{eb06}";
pub const LOCK: &str = "\u{e308}";
pub const EQUALS: &str = "\u{e21c}";
pub const DOT: &str = "\u{ece0}";
pub const CHECK: &str = "\u{e182}";
pub const PARALLEL: &str = "∥";
pub const PERPENDICULAR: &str = "⊥";

/// Draws an icon, painting the glyphs our fonts lack as vectors.
pub fn paint(painter: &egui::Painter, at: egui::Pos2, icon: &str, size: f32, color: egui::Color32) {
    let s = size * 0.36;
    let stroke = egui::Stroke::new((size / 10.0).max(1.2), color);
    match icon {
        PARALLEL => {
            for dx in [-0.35, 0.35] {
                painter.line_segment([at + egui::vec2(dx * s - 0.3 * s, s), at + egui::vec2(dx * s + 0.3 * s, -s)], stroke);
            }
        }
        PERPENDICULAR => {
            painter.line_segment([at + egui::vec2(-s, s), at + egui::vec2(s, s)], stroke);
            painter.line_segment([at + egui::vec2(0.0, s), at + egui::vec2(0.0, -s)], stroke);
        }
        _ => {
            painter.text(at, egui::Align2::CENTER_CENTER, icon, egui::FontId::proportional(size), color);
        }
    }
}
