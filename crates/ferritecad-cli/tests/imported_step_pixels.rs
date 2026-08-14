// SPDX-License-Identifier: MIT
//! The whole way through: a STEP file on disk, a document, and pixels.
//!
//! Every step of this path is gated somewhere already, and each of those gates
//! stops at the boundary of its own crate. This one starts with the shipped
//! command and ends with colours read back off a GPU, because the failures
//! worth catching here are the ones that live between two things that each
//! work: a document that stores a scene the loader cannot reopen, geometry
//! that reaches a snapshot and never reaches a triangle, an assembly framed so
//! badly that the picture is empty.
//!
//! Skipped when this build has no Open CASCADE or the machine has no usable
//! GPU adapter, and not otherwise: the pin workflow makes the first impossible,
//! and an error after an adapter was found is a failure rather than a skip.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

use ferritecad_kernel::{OperationContext, TessellationParams};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_scene::snapshot_of;
use ferritecad_types::ErrorKind;
use ferritecad_viewport::{Camera, Marked, Visibility};
use ferritecad_viewport_gpu::Renderer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MissingAdapter {
    Skip,
    Fail,
}

fn missing_adapter(required: bool) -> MissingAdapter {
    if required {
        MissingAdapter::Fail
    } else {
        MissingAdapter::Skip
    }
}

/// A renderer, unless this machine is allowed not to have one.
///
/// A contributor's headless machine may skip this gate. The pin workflow sets
/// `FERRITECAD_REQUIRE_GPU=1`, because its green result is a product claim that
/// the vertical path reached pixels rather than merely a log somebody has to
/// inspect for a skip line.
fn renderer_or_skip() -> Option<Renderer> {
    match Renderer::new() {
        Ok(renderer) => Some(renderer),
        Err(reason) if reason.kind() == ErrorKind::Unsupported => {
            match missing_adapter(std::env::var("FERRITECAD_REQUIRE_GPU").as_deref() == Ok("1")) {
                MissingAdapter::Fail => panic!(
                    "FERRITECAD_REQUIRE_GPU=1 was set, so the pixel gate may not skip: {reason}"
                ),
                MissingAdapter::Skip => {
                    eprintln!("skipped: {reason}");
                    None
                }
            }
        }
        Err(reason) => panic!("a renderer failed after adapter discovery: {reason}"),
    }
}

#[test]
fn the_pin_run_cannot_turn_a_missing_gpu_into_a_green_skip() {
    assert_eq!(missing_adapter(false), MissingAdapter::Skip);
    assert_eq!(missing_adapter(true), MissingAdapter::Fail);
}

fn ferritecad() -> PathBuf {
    let mut path = std::env::current_exe().expect("the test knows where it is");
    path.pop();
    path.pop();
    path.push(format!("ferritecad{}", std::env::consts::EXE_SUFFIX));
    path
}

fn corpus(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step/canonical")
        .join(name)
}

/// Imports one corpus file through the shipped command.
fn import(step: &str, into: &Path) {
    let output = Command::new(ferritecad())
        .arg("import-step")
        .arg(corpus(step))
        .arg("--output")
        .arg(into)
        .output()
        .expect("the command runs");
    assert!(
        output.status.success(),
        "importing {step} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        into.exists(),
        "the import reported success and wrote no document"
    );
}

/// Loads a document the way the viewer does.
fn picture(path: &Path) -> ferritecad_scene::LoadedScene {
    let mut kernel = OcctKernel::new().expect("opens a kernel session");
    let snapshot = snapshot_of(
        path,
        &mut kernel,
        |kernel, source| kernel.import_step(source),
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("the stored import reopens into a picture");
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "the picture is packed and Open CASCADE is still holding the assembly"
    );
    snapshot
}

/// How many pixels are neither the background nor nearly it.
fn drawn_pixels(frame: &ferritecad_viewport_gpu::Frame) -> usize {
    let mut drawn = 0;
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            let Some([r, g, b, _]) = frame.colour_at(x, y) else {
                continue;
            };
            // The clear colour is the darkest thing in the frame; anything the
            // shader lit is well above it.
            if u32::from(r) + u32::from(g) + u32::from(b) > 90 {
                drawn += 1;
            }
        }
    }
    drawn
}

#[test]
fn a_nested_assembly_goes_from_step_to_pixels() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let path = directory.path().join("assembly.fcad");
    import("03-nested-assembly.step", &path);

    // Four cubes in two groups: one definition drawn in four places, and the
    // groups themselves drawn nowhere. The same numbers the headless gates
    // assert about a fabricated scene, here about the real file.
    let loaded = picture(&path);
    let snapshot = loaded.snapshot;
    assert_eq!(snapshot.meshes().len(), 1, "the groups were meshed as well");
    assert_eq!(snapshot.draws().len(), 4);

    // And what a click on any of them would mean: the file's own name for the
    // cube, beside the source it came from. Four placements, one answer.
    assert_eq!(loaded.catalogue.len(), 1);
    let entry = &loaded.catalogue[0];
    let ferritecad_scene::SceneItem::Imported(reference) = &entry.item else {
        panic!("an imported definition was catalogued as a native body");
    };
    assert!(
        reference
            .definition_key()
            .starts_with("step.product_definition#"),
        "the catalogue does not name the definition the file named: {}",
        reference.definition_key()
    );

    let (min, max) = snapshot.bounds().expect("the assembly has extent");
    let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    assert!(
        size[0] > 30.0 && size[1] > 40.0,
        "four cubes 30 and 40 apart measure {size:?}"
    );

    let Some(mut renderer) = renderer_or_skip() else {
        return;
    };

    let mut camera = Camera::new();
    camera.resize(128, 128);
    camera
        .frame(snapshot.bounds().expect("the assembly has extent"))
        .expect("frames the assembly");

    let prepared = renderer
        .prepare(std::sync::Arc::new(snapshot))
        .expect("uploads the assembly");
    let frame = renderer
        .render(
            &prepared,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws a frame");

    // Pixels, not a promise. A snapshot that reached the GPU and drew nothing
    // is the failure this whole file exists to catch.
    let drawn = drawn_pixels(&frame);
    assert!(
        drawn > 200,
        "the assembly covered {drawn} pixels of {}",
        frame.width() * frame.height()
    );

    // What an empty document looks like through the same counter, so the
    // number above is known to be geometry rather than the background it is
    // drawn on.
    let nothing = renderer
        .prepare(std::sync::Arc::new(
            ferritecad_viewport::SnapshotBuilder::new().build(),
        ))
        .expect("uploads an empty scene");
    let blank = renderer
        .render(
            &nothing,
            &camera,
            Marked::Nothing,
            Marked::Nothing,
            &Visibility::default(),
        )
        .expect("draws an empty frame");
    assert_eq!(drawn_pixels(&blank), 0, "the background counts as drawn");

    // And what was clicked is one of the four cubes rather than a definition
    // number that outlived its snapshot.
    let picked = (0..frame.height())
        .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
        .find_map(|(x, y)| frame.snapshot().definition(frame.pick_at(x, y)));
    assert_eq!(picked, Some(0), "no pixel identified the cube");
}

#[test]
fn one_part_and_four_of_a_kind_measure_what_the_files_say() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }
    let directory = tempfile::tempdir().expect("a temporary directory is available");

    let single = directory.path().join("single.fcad");
    import("01-single-part.step", &single);
    let snapshot = picture(&single).snapshot;
    assert_eq!(snapshot.meshes().len(), 1);
    assert_eq!(snapshot.draws().len(), 1, "one part is drawn once");

    // The pattern: four bolts from one definition, one of them painted over
    // its definition's colour by the file.
    let pattern = directory.path().join("pattern.fcad");
    import("04-instance-colours.step", &pattern);
    let snapshot = picture(&pattern).snapshot;
    assert_eq!(snapshot.meshes().len(), 1, "four bolts are one definition");
    assert_eq!(snapshot.draws().len(), 4);

    let mut colours: Vec<[u32; 3]> = snapshot
        .draws()
        .iter()
        .map(|item| {
            [
                (item.colour[0] * 100.0).round() as u32,
                (item.colour[1] * 100.0).round() as u32,
                (item.colour[2] * 100.0).round() as u32,
            ]
        })
        .collect();
    colours.sort_unstable();
    colours.dedup();
    assert_eq!(
        colours.len(),
        2,
        "the file paints one bolt differently and the picture shows {colours:?}"
    );
}
