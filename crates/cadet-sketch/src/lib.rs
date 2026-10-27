mod profile;
mod solve;

pub use profile::{Loop, Region, Seg, loops, polyline, profiles, regions, triangulate};
pub use solve::{Analysis, analyze, solve};

use glam::DVec2;
use serde::{Deserialize, Serialize};

/// A point id: index of its x coordinate in `Sketch::params` (y follows).
pub type P = usize;
pub type E = usize;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Entity {
    Line { a: P, b: P },
    Circle { c: P, r: usize },
    Arc { c: P, a: P, b: P },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Constraint {
    Coincident(P, P),
    Horizontal(P, P),
    Vertical(P, P),
    Parallel(E, E),
    Perpendicular(E, E),
    Equal(E, E),
    Tangent(E, E),
    OnCurve(P, E),
    Distance(P, P, f64),
    Radius(E, f64),
    Fix(P),
    /// Point at fraction t along a line, from its start.
    Fraction(P, E, f64),
}

impl Constraint {
    pub fn value(&self) -> Option<f64> {
        match *self {
            Constraint::Distance(_, _, v) | Constraint::Radius(_, v) => Some(v),
            _ => None,
        }
    }

    pub fn set_value(&mut self, v: f64) {
        if let Constraint::Distance(_, _, x) | Constraint::Radius(_, x) = self {
            *x = v;
        }
    }

    fn points(&self) -> Vec<P> {
        use Constraint::*;
        match *self {
            Coincident(a, b) | Horizontal(a, b) | Vertical(a, b) | Distance(a, b, _) => vec![a, b],
            OnCurve(p, _) | Fix(p) | Fraction(p, _, _) => vec![p],
            _ => vec![],
        }
    }

    fn entities(&self) -> Vec<E> {
        use Constraint::*;
        match *self {
            Parallel(a, b) | Perpendicular(a, b) | Equal(a, b) | Tangent(a, b) => vec![a, b],
            OnCurve(_, e) | Radius(e, _) | Fraction(_, e, _) => vec![e],
            _ => vec![],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sketch {
    pub params: Vec<f64>,
    pub points: Vec<P>,
    pub entities: Vec<Option<Entity>>,
    pub constraints: Vec<Constraint>,
    /// Construction (center) geometry: drawn dash-dot, used for snapping and axes, ignored by profiles.
    #[serde(default)]
    pub construction: Vec<E>,
}

impl Default for Sketch {
    fn default() -> Self {
        Self::new()
    }
}

impl Sketch {
    pub const ORIGIN: P = 0;

    pub fn new() -> Self {
        Self { params: vec![0.0, 0.0], points: vec![Self::ORIGIN], entities: vec![], constraints: vec![], construction: vec![] }
    }

    pub fn pt(&self, p: P) -> DVec2 {
        DVec2::new(self.params[p], self.params[p + 1])
    }

    pub fn set_pt(&mut self, p: P, v: DVec2) {
        self.params[p] = v.x;
        self.params[p + 1] = v.y;
    }

    pub fn add_point(&mut self, v: DVec2) -> P {
        let p = self.params.len();
        self.params.extend([v.x, v.y]);
        self.points.push(p);
        p
    }

    fn add(&mut self, e: Entity) -> E {
        self.entities.push(Some(e));
        self.entities.len() - 1
    }

    pub fn add_line(&mut self, a: P, b: P) -> E {
        self.add(Entity::Line { a, b })
    }

    pub fn add_circle(&mut self, c: P, r: f64) -> E {
        self.params.push(r);
        let r = self.params.len() - 1;
        self.add(Entity::Circle { c, r })
    }

    pub fn add_arc(&mut self, c: P, a: P, b: P) -> E {
        self.add(Entity::Arc { c, a, b })
    }

    pub fn is_construction(&self, e: E) -> bool {
        self.construction.contains(&e)
    }

    pub fn toggle_construction(&mut self, e: E) {
        match self.construction.iter().position(|&c| c == e) {
            Some(i) => {
                self.construction.remove(i);
            }
            None => self.construction.push(e),
        }
    }

    pub fn entity(&self, e: E) -> Option<Entity> {
        self.entities.get(e).copied().flatten()
    }

    pub fn live(&self) -> impl Iterator<Item = (E, Entity)> + '_ {
        self.entities.iter().enumerate().filter_map(|(i, e)| e.map(|e| (i, e)))
    }

    pub fn radius(&self, e: E) -> f64 {
        match self.entity(e) {
            Some(Entity::Circle { r, .. }) => self.params[r],
            Some(Entity::Arc { c, a, .. }) => self.pt(a).distance(self.pt(c)),
            _ => 0.0,
        }
    }

    pub fn entity_points(e: Entity) -> Vec<P> {
        match e {
            Entity::Line { a, b } => vec![a, b],
            Entity::Circle { c, .. } => vec![c],
            Entity::Arc { c, a, b } => vec![c, a, b],
        }
    }

    pub fn remove_entity(&mut self, e: E) {
        self.entities[e] = None;
        self.construction.retain(|&c| c != e);
        let used: Vec<P> = std::iter::once(Self::ORIGIN).chain(self.live().flat_map(|(_, e)| Self::entity_points(e))).collect();
        self.points.retain(|p| used.contains(p));
        self.constraints.retain(|c| c.entities().iter().all(|&x| self.entities[x].is_some()) && c.points().iter().all(|p| used.contains(p)));
    }

    pub fn point_used_by(&self, p: P) -> impl Iterator<Item = E> + '_ {
        self.live().filter(move |(_, e)| Self::entity_points(*e).contains(&p)).map(|(i, _)| i)
    }
}

#[cfg(test)]
mod tests;
