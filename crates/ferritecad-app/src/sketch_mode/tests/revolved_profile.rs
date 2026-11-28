// SPDX-License-Identifier: MIT
use super::circular_rules::same_model;
use super::cli::{cli_edit, export};
use super::constraint_widgets::click_label;
use super::curve_edits::stored;
use super::polygon_rules::{add, close, distance, ends, orient, solve};
use super::widgets::Ui;
use super::window::{click, enter, finish, open, publish, version};
use super::*;
use crate::sessions::tests::tabs::Drawn;
use ferritecad_document::{Document, Expression, ExpressionSlot, SketchConstraintRule as Rule};
use ferritecad_ui::ModelEdit;

/// A stepped sector with a bore and fractional coordinates.
pub(crate) const SECTOR_P: [[f64; 2]; 5] = [
    [2.75, -3.25],
    [7.5, -3.25],
    [7.5, 2.125],
    [4.25, 9.5],
    [2.75, 9.5],
];
/// The same once its outer wall is dimensioned 7 mm tall.
const SECTOR_TALL: [[f64; 2]; 5] = [
    [2.75, -3.25],
    [7.5, -3.25],
    [7.5, 3.75],
    [4.25, 9.5],
    [2.75, 9.5],
];

/// A solid stepped shaft; Line 6 (index 5) is on the axis.
pub(crate) const SHAFT_T: [[f64; 2]; 6] = [
    [0., -2.25],
    [8., -2.25],
    [8., 3.5],
    [5.5, 3.5],
    [5.5, 12.75],
    [0., 12.75],
];
/// The same once its base is dimensioned 9.25 mm.
const SHAFT_WIDE: [[f64; 2]; 6] = [
    [0., -2.25],
    [9.25, -2.25],
    [9.25, 3.5],
    [6.75, 3.5],
    [6.75, 12.75],
    [0., 12.75],
];

fn check_profile(path: &Path, sketch: ObjectId, points: &[[f64; 2]]) {
    let document = Document::open_read_only(path).expect("published model");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let built = ferritecad_eval::rebuild_cold(&document, &mut kernel, &Default::default())
        .expect("cold rebuild");
    assert_eq!(
        built
            .solve_report(sketch)
            .expect("report")
            .degrees_of_freedom(),
        0
    );
    let curves = built
        .sketch_presentation(sketch)
        .expect("built profile")
        .curves();
    assert_eq!(curves.len(), points.len());
    for (i, (c, [x, y])) in curves.iter().zip(points).enumerate() {
        let SketchGeometry::Line { start: p, .. } = c.geometry() else {
            panic!("side {i}")
        };
        assert!(
            (p.x - x).abs() < 1e-7 && (p.y - y).abs() < 1e-7,
            "corner {i}: {p:?}"
        );
        if *x == 0. {
            assert_eq!(p.x, 0., "shaft remains on the axis");
        }
    }
    crate::test_names::assert_names(&built, &crate::test_names::names(&document));
    built.release_all(&mut kernel);
    document.close().expect("close");
}

fn keep_outputs(root: &Path, name: &str, window: &Path, cli: &Path) {
    let a = crate::sketch::drag_tests::sql_facts(window);
    assert_eq!(
        a,
        crate::sketch::drag_tests::sql_facts(cli),
        "every SQL cell and rowid"
    );
    if let Some(dir) = std::env::var_os("FCAD_REVOLVE_CONSTRAINT_ARTIFACTS") {
        std::fs::create_dir_all(&dir).expect("reader directory");
        for (suffix, p) in [("ui", window), ("cli", cli)] {
            std::fs::copy(p, Path::new(&dir).join(format!("{name}-{suffix}.fcad")))
                .expect("reader input");
        }
    }
    std::fs::write(root.join(format!("{name}.stl")), export(window, "stl")).expect("STL");
}

struct Profile {
    points: &'static [[f64; 2]],
    changed: &'static [[f64; 2]],
    lengths: &'static [(usize, f64)],
    side: usize,
    mm: f64,
    names: [&'static str; 2],
}

fn constrain_revolve(profile: Profile) {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_eval::solver_available(), "PLANEGCS"),
    ]) {
        return;
    }
    let root = tempfile::tempdir().expect("profiles");
    let root = root.path();
    let source = root.join("revolve.fcad");
    let points = profile.points;
    let changed = profile.changed;
    let mm = profile.mm;
    let changed_side = profile.side;
    let names = profile.names;
    let input = root.join("source.json");
    let request = serde_json::json!({
        "request_version": 2, "points_mm": points, "axis": "sketch_y",
        "extent": {"kind":"angle","degrees":137.5}
    });
    std::fs::write(&input, request.to_string()).expect("template");
    let output = std::process::Command::new(crate::creates::tests::ferritecad())
        .arg("create-sketch-revolve")
        .arg(input)
        .arg("-o")
        .arg(&source)
        .arg("--json")
        .output()
        .expect("create");
    assert!(output.status.success(), "{output:?}");
    let original = std::fs::read(&source).expect("source");
    let (sketch, saved) = stored(&source);
    let mut preview = saved.clone();
    assert!(saved.constraints.is_empty());
    let mut w = open(&source, Drawn::Native);
    let initial = version(&w);
    enter(&mut w, sketch, ModelEdit::Sketch);
    w.input
        .frame(([-2., -6., -1.], [12., 15., 1.]))
        .expect("fit profile");
    let mut ui = Ui::new(&mut w);
    close(&mut w, &mut ui, sketch, &mut preview);
    for i in 0..points.len() {
        let next = (i + 1) % points.len();
        let vertical = points[i][0] == points[next][0];
        if vertical || points[i][1] == points[next][1] {
            orient(&mut w, &mut ui, sketch, &mut preview, i, vertical);
        }
    }
    let literal = |v| Expression::constant(v).expect("literal");
    let pin = SketchPointRef::new(saved.curves[0].id, SketchPointSelector::Start);
    let p = point(points[0][0], points[0][1]);
    click(&mut w, p, false);
    add(
        &mut w,
        &mut ui,
        sketch,
        &mut preview,
        "Fix",
        Rule::Fixed {
            point: pin,
            x: literal(p.x),
            y: literal(p.y),
        },
    );
    for &(i, value) in profile.lengths {
        distance(&mut w, &mut ui, sketch, &mut preview, i, value);
    }
    let fixed = Sketch {
        constraints: preview.constraints.clone(),
        ..saved.clone()
    };
    let changes: Vec<_> = fixed
        .constraints
        .iter()
        .map(|c| EditChange::AddConstraint {
            sketch,
            constraint: c.clone(),
        })
        .collect();
    let peer = root.join("fixed-cli.fcad");
    cli_edit(&source, &peer, initial, &changes);
    let request = finish(&mut w, &mut ui);
    assert_eq!(request.changes, changes);
    publish(&mut w, request);
    let first = w.sessions.export_path().expect("accepted");
    same_model(&first, &peer, sketch, &fixed, 0);
    check_profile(&first, sketch, points);
    keep_outputs(root, names[0], &first, &peer);

    // Change only the original wall or base dimension. Every retained UUID survives Set.
    let index = fixed
        .constraints
        .iter()
        .position(|c| {
            matches!(c.rule,Rule::Distance{a,..}
        if a.curve==saved.curves[changed_side].id)
        })
        .expect("dimension on the chosen side");
    let id = fixed.constraints[index].id;
    let change = EditChange::constant(sketch, ExpressionSlot::Distance { constraint: id }, mm)
        .expect("dimension edit");
    let mut from_display = fixed.clone();
    assert_eq!(solve(sketch, &mut from_display), 0);
    let change_number = |s: &mut Sketch| {
        let Rule::Distance { distance, .. } = &mut s.constraints[index].rule else {
            panic!("distance")
        };
        *distance = literal(mm);
    };
    change_number(&mut from_display);
    assert_eq!(solve(sketch, &mut from_display), 0);
    let mut final_sketch = fixed.clone();
    change_number(&mut final_sketch);
    let mut from_storage = final_sketch.clone();
    assert_eq!(solve(sketch, &mut from_storage), 0);
    for i in 0..points.len() {
        let (a, b) = ends(&from_display, i);
        let (c, d) = ends(&from_storage, i);
        for (s, t) in [(a, c), (b, d)] {
            assert!((s.x - t.x).abs() < 1e-6 && (s.y - t.y).abs() < 1e-6);
        }
    }
    let second_cli = root.join("changed-cli.fcad");
    cli_edit(
        &first,
        &second_cli,
        version(&w),
        std::slice::from_ref(&change),
    );
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    click_label(&mut w, &mut ui, id);
    ui.replace_dimension_text(&mut w, &mm.to_string());
    ui.press(&mut w, "Set", false);
    let request = finish(&mut w, &mut ui);
    assert_eq!(request.changes, [change]);
    publish(&mut w, request);
    let second = w.sessions.export_path().expect("accepted");
    same_model(&second, &second_cli, sketch, &final_sketch, 0);
    check_profile(&second, sketch, changed);
    keep_outputs(root, names[1], &second, &second_cli);
    assert_ne!(
        std::fs::read(root.join(format!("{}.stl", names[0]))).expect("original solid"),
        std::fs::read(root.join(format!("{}.stl", names[1]))).expect("changed solid")
    );
    assert_eq!(std::fs::read(&source).expect("source intact"), original);
}
#[test]
fn native_viewport_constraints_keep_the_partial_revolve_and_resize_its_wall() {
    constrain_revolve(Profile {
        points: &SECTOR_P,
        changed: &SECTOR_TALL,
        lengths: &[(0, 4.75), (1, 5.375), (3, 1.5), (4, 12.75)],
        side: 1,
        mm: 7.,
        names: ["ui", "tall"],
    });
}
#[test]
fn native_viewport_constraints_keep_the_solid_shaft_on_axis_and_resize_its_base() {
    constrain_revolve(Profile {
        points: &SHAFT_T,
        changed: &SHAFT_WIDE,
        lengths: &[(0, 8.), (1, 5.75), (2, 2.5), (3, 9.25)],
        side: 0,
        mm: 9.25,
        names: ["axis-rigid", "axis-wide"],
    });
}
