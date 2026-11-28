// SPDX-License-Identifier: MIT
//! Widget drivers used by legacy session fixtures.
use super::*;

pub(crate) fn replace_segment_length(
    ctx: &egui::Context,
    e: &mut Editor,
    segment: &str,
    length: &str,
) {
    for _ in 0..3 {
        frame(ctx, e, vec![]);
    }
    click(ctx, e, segment);
    enter_length(ctx, e, length, false);
    click(ctx, e, "Replace length");
}

fn pressed(ctx: &egui::Context, mut e: Editor, unsaved: bool) -> (Editor, EditRequest) {
    e.set_session(true, true, unsaved);
    click(ctx, &mut e, "Apply constraints");
    let request = e.take_apply_request().expect("Apply constraints asked");
    (e, request)
}

fn opened(path: &Path, source: &ExtrudeEditSource, sketch: ObjectId) -> (Editor, egui::Context) {
    let mut e = Editor::default();
    e.set_session(true, true, true);
    assert!(e.begin(path, source, sketch));
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, &mut e, vec![]);
    }
    (e, ctx)
}

/// The stored length of Segment `segment` replaced by `length`, through the widgets.
pub(crate) fn typed_replace_length(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    segment: usize,
    length: &str,
) -> (Editor, EditRequest) {
    let (mut e, ctx) = opened(path, source, sketch);
    replace_segment_length(&ctx, &mut e, &format!("Segment {segment}"), length);
    pressed(&ctx, e, true)
}

/// A Radius and a Fixed centre on the profile's one circle, through the widgets.
pub(crate) fn typed_circle(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    radius: &str,
    centre: (&str, &str),
) -> (Editor, EditRequest) {
    let (mut e, ctx) = opened(path, source, sketch);
    let curve = source.constraint_sketches[0]
        .stored
        .as_ref()
        .expect("stored")
        .curves[0]
        .id;
    click(&ctx, &mut e, &format!("Circle · {}", short(curve)));
    enter_field(&ctx, &mut e, "Radius mm", radius, false);
    click(&ctx, &mut e, "Add radius");
    enter_field(&ctx, &mut e, "Fixed X (mm):", centre.0, false);
    enter_field(&ctx, &mut e, "Fixed Y (mm):", centre.1, false);
    click(&ctx, &mut e, "Add Fixed centre");
    assert_eq!(e.draft.as_ref().expect("draft").refusal, None);
    pressed(&ctx, e, true)
}

/// Two Lines forced parallel, through the widgets: the form cannot know whether
/// the solver will agree.
pub(crate) fn typed_parallel(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    a: usize,
    b: usize,
) -> (Editor, EditRequest) {
    let (mut e, ctx) = opened(path, source, sketch);
    click(&ctx, &mut e, &format!("Segment {a}"));
    click(&ctx, &mut e, "Pair Line A");
    click(&ctx, &mut e, &format!("Segment {b}"));
    click(&ctx, &mut e, "Pair Line B");
    click(&ctx, &mut e, "Add Parallel");
    assert_eq!(e.draft.as_ref().expect("draft").refusal, None);
    pressed(&ctx, e, true)
}

/// The draft of `typed_replace_length` without pressing Apply: what the form says.
pub(crate) fn replace_length_offers_apply(
    path: &Path,
    source: &ExtrudeEditSource,
    sketch: ObjectId,
    segment: usize,
    length: &str,
) -> (Editor, bool) {
    let (mut e, ctx) = opened(path, source, sketch);
    replace_segment_length(&ctx, &mut e, &format!("Segment {segment}"), length);
    let out = frame(&ctx, &mut e, vec![]);
    let offered = painted(&out, "Apply constraints");
    (e, offered)
}

/// §30P: one press of `label` on a form the window holds, through the widgets.
pub(crate) fn press(e: &mut Editor, label: &str) {
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, e, vec![]);
    }
    click(&ctx, e, label);
}
