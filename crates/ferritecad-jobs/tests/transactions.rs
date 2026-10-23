// SPDX-License-Identifier: MIT
use ferritecad_document::{Document, EditChange, ExpressionSlot};
use ferritecad_jobs::{EditRequest, edit_copy};
use ferritecad_kernel::OperationContext;

#[test]
fn a_dimension_and_its_fillet_can_be_repaired_together() {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_sketch_solver::is_available(), "PLANEGCS"),
    ]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("source.fcad");
    let chain = ferritecad_fixtures::write_constrained_fillet_chain(&source, 8.0, 60.0, 40.0)
        .expect("fixture");
    let before = std::fs::read(&source).expect("source bytes");
    let reading = ferritecad_jobs::read_extrude_source(&source).expect("reading");
    let dimension = reading
        .expression_slots
        .iter()
        .find(|s| matches!(s.slot, ExpressionSlot::Distance { .. }) && s.expression.value() == 60.0)
        .expect("width");
    let mut request = EditRequest {
        source: source.clone(),
        expected: reading.version,
        changes: vec![
            EditChange::constant(dimension.object, dimension.slot, 10.0).expect("dimension"),
        ],
        destination: root.path().join("edited.fcad"),
    };
    let context = OperationContext::default();
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let error = edit_copy(&request, &mut kernel, &context).expect_err("fillet no longer fits");
    assert!(error.to_string().contains("Fillet1"), "{error}");
    let refusal = ferritecad_eval::RebuildRefusal::from_error(&error).expect("full refusal");
    assert!(
        refusal.report.failed.iter().any(|f| f.id == chain.first),
        "{error}"
    );
    assert!(!refusal.lost_references.is_empty());
    assert!(!request.destination.exists());
    assert_eq!(std::fs::read(&source).expect("unchanged"), before);
    request.changes.push(
        EditChange::constant(chain.first, ExpressionSlot::FilletRadius, 2.0).expect("radius"),
    );
    let result = edit_copy(&request, &mut kernel, &context).expect("one transaction repairs both");
    assert!(result.rebuild.all_built());
    let reopened = Document::open_read_only(&request.destination).expect("reopen");
    assert_eq!(
        reopened.content_version().expect("version"),
        result.version.content
    );
    for change in &request.changes {
        let EditChange::SetExpression {
            object,
            slot,
            expression,
        } = change
        else {
            panic!("numeric transaction")
        };
        let slots = reopened.expression_slots().expect("slots");
        let saved = slots
            .iter()
            .find(|s| s.object == *object && s.slot == *slot)
            .expect("saved slot");
        assert_eq!(&saved.expression, expression);
    }
    let built =
        ferritecad_eval::rebuild_cold(&reopened, &mut kernel, &context).expect("cold rebuild");
    let volume = kernel
        .shape_stats(built.shape(chain.body).expect("body"))
        .expect("volume")
        .1;
    built.release_all(&mut kernel);
    let mut session = ferritecad_jobs::DocumentSession::open(&source).expect("session");
    session
        .current()
        .load_scene(
            &mut kernel,
            |k, b| k.import_step(b),
            &Default::default(),
            &context,
        )
        .expect("baseline");
    let mut step = session
        .begin_step()
        .edit(
            request.changes,
            session.current().version(),
            &mut kernel,
            |k, b| k.import_step(b),
            &context,
        )
        .expect("session transaction");
    assert!(step.take_scene().is_some());
    session.commit_step(step).expect("commit");
    let document = Document::open_read_only(session.current().path()).expect("accepted version");
    let built =
        ferritecad_eval::rebuild_cold(&document, &mut kernel, &context).expect("accepted rebuild");
    let session_volume = kernel
        .shape_stats(built.shape(chain.body).expect("body"))
        .expect("volume")
        .1;
    built.release_all(&mut kernel);
    assert!((session_volume - volume).abs() < 1e-7);
    let undo = session.begin_undo().expect("Undo");
    session.commit_move(undo).expect("commit Undo");
    assert_eq!(session.current().version(), reading.version);
    let redo = session.begin_redo().expect("Redo");
    session.commit_move(redo).expect("commit Redo");
}
