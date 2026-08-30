// SPDX-License-Identifier: MIT
//! Turning one sketch's presentation into something a viewport can draw.
//!
//! The one place this conversion happens. A [`SketchPresentation`] is the
//! document's own vocabulary – a plane, a circle, an arc's two angles, a flag
//! saying a curve guides rather than bounds – and a [`SketchDrawing`] is world
//! space and nothing else. This crate is where the two meet, because it is
//! already the only one that knows both what an evaluation produced and what a
//! viewport consumes; putting the arithmetic in the viewport would make a
//! renderer read documents, and putting it in the renderer would make a device
//! decide what a sketch means.
//!
//! # The plane is the plane
//!
//! Every coordinate goes through [`SketchPlane::to_model`], so a sketch on a
//! rotated, offset datum is drawn where that datum is. A drawing that assumed
//! the world XY plane would put every sketch of a document in one place and
//! look perfectly plausible for the one document whose datum happens to be
//! there.
//!
//! # A circle is closed and an arc is not
//!
//! A circle's run ends on the very point it began on – the first sample is
//! pushed again rather than recomputed – so it closes exactly rather than to
//! within a rounding. An arc's angle is walked from `start_angle` to
//! `end_angle`, in that order and with that sign, which is the same walk
//! `ferritecad-occt` makes when it builds one: an arc drawn the short way
//! round when the document said the long way is a different arc.
//!
//! # Nothing here is a second sketch model
//!
//! What comes out has no curve identifier, no plane, no angles and no
//! constraint. The durable name of a curve is not a drawing fact and does not
//! travel: it is what the next slice will need to point at one, and inventing
//! a channel for it now would be inventing a meaning for a pixel that this
//! slice deliberately does not give.

use ferritecad_document::SketchGeometry;
use ferritecad_eval::SketchPresentation;
use ferritecad_kernel::{PlanarPoint, SketchPlane};
use ferritecad_types::{CadError, Result};
use ferritecad_viewport::{SketchDrawing, SketchDrawingBuilder, SketchStyle};

/// How many segments a whole circle is drawn with.
///
/// Fixed rather than chosen from the camera, because the buffers are built
/// once when a document is opened and a camera that changed them would upload
/// geometry every time somebody orbited. The chord error is the radius times
/// about `1.5e-4` at this count, which is well under a pixel for anything a
/// screen can show at once, and an arc uses the same angular step so a quarter
/// circle is drawn as finely as a whole one.
pub const CIRCLE_SEGMENTS: u32 = 256;

/// The largest angle between two samples of a circle or an arc.
fn angular_step() -> f64 {
    std::f64::consts::TAU / f64::from(CIRCLE_SEGMENTS)
}

/// The drawing of one presented sketch, in world space.
///
/// Fallible for one reason: a coordinate the document stores may have no
/// `f32`, and a buffer holding an infinity draws nothing at all. The refusal
/// travels rather than being clamped, so a document with a coordinate no
/// picture can hold says so instead of being silently redrawn somewhere else.
pub fn sketch_drawing(presentation: &SketchPresentation) -> Result<SketchDrawing> {
    let plane = presentation.plane();
    let mut builder = SketchDrawingBuilder::new();
    for curve in presentation.curves() {
        let style = if curve.is_construction() {
            SketchStyle::Construction
        } else {
            SketchStyle::Model
        };
        match curve.geometry() {
            SketchGeometry::Point { at } => {
                builder.point(style, world(&plane, at.x, at.y)?)?;
            }
            SketchGeometry::Line { start, end } => {
                builder.stroke(
                    style,
                    &[
                        world(&plane, start.x, start.y)?,
                        world(&plane, end.x, end.y)?,
                    ],
                )?;
            }
            SketchGeometry::Circle { center, radius } => {
                builder.stroke(style, &circle(&plane, center.x, center.y, *radius)?)?;
            }
            SketchGeometry::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                builder.stroke(
                    style,
                    &arc(
                        &plane,
                        center.x,
                        center.y,
                        *radius,
                        *start_angle,
                        *end_angle,
                    )?,
                )?;
            }
            // The enum is `non_exhaustive`, so a kind this build has never
            // heard of is expressible. Refused rather than skipped: a drawing
            // quietly missing a curve looks exactly like a drawing, and the
            // person looking at it has no way to tell.
            other => {
                return Err(CadError::unsupported(format!(
                    "sketch curve {} has geometry {other:?}, which this build cannot draw",
                    curve.id()
                )));
            }
        }
    }
    Ok(builder.build())
}

/// The drawings of every presented sketch, in the order they were given.
///
/// One drawing per sketch, including a sketch nothing was raised from: a
/// drawing exists because somebody drew it, and what read it afterwards is not
/// a fact about whether it is there.
pub fn sketch_drawings(presentations: &[SketchPresentation]) -> Result<Vec<SketchDrawing>> {
    presentations.iter().map(sketch_drawing).collect()
}

/// One point of the plane, in model space.
fn world(plane: &SketchPlane, x: f64, y: f64) -> Result<[f64; 3]> {
    let point = plane.to_model(PlanarPoint::new(x, y)?)?;
    Ok([point.x, point.y, point.z])
}

/// A closed run around a circle, ending on the point it started on.
fn circle(plane: &SketchPlane, x: f64, y: f64, radius: f64) -> Result<Vec<[f64; 3]>> {
    let step = angular_step();
    let mut points = Vec::with_capacity(CIRCLE_SEGMENTS as usize + 1);
    for index in 0..CIRCLE_SEGMENTS {
        let angle = f64::from(index) * step;
        points.push(world(
            plane,
            x + radius * angle.cos(),
            y + radius * angle.sin(),
        )?);
    }
    // The first sample again, not a recomputation of it. A circle drawn from
    // an angle of `TAU` would land a rounding away from where it started and
    // leave a gap of a fraction of a pixel that no tolerance explains.
    let first = *points.first().expect("a circle is sampled at least once");
    points.push(first);
    Ok(points)
}

/// A run along an arc, from `start_angle` towards `end_angle`.
///
/// The sweep keeps its sign, so an arc the document says runs clockwise is
/// drawn clockwise, and the ends land exactly on the angles the document
/// stores rather than on the nearest sample.
fn arc(
    plane: &SketchPlane,
    x: f64,
    y: f64,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
) -> Result<Vec<[f64; 3]>> {
    let sweep = end_angle - start_angle;
    // At least one segment, so an arc of no sweep is still two points and
    // still a run rather than a refusal.
    let segments = ((sweep.abs() / angular_step()).ceil() as u64).max(1);
    let mut points = Vec::with_capacity(segments as usize + 1);
    for index in 0..=segments {
        // From the ends inwards: the last sample is `start + sweep` written
        // out, so it is the stored end angle and not `segments` steps of a
        // rounded increment away from it.
        let along = index as f64 / segments as f64;
        let angle = start_angle + sweep * along;
        points.push(world(
            plane,
            x + radius * angle.cos(),
            y + radius * angle.sin(),
        )?);
    }
    Ok(points)
}
