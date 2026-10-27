//! The Python console and plugin forms (RustPython, see cadet-script).

use crate::theme;
use cadet_core::Document;
use cadet_script::{Engine, Outcome, Plugin, Value};
use egui::{Align2, Id, Key, Modifiers, Pos2, RichText, vec2};
use std::path::PathBuf;

const BUILTIN: &[(&str, &str)] = &[
    ("gear.py", include_str!("../../../plugins/gear.py")),
    ("send_to_slicer.py", include_str!("../../../plugins/send_to_slicer.py")),
];

pub fn plugin_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("Cadet").join("plugins"))
}

#[derive(Clone, Copy, PartialEq)]
enum Line {
    Input,
    Output,
    Error,
}

pub struct Python {
    engine: Engine,
    pub open: bool,
    input: String,
    history: Vec<String>,
    recall: Option<usize>,
    log: Vec<(Line, String)>,
    form: Option<(usize, Vec<(String, Value)>)>,
    focus: bool,
}

impl Python {
    pub fn new() -> Self {
        let mut p = Self { engine: Engine::new(), open: false, input: String::new(), history: vec![], recall: None, log: vec![], form: None, focus: false };
        p.reload();
        p.log.push((Line::Output, "Python (RustPython). The `cadet` module is imported; try dir(cadet) or cadet.features().".into()));
        p
    }

    pub fn plugins(&self) -> &[Plugin] {
        self.engine.plugins()
    }

    /// Reloads built-in and user plugins. Returns the number that failed to load.
    pub fn reload(&mut self) -> usize {
        let errors = self.engine.load_plugins(plugin_dir().as_deref(), BUILTIN);
        for e in &errors {
            self.log.push((Line::Error, e.clone()));
        }
        errors.len()
    }

    /// Opens a plugin's form, or runs it right away if it takes no parameters.
    pub fn start(&mut self, i: usize, doc: &Document) -> Option<Outcome> {
        let params = self.engine.plugins().get(i)?.params.clone();
        if params.is_empty() {
            return Some(self.run_plugin(i, &[], doc));
        }
        self.form = Some((i, params));
        None
    }

    fn run_plugin(&mut self, i: usize, args: &[(String, Value)], doc: &Document) -> Outcome {
        let label = self.engine.plugins()[i].label.clone();
        self.log.push((Line::Input, format!("▶ {label}")));
        let out = self.engine.run_plugin(i, args, doc);
        self.record(&out);
        out
    }

    fn record(&mut self, out: &Outcome) {
        if !out.output.is_empty() {
            self.log.push((Line::Output, out.output.trim_end().to_string()));
        }
        if let Some(e) = &out.error {
            self.log.push((Line::Error, e.clone()));
            self.open = true;
        }
        let excess = self.log.len().saturating_sub(500);
        self.log.drain(..excess);
    }

    pub fn form_ui(&mut self, ctx: &egui::Context, doc: &Document) -> Option<Outcome> {
        let (i, values) = self.form.as_mut()?;
        let i = *i;
        let label = self.engine.plugins().get(i).map(|p| p.label.clone()).unwrap_or_default();
        let (mut run, mut close) = (false, false);
        egui::Window::new(label).id(Id::unique("plugin_form")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            egui::Grid::new("plugin_params").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                for (name, v) in values.iter_mut() {
                    ui.label(name.replace('_', " "));
                    match v {
                        Value::Number(x) => ui.add(egui::DragValue::new(x).speed(0.1).max_decimals(4)),
                        Value::Int(x) => ui.add(egui::DragValue::new(x).speed(0.2)),
                        Value::Bool(b) => ui.checkbox(b, ""),
                        Value::Text(s) => ui.text_edit_singleline(s),
                    };
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                close = ui.button("Cancel").clicked();
                let p = theme::palette(ui.ctx());
                run = ui.add(egui::Button::new(RichText::new("  Run  ").color(p.on_accent)).fill(p.accent)).clicked();
            });
        });
        if close {
            self.form = None;
        }
        if !run {
            return None;
        }
        let (i, values) = self.form.take()?;
        Some(self.run_plugin(i, &values, doc))
    }

    pub fn console_ui(&mut self, ctx: &egui::Context, at: Pos2, doc: &Document) -> Option<Outcome> {
        if !self.open {
            return None;
        }
        let p = theme::palette(ctx);
        let edit_id = Id::unique("python_input");
        let focused = ctx.memory(|m| m.has_focus(edit_id));
        let single_line = !self.input.contains('\n');
        let (enter, up, down) = if focused {
            ctx.input_mut(|i| (!i.modifiers.shift && i.consume_key(Modifiers::NONE, Key::Enter), single_line && i.consume_key(Modifiers::NONE, Key::ArrowUp), single_line && i.consume_key(Modifiers::NONE, Key::ArrowDown)))
        } else {
            (false, false, false)
        };
        if (up || down) && !self.history.is_empty() {
            let last = self.history.len() - 1;
            self.recall = match (self.recall, up) {
                (None, true) => Some(last),
                (Some(i), true) => Some(i.saturating_sub(1)),
                (Some(i), false) if i < last => Some(i + 1),
                _ => None,
            };
            self.input = self.recall.map(|i| self.history[i].clone()).unwrap_or_default();
        }
        let mut out = None;
        if enter && !self.input.trim().is_empty() {
            let code = std::mem::take(&mut self.input);
            self.log.push((Line::Input, code.clone()));
            self.history.push(code.clone());
            self.recall = None;
            let o = self.engine.run(&code, doc);
            self.record(&o);
            out = Some(o);
        }
        egui::Area::new(Id::unique("python_console")).pivot(Align2::CENTER_BOTTOM).fixed_pos(at).show(ctx, |ui| {
            egui::Frame::window(ui.style()).inner_margin(10.0).show(ui, |ui| {
                ui.set_width(640.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Python").heading());
                    ui.label(RichText::new("Enter runs · Shift+Enter new line · ↑↓ history").color(p.weak).small());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(crate::icons::CLOSE).clicked() {
                            self.open = false;
                        }
                        if ui.small_button("Clear").clicked() {
                            self.log.clear();
                        }
                    });
                });
                egui::ScrollArea::vertical().max_height(200.0).stick_to_bottom(true).auto_shrink([false, true]).show(ui, |ui| {
                    for (kind, text) in &self.log {
                        let (prefix, color) = match kind {
                            Line::Input => (">>> ", p.weak),
                            Line::Output => ("", p.text),
                            Line::Error => ("", p.error()),
                        };
                        ui.label(RichText::new(format!("{prefix}{text}")).monospace().color(color));
                    }
                });
                ui.separator();
                let edit = egui::TextEdit::multiline(&mut self.input).id(edit_id).code_editor().desired_rows(1).desired_width(f32::INFINITY).hint_text(">>> cadet.features()");
                let r = ui.add(edit);
                if std::mem::take(&mut self.focus) {
                    r.request_focus();
                }
            });
        });
        out
    }

    pub fn focus_input(&mut self) {
        self.focus = true;
    }
}
