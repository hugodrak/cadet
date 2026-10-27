//! File dialogs, autosave and session restore.

use cadet_core::Document;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Default, Serialize, Deserialize)]
struct Session {
    path: Option<PathBuf>,
    dirty: bool,
}

fn dir() -> Option<PathBuf> {
    let d = dirs::data_dir()?.join("Cadet");
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

/// Writes the current document and session so the next launch resumes here.
pub fn autosave(doc: &Document, path: Option<&Path>, dirty: bool) {
    let Some(d) = dir() else { return };
    let _ = doc.save(&d.join("autosave.cadet"));
    let session = Session { path: path.map(Path::to_path_buf), dirty };
    let _ = std::fs::write(d.join("session.json"), serde_json::to_string(&session).unwrap_or_default());
}

/// Restores the last session. A leftover `.loading` marker means the last restore crashed, so skip it.
pub fn restore() -> Option<(Document, Option<PathBuf>, bool)> {
    let d = dir()?;
    let marker = d.join(".loading");
    if marker.exists() {
        let _ = std::fs::remove_file(&marker);
        return None;
    }
    let session: Session = serde_json::from_str(&std::fs::read_to_string(d.join("session.json")).ok()?).ok()?;
    let _ = std::fs::write(&marker, "");
    let source = match (&session.path, session.dirty) {
        (Some(p), false) if p.exists() => p.clone(),
        _ => d.join("autosave.cadet"),
    };
    let doc = Document::load(&source).ok();
    doc.map(|doc| (doc, session.path, session.dirty))
}

/// Call once the restored document has been evaluated without crashing.
pub fn restored_ok() {
    if let Some(d) = dir() {
        let _ = std::fs::remove_file(d.join(".loading"));
    }
}

pub const CAD_FILES: &[&str] = &["step", "stp", "iges", "igs"];

/// STEP/IGES files are imported into the current design rather than opened.
pub fn is_cad_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| CAD_FILES.contains(&e.to_string_lossy().to_lowercase().as_str()))
}

pub fn pick_open() -> Option<PathBuf> {
    rfd::FileDialog::new().add_filter("Cadet design", &["cadet"]).add_filter("STEP / IGES (import)", CAD_FILES).pick_file()
}

pub fn pick_import() -> Option<PathBuf> {
    rfd::FileDialog::new().add_filter("STEP / IGES", CAD_FILES).pick_file()
}

pub fn pick_save(current: Option<&Path>) -> Option<PathBuf> {
    let name = current.and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or("Untitled.cadet".into());
    let path = rfd::FileDialog::new().add_filter("Cadet design", &["cadet"]).set_file_name(name).save_file()?;
    Some(if path.extension().is_none() { path.with_extension("cadet") } else { path })
}

pub fn pick_export(current: Option<&Path>, step: bool) -> Option<PathBuf> {
    let stem = current.and_then(|p| p.file_stem()).map(|n| n.to_string_lossy().into_owned()).unwrap_or("part".into());
    let (name, ext) = if step { ("STEP solid", "step") } else { ("STL mesh", "stl") };
    let path = rfd::FileDialog::new().add_filter(name, &[ext]).set_file_name(format!("{stem}.{ext}")).save_file()?;
    Some(if path.extension().is_none() { path.with_extension(ext) } else { path })
}

pub enum Unsaved {
    Save,
    Discard,
    Cancel,
}

pub fn ask_unsaved(name: &str) -> Unsaved {
    let buttons = rfd::MessageButtons::YesNoCancelCustom("Save".into(), "Don't Save".into(), "Cancel".into());
    let answer = rfd::MessageDialog::new()
        .set_title(format!("Save changes to “{name}”?"))
        .set_description("Your changes will be lost if you don't save them.")
        .set_buttons(buttons)
        .show();
    match answer {
        rfd::MessageDialogResult::Custom(s) if s == "Save" => Unsaved::Save,
        rfd::MessageDialogResult::Custom(s) if s == "Don't Save" => Unsaved::Discard,
        rfd::MessageDialogResult::Yes => Unsaved::Save,
        rfd::MessageDialogResult::No => Unsaved::Discard,
        _ => Unsaved::Cancel,
    }
}

/// Opens a file or folder, optionally with a named app (macOS app name, or a program on Windows/Linux).
pub fn launch(path: &Path, app: Option<&str>) -> std::io::Result<()> {
    use std::process::Command;
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        if let Some(a) = app {
            c.args(["-a", a]);
        }
        c
    };
    #[cfg(windows)]
    let mut cmd = match app {
        Some(a) => Command::new(a),
        None => {
            let mut c = Command::new("cmd");
            c.args(["/C", "start", ""]);
            c
        }
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = Command::new(app.unwrap_or("xdg-open"));
    cmd.arg(path).spawn().map(drop)
}
