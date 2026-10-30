// SPDX-License-Identifier: MIT
use ferritecad_document::*;
use ferritecad_jobs::{DocumentSession, EditRequest, edit_copy};
use ferritecad_kernel::{OperationContext, TessellationParams};
use ferritecad_occt::OcctKernel;
use ferritecad_types::{ContentHash, ObjectId};

#[test]
fn end_changes_offset_undo_and_redo_share_atomic_publication_and_cycle_refusal() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let mut doc = ferritecad_fixtures::open_plate(root.path()).expect("plate");
    let mut kernel = OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let f = ferritecad_fixtures::up_to_end::ceiling(&mut doc, &mut kernel).expect("ceiling");
    let path = doc.path().to_owned();
    doc.close().expect("close");
    let original = std::fs::read(&path).expect("original");
    let mut session = DocumentSession::open(&path).expect("session");
    session
        .current()
        .load_scene(
            &mut kernel,
            |_, _| unreachable!("no imports"),
            &TessellationParams::default(),
            &context,
        )
        .expect("baseline scene");
    for end_condition in [
        f.face_end(0.).expect("face"),
        f.vertex_end(),
        EndCondition::Blind {
            distance: Expression::constant(20.).expect("height"),
        },
    ] {
        let version = session.current().version();
        let changes = vec![EditChange::SetEndCondition {
            object: f.boss.feature,
            end_condition,
        }];
        let step = session
            .begin_step()
            .edit(
                changes,
                version,
                &mut kernel,
                |_, _| unreachable!("no imports"),
                &context,
            )
            .expect("change end");
        session.commit_step(step).expect("commit");
        let after = Document::open_read_only(session.current().path())
            .expect("after")
            .content_version()
            .expect("content");
        session
            .commit_move(session.begin_undo().expect("undo"))
            .expect("commit undo");
        assert_ne!(
            Document::open_read_only(session.current().path())
                .expect("undo")
                .content_version()
                .expect("content"),
            after
        );
        session
            .commit_move(session.begin_redo().expect("redo"))
            .expect("commit redo");
        assert_eq!(
            Document::open_read_only(session.current().path())
                .expect("redo")
                .content_version()
                .expect("content"),
            after
        );
    }
    let step = session
        .begin_step()
        .edit(
            vec![
                EditChange::SetEndCondition {
                    object: f.boss.feature,
                    end_condition: f.face_end(0.).expect("face"),
                },
                EditChange::constant(f.boss.feature, ExpressionSlot::ExtrudeEndOffset, 2.)
                    .expect("offset"),
            ],
            session.current().version(),
            &mut kernel,
            |_, _| unreachable!("no imports"),
            &context,
        )
        .expect("offset edit");
    session.commit_step(step).expect("offset commit");
    let source = session.current().path().to_owned();
    let before = std::fs::read(&source).expect("source bytes");
    for (index, (change, diagnostic)) in [
        (
            EditChange::SetEndCondition {
                object: f.boss.base,
                end_condition: f.face_end(0.).expect("dependent ceiling"),
            },
            "cycle",
        ),
        (
            EditChange::constant(f.boss.feature, ExpressionSlot::ExtrudeEndOffset, 100.)
                .expect("surface behind"),
            "does not cover",
        ),
        (
            EditChange::SetEndCondition {
                object: f.boss.feature,
                end_condition: EndCondition::UpToFace {
                    result: f.result,
                    face: EntityReference {
                        meaning: ReferenceMeaning {
                            producer: ObjectId::new(),
                            ..f.face.meaning
                        },
                        ..f.face
                    },
                    offset_mm: Expression::constant(0.).expect("zero"),
                },
            },
            "not in the consumed result's history",
        ),
        (
            EditChange::SetEndCondition {
                object: f.boss.feature,
                end_condition: EndCondition::UpToFace {
                    result: f.result,
                    face: EntityReference {
                        meaning: ReferenceMeaning {
                            digest: ContentHash::of_bytes(b"absent"),
                            ..f.face.meaning
                        },
                        ..f.face
                    },
                    offset_mm: Expression::constant(0.).expect("zero"),
                },
            },
            "unknown name",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let output = root.path().join(format!("refused-{index}.fcad"));
        let request = EditRequest {
            source: source.clone(),
            destination: output.clone(),
            expected: session.current().version(),
            changes: vec![change],
        };
        let error = edit_copy(&request, &mut kernel, &context).expect_err("atomic refusal");
        let full = format!("{error}: {:?}", std::error::Error::source(&error));
        assert!(full.contains(diagnostic), "{full}");
        assert_eq!(std::fs::read(&source).expect("source unchanged"), before);
        assert!(!output.exists());
    }
    assert_eq!(std::fs::read(path).expect("saved source"), original);
}

#[test]
fn deleting_a_consumed_pocket_floor_refuses_without_publishing() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let mut doc = ferritecad_fixtures::open_plate(root.path()).expect("plate");
    let mut kernel = OcctKernel::new().expect("kernel");
    let f = ferritecad_fixtures::up_to_end::pockets(&mut doc, &mut kernel).expect("pockets");
    let source = doc.path().to_owned();
    let expected = DocumentVersion {
        document_id: doc.meta().document_id,
        content: doc.content_version().expect("version"),
    };
    doc.close().expect("close");
    let before = std::fs::read(&source).expect("bytes");
    let destination = root.path().join("refused.fcad");
    let error = edit_copy(
        &EditRequest {
            source: source.clone(),
            destination: destination.clone(),
            expected,
            changes: vec![EditChange::SetEndCondition {
                object: f.first,
                end_condition: EndCondition::ThroughAll,
            }],
        },
        &mut kernel,
        &OperationContext::default(),
    )
    .expect_err("deleted meaning");
    let text = error.to_string();
    assert!(text.contains("face deleted"), "{text}");
    assert!(text.contains(&f.second.to_string()));
    assert!(text.contains(&f.face.id.to_string()));
    assert_eq!(std::fs::read(source).expect("unchanged"), before);
    assert!(!destination.exists());
}
