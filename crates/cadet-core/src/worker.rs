//! The geometry kernel runs in a child process so an OCCT crash can't take the app down.
//! Requests go over stdin; responses come back on stdout as framed postcard messages.
//! Frames start with a magic marker so stray kernel output on stdout is skipped.
//! `CADET_KERNEL_CRASH_ON=<feature name>` makes the worker abort there, to test recovery.

use crate::{Document, evaluate_with};
use cadet_kernel::{self as k, EdgeRef, Tessellation};
use glam::DVec3;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::{self, BufReader, Read, Write};
use std::path::PathBuf;

const MAGIC: &[u8; 8] = b"\x00CADETv1";

#[derive(Serialize, Deserialize)]
pub enum Request {
    /// `base`: also return the edges as they are just before that feature (for picking edges to round).
    Evaluate {
        doc: Document,
        upto: usize,
        skip: Vec<u32>,
        base: Option<usize>,
    },
    Ray {
        origin: DVec3,
        dir: DVec3,
    },
    /// `tolerance`: max STL deviation from the true surface, in mm.
    Export {
        path: PathBuf,
        step: bool,
        tolerance: f64,
    },
    /// Read a STEP/IGES file into BREP text for an Import feature.
    Import(PathBuf),
}

#[derive(Serialize, Deserialize)]
pub enum Response {
    /// Sent before each feature runs, so a crash can be blamed on it.
    Progress(usize),
    Model {
        mesh: Tessellation,
        edges: Vec<EdgeRef>,
        base_edges: Option<Vec<EdgeRef>>,
        props: Option<k::Props>,
        errors: Vec<(u32, String)>,
        millis: f64,
        bodies: usize,
    },
    Hit(Option<k::Hit>),
    Exported(Result<(), String>),
    Imported(Result<String, String>),
}

pub fn write<T: Serialize>(w: &mut impl Write, msg: &T) -> io::Result<()> {
    let bytes = postcard::to_stdvec(msg).map_err(io::Error::other)?;
    w.write_all(MAGIC)?;
    w.write_all(&(bytes.len() as u64).to_le_bytes())?;
    w.write_all(&bytes)?;
    w.flush()
}

/// Reads the next frame, skipping anything before the magic marker.
pub fn read<T: DeserializeOwned>(r: &mut impl Read) -> io::Result<T> {
    let mut matched = 0;
    let mut byte = [0u8];
    while matched < MAGIC.len() {
        r.read_exact(&mut byte)?;
        matched = if byte[0] == MAGIC[matched] {
            matched + 1
        } else if byte[0] == MAGIC[0] {
            1
        } else {
            0
        };
    }
    let mut len = [0u8; 8];
    r.read_exact(&mut len)?;
    let mut buf = vec![0; u64::from_le_bytes(len) as usize];
    r.read_exact(&mut buf)?;
    postcard::from_bytes(&buf).map_err(io::Error::other)
}

/// The worker's main loop. Returns when the app closes the pipe.
pub fn serve() {
    let mut input = BufReader::new(io::stdin().lock());
    let mut out = io::stdout();
    let mut bodies = vec![];
    let crash_on = std::env::var("CADET_KERNEL_CRASH_ON").ok();
    while let Ok(req) = read::<Request>(&mut input) {
        let resp = match req {
            Request::Evaluate { doc, upto, skip, base } => {
                let mut base_edges = None;
                let m = evaluate_with(&doc, upto, &skip, |i, now| {
                    let _ = write(&mut out, &Response::Progress(i));
                    if base == Some(i) {
                        base_edges = Some(k::edges(now));
                    }
                    if crash_on.as_deref() == Some(doc.features[i].name.as_str()) {
                        std::process::abort();
                    }
                });
                bodies = m.bodies;
                let edges = k::edges(&bodies);
                let base_edges = base.map(|_| base_edges.unwrap_or_else(|| edges.clone()));
                let props = k::props(&bodies);
                Response::Model { mesh: m.mesh, edges, base_edges, props, errors: m.errors, millis: m.millis, bodies: bodies.len() }
            }
            Request::Ray { origin, dir } => Response::Hit(k::ray(&bodies, origin, dir)),
            Request::Export { path, step, tolerance } => {
                Response::Exported(if step { k::write_step(&bodies, &path) } else { k::write_stl(&bodies, &path, tolerance) })
            }
            Request::Import(path) => Response::Imported(k::import(&path)),
        };
        if write(&mut out, &resp).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_skip_noise() {
        let mut buf = b"*** Statistics on Transfer ***\n\x00CA".to_vec();
        write(&mut buf, &Response::Progress(7)).unwrap();
        write(&mut buf, &Response::Hit(None)).unwrap();
        let mut r = &buf[..];
        assert!(matches!(read::<Response>(&mut r).unwrap(), Response::Progress(7)));
        assert!(matches!(read::<Response>(&mut r).unwrap(), Response::Hit(None)));
    }
}
