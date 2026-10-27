//! Python scripting with RustPython (built into the binary, nothing to install):
//! a `cadet` module over the document, a persistent console session, and plugins.
//! Scripts edit a copy of the document; the app applies it as one undoable step.

use cadet_core::{Axis, Document, Kind, Op, Plane};
use cadet_sketch::{Constraint, P, Sketch};
use glam::{DVec2, DVec3};
use rustpython_vm::{
    self as vm, AsObject, Interpreter, PyObject, PyObjectRef, PyResult, VirtualMachine,
    builtins::{PyBaseExceptionRef, PyStr},
    compiler::Mode,
    scope::Scope,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// Side effects a script asks the app to perform once it finishes, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Command(String),
    Export { path: PathBuf, step: bool },
    Open { path: PathBuf, app: Option<String> },
}

#[derive(Default)]
struct State {
    doc: Document,
    changed: bool,
    requests: Vec<Request>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub struct Outcome {
    /// The edited document, if the script changed it.
    pub doc: Option<Document>,
    pub output: String,
    pub error: Option<String>,
    pub requests: Vec<Request>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Int(i64),
    Bool(bool),
    Text(String),
}

#[derive(Debug, Clone)]
pub struct Plugin {
    pub label: String,
    /// Keyword parameters with their defaults, which the app turns into a form.
    pub params: Vec<(String, Value)>,
}

pub struct Engine {
    interp: Interpreter,
    scope: Scope,
    plugins: Vec<Plugin>,
}

const PRELUDE: &str = r#"
import sys, cadet

class _Out:
    def __init__(self):
        self.parts = []
    def write(self, s):
        self.parts.append(str(s))
        return len(s)
    def flush(self):
        pass
    def take(self):
        s = "".join(self.parts)
        self.parts = []
        return s

sys.stdout = sys.stderr = _Out()
_plugins = []

def _register(label, f=None):
    if f is None:
        return lambda f: _register(label, f)
    _plugins.append((label, f))
    return f

def _params(f):
    c = f.__code__
    names = c.co_varnames[:c.co_argcount]
    d = f.__defaults__ or ()
    first = len(names) - len(d)
    return [(n, d[i - first] if i >= first else 0.0) for i, n in enumerate(names)]

cadet.command = _register
"#;

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        let interp = vm::InterpreterBuilder::new()
            .add_native_modules(&rustpython_stdlib::stdlib_module_defs(vm::Context::genesis()))
            .add_native_module(cadet::module_def(vm::Context::genesis()))
            .build();
        let scope = interp.enter(|vm| {
            let scope = vm.new_scope_with_builtins();
            vm.run_string(scope.clone(), PRELUDE, "<prelude>").expect("prelude");
            scope
        });
        Self { interp, scope, plugins: vec![] }
    }

    pub fn plugins(&self) -> &[Plugin] {
        &self.plugins
    }

    /// Runs console input. Expressions print their value, like the Python REPL.
    pub fn run(&mut self, source: &str, doc: &Document) -> Outcome {
        let scope = self.scope.clone();
        self.session(doc, |vm| {
            let Ok(code) = vm.compile(source, Mode::Eval, "<console>".to_owned()) else {
                return vm.run_string(scope.clone(), source, "<console>").map(drop);
            };
            let value = vm.run_code_obj(code, scope.clone())?;
            if !vm.is_none(&value) {
                let line = format!("{}\n", value.repr(vm)?);
                let out = vm.sys_module.get_attr("stdout", vm)?;
                vm.call_method(&out, "write", (line,))?;
            }
            Ok(())
        })
    }

    /// Loads every `*.py` file (and `*/plugin.py`) in a folder, plus the given built-ins.
    /// Returns load errors, one line per plugin.
    pub fn load_plugins(&mut self, dir: Option<&Path>, builtin: &[(&str, &str)]) -> Vec<String> {
        let mut files: Vec<(String, String)> = builtin.iter().map(|(n, s)| (n.to_string(), s.to_string())).collect();
        if let Some(entries) = dir.and_then(|d| std::fs::read_dir(d).ok()) {
            let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for p in paths {
                let file = if p.is_dir() { p.join("plugin.py") } else { p.clone() };
                if file.extension().is_some_and(|e| e == "py")
                    && let Ok(src) = std::fs::read_to_string(&file)
                {
                    files.push((file.display().to_string(), src));
                }
            }
        }
        let scope = self.scope.clone();
        let mut errors = vec![];
        self.interp.enter(|vm| {
            let _ = vm.run_string(scope.clone(), "_plugins.clear()", "<plugins>");
            for (name, src) in &files {
                let module_scope = vm.new_scope_with_builtins();
                if let Err(e) = vm.run_string(module_scope, src, name) {
                    errors.push(format!("{name}: {}", format_error(vm, &e).lines().last().unwrap_or_default()));
                }
            }
            let listed = vm.run_block_expr(scope.clone(), "[(l, _params(f)) for l, f in _plugins]");
            self.plugins = listed.ok().and_then(|v| plugin_list(vm, v)).unwrap_or_default();
        });
        errors
    }

    pub fn run_plugin(&mut self, index: usize, args: &[(String, Value)], doc: &Document) -> Outcome {
        let scope = self.scope.clone();
        self.session(doc, |vm| {
            let f = vm.run_block_expr(scope.clone(), &format!("_plugins[{index}][1]"))?;
            let kwargs = vm::function::KwArgs::new(args.iter().map(|(k, v)| (k.as_str().into(), to_py(vm, v))).collect());
            f.call(vm::function::FuncArgs::new(vec![], kwargs), vm)?;
            Ok(())
        })
    }

    fn session(&mut self, doc: &Document, run: impl FnOnce(&VirtualMachine) -> Result<(), PyBaseExceptionRef>) -> Outcome {
        with_state(|s| *s = State { doc: doc.clone(), ..Default::default() });
        let scope = self.scope.clone();
        let (error, output) = self.interp.enter(|vm| {
            let error = run(vm).err().map(|e| format_error(vm, &e));
            let output = vm.run_block_expr(scope, "sys.stdout.take()").ok().and_then(|o| o.str(vm).ok().map(|s| text(&s).to_string()));
            (error, output.unwrap_or_default())
        });
        let state = with_state(std::mem::take);
        Outcome { doc: state.changed.then_some(state.doc), output, error, requests: state.requests }
    }
}

fn format_error(vm: &VirtualMachine, e: &PyBaseExceptionRef) -> String {
    let mut s = String::new();
    let _ = vm.write_exception(&mut s, e);
    s.trim_end().to_string()
}

fn text(s: &PyStr) -> &str {
    s.to_str().unwrap_or_default()
}

/// A Python sequence of sequences of numbers, e.g. [(x, y), ...].
fn rows(vm: &VirtualMachine, obj: &PyObject, width: usize) -> PyResult<Vec<Vec<f64>>> {
    let rows =
        vm.extract_elements_with(obj, |row| vm.extract_elements_with(&row, |x| x.try_into_value::<vm::function::ArgIntoFloat>(vm).map(|f| f.into_float())))?;
    match rows.iter().find(|r| r.len() != width) {
        Some(_) => Err(vm.new_value_error(format!("expected points with {width} coordinates"))),
        None => Ok(rows),
    }
}

fn list(vm: &VirtualMachine, items: Vec<PyObjectRef>) -> PyObjectRef {
    vm.ctx.new_list(items).into()
}

fn to_py(vm: &VirtualMachine, v: &Value) -> PyObjectRef {
    match v {
        Value::Number(x) => vm.ctx.new_float(*x).into(),
        Value::Int(x) => vm.ctx.new_int(*x).into(),
        Value::Bool(x) => vm.ctx.new_bool(*x).into(),
        Value::Text(x) => vm.ctx.new_str(x.as_str()).into(),
    }
}

fn from_py(vm: &VirtualMachine, o: &PyObjectRef) -> Value {
    if o.fast_isinstance(vm.ctx.types.bool_type) {
        Value::Bool(o.clone().try_to_bool(vm).unwrap_or(false))
    } else if let Ok(i) = o.clone().try_into_value::<i64>(vm) {
        Value::Int(i)
    } else if let Ok(x) = o.clone().try_into_value::<f64>(vm) {
        Value::Number(x)
    } else {
        Value::Text(o.str(vm).map(|s| text(&s).to_string()).unwrap_or_default())
    }
}

fn plugin_list(vm: &VirtualMachine, v: PyObjectRef) -> Option<Vec<Plugin>> {
    let items = vm.extract_elements_with(&v, |item| {
        let pair = vm.extract_elements_with(&item, Ok)?;
        let label = pair[0].str(vm).map(|s| text(&s).to_string())?;
        let params = vm.extract_elements_with(&pair[1], |p| {
            let kv = vm.extract_elements_with(&p, Ok)?;
            Ok((kv[0].str(vm).map(|s| text(&s).to_string())?, from_py(vm, &kv[1])))
        })?;
        Ok(Plugin { label, params })
    });
    items.ok()
}

/// Runs `f` on a sketch feature of the script's document.
fn edit_sketch<R>(id: u32, f: impl FnOnce(&mut Sketch) -> R) -> Result<R, String> {
    with_state(|s| {
        let i = s.doc.index_of(id).ok_or(format!("no feature with id {id}"))?;
        match &mut s.doc.features[i].kind {
            Kind::Sketch { sketch, .. } => {
                s.changed = true;
                Ok(f(sketch))
            }
            _ => Err(format!("feature {id} isn't a sketch")),
        }
    })
}

/// Reuses an existing point at the same spot, so consecutive lines form closed profiles.
fn point_at(sk: &mut Sketch, q: DVec2) -> P {
    sk.points.iter().copied().find(|&p| sk.pt(p).distance(q) < 1e-9).unwrap_or_else(|| sk.add_point(q))
}

/// Adds a feature; like the UI, a sketch used by a feature is hidden.
fn add_feature(kind: Kind) -> u32 {
    with_state(|s| {
        s.changed = true;
        if let Some(i) = kind.sketch_ref().and_then(|id| s.doc.index_of(id)) {
            s.doc.features[i].visible = false;
        }
        let i = s.doc.add(kind);
        s.doc.features[i].id
    })
}

fn op_of(op: Option<&str>) -> Result<Op, String> {
    match op.unwrap_or("new") {
        "new" => Ok(Op::New),
        "join" => Ok(Op::Join),
        "cut" => Ok(Op::Cut),
        other => Err(format!("op must be 'new', 'join' or 'cut', not '{other}'")),
    }
}

#[vm::pymodule]
mod cadet {
    use super::*;
    use vm::builtins::PyStrRef;
    use vm::function::{ArgIntoFloat as Num, OptionalArg};
    use vm::{FromArgs, PyResult};

    fn err<T>(vm: &VirtualMachine, r: Result<T, String>) -> PyResult<T> {
        r.map_err(|e| vm.new_value_error(e))
    }

    /// Starts a sketch on "XY", "XZ" or "YZ". Returns its feature id.
    #[pyfunction]
    fn sketch(plane: OptionalArg<PyStrRef>, vm: &VirtualMachine) -> PyResult<u32> {
        let plane = match plane.as_ref().map_or("XY", |p| text(p)) {
            "XY" => Plane::XY,
            "XZ" => Plane::XZ,
            "YZ" => Plane::YZ,
            other => return Err(vm.new_value_error(format!("plane must be XY, XZ or YZ, not {other}"))),
        };
        Ok(add_feature(Kind::Sketch { plane, sketch: Sketch::new() }))
    }

    #[pyfunction]
    fn line(sk: u32, x1: Num, y1: Num, x2: Num, y2: Num, vm: &VirtualMachine) -> PyResult<usize> {
        let [x1, y1, x2, y2] = [x1, y1, x2, y2].map(Num::into_float);
        err(
            vm,
            edit_sketch(sk, |s| {
                let (a, b) = (point_at(s, DVec2::new(x1, y1)), point_at(s, DVec2::new(x2, y2)));
                s.add_line(a, b)
            }),
        )
    }

    /// A closed polygon through the given (x, y) points.
    #[pyfunction]
    fn polygon(sk: u32, points: PyObjectRef, vm: &VirtualMachine) -> PyResult {
        let points = rows(vm, &points, 2)?;
        if points.len() < 3 {
            return Err(vm.new_value_error("a polygon needs at least 3 points"));
        }
        let lines: Vec<usize> = err(
            vm,
            edit_sketch(sk, |s| {
                let ps: Vec<P> = points.iter().map(|p| point_at(s, DVec2::new(p[0], p[1]))).collect();
                (0..ps.len()).map(|i| s.add_line(ps[i], ps[(i + 1) % ps.len()])).collect()
            }),
        )?;
        Ok(list(vm, lines.into_iter().map(|l| vm.ctx.new_int(l).into()).collect()))
    }

    /// An axis-aligned rectangle with horizontal/vertical constraints.
    #[pyfunction]
    fn rect(sk: u32, x: Num, y: Num, w: Num, h: Num, vm: &VirtualMachine) -> PyResult {
        let [x, y, w, h] = [x, y, w, h].map(Num::into_float);
        let lines: Vec<usize> = err(
            vm,
            edit_sketch(sk, |s| {
                let p = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)].map(|(a, b)| point_at(s, DVec2::new(a, b)));
                s.constraints.extend([
                    Constraint::Horizontal(p[0], p[1]),
                    Constraint::Vertical(p[1], p[2]),
                    Constraint::Horizontal(p[2], p[3]),
                    Constraint::Vertical(p[3], p[0]),
                ]);
                (0..4).map(|i| s.add_line(p[i], p[(i + 1) % 4])).collect()
            }),
        )?;
        Ok(list(vm, lines.into_iter().map(|l| vm.ctx.new_int(l).into()).collect()))
    }

    #[pyfunction]
    fn circle(sk: u32, cx: Num, cy: Num, r: Num, vm: &VirtualMachine) -> PyResult<usize> {
        let [cx, cy, r] = [cx, cy, r].map(Num::into_float);
        err(
            vm,
            edit_sketch(sk, |s| {
                let c = point_at(s, DVec2::new(cx, cy));
                s.add_circle(c, r)
            }),
        )
    }

    /// A counter-clockwise arc from `start` to `end` degrees.
    #[pyfunction]
    fn arc(sk: u32, cx: Num, cy: Num, r: Num, start: Num, end: Num, vm: &VirtualMachine) -> PyResult<usize> {
        let [cx, cy, r, start, end] = [cx, cy, r, start, end].map(Num::into_float);
        err(
            vm,
            edit_sketch(sk, |s| {
                let c = DVec2::new(cx, cy);
                let at = |deg: f64| c + DVec2::from_angle(deg.to_radians()) * r;
                let (pc, pa, pb) = (point_at(s, c), point_at(s, at(start)), point_at(s, at(end)));
                s.add_arc(pc, pa, pb)
            }),
        )
    }

    #[derive(FromArgs)]
    struct ExtrudeArgs {
        #[pyarg(positional)]
        sketch: u32,
        #[pyarg(positional)]
        distance: Num,
        #[pyarg(any, optional)]
        op: OptionalArg<PyStrRef>,
        #[pyarg(any, optional)]
        symmetric: OptionalArg<bool>,
    }

    #[pyfunction]
    fn extrude(a: ExtrudeArgs, vm: &VirtualMachine) -> PyResult<u32> {
        let op = err(vm, op_of(a.op.as_ref().map(|o| text(o)).into_option()))?;
        let symmetric = a.symmetric.unwrap_or(false);
        Ok(add_feature(Kind::Extrude { sketch: a.sketch, probes: vec![], distance: a.distance.into_float(), symmetric, op }))
    }

    #[derive(FromArgs)]
    struct RevolveArgs {
        #[pyarg(positional)]
        sketch: u32,
        #[pyarg(any, optional)]
        angle: OptionalArg<Num>,
        #[pyarg(any, optional)]
        axis: OptionalArg<PyStrRef>,
        #[pyarg(any, optional)]
        op: OptionalArg<PyStrRef>,
    }

    #[pyfunction]
    fn revolve(a: RevolveArgs, vm: &VirtualMachine) -> PyResult<u32> {
        let op = err(vm, op_of(a.op.as_ref().map(|o| text(o)).into_option()))?;
        let axis = match a.axis.as_ref().map_or("Y", |s| text(s)) {
            "X" => Axis::X,
            "Y" => Axis::Y,
            "Z" => Axis::Z,
            other => return Err(vm.new_value_error(format!("axis must be X, Y or Z, not {other}"))),
        };
        Ok(add_feature(Kind::Revolve { sketch: a.sketch, probes: vec![], axis, angle: a.angle.map_or(360.0, |x| x.into_float()), op }))
    }

    /// Rounds the edges passing through the given (x, y, z) points.
    #[pyfunction]
    fn fillet(points: PyObjectRef, radius: Num, vm: &VirtualMachine) -> PyResult<u32> {
        let edges = rows(vm, &points, 3)?.iter().map(|p| DVec3::from_slice(p)).collect();
        Ok(add_feature(Kind::Round { edges, size: radius.into_float(), chamfer: false }))
    }

    #[pyfunction]
    fn chamfer(points: PyObjectRef, distance: Num, vm: &VirtualMachine) -> PyResult<u32> {
        let edges = rows(vm, &points, 3)?.iter().map(|p| DVec3::from_slice(p)).collect();
        Ok(add_feature(Kind::Round { edges, size: distance.into_float(), chamfer: true }))
    }

    /// (id, name, kind) for every feature.
    #[pyfunction]
    fn features(vm: &VirtualMachine) -> PyObjectRef {
        let items = with_state(|s| s.doc.features.iter().map(|f| vm.new_tuple((f.id, f.name.clone(), f.kind.label())).into()).collect());
        list(vm, items)
    }

    /// Deletes a feature and what depends on it.
    #[pyfunction]
    fn delete(id: u32, vm: &VirtualMachine) -> PyResult<()> {
        let removed = with_state(|s| {
            let i = s.doc.index_of(id)?;
            s.changed = true;
            Some(s.doc.remove(i))
        });
        removed.map(|_| ()).ok_or_else(|| vm.new_value_error(format!("no feature with id {id}")))
    }

    /// Starts a new, empty design.
    #[pyfunction]
    fn clear() {
        with_state(|s| {
            s.doc = Document::default();
            s.changed = true;
        });
    }

    /// Runs an app command by id, e.g. "view.home" (see the command palette).
    #[pyfunction]
    fn run(command: PyStrRef) {
        with_state(|s| s.requests.push(Request::Command(text(&command).to_string())));
    }

    #[pyfunction]
    fn export_stl(path: PyStrRef) {
        with_state(|s| s.requests.push(Request::Export { path: text(&path).into(), step: false }));
    }

    #[pyfunction]
    fn export_step(path: PyStrRef) {
        with_state(|s| s.requests.push(Request::Export { path: text(&path).into(), step: true }));
    }

    /// Opens a file once earlier requests (like exports) are done, optionally in a named app.
    #[pyfunction]
    fn open_file(path: PyStrRef, app: OptionalArg<Option<PyStrRef>>) {
        let app = app.into_option().flatten().map(|a| text(&a).to_string());
        with_state(|s| s.requests.push(Request::Open { path: text(&path).into(), app }));
    }

    /// A path in the system temp folder.
    #[pyfunction]
    fn temp_path(name: PyStrRef) -> String {
        std::env::temp_dir().join(text(&name)).display().to_string()
    }

    /// Whether a file or app bundle exists.
    #[pyfunction]
    fn exists(path: PyStrRef) -> bool {
        Path::new(text(&path)).exists()
    }

    #[pyattr]
    const PLATFORM: &str = std::env::consts::OS;
}

#[cfg(test)]
mod tests;
