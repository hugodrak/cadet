use crate::commands::{self, Action, Choice};
use crate::python::Python;
use crate::features::{FeatureEdit, Outcome};
use crate::measure::Measure;
use crate::sketch::{self, Editor, Tool, View};
use crate::settings::{self, Settings};
use crate::{devtools, files, icons, python, snap, theme};
use crate::kernel::{Event, Kernel, Model};
use cadet_core::{Document, Kind, Plane};
use cadet_render::{Camera, MeshData, Vertex, Viewport};
use cadet_sketch::Sketch;
use eframe::egui_wgpu;
use egui::{Align2, Color32, FontId, Id, PointerButton, Pos2, Rect, RichText, Sense, Stroke, Theme, ViewportCommand, vec2};
use glam::{DVec3, Vec3};
use std::f32::consts::FRAC_PI_2;
use std::path::PathBuf;

const TOPBAR_H: f32 = 40.0;
const GAP: f32 = 12.0;

#[derive(Clone, Copy)]
enum PlanePick {
    Standard(usize),
    Face(Plane, DVec3),
}

enum Mode {
    Model,
    PickPlane(Option<PlanePick>),
    Sketch(Box<Editor>),
    Feature(Box<FeatureEdit>),
    Measure(Box<Measure>),
}

pub struct App {
    render: egui_wgpu::RenderState,
    viewport: Viewport,
    texture: Option<egui::TextureId>,
    camera: Camera,
    goal: Option<Camera>,
    show_grid: bool,
    show_edges: bool,
    doc: Document,
    model: Model,
    kernel: Kernel,
    last_hit: Option<cadet_core::kernel::Hit>,
    path: Option<PathBuf>,
    dirty: bool,
    history: Vec<Document>,
    future: Vec<Document>,
    persist: bool,
    mode: Mode,
    palette: commands::Palette,
    toast: Option<(String, f64)>,
    script: devtools::Script,
    show_shortcuts: bool,
    quitting: bool,
    /// Snap divisions for curves and edges (⌘1 … ⌘9).
    divisions: u32,
    python: Python,
    /// Commands a script asked for, run once the model it changed has been rebuilt.
    after_model: Vec<Action>,
    /// A file to open once the export in flight finishes.
    open_after_export: Option<(PathBuf, Option<String>)>,
    plugin_pick: Option<usize>,
    exporting: bool,
    settings: Settings,
    show_settings: bool,
    show_about: bool,
    /// Plugin labels the native menu was last built with.
    menu_plugins: Option<Vec<String>>,
    #[cfg(target_os = "macos")]
    dump_menu: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        theme::install(&cc.egui_ctx);
        let render = cc.wgpu_render_state.clone().expect("wgpu renderer");
        let script = devtools::Script::from_env();
        let persist = !script.active() || std::env::var_os("CADET_PERSIST").is_some();
        let mut app = Self {
            viewport: Viewport::new(&render.device),
            render,
            texture: None,
            camera: Camera::default(),
            goal: None,
            show_grid: true,
            show_edges: true,
            doc: Document::default(),
            model: Model::default(),
            kernel: Kernel::new(&cc.egui_ctx),
            last_hit: None,
            path: None,
            dirty: false,
            history: vec![],
            future: vec![],
            persist,
            mode: Mode::Model,
            palette: Default::default(),
            toast: None,
            script,
            show_shortcuts: false,
            quitting: false,
            divisions: 2,
            python: Python::new(),
            after_model: vec![],
            open_after_export: None,
            plugin_pick: None,
            exporting: false,
            settings: Settings::load(),
            show_settings: false,
            show_about: false,
            menu_plugins: None,
            #[cfg(target_os = "macos")]
            dump_menu: false,
        };
        app.settings.apply_theme(&cc.egui_ctx);
        app.divisions = app.settings.divisions;
        #[cfg(target_os = "macos")]
        crate::macos::set_context(&cc.egui_ctx);
        let arg = std::env::args().skip(1).map(PathBuf::from).find(|p| p.extension().is_some_and(|e| e == "cadet"));
        if let Some(doc) = arg.as_ref().and_then(|p| Document::load(p).ok()) {
            (app.doc, app.path) = (doc, arg);
            app.rebuild();
        } else if let Some((doc, path, dirty)) = (persist && app.settings.restore_session).then(files::restore).flatten() {
            (app.doc, app.path, app.dirty) = (doc, path, dirty);
            app.rebuild();
            files::restored_ok();
        }
        app.camera = app.home();
        app
    }

    fn title(&self) -> String {
        let name = self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or("Untitled".into());
        if self.dirty { format!("{name} •") } else { name }
    }

    /// Asks the kernel to re-evaluate the timeline (rolled back while a sketch is open).
    fn rebuild(&mut self) {
        match &self.mode {
            Mode::Sketch(ed) => self.kernel.evaluate(&self.doc, ed.index, None),
            Mode::Feature(fe) => {
                let d = fe.preview(&self.doc);
                self.kernel.evaluate(&d, d.features.len(), fe.is_round().then(|| fe.position(&self.doc)));
            }
            _ => self.kernel.evaluate(&self.doc, self.doc.features.len(), None),
        }
    }

    fn kernel_events(&mut self, ctx: &egui::Context) {
        for ev in self.kernel.poll() {
            match ev {
                Event::Model(m) => {
                    let fresh = m.errors.iter().find(|(id, _)| !self.model.errors.iter().any(|(old, _)| old == id));
                    let draft = matches!(self.mode, Mode::Feature(_));
                    if let Some((id, msg)) = fresh.filter(|_| !draft) {
                        let name = self.doc.index_of(*id).map(|i| self.doc.features[i].name.clone()).unwrap_or_default();
                        self.notify(ctx, format!("{name}: {msg}"));
                    }
                    self.model = m;
                    self.upload();
                    for a in std::mem::take(&mut self.after_model) {
                        self.run(ctx, a);
                    }
                }
                Event::Hit(hit) => {
                    self.last_hit = hit;
                    if let Mode::Measure(m) = &mut self.mode {
                        m.face_hit = hit.map(|h| h.point);
                    }
                }
                Event::Exported(Ok(())) => match self.open_after_export.take() {
                    Some((path, app)) => self.launch(ctx, &path, app.as_deref()),
                    None => self.notify(ctx, "Exported".into()),
                },
                Event::Exported(Err(e)) => self.notify(ctx, format!("Export failed: {e}")),
                Event::Crashed(msg) => self.notify(ctx, msg),
            }
        }
    }

    fn upload(&mut self) {
        let t = &self.model.mesh;
        let mesh = MeshData {
            vertices: t.positions.iter().zip(&t.normals).map(|(&pos, &normal)| Vertex { pos, normal }).collect(),
            indices: t.indices.clone(),
            edges: t.edges.clone(),
        };
        self.viewport.set_mesh(&self.render.device, &mesh);
    }

    /// Records the state before an edit so it can be undone.
    fn checkpoint(&mut self) {
        self.history.push(self.doc.clone());
        self.future.clear();
        self.dirty = true;
    }

    fn changed(&mut self) {
        self.rebuild();
        if self.persist {
            files::autosave(&self.doc, self.path.as_deref(), self.dirty);
        }
    }

    fn home(&self) -> Camera {
        let mut pts: Vec<Vec3> = self.model.mesh.positions.iter().map(|&p| Vec3::from(p)).collect();
        for f in self.doc.features.iter().filter(|f| f.visible) {
            if let Kind::Sketch { plane, sketch } = &f.kind {
                pts.extend(sketch.points.iter().map(|&p| plane.world(sketch.pt(p)).as_vec3()));
            }
        }
        let (lo, hi) = pts.into_iter().fold((Vec3::splat(-25.0), Vec3::splat(25.0)), |(lo, hi), p| (lo.min(p), hi.max(p)));
        Camera::iso(Vec3::ZERO, 1.0).fit(lo, hi)
    }

    fn edit(&mut self, i: usize) {
        self.finish_mode();
        match &self.doc.features[i].kind {
            Kind::Sketch { plane, sketch } => {
                let (yaw, pitch) = sketch::plane_view(plane);
                self.goal = Some(self.camera.looking(yaw, pitch));
                let ed = Editor::new(i, sketch);
                self.checkpoint();
                self.mode = Mode::Sketch(Box::new(ed));
            }
            _ => self.mode = Mode::Feature(Box::new(FeatureEdit::edit(&self.doc, i))),
        }
        self.rebuild();
    }

    fn start_sketch(&mut self, plane: Plane) {
        self.checkpoint();
        let i = self.doc.add(Kind::Sketch { plane, sketch: Sketch::new() });
        let (yaw, pitch) = sketch::plane_view(&plane);
        self.goal = Some(self.camera.looking(yaw, pitch));
        self.mode = Mode::Sketch(Box::new(Editor::new(i, &Sketch::new())));
        self.changed();
    }

    /// Leaves sketch or feature mode, keeping sketch edits and discarding feature drafts.
    fn finish_mode(&mut self) {
        if matches!(std::mem::replace(&mut self.mode, Mode::Model), Mode::Sketch(_)) {
            self.changed();
        }
    }

    fn delete_feature(&mut self, ctx: &egui::Context, i: usize) {
        self.finish_mode();
        self.checkpoint();
        let name = self.doc.features[i].name.clone();
        let n = self.doc.remove(i);
        self.changed();
        let msg = if n > 1 { format!("Deleted {name} and {} dependent feature(s)", n - 1) } else { format!("Deleted {name}") };
        self.notify(ctx, msg);
    }

    fn run(&mut self, ctx: &egui::Context, action: Action) {
        let view = |yaw: f32, pitch: f32| Some(self.camera.looking(yaw, pitch));
        match action {
            Action::Home => self.goal = Some(self.home()),
            Action::Front => self.goal = view(-FRAC_PI_2, 0.0),
            Action::Right => self.goal = view(0.0, 0.0),
            Action::Top => self.goal = view(-FRAC_PI_2, FRAC_PI_2 - 1e-3),
            Action::ToggleGrid => self.show_grid ^= true,
            Action::ToggleEdges => self.show_edges ^= true,
            Action::ToggleTheme => ctx.set_theme(if ctx.theme() == Theme::Dark { Theme::Light } else { Theme::Dark }),
            Action::Palette => self.palette.toggle(),
            Action::Sketch => {
                self.finish_mode();
                self.last_hit = None;
                self.mode = Mode::PickPlane(None);
            }
            Action::Finish => self.finish_mode(),
            Action::Extrude | Action::Revolve => {
                self.finish_mode();
                match FeatureEdit::new(&self.doc, action == Action::Revolve) {
                    Some(fe) => {
                        self.mode = Mode::Feature(Box::new(fe));
                        self.rebuild();
                    }
                    None => self.notify(ctx, "Draw a closed profile in a sketch first".into()),
                }
            }
            Action::Fillet | Action::Chamfer => {
                self.finish_mode();
                if self.model.bodies == 0 {
                    return self.notify(ctx, "Make a solid first, then pick its edges".into());
                }
                self.mode = Mode::Feature(Box::new(FeatureEdit::round(action == Action::Chamfer)));
                self.rebuild();
            }
            Action::Measure => {
                self.finish_mode();
                self.mode = Mode::Measure(Box::default());
            }
            Action::Export => self.export(ctx, false),
            Action::New => {
                if self.settle_unsaved(ctx) {
                    self.load(Document::default(), None);
                }
            }
            Action::Quit => {
                if self.settle_unsaved(ctx) {
                    self.quitting = true;
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
            Action::Shortcuts => self.show_shortcuts ^= true,
            Action::Settings => self.show_settings = true,
            Action::About => self.show_about = true,
            Action::Console => {
                self.python.open ^= true;
                if self.python.open {
                    self.python.focus_input();
                }
            }
            Action::OpenPlugins => {
                if let Some(dir) = python::plugin_dir() {
                    let _ = std::fs::create_dir_all(&dir);
                    self.launch(ctx, &dir, None);
                }
            }
            Action::ReloadPlugins => {
                let failed = self.python.reload();
                let n = self.python.plugins().len();
                self.notify(ctx, if failed > 0 { format!("{n} plugins loaded, {failed} failed (see the console)") } else { format!("{n} plugins loaded") });
            }
            Action::ExportStep => self.export(ctx, true),
            Action::Open => {
                if let Some(path) = files::pick_open() {
                    self.open(ctx, path);
                }
            }
            Action::Save | Action::SaveAs => self.save(ctx, action == Action::SaveAs),
            Action::Undo | Action::Redo if !matches!(self.mode, Mode::Sketch(_)) => {
                let (from, to) = if action == Action::Undo { (&mut self.history, &mut self.future) } else { (&mut self.future, &mut self.history) };
                if let Some(doc) = from.pop() {
                    to.push(std::mem::replace(&mut self.doc, doc));
                    self.mode = Mode::Model;
                    self.dirty = true;
                    self.changed();
                }
            }
            a => self.run_sketch(ctx, a),
        }
    }

    /// Asks what to do with unsaved changes. Returns false if the user cancelled.
    fn settle_unsaved(&mut self, ctx: &egui::Context) -> bool {
        if !self.dirty || !self.persist {
            return true;
        }
        match files::ask_unsaved(self.title().trim_end_matches(" •")) {
            files::Unsaved::Save => {
                self.save(ctx, false);
                !self.dirty
            }
            files::Unsaved::Discard => {
                self.dirty = false;
                files::autosave(&self.saved_version(), self.path.as_deref(), false);
                true
            }
            files::Unsaved::Cancel => false,
        }
    }

    /// What's on disk, for forgetting unsaved edits.
    fn saved_version(&self) -> Document {
        self.path.as_deref().and_then(|p| Document::load(p).ok()).unwrap_or_default()
    }

    fn open(&mut self, ctx: &egui::Context, path: PathBuf) {
        if !self.settle_unsaved(ctx) {
            return;
        }
        match Document::load(&path) {
            Ok(doc) => self.load(doc, Some(path)),
            Err(e) => self.notify(ctx, format!("Couldn't open: {e}")),
        }
    }

    fn load(&mut self, doc: Document, path: Option<PathBuf>) {
        self.mode = Mode::Model;
        (self.doc, self.path, self.dirty) = (doc, path, false);
        self.history.clear();
        self.future.clear();
        self.changed();
        self.goal = Some(self.home());
    }

    fn save(&mut self, ctx: &egui::Context, ask: bool) {
        self.finish_mode();
        let Some(path) = self.path.clone().filter(|_| !ask).or_else(|| files::pick_save(self.path.as_deref())) else { return };
        match self.doc.save(&path) {
            Ok(()) => {
                self.path = Some(path);
                self.dirty = false;
                self.changed();
                self.notify(ctx, format!("Saved {}", self.title()));
            }
            Err(e) => self.notify(ctx, format!("Couldn't save: {e}")),
        }
    }

    fn export(&mut self, ctx: &egui::Context, step: bool) {
        self.finish_mode();
        if self.model.bodies == 0 {
            return self.notify(ctx, "Nothing to export yet. Extrude a sketch first".into());
        }
        if let Some(path) = files::pick_export(self.path.as_deref(), step) {
            self.kernel.export(path, step, self.settings.stl_quality.tolerance());
        }
    }

    fn run_sketch(&mut self, ctx: &egui::Context, a: Action) {
        let Mode::Sketch(ed) = &mut self.mode else {
            return self.notify(ctx, format!("{} is coming in a later milestone", commands::get(a).label));
        };
        let i = ed.index;
        let Kind::Sketch { sketch: sk, .. } = &mut self.doc.features[i].kind else { return };
        match a {
            Action::Select => ed.set_tool(Tool::Select),
            Action::Line => ed.set_tool(Tool::Line),
            Action::Rect => ed.set_tool(Tool::Rect),
            Action::Circle => ed.set_tool(Tool::Circle),
            Action::Arc => ed.set_tool(Tool::Arc),
            Action::Dimension => ed.set_tool(Tool::Dimension),
            Action::Undo => ed.undo(sk),
            Action::Redo => ed.redo(sk),
            Action::Delete => ed.delete(sk),
            Action::Construction => ed.toggle_construction(sk),
            a => ed.constrain(sk, a),
        }
    }

    /// Builds the macOS menu bar from the command registry and returns this frame's picks.
    #[cfg(target_os = "macos")]
    fn native_menu(&mut self, plugins: &[String], sketch: bool) -> Vec<Choice> {
        use crate::macos::{self, Item};
        const PLUGIN_TAG: isize = 10_000;
        if self.menu_plugins.as_deref() != Some(plugins) {
            let pick = |a: Action| {
                let i = commands::COMMANDS.iter().position(|c| c.action == a).unwrap();
                let c = &commands::COMMANDS[i];
                let (key, shift, alt) = match c.key {
                    Some((m, k)) if m.command => (mac_key(k), m.shift, m.alt),
                    _ => (String::new(), false, false),
                };
                Item::Pick { label: c.label.trim_start_matches("View: ").to_string(), tag: i as isize, key, shift, alt }
            };
            let sys = |label: &str, selector, key, alt| Item::System { label: label.into(), selector, key, alt };
            let app = vec![
                pick(Action::About),
                Item::Separator,
                pick(Action::Settings),
                Item::Separator,
                sys("Hide Cadet", objc2::sel!(hide:), "h", false),
                sys("Hide Others", objc2::sel!(hideOtherApplications:), "h", true),
                sys("Show All", objc2::sel!(unhideAllApplications:), "", false),
                Item::Separator,
                pick(Action::Quit),
            ];
            let mut menus = vec![("Cadet", app)];
            for (name, groups) in commands::MENUS {
                let mut items = vec![];
                if *name == "Plugins" {
                    items.extend(plugins.iter().enumerate().map(|(i, l)| Item::Pick { label: l.clone(), tag: PLUGIN_TAG + i as isize, key: String::new(), shift: false, alt: false }));
                    items.push(Item::Separator);
                }
                for group in *groups {
                    let group: Vec<Action> = group.iter().copied().filter(|a| !matches!(a, Action::Quit | Action::Settings | Action::About)).collect();
                    if group.is_empty() {
                        continue;
                    }
                    if !items.is_empty() && !matches!(items.last(), Some(Item::Separator)) {
                        items.push(Item::Separator);
                    }
                    items.extend(group.into_iter().map(pick));
                }
                menus.push((*name, items));
            }
            menus.insert(
                menus.len() - 1,
                ("Window", vec![sys("Minimize", objc2::sel!(performMiniaturize:), "m", false), sys("Zoom", objc2::sel!(performZoom:), "", false), Item::Separator, sys("Bring All to Front", objc2::sel!(arrangeInFront:), "", false)]),
            );
            macos::set_menus(&menus);
            self.menu_plugins = Some(plugins.to_vec());
            self.dump_menu = std::env::var_os("CADET_DUMP_MENU").is_some();
        }
        macos::set_enabled(|tag| tag >= PLUGIN_TAG || commands::COMMANDS.get(tag as usize).is_some_and(|c| commands::in_scope(c, sketch)));
        if std::mem::take(&mut self.dump_menu) {
            eprint!("{}", macos::dump());
        }
        macos::take_picked()
            .into_iter()
            .filter_map(|tag| match tag {
                t if t >= PLUGIN_TAG => Some(Choice::Plugin((t - PLUGIN_TAG) as usize)),
                t => commands::COMMANDS.get(t as usize).map(|c| Choice::Action(c.action)),
            })
            .collect()
    }

    /// Applies what a Python script did: its document as one undoable step, then its requests.
    fn apply_script(&mut self, ctx: &egui::Context, out: Option<cadet_script::Outcome>) {
        let Some(out) = out else { return };
        if out.error.is_some() {
            self.notify(ctx, "Python error, see the console".into());
        }
        let changed = out.doc.is_some();
        if let Some(doc) = out.doc {
            self.finish_mode();
            self.checkpoint();
            self.doc = doc;
            self.changed();
        }
        for r in out.requests {
            match r {
                cadet_script::Request::Command(id) => match commands::by_id(&id) {
                    Some(a) if changed => self.after_model.push(a),
                    Some(a) => self.run(ctx, a),
                    None => self.notify(ctx, format!("Unknown command '{id}'")),
                },
                cadet_script::Request::Export { path, step } => {
                    self.kernel.export(path, step, self.settings.stl_quality.tolerance());
                    self.exporting = true;
                }
                cadet_script::Request::Open { path, app } if self.exporting => self.open_after_export = Some((path, app)),
                cadet_script::Request::Open { path, app } => self.launch(ctx, &path, app.as_deref()),
            }
        }
        self.exporting = false;
    }

    fn launch(&mut self, ctx: &egui::Context, path: &std::path::Path, app: Option<&str>) {
        if let Err(e) = files::launch(path, app) {
            self.notify(ctx, format!("Couldn't open {}: {e}", path.display()));
        }
    }

    fn notify(&mut self, ctx: &egui::Context, msg: String) {
        self.toast = Some((msg, ctx.input(|i| i.time)));
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let p = theme::palette(ui.ctx());
        let mut action = None;
        let frame = egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::symmetric(12, 0));
        egui::Panel::top("top").exact_size(TOPBAR_H).frame(frame).show(ui, |ui| {
            let bar = ui.max_rect();
            let drag = ui.interact(bar, Id::unique("titlebar"), Sense::click_and_drag());
            if drag.drag_started_by(PointerButton::Primary) {
                ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
            }
            if drag.double_clicked() {
                let max = ui.input(|i| i.viewport().maximized.unwrap_or(false));
                ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!max));
            }
            ui.horizontal_centered(|ui| {
                if cfg!(target_os = "macos") {
                    ui.add_space(68.0);
                }
                ui.label(RichText::new(icons::CUBE).size(18.0).color(p.accent));
                ui.label(RichText::new("Cadet").heading());
                ui.label(RichText::new("/").color(p.weak));
                ui.label(RichText::new(self.title()).color(p.weak));
                ui.add_space(10.0);
                #[cfg(not(target_os = "macos"))]
                {
                    let labels: Vec<String> = self.python.plugins().iter().map(|p| p.label.clone()).collect();
                    match menu_bar(ui, matches!(self.mode, Mode::Sketch(_)), &labels) {
                        Some(Choice::Action(a)) => action = Some(a),
                        Some(Choice::Plugin(i)) => self.plugin_pick = Some(i),
                        None => {}
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let dark = ui.visuals().dark_mode;
                    if icon_button(ui, if dark { icons::SUN } else { icons::MOON }, "Toggle light/dark").clicked() {
                        action = Some(Action::ToggleTheme);
                    }
                    if icon_button(ui, icons::TERMINAL, "Python console (⌘J)").clicked() {
                        action = Some(Action::Console);
                    }

                    let w = 260.0f32.min(ui.available_width() - 20.0).max(0.0);
                    let r = ui.add_sized([w, 28.0], egui::Button::new("").fill(p.card).stroke(Stroke::new(1.0, p.line)).corner_radius(8));
                    let painter = ui.painter();
                    let inner = r.rect.shrink2(vec2(10.0, 0.0));
                    painter.text(inner.left_center(), Align2::LEFT_CENTER, format!("{}  Search commands", icons::SEARCH), FontId::proportional(13.0), p.weak);
                    let keys = commands::shortcut_text(commands::get(Action::Palette));
                    painter.text(inner.right_center(), Align2::RIGHT_CENTER, keys, FontId::proportional(12.0), p.weak);
                    if r.clicked() {
                        action = Some(Action::Palette);
                    }
                });
            });
        });
        action
    }

    fn viewport_ui(&mut self, ui: &mut egui::Ui) -> Rect {
        let rect = ui.max_rect();
        let resp = ui.interact(rect, Id::unique("viewport"), Sense::click_and_drag());
        let ctx = ui.ctx().clone();
        let plane = match &self.mode {
            Mode::Sketch(ed) => match &self.doc.features[ed.index].kind {
                Kind::Sketch { plane, .. } => *plane,
                _ => Plane::XY,
            },
            _ => Plane::XY,
        };
        let view = View { rect, camera: self.camera, plane };

        let mut start = None;
        let mut consumed = false;
        match &mut self.mode {
            Mode::Sketch(ed) => {
                let Kind::Sketch { sketch: sk, .. } = &mut self.doc.features[ed.index].kind else { unreachable!() };
                ed.divisions = self.divisions;
                ed.model_snaps = model_snaps(&self.model.edges, &plane, self.divisions);
                consumed = ed.input(ui, &resp, &view, sk);
            }
            Mode::PickPlane(hover) => {
                *hover = resp.hover_pos().and_then(|pos| {
                    let (o, d) = view.ray(pos);
                    if self.model.bodies > 0 {
                        self.kernel.ray(o.as_dvec3(), d.as_dvec3());
                    }
                    let std = sketch::pick_plane(&view, pos).map(|(t, i)| (t, PlanePick::Standard(i)));
                    let face = self.last_hit.and_then(|h| h.plane.map(|f| (h.t, PlanePick::Face(Plane::from_frame(f), h.point))));
                    face.or(std).map(|h| h.1)
                });
                if resp.clicked_by(PointerButton::Primary) {
                    start = *hover;
                }
            }
            Mode::Feature(fe) => {
                let edges = self.model.base_edges.as_deref().unwrap_or(&self.model.edges);
                consumed = fe.drag_handle(ui, &resp, &view, &self.doc, edges, model_center(&self.model));
                if let Some(pos) = resp.interact_pointer_pos().filter(|_| !consumed && resp.clicked_by(PointerButton::Primary)) {
                    let edges = self.model.base_edges.as_deref().unwrap_or(&self.model.edges);
                    fe.click(&self.doc, &view, pos, edges);
                }
            }
            Mode::Measure(m) => {
                let pos = resp.hover_pos();
                if let Some(pos) = pos.filter(|_| self.model.bodies > 0) {
                    let (o, d) = view.ray(pos);
                    self.kernel.ray(o.as_dvec3(), d.as_dvec3());
                }
                m.hover(&view, pos, &self.model.edges, self.divisions);
                if resp.clicked_by(PointerButton::Primary) {
                    m.click();
                }
            }
            Mode::Model => {}
        }
        match start {
            Some(PlanePick::Standard(i)) => self.start_sketch(Plane::STANDARD[i]),
            Some(PlanePick::Face(p, _)) => self.start_sketch(p),
            None => {}
        }

        let d = resp.drag_delta();
        let shift = ui.input(|i| i.modifiers.shift);
        if consumed {
        } else if resp.dragged_by(PointerButton::Primary) && !shift {
            self.camera.orbit(d.x, d.y);
            self.goal = None;
        } else if resp.dragged() {
            self.camera.pan(d.x, d.y, rect.height());
            self.goal = None;
        }
        if resp.hovered() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            if scroll != 0.0 || zoom != 1.0 {
                self.camera.zoom(scroll);
                self.camera.dist /= zoom;
                if let Some(g) = &mut self.goal {
                    g.dist = self.camera.dist;
                }
            }
        }
        if resp.double_clicked() && matches!(self.mode, Mode::Model) {
            self.goal = Some(self.home());
        }
        if let Some(goal) = self.goal {
            let t = 1.0 - (-ui.input(|i| i.stable_dt) * 14.0).exp();
            if !self.camera.approach(&goal, t) {
                self.goal = None;
            }
            ctx.request_repaint();
        }

        let rs = self.render.clone();
        let ppp = ctx.pixels_per_point();
        let size = [(rect.width() * ppp).round() as u32, (rect.height() * ppp).round() as u32];
        let mut style = theme::viewport_style(ui.visuals().dark_mode, self.show_grid, self.show_edges);
        let axes = [style.axis_x, style.axis_y, theme::AXIS_Z];
        let (iu, iv) = sketch::plane_axes(&plane);
        (style.axis_x, style.axis_y) = (axes[iu], axes[iv]);
        style.grid_plane = [plane.o.as_vec3().into(), plane.u.as_vec3().into(), plane.v.as_vec3().into()];
        let (tex, recreated) = self.viewport.render(&rs.device, &rs.queue, size, &self.camera, &style);
        let mut renderer = rs.renderer.write();
        let id = match self.texture {
            Some(id) if recreated => {
                renderer.update_egui_texture_from_wgpu_texture(&rs.device, tex, wgpu_filter(), id);
                id
            }
            Some(id) => id,
            None => *self.texture.insert(renderer.register_native_texture(&rs.device, tex, wgpu_filter())),
        };
        drop(renderer);
        ui.painter().image(id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);

        let view = View { rect, camera: self.camera, plane };
        let painter = ui.painter_at(rect);
        let p = theme::palette(&ctx);
        let active = match &self.mode {
            Mode::Sketch(ed) => Some(ed.index),
            _ => None,
        };
        let feature_mode = matches!(self.mode, Mode::Feature(_));
        for (i, f) in self.doc.features.iter().enumerate() {
            if let Kind::Sketch { plane, sketch } = &f.kind
                && f.visible
                && Some(i) != active
                && !feature_mode
            {
                sketch::draw_passive(&painter, &View { plane: *plane, ..view }, sketch, p);
            }
        }
        match &mut self.mode {
            Mode::Sketch(ed) => {
                let Kind::Sketch { sketch: sk, .. } = &mut self.doc.features[ed.index].kind else { unreachable!() };
                ed.draw(ui, &painter, &view, sk);
            }
            Mode::PickPlane(hover) => {
                let std = match hover {
                    Some(PlanePick::Standard(i)) => Some(*i),
                    _ => None,
                };
                sketch::draw_planes(&painter, &view, std, p);
                if let Some(PlanePick::Face(plane, at)) = hover {
                    sketch::draw_face_hint(&painter, &View { plane: *plane, ..view }, plane, *at, p);
                }
            }
            Mode::Feature(fe) => fe.draw(&painter, &view, &self.doc, p, self.model.base_edges.as_deref().unwrap_or(&self.model.edges), model_center(&self.model)),
            Mode::Measure(m) => m.draw(&painter, &view, p),
            Mode::Model => {}
        }
        rect
    }

    fn toolbar(&mut self, ctx: &egui::Context, vp: Rect) -> Option<Action> {
        let mut action = None;
        let tool = if let Mode::Sketch(ed) = &self.mode { Some(ed.tool) } else { None };
        floating(ctx, "toolbar", Align2::CENTER_TOP, vp.center_top() + vec2(0.0, GAP), |ui| {
            ui.horizontal(|ui| {
                use Action::*;
                let groups: &[&[Action]] = if tool.is_some() {
                    &[&[Select], &[Line, Rect, Circle, Arc], &[Dimension], &[Horizontal, Vertical, Parallel, Perpendicular, Equal, Tangent, Coincident, Fix, Construction], &[Finish]]
                } else {
                    &[&[Sketch], &[Extrude, Revolve, Fillet, Chamfer], &[Measure, Export]]
                };
                for (i, group) in groups.iter().enumerate() {
                    if i > 0 {
                        ui.separator();
                    }
                    for &a in *group {
                        let c = commands::get(a);
                        let tip = format!("{}  {}", c.label, commands::shortcut_text(c));
                        let lit = a == Sketch || a == Finish || tool.is_some_and(|t| tool_of(a) == Some(t));
                        let compact = matches!(a, Horizontal | Vertical | Parallel | Perpendicular | Equal | Tangent | Coincident | Fix | Construction);
                        let r = tool_button(ui, c.icon, if compact { "" } else { c.label }, lit).on_hover_text(tip);
                        if r.clicked() {
                            action = Some(a);
                        }
                    }
                }
            });
        });
        action
    }

    fn feature_icon(kind: &Kind) -> &'static str {
        match kind {
            Kind::Sketch { .. } => icons::PENCIL,
            Kind::Extrude { .. } => icons::EXTRUDE,
            Kind::Revolve { .. } => icons::REVOLVE,
            Kind::Round { chamfer: false, .. } => icons::FILLET,
            Kind::Round { chamfer: true, .. } => icons::CHAMFER,
        }
    }

    fn error_of(&self, id: u32) -> Option<&str> {
        self.model.errors.iter().find(|e| e.0 == id).map(|e| e.1.as_str())
    }

    fn browser(&mut self, ctx: &egui::Context, vp: Rect) -> Option<usize> {
        let p = theme::palette(ctx);
        let active = if let Mode::Sketch(ed) = &self.mode { Some(ed.index) } else { None };
        let mut edit = None;
        let mut toggled = None;
        floating(ctx, "browser", Align2::LEFT_TOP, vp.left_top() + vec2(GAP, GAP), |ui| {
            ui.set_width(220.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(icons::STACK).color(p.weak));
                ui.label(RichText::new("Browser").heading());
            });
            ui.add_space(2.0);
            let bodies = self.model.bodies;
            ui.label(RichText::new(format!("{} {bodies} bod{}", icons::BODY, if bodies == 1 { "y" } else { "ies" })).color(p.weak));
            egui::CollapsingHeader::new("Features").default_open(true).show(ui, |ui| {
                if self.doc.features.is_empty() {
                    ui.label(RichText::new("Nothing yet. Press S to sketch").color(p.weak).small());
                }
                for (i, f) in self.doc.features.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let err = self.error_of(f.id);
                        let col = if err.is_some() { p.error() } else if Some(i) == active { p.accent } else { p.weak };
                        ui.label(RichText::new(Self::feature_icon(&f.kind)).color(col));
                        let r = ui.add(egui::Label::new(f.name.as_str()).sense(Sense::click())).on_hover_text(err.unwrap_or("Double-click to edit"));
                        if r.double_clicked() {
                            edit = Some(i);
                        }
                        if let Kind::Sketch { plane, .. } = &f.kind {
                            ui.label(RichText::new(plane.name()).color(p.weak).small());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if icon_button(ui, if f.visible { icons::EYE } else { icons::EYE_OFF }, "Show/hide").clicked() {
                                    toggled = Some(i);
                                }
                            });
                        }
                    });
                }
            });
        });
        if let Some(i) = toggled {
            self.doc.features[i].visible ^= true;
        }
        edit
    }

    fn view_gizmo(&mut self, ctx: &egui::Context, vp: Rect) -> Option<Action> {
        let size = 92.0;
        let center = vp.right_top() + vec2(-GAP - size * 0.5, GAP + size * 0.5);
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, Id::unique("gizmo")));
        let (right, up, fwd) = self.camera.basis();
        let mut axes = [
            (Vec3::X, "X", Color32::from_rgb(0xe5, 0x48, 0x4d), Action::Right),
            (Vec3::Y, "Y", Color32::from_rgb(0x46, 0xa7, 0x58), Action::Front),
            (Vec3::Z, "Z", Color32::from_rgb(0x3e, 0x8e, 0xf7), Action::Top),
        ]
        .into_iter()
        .flat_map(|(v, l, c, a)| [(v, l, c, a, true), (-v, l, c, a, false)])
        .collect::<Vec<_>>();
        axes.sort_by(|a, b| b.0.dot(fwd).total_cmp(&a.0.dot(fwd)));

        let p = theme::palette(ctx);
        painter.circle_filled(center, size * 0.5, p.card.gamma_multiply(0.6));
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let clicked = ctx.input(|i| i.pointer.primary_clicked());
        let mut action = None;
        for (v, label, color, a, positive) in axes {
            let tip = center + vec2(v.dot(right), -v.dot(up)) * size * 0.34;
            let r = if positive { 10.0 } else { 7.0 };
            let hot = pointer.is_some_and(|q| q.distance(tip) < r + 2.0);
            if positive {
                painter.line_segment([center, tip], Stroke::new(2.0, color));
                painter.circle_filled(tip, r, if hot { color.gamma_multiply(1.3) } else { color });
                painter.text(tip, Align2::CENTER_CENTER, label, FontId::new(11.0, egui::FontFamily::Name("bold".into())), Color32::WHITE);
            } else {
                painter.circle(tip, r, color.gamma_multiply(if hot { 0.7 } else { 0.25 }), Stroke::new(1.0, color));
            }
            if hot && clicked {
                self.goal = Some(match (a, positive) {
                    (Action::Top, true) => self.camera.looking(-FRAC_PI_2, FRAC_PI_2 - 1e-3),
                    (Action::Top, false) => self.camera.looking(-FRAC_PI_2, -FRAC_PI_2 + 1e-3),
                    (Action::Right, pos) => self.camera.looking(if pos { 0.0 } else { std::f32::consts::PI }, 0.0),
                    (_, pos) => self.camera.looking(if pos { FRAC_PI_2 } else { -FRAC_PI_2 }, 0.0),
                });
            }
        }
        let home = center + vec2(-size * 0.5, size * 0.5);
        let r = egui::Area::new(Id::unique("home")).fixed_pos(home - vec2(0.0, 8.0)).show(ctx, |ui| icon_button(ui, icons::HOME, "Home view (0)"));
        if r.inner.clicked() {
            action = Some(Action::Home);
        }
        action
    }

    fn timeline(&mut self, ctx: &egui::Context, vp: Rect, actions: &mut Vec<Action>) -> (Option<usize>, Option<usize>) {
        let p = theme::palette(ctx);
        let rolled = match &self.mode {
            Mode::Sketch(ed) => Some(ed.index),
            _ => None,
        };
        let (mut edit, mut delete) = (None, None);
        floating(ctx, "timeline", Align2::CENTER_BOTTOM, vp.center_bottom() - vec2(0.0, GAP), |ui| {
            ui.horizontal(|ui| {
                if icon_button(ui, icons::UNDO, "Undo").clicked() {
                    actions.push(Action::Undo);
                }
                if icon_button(ui, icons::REDO, "Redo").clicked() {
                    actions.push(Action::Redo);
                }
                ui.separator();
                if self.doc.features.is_empty() {
                    ui.label(RichText::new("Timeline").color(p.weak));
                }
                for (i, f) in self.doc.features.iter().enumerate() {
                    let on = Some(i) == rolled;
                    let after = rolled.is_some_and(|r| i > r);
                    let err = self.error_of(f.id);
                    let fg = if on { p.on_accent } else if err.is_some() { p.error() } else if after { p.weak } else { p.text };
                    let chip = egui::Button::new(RichText::new(format!("{}  {}", Self::feature_icon(&f.kind), f.name)).color(fg))
                        .fill(if on { p.accent } else { p.hover })
                        .corner_radius(6);
                    let r = ui.add(chip).on_hover_text(err.unwrap_or("Double-click to edit · right-click for more"));
                    if r.double_clicked() {
                        edit = Some(i);
                    }
                    r.context_menu(|ui| {
                        if ui.button("Edit").clicked() {
                            edit = Some(i);
                        }
                        if ui.button("Delete").clicked() {
                            delete = Some(i);
                        }
                    });
                }
                ui.add_space(2.0);
                ui.label(RichText::new("▏").color(p.accent));
            });
        });
        (edit, delete)
    }

    fn feature_panel(&mut self, ctx: &egui::Context, vp: Rect) {
        let at = vp.right_top() + vec2(-GAP, GAP + 150.0);
        if let Mode::Measure(m) = &self.mode {
            floating(ctx, "measure_panel", Align2::RIGHT_TOP, at, |ui| m.panel(ui, &self.model.edges, self.model.props.as_ref()));
            return;
        }
        let Mode::Feature(fe) = &mut self.mode else { return };
        let was_typing = ctx.memory(|m| m.focused().is_some());
        let id = match fe.index {
            Some(i) => self.doc.features[i].id,
            None => fe.preview(&self.doc).features.last().map_or(0, |f| f.id),
        };
        let error = self.model.errors.iter().find(|e| e.0 == id).map(|e| e.1.clone());
        let out = floating(ctx, "feature_panel", Align2::RIGHT_TOP, at, |ui| fe.panel(ui, &self.doc, error.as_deref()));
        let enter = !was_typing && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let out = out.or(enter.then_some(Outcome::Ok));
        let changed = std::mem::take(&mut fe.changed);
        match out {
            Some(Outcome::Ok) => {
                let Mode::Feature(fe) = std::mem::replace(&mut self.mode, Mode::Model) else { unreachable!() };
                self.checkpoint();
                fe.commit(&mut self.doc);
                self.changed();
            }
            Some(Outcome::Cancel) => {
                self.mode = Mode::Model;
                self.rebuild();
            }
            None if changed => self.rebuild(),
            None => {}
        }
    }

    fn status(&mut self, ctx: &egui::Context, vp: Rect, frame: &eframe::Frame) {
        let p = theme::palette(ctx);
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, Id::unique("status")));
        let ms = frame.info().cpu_usage.unwrap_or(0.0) * 1000.0;
        let busy = if self.kernel.busy() { "computing…" } else { "" };
        let text = format!("{busy}   mm   ·   model {:.0} ms   ·   frame {ms:.1} ms", self.model.millis);
        painter.text(vp.right_bottom() - vec2(GAP + 4.0, GAP + 8.0), Align2::RIGHT_BOTTOM, text, FontId::proportional(11.0), p.weak);

        let card = match &self.mode {
            Mode::Sketch(ed) => {
                let f = &self.doc.features[ed.index];
                let plane = if let Kind::Sketch { plane, .. } = &f.kind { plane.name() } else { "" };
                let (col, state) = match ed.dof() {
                    0 => (p.ok, "Fully constrained".to_string()),
                    1 => (p.free, "1 degree of freedom".to_string()),
                    n => (p.free, format!("{n} degrees of freedom")),
                };
                Some((format!("{} on {plane}", f.name), col, state, ed.hint(), format!("Snap: {} · {}1–{}9", snap::division_name(self.divisions), icons::COMMAND, icons::COMMAND)))
            }
            Mode::PickPlane(_) => Some(("New sketch".into(), p.accent, "Pick a plane or a flat face".into(), "Click to start · Esc to cancel", String::new())),
            _ => None,
        };
        if let Some((title, col, state, hint, extra)) = card {
            floating(ctx, "sketch_status", Align2::LEFT_BOTTOM, vp.left_bottom() + vec2(GAP, -GAP), |ui| {
                ui.set_max_width(280.0);
                ui.label(RichText::new(title).heading());
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 4.0, col);
                    ui.label(RichText::new(state).color(col));
                });
                ui.label(RichText::new(hint).color(p.weak).small());
                if !extra.is_empty() {
                    ui.label(RichText::new(extra).color(p.weak).small());
                }
            });
        }

        if let Some((msg, t0)) = &self.toast {
            let age = ctx.input(|i| i.time) - t0;
            if age > 3.0 {
                self.toast = None;
                return;
            }
            let alpha = (3.0 - age).clamp(0.0, 0.4) as f32 / 0.4;
            floating(ctx, "toast", Align2::CENTER_BOTTOM, vp.center_bottom() - vec2(0.0, GAP + 56.0), |ui| {
                ui.set_opacity(alpha);
                ui.label(RichText::new(msg.as_str()).color(p.text));
            });
            ctx.request_repaint();
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting && !self.settle_unsaved(&ctx) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        }
        self.kernel_events(&ctx);
        #[cfg(target_os = "macos")]
        for path in crate::macos::take_opened() {
            self.open(&ctx, path);
        }
        let in_sketch = matches!(self.mode, Mode::Sketch(_));
        if !self.palette.open && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            match &mut self.mode {
                Mode::Sketch(ed) => ed.escape(),
                Mode::Feature(_) => {
                    self.mode = Mode::Model;
                    self.rebuild();
                }
                _ => self.mode = Mode::Model,
            }
        }
        const DIGITS: [egui::Key; 9] = [egui::Key::Num1, egui::Key::Num2, egui::Key::Num3, egui::Key::Num4, egui::Key::Num5, egui::Key::Num6, egui::Key::Num7, egui::Key::Num8, egui::Key::Num9];
        if let Some(n) = (1..=9).find(|&n| ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, DIGITS[n as usize - 1]))) {
            self.divisions = n;
            self.notify(&ctx, format!("Snapping to {}", snap::division_name(n)));
        }
        let mut actions: Vec<Action> = commands::poll_shortcuts(&ctx, in_sketch).into_iter().collect();
        actions.extend(self.top_bar(ui));
        let vp = egui::CentralPanel::no_frame().show(ui, |ui| self.viewport_ui(ui)).inner;
        actions.extend(self.toolbar(&ctx, vp));
        let browse = self.browser(&ctx, vp);
        actions.extend(self.view_gizmo(&ctx, vp));
        let (edit, delete) = self.timeline(&ctx, vp, &mut actions);
        if let Some(i) = edit.or(browse) {
            self.edit(i);
        }
        if let Some(i) = delete {
            self.delete_feature(&ctx, i);
        }
        self.feature_panel(&ctx, vp);
        let labels: Vec<String> = self.python.plugins().iter().map(|p| p.label.clone()).collect();
        #[cfg(target_os = "macos")]
        for choice in self.native_menu(&labels, in_sketch) {
            match choice {
                Choice::Action(a) => actions.push(a),
                Choice::Plugin(i) => self.plugin_pick = Some(i),
            }
        }
        match self.palette.show(&ctx, in_sketch, &labels) {
            Some(Choice::Action(a)) => actions.push(a),
            Some(Choice::Plugin(i)) => self.plugin_pick = Some(i),
            None => {}
        }
        for a in actions {
            self.run(&ctx, a);
        }
        if let Some(i) = self.plugin_pick.take() {
            let out = self.python.start(i, &self.doc);
            self.apply_script(&ctx, out);
        }
        let out = self.python.form_ui(&ctx, &self.doc);
        self.apply_script(&ctx, out);
        let out = self.python.console_ui(&ctx, vp.center_bottom() - vec2(0.0, GAP + 60.0), &self.doc);
        self.apply_script(&ctx, out);
        if let Mode::Sketch(ed) = &mut self.mode
            && let Some(t) = ed.toast.take()
        {
            self.notify(&ctx, t);
        }
        self.status(&ctx, vp, frame);
        if self.show_shortcuts {
            shortcut_sheet(&ctx, &mut self.show_shortcuts);
        }
        if self.show_settings && let Some(settings::Request::OpenPluginsFolder) = settings::window(&ctx, &mut self.show_settings, &mut self.settings) {
            self.run(&ctx, Action::OpenPlugins);
        }
        if self.show_about {
            about(&ctx, &mut self.show_about);
        }
        ctx.send_viewport_cmd(ViewportCommand::Title(format!("{} — Cadet", self.title())));
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.script.feed(ctx, raw);
    }
}

fn model_center(m: &Model) -> glam::DVec3 {
    m.props.map_or(glam::DVec3::ZERO, |p| (p.min + p.max) * 0.5)
}

/// The existing model's vertices, circle centers and division points, projected into a sketch plane.
fn model_snaps(edges: &[cadet_core::kernel::EdgeRef], plane: &Plane, n: u32) -> Vec<sketch::ModelSnap> {
    let local = |q: glam::DVec3| glam::DVec2::new((q - plane.o).dot(plane.u), (q - plane.o).dot(plane.v));
    let mut out = vec![];
    for e in edges {
        let closed = e.closed();
        if !closed {
            out.extend([e.points[0], *e.points.last().unwrap()].map(|q| sketch::ModelSnap { at: local(q), marker: snap::Marker::Point }));
        }
        if let Some((c, _)) = e.circle {
            out.push(sketch::ModelSnap { at: local(c), marker: snap::Marker::Center });
        }
        out.extend(snap::divide(&e.points, n, closed).into_iter().map(|(k, q)| sketch::ModelSnap { at: local(q), marker: snap::Marker::Division(k.max(1), n) }));
    }
    out
}

/// The key-equivalent string AppKit expects for an egui key.
#[cfg(target_os = "macos")]
fn mac_key(k: egui::Key) -> String {
    use egui::Key;
    match k {
        Key::Enter => "\r".into(),
        Key::Slash => "/".into(),
        Key::Comma => ",".into(),
        k => k.name().to_lowercase(),
    }
}

fn about(ctx: &egui::Context, open: &mut bool) {
    let p = theme::palette(ctx);
    egui::Window::new("About Cadet").open(open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(icons::CUBE).size(32.0).color(p.accent));
            ui.vertical(|ui| {
                ui.label(RichText::new(format!("Cadet {}", env!("CARGO_PKG_VERSION"))).heading());
                ui.label(RichText::new("Fast, local-first parametric CAD for makers").color(p.weak));
            });
        });
        ui.add_space(6.0);
        ui.label(RichText::new("Geometry: Open CASCADE Technology · Python: RustPython · UI: egui + wgpu").small().color(p.weak));
    });
}

#[cfg(not(target_os = "macos"))]
fn menu_bar(ui: &mut egui::Ui, sketch: bool, plugins: &[String]) -> Option<Choice> {
    let mut picked = None;
    for (name, groups) in commands::MENUS {
        ui.menu_button(*name, |ui| {
            ui.set_min_width(220.0);
            if *name == "Plugins" {
                for (i, label) in plugins.iter().enumerate() {
                    if ui.button(label).clicked() {
                        picked = Some(Choice::Plugin(i));
                        ui.close();
                    }
                }
                ui.separator();
            }
            for (i, group) in groups.iter().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                for &a in *group {
                    let c = commands::get(a);
                    let button = egui::Button::new(c.label).shortcut_text(commands::shortcut_text(c));
                    if ui.add_enabled(commands::in_scope(c, sketch), button).clicked() {
                        picked = Some(Choice::Action(a));
                        ui.close();
                    }
                }
            }
        });
    }
    picked
}

fn shortcut_sheet(ctx: &egui::Context, open: &mut bool) {
    let p = theme::palette(ctx);
    egui::Window::new("Keyboard shortcuts").open(open).collapsible(false).resizable(false).default_width(560.0).show(ctx, |ui| {
        ui.columns(2, |cols| {
            for (col, (title, scope)) in [("Everywhere / model", false), ("In a sketch", true)].into_iter().enumerate() {
                let ui = &mut cols[col];
                ui.label(RichText::new(title).heading());
                egui::Grid::new(title).num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                    for c in commands::COMMANDS.iter().filter(|c| c.key.is_some() && (c.scope == commands::Scope::Sketch) == scope) {
                        ui.label(c.label);
                        ui.label(RichText::new(commands::shortcut_text(c)).color(p.weak));
                        ui.end_row();
                    }
                    if !scope {
                        ui.label("Snap: endpoints · halves · thirds …");
                        ui.label(RichText::new(format!("{}1 … {}9", icons::COMMAND, icons::COMMAND)).color(p.weak));
                        ui.end_row();
                    }
                });
            }
        });
    });
}

fn tool_of(a: Action) -> Option<Tool> {
    Some(match a {
        Action::Select => Tool::Select,
        Action::Line => Tool::Line,
        Action::Rect => Tool::Rect,
        Action::Circle => Tool::Circle,
        Action::Arc => Tool::Arc,
        Action::Dimension => Tool::Dimension,
        _ => return None,
    })
}

fn wgpu_filter() -> egui_wgpu::wgpu::FilterMode {
    egui_wgpu::wgpu::FilterMode::Linear
}

fn floating<R>(ctx: &egui::Context, id: &str, anchor: Align2, pos: Pos2, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Area::new(Id::unique(id))
        .pivot(anchor)
        .fixed_pos(pos)
        .show(ctx, |ui| egui::Frame::window(&ctx.global_style()).inner_margin(8.0).show(ui, add).inner)
        .inner
}

fn icon_button(ui: &mut egui::Ui, icon: &str, tip: &str) -> egui::Response {
    let p = theme::palette(ui.ctx());
    ui.add(egui::Button::new(RichText::new(icon).size(16.0).color(p.weak)).min_size(vec2(28.0, 28.0)))
        .on_hover_text(tip)
}

fn tool_button(ui: &mut egui::Ui, icon: &str, label: &str, primary: bool) -> egui::Response {
    let p = theme::palette(ui.ctx());
    let text_w = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), FontId::proportional(10.5), Color32::WHITE).size().x);
    let w = if label.is_empty() { 34.0 } else { (text_w + 14.0).max(52.0) };
    let (rect, r) = ui.allocate_exact_size(vec2(w, 46.0), Sense::click());
    let bg = match (primary, r.hovered()) {
        (true, _) => p.accent.gamma_multiply(if r.hovered() { 1.0 } else { 0.9 }),
        (false, true) => p.hover,
        _ => Color32::TRANSPARENT,
    };
    let fg = if primary { p.on_accent } else { p.text };
    let painter = ui.painter();
    painter.rect_filled(rect, 8, bg);
    if label.is_empty() {
        icons::paint(painter, rect.center(), icon, 17.0, fg);
    } else {
        painter.text(rect.center() - vec2(0.0, 7.0), Align2::CENTER_CENTER, icon, FontId::proportional(20.0), fg);
        painter.text(rect.center_bottom() - vec2(0.0, 9.0), Align2::CENTER_CENTER, label, FontId::proportional(10.5), if primary { fg } else { p.weak });
    }
    r
}
