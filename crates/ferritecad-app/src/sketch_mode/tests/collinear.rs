// SPDX-License-Identifier: MIT
use super::constraint_widgets::press_rule;
use super::widgets::Ui;
use super::window::{apply, click, enter, finish, open};
use super::*;
use crate::feature_tests::U_PROFILE;
use crate::sessions::tests::tabs::{Drawn, Window};
use ferritecad_ui::ModelEdit;
use std::path::Path;

/// Close the eight joints and constrain the two rims in one viewport publication.
pub(crate) fn constrain_u(path: &Path, sketch: ObjectId) -> Window {
    let mut w = open(path, Drawn::Native);
    w.input
        .frame(([-10., -10., -20.], [60., 32., 20.]))
        .expect("U in view");
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut controls = Ui::new(&mut w);
    // End of each line, then start of its neighbour, including the closing pair.
    for i in 0..8 {
        let [x, y] = U_PROFILE[(i + 1) % 8];
        click(&mut w, point(x, y), false);
        click(&mut w, point(x, y), true);
        if i == 7 {
            // The first line's start is picked before the last line's end at this corner.
            click(&mut w, point(x, y), true);
            click(&mut w, point(x, y), true);
        }
        press_rule(&mut controls, &mut w, "Coincident");
    }
    for (i, shift) in [(0, false), (4, true)] {
        let [x, y] = U_PROFILE[i];
        let [to_x, to_y] = U_PROFILE[i + 1];
        click(&mut w, point((x + to_x) / 2., (y + to_y) / 2.), shift);
    }
    press_rule(&mut controls, &mut w, "Collinear");
    let request = finish(&mut w, &mut controls);
    assert_eq!(request.changes.len(), 9);
    assert!(
        request
            .changes
            .iter()
            .all(|c| matches!(c, EditChange::AddConstraint { .. }))
    );
    let draft = w.creates.sketch.viewport.draft.as_ref().expect("draft");
    assert_eq!(
        draft
            .state
            .report
            .as_ref()
            .expect("solve")
            .degrees_of_freedom(),
        14
    );
    assert!(apply(&mut w, request));
    w
}
