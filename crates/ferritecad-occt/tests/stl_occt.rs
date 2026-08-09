// SPDX-License-Identifier: MIT
//! The plate exported from real geometry.
//!
//! The writer is pure Rust over a `Mesh` and is tested as such elsewhere. What
//! this adds is the part nobody can check by reading code: that the mesh Open
//! CASCADE actually produces is one an STL reader would accept — no zero-area
//! facets, every normal outward, and the same file every time.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_eval::rebuild_cold;
use ferritecad_export::{binary_stl, binary_stl_len};
use ferritecad_fixtures::open_plate;
use ferritecad_kernel::{GeometryKernel, OperationContext, TessellationParams};
use ferritecad_occt::{OcctKernel, is_available};

const WIDTH: f64 = 60.0;
const DEPTH: f64 = 40.0;
const HEIGHT: f64 = 10.0;

fn plate_stl(kernel: &mut OcctKernel) -> Vec<u8> {
    let dir = tempfile::tempdir().expect("temp dir");
    let document = open_plate(dir.path()).expect("the fixture opens");
    let built = rebuild_cold(&document, kernel, &OperationContext::default()).expect("rebuilds");

    let extrude = document
        .objects()
        .expect("reads objects")
        .into_iter()
        .find(|object| {
            matches!(
                object.payload,
                ferritecad_document::ObjectPayload::Extrude(_)
            )
        })
        .expect("the plate has an extrusion");
    let shape = built.shape(extrude.id).expect("a solid");

    let mesh = kernel
        .tessellate(
            shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("draws");
    let bytes = binary_stl(&mesh).expect("writes");
    built.release_all(kernel);
    bytes
}

#[test]
fn open_cascade_geometry_exports_to_a_file_a_reader_would_accept() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut kernel = OcctKernel::new().expect("opens");
    let bytes = plate_stl(&mut kernel);

    assert_eq!(
        bytes.len(),
        usize::try_from(binary_stl_len(12)).expect("the plate STL fits in memory"),
        "a box is twelve triangles"
    );
    let count = u32::from_le_bytes(bytes[80..84].try_into().expect("four bytes"));
    assert_eq!(count, 12);

    let read = |at: usize| {
        f64::from(f32::from_le_bytes(
            bytes[at..at + 4].try_into().expect("four"),
        ))
    };
    let centre = [WIDTH / 2.0, DEPTH / 2.0, HEIGHT / 2.0];
    let expected = 2.0 * (WIDTH * DEPTH + WIDTH * HEIGHT + DEPTH * HEIGHT);
    let mut total = 0.0;

    for index in 0..count as usize {
        let at = 84 + index * 50;
        let triple = |offset: usize| {
            [
                read(at + offset),
                read(at + offset + 4),
                read(at + offset + 8),
            ]
        };
        let normal = triple(0);
        let corners = [triple(12), triple(24), triple(36)];

        let u = [
            corners[1][0] - corners[0][0],
            corners[1][1] - corners[0][1],
            corners[1][2] - corners[0][2],
        ];
        let v = [
            corners[2][0] - corners[0][0],
            corners[2][1] - corners[0][1],
            corners[2][2] - corners[0][2],
        ];
        let cross = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let area = (cross[0].powi(2) + cross[1].powi(2) + cross[2].powi(2)).sqrt() / 2.0;
        assert!(area > 0.0, "facet {index} has no area");
        total += area;

        let middle = [
            (corners[0][0] + corners[1][0] + corners[2][0]) / 3.0,
            (corners[0][1] + corners[1][1] + corners[2][1]) / 3.0,
            (corners[0][2] + corners[1][2] + corners[2][2]) / 3.0,
        ];
        let outward: f64 = (0..3)
            .map(|axis| normal[axis] * (middle[axis] - centre[axis]))
            .sum();
        assert!(
            outward > 0.0,
            "facet {index} faces into the solid; Open CASCADE reports most faces reversed, \
             and this is what happens when that is not undone"
        );
    }

    assert!(
        (total - expected).abs() < 1e-6,
        "the plate's surface is {expected} mm^2, the file describes {total}"
    );
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn two_sessions_export_the_same_part_to_the_same_bytes() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let mut one = OcctKernel::new().expect("opens");
    let mut other = OcctKernel::new().expect("opens");
    assert_eq!(
        plate_stl(&mut one),
        plate_stl(&mut other),
        "the same part must export to the same file, whichever session drew it"
    );
}
