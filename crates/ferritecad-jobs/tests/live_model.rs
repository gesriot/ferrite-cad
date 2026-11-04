// SPDX-License-Identifier: MIT
#[path = "live_model/fixture.rs"]
mod fixture;
#[path = "live_model/observed.rs"]
mod observed;

use ferritecad_document::{EditChange, ExpressionSlot};
use ferritecad_jobs::{DocumentSession, LiveDocument, PendingRevision};
use ferritecad_kernel::{GeometryKernel, OperationContext, TessellationParams};
use ferritecad_occt::OcctKernel;
use ferritecad_types::{ErrorKind, ObjectId, Result};
use observed::{Calls, Observed, Stop};

fn compare<K: GeometryKernel, R>(pending: &PendingRevision<K, R>) -> Result<()> {
    let fresh = ferritecad_scene::snapshot_of(
        pending.snapshot().path(),
        &mut OcctKernel::new()?,
        |_, _| unreachable!("no imports"),
        &TessellationParams::default(),
        &OperationContext::default(),
    )?;
    assert_eq!(pending.scene().rebuild, fresh.rebuild);
    assert_eq!(pending.scene().catalogue, fresh.catalogue);
    assert_eq!(pending.scene().snapshot.meshes(), fresh.snapshot.meshes());
    assert_eq!(pending.scene(), &fresh);
    Ok(())
}

#[test]
fn native_reused_plate_and_cut_suffix_match_fresh_scenes() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let path = root.path().join("part.fcad");
    let case = fixture::write(&path)?;
    let (kernel, reading) = Observed::new()?;
    let context = OperationContext::default();
    let mut live = LiveDocument::open(
        DocumentSession::open(&path)?,
        kernel,
        |_: &mut Observed, _: &[u8]| unreachable!("no imports"),
        TessellationParams::default(),
        &context,
    )?;
    assert!(
        live.scene().rebuild.all_built(),
        "{:?}",
        live.scene().rebuild
    );
    let initial = reading.calls.take();
    assert_eq!(
        (
            initial.extrude,
            initial.boolean,
            initial.fillet,
            initial.tessellate
        ),
        (33, 30, 2, 3)
    );
    let features = 35;
    assert_eq!(reading.live.get(), features);
    let parameter = |object: ObjectId, value: &str| EditChange::SetParameter {
        object,
        expression: value.into(),
    };
    for (change, expected) in [
        (parameter(case.late, "4 mm"), (1, 1, 0, 1)),
        (parameter(case.early, "12 mm"), (29, 28, 2, 1)),
        (
            EditChange::constant(case.b, ExpressionSlot::ExtrudeDistance, 4.)?,
            (1, 1, 0, 1),
        ),
        (
            EditChange::RenameParameter {
                object: case.late,
                name: "pocket_depth".into(),
            },
            (0, 0, 0, 0),
        ),
        (
            EditChange::SetExpressionText {
                object: case.last,
                slot: ExpressionSlot::ExtrudeDistance,
                source: "2 * pocket_depth / 2".into(),
            },
            (0, 0, 0, 0),
        ),
    ] {
        for action in 0..3 {
            let step = match action {
                0 => live.apply(
                    live.session().current().version(),
                    vec![change.clone()],
                    &context,
                )?,
                1 => live.undo(&context)?,
                _ => live.redo(&context)?,
            };
            let calls = reading.calls.take();
            assert_eq!(
                (calls.extrude, calls.boolean, calls.fillet, calls.tessellate),
                expected
            );
            assert_eq!(
                (calls.revolve, calls.chamfer, calls.restore, calls.identity),
                (0, 0, 0, 0)
            );
            if expected.0 == 0 {
                assert_eq!(calls, Calls::default());
            }
            // Boolean tools are unpublished. Completed feature results remain shared.
            assert_eq!(reading.live.get(), features + expected.0 + expected.2);
            compare(&step)?;
            step.commit_step()?;
            assert_eq!(reading.live.get(), features);
            reading.calls.take();
        }
    }
    let scene = live.scene().clone();
    let version = live.session().current().version();
    let edit = parameter(case.late, "5 mm");
    let candidate = live.apply(version, vec![edit.clone()], &context)?;
    assert_eq!(reading.live.get(), features + 1);
    drop(candidate);
    assert_eq!(reading.live.get(), features);
    for stop in [Stop::Extrude, Stop::BetweenNodes, Stop::Mesh] {
        reading.stop.set(Some(stop));
        let err = live
            .apply(version, vec![edit.clone()], &OperationContext::default())
            .err()
            .expect("cancelled candidate");
        assert_eq!(err.kind(), ErrorKind::Cancellation, "{err}");
        assert_eq!(live.scene(), &scene);
        assert_eq!(live.session().current().version(), version);
        assert_eq!(reading.live.get(), features);
    }
    reading.stop.set(None);
    let refused = live
        .apply(version, vec![parameter(case.early, "0 mm")], &context)
        .err()
        .expect("invalid height");
    assert_eq!(refused.kind(), ErrorKind::Input);
    let refusal = live
        .apply(
            version,
            vec![EditChange::constant(
                case.first,
                ExpressionSlot::FilletRadius,
                1000.,
            )?],
            &context,
        )
        .err()
        .expect("invalid fillet");
    assert!(
        ferritecad_eval::RebuildRefusal::from_error(&refusal).is_some(),
        "{refusal}"
    );
    assert_eq!(live.scene(), &scene);
    assert_eq!(reading.live.get(), features);
    reading.calls.take();
    live.apply(version, vec![edit], &context)?.commit_step()?;
    let calls = reading.calls.get();
    assert_eq!((calls.extrude, calls.boolean, calls.tessellate), (1, 1, 1));
    assert_eq!(reading.live.get(), features);
    drop(live);
    assert_eq!(reading.live.get(), 0);
    Ok(())
}
