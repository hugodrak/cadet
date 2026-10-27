//! User preferences, stored as JSON in the OS config folder.

use crate::sketch::fmt_len;
use crate::{snap, theme};
use egui::RichText;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum ThemeChoice {
    System,
    Dark,
    Light,
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum Quality {
    Coarse,
    Normal,
    Fine,
}

impl Quality {
    /// Maximum distance between the exported mesh and the true surface, in mm.
    pub fn tolerance(self) -> f64 {
        match self {
            Quality::Coarse => 0.05,
            Quality::Normal => 0.01,
            Quality::Fine => 0.002,
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeChoice,
    pub divisions: u32,
    pub stl_quality: Quality,
    pub restore_session: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { theme: ThemeChoice::Dark, divisions: 2, stl_quality: Quality::Normal, restore_session: true }
    }
}

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("Cadet").join("settings.json"))
}

impl Settings {
    pub fn load() -> Self {
        path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(p) = path() {
            let _ = std::fs::create_dir_all(p.parent().unwrap());
            let _ = std::fs::write(p, serde_json::to_string_pretty(self).unwrap_or_default());
        }
    }

    pub fn apply_theme(&self, ctx: &egui::Context) {
        ctx.set_theme(match self.theme {
            ThemeChoice::System => egui::ThemePreference::System,
            ThemeChoice::Dark => egui::ThemePreference::Dark,
            ThemeChoice::Light => egui::ThemePreference::Light,
        });
    }
}

pub enum Request {
    OpenPluginsFolder,
}

/// The Settings window. Saves on every change.
pub fn window(ctx: &egui::Context, open: &mut bool, s: &mut Settings) -> Option<Request> {
    let p = theme::palette(ctx);
    let before = s.clone();
    let mut request = None;
    egui::Window::new("Settings").open(open).collapsible(false).resizable(false).default_width(420.0).show(ctx, |ui| {
        egui::Grid::new("settings").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label("Appearance");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut s.theme, ThemeChoice::System, "System");
                ui.selectable_value(&mut s.theme, ThemeChoice::Dark, "Dark");
                ui.selectable_value(&mut s.theme, ThemeChoice::Light, "Light");
            });
            ui.end_row();
            ui.label("Snap points");
            ui.add(egui::Slider::new(&mut s.divisions, 1..=9).custom_formatter(|n, _| snap::division_name(n as u32)));
            ui.end_row();
            ui.label("STL quality");
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut s.stl_quality, Quality::Coarse, "Coarse");
                    ui.selectable_value(&mut s.stl_quality, Quality::Normal, "Normal");
                    ui.selectable_value(&mut s.stl_quality, Quality::Fine, "Fine");
                });
                ui.label(RichText::new(format!("Curved surfaces within {} mm of the model", fmt_len(s.stl_quality.tolerance()))).color(p.weak).small());
            });
            ui.end_row();
            ui.label("On launch");
            ui.checkbox(&mut s.restore_session, "Reopen where I left off");
            ui.end_row();
            ui.label("Plugins");
            if ui.button("Open plugins folder").clicked() {
                request = Some(Request::OpenPluginsFolder);
            }
            ui.end_row();
        });
    });
    if *s != before {
        s.save();
        s.apply_theme(ctx);
    }
    request
}
