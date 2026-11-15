// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_kernel::{OperationContext, TessellationParams, mock::MockKernel};
use ferritecad_types::{ErrorKind, Result};
use ferritecad_viewport::{Hovered, Marked, PixelRect, Visibility};
use ferritecad_viewport_gpu::{Frame, PreparedSnapshot, Renderer};
use std::sync::Arc;

fn render(renderer: &mut Renderer, prepared: &PreparedSnapshot, camera: &Camera) -> Frame {
    renderer
        .render(
            prepared,
            camera,
            Marked::Nothing,
            Hovered::Nothing,
            &Visibility::new(prepared.snapshot()),
        )
        .expect("frame")
}

#[test]
fn gpu_mode_hides_only_its_saved_drawing_and_uploads_once_per_transition() -> Result<()> {
    let mut renderer = match Renderer::new() {
        Ok(renderer) => renderer,
        Err(error) if error.kind() == ErrorKind::Unsupported => {
            ferritecad_fixtures::native_dependencies(&[(false, "GPU")]);
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let mut f = fixtures::Drawing::new(vec![line([-10., -5.], [10., -5.])], false);
    let other = ObjectId::new();
    f.doc.write_changes(&[EditChange::AddSketch {
        object: other,
        name: Some("Other".into()),
        sketch: Sketch {
            plane: f.plane,
            curves: vec![line([-10., 5.], [10., 5.])],
            constraints: vec![],
        },
    }])?;
    let loaded = ferritecad_scene::snapshot_of(
        &f.path,
        &mut MockKernel::new(),
        |_, _| unreachable!("no STEP"),
        &TessellationParams::default(),
        &OperationContext::default(),
    )?;
    let source = Source::from(loaded.edit_source.expect("source"));
    let mut accepted = Accepted::new(Some(source.version), loaded.sketch_presentations);
    let snapshot = Arc::new(loaded.snapshot);
    let mut prepared = renderer.prepare(Arc::clone(&snapshot))?;
    renderer.replace_sketches(&mut prepared, &accepted.drawings_except(None)?)?;
    let camera = camera(SketchPlane::world_xy(), Projection::Orthographic);
    let full = render(&mut renderer, &prepared, &camera);
    let blank = renderer.prepare(Arc::clone(&snapshot))?;
    let blank = render(&mut renderer, &blank, &camera);
    assert_ne!(full.colour(), blank.colour());
    let uploads = renderer.sketch_uploads();
    let geometry = renderer.geometry_uploads();
    let mut editor = Editor::default();
    assert!(editor.begin_with(&f.path, &source, &accepted, f.sketch));
    sync(&mut renderer, &mut prepared, &mut accepted, &editor)?;
    assert_eq!(renderer.sketch_uploads(), uploads + 1);
    let hidden = render(&mut renderer, &prepared, &camera);
    assert_ne!(
        hidden.colour(),
        full.colour(),
        "saved edited drawing must disappear"
    );
    assert_ne!(hidden.colour(), blank.colour(), "the other drawing remains");
    let expected = accepted.presentation(&source, other).expect("other");
    let expected = ferritecad_scene::sketch_drawing(expected)?;
    assert_eq!(accepted.drawings_except(Some(f.sketch))?, vec![expected]);
    let ctx = egui::Context::default();
    for i in 0..8 {
        let d = editor.draft.as_mut().expect("draft");
        d.tool = Tool::Point;
        d.click(point(i as f64, 1.));
        sync(&mut renderer, &mut prepared, &mut accepted, &editor)?;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800., 600.),
                )),
                ..Default::default()
            },
            |ui| editor.paint(ui.ctx(), &camera, PixelRect::sized(800, 600)),
        );
        let circles = output
            .shapes
            .iter()
            .filter(|s| matches!(s.shape, egui::Shape::Circle(_)))
            .count();
        assert_eq!(
            circles,
            (i + 1) as usize,
            "draft edits appear in the overlay"
        );
        assert!(
            output
                .shapes
                .iter()
                .all(|s| s.clip_rect.min.x >= 0. && s.clip_rect.max.x <= 800.)
        );
        output.textures_delta.clear();
        assert_eq!(
            render(&mut renderer, &prepared, &camera).colour(),
            hidden.colour()
        );
        assert_eq!(
            renderer.sketch_uploads(),
            uploads + 1,
            "draft frames must not upload sketches"
        );
        assert_eq!(
            renderer.geometry_uploads(),
            geometry,
            "draft frames must not upload bodies"
        );
    }
    editor = Editor::default();
    sync(&mut renderer, &mut prepared, &mut accepted, &editor)?;
    assert_eq!(renderer.sketch_uploads(), uploads + 2);
    assert_eq!(
        render(&mut renderer, &prepared, &camera).colour(),
        full.colour()
    );
    sync(&mut renderer, &mut prepared, &mut accepted, &editor)?;
    assert_eq!(renderer.sketch_uploads(), uploads + 2);
    Ok(())
}
