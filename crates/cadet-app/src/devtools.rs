//! Scripted input for visual checks: `CADET_SCRIPT=file cargo run`.
//! Lines: `wait N`, `move X Y`, `click X Y [shift]`, `dblclick X Y`, `drag X1 Y1 X2 Y2`,
//! `key NAME [shift] [cmd]`, `text STR`, `sleep MS`, `shot PATH`, `quit`.
//! `CADET_SCREENSHOT=out.png` is shorthand for `wait 20`, `shot out.png`, `quit`.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, ViewportCommand};
use std::collections::VecDeque;

enum Step {
    Events(Vec<Event>),
    /// Wait for real time to pass (animations run on wall-clock time).
    Sleep(std::time::Duration),
    Shot(String),
    Quit,
}

#[derive(Default)]
pub struct Script {
    steps: VecDeque<Step>,
    slept: Option<std::time::Instant>,
}

impl Script {
    pub fn from_env() -> Self {
        let text = match (std::env::var("CADET_SCRIPT"), std::env::var("CADET_SCREENSHOT")) {
            (Ok(path), _) => std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}")),
            (_, Ok(shot)) => format!("wait 20\nshot {shot}\nquit"),
            _ => return Self::default(),
        };
        let mut steps = VecDeque::new();
        steps.extend((0..5).map(|_| Step::Events(vec![])));
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
            let n: Vec<f32> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
            let at = |i: usize| Pos2::new(n[i], n[i + 1]);
            let modifiers = if rest.contains("shift") { Modifiers::SHIFT } else { Modifiers::NONE };
            let button = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers };
            let frame = |e: Vec<Event>| Step::Events(e);
            match cmd {
                "wait" => steps.extend((0..n[0] as usize).map(|_| frame(vec![]))),
                "sleep" => steps.push_back(Step::Sleep(std::time::Duration::from_millis(n[0] as u64))),
                "move" => steps.extend([frame(vec![Event::PointerMoved(at(0))]), frame(vec![])]),
                "click" | "dblclick" => {
                    steps.push_back(frame(vec![Event::ModifiersChanged(modifiers), Event::PointerMoved(at(0))]));
                    for _ in 0..if cmd == "click" { 1 } else { 2 } {
                        steps.push_back(frame(vec![button(at(0), true)]));
                        steps.push_back(frame(vec![button(at(0), false)]));
                    }
                    steps.push_back(frame(vec![Event::ModifiersChanged(Modifiers::NONE)]));
                }
                "drag" => {
                    let (a, b) = (at(0), at(2));
                    steps.push_back(frame(vec![Event::PointerMoved(a), button(a, true)]));
                    steps.extend((1..=12).map(|i| frame(vec![Event::PointerMoved(a.lerp(b, i as f32 / 12.0))])));
                    steps.extend([frame(vec![button(b, false)]), frame(vec![])]);
                }
                "key" => {
                    let mut it = rest.split_whitespace();
                    let key = Key::from_name(it.next().unwrap_or("")).unwrap_or_else(|| panic!("unknown key in '{line}'"));
                    let mut modifiers = Modifiers::NONE;
                    for m in it {
                        match m {
                            "shift" => modifiers.shift = true,
                            "cmd" => modifiers |= Modifiers::COMMAND,
                            _ => {}
                        }
                    }
                    let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
                    steps.extend([frame(vec![ev(true)]), frame(vec![ev(false)]), frame(vec![])]);
                }
                "text" => steps.extend([frame(vec![Event::Text(rest.to_string())]), frame(vec![])]),
                "shot" => steps.extend([Step::Shot(rest.to_string()), frame(vec![]), frame(vec![]), frame(vec![])]),
                "quit" => steps.push_back(Step::Quit),
                _ => panic!("unknown script command '{line}'"),
            }
        }
        Self { steps, slept: None }
    }

    pub fn active(&self) -> bool {
        !self.steps.is_empty()
    }

    pub fn feed(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(Step::Sleep(d)) = self.steps.front_mut() {
            ctx.request_repaint();
            raw.events.clear();
            let started = *self.slept.get_or_insert_with(std::time::Instant::now);
            if started.elapsed() < *d {
                return;
            }
            self.slept = None;
            self.steps.pop_front();
        }
        let Some(step) = self.steps.pop_front() else { return };
        ctx.request_repaint();
        raw.focused = true;
        raw.events.clear();
        match step {
            Step::Events(e) => raw.events.extend(e),
            Step::Sleep(_) => {}
            Step::Shot(path) => ctx.send_viewport_cmd(ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                let [w, h] = img.size;
                image::save_buffer(&path, img.as_raw(), w as u32, h as u32, image::ExtendedColorType::Rgba8).expect("save screenshot");
            }))),
            Step::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
        }
    }
}
