// SPDX-License-Identifier: MIT
//! How far Open CASCADE can be pushed on fillets and shells, measured.
//!
//! This is a corpus, not a feature test. Its job is to answer the question
//! stage 0 asks — can this kernel round and hollow real parts, and does it say
//! so when it cannot — before a fillet feature is designed around the answer.
//!
//! # What it found, and why the adapter refuses results
//!
//! `IsDone()` is not enough. On a 60 x 40 x 10 plate, radius 5 is correctly
//! reported as failure, and radius 5.1 and 6 are reported as *success* while
//! producing shapes that fail `BRepCheck_Analyzer` and enclose more volume
//! than the block they were cut from. Rounding a convex edge removes material,
//! so those are not poor answers, they are not answers. The adapter checks
//! every result and refuses the ones the kernel cannot vouch for, which is
//! what these tests hold it to.
//!
//! # Provenance
//!
//! Every part is generated from named parameters and every assertion prints
//! them, so a failure here is a recipe rather than a mystery.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_kernel::{
    ExtrudeExtent, ExtrudeRequest, GeometryKernel, OperationContext, PlanarPoint, Profile,
    ProfileLoop, ProfileSegment, SegmentGeometry, ShapeHandle, SketchPlane,
};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_types::{ErrorKind, Result, StableEntityId};

/// One part of the corpus, and the parameters that made it.
struct Part {
    name: &'static str,
    /// The plan view, as corners in millimetres.
    corners: Vec<(f64, f64)>,
    /// Corners rounded with an arc of this radius, or `None` for sharp ones.
    corner_radius: Option<f64>,
    height: f64,
}

impl Part {
    /// The smallest of the part's three dimensions, which is what bounds both
    /// a fillet radius and a wall thickness.
    fn smallest_dimension(&self) -> f64 {
        let xs: Vec<f64> = self.corners.iter().map(|(x, _)| *x).collect();
        let ys: Vec<f64> = self.corners.iter().map(|(_, y)| *y).collect();
        let width = max(&xs) - min(&xs);
        let depth = max(&ys) - min(&ys);
        [width, depth, self.height]
            .into_iter()
            .fold(f64::INFINITY, f64::min)
    }

    fn provenance(&self) -> String {
        format!(
            "{} ({} corners, {}, height {})",
            self.name,
            self.corners.len(),
            match self.corner_radius {
                Some(r) => format!("corner radius {r}"),
                None => "sharp corners".to_owned(),
            },
            self.height
        )
    }
}

fn min(values: &[f64]) -> f64 {
    values.iter().copied().fold(f64::INFINITY, f64::min)
}
fn max(values: &[f64]) -> f64 {
    values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
}

/// Twenty parts: plain blocks of varying proportion, L-shapes, and outlines
/// whose corners are arcs rather than sharp.
fn corpus() -> Vec<Part> {
    let rectangle = |w: f64, d: f64| vec![(0.0, 0.0), (w, 0.0), (w, d), (0.0, d)];
    let ell = |w: f64, d: f64, cut: f64| {
        vec![
            (0.0, 0.0),
            (w, 0.0),
            (w, d - cut),
            (w - cut, d - cut),
            (w - cut, d),
            (0.0, d),
        ]
    };

    let mut parts = Vec::new();
    for (name, corners, height) in [
        ("block-square", rectangle(40.0, 40.0), 10.0),
        ("block-wide", rectangle(120.0, 20.0), 10.0),
        ("block-tall", rectangle(20.0, 20.0), 80.0),
        ("block-thin", rectangle(60.0, 40.0), 2.0),
        ("block-small", rectangle(6.0, 4.0), 3.0),
        ("block-large", rectangle(400.0, 300.0), 60.0),
        ("block-sliver", rectangle(100.0, 3.0), 20.0),
        ("plate", rectangle(60.0, 40.0), 10.0),
    ] {
        parts.push(Part {
            name,
            corners,
            corner_radius: None,
            height,
        });
    }
    for (name, corners, height) in [
        ("ell-even", ell(40.0, 40.0, 15.0), 10.0),
        ("ell-deep", ell(60.0, 60.0, 45.0), 12.0),
        ("ell-shallow", ell(80.0, 40.0, 8.0), 6.0),
        ("ell-thick", ell(50.0, 50.0, 20.0), 40.0),
        ("ell-thin", ell(50.0, 50.0, 20.0), 2.5),
        ("ell-small", ell(10.0, 10.0, 4.0), 4.0),
    ] {
        parts.push(Part {
            name,
            corners,
            corner_radius: None,
            height,
        });
    }
    for (name, corners, radius, height) in [
        ("rounded-plate", rectangle(60.0, 40.0), 5.0, 10.0),
        ("rounded-square", rectangle(40.0, 40.0), 10.0, 10.0),
        ("rounded-wide", rectangle(120.0, 30.0), 8.0, 12.0),
        ("rounded-tight", rectangle(30.0, 30.0), 2.0, 8.0),
        ("rounded-tall", rectangle(30.0, 30.0), 6.0, 60.0),
        ("rounded-thin", rectangle(70.0, 50.0), 10.0, 3.0),
    ] {
        parts.push(Part {
            name,
            corners,
            corner_radius: Some(radius),
            height,
        });
    }
    parts
}

/// The extrusion request for a part.
///
/// Corners become arcs when the part asks for them, which is what puts
/// cylindrical faces into the corpus rather than only planar ones.
fn request(part: &Part) -> Result<ExtrudeRequest> {
    let mut segments = Vec::new();

    match part.corner_radius {
        None => {
            let points: Vec<PlanarPoint> = part
                .corners
                .iter()
                .map(|(x, y)| PlanarPoint::new(*x, *y))
                .collect::<Result<_>>()?;
            for (index, start) in points.iter().enumerate() {
                segments.push(ProfileSegment::new(
                    StableEntityId::new(),
                    SegmentGeometry::line(*start, points[(index + 1) % points.len()])?,
                ));
            }
        }
        Some(radius) => {
            // Every rounded outline in this corpus is a rectangle, which keeps
            // the arithmetic exact: each corner is a quarter turn about a
            // centre inset by the radius on both axes, joined by straight runs.
            use std::f64::consts::{FRAC_PI_2, PI};

            let xs: Vec<f64> = part.corners.iter().map(|(x, _)| *x).collect();
            let ys: Vec<f64> = part.corners.iter().map(|(_, y)| *y).collect();
            let (x0, x1) = (min(&xs), max(&xs));
            let (y0, y1) = (min(&ys), max(&ys));

            // Anticlockwise from the bottom-left corner, each arc starting
            // where the run into it ends.
            let corners = [
                (x0 + radius, y0 + radius, PI),
                (x1 - radius, y0 + radius, PI + FRAC_PI_2),
                (x1 - radius, y1 - radius, 0.0),
                (x0 + radius, y1 - radius, FRAC_PI_2),
            ];

            let at = |cx: f64, cy: f64, angle: f64| -> Result<PlanarPoint> {
                PlanarPoint::new(cx + radius * angle.cos(), cy + radius * angle.sin())
            };

            for (index, (cx, cy, from)) in corners.iter().enumerate() {
                segments.push(ProfileSegment::new(
                    StableEntityId::new(),
                    SegmentGeometry::arc(
                        PlanarPoint::new(*cx, *cy)?,
                        radius,
                        *from,
                        from + FRAC_PI_2,
                    )?,
                ));

                let (nx, ny, next_from) = corners[(index + 1) % corners.len()];
                segments.push(ProfileSegment::new(
                    StableEntityId::new(),
                    SegmentGeometry::line(at(*cx, *cy, from + FRAC_PI_2)?, at(nx, ny, next_from)?)?,
                ));
            }
        }
    }

    Ok(ExtrudeRequest::new(
        Profile::new(
            SketchPlane::world_xy(),
            ProfileLoop::new(segments)?,
            Vec::new(),
        )?,
        ExtrudeExtent::blind(part.height)?,
        false,
    ))
}

/// Builds a part, or explains which part could not be built.
fn build(kernel: &mut OcctKernel, part: &Part) -> ShapeHandle {
    let request = request(part)
        .unwrap_or_else(|e| panic!("{}: the corpus itself is malformed: {e}", part.provenance()));
    kernel
        .extrude(&request, &OperationContext::default())
        .unwrap_or_else(|e| panic!("{}: could not be extruded: {e}", part.provenance()))
        .shape
}

#[test]
fn every_part_of_the_corpus_builds_into_a_sound_solid() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut kernel = OcctKernel::new().expect("opens");
    for part in corpus() {
        let shape = build(&mut kernel, &part);
        assert!(
            kernel.is_valid(shape).expect("checks"),
            "{}: the corpus must be sound before it can test anything",
            part.provenance()
        );
        let (faces, volume) = kernel.shape_stats(shape).expect("measures");
        assert!(faces >= 5, "{}: only {faces} faces", part.provenance());
        assert!(volume > 0.0, "{}: no volume", part.provenance());
        kernel.release(shape);
    }
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_fillet_is_either_sound_or_refused_at_every_radius() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut kernel = OcctKernel::new().expect("opens");
    let context = OperationContext::default();
    let mut report = Vec::new();

    for part in corpus() {
        let limit = part.smallest_dimension() / 2.0;
        let mut largest_sound: Option<f64> = None;
        let mut refusals = 0;

        // From well inside the geometric limit to well past it. The point of
        // going past is that the interesting failures live there.
        for step in 1..=12 {
            let radius = limit * f64::from(step) / 6.0;
            let solid = build(&mut kernel, &part);
            let outcome = kernel.fillet_all(solid, radius, &context);

            match outcome {
                Ok(rounded) => {
                    // The adapter promises everything it returns is sound.
                    assert!(
                        kernel.is_valid(rounded).expect("checks"),
                        "{} at radius {radius}: a shape was returned that Open \
                         CASCADE reports as invalid; the adapter's check did not hold",
                        part.provenance()
                    );
                    let (_, volume) = kernel.shape_stats(rounded).expect("measures");
                    assert!(
                        volume > 0.0,
                        "{} at radius {radius}: rounded to nothing",
                        part.provenance()
                    );
                    largest_sound = Some(radius);
                    kernel.release(rounded);
                }
                Err(error) => {
                    // A refusal must be a kernel failure, not a panic and not
                    // a wrong-argument complaint about a legal radius.
                    assert_eq!(
                        error.kind(),
                        ErrorKind::Kernel,
                        "{} at radius {radius}: {error}",
                        part.provenance()
                    );
                    refusals += 1;
                }
            }
            kernel.release(solid);
        }

        report.push(format!(
            "  {:<16} limit {:>7.3}  largest sound {:>8}  refusals {refusals}/12",
            part.name,
            limit,
            match largest_sound {
                Some(r) => format!("{r:.3}"),
                None => "none".to_owned(),
            }
        ));
        assert!(
            largest_sound.is_some(),
            "{}: no radius at all could be rounded, which is not a limit but a \
             failure to fillet anything",
            part.provenance()
        );
    }

    eprintln!("fillet limits, radius in mm:\n{}", report.join("\n"));
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_shell_is_either_sound_or_refused_at_every_thickness() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut kernel = OcctKernel::new().expect("opens");
    let context = OperationContext::default();
    let mut report = Vec::new();

    for part in corpus() {
        let limit = part.smallest_dimension() / 2.0;
        let mut largest_sound: Option<f64> = None;
        let mut refusals = 0;

        for step in 1..=12 {
            let thickness = limit * f64::from(step) / 6.0;
            let request = request(&part).expect("a valid part");
            let result = kernel
                .extrude(&request, &context)
                .unwrap_or_else(|e| panic!("{}: {e}", part.provenance()));

            // Opened at the end cap, which the extrusion already named.
            let open: Vec<_> = result.end_cap.clone();
            assert!(
                !open.is_empty(),
                "{}: no end cap to open",
                part.provenance()
            );

            match kernel.shell(result.shape, thickness, &open, &context) {
                Ok(hollow) => {
                    assert!(
                        kernel.is_valid(hollow).expect("checks"),
                        "{} at wall {thickness}: an invalid shape was returned",
                        part.provenance()
                    );
                    let (_, hollow_volume) = kernel.shape_stats(hollow).expect("measures");
                    let (_, solid_volume) = kernel.shape_stats(result.shape).expect("measures");
                    assert!(
                        hollow_volume > 0.0 && hollow_volume <= solid_volume + 1e-6,
                        "{} at wall {thickness}: hollowing produced {hollow_volume} mm^3 \
                         from a solid of {solid_volume}",
                        part.provenance()
                    );
                    largest_sound = Some(thickness);
                    kernel.release(hollow);
                }
                Err(error) => {
                    assert_eq!(
                        error.kind(),
                        ErrorKind::Kernel,
                        "{} at wall {thickness}: {error}",
                        part.provenance()
                    );
                    refusals += 1;
                }
            }
            kernel.release(result.shape);
        }

        report.push(format!(
            "  {:<16} limit {:>7.3}  largest sound {:>8}  refusals {refusals}/12",
            part.name,
            limit,
            match largest_sound {
                Some(t) => format!("{t:.3}"),
                None => "none".to_owned(),
            }
        ));
    }

    eprintln!("shell limits, wall in mm:\n{}", report.join("\n"));
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn the_same_operation_on_the_same_part_gives_the_same_result_twice() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut kernel = OcctKernel::new().expect("opens");
    let context = OperationContext::default();

    for part in corpus() {
        let radius = part.smallest_dimension() / 8.0;
        let mut measured = Vec::new();

        for _ in 0..2 {
            let solid = build(&mut kernel, &part);
            let rounded = kernel
                .fillet_all(solid, radius, &context)
                .unwrap_or_else(|e| panic!("{} at radius {radius}: {e}", part.provenance()));
            measured.push(kernel.shape_stats(rounded).expect("measures"));
            kernel.release(rounded);
            kernel.release(solid);
        }

        assert_eq!(
            measured[0].0,
            measured[1].0,
            "{}: two identical fillets gave different face counts",
            part.provenance()
        );
        assert!(
            (measured[0].1 - measured[1].1).abs() < 1e-9,
            "{}: two identical fillets gave volumes {} and {}",
            part.provenance(),
            measured[0].1,
            measured[1].1
        );
    }
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn an_impossible_request_is_refused_rather_than_approximated() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut kernel = OcctKernel::new().expect("opens");
    let context = OperationContext::default();
    let part = &corpus()[7];
    let solid = build(&mut kernel, part);

    // Radii and walls far beyond anything the part could carry.
    for radius in [1_000.0, 10_000.0] {
        let err = kernel
            .fillet_all(solid, radius, &context)
            .err()
            .unwrap_or_else(|| panic!("{}: radius {radius} must not succeed", part.provenance()));
        assert_eq!(err.kind(), ErrorKind::Kernel);
    }
    for radius in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let err = kernel
            .fillet_all(solid, radius, &context)
            .err()
            .unwrap_or_else(|| panic!("radius {radius} is not a radius"));
        assert_eq!(err.kind(), ErrorKind::Input, "radius {radius}: {err}");
    }

    let result = kernel
        .extrude(&request(part).expect("valid"), &context)
        .expect("builds");
    for thickness in [0.0, -1.0, f64::NAN] {
        let err = kernel
            .shell(result.shape, thickness, &result.end_cap, &context)
            .err()
            .unwrap_or_else(|| panic!("wall {thickness} is not a wall"));
        assert_eq!(err.kind(), ErrorKind::Input);
    }
    assert_eq!(
        kernel
            .shell(result.shape, 1.0, &[], &context)
            .expect_err("a shell with nothing open is the solid it came from")
            .kind(),
        ErrorKind::Input
    );

    kernel.release(result.shape);
    kernel.release(solid);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn the_radii_that_open_cascade_calls_success_are_still_refused() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    // The measured case, kept as a regression. On a 60 x 40 x 10 plate,
    // BRepFilletAPI_MakeFillet reports IsDone() for radius 5.1 and 6.0 and
    // produces shapes that fail BRepCheck_Analyzer and enclose ~25 800 mm^3 —
    // more than the 24 000 mm^3 block they were cut from. Rounding a convex
    // edge removes material, so those shapes are not answers. If this test
    // ever starts failing because the fillets succeed, check the volume before
    // celebrating.
    let mut kernel = OcctKernel::new().expect("opens");
    let context = OperationContext::default();
    let plate = Part {
        name: "plate",
        corners: vec![(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)],
        corner_radius: None,
        height: 10.0,
    };

    let solid = build(&mut kernel, &plate);
    let (_, block) = kernel.shape_stats(solid).expect("measures");
    assert!((block - 24_000.0).abs() < 1e-6, "the plate is 24000 mm^3");

    for radius in [5.0, 5.1, 6.0] {
        let err = kernel
            .fillet_all(solid, radius, &context)
            .err()
            .unwrap_or_else(|| panic!("radius {radius} is past this plate's limit and must fail"));
        assert_eq!(err.kind(), ErrorKind::Kernel, "radius {radius}: {err}");
    }

    // And a radius inside the limit still works, so the refusals above are a
    // limit and not a fillet that never works.
    let rounded = kernel
        .fillet_all(solid, 4.0, &context)
        .expect("4 mm is well within a 10 mm plate");
    let (_, volume) = kernel.shape_stats(rounded).expect("measures");
    assert!(
        volume < block,
        "rounding convex edges removed nothing: {volume} from {block}"
    );

    kernel.release(rounded);
    kernel.release(solid);
    assert_eq!(kernel.live_shape_count(), 0);
}
