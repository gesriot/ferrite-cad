// SPDX-License-Identifier: MIT
use super::*;
use crate::curve_fixtures::*;
use ferritecad_kernel::{OperationContext, TessellationParams, mock::MockKernel};

fn opened(f: &Drawing) -> ferritecad_scene::LoadedScene {
    let mut kernel = MockKernel::new();
    let context = OperationContext::default();
    let built = ferritecad_eval::rebuild_cold(&f.doc, &mut kernel, &context).expect("rebuild");
    ferritecad_scene::snapshot_from_rebuild(
        &f.doc,
        &built,
        &mut kernel,
        |_, _| unreachable!("no imports"),
        &TessellationParams::default(),
        &context,
    )
    .expect("open drawing")
}
fn check_drawing(curves: Vec<ferritecad_document::SketchCurve>) {
    let f = Drawing::new(curves.clone(), false);
    let loaded = opened(&f);
    assert!(loaded.rebuild.all_built());
    let mut source = crate::sketch_facts::Source::from(loaded.edit_source.expect("source"));
    source.built_sketches = loaded
        .sketch_presentations
        .iter()
        .map(|p| p.sketch())
        .collect();
    let rows: Vec<_> = source
        .model_tree
        .rows
        .iter()
        .filter(|r| r.id.0 == f.sketch)
        .collect();
    assert!(!rows.is_empty());
    for row in rows {
        assert_eq!(row.name, "Drawing");
        assert_eq!(row.note, "Stored object; references do not imply ownership");
        assert_eq!(
            actions(&source, row.id),
            vec![ModelEdit::Sketch, ModelEdit::Constraints]
        );
        assert_eq!(
            Command::Delete.available(&source, row.id, Ok(Access::Idle)),
            Ok(())
        );
    }
    let presentation = loaded
        .sketch_presentations
        .iter()
        .find(|s| s.sketch() == f.sketch)
        .expect("drawing presentation");
    let drawing = ferritecad_scene::sketch_drawing(presentation).expect("world drawing");
    assert_eq!(drawing.strokes().len(), curves.len());
    for (stroke, curve) in drawing.strokes().iter().zip(&curves) {
        let ferritecad_document::SketchGeometry::Line { start, end } = curve.geometry else {
            unreachable!("line fixture")
        };
        assert_eq!(
            stroke.points(),
            &[
                [start.x as f32, start.y as f32, 0.],
                [end.x as f32, end.y as f32, 0.]
            ]
        );
    }
    let accepted =
        crate::sketch::accepted::Accepted::new(Some(source.version), loaded.sketch_presentations);
    let mut editor = crate::constraints::Editor::default();
    assert!(editor.begin_with(&f.path, &source, &accepted, f.sketch));
    assert!(editor.active());
    let ctx = egui::Context::default();
    let frame = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(988., 768.),
        )),
        ..Default::default()
    };
    // Let the form settle its initial size before reading the painted list.
    let mut initial = ctx.run_ui(frame(), |ui| editor.draw(ui, false));
    initial.textures_delta.clear();
    let mut out = ctx.run_ui(frame(), |ui| editor.draw(ui, false));
    out.textures_delta.clear();
    let shown: Vec<_> = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text().starts_with("Segment ") => {
                Some(t.galley.text().to_owned())
            }
            _ => None,
        })
        .collect();
    let expected: Vec<_> = curves
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let ferritecad_document::SketchGeometry::Line { start, end } = c.geometry else {
                unreachable!("line fixture")
            };
            format!(
                "Segment {} · {} · ({}, {}) → ({}, {}) mm",
                i + 1,
                crate::constraints::short(c.id),
                start.x,
                start.y,
                end.x,
                end.y
            )
        })
        .collect();
    assert_eq!(shown, expected);
    assert!(editor.active());
}
#[test]
fn empty_drawing_opens_constraints_with_its_exact_curve_list_and_keeps_delete() {
    check_drawing(vec![]);
}
#[test]
fn open_drawing_opens_constraints_with_its_exact_curve_list_and_keeps_delete() {
    check_drawing(vec![line([0., 0.], [10., 0.])]);
}
#[test]
fn scrambled_drawing_opens_constraints_with_its_exact_curve_list_and_keeps_delete() {
    check_drawing(variants(&rectangle()).remove(5));
}
#[test]
fn repair_document_attributes_missing_profile_to_the_consumer_and_body() {
    let curves = vec![line([0., 0.], [10., 0.])];
    let reason = format!(
        "invalid input: the profile does not close: curve {} has a free end at (10, 0)",
        curves[0].id
    );
    let f = Drawing::new(curves, true);
    let loaded = opened(&f);
    let source = loaded.edit_source.expect("source");
    let original = ferritecad_document::ModelTree::from_dependencies(
        &f.doc.objects().expect("objects"),
        f.doc.dependencies(),
    );
    for row in source.model_tree.rows.iter().filter(|r| r.id.0 == f.sketch) {
        let unchanged = original
            .rows
            .iter()
            .find(|r| r.id == row.id)
            .expect("original row");
        assert_eq!(row.name, unchanged.name);
        assert_eq!(row.note, unchanged.note);
    }

    for (id, name, note) in [
        (
            f.feature,
            "⚠ Extrude",
            format!(
                "Failed: invalid input: sketch {} gives no profile: {reason}",
                f.sketch
            ),
        ),
        (
            f.body,
            "⚠ Part",
            "Rolled back to no built feature because of Extrude".into(),
        ),
    ] {
        let rows: Vec<_> = source
            .model_tree
            .rows
            .iter()
            .filter(|r| r.id.0 == id)
            .collect();
        assert!(!rows.is_empty());
        for row in rows {
            assert_eq!(row.name, name);
            assert_eq!(row.note, note);
        }
    }
}
