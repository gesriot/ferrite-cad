// SPDX-License-Identifier: MIT
//! §27A: a full turn about the sketch Y axis, against the kernel that ships.
//!
//! Every assertion is about the solid Open CASCADE built, measured after the
//! fact: its volume against Pappus, and each face — found through the history
//! entry of the Line that raised it, never by position — against the surface
//! that Line must sweep.

#![allow(clippy::panic)]

use std::collections::BTreeSet;
use std::f64::consts::PI;

use ferritecad_kernel::{
    CancelToken, FaceSurface, GeometryKernel, HistoryInput, OperationContext, PlanarPoint, Profile,
    ProfileLoop, ProfileSegment, RevolveAxis, RevolveRequest, RevolveTurn, SegmentGeometry,
    SketchPlane, SubShapeHandle, TessellationParams,
};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_types::{ErrorKind, Result, StableEntityId};

fn native() -> bool {
    if is_available() {
        return true;
    }
    assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
    eprintln!("skipped: this build has no Open CASCADE");
    false
}

fn request(points: &[[f64; 2]]) -> Result<(RevolveRequest, Vec<StableEntityId>)> {
    let corners: Vec<PlanarPoint> = points
        .iter()
        .map(|[x, y]| PlanarPoint::new(*x, *y))
        .collect::<Result<_>>()?;
    let mut segments = Vec::new();
    let mut labels = Vec::new();
    for (index, start) in corners.iter().enumerate() {
        let label = StableEntityId::new();
        labels.push(label);
        segments.push(ProfileSegment::new(
            label,
            SegmentGeometry::line(*start, corners[(index + 1) % corners.len()])?,
        ));
    }
    let profile = Profile::new(
        SketchPlane::world_xy(),
        ProfileLoop::new(segments)?,
        Vec::new(),
    )?;
    Ok((
        RevolveRequest::new(profile, RevolveAxis::PlaneY, RevolveTurn::Full),
        labels,
    ))
}

/// What one Line must sweep into, about the world Y axis.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Expected {
    /// A radial Line: an annulus in the plane y = `at`.
    Annulus { at: f64 },
    /// An axial Line: a cylinder of this radius.
    Cylinder { radius: f64 },
    /// Any other Line: a cone.
    Cone,
}

fn expected(a: [f64; 2], b: [f64; 2]) -> Expected {
    if a[1] == b[1] {
        Expected::Annulus { at: a[1] }
    } else if a[0] == b[0] {
        Expected::Cylinder { radius: a[0] }
    } else {
        Expected::Cone
    }
}

/// Builds, then checks the volume, the face count, and every Line's own face.
fn check(points: &[[f64; 2]], volume: f64) {
    let mut kernel = OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let (request, labels) = request(points).expect("request");
    let result = kernel.revolve(&request, &context).expect("revolve");
    let (faces, measured) = kernel.shape_stats(result.shape).expect("stats");
    assert!(
        (measured - volume).abs() < 1e-6 * volume,
        "{measured} != {volume} for {points:?}"
    );
    assert_eq!(faces as usize, points.len(), "one face per Line, no caps");
    let mesh = kernel
        .tessellate(result.shape, &TessellationParams::default(), &context)
        .expect("mesh");
    let mut seen: BTreeSet<SubShapeHandle> = BTreeSet::new();
    let n = points.len();
    for (i, label) in labels.iter().enumerate() {
        let generated: Vec<_> = result
            .history
            .generated(HistoryInput::Segment(*label))
            .collect();
        let [face] = generated.as_slice() else {
            panic!("Line {i} raised {generated:?}")
        };
        assert!(seen.insert(*face), "one face for two Lines");
        let (a, b) = (points[i], points[(i + 1) % n]);
        let range = mesh
            .faces
            .iter()
            .find(|r| r.face == *face)
            .expect("the named face is drawn");
        let vertices: Vec<[f64; 3]> = mesh.indices
            [range.first_index as usize..(range.first_index + range.index_count) as usize]
            .iter()
            .map(|v| std::array::from_fn(|j| f64::from(mesh.positions[*v as usize * 3 + j])))
            .collect();
        let radius = |p: &[f64; 3]| p[0].hypot(p[2]);
        let (lo, hi) = (a[1].min(b[1]), a[1].max(b[1]));
        let (inner, outer) = (a[0].min(b[0]), a[0].max(b[0]));
        // Every point of the face lies within the Line's own band, turned.
        for p in &vertices {
            assert!(
                p[1] > lo - 1e-4 && p[1] < hi + 1e-4,
                "{p:?} outside {lo}..{hi}"
            );
            assert!(
                radius(p) > inner - 0.02 && radius(p) < outer + 1e-4,
                "{p:?} outside radius {inner}..{outer}"
            );
        }
        // And it goes all the way round: points in all four quadrants.
        for (sx, sz) in [(1., 1.), (-1., 1.), (-1., -1.), (1., -1.)] {
            assert!(
                vertices
                    .iter()
                    .any(|p| p[0] * sx > 1e-3 && p[2] * sz > 1e-3),
                "Line {i} does not turn a full circle"
            );
        }
        let surface = kernel.face_surface(*face).expect("surface");
        match expected(a, b) {
            Expected::Annulus { at } => {
                assert_eq!(surface, FaceSurface::Plane, "Line {i}");
                assert!(vertices.iter().all(|p| (p[1] - at).abs() < 1e-6));
            }
            Expected::Cylinder { radius: r } => {
                assert_eq!(surface, FaceSurface::Cylinder { radius: r }, "Line {i}");
                let (origin, direction) = kernel.surface_axis(*face).expect("axis");
                assert!(
                    origin[0].abs() < 1e-9 && origin[2].abs() < 1e-9,
                    "{origin:?}"
                );
                assert!((direction[1].abs() - 1.).abs() < 1e-12, "{direction:?}");
                assert!(vertices.iter().all(|p| (radius(p) - r).abs() < 1e-4));
            }
            Expected::Cone => {
                assert_eq!(surface, FaceSurface::Cone, "Line {i}");
                let (origin, direction) = kernel.surface_axis(*face).expect("axis");
                assert!(
                    origin[0].abs() < 1e-9 && origin[2].abs() < 1e-9,
                    "{origin:?}"
                );
                assert!((direction[1].abs() - 1.).abs() < 1e-12, "{direction:?}");
                // On the line, turned: radius is the Line's own at that height.
                for p in &vertices {
                    let t = (p[1] - a[1]) / (b[1] - a[1]);
                    let r = a[0] + t * (b[0] - a[0]);
                    assert!((radius(p) - r).abs() < 0.02, "{p:?} off the cone");
                }
            }
        }
    }
    kernel.release(result.shape);
    assert_eq!(kernel.live_shape_count(), 0);
}

const BUSHING: [[f64; 2]; 4] = [[4., 0.], [10., 0.], [10., 15.], [4., 15.]];
const STEPPED: [[f64; 2]; 6] = [
    [4., 0.],
    [10., 0.],
    [10., 5.],
    [7., 5.],
    [7., 15.],
    [4., 15.],
];
const SLOPED: [[f64; 2]; 4] = [[4., 0.], [10., 0.], [7., 15.], [4., 15.]];

fn variants(points: &[[f64; 2]]) -> Vec<Vec<[f64; 2]>> {
    let shifted: Vec<_> = points.iter().map(|[x, y]| [*x, y + 2.625]).collect();
    let mut reversed = points.to_vec();
    reversed.reverse();
    let mut both = shifted.clone();
    both.reverse();
    vec![points.to_vec(), reversed, shifted, both]
}

#[test]
fn native_full_turn_revolutions_are_what_pappus_and_their_lines_say() {
    if !native() {
        return;
    }
    for (points, volume) in [
        (&BUSHING[..], PI * (100. - 16.) * 15.),
        (&STEPPED[..], 750. * PI),
        (&SLOPED[..], 855. * PI),
    ] {
        for variant in variants(points) {
            check(&variant, volume);
        }
    }
}

#[test]
fn native_revolve_refuses_the_axis_other_kinds_and_cancellation() {
    if !native() {
        return;
    }
    let mut kernel = OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    // Touching and crossing the axis are refused by the bridge itself, even
    // though the caller's policy would have refused them first.
    for points in [
        [[0., 0.], [10., 0.], [10., 15.], [0., 15.]],
        [[-2., 0.], [10., 0.], [10., 15.], [-2., 15.]],
    ] {
        let (request, _) = request(&points).expect("request");
        let error = kernel.revolve(&request, &context).expect_err("axis");
        assert_eq!(error.kind(), ErrorKind::Input, "{error}");
        assert!(error.to_string().contains("axis"), "{error}");
    }
    // Only Lines.
    let arc = Profile::new(
        SketchPlane::world_xy(),
        ProfileLoop::closed_curve(ProfileSegment::new(
            StableEntityId::new(),
            SegmentGeometry::circle(PlanarPoint::new(10., 5.).expect("point"), 2.).expect("circle"),
        ))
        .expect("loop"),
        Vec::new(),
    )
    .expect("profile");
    let error = kernel
        .revolve(
            &RevolveRequest::new(arc, RevolveAxis::PlaneY, RevolveTurn::Full),
            &context,
        )
        .expect_err("a circle");
    assert_eq!(error.kind(), ErrorKind::Unsupported, "{error}");
    // Cancelled before anything is built: nothing is left behind.
    let token = CancelToken::new();
    token.cancel();
    let (request, _) = request(&BUSHING).expect("request");
    let error = kernel
        .revolve(&request, &OperationContext::default().with_cancel(token))
        .expect_err("cancelled");
    assert_eq!(error.kind(), ErrorKind::Cancellation);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn native_a_revolution_survives_the_named_archive_face_by_face() {
    if !native() {
        return;
    }
    let mut kernel = OcctKernel::new().expect("kernel");
    let (request, labels) = request(&STEPPED).expect("request");
    let result = kernel
        .revolve(&request, &OperationContext::default())
        .expect("revolve");
    let faces: Vec<SubShapeHandle> = labels
        .iter()
        .map(|l| {
            result
                .history
                .generated(HistoryInput::Segment(*l))
                .next()
                .expect("face")
        })
        .collect();
    let surfaces: Vec<_> = faces
        .iter()
        .map(|f| kernel.face_surface(*f).expect("surface"))
        .collect();
    let (blob, slots) = kernel
        .encode_shape_with(result.shape, &faces)
        .expect("archive");
    let (restored, back) = kernel.decode_shape_with(&blob, &slots).expect("restore");
    let (_, volume) = kernel.shape_stats(restored).expect("stats");
    assert!((volume - 750. * PI).abs() < 1e-6 * volume);
    let restored_surfaces: Vec<_> = back
        .iter()
        .map(|f| kernel.face_surface(*f).expect("surface"))
        .collect();
    assert_eq!(surfaces, restored_surfaces, "each slot is its Line's face");
    kernel.release(result.shape);
    kernel.release(restored);
    assert_eq!(kernel.live_shape_count(), 0);
}
