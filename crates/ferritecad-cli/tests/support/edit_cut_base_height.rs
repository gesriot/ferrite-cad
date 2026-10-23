// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_document::ExpressionSlot;

#[test]
fn native_height_transaction_preserves_tools_references_and_cold_geometry() {
    if !native() {
        return;
    }
    for n in [1, 4, 16] {
        let root = tempfile::tempdir().expect("root");
        let tools = history::tools(n);
        let source = history::fixture(root.path(), &tools);
        let original = std::fs::read(&source).expect("source");
        let d = Document::open_read_only(&source).expect("document");
        let base = d.objects().expect("objects").into_iter().find(|o|matches!(&o.payload,
            ferritecad_document::ObjectPayload::Extrude(e) if e.operation == ferritecad_document::SolidOperation::NewBody && e.previous.is_none())).expect("base").id;
        let refs = d.topology_refs().expect("refs");
        let request = root.path().join("transaction.json");
        std::fs::write(&request,serde_json::to_vec(&json!({"request_version":1,"changes":[{"object":base,"slot":ExpressionSlot::ExtrudeDistance,"value":14.0,"unit":"mm"}]})).expect("JSON")).expect("request");
        let output = root.path().join("height.fcad");
        reply(
            cli()
                .arg("edit")
                .arg(&source)
                .arg("--expect-version")
                .arg(d.content_version().expect("version").to_string())
                .arg("--request")
                .arg(&request)
                .arg("--output")
                .arg(&output)
                .arg("--json")
                .output()
                .expect("edit"),
            "edit",
            0,
        );
        let edited = Document::open_read_only(&output).expect("copy");
        let after_refs = edited.topology_refs().expect("refs");
        assert!(
            refs.iter().all(|r| after_refs.contains(r)),
            "existing references survive"
        );
        assert_eq!(
            edited.dependencies().expect("deps"),
            d.dependencies().expect("deps")
        );
        for old in d.objects().expect("objects") {
            let now = edited
                .object(old.id)
                .expect("object")
                .expect("same identity");
            if old.id != base {
                assert_eq!(old, now);
            }
        }
        // New pocket floors must remain named so structural edits can follow.
        let catalog = ferritecad_document::ExtrudeEditSource::read(&edited).expect("reading");
        for choice in &catalog.cut_features {
            if choice.feature != base {
                assert!(
                    choice.saved.is_some(),
                    "Cut history must remain editable: {:?}",
                    choice.refusal
                );
            }
        }
        super::measure_history_at(&output, &tools, [[0., 0.], [SIZE[0], SIZE[1]]], 14.0, None);
        assert_eq!(std::fs::read(&source).expect("unchanged"), original);
    }
}

#[test]
fn native_depth_and_combined_transactions_mint_floors_and_refuse_every_lost_name() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    use ferritecad_document::{CapSide, ExpressionChange, SemanticRole};
    for (height, depth, change_height, change_depth) in [
        (14.0, 12.0, true, false),
        (12.0, 10.0, false, true),
        (14.0, 13.0, true, true),
    ] {
        let root = tempfile::tempdir().expect("root");
        let mut tools = history::tools(4);
        let source = history::fixture(root.path(), &tools);
        let before = std::fs::read(&source).expect("source");
        let saved = history::ordered(&source);
        let first = &saved[0];
        let mut changes = Vec::new();
        if change_height {
            changes.push(
                ExpressionChange::constant(
                    first.base_feature,
                    ExpressionSlot::ExtrudeDistance,
                    height,
                )
                .expect("height"),
            );
        }
        if change_depth {
            changes.push(
                ExpressionChange::constant(first.feature, ExpressionSlot::ExtrudeDistance, depth)
                    .expect("depth"),
            );
        }
        let reading = ferritecad_jobs::read_extrude_source(&source).expect("source");
        let request = root.path().join("changes.json");
        write(
            &request,
            &json!({"request_version":1,"changes":changes.iter().map(|c|json!({"object":c.object,"slot":c.slot,"value":c.expression.value(),"unit":"mm"})).collect::<Vec<_>>()}),
        );
        let output = root.path().join("pocket.fcad");
        reply(
            crate::transaction::command(
                &source,
                &reading.version.content.to_string(),
                &request,
                &output,
            )
            .output()
            .expect("CLI"),
            "edit",
            0,
        );
        let edited = Document::open_read_only(&output).expect("reopen");
        let all_refs = edited.topology_refs().expect("names");
        let floor_names: Vec<_> = all_refs
            .iter()
            .filter(|r| match r.output_role {
                SemanticRole::ExtrudeCap { side: CapSide::End } => {
                    r.producer_feature == first.feature
                }
                SemanticRole::OriginCap {
                    origin_feature,
                    side: CapSide::End,
                } => origin_feature == first.feature,
                _ => false,
            })
            .map(|r| r.id)
            .collect();
        assert_eq!(
            floor_names.len(),
            saved.len(),
            "own floor and every later origin"
        );
        tools[0].extent = CutExtent::Blind { depth_mm: depth };
        let cold_volume = super::measure_history_at(
            &output,
            &tools,
            [[0., 0.], [SIZE[0], SIZE[1]]],
            height,
            None,
        );
        let following = root.path().join("following.fcad");
        let current = history::ordered(&output);
        let mut tool = tools[0];
        tool.radius_mm += 0.1;
        super::edits::edit(&output, &following, &current[0], tool, 0);
        tools[0] = tool;
        super::measure_history_at(
            &following,
            &tools,
            [[0., 0.], [SIZE[0], SIZE[1]]],
            height,
            None,
        );

        let context = OperationContext::default();
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
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
        let step = session
            .begin_step()
            .edit(
                changes,
                reading.version,
                &mut kernel,
                |k, b| k.import_step(b),
                &context,
            )
            .expect("session edit");
        session.commit_step(step).expect("commit");
        let reopened =
            Document::open_read_only(session.current().path()).expect("session snapshot");
        let built = ferritecad_eval::rebuild_cold(&reopened, &mut kernel, &context).expect("cold");
        let volume = kernel
            .shape_stats(built.shape(first.body).expect("body"))
            .expect("stats")
            .1;
        assert!((volume - cold_volume).abs() < 1e-7);
        built.release_all(&mut kernel);
        let snapshot = std::fs::read(session.current().path()).expect("snapshot");
        let session_refs = reopened.topology_refs().expect("refs");
        let session_floor_names: Vec<_> = session_refs
            .iter()
            .filter(|r| match r.output_role {
                SemanticRole::ExtrudeCap { side: CapSide::End } => {
                    r.producer_feature == first.feature
                }
                SemanticRole::OriginCap {
                    origin_feature,
                    side: CapSide::End,
                } => origin_feature == first.feature,
                _ => false,
            })
            .map(|r| r.id)
            .collect();
        let losing =
            ExpressionChange::constant(first.feature, ExpressionSlot::ExtrudeDistance, height)
                .expect("through");
        let error = session
            .begin_step()
            .edit(
                vec![losing],
                session.current().version(),
                &mut kernel,
                |k, b| k.import_step(b),
                &context,
            )
            .expect_err("cannot lose names");
        let refusal = ferritecad_eval::RebuildRefusal::from_error(&error).expect("generic gate");
        for id in session_floor_names {
            assert!(refusal.lost_references.contains_key(&id), "{error}");
        }
        assert_eq!(
            std::fs::read(session.current().path()).expect("unchanged"),
            snapshot
        );

        write(
            &request,
            &json!({"request_version":1,"changes":[{"object":first.feature,"slot":ExpressionSlot::ExtrudeDistance,"value":height,"unit":"mm"}]}),
        );
        let rejected = root.path().join("rejected.fcad");
        let bytes = std::fs::read(&output).expect("copy bytes");
        let failed = reply(
            crate::transaction::command(
                &output,
                &edited.content_version().expect("version").to_string(),
                &request,
                &rejected,
            )
            .output()
            .expect("CLI refusal"),
            "edit",
            2,
        );
        for id in floor_names {
            assert!(failed.to_string().contains(&id.to_string()), "{failed}");
        }
        assert!(!rejected.exists());
        assert_eq!(std::fs::read(&output).expect("unchanged"), bytes);
        assert_eq!(std::fs::read(&source).expect("unchanged source"), before);
    }
}
