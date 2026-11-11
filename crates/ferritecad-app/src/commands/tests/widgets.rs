// SPDX-License-Identifier: MIT
use super::*;

fn clicks(at: egui::Pos2, button: egui::PointerButton) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(at),
        egui::Event::PointerButton {
            pos: at,
            button,
            pressed: true,
            modifiers: Default::default(),
        },
        egui::Event::PointerButton {
            pos: at,
            button,
            pressed: false,
            modifiers: Default::default(),
        },
    ]
}

fn rect(output: &egui::FullOutput, label: &str, last: bool) -> egui::Rect {
    let rows: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == label => {
                Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
            }
            _ => None,
        })
        .collect();
    *if last { rows.last() } else { rows.first() }
        .unwrap_or_else(|| panic!("missing widget {label}"))
}

#[allow(
    clippy::too_many_arguments,
    reason = "exercise both command surfaces with identical input"
)]
fn frame(
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    source: &ExtrudeEditSource,
    row: ModelKey,
    navigation: &mut ferritecad_ui::ModelNavigation,
    entity: bool,
    idle: std::result::Result<(), &str>,
) -> (egui::FullOutput, Option<Action>) {
    let mut answer = None;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900., 900.),
        )),
        events,
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| {
        if entity {
            answer = Menu {
                tab: 7,
                epoch: navigation.epoch,
                row,
                position: egui::pos2(20., 20.),
            }
            .draw(ui.ctx(), source, idle)
            .0;
        } else {
            egui::CentralPanel::default().show(ui, |ui| {
                answer = ferritecad_ui::model_panel(
                    ui,
                    7,
                    &[ferritecad_ui::ModelRow {
                        key: row,
                        parent: None,
                        depth: 0,
                        name: "Selected Cut",
                        kind: "Cut",
                        note: "built",
                        solid_count: Some(1),
                    }],
                    navigation,
                    |key| offers(source, key, idle),
                );
            });
        }
    });
    output.textures_delta.clear();
    (output, answer)
}

#[test]
fn real_tree_buttons_context_menus_and_shortcuts_share_the_command_catalogue() {
    let (_root, _path, source) = crate::cuts::tests::session_apply::fixture(1);
    let saved = crate::cuts::tests::session_apply::selected(&source, 0);
    let row = (saved.feature, Some(saved.body), None);
    let expected = Action {
        tab: 7,
        epoch: 0,
        row,
        edit: Command::Delete,
    };
    for entity in [false, true] {
        let ctx = egui::Context::default();
        let mut navigation = ferritecad_ui::ModelNavigation {
            selected: Some(row),
            ..Default::default()
        };
        let mut output = frame(&ctx, vec![], &source, row, &mut navigation, entity, Ok(())).0;
        for _ in 0..3 {
            output = frame(&ctx, vec![], &source, row, &mut navigation, entity, Ok(())).0;
        }
        for offer in offers(&source, row, Ok(())) {
            assert!(rect(&output, offer.label, false).is_positive());
        }
        let at = rect(&output, "Delete", false).center();
        assert_eq!(
            frame(
                &ctx,
                clicks(at, egui::PointerButton::Primary),
                &source,
                row,
                &mut navigation,
                entity,
                Ok(())
            )
            .1,
            Some(expected)
        );
        let refusal = Err("Finish the current operation, New, dialog or gesture before editing.");
        frame(&ctx, vec![], &source, row, &mut navigation, entity, refusal);
        assert_eq!(
            frame(
                &ctx,
                clicks(at, egui::PointerButton::Primary),
                &source,
                row,
                &mut navigation,
                entity,
                refusal
            )
            .1,
            None
        );
        if !entity {
            let key = egui::Event::Key {
                key: egui::Key::Delete,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            };
            assert_eq!(
                frame(
                    &ctx,
                    vec![key.clone()],
                    &source,
                    row,
                    &mut navigation,
                    false,
                    refusal
                )
                .1,
                None
            );
            assert_eq!(
                frame(
                    &ctx,
                    vec![key],
                    &source,
                    row,
                    &mut navigation,
                    false,
                    Ok(())
                )
                .1,
                Some(expected)
            );
            let output = frame(&ctx, vec![], &source, row, &mut navigation, false, Ok(())).0;
            let at = rect(&output, "Ref: Selected Cut · Cut", false).center();
            frame(
                &ctx,
                clicks(at, egui::PointerButton::Secondary),
                &source,
                row,
                &mut navigation,
                false,
                Ok(()),
            );
            let output = frame(&ctx, vec![], &source, row, &mut navigation, false, Ok(())).0;
            for offer in offers(&source, row, Ok(())) {
                assert!(rect(&output, offer.label, true).is_positive());
            }
            let at = rect(&output, "Delete", true).center();
            assert_eq!(
                frame(
                    &ctx,
                    clicks(at, egui::PointerButton::Primary),
                    &source,
                    row,
                    &mut navigation,
                    false,
                    Ok(())
                )
                .1,
                Some(expected)
            );
        }
    }
}
