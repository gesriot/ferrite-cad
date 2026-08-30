// SPDX-License-Identifier: MIT
//! What a sketch looks like once the rebuild has read it.
//!
//! A [`Profile`][ferritecad_kernel::Profile] is what the kernel extrudes, and
//! it is deliberately less than the drawing: construction geometry bounds no
//! face and is dropped, a circle and a point are refused rather than swept,
//! and what survives is reordered head to tail into one loop. None of that is
//! wrong — it is what an extrusion needs — but it means a profile cannot be
//! turned back into the sketch it came from.
//!
//! So the drawing travels separately, and it travels out of the same
//! evaluation: the coordinates here are the ones the profile was built from,
//! not the ones the document stores, and there is no second solve behind
//! them. For a sketch nobody constrained the two are the same thing, and no
//! solver is asked anything.
//!
//! # Nothing here is a document
//!
//! This is what one rebuild saw, the way a [`SketchSolveReport`] is what one
//! solve found out. It is not serialisable and must not become so: storing a
//! solved coordinate would make a file's meaning depend on which build last
//! opened it, when the constraints are what the drawing means.
//!
//! # Nothing here is a solver's
//!
//! Curves are named by the [`StableEntityId`] the document minted for them and
//! by nothing else — not by where they sit in a list, not by a coordinate, and
//! not by anything a solver numbered for the length of one call. The derived
//! `Debug` can therefore publish no `PointId`, no `ConstraintId` and no native
//! tag, because none is here to publish.
//!
//! [`SketchSolveReport`]: crate::SketchSolveReport

use ferritecad_document::{Sketch, SketchGeometry};
use ferritecad_kernel::SketchPlane;
use ferritecad_types::{ObjectId, StableEntityId};

/// One sketch of a document, at the coordinates the rebuild built from.
///
/// The fields are private and there is no constructor outside this crate.
/// A sketch identifier paired with somebody else's curves, or with the plane
/// of a different datum, would be this type saying something no rebuild ever
/// saw, and the compiler is what stops it being written rather than a test.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchPresentation {
    /// The document's word for this sketch.
    sketch: ObjectId,
    /// Where the sketch actually sits, after its datum's placement was read.
    plane: SketchPlane,
    /// Every curve of it, in the order the document stores them.
    curves: Vec<PresentedCurve>,
}

impl SketchPresentation {
    /// The geometry of `evaluated`, which is whatever the rebuild built the
    /// profile from: the solved sketch when there were constraints, and the
    /// stored one when there were none.
    ///
    /// `plane` is the placement the datum actually resolved to, not the one a
    /// sketch is drawn on in its own local coordinates. Two sketches on two
    /// datums are two drawings in two places, and a presentation that assumed
    /// the world XY plane would put both of them in one.
    pub(crate) fn of(sketch: ObjectId, plane: SketchPlane, evaluated: &Sketch) -> Self {
        Self {
            sketch,
            plane,
            curves: evaluated
                .curves
                .iter()
                .map(|curve| PresentedCurve {
                    id: curve.id,
                    construction: curve.construction,
                    geometry: curve.geometry.clone(),
                })
                .collect(),
        }
    }

    /// The sketch this drawing belongs to.
    pub fn sketch(&self) -> ObjectId {
        self.sketch
    }

    /// The plane it is drawn on, in model space.
    pub fn plane(&self) -> SketchPlane {
        self.plane
    }

    /// Every curve, in the order the document stores them.
    ///
    /// Document order, because that is the order the person who drew it sees.
    /// Not the order a profile chains them into: chaining is arithmetic an
    /// extrusion needs, it drops everything that bounds no face, and a drawing
    /// that reordered itself to suit one consumer would be a different drawing
    /// for the next one.
    pub fn curves(&self) -> &[PresentedCurve] {
        &self.curves
    }
}

/// One curve of a drawing, named durably and carried whole.
///
/// The geometry is the document's own [`SketchGeometry`], not a form chosen
/// for drawing: a circle stays a circle, an arc keeps the centre, radius and
/// angles a solver never touched, and nothing is turned into anything else on
/// the way out. A presentation that had already decided a circle was a
/// polyline would have thrown away the only thing that says how finely to draw
/// it.
#[derive(Debug, Clone, PartialEq)]
pub struct PresentedCurve {
    id: StableEntityId,
    construction: bool,
    geometry: SketchGeometry,
}

impl PresentedCurve {
    /// The identifier the document stores this curve under.
    ///
    /// The durable half of the naming scheme, and the only name for a curve
    /// there is here. Where it sits in the list is not one: inserting a curve
    /// ahead of it would silently rename it.
    pub fn id(&self) -> StableEntityId {
        self.id
    }

    /// Whether this curve guides the drawing rather than bounding a face.
    pub fn is_construction(&self) -> bool {
        self.construction
    }

    /// Its shape, at the coordinates the rebuild built from.
    pub fn geometry(&self) -> &SketchGeometry {
        &self.geometry
    }
}
