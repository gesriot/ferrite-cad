// SPDX-License-Identifier: MIT
//! §27A: a full turn about the sketch Y axis, against the kernel that ships.
//!
//! Every assertion is about the solid Open CASCADE built, measured after the
//! fact: its volume against Pappus, and each face — found through the history
//! entry of the Line that raised it, never by position — against the surface
//! that Line must sweep.

#![allow(clippy::panic)]

#[path = "../../ferritecad-fixtures/tests/support/naming.rs"]
mod naming;

use std::collections::BTreeSet;
use std::f64::consts::PI;

use ferritecad_kernel::{
    CancelToken, FaceSurface, GeometryKernel, NamingInput, OperationContext, PartialTurn,
    PlanarPoint, Profile, ProfileLoop, ProfileSegment, RevolveAxis, RevolveRequest, RevolveTurn,
    SegmentGeometry, SketchPlane, SubShapeHandle, TessellationParams,
};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_types::{ErrorKind, Result, StableEntityId};

fn native() -> bool {
    if is_available() {
        return true;
    }

    ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
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
        let generated: Vec<_> = naming::generated(
            &mut kernel,
            result.shape,
            NamingInput::Curve(*label),
            ferritecad_kernel::SubShapeKind::Face,
        )
        .into_iter()
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
            naming::generated(
                &mut kernel,
                result.shape,
                NamingInput::Curve(*l),
                ferritecad_kernel::SubShapeKind::Face,
            )
            .into_iter()
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

/// §27C: the checked request for a profile closed on the axis along the Line
/// that starts at `axis` in `points`.
fn solid_request(points: &[[f64; 2]], axis: usize) -> (RevolveRequest, Vec<StableEntityId>) {
    let (request, labels) = request(points).expect("request");
    let request = request
        .with_axis_segment(labels[axis])
        .expect("a segment of this profile");
    (request, labels)
}

/// A solid part: the stated axis Line raises nothing; every other Line
/// raises its own face — plane, cylinder or cone on the Y axis — and the
/// solid has exactly those faces and the analytic volume.
fn check_solid(points: &[[f64; 2]], axis: usize, volume: f64) {
    let mut kernel = OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let (request, labels) = solid_request(points, axis);
    let result = kernel.revolve(&request, &context).expect("revolve");
    let (faces, measured) = kernel.shape_stats(result.shape).expect("stats");
    assert!(
        (measured - volume).abs() < 1e-6 * volume,
        "{measured} != {volume} for {points:?}"
    );
    assert_eq!(
        faces as usize,
        points.len() - 1,
        "one face per Line off the axis, none for the axis Line, no cap"
    );
    let n = points.len();
    let mut seen = BTreeSet::new();
    for (i, label) in labels.iter().enumerate() {
        let generated: Vec<_> = naming::generated(
            &mut kernel,
            result.shape,
            NamingInput::Curve(*label),
            ferritecad_kernel::SubShapeKind::Face,
        )
        .into_iter()
        .collect();
        if i == axis {
            assert!(generated.is_empty(), "the axis Line raised {generated:?}");
            continue;
        }
        let [face] = generated.as_slice() else {
            panic!("Line {i} raised {generated:?}")
        };
        assert!(seen.insert(*face), "one face for two Lines");
        let (a, b) = (points[i], points[(i + 1) % n]);
        let surface = kernel.face_surface(*face).expect("surface");
        match expected(a, b) {
            Expected::Annulus { .. } => assert_eq!(surface, FaceSurface::Plane, "Line {i}"),
            Expected::Cylinder { radius } => {
                assert_eq!(surface, FaceSurface::Cylinder { radius }, "Line {i}");
            }
            Expected::Cone => assert_eq!(surface, FaceSurface::Cone, "Line {i}"),
        }
        if !matches!(surface, FaceSurface::Plane) {
            let (origin, direction) = kernel.surface_axis(*face).expect("axis");
            assert!(
                origin[0].abs() < 1e-9 && origin[2].abs() < 1e-9,
                "{origin:?}"
            );
            assert!((direction[1].abs() - 1.).abs() < 1e-12, "{direction:?}");
        }
    }
    assert_eq!(seen.len(), faces as usize, "every face is some Line's");
    kernel.release(result.shape);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn native_axis_closed_revolutions_are_solid_and_name_every_line_but_the_axis() {
    if !native() {
        return;
    }
    let cylinder = |r: f64, h: f64| PI * r * r * h;
    let cone = |r: f64, h: f64| PI * r * r * h / 3.;
    for (points, axis, volume) in [
        (
            vec![[0., 0.], [10., 0.], [10., 15.], [0., 15.]],
            3,
            cylinder(10., 15.),
        ),
        (vec![[0., 0.], [10., 0.], [0., 15.]], 2, cone(10., 15.)),
        (
            vec![
                [0., 0.],
                [10., 0.],
                [10., 5.],
                [6., 5.],
                [6., 15.],
                [0., 15.],
            ],
            5,
            cylinder(10., 5.) + cylinder(6., 10.),
        ),
        (
            vec![[0., -1.25], [3.5, -1.25], [3.5, 7.75], [0., 7.75]],
            3,
            cylinder(3.5, 9.),
        ),
    ] {
        let n = points.len();
        // Every place in the saved order, and both windings.
        for rotation in 0..n {
            let rotated: Vec<_> = (0..n).map(|i| points[(i + rotation) % n]).collect();
            let at = (axis + n - rotation) % n;
            check_solid(&rotated, at, volume);
            let reversed: Vec<_> = rotated.iter().rev().copied().collect();
            // Reversed, the Line from rotated[at] to rotated[at+1] runs from
            // reversed[n-2-at] to reversed[n-1-at].
            check_solid(&reversed, (2 * n - 2 - at) % n, volume);
        }
    }
}

#[test]
fn native_axis_closed_requests_are_checked_not_trusted() {
    if !native() {
        return;
    }
    let mut kernel = OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let cylinder = [[0., 0.], [10., 0.], [10., 15.], [0., 15.]];
    // The Line on the axis, not stated: refused as before.
    let (undeclared, _) = request(&cylinder).expect("request");
    let error = kernel
        .revolve(&undeclared, &context)
        .expect_err("undeclared");
    assert!(error.to_string().contains("lies on the axis"), "{error}");
    // A stated axis Line that is not on the axis.
    let (off, _) = solid_request(&cylinder, 1);
    // Vertices are checked in order, so the true axis vertex 0 may be
    // refused as unstated before the stated one is found off the axis.
    let error = kernel.revolve(&off, &context).expect_err("off the axis");
    assert_eq!(error.kind(), ErrorKind::Input);
    assert!(
        ["not on the axis", "lies on the axis"]
            .iter()
            .any(|m| error.to_string().contains(m)),
        "{error}"
    );
    // A part with a bore cannot state an axis Line.
    let (bore, _) = solid_request(&[[4., 0.], [10., 0.], [10., 15.], [4., 15.]], 3);
    let error = kernel.revolve(&bore, &context).expect_err("a bore");
    assert!(error.to_string().contains("not on the axis"), "{error}");
    let (solid, labels) = solid_request(&cylinder, 3);
    let result = kernel.revolve(&solid, &context).expect("revolve");
    let producer = ferritecad_types::ObjectId::new();
    let map =
        naming::bind(&mut kernel, producer, solid.profile(), &result).expect("complete provenance");
    let trace = kernel.naming_history(result.shape).expect("history");
    for (i, label) in labels.iter().enumerate() {
        assert_eq!(
            naming::generated(
                &mut kernel,
                result.shape,
                NamingInput::Curve(*label),
                ferritecad_kernel::SubShapeKind::Face
            )
            .len(),
            usize::from(i != 3)
        );
    }
    assert!(trace.relations.contains(&(
        NamingInput::Curve(labels[3]),
        ferritecad_kernel::NamingRelation::Deleted,
        None
    )));
    assert_eq!(map.features[&producer].named_face_count(), 3);
    kernel.release(result.shape);
    // A label from another profile is refused before any kernel work.
    let (plain, _) = request(&cylinder).expect("request");
    assert_eq!(
        plain
            .with_axis_segment(StableEntityId::new())
            .expect_err("foreign")
            .kind(),
        ErrorKind::Input
    );
    assert_eq!(kernel.live_shape_count(), 0);
}

/// §27D: a checked partial request: the profile, the Line on the axis if the
/// profile closes on it, and the angle in degrees.
fn partial_request(points: &[[f64; 2]], degrees: f64) -> (RevolveRequest, Vec<StableEntityId>) {
    let (full, labels) = request(points).expect("request");
    let turn = RevolveTurn::Partial(PartialTurn::new(degrees.to_radians()).expect("a sector"));
    let partial = RevolveRequest::new(full.profile().clone(), RevolveAxis::PlaneY, turn);
    let n = points.len();
    match (0..n).find(|&i| points[i][0] == 0. && points[(i + 1) % n][0] == 0.) {
        Some(axis) => (
            partial.with_axis_segment(labels[axis]).expect("axis Line"),
            labels,
        ),
        None => (partial, labels),
    }
}

/// The outward normal and area of one face, from the kernel's own mesh: the
/// winding the mesh gives it, summed over its triangles.
fn face_normal_and_area(
    kernel: &mut OcctKernel,
    shape: ferritecad_kernel::ShapeHandle,
    face: SubShapeHandle,
) -> ([f64; 3], f64, Vec<[f64; 3]>) {
    let mesh = kernel
        .tessellate(
            shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("mesh");
    let range = mesh.faces.iter().find(|r| r.face == face).expect("drawn");
    let at = |v: u32| -> [f64; 3] {
        std::array::from_fn(|j| f64::from(mesh.positions[v as usize * 3 + j]))
    };
    let (mut sum, mut area, mut points) = ([0.; 3], 0., Vec::new());
    for t in mesh.indices
        [range.first_index as usize..(range.first_index + range.index_count) as usize]
        .chunks_exact(3)
    {
        let [a, b, c] = [at(t[0]), at(t[1]), at(t[2])];
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        area += (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() / 2.;
        sum = [sum[0] + n[0], sum[1] + n[1], sum[2] + n[2]];
        points.extend([a, b, c]);
    }
    let length = (sum[0] * sum[0] + sum[1] * sum[1] + sum[2] * sum[2]).sqrt();
    (sum.map(|x| x / length), area, points)
}

#[test]
fn native_partial_revolutions_name_two_caps_from_the_sweep_history() {
    if !native() {
        return;
    }
    let mut kernel = OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    for (points, full, area) in [
        (
            vec![[4., 0.], [10., 0.], [10., 15.], [4., 15.]],
            PI * (100. - 16.) * 15.,
            90.,
        ),
        (
            vec![[0., 0.], [10., 0.], [10., 15.], [0., 15.]],
            PI * 100. * 15.,
            150.,
        ),
        (
            vec![[0., 0.], [10., 0.], [0., 15.]],
            PI * 100. * 15. / 3.,
            75.,
        ),
    ] {
        for drawn in [points.clone(), points.iter().rev().copied().collect()] {
            for degrees in [90., 180., 270., 137.5] {
                let (request, labels) = partial_request(&drawn, degrees);
                let axis = request.axis_segment();
                let result = kernel.revolve(&request, &context).expect("a sector");
                let (faces, volume) = kernel.shape_stats(result.shape).expect("stats");
                let expected = full * degrees / 360.;
                assert!((volume - expected).abs() < 1e-9 * expected, "{volume}");
                let raised = labels.len() - usize::from(axis.is_some());
                assert_eq!(faces as usize, raised + 2);
                let start = &naming::cap(&mut kernel, result.shape, false);
                let end = &naming::cap(&mut kernel, result.shape, true);
                let mut line_faces = BTreeSet::new();
                for label in &labels {
                    let faces: Vec<_> = naming::generated(
                        &mut kernel,
                        result.shape,
                        NamingInput::Curve(*label),
                        ferritecad_kernel::SubShapeKind::Face,
                    )
                    .into_iter()
                    .collect();
                    assert_eq!(faces.len(), usize::from(Some(*label) != axis));
                    line_faces.extend(faces);
                }
                assert!(!line_faces.contains(start) && !line_faces.contains(end));
                assert_ne!(start, end);
                let (s, c) = f64::to_radians(degrees).sin_cos();
                for (face, outward) in [(*start, [0., 0., 1.]), (*end, [-s, 0., -c])] {
                    assert_eq!(
                        kernel.face_surface(face).expect("surface"),
                        FaceSurface::Plane
                    );
                    let (normal, covered, on) =
                        face_normal_and_area(&mut kernel, result.shape, face);
                    let dot =
                        normal[0] * outward[0] + normal[1] * outward[1] + normal[2] * outward[2];
                    assert!(dot > 1. - 1e-9, "{normal:?} is not {outward:?}");
                    assert!((covered - area).abs() < 1e-5 * area, "{covered} of {area}");
                    for p in on {
                        let off = p[0] * outward[0] + p[1] * outward[1] + p[2] * outward[2];
                        assert!(off.abs() < 1e-5, "{p:?} off its plane");
                    }
                }
                let producer = ferritecad_types::ObjectId::new();
                let map = naming::bind(&mut kernel, producer, request.profile(), &result)
                    .expect("sector provenance");
                let reference = ferritecad_document::TopologyRef {
                    id: StableEntityId::new(),
                    owner: producer,
                    producer_feature: producer,
                    expected_kind: ferritecad_document::EntityKind::Face,
                    output_role: ferritecad_document::SemanticRole::RevolveCap {
                        side: ferritecad_document::CapSide::Start,
                    },
                    selection: ferritecad_document::SelectionRule::Exact,
                    fallback_signature: None,
                };
                assert_eq!(
                    ferritecad_topology::resolve(&map, &reference).expect("start"),
                    vec![*start]
                );
                // Both caps survive the named archive as the faces they were.
                let slots = [*start, *end];
                let (blob, archived) = kernel
                    .encode_shape_with(result.shape, &slots)
                    .expect("archive");
                let (restored, back) = kernel.decode_shape_with(&blob, &archived).expect("restore");
                for (face, outward) in [(back[0], [0., 0., 1.]), (back[1], [-s, 0., -c])] {
                    let (normal, covered, _) = face_normal_and_area(&mut kernel, restored, face);
                    let dot =
                        normal[0] * outward[0] + normal[1] * outward[1] + normal[2] * outward[2];
                    assert!(dot > 1. - 1e-9, "restored {normal:?} is not {outward:?}");
                    assert!((covered - area).abs() < 1e-5 * area);
                }
                kernel.release(result.shape);
                kernel.release(restored);
            }
        }
    }
    assert_eq!(kernel.live_shape_count(), 0);
}
