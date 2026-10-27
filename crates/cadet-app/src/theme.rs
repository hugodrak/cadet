use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke, TextStyle, Theme, Visuals};
use std::sync::Arc;

pub struct Palette {
    pub bg: Color32,
    pub card: Color32,
    pub line: Color32,
    pub text: Color32,
    pub weak: Color32,
    pub hover: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub free: Color32,
    pub ok: Color32,
}

pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x12, 0x13, 0x16),
    card: Color32::from_rgb(0x1c, 0x1e, 0x22),
    line: Color32::from_rgb(0x2c, 0x2f, 0x35),
    text: Color32::from_rgb(0xe6, 0xe7, 0xea),
    weak: Color32::from_rgb(0x8b, 0x8f, 0x98),
    hover: Color32::from_rgb(0x2a, 0x2d, 0x33),
    accent: Color32::from_rgb(0xf5, 0xa5, 0x24),
    on_accent: Color32::from_rgb(0x1a, 0x12, 0x02),
    free: Color32::from_rgb(0x5a, 0xa9, 0xff),
    ok: Color32::from_rgb(0x4c, 0xc3, 0x8a),
};

pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xf3, 0xf4, 0xf6),
    card: Color32::from_rgb(0xff, 0xff, 0xff),
    line: Color32::from_rgb(0xe1, 0xe3, 0xe8),
    text: Color32::from_rgb(0x1d, 0x1f, 0x23),
    weak: Color32::from_rgb(0x6b, 0x70, 0x7a),
    hover: Color32::from_rgb(0xec, 0xee, 0xf1),
    accent: Color32::from_rgb(0xe0, 0x84, 0x0b),
    on_accent: Color32::WHITE,
    free: Color32::from_rgb(0x1f, 0x6f, 0xeb),
    ok: Color32::from_rgb(0x1a, 0x9a, 0x5c),
};

impl Palette {
    pub fn error(&self) -> Color32 {
        Color32::from_rgb(0xe5, 0x48, 0x4d)
    }
}

pub const AXIS_Z: [f32; 4] = [0x3e as f32 / 255.0, 0x8e as f32 / 255.0, 0xf7 as f32 / 255.0, 0.9];

pub fn palette(ctx: &egui::Context) -> &'static Palette {
    if ctx.global_style().visuals.dark_mode { &DARK } else { &LIGHT }
}

pub fn viewport_style(dark: bool, show_grid: bool, show_edges: bool) -> cadet_render::Style {
    let c = |hex: u32, a: f32| {
        let [_, r, g, b] = hex.to_be_bytes();
        [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a]
    };
    if dark {
        cadet_render::Style {
            bg_top: c(0x2b2f36, 1.0),
            bg_bottom: c(0x141619, 1.0),
            body: c(0x9aa6b6, 1.0),
            edge: c(0x0b0c0e, 0.8),
            grid_minor: c(0x6a707c, 0.10),
            grid_major: c(0x8a919e, 0.26),
            axis_x: c(0xe5484d, 0.9),
            axis_y: c(0x46a758, 0.9),
            show_grid,
            show_edges,
            show_body: true,
            grid_plane: [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        }
    } else {
        cadet_render::Style {
            bg_top: c(0xfbfbfc, 1.0),
            bg_bottom: c(0xd9dde4, 1.0),
            body: c(0xc3ccd8, 1.0),
            edge: c(0x2b2f36, 0.85),
            grid_minor: c(0x8a919e, 0.14),
            grid_major: c(0x6a707c, 0.32),
            axis_x: c(0xe5484d, 0.9),
            axis_y: c(0x46a758, 0.9),
            show_grid,
            show_edges,
            show_body: true,
            grid_plane: [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        }
    }
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let font = |bytes: &'static [u8]| Arc::new(FontData::from_static(bytes));
    fonts.font_data.insert("inter".into(), font(include_bytes!("../../../assets/fonts/Inter-Regular.ttf")));
    fonts.font_data.insert("inter-semibold".into(), font(include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf")));
    fonts.font_data.insert("phosphor".into(), font(include_bytes!("../../../assets/fonts/Phosphor.ttf")));
    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    prop.insert(0, "inter".into());
    prop.insert(1, "phosphor".into());
    fonts.families.insert(FontFamily::Name("bold".into()), vec!["inter-semibold".into(), "phosphor".into()]);
    ctx.set_fonts(fonts);

    ctx.set_visuals_of(Theme::Dark, visuals(&DARK, Visuals::dark()));
    ctx.set_visuals_of(Theme::Light, visuals(&LIGHT, Visuals::light()));
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 6.0);
        s.spacing.button_padding = egui::vec2(8.0, 5.0);
        s.spacing.interact_size.y = 26.0;
        s.text_styles = [
            (TextStyle::Small, FontId::proportional(11.0)),
            (TextStyle::Body, FontId::proportional(13.0)),
            (TextStyle::Button, FontId::proportional(13.0)),
            (TextStyle::Monospace, FontId::monospace(12.0)),
            (TextStyle::Heading, FontId::new(15.0, FontFamily::Name("bold".into()))),
        ]
        .into();
    });
    ctx.set_theme(Theme::Dark);
}

fn visuals(p: &Palette, mut v: Visuals) -> Visuals {
    let r = CornerRadius::same(6);
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.window_stroke = Stroke::new(1.0, p.line);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(if v.dark_mode { 110 } else { 35 }) };
    v.popup_shadow = v.window_shadow;
    v.extreme_bg_color = p.bg;
    v.faint_bg_color = p.hover;
    v.override_text_color = Some(p.text);
    v.selection.bg_fill = p.accent.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.hyperlink_color = p.accent;
    let w = &mut v.widgets;
    for (s, fill) in [(&mut w.noninteractive, p.card), (&mut w.inactive, Color32::TRANSPARENT), (&mut w.hovered, p.hover), (&mut w.active, p.line), (&mut w.open, p.hover)] {
        s.bg_fill = fill;
        s.weak_bg_fill = fill;
        s.corner_radius = r;
        s.bg_stroke = Stroke::NONE;
        s.expansion = 0.0;
    }
    w.inactive.bg_fill = p.hover;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.active.fg_stroke = Stroke::new(1.0, p.text);
    v
}
