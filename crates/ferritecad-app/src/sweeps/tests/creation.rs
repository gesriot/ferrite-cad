// SPDX-License-Identifier: MIT
//! Create the five former drawing templates with viewport widgets. Each CLI peer
//! starts with its own IDs and an explicit description of the expected drawing.
use super::*;
use crate::creates::tests::{ferritecad, read_semantics};
use crate::sketch_mode::tests::window::{apply_with_upload, click, finish, start_new};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

#[derive(Clone, Copy)]
enum Model {
    Polygon,
    Circle,
    Annulus,
    FullTurn,
    Sector,
}

const POLYGON: [[i32; 2]; 6] = [[0, 0], [60, 0], [60, 20], [20, 20], [20, 40], [0, 40]];
const BUSHING: [[i32; 2]; 6] = [[4, 0], [10, 0], [10, 5], [7, 5], [7, 15], [4, 15]];

fn click_xy(w: &mut Window, [x, y]: [i32; 2]) {
    click(w, point(f64::from(x), f64::from(y)), false);
}

fn create_in_window(model: Model, output: &Path) {
    let mut w = Window::new(Drawn::Native, None);
    assert!(w.begin_new(false));
    let empty = w
        .answer_new(ferritecad_ui::NewChoice::Create)
        .expect("Empty");
    w.create_from_new(empty, Ok(())).expect("show Empty");
    assert!(w.sessions.untitled());
    let size = if matches!(model, Model::Polygon) {
        70.
    } else {
        20.
    };
    w.input.frame(([-size; 3], [size; 3])).expect("frame");
    let mut ui = Ui::new(&mut w);
    start_new(&mut w, &mut ui);
    ui.press(&mut w, "Off", false);
    ui.press(&mut w, "1 mm", false);
    match model {
        Model::Circle | Model::Annulus => {
            ui.press(&mut w, "Circle", false);
            for p in [[12, -7], [22, -7]] {
                click_xy(&mut w, p);
            }
            if matches!(model, Model::Annulus) {
                for p in [[12, -7], [16, -7]] {
                    click_xy(&mut w, p);
                }
            }
        }
        Model::Polygon | Model::FullTurn | Model::Sector => {
            ui.press(&mut w, "Line", false);
            let points = if matches!(model, Model::Polygon) {
                [[0, 0], [60, 0], [60, 20], [20, 20], [20, 40], [0, 40]]
            } else {
                [[4, 0], [10, 0], [10, 5], [7, 5], [7, 15], [4, 15]]
            };
            for p in points.into_iter().chain([points[0]]) {
                click_xy(&mut w, p);
            }
        }
    }

    // Refusing the picture must retain the complete draft without resubmitting it.
    let asked = finish(&mut w, &mut ui);
    let before = format!("{:?}", w.creates.sketch.viewport);
    let original = version(&w);
    let refusal = ferritecad_types::CadError::rendering("test upload refused");
    assert!(!apply_with_upload(&mut w, asked, Err(refusal)));
    assert_eq!(format!("{:?}", w.creates.sketch.viewport), before);
    assert_eq!(version(&w), original);
    assert!(w.creates.sketch.viewport.take_request().is_none());
    let asked = finish(&mut w, &mut ui);
    assert!(apply(&mut w, asked));
    assert!(!w.creates.sketch.viewport.active());

    let sketch = objects(&w)
        .into_iter()
        .find(|o| o.name.as_deref() == Some("Sketch 1"))
        .expect("published sketch")
        .id;
    let command = match model {
        Model::Polygon | Model::Circle | Model::Annulus => ModelEdit::Extrude,
        Model::FullTurn | Model::Sector => ModelEdit::Revolve,
    };
    enter(&mut w, sketch, command);
    match model {
        Model::Circle | Model::Annulus => ui.type_value(&mut w, "10", "15"),
        Model::Sector => {
            ui.press(&mut w, "Angle (°)", true);
            ui.type_value(&mut w, "90", "137.5");
        }
        Model::Polygon | Model::FullTurn => {}
    }
    let asked = request(&mut w, &mut ui);
    assert!(apply(&mut w, asked));
    assert!(w.sessions.untitled());
    assert!(
        w.save(ferritecad_jobs::SaveTarget::As(output.to_owned()))
            .published
    );
}

fn cli(dir: &Path, args: &[&str]) -> Vec<u8> {
    let result = Command::new(ferritecad())
        .current_dir(dir)
        .args(args)
        .output()
        .expect("CLI");
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    result.stdout
}

fn mm(value: i32) -> Value {
    json!({"value": value, "unit": "mm"})
}

fn cli_point([x, y]: [i32; 2]) -> Value {
    json!({"x": mm(x), "y": mm(y)})
}

fn polygon_curves(points: &[[i32; 2]]) -> Vec<Value> {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(&start, &end)| {
            json!({"id": StableEntityId::new(), "construction": false, "geometry":{
                "kind":"line","start":cli_point(start),"end":cli_point(end)
            }})
        })
        .collect()
}

fn add_version(dir: &Path, source: &str, target: &str, changes: Value) {
    let inspection: Value =
        serde_json::from_slice(&cli(dir, &["inspect", source, "--json"])).expect("inspect JSON");
    let version = inspection["result"]["content_version"]
        .as_str()
        .expect("version");
    let request = json!({"request_version": 1, "changes": changes});
    fs::write(dir.join("edit.json"), request.to_string()).expect("request");
    cli(
        dir,
        &[
            "edit",
            source,
            "--expect-version",
            version,
            "--request",
            "edit.json",
            "-o",
            target,
        ],
    );
}

fn independent_document(model: Model, dir: &Path) {
    let plane = ObjectId::new();
    let sketch = ObjectId::new();
    let circle = |radius| {
        json!({
            "id": StableEntityId::new(), "construction": false,
            "geometry": {"kind": "circle", "center": cli_point([12, -7]), "radius": mm(radius)}
        })
    };
    let curves = match model {
        Model::Polygon => polygon_curves(&POLYGON),
        Model::Circle => vec![circle(10)],
        Model::Annulus => vec![circle(10), circle(4)],
        Model::FullTurn | Model::Sector => polygon_curves(&BUSHING),
    };
    cli(dir, &["create", "empty.fcad"]);
    add_version(
        dir,
        "empty.fcad",
        "drawing.fcad",
        json!([
            {"kind":"add_datum_plane","object":plane,"name":"XY","plane":{
                "kind":"fixed","placement":{"rows":[[1,0,0,0],[0,1,0,0],[0,0,1,0]]}
            }},
            {"kind":"add_sketch","object":sketch,"name":"Sketch 1","plane":plane,
             "curves":curves,"constraints":[]}
        ]),
    );
    let (name, feature) = match model {
        Model::Polygon | Model::Circle | Model::Annulus => {
            let distance = if matches!(model, Model::Polygon) {
                10
            } else {
                15
            };
            (
                "Extrude 1",
                json!({"kind":"extrude","profile":sketch,"operation":"new_body",
                "reversed":false,"end_condition":{"kind":"blind","distance":mm(distance)}}),
            )
        }
        Model::FullTurn | Model::Sector => {
            let extent = if matches!(model, Model::Sector) {
                json!({"kind":"partial","angle":{"value":137.5,"unit":"deg"}})
            } else {
                json!({"kind":"full_turn"})
            };
            (
                "Revolve 1",
                json!({"kind":"revolve","profile":sketch,"operation":"new_body",
                "axis":"sketch_y","extent":extent}),
            )
        }
    };
    add_version(
        dir,
        "drawing.fcad",
        "cli.fcad",
        json!([{
            "kind":"add_body","object":ObjectId::new(),"name":"Body 1",
            "feature_object":ObjectId::new(),"feature_name":name,"feature":feature
        }]),
    );
}

fn create_template(model: Model, dir: &Path) {
    let (command, input) = match model {
        Model::Polygon => (
            "create-sketch-extrude",
            json!({"request_version":1,"points_mm":POLYGON,"height_mm":10}),
        ),
        Model::Circle => (
            "create-circle-extrude",
            json!({"schema_version":1,"center_mm":[12,-7],"radius_mm":10,"height_mm":15}),
        ),
        Model::Annulus => (
            "create-annular-extrude",
            json!({"schema_version":1,
            "center_mm":[12,-7],"outer_radius_mm":10,"inner_radius_mm":4,"height_mm":15}),
        ),
        Model::FullTurn => (
            "create-sketch-revolve",
            json!({"request_version":1,
            "points_mm":BUSHING,"axis":"sketch_y","angle":"full_turn"}),
        ),
        Model::Sector => (
            "create-sketch-revolve",
            json!({"request_version":2,
            "points_mm":BUSHING,"axis":"sketch_y","extent":{"kind":"angle","degrees":137.5}}),
        ),
    };
    fs::write(dir.join("template.json"), input.to_string()).expect("template request");
    cli(
        dir,
        &[command, "template.json", "-o", "template.fcad", "--json"],
    );
}

fn export(dir: &Path, stem: &str, format: &str) -> Vec<u8> {
    let target = format!("{stem}.{format}");
    cli(
        dir,
        &[
            &format!("export-{format}"),
            &format!("{stem}.fcad"),
            "-o",
            &target,
        ],
    );
    fs::read(dir.join(target)).expect("exported bytes")
}

fn mapped_fbx(dir: &Path, stem: &str) -> String {
    let document = Document::open_read_only(dir.join(format!("{stem}.fcad"))).expect("document");
    let body = document
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .expect("Body");
    let text = String::from_utf8(export(dir, stem, "fbx")).expect("ASCII FBX");
    assert_eq!(text.matches(&body.id.to_string()).count(), 3);
    text.replace(&body.id.to_string(), "same-body")
}

fn compare_creation(model: Model) {
    if !native() {
        return;
    }
    let temp = tempfile::tempdir().expect("directory");
    let dir = temp.path();
    create_in_window(model, &dir.join("ui.fcad"));
    independent_document(model, dir);
    create_template(model, dir);

    let ui = read_semantics(&dir.join("ui.fcad"));
    let peer = read_semantics(&dir.join("cli.fcad"));
    assert_eq!(ui.0, peer.0);
    assert_ne!(ui.1, peer.1);
    let window_stl = export(dir, "ui", "stl");
    assert_eq!(
        window_stl,
        export(dir, "cli", "stl"),
        "independent edit STL"
    );
    assert_eq!(
        window_stl,
        export(dir, "template", "stl"),
        "original template STL"
    );
    match model {
        Model::Polygon => {}
        Model::FullTurn => assert_eq!(
            export(dir, "ui", "fbx").len(),
            export(dir, "cli", "fbx").len()
        ),
        Model::Sector => assert_eq!(mapped_fbx(dir, "ui"), mapped_fbx(dir, "cli")),
        Model::Circle | Model::Annulus => {
            let (kind, variable) = if matches!(model, Model::Circle) {
                ("circle", "FCAD_CIRCLE_ARTIFACTS")
            } else {
                ("annulus", "FCAD_ANNULUS_ARTIFACTS")
            };
            for stem in ["ui", "cli", "template"] {
                export(dir, stem, "fbx");
            }
            if let Some(root) = std::env::var_os(variable) {
                let root = Path::new(&root);
                fs::create_dir_all(root).expect("artifact directory");
                fs::write(root.join(format!("{kind}-ui.stl")), window_stl).expect("reference STL");
                for stem in ["ui", "cli", "template"] {
                    let dst = root.join(format!("{kind}-{stem}.fbx"));
                    fs::copy(dir.join(format!("{stem}.fbx")), dst).expect("reader artifact");
                }
            }
        }
    }
}

#[test]
fn native_polygon_creation_matches_both_cli_references() {
    compare_creation(Model::Polygon);
}
#[test]
fn native_circle_creation_matches_both_cli_references() {
    compare_creation(Model::Circle);
}
#[test]
fn native_annulus_creation_matches_both_cli_references() {
    compare_creation(Model::Annulus);
}
#[test]
fn native_full_turn_creation_matches_both_cli_references() {
    compare_creation(Model::FullTurn);
}
#[test]
fn native_sector_creation_matches_both_cli_references() {
    compare_creation(Model::Sector);
}
