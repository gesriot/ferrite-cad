// SPDX-License-Identifier: MIT
//! Reading the committed STEP corpus, seven sound files and five damaged.
//!
//! The seven say what a correct import looks like. The five say what happens
//! when a file is not correct, and the answer measured on 8.0.1 is not one
//! thing: two are refused, two are read and described precisely, and one is
//! read, transferred and reported clean while carrying a malformed
//! coordinate. These tests hold the import to reporting all of that and to
//! claiming none of it as soundness.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::thread;

use ferritecad_exchange::{ColourSource, Import, Severity};
use ferritecad_kernel::GeometryKernel;
use ferritecad_occt::{OcctKernel, is_available};

fn corpus(kind: &str, name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step")
        .join(kind)
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn import(kernel: &mut OcctKernel, kind: &str, name: &str) -> Import {
    kernel
        .import_step(&corpus(kind, name))
        .unwrap_or_else(|e| panic!("{name}: the import itself failed: {e}"))
}

/// Releases every shape a scene refers to.
fn release(kernel: &mut OcctKernel, import: &Import) {
    if let Some(scene) = import.scene() {
        for shape in scene.shapes() {
            kernel.release(shape);
        }
    }
}

macro_rules! kernel_or_skip {
    () => {
        if !is_available() {
            eprintln!("skipped: this build has no Open CASCADE");
            return;
        } else {
            OcctKernel::new().expect("opens")
        }
    };
}

#[test]
fn every_sound_file_imports_into_a_scene() {
    let mut kernel = kernel_or_skip!();

    for name in [
        "01-single-part.step",
        "02-flat-assembly.step",
        "03-nested-assembly.step",
        "04-instance-colours.step",
        "05-inch-units.step",
        "06-unicode-names.step",
        "07-bare-geometry.step",
    ] {
        let outcome = import(&mut kernel, "canonical", name);
        let scene = outcome
            .scene()
            .unwrap_or_else(|| panic!("{name} was rejected: {:?}", outcome.diagnostics()));

        assert_eq!(
            outcome.diagnostics().len(),
            0,
            "{name} is a sound file and should have nothing to report: {:?}",
            outcome.diagnostics()
        );
        assert!(!scene.definitions.is_empty(), "{name} defined nothing");
        assert!(!scene.instances.is_empty(), "{name} placed nothing");
        assert!(
            scene.schema.contains("AP242"),
            "{name} declares {}",
            scene.schema
        );
        assert_eq!(scene.roots().count(), 1, "{name} should have one root");

        // Every definition's shape belongs to this session and is real.
        for definition in &scene.definitions {
            assert!(
                kernel.is_valid(definition.shape).expect("checks"),
                "{name}: the shape of {} is not sound",
                definition.name
            );
        }

        release(&mut kernel, &outcome);
    }
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn an_assembly_arrives_as_definitions_and_placements() {
    let mut kernel = kernel_or_skip!();

    // Four placements of one definition: the file says so, and collapsing it
    // into four copies would lose the only thing that made it a pattern.
    let outcome = import(&mut kernel, "canonical", "04-instance-colours.step");
    let scene = outcome.scene().expect("imports");

    assert_eq!(scene.definitions.len(), 2, "the pattern and the bolt");
    let bolt = scene
        .definitions
        .iter()
        .position(|definition| definition.name == "Bolt")
        .expect("the bolt is named");

    let placements: Vec<&ferritecad_exchange::Instance> = scene
        .instances
        .iter()
        .filter(|instance| instance.definition == bolt)
        .collect();
    assert_eq!(placements.len(), 4, "four bolts from one definition");

    // Each sits somewhere different, which is the whole point of an instance.
    let mut positions: Vec<String> = placements
        .iter()
        .map(|instance| format!("{:?}", instance.translation()))
        .collect();
    positions.sort();
    positions.dedup();
    assert_eq!(positions.len(), 4, "the placements are not distinct");

    // One of them was painted over its definition.
    let overridden = placements
        .iter()
        .filter(|instance| instance.colour_source == ColourSource::Instance)
        .count();
    assert_eq!(overridden, 1, "exactly one bolt is recoloured");
    assert!(
        placements
            .iter()
            .any(|instance| instance.colour_source == ColourSource::Definition),
        "the others take the definition's colour"
    );

    release(&mut kernel, &outcome);
}

#[test]
fn names_units_and_colours_survive_the_journey() {
    let mut kernel = kernel_or_skip!();

    let outcome = import(&mut kernel, "canonical", "06-unicode-names.step");
    let scene = outcome.scene().expect("imports");
    let names: Vec<&str> = scene
        .definitions
        .iter()
        .map(|definition| definition.name.as_str())
        .collect();
    assert!(
        names.iter().any(|name| name.contains('組')),
        "the Japanese name did not survive: {names:?}"
    );
    assert!(
        names.iter().any(|name| name.contains('К')),
        "the Cyrillic name did not survive: {names:?}"
    );
    assert!(
        names.iter().any(|name| name.contains('É')),
        "the accented name did not survive: {names:?}"
    );
    release(&mut kernel, &outcome);

    // The unit the file declared, not the one we would have preferred.
    let outcome = import(&mut kernel, "canonical", "05-inch-units.step");
    assert_eq!(
        outcome.scene().expect("imports").source_unit.to_uppercase(),
        "INCH"
    );
    release(&mut kernel, &outcome);

    // Colours arrive linear. sRGB (0.8, 0.2, 0.2) is linear (0.6038, 0.0331,
    // 0.0331); calling what comes back sRGB would be wrong by a whole
    // transfer function.
    let outcome = import(&mut kernel, "canonical", "02-flat-assembly.step");
    let scene = outcome.scene().expect("imports");
    let coloured: Vec<[f64; 3]> = scene
        .instances
        .iter()
        .filter(|instance| instance.colour_source != ColourSource::None)
        .map(|instance| instance.colour)
        .collect();
    assert!(!coloured.is_empty(), "the assembly is painted");
    assert!(
        coloured
            .iter()
            .any(|colour| (colour[0] - 0.603_827).abs() < 1e-4),
        "expected a linear red of 0.6038, got {coloured:?}"
    );
    release(&mut kernel, &outcome);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_file_that_cannot_be_read_is_rejected_with_its_reasons() {
    let mut kernel = kernel_or_skip!();

    // Measured on 8.0.1: these two are refused at the reading stage.
    for name in ["01-truncated.step", "03-missing-terminator.step"] {
        let outcome = import(&mut kernel, "damaged", name);
        assert!(
            matches!(outcome, Import::Rejected { .. }),
            "{name} should have been rejected"
        );
        assert!(
            outcome.scene().is_none(),
            "{name} produced a scene it should not have"
        );
        // A refusal with no reason is a refusal nobody can act on.
        assert!(
            !outcome.diagnostics().is_empty(),
            "{name} was rejected without saying why"
        );
    }
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_recovered_file_imports_and_says_what_was_wrong_with_it() {
    let mut kernel = kernel_or_skip!();

    // These two are read completely and produce the same geometry as the
    // undamaged originals. What separates them from a sound file is only the
    // diagnostics, which is exactly why they must not be discarded.
    for (name, expected) in [
        ("02-broken-reference.step", "unresolved"),
        ("05-duplicate-entity-id.step", "SEVERAL TIMES"),
    ] {
        let outcome = import(&mut kernel, "damaged", name);
        let scene = outcome
            .scene()
            .unwrap_or_else(|| panic!("{name} should still import"));
        assert!(!scene.definitions.is_empty());

        assert!(
            outcome.failures() > 0,
            "{name} imported with nothing reported, and it should not have"
        );
        let said = outcome.diagnostics().iter().any(|diagnostic| {
            diagnostic
                .message
                .to_lowercase()
                .contains(&expected.to_lowercase())
                || diagnostic
                    .entity
                    .to_lowercase()
                    .contains(&expected.to_lowercase())
        });
        assert!(
            said,
            "{name}: nothing mentioned {expected}: {:?}",
            outcome.diagnostics()
        );

        // And a person could read it.
        let sentence = outcome.diagnostics()[0].to_string();
        assert!(
            sentence.contains("reading") || sentence.contains("building"),
            "a diagnostic should say when it happened: {sentence}"
        );

        release(&mut kernel, &outcome);
    }
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn a_file_nothing_was_noticed_about_is_not_thereby_sound() {
    let mut kernel = kernel_or_skip!();

    // The measurement this whole design rests on. A coordinate written `30..`
    // is malformed; Open CASCADE 8.0.1 reads it, transfers it, reports
    // nothing, and produces the same geometry as the undamaged file. There is
    // no flag that could have been set correctly here, which is why there is
    // none.
    let damaged = import(&mut kernel, "damaged", "04-corrupted-number.step");
    let sound = import(&mut kernel, "canonical", "04-instance-colours.step");

    assert!(damaged.scene().is_some(), "it imports");
    assert_eq!(
        damaged.diagnostics().len(),
        0,
        "nothing is reported about it, which is the point"
    );

    // Indistinguishable from the sound file by everything available.
    let (a, b) = (
        damaged.scene().expect("scene"),
        sound.scene().expect("scene"),
    );
    assert_eq!(a.definitions.len(), b.definitions.len());
    assert_eq!(a.instances.len(), b.instances.len());
    assert_eq!(a.source_unit, b.source_unit);

    release(&mut kernel, &damaged);
    release(&mut kernel, &sound);
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn nothing_that_is_not_a_step_file_becomes_a_scene() {
    let mut kernel = kernel_or_skip!();

    for rubbish in [
        b"".to_vec(),
        b"not a step file at all".to_vec(),
        vec![0u8; 512],
    ] {
        // An outright error is equally acceptable; what is not is a scene.
        if let Ok(outcome) = kernel.import_step(&rubbish) {
            assert!(
                matches!(outcome, Import::Rejected { .. }),
                "rubbish produced a scene"
            );
        }
    }
    assert_eq!(kernel.live_shape_count(), 0);
}

#[test]
fn simultaneous_imports_do_not_cross_sessions_or_abort_the_process() {
    if !is_available() {
        eprintln!("skipped: this build has no Open CASCADE");
        return;
    }

    // The first §18A pin run aborted on macOS while seven independent import
    // tests ran in parallel. This concentrates that pressure at one instant
    // instead of relying on the test harness's scheduling. It verifies the
    // public guarantee — concurrent sessions import safely and independently
    // — without claiming whether XDE document creation or the global messenger
    // caused the original abort; the observation did not isolate them.
    const WORKERS: usize = 8;
    const ROUNDS: usize = 4;
    let bytes = Arc::new(corpus("canonical", "04-instance-colours.step"));
    let start = Arc::new(Barrier::new(WORKERS));

    let workers: Vec<_> = (0..WORKERS)
        .map(|worker| {
            let bytes = Arc::clone(&bytes);
            let start = Arc::clone(&start);
            thread::spawn(move || {
                let mut kernel = OcctKernel::new().expect("opens a private session");
                start.wait();

                for round in 0..ROUNDS {
                    let outcome = kernel
                        .import_step(bytes.as_slice())
                        .unwrap_or_else(|error| {
                            panic!("worker {worker}, round {round}: import failed: {error}")
                        });
                    let scene = outcome.scene().unwrap_or_else(|| {
                        panic!(
                            "worker {worker}, round {round}: rejected: {:?}",
                            outcome.diagnostics()
                        )
                    });

                    assert!(outcome.diagnostics().is_empty());
                    assert_eq!(scene.definitions.len(), 2);
                    assert_eq!(scene.instances.len(), 5);
                    assert_eq!(
                        scene
                            .definitions
                            .iter()
                            .filter(|definition| definition.name == "Bolt")
                            .count(),
                        1
                    );
                    assert_eq!(
                        scene
                            .instances
                            .iter()
                            .filter(|instance| { instance.colour_source == ColourSource::Instance })
                            .count(),
                        1
                    );

                    release(&mut kernel, &outcome);
                    assert_eq!(
                        kernel.live_shape_count(),
                        0,
                        "worker {worker}, round {round} leaked another session's shape"
                    );
                }
            })
        })
        .collect();

    for worker in workers {
        worker.join().expect("an import worker did not survive");
    }
}

// Referenced so the severity type cannot quietly stop being part of the API.
const _: fn(Severity) -> bool = |severity| matches!(severity, Severity::Fail);
