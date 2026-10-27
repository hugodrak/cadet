use crate::{icons, theme};
use egui::{Key, Modifiers, RichText};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Sketch,
    Extrude,
    Revolve,
    Fillet,
    Chamfer,
    Measure,
    Export,
    Home,
    Front,
    Right,
    Top,
    ToggleTheme,
    ToggleGrid,
    ToggleEdges,
    Palette,
    Undo,
    Redo,
    Select,
    Line,
    Rect,
    Circle,
    Arc,
    Dimension,
    Horizontal,
    Vertical,
    Parallel,
    Perpendicular,
    Equal,
    Tangent,
    Coincident,
    Fix,
    Construction,
    Delete,
    Finish,
    New,
    Open,
    Save,
    SaveAs,
    ExportStep,
    Import,
    Quit,
    Shortcuts,
    Console,
    OpenPlugins,
    ReloadPlugins,
    Settings,
    About,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Scope {
    Any,
    Model,
    Sketch,
}

pub struct Command {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    pub key: Option<(Modifiers, Key)>,
    pub action: Action,
    pub scope: Scope,
}

const fn cmd(id: &'static str, label: &'static str, icon: &'static str, key: Option<(Modifiers, Key)>, action: Action, scope: Scope) -> Command {
    Command { id, label, icon, key, action, scope }
}

const NONE: Modifiers = Modifiers::NONE;
const fn k(key: Key) -> Option<(Modifiers, Key)> {
    Some((NONE, key))
}
const fn shift(key: Key) -> Option<(Modifiers, Key)> {
    Some((Modifiers::SHIFT, key))
}
const fn cmd_key(key: Key) -> Option<(Modifiers, Key)> {
    Some((Modifiers::COMMAND, key))
}
const CMD_SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::COMMAND };

use Scope::{Any, Model, Sketch};
pub const COMMANDS: &[Command] = &[
    cmd("sketch.new", "Sketch", icons::PENCIL, k(Key::S), Action::Sketch, Model),
    cmd("feature.extrude", "Extrude", icons::EXTRUDE, k(Key::E), Action::Extrude, Model),
    cmd("feature.revolve", "Revolve", icons::REVOLVE, k(Key::R), Action::Revolve, Model),
    cmd("feature.fillet", "Fillet", icons::FILLET, k(Key::F), Action::Fillet, Model),
    cmd("feature.chamfer", "Chamfer", icons::CHAMFER, shift(Key::F), Action::Chamfer, Model),
    cmd("inspect.measure", "Measure", icons::RULER, k(Key::I), Action::Measure, Model),
    cmd("file.export", "Export STL…", icons::EXPORT, cmd_key(Key::E), Action::Export, Any),
    cmd("file.export_step", "Export STEP…", icons::EXPORT, Some((CMD_SHIFT, Key::E)), Action::ExportStep, Any),
    cmd("file.import", "Import STEP/IGES…", icons::IMPORT, Some((CMD_SHIFT, Key::I)), Action::Import, Any),
    cmd("app.quit", "Quit", icons::EXPORT, cmd_key(Key::Q), Action::Quit, Any),
    cmd("python.console", "Python console", icons::TERMINAL, cmd_key(Key::J), Action::Console, Any),
    cmd("python.plugins_folder", "Open plugins folder", icons::PLUGIN, None, Action::OpenPlugins, Any),
    cmd("python.reload", "Reload plugins", icons::PLUGIN, None, Action::ReloadPlugins, Any),
    cmd("app.settings", "Settings…", icons::SETTINGS, cmd_key(Key::Comma), Action::Settings, Any),
    cmd("app.about", "About Cadet", icons::CUBE, None, Action::About, Any),
    cmd("help.shortcuts", "Keyboard shortcuts", icons::SEARCH, cmd_key(Key::Slash), Action::Shortcuts, Any),
    cmd("sketch.select", "Select", icons::CURSOR, None, Action::Select, Sketch),
    cmd("sketch.line", "Line", icons::LINE, k(Key::L), Action::Line, Sketch),
    cmd("sketch.rect", "Rectangle", icons::RECT, k(Key::R), Action::Rect, Sketch),
    cmd("sketch.circle", "Circle", icons::CIRCLE, k(Key::C), Action::Circle, Sketch),
    cmd("sketch.arc", "Arc", icons::ARC, k(Key::A), Action::Arc, Sketch),
    cmd("sketch.dimension", "Dimension", icons::DIMENSION, k(Key::D), Action::Dimension, Sketch),
    cmd("constraint.horizontal", "Horizontal", "H", k(Key::H), Action::Horizontal, Sketch),
    cmd("constraint.vertical", "Vertical", "V", k(Key::V), Action::Vertical, Sketch),
    cmd("constraint.parallel", "Parallel", icons::PARALLEL, k(Key::P), Action::Parallel, Sketch),
    cmd("constraint.perpendicular", "Perpendicular", icons::PERPENDICULAR, shift(Key::P), Action::Perpendicular, Sketch),
    cmd("constraint.equal", "Equal", icons::EQUALS, k(Key::E), Action::Equal, Sketch),
    cmd("constraint.tangent", "Tangent", "T", k(Key::T), Action::Tangent, Sketch),
    cmd("constraint.coincident", "Coincident", icons::DOT, k(Key::O), Action::Coincident, Sketch),
    cmd("constraint.fix", "Fix", icons::LOCK, k(Key::F), Action::Fix, Sketch),
    cmd("sketch.construction", "Toggle centerline / construction", icons::DASHED, k(Key::X), Action::Construction, Sketch),
    cmd("sketch.delete", "Delete selection", icons::EDGES, None, Action::Delete, Sketch),
    cmd("sketch.finish", "Finish", icons::CHECK, cmd_key(Key::Enter), Action::Finish, Sketch),
    cmd("file.new", "New document", icons::NEW, cmd_key(Key::N), Action::New, Any),
    cmd("file.open", "Open…", icons::OPEN, cmd_key(Key::O), Action::Open, Any),
    cmd("file.save", "Save", icons::SAVE, cmd_key(Key::S), Action::Save, Any),
    cmd("file.save_as", "Save as…", icons::SAVE, Some((CMD_SHIFT, Key::S)), Action::SaveAs, Any),
    cmd("edit.undo", "Undo", icons::UNDO, cmd_key(Key::Z), Action::Undo, Any),
    cmd("edit.redo", "Redo", icons::REDO, Some((CMD_SHIFT, Key::Z)), Action::Redo, Any),
    cmd("view.home", "View: Home", icons::HOME, k(Key::Num0), Action::Home, Any),
    cmd("view.front", "View: Front", icons::CUBE, k(Key::Num1), Action::Front, Any),
    cmd("view.right", "View: Right", icons::CUBE, k(Key::Num2), Action::Right, Any),
    cmd("view.top", "View: Top", icons::CUBE, k(Key::Num3), Action::Top, Any),
    cmd("view.grid", "Toggle grid", icons::GRID, k(Key::G), Action::ToggleGrid, Any),
    cmd("view.edges", "Toggle edges", icons::EDGES, shift(Key::E), Action::ToggleEdges, Any),
    cmd("app.theme", "Toggle light/dark", icons::SUN, None, Action::ToggleTheme, Any),
    cmd("app.palette", "Command palette", icons::SEARCH, cmd_key(Key::K), Action::Palette, Any),
];

/// The menu bar: menus of separator-delimited groups.
pub const MENUS: &[(&str, &[&[Action]])] = {
    use Action::*;
    &[
        ("File", &[&[New, Open], &[Save, SaveAs], &[Import], &[Export, ExportStep], &[Quit]]),
        ("Edit", &[&[Undo, Redo], &[Delete], &[Palette], &[Settings]]),
        ("View", &[&[Home, Front, Right, Top], &[ToggleGrid, ToggleEdges, ToggleTheme]]),
        (
            "Sketch",
            &[
                &[Sketch, Finish],
                &[Line, Rect, Circle, Arc, Dimension],
                &[Horizontal, Vertical, Parallel, Perpendicular, Equal, Tangent, Coincident, Fix],
                &[Construction],
            ],
        ),
        ("Model", &[&[Extrude, Revolve], &[Fillet, Chamfer]]),
        ("Inspect", &[&[Measure]]),
        ("Plugins", &[&[Console], &[OpenPlugins, ReloadPlugins]]),
        ("Help", &[&[Shortcuts], &[About]]),
    ]
};

pub fn in_scope(c: &Command, sketch: bool) -> bool {
    c.scope == Any || (c.scope == Sketch) == sketch
}

pub fn by_id(id: &str) -> Option<Action> {
    COMMANDS.iter().find(|c| c.id == id).map(|c| c.action)
}

pub fn get(action: Action) -> &'static Command {
    COMMANDS.iter().find(|c| c.action == action).unwrap()
}

pub fn shortcut_text(c: &Command) -> String {
    const MAC: egui::ModifierNames =
        egui::ModifierNames { is_short: true, alt: "\u{e8a8}", ctrl: "\u{eca6}", shift: "\u{e52e}", mac_cmd: "\u{e1c4}", mac_alt: "\u{e8a8}", concat: "" };
    let mac = cfg!(target_os = "macos");
    let names = if mac { MAC } else { egui::ModifierNames::NAMES };
    c.key.map(|(m, k)| egui::KeyboardShortcut::new(m, k).format(&names, mac).replace('⏎', "\u{e782}")).unwrap_or_default()
}

pub fn poll_shortcuts(ctx: &egui::Context, sketch: bool) -> Option<Action> {
    let typing = ctx.egui_wants_keyboard_input();
    ctx.input_mut(|i| {
        let mut sorted: Vec<_> = COMMANDS.iter().filter(|c| in_scope(c, sketch)).filter_map(|c| c.key.map(|k| (k, c.action))).collect();
        sorted.sort_by_key(|((m, _), _)| std::cmp::Reverse(m.shift as u8 + m.command as u8 + m.alt as u8));
        sorted.into_iter().find(|&((m, k), _)| (!typing || m.command) && i.consume_key(m, k)).map(|(_, a)| a)
    })
}

fn fuzzy(query: &str, text: &str) -> Option<i32> {
    let mut score = 0;
    let mut chars = text.chars().map(|c| c.to_ascii_lowercase()).enumerate();
    let mut last = 0;
    for q in query.chars().filter(|c| !c.is_whitespace()).map(|c| c.to_ascii_lowercase()) {
        let (i, _) = chars.find(|&(_, c)| c == q)?;
        score += if i == last + 1 || i == 0 { 3 } else { 1 };
        last = i;
    }
    Some(score - text.len() as i32 / 8)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Choice {
    Action(Action),
    Plugin(usize),
}

#[derive(Default)]
pub struct Palette {
    pub open: bool,
    query: String,
    selected: usize,
}

impl Palette {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.query.clear();
        self.selected = 0;
    }

    /// Shows the palette over the built-in commands plus `extra` entries (plugins).
    pub fn show(&mut self, ctx: &egui::Context, sketch: bool, extra: &[String]) -> Option<Choice> {
        if !self.open {
            return None;
        }
        let p = theme::palette(ctx);
        let builtin = COMMANDS
            .iter()
            .filter(|c| c.action != Action::Palette && in_scope(c, sketch))
            .map(|c| (c.label.to_string(), c.id, c.icon, shortcut_text(c), Choice::Action(c.action)));
        let plugins = extra.iter().enumerate().map(|(i, l)| (format!("Plugin: {l}"), "plugin", icons::PLUGIN, String::new(), Choice::Plugin(i)));
        let mut hits: Vec<_> = builtin.chain(plugins).filter_map(|e| fuzzy(&self.query, &e.0).or_else(|| fuzzy(&self.query, e.1)).map(|s| (s, e))).collect();
        hits.sort_by_key(|(s, _)| -s);
        self.selected = self.selected.min(hits.len().saturating_sub(1));

        let (up, down, enter) = ctx.input_mut(|i| (i.consume_key(NONE, Key::ArrowUp), i.consume_key(NONE, Key::ArrowDown), i.consume_key(NONE, Key::Enter)));
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        if down {
            self.selected = (self.selected + 1).min(hits.len().saturating_sub(1));
        }
        let mut chosen = enter.then(|| hits.get(self.selected).map(|(_, e)| e.4)).flatten();

        let area = egui::Modal::default_area(egui::Id::unique("palette")).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 90.0));
        let resp = egui::Modal::new(egui::Id::unique("palette"))
            .area(area)
            .backdrop_color(egui::Color32::from_black_alpha(60))
            .frame(egui::Frame::popup(&ctx.global_style()).inner_margin(8.0).corner_radius(12))
            .show(ctx, |ui| {
                ui.set_width(460.0);
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text(format!("{}  Type a command…", icons::SEARCH))
                        .frame(egui::Frame::NONE)
                        .desired_width(f32::INFINITY)
                        .font(egui::FontId::proportional(16.0))
                        .margin(egui::vec2(6.0, 8.0)),
                );
                edit.request_focus();
                if edit.changed() {
                    self.selected = 0;
                }
                ui.separator();
                for (i, (_, (label, _, icon, keys, choice))) in hits.iter().enumerate().take(10) {
                    let sel = i == self.selected;
                    let r = ui.add_sized([ui.available_width(), 30.0], egui::Button::new("").fill(if sel { p.hover } else { egui::Color32::TRANSPARENT }));
                    let rect = r.rect.shrink2(egui::vec2(10.0, 0.0));
                    let painter = ui.painter();
                    let font = egui::FontId::proportional(13.0);
                    let col = if sel { p.accent } else { p.weak };
                    icons::paint(painter, rect.left_center() + egui::vec2(8.0, 0.0), icon, 16.0, col);
                    painter.text(rect.left_center() + egui::vec2(26.0, 0.0), egui::Align2::LEFT_CENTER, label, font.clone(), p.text);
                    painter.text(rect.right_center(), egui::Align2::RIGHT_CENTER, keys, font, p.weak);
                    if r.clicked() {
                        chosen = Some(*choice);
                    }
                    if r.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                        self.selected = i;
                    }
                }
                if hits.is_empty() {
                    ui.label(RichText::new("No matching commands").color(p.weak));
                }
            });
        if chosen.is_some() || resp.should_close() {
            self.open = false;
        }
        chosen
    }
}
