// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_document::{EditChange, ExpressionSlot};

#[test]
fn native_open_draws_and_saves_a_parameter_driven_extrusion() {
    if !native() {
        return;
    }
    let f = fixture();
    let parameter = ObjectId::new();
    let mut doc = Document::open(&f.file).expect("plate");
    doc.write_changes(&[
        EditChange::AddParameter {
            object: parameter,
            name: "stock".into(),
            dimension: ferritecad_types::Dimension::Length,
            expression: "2 cm".into(),
        },
        EditChange::SetExpressionText {
            object: f.feature,
            slot: ExpressionSlot::ExtrudeDistance,
            source: "stock + 3 mm".into(),
        },
    ])
    .expect("driven extrusion");
    doc.close().expect("close");
    let (scene, session) = open_for_view(f.private.path(), &f.file, &OperationContext::default())
        .expect("open through the window");
    assert!(scene.rebuild.all_built());
    let reading = crate::sketch_facts::Source::from(scene.edit_source.expect("form facts"));
    let slot = reading
        .expression_slot(f.feature, ExpressionSlot::ExtrudeDistance)
        .expect("height");
    assert_eq!(
        slot.refusal.as_deref(),
        Some("This value is an expression; edit its text in Parameters → Values.")
    );
    assert!(!crate::edits::Edits::default().begin_selected(&f.file, &reading, f.feature));
    let snapshot = Arc::new(scene.snapshot);
    let bounds = snapshot.bounds().expect("visible body");
    assert!((bounds.1[2] - 23.).abs() < 1e-5);
    let mut renderer = match ferritecad_viewport_gpu::Renderer::new() {
        Ok(r) => r,
        Err(e) if e.kind() == ferritecad_types::ErrorKind::Unsupported => {
            ferritecad_fixtures::native_dependencies(&[(false, "GPU")]);
            return;
        }
        Err(e) => panic!("renderer: {e}"),
    };
    let pick = snapshot.pick_of(0).expect("plate pick");
    let prepared = renderer
        .prepare(Arc::clone(&snapshot))
        .expect("upload plate");
    let mut view = crate::ViewportInput::new();
    view.resize(160, 160);
    view.frame(bounds).expect("frame plate");
    let pixels = renderer
        .render(
            &prepared,
            view.camera(),
            crate::Marked::Nothing,
            crate::Hovered::Nothing,
            &crate::Visibility::new(&snapshot),
        )
        .expect("draw plate");
    assert!(
        (0..pixels.height()).any(|y| (0..pixels.width()).any(|x| pixels.pick_at(x, y) == pick))
    );
    let saved = f.root.path().join("saved.fcad");
    let mut sessions = Sessions::default();
    sessions.adopt(session);
    super::fillet::saved(&mut sessions, SaveTarget::As(saved.clone()));
    let reopened = Document::open_read_only(&saved).expect("open saved parameter graph");
    let height = reopened
        .expression_slots()
        .expect("slots")
        .into_iter()
        .find(|s| s.object == f.feature && s.slot == ExpressionSlot::ExtrudeDistance)
        .expect("height");
    assert_eq!(height.expression.source, "stock + 3 mm");
    assert_eq!(height.expression.value(), 23.);
    assert!(
        reopened
            .dependencies()
            .expect("graph")
            .iter()
            .any(|edge| edge.dependent == f.feature
                && edge.dependency == parameter
                && edge.role == ferritecad_document::DependencyRole::Parameter)
    );
}
