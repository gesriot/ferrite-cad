// SPDX-License-Identifier: MIT
//! Turning a mesh into a file another program will accept.
//!
//! Only binary STL so far, and deliberately written here rather than asked of
//! Open CASCADE. A kernel's own writer is a black box that may change what it
//! emits between releases, and the one property this needs above all is that
//! the same mesh always produces the same bytes.
//!
//! # Why the same bytes matter
//!
//! Two runs of a build that differ only in a timestamp cannot be compared,
//! diffed, deduplicated or checksummed, and every one of those is something a
//! person exporting a part eventually wants. STL's 80-byte header is where
//! most writers put the date; this one puts a constant there and nothing else.
//!
//! # What is refused
//!
//! A mesh that fails [`Mesh::validate`], and a triangle with no area. The
//! second is not pedantry: a zero-area facet has no direction, so the normal
//! written for it would be invented, and a reader that trusts normals would be
//! misled about a surface that is not there.

use ferritecad_kernel::Mesh;
use ferritecad_types::{CadError, Result};

/// The fixed 80 bytes every file starts with.
///
/// Deliberately not beginning with `solid`: readers distinguish binary STL
/// from the ASCII form by exactly that word, and a binary file that opens with
/// it is read as text and rejected.
const HEADER: &[u8; HEADER_LEN] =
    b"FerriteCAD binary STL. Units are millimetres. No timestamp, by design.\0\0\0\0\0\0\0\0\0\0";

const HEADER_LEN: usize = 80;
const TRIANGLE_BYTES: usize = 50;

/// The exact byte size a mesh of this many triangles will occupy.
///
/// The count is a `u32` because that is the largest file binary STL can
/// describe. The result is a `u64` because such a file need not fit in a
/// 32-bit process's address space even though its size is well-defined.
pub const fn binary_stl_len(triangles: u32) -> u64 {
    (HEADER_LEN + size_of::<u32>()) as u64 + triangles as u64 * TRIANGLE_BYTES as u64
}

/// Writes `mesh` as binary STL.
///
/// Every triangle in the mesh is written, whichever face it belongs to: STL
/// has no notion of a face, so the names this project works so hard to keep
/// are exactly what the format throws away. Callers who need them must export
/// something else.
///
/// The facet normal is computed from the winding rather than copied from the
/// mesh's vertex normals. The two agree for a correct mesh, and where they
/// disagree the winding is what a reader will actually use to decide which
/// side of a surface is outside.
pub fn binary_stl(mesh: &Mesh) -> Result<Vec<u8>> {
    mesh.validate()?;

    let triangles = mesh.triangle_count();
    let count = u32::try_from(triangles).map_err(|_| {
        CadError::input(format!(
            "binary STL counts triangles in 32 bits and this mesh has {triangles}"
        ))
    })?;

    let file_len = usize::try_from(binary_stl_len(count)).map_err(|_| {
        CadError::input(format!(
            "the binary STL needs {} bytes, which this platform cannot address",
            binary_stl_len(count)
        ))
    })?;
    let mut out = Vec::new();
    out.try_reserve_exact(file_len).map_err(|source| {
        CadError::input_because(
            format!("the binary STL needs {file_len} bytes, which cannot be reserved"),
            source,
        )
    })?;
    out.extend_from_slice(HEADER);
    out.extend_from_slice(&count.to_le_bytes());

    for (index, triangle) in mesh.indices.chunks_exact(3).enumerate() {
        let corners = [
            vertex(mesh, triangle[0]),
            vertex(mesh, triangle[1]),
            vertex(mesh, triangle[2]),
        ];

        let normal = facet_normal(&corners).ok_or_else(|| {
            CadError::input(format!(
                "triangle {index} has no area, so it has no direction to write down"
            ))
        })?;

        for value in normal {
            write_float(&mut out, value);
        }
        for corner in &corners {
            for value in corner {
                write_float(&mut out, *value);
            }
        }
        // The attribute byte count. Non-zero values are a colour convention
        // that no two programs agree on, so this writes the only value every
        // reader understands.
        out.extend_from_slice(&0u16.to_le_bytes());
    }

    debug_assert_eq!(out.len(), file_len);
    Ok(out)
}

/// Narrows one STL scalar and gives zero its single canonical representation.
///
/// Rust, like IEEE 754, compares `-0.0` equal to `0.0`, but their bytes differ.
/// Leaving the sign in the file would let two equal [`Mesh`] values produce
/// different exports depending on the CPU operations that made their zeros.
fn write_float(out: &mut Vec<u8>, value: f64) {
    let narrowed = value as f32;
    let canonical = if narrowed == 0.0 { 0.0 } else { narrowed };
    out.extend_from_slice(&canonical.to_le_bytes());
}

fn vertex(mesh: &Mesh, index: u32) -> [f64; 3] {
    let at = index as usize * 3;
    [
        f64::from(mesh.positions[at]),
        f64::from(mesh.positions[at + 1]),
        f64::from(mesh.positions[at + 2]),
    ]
}

/// The unit normal of a triangle, or `None` when it has no area.
///
/// Computed in `f64` from `f32` positions on purpose: the cross product of two
/// short edges loses most of its precision, and the result is narrowed only
/// once, at the end.
fn facet_normal(corners: &[[f64; 3]; 3]) -> Option<[f64; 3]> {
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

    let length = (cross[0].powi(2) + cross[1].powi(2) + cross[2].powi(2)).sqrt();
    if !length.is_finite() || length <= 0.0 {
        return None;
    }
    Some([cross[0] / length, cross[1] / length, cross[2] / length])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferritecad_kernel::{MeshFaceRange, SessionId, ShapeHandle, SubShapeHandle, SubShapeKind};

    fn some_face() -> SubShapeHandle {
        SubShapeHandle::new(ShapeHandle::new(SessionId::new(), 0), SubShapeKind::Face, 0)
    }

    /// One triangle, belonging to one face.
    ///
    /// A face range is not optional decoration: `Mesh::validate` requires the
    /// ranges to account for every index, so there is no such thing as a
    /// triangle no face owns.
    fn one_triangle(positions: Vec<f32>) -> Mesh {
        Mesh {
            normals: [0.0, 0.0, 1.0].repeat(positions.len() / 3),
            positions,
            indices: vec![0, 1, 2],
            faces: vec![MeshFaceRange {
                face: some_face(),
                first_index: 0,
                index_count: 3,
            }],
            edges: None,
        }
    }

    #[test]
    fn the_header_is_fixed_and_says_nothing_about_when() {
        assert_eq!(HEADER.len(), HEADER_LEN);
        assert!(
            !HEADER.starts_with(b"solid"),
            "a binary file that opens with `solid` is read as text"
        );
    }

    #[test]
    fn the_size_is_exactly_what_the_format_says() {
        assert_eq!(binary_stl_len(0), 84);
        assert_eq!(binary_stl_len(12), 684);
        assert_eq!(binary_stl_len(u32::MAX), 214_748_364_834);
    }

    #[test]
    fn signed_zero_does_not_change_the_file() {
        let positive = one_triangle(vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        let mut negative = positive.clone();
        for coordinate in &mut negative.positions {
            if *coordinate == 0.0 {
                *coordinate = -0.0;
            }
        }

        assert_eq!(positive, negative, "signed zero is the same mesh value");
        assert_eq!(
            binary_stl(&positive).expect("writes"),
            binary_stl(&negative).expect("writes"),
            "equal mesh values must have one canonical byte representation"
        );
    }

    #[test]
    fn a_triangle_with_no_area_is_refused() {
        let flat = one_triangle(vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0]);
        let err = binary_stl(&flat).expect_err("collinear corners enclose nothing");
        assert!(err.to_string().contains("no area"));

        let same = one_triangle(vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
        assert!(binary_stl(&same).is_err());
    }

    #[test]
    fn the_normal_follows_the_winding() {
        let up = one_triangle(vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        let bytes = binary_stl(&up).expect("writes");
        let read =
            |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"));

        // The first triangle's normal begins right after the header and count.
        let at = HEADER_LEN + 4;
        assert_eq!([read(at), read(at + 4), read(at + 8)], [0.0, 0.0, 1.0]);

        let mut reversed = up.clone();
        reversed.indices = vec![0, 2, 1];
        let bytes = binary_stl(&reversed).expect("writes");
        let read =
            |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"));
        assert_eq!([read(at), read(at + 4), read(at + 8)], [0.0, 0.0, -1.0]);
    }

    #[test]
    fn a_mesh_that_does_not_hold_together_is_refused() {
        let mut dangling = one_triangle(vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        dangling.indices = vec![0, 1, 9];
        assert!(binary_stl(&dangling).is_err());

        let mut split = one_triangle(vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        split.faces = vec![MeshFaceRange {
            face: some_face(),
            first_index: 0,
            index_count: 2,
        }];
        assert!(
            binary_stl(&split).is_err(),
            "a range must not split a triangle"
        );
    }

    #[test]
    fn an_empty_mesh_is_a_file_with_no_triangles() {
        let bytes = binary_stl(&Mesh::default()).expect("writes");
        assert_eq!(
            bytes.len(),
            usize::try_from(binary_stl_len(0)).expect("an empty STL fits in memory")
        );
        assert_eq!(
            u32::from_le_bytes(
                bytes[HEADER_LEN..HEADER_LEN + 4]
                    .try_into()
                    .expect("four bytes")
            ),
            0
        );
    }
}
