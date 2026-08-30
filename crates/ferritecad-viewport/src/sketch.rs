// SPDX-License-Identifier: MIT
//! A drawing to put in a picture: world-space strokes and points.
//!
//! This is the render input for a sketch, and it is deliberately not a sketch.
//! There is no plane here, no circle, no arc, no curve identifier and no
//! construction flag that means anything to a solver: a circle has already
//! become a closed run of world-space points, and an arc a run that starts and
//! ends where its angles put it. What survives is what a renderer needs and
//! nothing a renderer could be tempted to reinterpret.
//!
//! # Why the arithmetic is not here
//!
//! Turning a sketch into these runs needs the plane the sketch sits on, which
//! is a kernel fact, and the curves it holds, which are a document fact. This
//! crate has neither, and giving it either would make a viewport depend on
//! what it is looking at. So the conversion lives at the boundary that already
//! knows both, and what arrives here is already in the world.
//!
//! # Why it is validated on the way in
//!
//! Positions are stored as `f32`, because that is what a vertex buffer holds.
//! A `f64` coordinate a document happily stores can be finite and still have
//! no `f32`, and a buffer holding an infinity draws nothing anywhere rather
//! than something wrong somewhere. [`SketchDrawingBuilder`] refuses one, so a
//! [`SketchDrawing`] that exists is one every coordinate of which is a finite
//! `f32`.
//!
//! # What it is not
//!
//! It is not part of [`RenderSnapshot`][crate::RenderSnapshot], does not enter
//! its bytes, its hash or its bounds, and carries no identity of any kind. A
//! drawing is something to look at in this slice; what a click means is
//! answered entirely by the picture beside it.

use ferritecad_types::{CadError, Result};

/// Whether a piece of a drawing bounds a face or only guides the drawing.
///
/// The one distinction that survives into the render input, because it is the
/// one a person has to be able to see. Everything else about what a curve is –
/// which curve it is, what constrains it, whether anything was raised from it –
/// stays behind the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SketchStyle {
    /// Geometry that could bound a face.
    Model,
    /// Geometry that guides the drawing and bounds nothing.
    Construction,
}

/// One run of world-space points, drawn as a stroke.
///
/// At least two points, consecutive pairs being the segments. A circle is a
/// run whose last point is its first; an arc is a run that is not closed.
/// Nothing here says which it was, because nothing that draws it needs to
/// know.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchStroke {
    points: Vec<[f32; 3]>,
    style: SketchStyle,
}

impl SketchStroke {
    /// The run, in the order it is drawn.
    pub fn points(&self) -> &[[f32; 3]] {
        &self.points
    }

    pub fn style(&self) -> SketchStyle {
        self.style
    }
}

/// One world-space point of a drawing.
///
/// Its own kind rather than a run of length one, because a point is a thing to
/// see and a degenerate stroke is a thing to argue about: a renderer given a
/// run of one point would have to decide what direction it ran in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SketchPoint {
    at: [f32; 3],
    style: SketchStyle,
}

impl SketchPoint {
    pub fn at(&self) -> [f32; 3] {
        self.at
    }

    pub fn style(&self) -> SketchStyle {
        self.style
    }
}

/// One drawing, in world space, ready to be put on a device.
///
/// Immutable and built only through [`SketchDrawingBuilder`], so every
/// coordinate in one has already been checked.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SketchDrawing {
    strokes: Vec<SketchStroke>,
    points: Vec<SketchPoint>,
}

impl SketchDrawing {
    /// Every run of this drawing, in the order it was given.
    pub fn strokes(&self) -> &[SketchStroke] {
        &self.strokes
    }

    /// Every point of it, in the order it was given.
    pub fn points(&self) -> &[SketchPoint] {
        &self.points
    }

    /// Whether there is anything at all to draw.
    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty() && self.points.is_empty()
    }
}

/// Collects a drawing, refusing anything a vertex buffer cannot hold.
#[derive(Debug, Default)]
pub struct SketchDrawingBuilder {
    strokes: Vec<SketchStroke>,
    points: Vec<SketchPoint>,
}

impl SketchDrawingBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a run, refusing one with fewer than two points or a coordinate
    /// that is not a finite `f32`.
    pub fn stroke(&mut self, style: SketchStyle, points: &[[f64; 3]]) -> Result<()> {
        if points.len() < 2 {
            return Err(CadError::input(format!(
                "a stroke is drawn between points and needs at least two, got {}",
                points.len()
            )));
        }
        let mut packed = Vec::new();
        packed
            .try_reserve_exact(points.len())
            .map_err(|error| CadError::rendering_because("collecting a drawing's stroke", error))?;
        for point in points {
            packed.push(drawable(*point)?);
        }
        self.strokes.push(SketchStroke {
            points: packed,
            style,
        });
        Ok(())
    }

    /// Adds a point, refusing a coordinate that is not a finite `f32`.
    pub fn point(&mut self, style: SketchStyle, at: [f64; 3]) -> Result<()> {
        self.points.push(SketchPoint {
            at: drawable(at)?,
            style,
        });
        Ok(())
    }

    pub fn build(self) -> SketchDrawing {
        SketchDrawing {
            strokes: self.strokes,
            points: self.points,
        }
    }
}

/// One position, as a vertex buffer will hold it.
///
/// The cast is where a coordinate is lost, so it is where the refusal is: a
/// value far outside the `f32` range is finite as a `f64` and infinite the
/// moment it is packed, and a buffer full of infinities is a drawing that
/// vanishes rather than one that is visibly wrong.
fn drawable(point: [f64; 3]) -> Result<[f32; 3]> {
    let packed = [point[0] as f32, point[1] as f32, point[2] as f32];
    if packed.iter().all(|value| value.is_finite()) {
        return Ok(packed);
    }
    Err(CadError::input(format!(
        "a drawing's point {point:?} is not somewhere a picture can put it"
    )))
}

#[cfg(test)]
#[allow(clippy::panic, reason = "a gate that cannot fail is not a gate")]
mod tests {
    use super::*;
    use ferritecad_types::ErrorKind;

    #[test]
    fn a_run_of_one_point_is_not_a_stroke() {
        let mut builder = SketchDrawingBuilder::new();
        let refusal = builder
            .stroke(SketchStyle::Model, &[[0.0, 0.0, 0.0]])
            .expect_err("one point is not a segment");
        assert_eq!(refusal.kind(), ErrorKind::Input);
    }

    #[test]
    fn a_coordinate_with_no_f32_is_refused_rather_than_packed_as_an_infinity() {
        let far = 1.0e300_f64;
        assert!(far.is_finite(), "the input is a finite f64");
        let mut builder = SketchDrawingBuilder::new();
        let refusal = builder
            .point(SketchStyle::Model, [far, 0.0, 0.0])
            .expect_err("no f32 holds it");
        assert_eq!(refusal.kind(), ErrorKind::Input);

        let refusal = builder
            .stroke(SketchStyle::Model, &[[0.0, 0.0, 0.0], [0.0, far, 0.0]])
            .expect_err("no f32 holds it");
        assert_eq!(refusal.kind(), ErrorKind::Input);
    }

    #[test]
    fn a_non_finite_coordinate_is_refused() {
        let mut builder = SketchDrawingBuilder::new();
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                builder
                    .point(SketchStyle::Model, [bad, 0.0, 0.0])
                    .expect_err("not a place")
                    .kind(),
                ErrorKind::Input
            );
        }
    }

    #[test]
    fn zero_is_an_ordinary_place_to_draw() {
        let mut builder = SketchDrawingBuilder::new();
        builder
            .point(SketchStyle::Model, [0.0, 0.0, 0.0])
            .expect("the origin is somewhere");
        builder
            .stroke(
                SketchStyle::Construction,
                &[[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
            )
            .expect("a zero-length run is still two points");
        let drawing = builder.build();
        assert!(!drawing.is_empty());
        assert_eq!(drawing.points()[0].style(), SketchStyle::Model);
        assert_eq!(drawing.strokes()[0].style(), SketchStyle::Construction);
    }

    #[test]
    fn an_empty_drawing_says_so() {
        assert!(SketchDrawingBuilder::new().build().is_empty());
    }
}
