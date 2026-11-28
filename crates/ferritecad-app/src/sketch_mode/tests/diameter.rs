// SPDX-License-Identifier: MIT
use super::cli::{cli_edit, export};
use super::constraint_widgets::press_rule;
use super::dragging::{frame, movement, press, release};
use super::widgets::Ui;
use super::window::{click, enter, finish, open, publish};
use super::*;
use crate::sessions::tests::tabs::{Drawn, Window};
use ferritecad_document::{Document, Expression, ObjectPayload};
use ferritecad_kernel::OperationContext;
use ferritecad_ui::ModelEdit;
use std::path::{Path, PathBuf};

pub(super) const CENTER: [f64; 2] = [-3.5, 4.3];

pub(super) fn move_centers(w: &mut Window, ui: &mut Ui, count: usize) {
    ui.press(w, "Off", false);
    ui.press(w, "0.1 mm", false);
    let old = point(12., -7.);
    let center = point(CENTER[0], CENTER[1]);
    for _ in 0..count {
        press(w, old);
        assert!(w.creates.sketch.viewport.gesturing());
        movement(w, center);
        frame(w);
        release(w);
    }
}

fn set_diameters(path: &Path, sketch: ObjectId, diameters: [f64; 2]) -> Window {
    let mut w = open(path, Drawn::Native);
    w.input
        .frame(([-30., -30., -20.], [33., 32., 20.]))
        .expect("ring in view");
    enter(&mut w, sketch, ModelEdit::Sketch);
    let mut ui = Ui::new(&mut w);
    move_centers(&mut w, &mut ui, 2);
    let center = point(CENTER[0], CENTER[1]);
    for (saved, diameter) in [4., 10.].into_iter().zip(diameters) {
        click(&mut w, point(center.x, center.y + saved), false);
        press_rule(&mut ui, &mut w, "Diameter");
        ui.type_value(&mut w, &(saved * 2.).to_string(), &diameter.to_string());
        ui.press(&mut w, "Set", false);
    }
    click(&mut w, center, false);
    click(&mut w, center, true);
    press_rule(&mut ui, &mut w, "Coincident");
    // Both centres coincide. Shift twice leaves only the second, boundary centre selected.
    click(&mut w, center, false);
    click(&mut w, center, true);
    click(&mut w, center, true);
    press_rule(&mut ui, &mut w, "Fix");
    let request = finish(&mut w, &mut ui);
    assert!(
        request
            .changes
            .iter()
            .all(|c| matches!(c, EditChange::AddConstraint { .. }))
    );
    assert_eq!(request.changes.len(), 4);
    publish(&mut w, request);
    w
}

pub(super) fn compare_circles(prediction: &Sketch, shown: &Sketch) {
    assert_eq!(prediction.curves.len(), shown.curves.len());
    for (predicted, visible) in prediction.curves.iter().zip(&shown.curves) {
        let (
            SketchGeometry::Circle {
                center: a,
                radius: r,
            },
            SketchGeometry::Circle {
                center: b,
                radius: s,
            },
        ) = (&predicted.geometry, &visible.geometry)
        else {
            panic!("two circles")
        };
        assert_eq!(predicted.id, visible.id);
        assert!((a.x - b.x).abs() <= 1e-6 && (a.y - b.y).abs() <= 1e-6 && (r - s).abs() <= 1e-6);
    }
}

/// A fully constrained ring has one solution from both the shown and the stored seeds.
/// Compare each seed through the public solver before sending the four rules to the CLI.
fn reference(sketch: ObjectId, saved: &Sketch, diameters: [f64; 2]) -> Vec<EditChange> {
    let ids = [saved.curves[0].id, saved.curves[1].id];
    let centers = ids.map(|curve| SketchPointRef::new(curve, SketchPointSelector::Center));
    let literal = |n| Expression::constant(n).expect("dimension");
    let rules = [
        SketchConstraintRule::Radius {
            curve: ids[0],
            radius: literal(diameters[0] / 2.),
        },
        SketchConstraintRule::Radius {
            curve: ids[1],
            radius: literal(diameters[1] / 2.),
        },
        SketchConstraintRule::Coincident {
            a: centers[0],
            b: centers[1],
        },
        SketchConstraintRule::Fixed {
            point: centers[1],
            x: literal(CENTER[0]),
            y: literal(CENTER[1]),
        },
    ]
    .map(|rule| SketchConstraint {
        id: StableEntityId::new(),
        rule,
    });
    assert!(
        ferritecad_eval::solve_sketch(saved, sketch)
            .expect("unconstrained seed")
            .is_none()
    );
    let solve = |s: &Sketch| {
        ferritecad_eval::solve_sketch(s, sketch)
            .expect("consistent ring")
            .expect("constrained")
            .0
    };
    let mut shown = saved.clone();
    for curve in &mut shown.curves {
        let SketchGeometry::Circle { center, .. } = &mut curve.geometry else {
            panic!("circle")
        };
        *center = point(CENTER[0], CENTER[1]);
    }
    for rule in &rules {
        shown.constraints.push(rule.clone());
        shown = solve(&shown);
    }
    let stored = Sketch {
        constraints: rules.to_vec(),
        ..saved.clone()
    };
    let prediction = solve(&stored);
    compare_circles(&prediction, &shown);
    rules
        .into_iter()
        .map(|constraint| EditChange::AddConstraint { sketch, constraint })
        .collect()
}

#[test]
fn native_viewport_diameters_and_cli_publish_the_same_radii_ids_sql_and_solid() {
    use crate::creates::tests::ferritecad;
    type Rows = Vec<Vec<rusqlite::types::Value>>;
    use ferritecad_kernel::FaceSurface;
    // Each missing piece is excused only where that piece is not required: a build
    // with the kernel and no solver skips here and still runs the kernel-only gates.
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_sketch_solver::is_available(), "PLANEGCS"),
    ]) {
        return;
    }
    // The ring carries data this editor knows nothing about (an attached source
    // claim and an extension table): it has to come through.
    let root = tempfile::tempdir().expect("dir");
    let source_path = root.path().join("round.fcad");
    crate::feature_tests::write_round(&source_path, [12., -7.], &[4., 10.], 15., false);
    crate::sketch::drag_tests::attach_source_claim(&source_path);
    let doc = Document::open_read_only(&source_path).expect("ring");
    let (sketch, saved) = ferritecad_fixtures::sketch(&doc).expect("sketch");
    let (bore, boundary) = (saved.curves[0].id, saved.curves[1].id);
    assert_eq!(
        ferritecad_document::constraint_circle_roles(&saved),
        Some((boundary, bore))
    );
    doc.close().expect("close");
    let before = std::fs::read(&source_path).expect("source bytes");
    let w = set_diameters(&source_path, sketch, [4.25, 13.5]);
    let ui = w.sessions.export_path().expect("accepted ring");
    assert!(!w.creates.sketch.viewport.active());
    let swapped = set_diameters(&source_path, sketch, [13.5, 4.25]);
    let crossed = swapped
        .sessions
        .export_path()
        .expect("accepted crossed ring");
    assert!(!swapped.creates.sketch.viewport.active());
    let d = Document::open_read_only(&crossed).expect("crossed");
    let (_, drawn) = ferritecad_fixtures::sketch(&d).expect("circles");
    assert!(drawn.curves.iter().any(|c| c.id == bore));
    assert!(drawn.curves.iter().any(|c| c.id == boundary));
    d.close().expect("close");
    let doc = Document::open_read_only(&source_path).expect("source");
    let v = ferritecad_document::ExtrudeEditSource::read(&doc)
        .expect("version")
        .version;
    doc.close().expect("close");
    let cli = root.path().join("diameters.fcad");
    let crossed_cli = root.path().join("crossed-cli.fcad");
    for (path, peer, values) in [
        (&ui, &cli, [4.25, 13.5]),
        (&crossed, &crossed_cli, [13.5, 4.25]),
    ] {
        cli_edit(&source_path, peer, v, &reference(sketch, &saved, values));
        for file in [path, peer] {
            assert_eq!(
                crate::transaction_tests::solve_path(file)
                    .expect("solve")
                    .degrees_of_freedom(),
                0
            );
        }
        for format in ["stl", "fbx"] {
            assert_eq!(export(path, format), export(peer, format));
        }
    }
    let run = |args: &[&std::ffi::OsStr]| {
        let out = std::process::Command::new(ferritecad())
            .args(args)
            .output()
            .expect("peer CLI");
        assert!(out.status.success(), "{out:?}");
        out
    };
    assert_eq!(std::fs::read(&source_path).expect("source"), before);

    // Same Radius per UUID; the only difference is the freshly minted constraint
    // UUIDs, matched here by the rule they carry, once, explicitly.
    let sketch_of = |path: &Path| {
        let d = Document::open_read_only(path).expect("copy");
        let objects = d.objects().expect("objects");
        let row = objects.iter().find(|o| o.id == sketch).expect("sketch");
        let ObjectPayload::Sketch(s) = row.payload.clone() else {
            panic!("a Sketch")
        };
        let others: Vec<_> = objects.iter().filter(|o| o.id != sketch).cloned().collect();
        let facts = (
            d.meta().document_id,
            others,
            d.dependencies().expect("dependencies"),
            crate::test_names::names(&d),
        );
        d.close().expect("close");
        (s, facts)
    };
    let (ui_sketch, ui_facts) = sketch_of(&ui);
    let (cli_sketch, cli_facts) = sketch_of(&cli);
    let (source_sketch, source_facts) = sketch_of(&source_path);
    assert_eq!(
        ui_facts, cli_facts,
        "objects, links and refs of the two copies"
    );
    assert_eq!(
        ui_facts.1, source_facts.1,
        "every other object is the source's"
    );
    assert_eq!(
        ui_facts.3, source_facts.3,
        "the stored references are the source's"
    );
    assert_eq!(ui_sketch.curves, source_sketch.curves, "the guess was kept");
    assert_eq!(cli_sketch.curves, source_sketch.curves);
    let by_rule = |s: &Sketch| {
        let mut v: Vec<_> = s
            .constraints
            .iter()
            .map(|c| (format!("{:?}", c.rule), c.id))
            .collect();
        v.sort();
        v
    };
    let (u, c) = (by_rule(&ui_sketch), by_rule(&cli_sketch));
    assert_eq!(u.len(), 4);
    assert_eq!(
        u.iter().map(|x| &x.0).collect::<Vec<_>>(),
        c.iter().map(|x| &x.0).collect::<Vec<_>>(),
        "the same four rules"
    );
    for (left, right) in u.iter().zip(&c) {
        assert_ne!(left.1, right.1, "each publication minted its own UUID");
    }
    let radii: std::collections::BTreeMap<_, _> = ui_sketch
        .constraints
        .iter()
        .filter_map(|c| match c.rule.clone() {
            SketchConstraintRule::Radius { curve, radius } => Some((curve, radius.value())),
            _ => None,
        })
        .collect();
    assert_eq!(radii, [(bore, 2.125), (boundary, 6.75)].into());

    // Every SQL cell is literally the other file's, but the Sketch row's payload
    // and hash (where the new UUIDs live) and the modified timestamp.
    let (a, b, s) = (
        crate::fillets::tests::tables(&ui),
        crate::fillets::tests::tables(&cli),
        crate::fillets::tests::tables(&source_path),
    );
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    for table in [
        "meta",
        "objects",
        "imported_sources",
        "imported_source_refs",
        "extension",
    ] {
        assert!(
            !a[table].1.is_empty(),
            "{table} must participate in the comparison"
        );
    }
    let selected = rusqlite::types::Value::Blob(sketch.to_bytes().to_vec());
    let mut differing = Vec::new();
    for (table, (columns, rows)) in &a {
        assert_eq!(rows.len(), b[table].1.len(), "{table}");
        for (x, y) in rows.iter().zip(&b[table].1) {
            for (i, cell) in x.iter().enumerate() {
                if cell == &y[i] {
                    continue;
                }
                let column = columns[i].as_str();
                let allowed = match table.as_str() {
                    "objects" => {
                        x.contains(&selected) && matches!(column, "payload" | "payload_hash")
                    }
                    "meta" => column == "modified_at",
                    _ => false,
                };
                assert!(allowed, "{table}.{column} differs between the two copies");
                differing.push(format!("{table}.{column}"));
            }
        }
    }
    assert!(
        ["objects.payload", "objects.payload_hash"]
            .iter()
            .all(|c| differing.iter().any(|d| d == c)),
        "the minted UUIDs must have differed: {differing:?}"
    );
    // And the copy differs from the source in the Sketch row (payload, hash)
    // and the timestamp;
    // every other table, and every other object row, is the source's.
    for (table, (_, rows)) in &s {
        match table.as_str() {
            "meta" => {}
            "objects" => {
                let rest = |rows: &Rows| {
                    rows.iter()
                        .filter(|r| !r.contains(&selected))
                        .cloned()
                        .collect::<Vec<_>>()
                };
                assert_eq!(rest(&a[table].1), rest(rows), "another object changed");
            }
            _ => assert_eq!(a[table].1, *rows, "{table} was not preserved"),
        }
    }

    // Reopen and rebuild cold: the kernel's own faces, under the right references.
    let analytic = |path: &Path| {
        let doc = Document::open_read_only(path).expect("reopen");
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        let built = ferritecad_eval::rebuild_cold(&doc, &mut kernel, &OperationContext::default())
            .expect("cold rebuild after reopen");
        assert_eq!(built.shape_count(), 1);
        let mut walls = std::collections::BTreeMap::new();
        let mut volume = None;
        for (input, face) in crate::test_names::root_faces(&doc, &built) {
            if let ferritecad_kernel::NamingInput::Curve(curve) = input {
                walls.insert(curve, kernel.face_surface(face).expect("surface"));
            }
            let stats = kernel.shape_stats(face.shape()).expect("stats");
            assert_eq!(stats.0, 4, "two walls and two caps");
            volume = Some(stats.1);
        }
        built.release_all(&mut kernel);
        doc.close().expect("close");
        (walls, volume.expect("a resolved reference"))
    };
    let (outer, inner, height) = (6.75, 2.125, 15.);
    let exact = std::f64::consts::PI * (outer - inner) * (outer + inner) * height;
    for path in [&ui, &cli] {
        let (walls, volume) = analytic(path);
        assert_eq!(
            walls,
            [
                (boundary, FaceSurface::Cylinder { radius: outer }),
                (bore, FaceSurface::Cylinder { radius: inner }),
            ]
            .into(),
            "each wall under its own circle's reference"
        );
        assert!((volume - exact).abs() < 1e-6 * exact, "{volume} vs {exact}");
        let rebuilt = run(&["rebuild".as_ref(), path.as_os_str(), "--cold".as_ref()]);
        let text = String::from_utf8(rebuilt.stdout).expect("UTF-8");
        assert!(
            text.lines()
                .filter(|line| line.contains(" kind=face "))
                .count()
                == 4,
            "{text}"
        );
    }

    // The exported mesh, read here: a closed oriented surface whose rims sit at the
    // two solved radii about the solved centre. A mesh is a prism inscribed in a
    // circle, so it is bounded by the analytic solid, not compared with it for pi.
    let (stl, fbx) = (ui.with_extension("stl"), ui.with_extension("fbx"));
    run(&[
        "export-stl".as_ref(),
        ui.as_os_str(),
        "-o".as_ref(),
        stl.as_os_str(),
        "--linear-deflection".as_ref(),
        "0.05".as_ref(),
        "--angular-deflection".as_ref(),
        "0.1".as_ref(),
    ]);
    run(&[
        "export-fbx".as_ref(),
        ui.as_os_str(),
        "-o".as_ref(),
        fbx.as_os_str(),
    ]);
    let (faces, volume) = crate::feature_tests::read_stl(&std::fs::read(&stl).expect("STL"));
    let q = |v: [f64; 3]| v.map(|x| (x * 1e4).round() as i64);
    let mut directed = std::collections::BTreeMap::<_, usize>::new();
    for f in &faces {
        for k in 0..3 {
            *directed.entry((q(f[k]), q(f[(k + 1) % 3]))).or_default() += 1;
        }
    }
    assert!(
        directed.values().all(|n| *n == 1),
        "not one oriented surface"
    );
    assert!(
        directed
            .keys()
            .all(|(a, b)| directed.contains_key(&(*b, *a)))
    );
    let (centre, delta) = (CENTER, 0.05 + 1e-4);
    let on = |radius: f64| {
        faces
            .iter()
            .filter(|f| {
                f.iter()
                    .all(|p| ((p[0] - centre[0]).hypot(p[1] - centre[1]) - radius).abs() < 1e-4)
            })
            .count()
    };
    assert!(on(outer) >= 12 && on(inner) >= 12, "a wall is missing");
    let pi = std::f64::consts::PI;
    assert!(
        volume >= pi * ((outer - delta).powi(2) - inner * inner) * height
            && volume <= pi * (outer * outer - (inner - delta).powi(2)) * height,
        "mesh volume {volume}"
    );
    assert!(
        faces
            .iter()
            .all(|f| f.iter().all(|p| p[2] >= -1e-4 && p[2] <= height + 1e-4))
    );
    if let Some(dir) = std::env::var_os("FCAD_DIAMETER_ARTIFACT_DIR") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("artifact directory");
        std::fs::copy(&fbx, dir.join("diameter.fbx")).expect("FBX for the pinned reader");
    }
}
