// SPDX-License-Identifier: MIT
//! A handle owns the primary press until release or cancellation. Only frames solve samples.
use super::{
    Editor, Selection,
    draft::{Draft, State, Tool, solve_message},
    geometry, rules,
};
use ferritecad_document::{Point2, Sketch, SketchGeometry, SketchPointRef, SketchPointSelector};
use ferritecad_eval::SketchDrag;
use ferritecad_ui::{PointerButton, ViewportEvent, ViewportInput};
use ferritecad_viewport::Camera;

const NO_SOLVER: &str =
    "This build has no sketch solver, so constrained drawings cannot be dragged.";

/// Events following a released gesture whose final sample still awaits a frame.
#[derive(Debug)]
pub(super) struct Input {
    event: ViewportEvent,
    claimed: bool,
    editable: bool,
    shift: bool,
}

#[derive(Debug)]
enum Motion {
    Armed,
    Free,
    Constrained(Box<SketchDrag>),
    Refused,
}

#[derive(Debug)]
pub(super) struct Gesture {
    handle: SketchPointRef,
    anchor: Point2,
    press: (f32, f32),
    before: State,
    motion: Motion,
    sample: Option<[f64; 2]>,
    released: bool,
}

impl Gesture {
    pub(super) fn pending_frame(&self) -> bool {
        self.sample.is_some() || self.released
    }

    fn start(&mut self, draft: &mut Draft) {
        if draft.state.constraints.is_empty() {
            self.motion = Motion::Free;
            return;
        }
        if !ferritecad_eval::solver_available() {
            draft.message = NO_SOLVER.into();
            self.motion = Motion::Refused;
            return;
        }
        #[cfg(test)]
        STARTS.with(|n| n.set(n.get() + 1));
        let sketch = Sketch {
            plane: draft.planes[draft.plane].id,
            curves: draft.state.curves.clone(),
            constraints: draft.state.constraints.clone(),
        };
        self.motion = match SketchDrag::begin(sketch, draft.sketch, self.handle) {
            Ok(drag) => Motion::Constrained(Box::new(drag)),
            Err(e) => {
                draft.message = solve_message(&e);
                Motion::Refused
            }
        };
    }

    fn sample(&mut self, draft: &mut Draft, target: Point2) {
        match &mut self.motion {
            Motion::Free => {
                let curve = draft
                    .state
                    .curves
                    .iter_mut()
                    .find(|c| c.id == self.handle.curve)
                    .expect("the press owns a curve in the draft");
                let coordinate = match (&mut curve.geometry, self.handle.at) {
                    (SketchGeometry::Line { start, .. }, SketchPointSelector::Start) => start,
                    (SketchGeometry::Line { end, .. }, SketchPointSelector::End) => end,
                    (SketchGeometry::Circle { center, .. }, SketchPointSelector::Center) => center,
                    (SketchGeometry::Point { at }, SketchPointSelector::At) => at,
                    _ => unreachable!("only a selectable handle can own a drag"),
                };
                *coordinate = target;
                draft.message.clear();
            }
            Motion::Constrained(drag) => {
                #[cfg(test)]
                SAMPLES.with(|n| n.set(n.get() + 1));
                match drag.move_to(target) {
                    Ok(solved) => {
                        draft.state.curves = solved.curves;
                        draft.message.clear();
                    }
                    Err(e) => draft.message = solve_message(&e),
                }
            }
            Motion::Armed | Motion::Refused => {}
        }
    }

    fn finish(self, draft: &mut Draft) {
        let changed = draft
            .state
            .curves
            .iter()
            .zip(&self.before.curves)
            .any(|(a, b)| match &self.motion {
                Motion::Free => a.geometry != b.geometry,
                Motion::Constrained(_) => !super::prediction::agrees(&a.geometry, &b.geometry),
                Motion::Armed | Motion::Refused => false,
            });
        if changed {
            draft.undo.push(self.before);
            draft.redo.clear();
        } else {
            draft.state = self.before;
        }
    }
}

pub(super) fn cancel(gesture: &mut Option<Gesture>, draft: &mut Draft) {
    if let Some(g) = gesture.take() {
        draft.state = g.before;
        draft.message.clear();
    }
}

impl Editor {
    /// Give the view every event except a primary gesture that began on an editable handle.
    pub(crate) fn route(
        &mut self,
        input: &mut ViewportInput,
        event: ViewportEvent,
        claimed: bool,
        editable: bool,
        shift: bool,
    ) {
        if !self.deferred.is_empty() || self.gesture.as_ref().is_some_and(|g| g.released) {
            self.deferred.push_back(Input {
                event,
                claimed,
                editable,
                shift,
            });
            input.request_redraw();
            return;
        }
        self.deliver(input, event, claimed, editable, shift);
    }

    pub(super) fn resume_events(&mut self, input: &mut ViewportInput) {
        while !self.gesture.as_ref().is_some_and(|g| g.released) {
            let Some(next) = self.deferred.pop_front() else {
                break;
            };
            self.deliver(input, next.event, next.claimed, next.editable, next.shift);
        }
    }

    fn deliver(
        &mut self,
        input: &mut ViewportInput,
        event: ViewportEvent,
        claimed: bool,
        editable: bool,
        shift: bool,
    ) {
        let capture = self.capture(input, event, claimed, editable, shift);
        input.handle(event, claimed || capture);
        if capture {
            input.request_redraw();
        }
    }

    fn capture(
        &mut self,
        input: &ViewportInput,
        event: ViewportEvent,
        claimed: bool,
        editable: bool,
        shift: bool,
    ) -> bool {
        let Some(draft) = &mut self.draft else {
            return false;
        };
        match event {
            ViewportEvent::PointerPressed(PointerButton::Primary) => {
                if claimed || !editable || draft.tool != Tool::Select || input.is_dragging() {
                    return false;
                }
                if self.gesture.is_some() {
                    return true;
                }
                let Some(press) = input
                    .pointer_position()
                    .filter(|&p| input.region().contains(p))
                else {
                    return false;
                };
                let Some(plane) = draft.planes[draft.plane].frame else {
                    return false;
                };
                let (x, y) = input.region().local(press);
                let hit = geometry::selection(
                    &draft.state.curves,
                    input.camera(),
                    plane,
                    [f64::from(x), f64::from(y)],
                    &draft.state.selected,
                    shift,
                    &self.labels,
                );
                let Some(Selection::Point(handle)) = hit else {
                    return false;
                };
                let anchor = rules::position(&draft.state.curves, handle).expect("selected handle");
                self.gesture = Some(Gesture {
                    handle,
                    anchor,
                    press,
                    before: draft.state.clone(),
                    motion: Motion::Armed,
                    sample: None,
                    released: false,
                });
                self.snapping.held = None;
                true
            }
            ViewportEvent::PointerMoved { x, y } => {
                let Some(g) = &mut self.gesture else {
                    return false;
                };
                if !g.released
                    && (g.sample.is_some()
                        || !matches!(g.motion, Motion::Armed)
                        || !ViewportInput::within_click_slop(g.press, (x, y)))
                {
                    let (x, y) = input.region().local((x, y));
                    g.sample = Some([f64::from(x), f64::from(y)]);
                }
                true
            }
            ViewportEvent::PointerReleased(PointerButton::Primary) => {
                let Some(g) = &mut self.gesture else {
                    return false;
                };
                if matches!(g.motion, Motion::Armed) && g.sample.is_none() {
                    draft.select(Some(Selection::Point(g.handle)), shift);
                    self.gesture = None;
                } else if g.sample.is_none() {
                    self.gesture.take().expect("active gesture").finish(draft);
                } else {
                    g.released = true;
                }
                self.snapping.held = None;
                true
            }
            ViewportEvent::GestureCancelled => {
                let active = self.gesture.is_some();
                cancel(&mut self.gesture, draft);
                self.snapping.held = None;
                active
            }
            _ => false,
        }
    }

    pub(super) fn drag_frame(&mut self, camera: &Camera) -> bool {
        let (Some(draft), Some(g)) = (&mut self.draft, &mut self.gesture) else {
            return false;
        };
        if let Some(pixel) = g.sample.take() {
            if matches!(g.motion, Motion::Armed) {
                g.start(draft);
            }
            if let Some(plane) = draft.planes[draft.plane].frame
                && let Some(raw) = geometry::point_on_plane(camera, pixel, plane)
            {
                let object = geometry::snap(
                    geometry::targets_except(&draft.state.curves, Some(g.handle)),
                    camera,
                    plane,
                    pixel,
                );
                let (target, snapped) =
                    self.snapping
                        .resolve(raw, object, Some(g.anchor), camera, plane);
                self.cursor = Some(target);
                self.snapped = snapped.then_some(target);
                self.status.clear();
                g.sample(draft, target);
            }
        }
        if g.released {
            self.gesture.take().expect("released gesture").finish(draft);
            self.snapping.held = None;
        }
        true
    }
}

#[cfg(test)]
thread_local! {
    pub(super) static STARTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static SAMPLES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve_fixtures::{line, point};
    use ferritecad_document::{SketchConstraint, SketchConstraintRule};
    use ferritecad_types::StableEntityId;
    #[test]
    fn a_refused_sample_retains_the_last_solution_and_the_next_sample_recovers() {
        if !ferritecad_fixtures::native_dependencies(&[(
            ferritecad_eval::solver_available(),
            "PlaneGCS",
        )]) {
            return;
        }
        let mut draft = Draft::new("Line".into(), super::super::draft::planes(&[], &[]), None);
        let side = line([0., 0.], [10., 0.]);
        let a = SketchPointRef::new(side.id, SketchPointSelector::Start);
        let b = SketchPointRef::new(side.id, SketchPointSelector::End);
        draft.state.curves.push(side);
        draft.state.constraints.push(SketchConstraint {
            id: StableEntityId::new(),
            rule: SketchConstraintRule::Horizontal { a, b },
        });
        let mut g = Gesture {
            handle: b,
            anchor: point(10., 0.),
            press: (4., 4.),
            before: draft.state.clone(),
            motion: Motion::Armed,
            sample: None,
            released: false,
        };
        g.start(&mut draft);
        g.sample(&mut draft, point(15., 3.));
        let last = draft.state.clone();
        assert_eq!(draft.message, "");
        g.sample(&mut draft, Point2 { x: f64::NAN, y: 4. });
        assert_eq!(
            draft.message,
            format!("constraint problem: {b} has a coordinate that is not a number")
        );
        assert_eq!(draft.state, last);
        assert!(draft.undo.is_empty());
        g.sample(&mut draft, point(17., 5.));
        assert_eq!(draft.message, "");
        assert_ne!(draft.state, last);
    }
}
