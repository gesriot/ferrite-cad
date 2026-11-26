// SPDX-License-Identifier: MIT
//! Viewport endpoint gestures and the original independent publication witnesses.
#![allow(clippy::panic)]
use super::grid::Snap;
use super::*;
use ferritecad_jobs::PolygonExtrusion;

pub(crate) fn hold_vertex(e: &mut Editor) -> (egui::Context, egui::Pos2) {
    crate::sketch_mode::tests::curve_edits::hold(e)
}
pub(crate) fn release_vertex(ctx: &egui::Context, e: &mut Editor, at: egui::Pos2) {
    // The egui release also ends any widget-owned pointer state.
    super::tests::frame(
        ctx,
        e,
        vec![egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    crate::sketch_mode::tests::curve_edits::release_held(e);
}
fn assert_revolve_edit_builds(path: &std::path::Path, points: &[[f64; 2]]) {
    let bytes = std::fs::read(path).expect("source");
    let (_window, mut request, _) = crate::sketch_mode::tests::curve_edits::polygon(path, points);
    let root = tempfile::tempdir().expect("destination");
    request.destination = root.path().join("accepted.fcad");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let result = ferritecad_jobs::edit_copy(&request, &mut kernel, &Default::default())
        .expect("the kernel accepts the changed axis contact");
    assert!(result.rebuild.failed.is_empty() && result.rebuild.skipped.is_empty());
    assert!(request.destination.exists());
    assert_eq!(std::fs::read(path).expect("source"), bytes);
}

#[test]
fn snap_rounds_values_below_half_a_step_toward_the_nearer_grid_point() {
    let below_half = f64::from_bits(0.05_f64.to_bits() - 1);
    assert_eq!(Snap::Tenth.round(below_half), 0.);
    assert_eq!(Snap::Tenth.round(-below_half), 0.);
    assert_eq!(Snap::Tenth.round(0.05), 0.1_f64);
    assert_eq!(Snap::Tenth.round(-0.05), -0.1_f64);
}

#[test]
fn snap_large_finite_displacements_remain_refused_without_overflow() {
    for (snap, _) in Snap::CHOICES {
        for value in [
            f64::from(f32::MAX),
            -f64::from(f32::MAX),
            f64::MAX,
            -f64::MAX,
        ] {
            let coordinate = snap.round(value);
            assert!(
                PolygonExtrusion::new(
                    vec![[coordinate, 0.], [60., 0.], [60., 40.], [0., 40.]],
                    10.
                )
                .is_err(),
                "{snap:?} must retain the shared range refusal"
            );
        }
    }
}

#[test]
fn snap_preserves_invalid_coordinate_refusals_without_panicking() {
    for (snap, _) in Snap::CHOICES {
        for value in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
            -f64::MAX,
        ] {
            let coordinate = snap.round(value);
            assert!(
                PolygonExtrusion::new(
                    vec![[coordinate, 0.], [60., 0.], [60., 40.], [0., 40.]],
                    10.,
                )
                .is_err(),
                "{snap:?} must not make {value} publishable"
            );
        }
    }
}

#[test]
fn snap_step_rounds_pointer_input_without_changing_fields_or_off_precision() {
    assert_eq!(Snap::Off.round(80.07456568667763), 80.07456568667763_f64);
    assert_eq!(Snap::One.round(80.07456568667763), 80_f64);
    assert_eq!(Snap::Tenth.round(0.1 + 0.2), 0.3_f64);
    assert_eq!(Snap::Tenth.round(0.15), 0.2_f64);
    assert_eq!(Snap::Tenth.round(-0.15), -0.2_f64);
    assert_eq!(Snap::Tenth.round(0.05), 0.1_f64);
    assert_eq!(Snap::Tenth.round(-0.05), -0.1_f64);
    assert_eq!(Snap::Five.round(-7.5), -10_f64);
    assert_eq!(Snap::Ten.round(15.), 20_f64);
}

pub(crate) fn attach_source_claim(source: &std::path::Path) {
    let db = rusqlite::Connection::open(source).expect("SQL");
    let bytes = b"attached Sketch source must remain owned after pointer editing";
    db.execute("INSERT INTO imported_sources(id,format,bytes,content_hash,byte_len,created_at) VALUES(zeroblob(16),'future.sketch-source',?1,?2,?3,'saved')",
        rusqlite::params![bytes.as_slice(),ferritecad_types::ContentHash::of_bytes(bytes).as_bytes().as_slice(),bytes.len() as i64]).expect("source bytes");
    db.execute_batch("INSERT INTO imported_source_refs SELECT id,zeroblob(16) FROM objects WHERE kind='sketch'; CREATE TABLE extension(value BLOB); INSERT INTO extension(rowid,value) VALUES(73,X'00FF');").expect("claims and other SQL");
}
pub(crate) fn sql_facts(
    path: &std::path::Path,
) -> std::collections::BTreeMap<String, Vec<Vec<rusqlite::types::Value>>> {
    use rusqlite::{Connection, OpenFlags};
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("SQL");
    let names: Vec<String> = db
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
        .expect("tables")
        .query_map([], |r| r.get(0))
        .expect("rows")
        .collect::<rusqlite::Result<_>>()
        .expect("names");
    names
        .into_iter()
        .map(|name| {
            let quoted = name.replace('"', "\"\"");
            let mut query = db
                .prepare(&format!("SELECT rowid,* FROM \"{quoted}\" ORDER BY rowid"))
                .or_else(|_| db.prepare(&format!("SELECT * FROM \"{quoted}\"")))
                .expect("table");
            let n = query.column_count();
            let modified = (name == "meta")
                .then(|| query.column_names().position(|c| c == "modified_at"))
                .flatten();
            let rows = query
                .query_map([], |r| {
                    (0..n)
                        .map(|i| {
                            if modified == Some(i) {
                                Ok(rusqlite::types::Value::Null)
                            } else {
                                r.get(i)
                            }
                        })
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .expect("rows")
                .collect::<rusqlite::Result<_>>()
                .expect("cells");
            (name, rows)
        })
        .collect()
}
/// What a published dragged copy of the editor's L must contain, in mm: its
/// vertices in saved order, the X of the first Line's far end (the side face),
/// the box's high corner and the volume; and where an independent reader may
/// pick the files up (an environment variable naming a directory, and a stem).
pub(super) struct Published {
    pub(super) vertices: [[f64; 2]; 6],
    pub(super) side_x: f64,
    pub(super) high: [f64; 3],
    pub(super) volume_mm3: f64,
    pub(super) artifacts: (&'static str, &'static str),
}

pub(super) fn verify_publications(
    source: &std::path::Path,
    ui: &std::path::Path,
    cli: &std::path::Path,
) {
    verify_published(
        source,
        ui,
        cli,
        &Published {
            vertices: [
                [0., 0.],
                [80., 0.],
                [80., 20.],
                [20., 20.],
                [20., 40.],
                [0., 40.],
            ],
            side_x: 80.,
            high: [80., 40., 10.],
            volume_mm3: 20000.,
            artifacts: ("FCAD_SKETCH_DRAG_ARTIFACT_DIR", "drag"),
        },
    );
}

pub(super) fn verify_published(
    source: &std::path::Path,
    ui: &std::path::Path,
    cli: &std::path::Path,
    expected: &Published,
) {
    use crate::creates::tests::ferritecad;
    use ferritecad_document::Document;
    use ferritecad_kernel::{GeometryKernel, OperationContext, TessellationParams};
    let a = sql_facts(ui);
    assert_eq!(a, sql_facts(cli), "all SQL cells, including rowids");
    for (table, rows) in sql_facts(source) {
        if table != "objects" {
            assert_eq!(a[&table], rows, "preserve {table}");
        }
    }
    let d = Document::open_read_only(ui).expect("UI copy");
    let original = Document::open_read_only(source).expect("source");
    let choices = crate::sketch_facts::Source::read(&d).expect("catalog");
    assert_eq!(
        crate::transaction_tests::stored_vertices(
            &choices,
            crate::transaction_tests::line_sketch(&choices)
        )
        .iter()
        .map(|v| v.start_mm)
        .collect::<Vec<_>>(),
        expected.vertices
    );
    let selected = crate::sketch_mode::tests::curve_edits::sketch_id(&choices);
    for old in original.objects().expect("objects") {
        let new = d.object(old.id).expect("object").expect("same ID");
        if old.id != selected {
            assert_eq!(old, new);
        } else {
            assert_eq!(
                (old.id, old.name, old.parent, old.ordinal),
                (new.id, new.name, new.parent, new.ordinal)
            );
        }
    }
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = ferritecad_eval::rebuild_cold(&d, &mut kernel, &context).expect("cold rebuild");
    let faces = crate::test_names::root_faces(&d, &built);
    assert_eq!(
        faces.len(),
        crate::transaction_tests::stored_vertices(
            &choices,
            crate::transaction_tests::line_sketch(&choices)
        )
        .len()
            + 2,
        "every curve side and both caps have a derived face name"
    );
    let selected_curve = crate::transaction_tests::stored_vertices(
        &choices,
        crate::transaction_tests::line_sketch(&choices),
    )[0]
    .curve_id;
    let mut checked = 0;
    for (input, face) in faces {
        if matches!(input, ferritecad_kernel::NamingInput::Curve(c) if c != selected_curve) {
            continue;
        }
        checked += 1;
        let mesh = kernel
            .tessellate(face.shape(), &TessellationParams::default(), &context)
            .expect("face mesh");
        let range = mesh
            .faces
            .iter()
            .find(|m| m.face == face)
            .expect("actual face");
        let points: Vec<_> = mesh.indices
            [range.first_index as usize..(range.first_index + range.index_count) as usize]
            .iter()
            .map(|&i| &mesh.positions[3 * i as usize..3 * i as usize + 3])
            .collect();
        match input {
            ferritecad_kernel::NamingInput::Cap { end: false } => {
                assert!(points.iter().all(|p| p[2] == 0.))
            }
            ferritecad_kernel::NamingInput::Cap { end: true } => {
                assert!(points.iter().all(|p| p[2] == 10.))
            }
            ferritecad_kernel::NamingInput::Curve(curve) => {
                assert_eq!(curve, selected_curve);
                assert!(points.iter().all(|p| p[1] == 0.));
                assert!(points.iter().any(|p| p[0] == expected.side_x as f32));
            }
            _ => panic!("unexpected root"),
        }
    }
    assert_eq!(checked, 3);
    built.release_all(&mut kernel);
    d.close().expect("close");
    original.close().expect("close");
    let mut artifacts = Vec::new();
    for path in [ui, cli] {
        for command in ["validate", "rebuild"] {
            let mut cmd = std::process::Command::new(ferritecad());
            cmd.arg(command).arg(path);
            if command == "rebuild" {
                cmd.arg("--cold");
            }
            let output = cmd.output().expect("process");
            assert!(output.status.success(), "{output:?}");
        }
        let mut files = Vec::new();
        for (op, suffix) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
            let out = path.with_extension(suffix);
            let p = std::process::Command::new(ferritecad())
                .arg(op)
                .arg(path)
                .arg("-o")
                .arg(&out)
                .arg("--json")
                .output()
                .expect("export");
            assert!(p.status.success(), "{p:?}");
            files.push(std::fs::read(&out).expect("artifact"));
        }
        artifacts.push(files);
    }
    assert_eq!(artifacts[0], artifacts[1]);
    let bytes = &artifacts[0][0];
    let triangles = u32::from_le_bytes(bytes[80..84].try_into().expect("count")) as usize;
    assert_eq!(bytes.len(), 84 + triangles * 50);
    assert_eq!(triangles, 20);
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut volume6 = 0.;
    for i in 0..triangles {
        let v: [f64; 9] = std::array::from_fn(|j| {
            let at = 84 + 50 * i + 12 + 4 * j;
            f64::from(f32::from_le_bytes(
                bytes[at..at + 4].try_into().expect("f32"),
            ))
        });
        for p in v.as_chunks::<3>().0 {
            for j in 0..3 {
                lo[j] = lo[j].min(p[j]);
                hi[j] = hi[j].max(p[j]);
            }
        }
        volume6 += v[0] * (v[4] * v[8] - v[5] * v[7])
            + v[1] * (v[5] * v[6] - v[3] * v[8])
            + v[2] * (v[3] * v[7] - v[4] * v[6]);
    }
    assert_eq!(lo, [0., 0., 0.]);
    assert_eq!(hi, expected.high);
    // Same 1 ppm tolerance as §25B; planar integer vertices are exact in f32.
    assert!((volume6.abs() / 6. - expected.volume_mm3).abs() < expected.volume_mm3 * 1e-6);
    if let Some(dir) = std::env::var_os(expected.artifacts.0) {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("artifact directory");
        let stem = expected.artifacts.1;
        for (from, name) in [
            (ui.to_path_buf(), format!("{stem}.fcad")),
            (ui.with_extension("stl"), format!("{stem}.stl")),
            (ui.with_extension("fbx"), format!("{stem}.fbx")),
        ] {
            std::fs::copy(from, dir.join(name)).expect("retain artifacts for independent reader");
        }
    }
}

#[test]
fn native_dragged_sketch_worker_and_cli_preserve_identity_and_geometry() {
    super::tests::saved_sketch_and_cli(true);
}

/// §27C: a solid cylinder drawn on the real canvas and published by the
/// window's worker and the peer CLI alike, then reopened and reshaped into a
/// frustum by one drag, with the saved axis Line held on the axis.
#[test]
fn native_solid_revolve_widgets_drag_worker_and_cli_create_and_edit() {
    use crate::creates::tests::{ferritecad, read_semantics};
    use ferritecad_document::Document;
    use ferritecad_kernel::OperationContext;
    if !ferritecad_occt::is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        return;
    }
    let pi = std::f64::consts::PI;
    let root = tempfile::tempdir().expect("directory");
    let ui = root.path().join("cylinder-ui.fcad");
    let cli = root.path().join("cylinder-cli.fcad");
    crate::sweeps::tests::window::creation::solid_cylinder(&ui, root.path());
    let independent = root.path().join("cli.fcad");
    assert_eq!(read_semantics(&ui).0, read_semantics(&independent).0);
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let cylinder = Document::open_read_only(&ui).expect("cylinder");
    let built = ferritecad_eval::rebuild_cold(&cylinder, &mut kernel, &Default::default())
        .expect("solid cylinder");
    let body = cylinder
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ferritecad_document::ObjectPayload::Body(_)))
        .expect("Body");
    let (_, volume) = kernel
        .shape_stats(built.shape(body.id).expect("shape"))
        .expect("volume");
    assert!((volume - 1500. * pi).abs() < 1e-9);
    built.release_all(&mut kernel);
    cylinder.close().expect("close");

    let input = root.path().join("cylinder.json");
    std::fs::write(
        &input,
        r#"{"request_version":1,"points_mm":[[0,0],[10,0],[10,15],[0,15]],"axis":"sketch_y","angle":"full_turn"}"#,
    )
    .expect("request");
    let p = std::process::Command::new(ferritecad())
        .arg("create-sketch-revolve")
        .arg(&input)
        .arg("-o")
        .arg(&cli)
        .arg("--json")
        .output()
        .expect("peer CLI");
    assert!(p.status.success(), "{p:?}");
    let export = |path: &std::path::Path| {
        let mut bytes = Vec::new();
        for (op, extension) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
            let out = path.with_extension(extension);
            let p = std::process::Command::new(ferritecad())
                .arg(op)
                .arg(path)
                .arg("-o")
                .arg(&out)
                .output()
                .expect("export");
            assert!(p.status.success(), "{p:?}");
            bytes.push(std::fs::read(&out).expect("bytes"));
        }
        bytes
    };
    let (a, b) = (export(&ui), export(&independent));
    assert_eq!(a[0], export(&cli)[0], "original cylinder STL");
    assert_eq!(a[0], b[0], "worker/CLI STL bytes");
    let mapped_fbx = |path: &std::path::Path, bytes: &[u8]| {
        let document = Document::open_read_only(path).expect("document");
        let body = document
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ferritecad_document::ObjectPayload::Body(_)))
            .expect("Body");
        let text = std::str::from_utf8(bytes).expect("ASCII FBX");
        assert_eq!(text.matches(&body.id.to_string()).count(), 3);
        let mapped = text.replace(&body.id.to_string(), "same-body");
        document.close().expect("close");
        mapped
    };
    assert_eq!(
        mapped_fbx(&ui, &a[1]),
        mapped_fbx(&independent, &b[1]),
        "all FBX bytes agree after mapping the two created Body identities"
    );

    // Reopen the worker's file and edit it in the same Line editor.
    let source_bytes = std::fs::read(&ui).expect("source");
    let reading: crate::sketch_facts::Source = {
        let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
        ferritecad_scene::snapshot_of(
            &ui,
            &mut k,
            |k, b| k.import_step(b),
            &Default::default(),
            &OperationContext::default(),
        )
        .expect("accepted scene")
        .edit_source
        .expect("accepted edit facts")
        .into()
    };
    assert_revolve_edit_builds(&ui, &[[3., 0.], [10., 0.], [5., 15.], [0., 15.]]);
    assert_revolve_edit_builds(&ui, &[[1., 0.], [10., 0.], [5., 15.], [1., 15.]]);
    let literal = [[0., 0.], [10., 0.], [5., 15.], [0., 15.]];
    let (mut window, request, expected) =
        crate::sketch_mode::tests::curve_edits::polygon(&ui, &literal);
    assert_eq!(request.expected, reading.version);
    assert!(crate::sketch_mode::tests::window::apply(
        &mut window,
        request
    ));
    let edited = root.path().join("frustum-ui.fcad");
    assert!(
        window
            .save(ferritecad_jobs::SaveTarget::As(edited.clone()))
            .published
    );

    let edit = root.path().join("edit.json");

    std::fs::write(
        &edit,
        ferritecad_fixtures::edit_request_json(&expected).to_string(),
    )
    .expect("edit request");
    let peer = root.path().join("frustum-cli.fcad");
    let p = std::process::Command::new(ferritecad())
        .arg("edit")
        .arg(&ui)
        .arg("--expect-version")
        .arg(reading.version.content.to_string())
        .arg("--request")
        .arg(&edit)
        .arg("-o")
        .arg(&peer)
        .arg("--json")
        .output()
        .expect("peer CLI");
    assert!(p.status.success(), "{p:?}");
    let a = Document::open_read_only(&edited).expect("UI");
    let b = Document::open_read_only(&peer).expect("CLI");
    assert_eq!(a.objects().expect("objects"), b.objects().expect("objects"));
    let original = Document::open_read_only(&ui).expect("source");
    assert_eq!(
        crate::test_names::names(&a),
        crate::test_names::names(&original),
        "no new names, and still none for the axis Line"
    );
    assert_eq!(crate::test_names::names(&a), crate::test_names::names(&b));
    original.close().expect("close");
    // π·h/3·(R² + R·r + r²) for R = 10, r = 5, h = 15.
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = ferritecad_eval::rebuild_cold(&a, &mut kernel, &context).expect("cold");
    let body = a
        .objects()
        .expect("objects")
        .iter()
        .find(|o| matches!(o.payload, ferritecad_document::ObjectPayload::Body(_)))
        .map(|o| o.id)
        .expect("Body");
    let (faces, volume) = kernel
        .shape_stats(built.shape(body).expect("one solid"))
        .expect("stats");
    assert_eq!(faces, 3, "two discs and one cone, no face for the axis");
    assert!((volume - 875. * pi).abs() < 1e-6 * 875. * pi, "{volume}");
    built.release_all(&mut kernel);
    a.close().expect("close");
    b.close().expect("close");
    assert_eq!(export(&edited), export(&peer), "STL and FBX bytes");
    assert_eq!(std::fs::read(&ui).expect("source"), source_bytes);
}

/// §27F through the real widgets: a saved 220° sector of a solid cylinder is
/// opened from its own Edit Sketch row, shows its angle read-only, is edited
/// by one drag and exact fields, refuses an axis end moved off the axis, and
/// the shared worker and the peer CLI publish the same copy — the same SQL,
/// the same stored angle and names, and the same STL and FBX bytes.
#[test]
fn native_partial_revolve_profile_widgets_drag_worker_and_cli_edit_one_copy() {
    use crate::creates::tests::{ferritecad, read_semantics};
    use ferritecad_document::{Document, ObjectPayload, RevolveExtent};
    use ferritecad_kernel::OperationContext;
    if !ferritecad_occt::is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("sector.fcad");
    let input = root.path().join("create.json");
    std::fs::write(
        &input,
        r#"{"request_version":2,"points_mm":[[0,0],[10,0],[10,15],[0,15]],"axis":"sketch_y","extent":{"kind":"angle","degrees":220}}"#,
    )
    .expect("request");
    let p = std::process::Command::new(ferritecad())
        .arg("create-sketch-revolve")
        .arg(&input)
        .arg("-o")
        .arg(&source)
        .arg("--json")
        .output()
        .expect("create");
    assert!(p.status.success(), "{p:?}");
    let source_bytes = std::fs::read(&source).expect("source");
    let reading: crate::sketch_facts::Source = {
        let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
        ferritecad_scene::snapshot_of(
            &source,
            &mut k,
            |k, b| k.import_step(b),
            &Default::default(),
            &OperationContext::default(),
        )
        .expect("accepted scene")
        .edit_source
        .expect("accepted edit facts")
        .into()
    };
    assert_revolve_edit_builds(&source, &[[3., 0.], [10., 0.], [5., 12.5], [0., 12.5]]);
    let literal = [[0., 0.], [10., 0.], [5., 12.5], [0., 12.5]];
    let (mut window, request, expected) =
        crate::sketch_mode::tests::curve_edits::polygon(&source, &literal);
    assert_eq!(request.expected, reading.version);
    assert!(crate::sketch_mode::tests::window::apply(
        &mut window,
        request
    ));
    let ui = root.path().join("ui.fcad");
    assert!(window.save(ferritecad_jobs::SaveTarget::As(ui)).published);

    // The peer CLI, the same request, the same source.
    let edit = root.path().join("edit.json");

    std::fs::write(
        &edit,
        ferritecad_fixtures::edit_request_json(&expected).to_string(),
    )
    .expect("edit request");
    let ui = root.path().join("ui.fcad");
    let peer = root.path().join("cli.fcad");
    let p = std::process::Command::new(ferritecad())
        .arg("edit")
        .arg(&source)
        .arg("--expect-version")
        .arg(reading.version.content.to_string())
        .arg("--request")
        .arg(&edit)
        .arg("-o")
        .arg(&peer)
        .arg("--json")
        .output()
        .expect("peer CLI");
    assert!(p.status.success(), "{p:?}");
    assert_eq!(sql_facts(&ui), sql_facts(&peer), "every SQL cell");
    assert_eq!(read_semantics(&ui), read_semantics(&peer));
    let a = Document::open_read_only(&ui).expect("UI");
    let original = Document::open_read_only(&source).expect("source");
    assert_eq!(
        crate::test_names::names(&a),
        crate::test_names::names(&original),
        "no new names, both caps kept"
    );
    let revolve = |d: &Document| {
        d.objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Revolve(_)))
            .expect("Revolve")
    };
    assert_eq!(
        revolve(&a),
        revolve(&original),
        "the Revolve row, angle included"
    );
    let ObjectPayload::Revolve(r) = revolve(&a).payload else {
        panic!("a Revolve")
    };
    assert!(
        matches!(r.extent, RevolveExtent::Partial { angle } if angle.value().to_degrees() == 220.)
    );
    original.close().expect("close");
    // A frustum R = 10, r = 5, h = 12.5, turned through 220°.
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = ferritecad_eval::rebuild_cold(&a, &mut kernel, &context).expect("cold");
    let body = a
        .objects()
        .expect("objects")
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .map(|o| o.id)
        .expect("Body");
    let (faces, volume) = kernel
        .shape_stats(built.shape(body).expect("one solid"))
        .expect("stats");
    assert_eq!(
        faces, 5,
        "two discs, one cone, two caps, no face for the axis"
    );
    let expected = std::f64::consts::PI * 12.5 / 3. * (100. + 50. + 25.) * 220. / 360.;
    assert!(
        (volume - expected).abs() < 1e-9 * expected,
        "{volume} != {expected}"
    );
    built.release_all(&mut kernel);
    a.close().expect("close");
    let export = |path: &std::path::Path| {
        let mut bytes = Vec::new();
        for (op, extension) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
            let out = path.with_extension(extension);
            let p = std::process::Command::new(ferritecad())
                .arg(op)
                .arg(path)
                .arg("-o")
                .arg(&out)
                .output()
                .expect("export");
            assert!(p.status.success(), "{p:?}");
            bytes.push(std::fs::read(&out).expect("bytes"));
        }
        bytes
    };
    assert_eq!(export(&ui), export(&peer), "STL and FBX bytes, no mapping");
    assert_eq!(std::fs::read(&source).expect("source"), source_bytes);
}
