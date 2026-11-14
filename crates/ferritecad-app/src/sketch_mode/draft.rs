// SPDX-License-Identifier: MIT
//! Disposable drawing steps and the single ordered transaction they describe.
use ferritecad_document::{
    DatumPlane, EditChange, ObjectPayload, ObjectRecord, Point2, Sketch, SketchCurve,
    SketchGeometry,
};
use ferritecad_kernel::SketchPlane;
use ferritecad_types::{ObjectId, Point3, StableEntityId, Transform, Vec3};
use std::collections::BTreeSet;

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

pub(super) fn sketch_name(objects: &[ObjectRecord]) -> String {
    (1..)
        .map(|n| format!("Sketch {n}"))
        .find(|name| !objects.iter().any(|o| o.name.as_ref() == Some(name)))
        .expect("a finite document has an unused name")
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
    pub selected: BTreeSet<StableEntityId>,
}

#[derive(Debug)]
pub(super) struct Draft {
    pub sketch: ObjectId,
    pub name: String,
    pub original: Option<Sketch>,
    /// Solved facts retained for snapping, including curves deleted in this draft.
    pub accepted: Vec<SketchCurve>,
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
            original: None,
            accepted: Vec::new(),
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

    pub fn select(&mut self, id: Option<StableEntityId>, shift: bool) {
        if !shift || id.is_none() {
            self.state.selected.clear();
        }
        if let Some(id) = id
            && !self.state.selected.insert(id)
        {
            self.state.selected.remove(&id);
        }
    }

    pub fn delete(&mut self) {
        if self.state.selected.is_empty() {
            return;
        }
        self.begin_step();
        self.state
            .curves
            .retain(|c| !self.state.selected.contains(&c.id));
        self.state.selected.clear();
        self.pending = None;
    }

    pub fn toggle(&mut self) {
        if self.state.selected.is_empty() {
            return;
        }
        let construction = !self
            .state
            .curves
            .iter()
            .filter(|c| self.state.selected.contains(&c.id))
            .all(|c| c.construction);
        self.begin_step();
        for c in &mut self.state.curves {
            if self.state.selected.contains(&c.id) {
                c.construction = construction;
            }
        }
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

    /// One ordered request; original saved geometry is never replaced by solved geometry.
    pub fn changes(&self) -> Vec<EditChange> {
        let sketch = self.sketch;
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
                sketch: Sketch {
                    plane: plane.id,
                    curves: self.state.curves.clone(),
                    constraints: Vec::new(),
                },
            });
            return changes;
        };
        let mut changes: Vec<_> = self
            .removed_constraints()
            .into_iter()
            .map(|constraint| EditChange::RemoveConstraint { sketch, constraint })
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
        changes
    }

    pub fn counts(&self) -> (usize, usize, usize) {
        let Some(original) = &self.original else {
            return (self.state.curves.len(), 0, 0);
        };
        let added = self
            .state
            .curves
            .iter()
            .filter(|c| !original.curves.iter().any(|o| o.id == c.id))
            .count();
        let removed = original
            .curves
            .iter()
            .filter(|o| !self.state.curves.iter().any(|c| c.id == o.id))
            .count();
        let changed = self
            .state
            .curves
            .iter()
            .filter(|c| {
                original
                    .curves
                    .iter()
                    .any(|o| o.id == c.id && o.construction != c.construction)
            })
            .count();
        (added, removed, changed)
    }
}
