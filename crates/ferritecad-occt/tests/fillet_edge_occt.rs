// SPDX-License-Identifier: MIT
//! §28A: the selected-edge fillet the Open CASCADE adapter added, against real
//! geometry and through the kernel trait the evaluator calls.
//!
//! Measured, not assumed: the removed volume against (1 − π/4)·r²·H, the one
//! generated face's surface, radius and axis, and what the fillet says became
//! of every face it was asked to track.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

#[path = "../../ferritecad-fixtures/tests/support/naming.rs"]
mod naming;

use std::f64::consts::PI;

use ferritecad_kernel::{
    CancelToken, ExtrudeExtent, ExtrudeRequest, FaceSurface, FilletRequest, GeometryKernel,
    NamingInput, NamingRelation, OperationContext, PlanarPoint, Profile, ProfileLoop,
    ProfileSegment, SegmentGeometry, SketchPlane, SubShapeHandle, SubShapeKind,
};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_types::{Result, StableEntityId};

macro_rules! kernel_or_skip {
    () => {{
        if !is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
            return;
        }
        OcctKernel::new().expect("a build with Open CASCADE opens a session")
    }};
}

const X0: f64 = -4.5;
const Y0: f64 = 3.25;
const W: f64 = 37.5;
const D: f64 = 12.25;
const H: f64 = 6.75;

/// The asymmetric translated plate of the §28A probe, as the segments
/// `corners` list, so a winding or a starting segment can be chosen.
fn plate(corners: &[[f64; 2]; 4]) -> Result<(ExtrudeRequest, Vec<StableEntityId>)> {
    let mut segments = Vec::new();
    let mut labels = Vec::new();
    for index in 0..4 {
        let label = StableEntityId::new();
        labels.push(label);
        let [sx, sy] = corners[index];
        let [ex, ey] = corners[(index + 1) % 4];
        segments.push(ProfileSegment::new(
            label,
            SegmentGeometry::line(PlanarPoint::new(sx, sy)?, PlanarPoint::new(ex, ey)?)?,
        ));
    }
    let profile = Profile::new(
        SketchPlane::world_xy(),
        ProfileLoop::new(segments)?,
        Vec::new(),
    )?;
    Ok((
        ExtrudeRequest::new(profile, ExtrudeExtent::blind(H)?, false),
        labels,
    ))
}

fn corner_of(
    corners: &[[f64; 2]; 4],
    labels: &[StableEntityId],
    joint: [StableEntityId; 2],
) -> [f64; 2] {
    let [a, b] = joint;
    let ia = labels.iter().position(|l| *l == a).expect("a");
    let ib = labels.iter().position(|l| *l == b).expect("b");
    let ends = |i: usize| [corners[i], corners[(i + 1) % 4]];
    ends(ia)
        .into_iter()
        .find(|p| ends(ib).contains(p))
        .expect("adjacent Lines share a corner")
}

#[test]
fn one_selected_vertical_edge_is_rounded_and_every_tracked_face_is_accounted_for() {
    let mut kernel = kernel_or_skip!();
    let context = OperationContext::default();
    let ccw = [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]];
    let cw_from_third = [[X0 + W, Y0 + D], [X0 + W, Y0], [X0, Y0], [X0, Y0 + D]];
    for corners in [ccw, cw_from_third] {
        let (request, labels) = plate(&corners).expect("a plate");
        let built = kernel.extrude(&request, &context).expect("the plate");
        let faces = naming::faces(&mut kernel, &built, &labels);
        assert_eq!(faces.len(), 6);
        assert_eq!(
            naming::sweep_edges(&mut kernel, built.shape).len(),
            4,
            "four vertical edges"
        );
        let before = kernel.encode_shape(built.shape).expect("archive target");

        for (joint, edge) in naming::sweep_edges(&mut kernel, built.shape).clone() {
            let r = 3.0625;
            let fillet = FilletRequest::new(built.shape, &[edge], r).expect("request");
            let mut track = faces.clone();
            track.push(edge);
            let result = kernel
                .fillet_edges(&fillet, &context)
                .expect("a real fillet");
            let trace = kernel
                .naming_history(result.shape)
                .expect("complete history");
            assert_eq!(
                kernel.encode_shape(built.shape).expect("target after"),
                before,
                "a fillet must not mutate the feature it rounds"
            );

            let removed = (1.0 - PI / 4.0) * r * r * H;
            assert!((result.removed_volume - removed).abs() < 1e-9 * W * D * H);
            let (count, volume) = kernel.shape_stats(result.shape).expect("stats");
            assert_eq!(count, 7, "two caps, four sides and one fillet face");
            assert!(
                (volume - (W * D * H - removed)).abs() < 1e-9 * W * D * H,
                "{volume}"
            );

            // One face, a cylinder of the radius asked for, about a vertical
            // axis r inward of exactly the corner the joint names.
            assert_eq!(
                naming::outputs(
                    &trace,
                    NamingInput::SubShape(edge),
                    NamingRelation::Generated,
                    SubShapeKind::Face
                )
                .len(),
                1
            );
            let face = naming::outputs(
                &trace,
                NamingInput::SubShape(edge),
                NamingRelation::Generated,
                SubShapeKind::Face,
            )[0];
            assert_eq!(face.shape(), result.shape);
            assert_eq!(
                kernel.face_surface(face).expect("surface"),
                FaceSurface::Cylinder { radius: r }
            );
            let (origin, axis) = kernel.cylinder_axis(face).expect("axis");
            assert!(axis[0].abs() < 1e-12 && axis[1].abs() < 1e-12);
            let [cx, cy] = corner_of(&corners, &labels, joint);
            let inward = [
                cx + if cx == X0 { r } else { -r },
                cy + if cy == Y0 { r } else { -r },
            ];
            assert!(
                (origin[0] - inward[0]).abs() < 1e-9 && (origin[1] - inward[1]).abs() < 1e-9,
                "{origin:?} is not r inward of {:?}",
                [cx, cy]
            );

            // The rounded edge is gone. The two sides meeting there and both
            // caps are trimmed; the two sides away from it are what they were.
            assert_eq!(naming::survival(&trace, edge), NamingRelation::Deleted);
            let [a, b] = joint;
            for (label, side) in labels.iter().zip(&faces) {
                let touched = *label == a || *label == b;
                let outputs: Vec<_> = naming::surviving(&trace, *side).collect();
                match naming::survival(&trace, *side) {
                    NamingRelation::Kept => assert!(!touched, "an adjacent side was kept"),
                    NamingRelation::Modified => {
                        assert!(touched, "a far side was modified");
                        assert_eq!(outputs.len(), 1);
                        assert_eq!(
                            kernel.face_surface(outputs[0]).expect("surface"),
                            FaceSurface::Plane
                        );
                    }
                    other => panic!("a side was {other:?}"),
                }
            }
            for cap in faces[4..].iter() {
                assert_eq!(naming::survival(&trace, *cap), NamingRelation::Modified);
                assert_eq!(
                    naming::surviving(&trace, *cap).count(),
                    1,
                    "a cap is trimmed into one face"
                );
            }
            // No output is both the fillet face and a carried face.
            for input in &track {
                assert!(naming::surviving(&trace, *input).all(|output| output != face));
            }
            kernel.release(result.shape);
        }
        kernel.release(built.shape);
    }
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_fillet_that_cannot_be_what_it_claims_is_refused_and_keeps_nothing() {
    let mut kernel = kernel_or_skip!();
    let context = OperationContext::default();
    let corners = [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]];
    let (request, labels) = plate(&corners).expect("a plate");
    let built = kernel.extrude(&request, &context).expect("the plate");
    let other = kernel.extrude(&request, &context).expect("another plate");
    let faces = naming::faces(&mut kernel, &built, &labels);
    let edge = *naming::sweep_edges(&mut kernel, built.shape)
        .values()
        .next()
        .expect("an edge");
    let foreign_edge = *naming::sweep_edges(&mut kernel, other.shape)
        .values()
        .next()
        .expect("an edge");
    let live = kernel.live_shape_count();

    // The request itself: a face is not an edge, an edge of another shape is
    // not this shape's, and a radius must be finite and positive.
    assert!(FilletRequest::new(built.shape, &[faces[0]], 1.0).is_err());
    assert!(FilletRequest::new(built.shape, &[foreign_edge], 1.0).is_err());
    for radius in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(FilletRequest::new(built.shape, &[edge], radius).is_err());
    }
    // An edge index the shape does not have.
    let missing = SubShapeHandle::new(built.shape, SubShapeKind::Edge, u64::MAX);
    let r = FilletRequest::new(built.shape, &[missing], 1.0).expect("well formed");
    assert!(kernel.fillet_edges(&r, &context).is_err());
    // The kernel's own limit, never clamped to something it can build.
    let wide = FilletRequest::new(built.shape, &[edge], D).expect("request");
    assert!(kernel.fillet_edges(&wide, &context).is_err());
    let good = FilletRequest::new(built.shape, &[edge], 1.0).expect("request");
    // A cancelled run.
    let cancel = CancelToken::new();
    cancel.cancel();
    let cancelled = OperationContext::default().with_cancel(cancel);
    assert!(kernel.fillet_edges(&good, &cancelled).is_err());
    assert_eq!(kernel.live_shape_count(), live, "a refusal kept a shape");

    // A released target is not a target.
    kernel.release(other.shape);
    let gone = FilletRequest::new(other.shape, &[foreign_edge], 1.0).expect("request");
    assert!(kernel.fillet_edges(&gone, &context).is_err());
    kernel.release(built.shape);
    assert_eq!(kernel.live_shape_count(), 0);
}
