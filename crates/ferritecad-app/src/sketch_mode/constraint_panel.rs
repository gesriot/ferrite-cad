// SPDX-License-Identifier: MIT
use super::{
    draft::{Draft, dimension},
    rules::{Button, READ_ONLY, Selection},
};
use ferritecad_document::ExpressionSlot;
use ferritecad_types::StableEntityId;

pub(super) fn controls(
    ui: &mut egui::Ui,
    draft: &mut Draft,
    field: &mut Option<(StableEntityId, f64, String)>,
) {
    let available = ferritecad_eval::solver_available();
    ui.horizontal_wrapped(|ui| {
        for button in Button::ALL {
            let answer = button.rule(&draft.state.curves, &draft.state.selected, available);
            let response = ui.add_enabled(answer.is_ok(), egui::Button::new(button.label()));
            match answer {
                Ok(rule) if response.clicked() => draft.add_rule(rule, button),
                Ok(_) => {}
                Err(why) => {
                    response.on_disabled_hover_text(why);
                }
            }
        }
    });
    if let Some(report) = &draft.state.report {
        let n = report.degrees_of_freedom();
        ui.label(match n {
            0 => "Fully constrained".into(),
            1 => "1 degree of freedom".into(),
            n => format!("{n} degrees of freedom"),
        });
        for id in report.redundant() {
            if let Some(c) = draft.state.constraints.iter().find(|c| c.id == *id) {
                ui.label(format!(
                    "Redundant: {} ({id})",
                    crate::describe_constraint(&c.rule)
                ));
            }
        }
    }
    let [Selection::Constraint(id)] = *draft.state.selected.as_slice() else {
        *field = None;
        return;
    };
    let Some((slot, expr)) = draft
        .state
        .constraints
        .iter()
        .find(|c| c.id == id)
        .and_then(dimension)
    else {
        *field = None;
        return;
    };
    if expr.numeric_form_refusal().is_some() {
        ui.horizontal(|ui| {
            ui.label("Value");
            ui.monospace(&expr.source);
        });
        ui.label(READ_ONLY);
        *field = None;
        return;
    }
    let value = expr.value();
    let mut diameter = draft.state.diameters.contains(&id);
    if matches!(slot, ExpressionSlot::Radius { .. }) {
        let was_diameter = diameter;
        ui.add_enabled_ui(available, |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut diameter, false, "Radius (R)");
                ui.radio_value(&mut diameter, true, "Diameter (Ø)");
            });
        });
        if diameter != was_diameter {
            draft.begin_step();
            if diameter {
                draft.state.diameters.insert(id);
            } else {
                draft.state.diameters.remove(&id);
            }
        }
    }
    let (shown, unit) = match slot {
        // The degrees that read back as exactly the stored radians: 30 stays 30, not 29.999….
        ExpressionSlot::DirectedAngle { .. } => (
            ferritecad_document::DirectedAngle::from_radians(value)
                .map_or(f64::NAN, crate::sketch::dimension::editable_degrees),
            "°",
        ),
        ExpressionSlot::Radius { .. } if diameter => (value * 2., "mm"),
        _ => (value, "mm"),
    };
    if field.as_ref().map(|(key, old, _)| (*key, *old)) != Some((id, shown)) {
        *field = Some((id, shown, shown.to_string()));
    }
    let (_, _, text) = field.as_mut().expect("selected dimension");
    let submit = ui
        .horizontal(|ui| {
            ui.label("Value");
            let can_set = ui.is_enabled() && available;
            let response = ui.add(
                egui::TextEdit::singleline(text)
                    .desired_width(80.)
                    .interactive(can_set),
            );
            ui.label(unit);
            ui.add_enabled(can_set, egui::Button::new("Set")).clicked()
                || (can_set
                    && response.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter)))
        })
        .inner;
    if submit {
        draft.set_value(id, text);
        if draft.message.is_empty() {
            *field = None;
        }
    }
}
