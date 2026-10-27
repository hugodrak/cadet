use super::*;

#[test]
fn console_builds_a_part() {
    let mut e = Engine::new();
    let out = e.run("s = cadet.sketch('XY')\ncadet.rect(s, 0, 0, 20, 10)\ncadet.circle(s, 10, 5, 2)\ncadet.extrude(s, 5, op='new')", &Document::default());
    assert!(out.error.is_none(), "{:?}", out.error);
    let doc = out.doc.expect("changed");
    assert_eq!(doc.features.len(), 2);
    let m = cadet_core::evaluate(&doc, 2);
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    assert_eq!(m.bodies.len(), 1);
    let p = cadet_core::kernel::props(&m.bodies).unwrap();
    assert!((p.volume - (200.0 - std::f64::consts::PI * 4.0) * 5.0).abs() < 1e-6, "{}", p.volume);
}

#[test]
fn console_session_keeps_variables_and_prints() {
    let mut e = Engine::new();
    let doc = Document::default();
    assert!(e.run("import math\nr = math.sqrt(16)", &doc).error.is_none());
    let out = e.run("r * 2", &doc);
    assert_eq!(out.output.trim(), "8.0");
    let out = e.run("print('hello', len(cadet.features()))", &doc);
    assert_eq!(out.output.trim(), "hello 0");
    assert!(out.doc.is_none(), "reading doesn't change the document");
}

#[test]
fn errors_are_reported_not_fatal() {
    let mut e = Engine::new();
    let doc = Document::default();
    let out = e.run("cadet.extrude(99, 5, op='sideways')", &doc);
    assert!(out.error.as_deref().is_some_and(|m| m.contains("op must be")), "{:?}", out.error);
    let out = e.run("def (", &doc);
    assert!(out.error.as_deref().is_some_and(|m| m.contains("SyntaxError")), "{:?}", out.error);
    assert!(e.run("1 + 1", &doc).output.contains('2'));
}

#[test]
fn plugins_register_with_params() {
    let mut e = Engine::new();
    let src = "import cadet\n@cadet.command('Plate')\ndef plate(width=40.0, depth=30.0, holes=True, name='p'):\n    s = cadet.sketch('XY')\n    cadet.rect(s, 0, 0, width, depth)\n    cadet.extrude(s, 3)\n    cadet.run('view.home')\n";
    let errors = e.load_plugins(None, &[("plate.py", src), ("broken.py", "import nope_missing")]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(e.plugins().len(), 1);
    let p = &e.plugins()[0];
    assert_eq!(p.label, "Plate");
    assert_eq!(p.params[0], ("width".into(), Value::Number(40.0)));
    assert_eq!(p.params[2], ("holes".into(), Value::Bool(true)));
    assert_eq!(p.params[3], ("name".into(), Value::Text("p".into())));
    let out = e.run_plugin(0, &[("width".into(), Value::Number(10.0))], &Document::default());
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.requests, vec![Request::Command("view.home".into())]);
    let doc = out.doc.unwrap();
    let m = cadet_core::evaluate(&doc, doc.features.len());
    let p = cadet_core::kernel::props(&m.bodies).unwrap();
    assert!((p.max.x - 10.0).abs() < 1e-9 && (p.volume - 10.0 * 30.0 * 3.0).abs() < 1e-6, "{p:?}");
}

#[test]
fn sample_plugins_work() {
    let mut e = Engine::new();
    let gear = include_str!("../../../plugins/gear.py");
    let slicer = include_str!("../../../plugins/send_to_slicer.py");
    let t = std::time::Instant::now();
    let errors = e.load_plugins(None, &[("gear.py", gear), ("send_to_slicer.py", slicer)]);
    eprintln!("engine + plugins ready in {:?}", t.elapsed());
    assert!(errors.is_empty(), "{errors:?}");
    let labels: Vec<&str> = e.plugins().iter().map(|p| p.label.as_str()).collect();
    assert_eq!(labels, ["Spur gear", "Send to slicer"]);

    let out = e.run_plugin(0, &[], &Document::default());
    assert!(out.error.is_none(), "{:?}", out.error);
    assert!(out.output.contains("pitch Ø 40.000"), "{}", out.output);
    let doc = out.doc.unwrap();
    let m = cadet_core::evaluate(&doc, doc.features.len());
    assert!(m.errors.is_empty(), "{:?}", m.errors);
    let p = cadet_core::kernel::props(&m.bodies).unwrap();
    let d = p.max - p.min;
    eprintln!("gear: {:?} mm, {:.1} mm³ in {:.1} ms", d, p.volume, m.millis);
    assert!((d.z - 6.0).abs() < 1e-9 && (d.x - 44.0).abs() < 0.2, "outside Ø ≈ 44: {d:?}");

    let out = e.run_plugin(1, &[], &doc);
    assert!(out.error.is_none(), "{:?}", out.error);
    assert!(matches!(out.requests[0], Request::Export { step: false, .. }) && matches!(out.requests[1], Request::Open { .. }));
}

