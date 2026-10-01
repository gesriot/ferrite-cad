// SPDX-License-Identifier: MIT
//! §29A: the selected-edge chamfer the Open CASCADE adapter added, against real
//! geometry and through the kernel trait the evaluator calls.
//!
//! Measured, not assumed: the removed volume against `d²/2·H`, the one
//! generated face's plane (outward normal, the two points `d` along each
//! adjacent edge, area `d·√2·H`) at exactly the corner the joint names, and
//! what the chamfer says became of every face it was asked to track.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_kernel::{
    CancelToken, CarriedOutcome, ChamferRequest, ExtrudeExtent, ExtrudeRequest, ExtrudeResult,
    FaceSurface, GeometryKernel, HistoryInput, OperationContext, PlanarPoint, Profile, ProfileLoop,
    ProfileSegment, SegmentGeometry, SketchPlane, SubShapeHandle, SubShapeKind,
};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_types::{ProfileJoint, Result, StableEntityId};

macro_rules! kernel_or_skip {
    () => {{
        if !is_available() {
            assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
            eprintln!("skipped: this build has no Open CASCADE");
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

fn named_faces(result: &ExtrudeResult, labels: &[StableEntityId]) -> Vec<SubShapeHandle> {
    let mut faces: Vec<SubShapeHandle> = labels
        .iter()
        .flat_map(|label| result.history.generated(HistoryInput::Segment(*label)))
        .collect();
    faces.extend(result.start_cap.iter().copied());
    faces.extend(result.end_cap.iter().copied());
    faces
}

/// The corner two Lines meet at: the one point both of them have.
fn corner_of(corners: &[[f64; 2]; 4], labels: &[StableEntityId], joint: ProfileJoint) -> [f64; 2] {
    let [a, b] = joint.segments();
    let ia = labels.iter().position(|l| *l == a).expect("a");
    let ib = labels.iter().position(|l| *l == b).expect("b");
    let ends = |i: usize| [corners[i], corners[(i + 1) % 4]];
    ends(ia)
        .into_iter()
        .find(|p| ends(ib).contains(p))
        .expect("adjacent Lines share a corner")
}

#[test]
fn one_selected_vertical_edge_is_chamfered_to_the_corner_it_names_and_every_tracked_face_is_accounted_for()
 {
    let mut kernel = kernel_or_skip!();
    let context = OperationContext::default();
    let ccw = [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]];
    let cw_from_third = [[X0 + W, Y0 + D], [X0 + W, Y0], [X0, Y0], [X0, Y0 + D]];
    for corners in [ccw, cw_from_third] {
        let (request, labels) = plate(&corners).expect("a plate");
        let built = kernel.extrude(&request, &context).expect("the plate");
        let faces = named_faces(&built, &labels);
        assert_eq!(faces.len(), 6);
        assert_eq!(built.sweep_edges.len(), 4, "four vertical edges");
        let before = kernel.encode_shape(built.shape).expect("archive target");

        for (joint, edge) in built.sweep_edges.clone() {
            for d in [0.75, 3.0625] {
                let chamfer = ChamferRequest::new(built.shape, edge, d).expect("request");
                let mut track = faces.clone();
                track.push(edge);
                let result = kernel
                    .chamfer_edge(&chamfer, &track, &context)
                    .expect("a real chamfer");
                result.validate(built.shape, edge).expect("valid result");
                assert_eq!(
                    kernel.encode_shape(built.shape).expect("target after"),
                    before,
                    "a chamfer must not mutate the feature it cuts"
                );

                let removed = d * d / 2.0 * H;
                assert!((result.removed_volume - removed).abs() < 1e-9 * W * D * H);
                let (count, volume) = kernel.shape_stats(result.shape).expect("stats");
                assert_eq!(count, 7, "two caps, four sides and one chamfer face");
                assert!(
                    (volume - (W * D - d * d / 2.0) * H).abs() < 1e-9 * W * D * H,
                    "{volume}"
                );

                // One face, planar, at exactly the corner the joint names: its
                // outward normal is the diagonal pointing away from the plate,
                // it passes through the points d along each adjacent edge from
                // the corner, and its area is d·√2·H.
                assert_eq!(result.chamfer_faces.len(), 1);
                let face = result.chamfer_faces[0];
                assert_eq!(face.shape(), result.shape);
                assert_eq!(
                    kernel.face_surface(face).expect("surface"),
                    FaceSurface::Plane
                );
                let (origin, normal, area) = kernel.face_plane(face).expect("plane");
                let [cx, cy] = corner_of(&corners, &labels, joint);
                let (sx, sy) = (
                    if cx == X0 { -1.0 } else { 1.0 },
                    if cy == Y0 { -1.0 } else { 1.0 },
                );
                let root = 0.5f64.sqrt();
                assert!(
                    (normal[0] - sx * root).abs() < 1e-12
                        && (normal[1] - sy * root).abs() < 1e-12
                        && normal[2].abs() < 1e-12,
                    "the normal {normal:?} does not point out of the corner {:?}",
                    [cx, cy]
                );
                for point in [[cx - sx * d, cy], [cx, cy - sy * d]] {
                    let off =
                        (point[0] - origin[0]) * normal[0] + (point[1] - origin[1]) * normal[1];
                    assert!(off.abs() < 1e-9, "{point:?} is {off} mm off the plane");
                }
                assert!(
                    (area - d * 2.0f64.sqrt() * H).abs() < 1e-9 * W * D,
                    "{area}"
                );

                // The cut edge is gone. The two sides meeting there and both
                // caps are trimmed; the two sides away from it are what they were.
                assert_eq!(result.carried[&edge], CarriedOutcome::Deleted);
                let [a, b] = joint.segments();
                for (label, side) in labels.iter().zip(&faces) {
                    let touched = *label == a || *label == b;
                    let outputs: Vec<_> = result
                        .history
                        .modified(HistoryInput::SubShape(*side))
                        .collect();
                    match result.carried[side] {
                        CarriedOutcome::Kept => assert!(!touched, "an adjacent side was kept"),
                        CarriedOutcome::Modified => {
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
                    assert_eq!(result.carried[cap], CarriedOutcome::Modified);
                    assert_eq!(
                        result
                            .history
                            .modified(HistoryInput::SubShape(*cap))
                            .count(),
                        1,
                        "a cap is trimmed into one face"
                    );
                }
                for input in &track {
                    assert!(
                        result
                            .history
                            .modified(HistoryInput::SubShape(*input))
                            .all(|output| output != face)
                    );
                }
                kernel.release(result.shape);
            }
        }
        kernel.release(built.shape);
    }
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_chamfer_that_cannot_be_what_it_claims_is_refused_and_keeps_nothing() {
    let mut kernel = kernel_or_skip!();
    let context = OperationContext::default();
    let corners = [[X0, Y0], [X0 + W, Y0], [X0 + W, Y0 + D], [X0, Y0 + D]];
    let (request, labels) = plate(&corners).expect("a plate");
    let built = kernel.extrude(&request, &context).expect("the plate");
    let other = kernel.extrude(&request, &context).expect("another plate");
    let faces = named_faces(&built, &labels);
    let edge = *built.sweep_edges.values().next().expect("an edge");
    let foreign_edge = *other.sweep_edges.values().next().expect("an edge");
    let live = kernel.live_shape_count();

    assert!(ChamferRequest::new(built.shape, faces[0], 1.0).is_err());
    assert!(ChamferRequest::new(built.shape, foreign_edge, 1.0).is_err());
    for distance in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(ChamferRequest::new(built.shape, edge, distance).is_err());
    }
    let missing = SubShapeHandle::new(built.shape, SubShapeKind::Edge, u64::MAX);
    let r = ChamferRequest::new(built.shape, missing, 1.0).expect("well formed");
    assert!(kernel.chamfer_edge(&r, &[], &context).is_err());
    let good = ChamferRequest::new(built.shape, edge, 1.0).expect("request");
    let foreign_face = named_faces(&other, &[])[0];
    assert!(
        kernel
            .chamfer_edge(&good, &[foreign_face], &context)
            .is_err()
    );
    // The kernel's own limit — the whole shorter side — never clamped to
    // something it can build.
    let wide = ChamferRequest::new(built.shape, edge, D).expect("request");
    assert!(kernel.chamfer_edge(&wide, &faces, &context).is_err());
    // A planar answer is asked of a plane only.
    let cancel = CancelToken::new();
    cancel.cancel();
    let cancelled = OperationContext::default().with_cancel(cancel);
    assert!(kernel.chamfer_edge(&good, &faces, &cancelled).is_err());
    assert!(kernel.face_plane(faces[0]).is_ok());
    assert_eq!(kernel.live_shape_count(), live, "a refusal kept a shape");

    kernel.release(other.shape);
    let gone = ChamferRequest::new(other.shape, foreign_edge, 1.0).expect("request");
    assert!(kernel.chamfer_edge(&gone, &[], &context).is_err());
    kernel.release(built.shape);
    assert_eq!(kernel.live_shape_count(), 0);
}

/// A planar face's centre of mass need not lie on its trimmed material.
/// In particular, both caps of a ring have their centroid in the bore, so
/// probing from that point cannot determine the solid's outward normal.
#[test]
fn planar_normals_follow_the_solid_when_the_face_centroid_is_in_a_hole() {
    let mut kernel = kernel_or_skip!();
    let context = OperationContext::default();
    let circle = |radius| {
        ProfileLoop::closed_curve(ProfileSegment::new(
            StableEntityId::new(),
            SegmentGeometry::circle(PlanarPoint::new(12., -7.).expect("centre"), radius)
                .expect("circle"),
        ))
        .expect("closed circle")
    };
    for reversed in [false, true] {
        let profile = Profile::new(SketchPlane::world_xy(), circle(10.), vec![circle(4.)])
            .expect("annular profile");
        let request =
            ExtrudeRequest::new(profile, ExtrudeExtent::blind(H).expect("height"), reversed);
        let built = kernel.extrude(&request, &context).expect("ring");
        let caps: Vec<_> = built
            .start_cap
            .iter()
            .chain(&built.end_cap)
            .copied()
            .collect();
        assert_eq!(caps.len(), 2);
        let (blob, slots) = kernel
            .encode_shape_with(built.shape, &caps)
            .expect("archive");
        let (restored, back) = kernel.decode_shape_with(&blob, &slots).expect("restore");
        for faces in [&caps, &back] {
            for (index, face) in faces.iter().enumerate() {
                let (point, normal, area) = kernel.face_plane(*face).expect("cap plane");
                let direction = if reversed { -1. } else { 1. };
                let expected_z = if index == 0 { -direction } else { direction };
                assert_eq!(
                    normal,
                    [0., 0., expected_z],
                    "cap {index}, reversed={reversed}"
                );
                let expected_height = if index == 0 { 0. } else { direction * H };
                assert!((point[2] - expected_height).abs() < 1e-10);
                assert!((area - std::f64::consts::PI * 84.).abs() < 1e-9);
            }
        }
        kernel.release(restored);
        kernel.release(built.shape);
    }
    assert_eq!(kernel.live_shape_count(), 0);
}
