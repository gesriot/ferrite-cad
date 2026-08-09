// SPDX-License-Identifier: MIT
//! The committed plate, written out as STL.
//!
//! The unit tests hold the format to its own rules; this holds it to a real
//! part. A 60 x 40 x 10 plate is twelve triangles and 684 bytes, its facets add
//! up to the surface area of a box, and every one of them faces outward — none
//! of which can be checked by reading the writer.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use ferritecad_eval::rebuild_cold;
use ferritecad_export::{binary_stl, binary_stl_len};
use ferritecad_fixtures::open_plate;
use ferritecad_kernel::{
    GeometryKernel, Mesh, OperationContext, TessellationParams, mock::MockKernel,
};

const WIDTH: f64 = 60.0;
const DEPTH: f64 = 40.0;
const HEIGHT: f64 = 10.0;

/// The plate, drawn.
fn plate_mesh(kernel: &mut MockKernel) -> Mesh {
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
    built.release_all(kernel);
    mesh
}

/// Every facet, read back out of the file.
fn facets(bytes: &[u8]) -> Vec<([f64; 3], [[f64; 3]; 3])> {
    let count = u32::from_le_bytes(bytes[80..84].try_into().expect("four bytes")) as usize;
    let read = |at: usize| {
        f64::from(f32::from_le_bytes(
            bytes[at..at + 4].try_into().expect("four"),
        ))
    };

    (0..count)
        .map(|index| {
            let at = 84 + index * 50;
            let triple = |offset: usize| {
                [
                    read(at + offset),
                    read(at + offset + 4),
                    read(at + offset + 8),
                ]
            };
            assert_eq!(
                u16::from_le_bytes(bytes[at + 48..at + 50].try_into().expect("two bytes")),
                0,
                "the attribute word means different things to different readers"
            );
            (triple(0), [triple(12), triple(24), triple(36)])
        })
        .collect()
}

#[test]
fn the_plate_is_twelve_triangles_and_684_bytes() {
    let mut kernel = MockKernel::new();
    let mesh = plate_mesh(&mut kernel);
    assert_eq!(mesh.triangle_count(), 12, "a box is six faces of two");

    let bytes = binary_stl(&mesh).expect("writes");
    assert_eq!(bytes.len(), 684);
    assert_eq!(bytes.len(), binary_stl_len(12));
    assert_eq!(
        u32::from_le_bytes(bytes[80..84].try_into().expect("four bytes")),
        12,
        "the count is little-endian, whatever this machine is"
    );
}

#[test]
fn writing_the_same_mesh_twice_gives_the_same_bytes() {
    let mut kernel = MockKernel::new();
    let mesh = plate_mesh(&mut kernel);

    let once = binary_stl(&mesh).expect("writes");
    let twice = binary_stl(&mesh).expect("writes again");
    assert_eq!(once, twice);

    // And a second run of the whole thing, in a session of its own, because a
    // timestamp would only show up between processes if it showed up at all.
    let mut other = MockKernel::new();
    let again = binary_stl(&plate_mesh(&mut other)).expect("writes");
    assert_eq!(once, again, "the same part must export to the same file");
}

#[test]
fn the_facets_add_up_to_the_surface_of_the_plate() {
    let mut kernel = MockKernel::new();
    let bytes = binary_stl(&plate_mesh(&mut kernel)).expect("writes");

    let expected = 2.0 * (WIDTH * DEPTH + WIDTH * HEIGHT + DEPTH * HEIGHT);
    let total: f64 = facets(&bytes)
        .iter()
        .map(|(_, corners)| {
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
            (cross[0].powi(2) + cross[1].powi(2) + cross[2].powi(2)).sqrt() / 2.0
        })
        .sum();

    assert!(
        (total - expected).abs() < 1e-6,
        "the plate's surface is {expected} mm^2, the file describes {total}"
    );
}

#[test]
fn every_facet_faces_out_of_the_solid() {
    let mut kernel = MockKernel::new();
    let bytes = binary_stl(&plate_mesh(&mut kernel)).expect("writes");
    let centre = [WIDTH / 2.0, DEPTH / 2.0, HEIGHT / 2.0];

    for (index, (normal, corners)) in facets(&bytes).iter().enumerate() {
        let length = (normal[0].powi(2) + normal[1].powi(2) + normal[2].powi(2)).sqrt();
        assert!(
            (length - 1.0).abs() < 1e-6,
            "facet {index} has a normal of length {length}"
        );

        // Pointing away from the middle of the plate is what outward means for
        // a convex solid, and a box is convex.
        let middle = [
            (corners[0][0] + corners[1][0] + corners[2][0]) / 3.0,
            (corners[0][1] + corners[1][1] + corners[2][1]) / 3.0,
            (corners[0][2] + corners[1][2] + corners[2][2]) / 3.0,
        ];
        let outward: f64 = (0..3)
            .map(|axis| normal[axis] * (middle[axis] - centre[axis]))
            .sum();
        assert!(outward > 0.0, "facet {index} faces into the solid");
    }
}

#[test]
fn the_header_carries_no_date() {
    let mut kernel = MockKernel::new();
    let bytes = binary_stl(&plate_mesh(&mut kernel)).expect("writes");
    let header = String::from_utf8_lossy(&bytes[..80]);

    assert!(
        !header.starts_with("solid"),
        "that word makes readers expect text"
    );
    for digit in ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'] {
        assert!(
            !header.contains(digit),
            "a header with a number in it is a header that might carry a date: {header:?}"
        );
    }
}
