//! Talks to the kernel worker process (`cadet --kernel`), restarting it if it crashes.

use cadet_core::Document;
use cadet_core::kernel::{EdgeRef, Hit, Tessellation};
use cadet_core::worker::{Request, Response, read, write};
use glam::DVec3;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};

#[derive(Default)]
pub struct Model {
    pub mesh: Tessellation,
    pub edges: Vec<EdgeRef>,
    pub base_edges: Option<Vec<EdgeRef>>,
    /// Exact size, volume and area.
    pub props: Option<cadet_core::kernel::Props>,
    pub errors: Vec<(u32, String)>,
    pub millis: f64,
    pub bodies: usize,
}

pub enum Event {
    Model(Model),
    Hit(Option<Hit>),
    Exported(Result<(), String>),
    /// The file's name and its geometry as BREP text.
    Imported(String, Result<String, String>),
    Crashed(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Pending {
    Evaluate,
    Pick,
    Export,
    Import,
}

struct Worker {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Option<Response>>,
}

pub struct Kernel {
    worker: Worker,
    ctx: egui::Context,
    pending: VecDeque<Pending>,
    queued: Option<(Document, usize, Option<usize>)>,
    last: Option<(Document, usize, Option<usize>)>,
    /// Features that crashed the kernel, by id and definition, so an edited feature is retried.
    skip: Vec<(u32, String)>,
    progress: Option<usize>,
    /// File names of imports in flight, in order.
    importing: VecDeque<String>,
}

fn spawn(ctx: &egui::Context) -> Worker {
    let exe = std::env::current_exe().expect("current exe");
    let mut cmd = Command::new(exe);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000);
    let mut child = cmd.arg("--kernel").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().expect("start kernel worker");
    let stdin = child.stdin.take().unwrap();
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        loop {
            let msg = read::<Response>(&mut stdout).ok();
            let done = msg.is_none();
            let _ = tx.send(msg);
            ctx.request_repaint();
            if done {
                return;
            }
        }
    });
    Worker { child, stdin, rx }
}

impl Kernel {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            worker: spawn(ctx),
            ctx: ctx.clone(),
            pending: VecDeque::new(),
            queued: None,
            last: None,
            skip: vec![],
            progress: None,
            importing: VecDeque::new(),
        }
    }

    fn send(&mut self, req: Request, kind: Pending) {
        if write(&mut self.worker.stdin, &req).is_ok() {
            self.pending.push_back(kind);
        }
    }

    /// Evaluates the first `upto` features, optionally capturing edges before feature `base`.
    /// Coalesces while an evaluation is in flight.
    pub fn evaluate(&mut self, doc: &Document, upto: usize, base: Option<usize>) {
        if self.pending.contains(&Pending::Evaluate) {
            self.queued = Some((doc.clone(), upto, base));
            return;
        }
        self.last = Some((doc.clone(), upto, base));
        let skip = doc.features.iter().filter(|f| self.skip.iter().any(|(id, fp)| *id == f.id && *fp == f.kind.fingerprint())).map(|f| f.id).collect();
        self.send(Request::Evaluate { doc: doc.clone(), upto, skip, base }, Pending::Evaluate);
    }

    /// Casts a ray at the model unless one is already in flight.
    pub fn ray(&mut self, origin: DVec3, dir: DVec3) {
        if !self.pending.contains(&Pending::Pick) {
            self.send(Request::Ray { origin, dir }, Pending::Pick);
        }
    }

    /// Exports the latest model: a coalesced evaluation is sent first so the export sees it.
    pub fn export(&mut self, path: PathBuf, step: bool, tolerance: f64) {
        if let Some((doc, upto, base)) = self.queued.take() {
            self.last = Some((doc.clone(), upto, base));
            let skip = doc.features.iter().filter(|f| self.skip.iter().any(|(id, fp)| *id == f.id && *fp == f.kind.fingerprint())).map(|f| f.id).collect();
            self.send(Request::Evaluate { doc, upto, skip, base }, Pending::Evaluate);
        }
        self.send(Request::Export { path, step, tolerance }, Pending::Export);
    }

    /// Reads a STEP/IGES file in the worker; the result arrives as `Event::Imported`.
    pub fn import(&mut self, path: PathBuf) {
        self.importing.push_back(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        self.send(Request::Import(path), Pending::Import);
    }

    pub fn busy(&self) -> bool {
        self.pending.contains(&Pending::Evaluate)
    }

    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = vec![];
        while let Ok(msg) = self.worker.rx.try_recv() {
            match msg {
                Some(Response::Progress(i)) => self.progress = Some(i),
                Some(resp) => {
                    self.pending.pop_front();
                    match resp {
                        Response::Model { mesh, edges, base_edges, props, errors, millis, bodies } => {
                            self.progress = None;
                            events.push(Event::Model(Model { mesh, edges, base_edges, props, errors, millis, bodies }));
                            if let Some((doc, upto, base)) = self.queued.take() {
                                self.evaluate(&doc, upto, base);
                            }
                        }
                        Response::Hit(h) => events.push(Event::Hit(h)),
                        Response::Exported(r) => events.push(Event::Exported(r)),
                        Response::Imported(r) => events.push(Event::Imported(self.importing.pop_front().unwrap_or_default(), r)),
                        Response::Progress(_) => {}
                    }
                }
                None => {
                    events.push(self.recover());
                    break;
                }
            }
        }
        events
    }

    /// Restarts the worker after a crash, blaming the feature that was running, and re-evaluates.
    fn recover(&mut self) -> Event {
        let _ = self.worker.child.wait();
        let what = self.pending.front().copied();
        let blamed = match (what, &self.last, self.progress) {
            (Some(Pending::Evaluate), Some((doc, ..)), Some(i)) => doc.features.get(i).map(|f| (f.id, f.kind.fingerprint(), f.name.clone())),
            _ => None,
        };
        if let Some((id, fp, _)) = &blamed {
            self.skip.push((*id, fp.clone()));
        }
        self.worker = spawn(&self.ctx);
        self.pending.clear();
        self.progress = None;
        if let Some((doc, upto, base)) = self.queued.take().or(self.last.take()) {
            self.evaluate(&doc, upto, base);
        }
        Event::Crashed(match (what, blamed) {
            (_, Some((.., name))) => format!("{name} couldn't be built (the kernel failed on it). Try other values"),
            (Some(Pending::Export), _) => "Export crashed the geometry kernel".into(),
            (Some(Pending::Import), _) => format!("Couldn't import {}: it crashed the geometry kernel", self.importing.pop_front().unwrap_or_default()),
            _ => "The geometry kernel restarted after a crash".into(),
        })
    }
}

impl Drop for Kernel {
    fn drop(&mut self) {
        let _ = self.worker.child.kill();
    }
}
