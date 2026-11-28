// SPDX-License-Identifier: MIT
//! Disposable drawing steps and the single ordered transaction they describe.
use super::rules::{Button, Selection};
use ferritecad_document::{
    DatumPlane, EditChange, ObjectPayload, ObjectRecord, Point2, Sketch, SketchConstraint,
    SketchCurve, SketchGeometry,
};
use ferritecad_kernel::SketchPlane;
use ferritecad_types::{ObjectId, Point3, StableEntityId, Transform, Vec3};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub(super) struct Plane {
    pub id: ObjectId,
    pub name: String,
    pub frame: Option<SketchPlane>,
    pub addition: Option<Transform>,
}

pub(super) fn planes(objects: &[ObjectRecord], built: &[(ObjectId, SketchPlane)]) -> Vec<Plane> {
    let mut choices: Vec<_> = objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::DatumPlane(_)))
        .map(|o| Plane {
            id: o.id,
            name: o
                .name
                .clone()
                .unwrap_or_else(|| format!("Datum plane {}", o.id)),
            frame: built.iter().find(|(id, _)| *id == o.id).map(|(_, p)| *p),
            addition: None,
        })
        .collect();
    for (name, x, y) in [
        ("XY", Vec3::X, Vec3::Y),
        ("XZ", Vec3::X, Vec3::Z),
        ("YZ", Vec3::Y, Vec3::Z),
    ] {
        let normal = x.cross(y).expect("standard axes are finite");
        let placement = Transform::from_rows([
            [x.x, y.x, normal.x, 0.],
            [x.y, y.y, normal.y, 0.],
            [x.z, y.z, normal.z, 0.],
        ])
        .expect("standard frame is rigid");
        if objects.iter().any(|o| {
            matches!(&o.payload,
            ObjectPayload::DatumPlane(DatumPlane::Fixed { placement: saved })
                if *saved == placement)
        }) {
            continue;
        }
        choices.push(Plane {
            id: ObjectId::new(),
            name: name.into(),
            addition: Some(placement),
            frame: Some(
                SketchPlane::new(Point3::ORIGIN, x, normal)
                    .expect("standard plane axes are orthogonal"),
            ),
        });
    }
    choices
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Tool {
    #[default]
    Select,
    Line,
    Circle,
    Point,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Line => "Line",
            Self::Circle => "Circle",
            Self::Point => "Point",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Pending {
    Line { first: Point2, last: Point2 },
    Circle(Point2),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct State {
    pub curves: Vec<SketchCurve>,
    pub constraints: Vec<SketchConstraint>,
    pub selected: Vec<Selection>,
    pub diameters: BTreeSet<StableEntityId>,
    pub report: Option<ferritecad_eval::SketchSolveReport>,
}

#[derive(Debug)]
pub(super) struct Draft {
    pub sketch: ObjectId,
    pub name: String,
    pub message: String,
    pub original: Option<Sketch>,
    /// Construction flags and solved geometry shown when this draft opened.
    opened_curves: BTreeMap<StableEntityId, (bool, SketchGeometry)>,
    pub planes: Vec<Plane>,
    pub plane: usize,
    pub state: State,
    pub undo: Vec<State>,
    pub redo: Vec<State>,
    pub tool: Tool,
    pub construction: bool,
    pub pending: Option<Pending>,
}

impl Draft {
    pub fn new(name: String, planes: Vec<Plane>, chosen: Option<ObjectId>) -> Self {
        let plane = chosen
            .and_then(|id| planes.iter().position(|p| p.id == id))
            .unwrap_or(0);
        Self {
            sketch: ObjectId::new(),
            name,
            message: String::new(),
            original: None,
            opened_curves: BTreeMap::new(),
            planes,
            plane,
            state: State::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            tool: Tool::Select,
            construction: false,
            pending: None,
        }
    }

    pub fn begin_step(&mut self) {
        self.undo.push(self.state.clone());
        self.redo.clear();
    }

    pub fn history(&mut self, undo: bool) {
        let (from, to) = if undo {
            (&mut self.undo, &mut self.redo)
        } else {
            (&mut self.redo, &mut self.undo)
        };
        if let Some(state) = from.pop() {
            to.push(std::mem::replace(&mut self.state, state));
            self.pending = None;
            self.message.clear();
        }
    }

    pub fn escape(&mut self) {
        if self.pending.take().is_none() {
            self.tool = Tool::Select;
        }
    }

    fn add(&mut self, geometry: SketchGeometry) {
        self.begin_step();
        self.state.curves.push(SketchCurve {
            id: StableEntityId::new(),
            construction: self.construction,
            geometry,
        });
    }

    pub fn click(&mut self, point: Point2) {
        match self.tool {
            Tool::Select => {}
            Tool::Point => self.add(SketchGeometry::Point { at: point }),
            Tool::Line => match self.pending {
                Some(Pending::Line { first, last }) => {
                    if point == last {
                        self.pending = None;
                        return;
                    }
                    self.add(SketchGeometry::Line {
                        start: last,
                        end: point,
                    });
                    self.pending = (point != first).then_some(Pending::Line { first, last: point });
                }
                Some(Pending::Circle(_)) | None => {
                    self.pending = Some(Pending::Line {
                        first: point,
                        last: point,
                    });
                }
            },
            Tool::Circle => match self.pending.take() {
                Some(Pending::Circle(center)) => {
                    let radius = (point.x - center.x).hypot(point.y - center.y);
                    if radius > 0.0 {
                        self.add(SketchGeometry::Circle { center, radius });
                    }
                }
                Some(Pending::Line { .. }) | None => self.pending = Some(Pending::Circle(point)),
            },
        }
    }

    pub fn select(&mut self, item: Option<Selection>, shift: bool) {
        if !shift || item.is_none() {
            self.state.selected.clear();
        }
        if let Some(item) = item {
            if let Some(index) = self.state.selected.iter().position(|s| *s == item) {
                self.state.selected.remove(index);
            } else {
                self.state.selected.push(item);
            }
        }
    }

    pub fn delete(&mut self) {
        let selected = &self.state.selected;
        let mut next = self.state.clone();
        next.curves
            .retain(|c| !selected.iter().any(|s| s.whole() == Some(c.id)));
        next.constraints.retain(|c| {
            !selected.contains(&Selection::Constraint(c.id))
                && c.rule
                    .curves()
                    .into_iter()
                    .chain(c.rule.points().into_iter().map(|p| p.curve))
                    .all(|id| next.curves.iter().any(|c| c.id == id))
        });
        if next.curves.len() == self.state.curves.len()
            && next.constraints.len() == self.state.constraints.len()
        {
            return;
        }
        next.diameters
            .retain(|id| next.constraints.iter().any(|c| c.id == *id));
        let solve = next.constraints.len() != self.state.constraints.len()
            && ferritecad_eval::solver_available();
        next.selected.clear();
        self.accept(next, solve);
    }

    pub fn toggle(&mut self) {
        let selected: Vec<_> = self
            .state
            .selected
            .iter()
            .filter_map(|s| s.whole())
            .collect();
        if selected.is_empty() {
            return;
        }
        let flag = !self
            .state
            .curves
            .iter()
            .filter(|c| selected.contains(&c.id))
            .all(|c| c.construction);
        self.begin_step();
        for c in &mut self.state.curves {
            if selected.contains(&c.id) {
                c.construction = flag;
            }
        }
    }

    pub fn add_rule(&mut self, rule: ferritecad_document::SketchConstraintRule, button: Button) {
        let mut next = self.state.clone();
        let id = StableEntityId::new();
        next.constraints.push(SketchConstraint { id, rule });
        if button == Button::Diameter {
            next.diameters.insert(id);
        }
        next.selected = vec![Selection::Constraint(id)];
        self.accept(next, true);
    }

    pub fn set_value(&mut self, id: StableEntityId, value: &str) {
        let mut next = self.state.clone();
        let Some(c) = next.constraints.iter_mut().find(|c| c.id == id) else {
            return;
        };
        let Some((_, expr)) = dimension(c) else {
            return;
        };
        if expr.numeric_form_refusal().is_some() {
            self.message = super::rules::READ_ONLY.into();
            return;
        }
        if let Err(e) = super::rules::set_number(&mut c.rule, value, next.diameters.contains(&id)) {
            self.message = e.to_string();
            return;
        }
        // Restore the exact saved spelling when the value is returned to its starting value.
        if let Some(old) = self
            .original
            .iter()
            .flat_map(|s| &s.constraints)
            .find(|k| k.id == id)
            && dimension(old).map(|(_, v)| v.value()) == dimension(c).map(|(_, v)| v.value())
        {
            c.rule = old.rule.clone();
        }
        if next.constraints != self.state.constraints {
            self.accept(next, true);
        } else {
            self.message.clear();
        }
    }

    fn accept(&mut self, mut next: State, solve: bool) {
        if solve {
            let sketch = Sketch {
                plane: self.planes[self.plane].id,
                curves: std::mem::take(&mut next.curves),
                constraints: std::mem::take(&mut next.constraints),
            };
            match solve_drawing(&sketch, self.sketch) {
                Ok(answer) => {
                    let (drawing, report) = match answer {
                        Some((solved, report)) => (solved, Some(report)),
                        None => (sketch, None),
                    };
                    next.curves = drawing.curves;
                    next.constraints = drawing.constraints;
                    next.report = report;
                }
                Err(msg) => {
                    self.message = msg;
                    return;
                }
            }
        }
        self.undo.push(std::mem::replace(&mut self.state, next));
        self.redo.clear();
        self.message.clear();
        self.pending = None;
    }

    pub fn constraint_counts(&self) -> (usize, usize, usize) {
        let original = self
            .original
            .as_ref()
            .map_or(&[][..], |s| s.constraints.as_slice());
        let current = &self.state.constraints;
        let added = current
            .iter()
            .filter(|c| !original.iter().any(|s| s.id == c.id))
            .count();
        let removed = original
            .iter()
            .filter(|c| !current.iter().any(|s| s.id == c.id))
            .count();
        let changed = current
            .iter()
            .filter(|c| original.iter().any(|s| s.id == c.id && *s != **c))
            .count();
        (added, removed, changed)
    }

    pub fn removed_constraints(&self) -> Vec<StableEntityId> {
        self.original
            .iter()
            .flat_map(|s| &s.constraints)
            .filter(|c| {
                c.rule
                    .curves()
                    .into_iter()
                    .chain(c.rule.points().into_iter().map(|p| p.curve))
                    .any(|id| !self.state.curves.iter().any(|curve| curve.id == id))
            })
            .map(|c| c.id)
            .collect()
    }

    /// Assemble the request only after verifying its rebuilt geometry.
    pub fn changes(&self) -> Result<Vec<EditChange>, String> {
        self.changes_with(|s| solve_drawing(s, self.sketch).map(|r| r.map(|(solved, _)| solved)))
    }

    pub fn changes_with(
        &self,
        solve: impl FnMut(&Sketch) -> Result<Option<Sketch>, String>,
    ) -> Result<Vec<EditChange>, String> {
        let sketch = self.sketch;
        let drawing = Sketch {
            plane: self.planes[self.plane].id,
            curves: self.state.curves.clone(),
            constraints: self.state.constraints.clone(),
        };
        let geometry =
            super::prediction::geometry_changes(sketch, self.original.as_ref(), &drawing, solve)?;
        let Some(original) = &self.original else {
            let plane = &self.planes[self.plane];
            let mut changes = Vec::new();
            if let Some(placement) = plane.addition {
                changes.push(EditChange::AddDatumPlane {
                    object: plane.id,
                    name: Some(plane.name.clone()),
                    plane: DatumPlane::Fixed { placement },
                });
            }
            changes.push(EditChange::AddSketch {
                object: sketch,
                name: Some(self.name.clone()),
                sketch: drawing,
            });
            return Ok(changes);
        };
        let mut changes: Vec<_> = original
            .constraints
            .iter()
            .filter(|c| !drawing.constraints.iter().any(|k| k.id == c.id))
            .map(|c| EditChange::RemoveConstraint {
                sketch,
                constraint: c.id,
            })
            .collect();
        for curve in &original.curves {
            if !self.state.curves.iter().any(|c| c.id == curve.id) {
                changes.push(EditChange::RemoveCurve {
                    sketch,
                    curve: curve.id,
                });
            }
        }
        for curve in &self.state.curves {
            if !original.curves.iter().any(|c| c.id == curve.id) {
                changes.push(EditChange::AddCurve {
                    sketch,
                    curve: curve.clone(),
                });
            }
        }
        changes.extend(geometry);
        for curve in &self.state.curves {
            if original
                .curves
                .iter()
                .any(|c| c.id == curve.id && c.construction != curve.construction)
            {
                changes.push(EditChange::SetConstruction {
                    sketch,
                    curve: curve.id,
                    construction: curve.construction,
                });
            }
        }
        for c in &drawing.constraints {
            if !original.constraints.iter().any(|old| old.id == c.id) {
                changes.push(EditChange::AddConstraint {
                    sketch,
                    constraint: c.clone(),
                });
            }
        }
        for c in &drawing.constraints {
            if let Some(old) = original.constraints.iter().find(|old| old.id == c.id)
                && let Some((slot, expr)) = dimension(c)
                && dimension(old).is_some_and(|(_, old)| old != expr)
            {
                changes.push(
                    EditChange::constant(sketch, slot, expr.value()).map_err(|e| e.to_string())?,
                );
            }
        }
        Ok(changes)
    }

    pub(super) fn open_curves(&mut self, curves: Vec<SketchCurve>) {
        self.opened_curves = curves
            .iter()
            .map(|c| (c.id, (c.construction, c.geometry.clone())))
            .collect();
        self.state.curves = curves;
    }

    pub fn counts(&self) -> (usize, usize, usize) {
        let mut added = 0;
        let mut changed = 0;
        for curve in &self.state.curves {
            if let Some((construction, geometry)) = self.opened_curves.get(&curve.id) {
                if curve.construction != *construction
                    || !super::prediction::agrees(geometry, &curve.geometry)
                {
                    changed += 1;
                }
            } else {
                added += 1;
            }
        }
        let retained = self.state.curves.len() - added;
        (added, self.opened_curves.len() - retained, changed)
    }
}

pub(super) fn dimension(
    c: &SketchConstraint,
) -> Option<(
    ferritecad_document::ExpressionSlot,
    &ferritecad_document::Expression,
)> {
    use ferritecad_document::{ExpressionSlot as S, SketchConstraintRule as R};
    let constraint = c.id;
    match &c.rule {
        R::Distance { distance, .. } => Some((S::Distance { constraint }, distance)),
        R::Radius { radius, .. } => Some((S::Radius { constraint }, radius)),
        R::DirectedAngle { angle, .. } => Some((S::DirectedAngle { constraint }, angle)),
        _ => None,
    }
}

fn solve_drawing(
    sketch: &Sketch,
    id: ObjectId,
) -> Result<Option<(Sketch, ferritecad_eval::SketchSolveReport)>, String> {
    #[cfg(test)]
    if !sketch.constraints.is_empty() {
        SOLVES.with(|n| n.set(n.get() + 1));
    }
    ferritecad_eval::solve_sketch(sketch, id).map_err(|e| solve_message(&e))
}

pub(super) fn solve_message(e: &ferritecad_types::CadError) -> String {
    let mut message = e.to_string();
    if let Some(conflict) = ferritecad_eval::SketchConflict::of(e) {
        for c in conflict.constraints() {
            let words = crate::ConstraintWords::of(c.id(), c.rule());
            message.push_str(&format!("\n{}: {}", words.identifier, words.says));
        }
    }
    message
}
#[cfg(test)]
thread_local! {
    pub(super) static SOLVES: std::cell::Cell<usize> = const {std::cell::Cell::new(0)};
}
