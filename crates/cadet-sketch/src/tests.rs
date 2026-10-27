use crate::*;
use glam::DVec2;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn rect(sk: &mut Sketch, a: DVec2, b: DVec2) -> [E; 4] {
    let p = [a, v(b.x, a.y), b, v(a.x, b.y)].map(|q| sk.add_point(q));
    let l = [0, 1, 2, 3].map(|i| sk.add_line(p[i], p[(i + 1) % 4]));
    sk.constraints.extend([
        Constraint::Horizontal(p[0], p[1]),
        Constraint::Vertical(p[1], p[2]),
        Constraint::Horizontal(p[2], p[3]),
        Constraint::Vertical(p[3], p[0]),
    ]);
    l
}

#[test]
fn rectangle_fully_constrained() {
    let mut sk = Sketch::new();
    let l = rect(&mut sk, v(1.0, 1.0), v(11.0, 6.0));
    assert_eq!(analyze(&sk).dof, 4);
    let Entity::Line { a, b } = sk.entity(l[0]).unwrap() else { panic!() };
    sk.constraints.push(Constraint::Coincident(a, Sketch::ORIGIN));
    sk.constraints.push(Constraint::Distance(a, b, 40.0));
    let Entity::Line { a: c, b: d } = sk.entity(l[1]).unwrap() else { panic!() };
    sk.constraints.push(Constraint::Distance(c, d, 25.0));
    assert!(solve(&mut sk, &[]));
    let an = analyze(&sk);
    assert_eq!(an.dof, 0);
    assert!(an.determined.iter().all(|&d| d));
    assert!(sk.pt(d).distance(v(40.0, 25.0)) < 1e-6, "{:?}", sk.pt(d));
}

#[test]
fn unconstrained_geometry_stays_put() {
    let mut sk = Sketch::new();
    let a = sk.add_point(v(0.0, 5.0));
    let b = sk.add_point(v(10.0, 7.0));
    let far = sk.add_point(v(50.0, 50.0));
    let c = sk.add_point(v(60.0, 50.0));
    sk.add_line(a, b);
    sk.add_line(far, c);
    sk.constraints.push(Constraint::Horizontal(a, b));
    assert!(solve(&mut sk, &[]));
    assert!((sk.pt(a).y - sk.pt(b).y).abs() < 1e-9);
    assert_eq!(sk.pt(far), v(50.0, 50.0));
    assert!(!analyze(&sk).determined[far]);
}

#[test]
fn tangent_and_parallel() {
    let mut sk = Sketch::new();
    let c = sk.add_point(v(0.0, 0.0));
    let circle = sk.add_circle(c, 5.0);
    sk.constraints.extend([Constraint::Fix(c), Constraint::Radius(circle, 5.0)]);
    let a = sk.add_point(v(-10.0, 7.0));
    let b = sk.add_point(v(10.0, 8.0));
    let l1 = sk.add_line(a, b);
    let p = sk.add_point(v(-10.0, -3.0));
    let q = sk.add_point(v(10.0, -1.0));
    let l2 = sk.add_line(p, q);
    sk.constraints.extend([Constraint::Tangent(l1, circle), Constraint::Parallel(l1, l2), Constraint::Horizontal(a, b)]);
    assert!(solve(&mut sk, &[]));
    assert!((sk.pt(a).y.abs() - 5.0).abs() < 1e-6);
    assert!((sk.pt(p).y - sk.pt(q).y).abs() < 1e-6);
}

#[test]
fn drag_respects_constraints() {
    let mut sk = Sketch::new();
    let c = sk.add_point(v(0.0, 0.0));
    sk.constraints.push(Constraint::Fix(c));
    let a = sk.add_point(v(10.0, 0.0));
    let line = sk.add_line(c, a);
    sk.constraints.push(Constraint::Distance(c, a, 10.0));
    assert!(solve(&mut sk, &[(a, 0.0), (a + 1, 20.0)]));
    assert!(sk.pt(a).distance(v(0.0, 10.0)) < 1e-6, "{:?}", sk.pt(a));
    let _ = line;
}

#[test]
fn conflict_is_rejected() {
    let mut sk = Sketch::new();
    let a = sk.add_point(v(0.0, 0.0));
    let b = sk.add_point(v(10.0, 1.0));
    sk.add_line(a, b);
    sk.constraints.extend([Constraint::Fix(a), Constraint::Fix(b), Constraint::Horizontal(a, b)]);
    let before = sk.params.clone();
    assert!(!solve(&mut sk, &[]));
    assert_eq!(sk.params, before);
}

#[test]
fn profiles_found() {
    let mut sk = Sketch::new();
    rect(&mut sk, v(0.0, 0.0), v(10.0, 10.0));
    let c = sk.add_point(v(5.0, 5.0));
    sk.add_circle(c, 2.0);
    let a = sk.add_point(v(20.0, 0.0));
    let b = sk.add_point(v(30.0, 0.0));
    sk.add_line(a, b);
    let loops = profiles(&sk);
    assert_eq!(loops.len(), 2);
    let quad = loops.iter().find(|l| l.len() == 4).unwrap();
    assert_eq!(triangulate(quad).len(), 2);
}

#[test]
fn concave_triangulation() {
    let l = [v(0.0, 0.0), v(4.0, 0.0), v(4.0, 1.0), v(1.0, 1.0), v(1.0, 4.0), v(0.0, 4.0)];
    assert_eq!(triangulate(&l).len(), 4);
}

#[test]
fn solve_speed() {
    let mut sk = Sketch::new();
    for i in 0..50 {
        let o = v(i as f64 * 20.0, 0.0);
        let l = rect(&mut sk, o, o + v(10.0, 5.0));
        let Entity::Line { a, b } = sk.entity(l[0]).unwrap() else { panic!() };
        sk.constraints.push(Constraint::Distance(a, b, 12.0));
    }
    let t = std::time::Instant::now();
    assert!(solve(&mut sk, &[]));
    let solved = t.elapsed();
    let dof = analyze(&sk).dof;
    eprintln!("200 lines: solve {solved:?}, analyze {:?}", t.elapsed() - solved);
    assert_eq!(dof, 50 * 3);
}

#[test]
fn regions_nest_holes() {
    let mut sk = Sketch::new();
    rect(&mut sk, v(0.0, 0.0), v(10.0, 10.0));
    let c = sk.add_point(v(5.0, 5.0));
    sk.add_circle(c, 2.0);
    let c2 = sk.add_point(v(5.0, 5.0));
    sk.add_circle(c2, 1.0);
    let loops = loops(&sk);
    let regions = regions(&loops);
    assert_eq!(regions.len(), 3);
    assert_eq!(regions.iter().filter(|r| r.depth % 2 == 0).count(), 2);
    let outer = regions.iter().find(|r| loops[r.outer].segs.len() == 4).unwrap();
    assert_eq!(outer.holes.len(), 1);
    assert!(outer.contains(&loops, v(1.0, 1.0)));
    assert!(!outer.contains(&loops, v(5.0, 6.5)));
}

#[test]
fn fraction_follows_line() {
    let mut sk = Sketch::new();
    let a = sk.add_point(v(0.0, 0.0));
    let b = sk.add_point(v(30.0, 0.0));
    let l = sk.add_line(a, b);
    let p = sk.add_point(v(10.0, 0.0));
    sk.constraints.extend([Constraint::Fix(a), Constraint::Fraction(p, l, 1.0 / 3.0)]);
    assert!(solve(&mut sk, &[(b, 60.0), (b + 1, 30.0)]));
    assert!(sk.pt(p).distance(sk.pt(b) / 3.0) < 1e-6, "{:?}", sk.pt(p));
    assert_eq!(analyze(&sk).dof, 2);
}

#[test]
fn construction_is_not_a_profile() {
    let mut sk = Sketch::new();
    let l = rect(&mut sk, v(0.0, 0.0), v(10.0, 10.0));
    let c = sk.add_point(v(5.0, 5.0));
    let circle = sk.add_circle(c, 2.0);
    assert_eq!(loops(&sk).len(), 2);
    sk.toggle_construction(circle);
    assert_eq!(loops(&sk).len(), 1);
    sk.toggle_construction(l[0]);
    assert!(loops(&sk).is_empty(), "an open rectangle with a centerline side has no profile");
    sk.remove_entity(l[0]);
    assert!(sk.construction == vec![circle]);
}
