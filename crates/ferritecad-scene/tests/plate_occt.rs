// SPDX-License-Identifier: MIT
//! The committed plate, drawn from geometry Open CASCADE actually built.
//!
//! Everything else about the loader is settled against the mock, which is what
//! lets those rules be stated on every platform. What cannot be settled that
//! way is whether the numbers are real: a mock that reported a 60 x 40 x 10 box
//! would satisfy the same assertions while computing nothing. So this file runs
//! the same path against the pinned kernel and measures what came back.
//!
//! Skipped rather than failed on a build without Open CASCADE: its absence is a
//! build configuration. The pin workflow sets `FERRITECAD_REQUIRE_OCCT=1`, so
//! the run whose purpose is to prove the adapter works cannot pass by skipping.

use ferritecad_kernel::{OperationContext, TessellationParams};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_scene::snapshot_of;

#[test]
fn the_plate_is_read_from_disk_into_real_geometry() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plate.fcad");
    std::fs::copy(ferritecad_fixtures::plate_source(), &path).expect("copies the fixture");
    let before = std::fs::read(&path).expect("reads the copy");

    let mut kernel = OcctKernel::new().expect("opens a kernel session");
    let snapshot = snapshot_of(
        &path,
        &mut kernel,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the plate loads");

    // One body, and a box's worth of triangles at the very least: six planar
    // faces cannot be covered by fewer than twelve.
    assert_eq!(snapshot.meshes().len(), 1);
    assert_eq!(snapshot.draws().len(), 1);
    assert!(
        snapshot.meshes()[0].triangle_count() >= 12,
        "a box came back as {} triangles",
        snapshot.meshes()[0].triangle_count()
    );

    // 60 x 40 x 10 is what the fixture describes, so it is what the kernel must
    // have built. A loader that dropped the extrusion or the placement would
    // still produce a mesh, and it would be the wrong size.
    let (min, max) = snapshot.bounds().expect("the plate has extent");
    let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    assert!((size[0] - 60.0).abs() < 1e-3, "{size:?}");
    assert!((size[1] - 40.0).abs() < 1e-3, "{size:?}");
    assert!((size[2] - 10.0).abs() < 1e-3, "{size:?}");

    // The real session, not a counter kept by a mock.
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "the picture is packed and Open CASCADE is still holding the solids"
    );

    // And the document is exactly as it was found.
    assert_eq!(std::fs::read(&path).expect("reads the copy"), before);
    for sidecar in ["fcad-wal", "fcad-shm", "fcad-cache"] {
        assert!(
            !path.with_extension(sidecar).exists(),
            "reading the plate left a .{sidecar} beside it"
        );
    }
}

#[test]
fn a_cancelled_load_leaves_the_real_session_empty() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("plate.fcad");
    std::fs::copy(ferritecad_fixtures::plate_source(), &path).expect("copies the fixture");

    // Load it once so the session has certainly held real shapes, then abandon
    // a second load. Cancellation is checked before each feature, so this ends
    // between the document being read and the first solid surviving.
    let mut kernel = OcctKernel::new().expect("opens a kernel session");
    snapshot_of(
        &path,
        &mut kernel,
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the plate loads");

    let cancel = ferritecad_kernel::CancelToken::new();
    cancel.cancel();
    let error = snapshot_of(
        &path,
        &mut kernel,
        &TessellationParams::default(),
        &OperationContext::default().with_cancel(cancel),
    )
    .expect_err("a cancelled load must not produce a picture");
    assert_eq!(error.kind(), ferritecad_types::ErrorKind::Cancellation);
    assert_eq!(kernel.live_shape_count(), 0);
}
