// SPDX-License-Identifier: MIT
//! §31A: real projection, tab/form owners and egui; no window or device.
use super::*;
use crate::model_tree::{actions, availability, open};
use ferritecad_document::{Dependency, DependencyRole, ModelTree, ObjectPayload};
use ferritecad_ui::{ModelAction, ModelEdit, ModelKey};

fn source(w: &Window) -> &ferritecad_document::ExtrudeEditSource {
    w.scene.edit_source.as_ref().expect("accepted catalogue")
}
fn key(w: &Window, object: ObjectId) -> ModelKey {
    source(w)
        .model_tree
        .rows
        .iter()
        .find(|r| r.id.0 == object)
        .expect("UUID row")
        .id
}
fn choose(w: &mut Window, object: ObjectId, edit: ModelEdit) -> ModelAction {
    let row = key(w, object);
    let tab = w.sessions.tab();
    let state = w.tabs.model_navigation(tab);
    state.selected = Some(row);
    ModelAction {
        tab: tab.key(),
        epoch: state.epoch,
        row,
        edit,
    }
}
fn open_action(w: &mut Window, asked: ModelAction) -> bool {
    let reading = source(w).clone();
    open(
        asked,
        &reading,
        &mut w.creates,
        &mut w.edits,
        &w.sessions,
        &w.tabs,
        &Loads::default(),
        &Exports::default(),
        &w.input,
    )
}

// Kernel-free facts arrival seam: an explicitly empty render snapshot. This is
// not geometry evidence; native tests below use the production scene worker.
fn facts_scene(path: &Path) -> Result<LoadedScene> {
    Ok(LoadedScene {
        edit_source: Some(read_extrude_source(path)?),
        snapshot: ferritecad_viewport::SnapshotBuilder::new().build(),
        catalogue: vec![],
        faces: Default::default(),
        edges: Default::default(),
        vertices: Default::default(),
        sketch_solves: vec![],
        sketch_presentations: vec![],
    })
}
fn open_facts(w: &mut Window, path: &Path) {
    let session = DocumentSession::open_in(w.private.path(), path, HistoryLimits::default())
        .expect("session");
    let path = session.current().path().to_path_buf();
    w.present(
        &path,
        facts_scene(&path),
        Bind::Open(Box::new(session)),
        Ok(()),
    )
    .expect("facts accepted");
}
fn switch_facts(w: &mut Window, tab: TabId) {
    let (tx, rx) = mpsc::channel();
    let generation = w
        .tabs
        .begin_switch(&mut w.sessions, tab, None, move |path, _, _| {
            std::thread::spawn(move || {
                tx.send(facts_scene(&path)).expect("facts");
            })
        })
        .expect("switch");
    let (outcome, _) = w
        .deliver_switch(generation, wait(&rx), Ok(()))
        .expect("awaited");
    outcome.expect("accepted");
}

#[test]
fn history_uses_dependencies_exact_uuids_and_honest_shared_references() {
    let (_root, path, reading) = crate::cuts::tests::session_apply::fixture(16);
    let d = Document::open_read_only(&path).expect("document");
    let mut objects = d.objects().expect("objects");
    let mut deps = d.dependencies().expect("dependencies");
    let saved = crate::cuts::tests::session_apply::selected(&reading, 0);
    let tree = &reading.model_tree;
    let chain: Vec<_> = tree
        .rows
        .iter()
        .filter(|r| r.depth == 1)
        .map(|r| r.id.0)
        .collect();
    assert_eq!(
        chain,
        std::iter::once(saved.base_feature)
            .chain(saved.tools.iter().map(|t| t.feature))
            .collect::<Vec<_>>(),
        "history must ignore SQL order and identical names"
    );
    assert_eq!(tree.rows.iter().filter(|r| r.kind == "Cut").count(), 16);
    assert!(tree.rows.iter().all(|r| r.name == "same"));
    // A second Body shares the same chain. Its occurrences are references,
    // while two different features also read exactly the same profile.
    let mut second = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .expect("Body")
        .clone();
    let first_body = second.id;
    second.id = ObjectId::new();
    let second_body = second.id;
    objects.push(second);
    let tip = deps
        .iter()
        .find(|d| d.dependent == first_body && d.role == DependencyRole::BodyTip)
        .expect("tip")
        .dependency;
    deps.push(Dependency {
        dependent: second_body,
        dependency: tip,
        role: DependencyRole::BodyTip,
    });
    let profile = deps
        .iter()
        .find(|d| d.dependent == saved.base_feature && d.role == DependencyRole::Profile)
        .expect("profile")
        .dependency;
    deps.retain(|d| !(d.dependent == saved.feature && d.role == DependencyRole::Profile));
    deps.push(Dependency {
        dependent: saved.feature,
        dependency: profile,
        role: DependencyRole::Profile,
    });
    // An unknown envelope remains readable, but has no fabricated editor.
    let mut unknown = objects[0].clone();
    unknown.id = ObjectId::new();
    unknown.payload = ObjectPayload::from_storage_bytes(
        &ferritecad_document::Envelope::encode("future.widget", 77, vec![], &42u32)
            .expect("envelope")
            .to_bytes()
            .expect("bytes"),
    )
    .expect("unknown");
    let unknown_id = unknown.id;
    objects.push(unknown);
    objects.reverse();
    deps.reverse();
    let tree = ModelTree::project(&objects, &deps);
    let keys: std::collections::HashSet<_> = tree.rows.iter().map(|r| r.id).collect();
    assert_eq!(
        keys.len(),
        tree.rows.len(),
        "shared DAG occurrences must have distinct identities"
    );
    assert_eq!(
        tree.rows
            .iter()
            .filter(|r| r.id.0 == profile && r.depth == 2)
            .count(),
        4
    );
    assert!(
        tree.rows
            .iter()
            .filter(|r| r.depth > 0)
            .all(|r| r.note.contains("reference"))
    );
    assert!(tree.rows.iter().any(|r| r.id.0 == unknown_id
        && r.kind == "future.widget"
        && r.note.contains("no editor")));
    let unavailable = ModelTree::from_dependencies(
        &objects,
        Err(CadError::unsupported("future dependency role")),
    );
    assert_eq!(unavailable.rows.len(), objects.len());
    assert!(
        unavailable
            .rows
            .iter()
            .all(|r| r.depth == 0 && r.note.contains("History unavailable"))
    );
    assert!(unavailable.rows.iter().any(|r| r.id.0 == unknown_id));
    println!("\nFCAD_31A_PROJECTION_EXECUTED cuts=16");
}

#[test]
fn accepted_snapshot_tab_identity_stale_actions_and_form_history_are_preserved() {
    let (root, path, reading) = crate::cuts::tests::session_apply::fixture(3);
    let copy = root.path().join("copy.fcad");
    std::fs::copy(&path, &copy).expect("copy");
    let mut w = Window::new(Drawn::Mock, None);
    open_facts(&mut w, &path);
    let a = w.sessions.tab();
    let saved = crate::cuts::tests::session_apply::selected(&reading, 0);
    let early = saved.feature;
    let action = choose(&mut w, early, ModelEdit::Cut);
    w.tabs
        .model_navigation(a)
        .collapsed
        .insert((reading.cut_bodies[0].body, None, None));
    open_facts(&mut w, &copy);
    let b = w.sessions.tab();
    assert_ne!(a, b);
    assert_eq!(source(&w).version.document_id, reading.version.document_id);
    assert_eq!(w.tabs.model_navigation(b).selected, None);
    assert!(w.tabs.model_navigation(b).collapsed.is_empty());
    assert!(
        !open_action(&mut w, action),
        "foreign TabId must not open an identical UUID"
    );
    switch_facts(&mut w, a);
    assert_eq!(w.tabs.model_navigation(a).selected, Some(action.row));
    assert!(
        !open_action(&mut w, action),
        "old scene epoch must not open a form"
    );
    let action = choose(&mut w, early, ModelEdit::Cut);
    assert!(open_action(&mut w, action));
    // Exercise the existing form's real request Undo/Redo while navigation
    // changes. Opening another editor is refused without touching its fields.
    w.creates.sketch.cuts.set_session(true, true, false);
    let ctx = egui::Context::default();
    for _ in 0..3 {
        crate::cuts::tests::frame(&ctx, &mut w.creates.sketch.cuts, false);
    }
    crate::cuts::tests::enter_field(&ctx, &mut w.creates.sketch.cuts, "Radius (mm):", "1.625");
    crate::cuts::tests::click(&ctx, &mut w.creates.sketch.cuts, "Confirm draft numbers");
    let before = crate::cuts::tests::session_apply::draft_state(&w.creates.sketch);
    let other = choose(&mut w, saved.base_feature, ModelEdit::Height);
    assert!(
        availability(
            &w.creates,
            &w.edits,
            &w.sessions,
            &w.tabs,
            &Loads::default(),
            &Exports::default(),
            &w.input
        )
        .expect_err("open form must hold navigation editing")
        .contains("Apply or Cancel")
    );
    assert!(!open_action(&mut w, other));
    assert_eq!(
        crate::cuts::tests::session_apply::draft_state(&w.creates.sketch),
        before
    );
    crate::cuts::tests::click(&ctx, &mut w.creates.sketch.cuts, "Undo");
    assert_ne!(
        crate::cuts::tests::session_apply::draft_state(&w.creates.sketch),
        before
    );
    crate::cuts::tests::click(&ctx, &mut w.creates.sketch.cuts, "Redo");
    assert_eq!(
        crate::cuts::tests::session_apply::draft_state(&w.creates.sketch),
        before
    );
    crate::cuts::tests::click(&ctx, &mut w.creates.sketch.cuts, "Apply cut");
    assert_eq!(
        w.creates
            .sketch
            .cuts
            .take_apply_request()
            .expect("actual editor request")
            .cut,
        early,
        "navigation must open exactly the selected UUID"
    );
    switch_facts(&mut w, b);
    switch_facts(&mut w, a);
    assert_eq!(
        crate::cuts::tests::session_apply::draft_state(&w.creates.sketch),
        before
    );
    w.creates.sketch.dismiss();
    // New keeps the same refusal.
    assert!(w.begin_new(false));
    assert!(!open_action(&mut w, other));
    w.answer_new(ferritecad_ui::NewChoice::Cancel);
    let current = source(&w).clone();
    let selected = w.tabs.model_navigation(a).selected;
    let candidate = DocumentSession::open(&copy).expect("candidate");
    let staged = candidate.current().path().to_path_buf();
    let loaded = facts_scene(&staged);
    assert!(
        w.present(
            &staged,
            loaded,
            Bind::Open(Box::new(candidate)),
            Err(CadError::unsupported("device refusal"))
        )
        .is_err()
    );
    assert_eq!(source(&w), &current);
    assert_eq!(w.tabs.model_navigation(a).selected, selected);
    // A different accepted model replaces the projection as one arrival;
    // the prior refused arrival preserved it exactly.
    let plate_path = root.path().join("plate.fcad");
    plate(&plate_path, 12.);
    w.open(&plate_path);
    let accepted_id = source(&w).features[0].feature;
    assert!(
        source(&w)
            .model_tree
            .rows
            .iter()
            .any(|r| r.id.0 == accepted_id),
        "tree must follow the accepted snapshot, never the old file"
    );
    let current_tab = w.sessions.tab();
    choose(&mut w, accepted_id, ModelEdit::Height);
    w.apply(17.25);
    w.step(true);
    w.step(false);
    assert_eq!(source(&w).features[0].distance_mm, Some(17.25));
    assert!(!open_action(&mut w, other));
    let selected = w.tabs.model_navigation(current_tab).selected;
    let mut vanished = source(&w).clone();
    vanished.model_tree.rows.retain(|r| Some(r.id) != selected);
    w.tabs.model_arrived(&w.sessions, Some(&vanished));
    assert_eq!(
        w.tabs.model_navigation(current_tab).selected,
        None,
        "vanished object must not choose its neighbour"
    );
    println!("\nFCAD_31A_OWNERS_EXECUTED");
}

#[test]
fn navigation_open_uses_one_foreground_gesture_and_new_refusal() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("a.fcad");
    plate(&path, 12.);
    let mut w = Window::new(Drawn::Mock, None);
    w.open(&path);
    let feature = w.feature();
    let asked = choose(&mut w, feature, ModelEdit::Height);
    let g = w
        .sessions
        .begin_apply(|_, _, _| std::thread::spawn(|| {}))
        .expect("operation");
    assert!(!open_action(&mut w, asked));
    assert!(!w.edits.form_open());
    w.sessions.finish_apply(g, Err(CadError::Cancelled));
    w.input
        .handle(ViewportEvent::PointerPressed(PointerButton::Primary), false);
    assert!(!open_action(&mut w, asked));
    assert!(!w.edits.form_open());
    w.input.forget_pending();
    let mut loads = Loads::default();
    Window::begin_load(&mut loads, &path);
    assert!(
        availability(
            &w.creates,
            &w.edits,
            &w.sessions,
            &w.tabs,
            &loads,
            &Exports::default(),
            &w.input
        )
        .is_err()
    );
    loads.stop_all();
    assert!(w.begin_new(false));
    assert!(!open_action(&mut w, asked));
    w.answer_new(ferritecad_ui::NewChoice::Cancel);
    assert!(open_action(&mut w, asked));
    assert_eq!(w.edits.typed(), Some((Some(feature), "12")));
    println!("\nFCAD_31A_GUARDS_EXECUTED");
}

fn widget_frame(
    w: &mut Window,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Option<ModelAction>) {
    let reading = source(w).clone();
    let rows: Vec<_> = reading
        .model_tree
        .rows
        .iter()
        .map(|r| ferritecad_ui::ModelRow {
            key: r.id,
            parent: r.parent,
            depth: r.depth,
            name: &r.name,
            kind: &r.kind,
            note: &r.note,
        })
        .collect();
    let available = availability(
        &w.creates,
        &w.edits,
        &w.sessions,
        &w.tabs,
        &Loads::default(),
        &Exports::default(),
        &w.input,
    );
    let mut asked = None;
    let tab = w.sessions.tab();
    let navigation = w.tabs.model_navigation(tab);
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100., 800.),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            ui.label("Toolbar and tabs");
            egui::Panel::left("model-panel")
                .default_size(260.)
                .size_range(180.0..=340.0)
                .show(ui, |ui| {
                    asked = ferritecad_ui::model_panel(
                        ui,
                        tab.key(),
                        &rows,
                        navigation,
                        |key| actions(&reading, key),
                        available,
                    );
                });
        },
    );
    output.textures_delta.clear();
    (output, asked)
}
fn text_rect(out: &egui::FullOutput, label: &str) -> egui::Rect {
    out.shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) if t.galley.text() == label => {
                Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing real widget {label}"))
}
fn click_widget(w: &mut Window, ctx: &egui::Context, at: egui::Pos2) -> Option<ModelAction> {
    widget_frame(
        w,
        ctx,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ],
    )
    .1
}
#[test]
fn real_widgets_select_uuid_then_send_the_supported_action_and_bound_the_list() {
    let (_root, path, reading) = crate::cuts::tests::session_apply::fixture(16);
    let mut w = Window::new(Drawn::Mock, None);
    open_facts(&mut w, &path);
    let ctx = egui::Context::default();
    let mut out = widget_frame(&mut w, &ctx, vec![]).0;
    for _ in 0..3 {
        out = widget_frame(&mut w, &ctx, vec![]).0;
    }
    // Stable UUID rows with identical names: select the first Cut by its
    // actual position in the dependency-derived list, then check its UUID.
    let cut_rects: Vec<_> = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t)
                if t.galley.text() == "Ref: same · Cut" && s.clip_rect.contains(t.pos) =>
            {
                Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
            }
            _ => None,
        })
        .collect();
    assert!(!cut_rects.is_empty());
    assert!(
        cut_rects.len() < 16,
        "16 Cuts must scroll instead of filling the screen"
    );
    assert!(cut_rects[0].top() > text_rect(&out, "Toolbar and tabs").bottom());
    assert!(click_widget(&mut w, &ctx, cut_rects[0].center()).is_none());
    let early = crate::cuts::tests::session_apply::selected(&reading, 0).feature;
    assert_eq!(
        w.tabs
            .model_navigation(w.sessions.tab())
            .selected
            .expect("selected")
            .0,
        early
    );
    out = widget_frame(&mut w, &ctx, vec![]).0;
    let asked =
        click_widget(&mut w, &ctx, text_rect(&out, "Edit Cut…").center()).expect("explicit action");
    assert_eq!(asked.row.0, early);
    assert_eq!(asked.edit, ModelEdit::Cut);
    assert!(open_action(&mut w, asked));
    out = widget_frame(&mut w, &ctx, vec![]).0;
    assert!(
        click_widget(&mut w, &ctx, text_rect(&out, "Edit Cut…").center()).is_none(),
        "open form disables action"
    );
    println!("\nFCAD_31A_WIDGETS_EXECUTED");
}

#[test]
fn native_early_cut_navigation_apply_history_save_matches_existing_cli() {
    if !native() {
        return;
    }
    let (root, path, reading) = crate::cuts::tests::session_apply::fixture(3);
    let original = std::fs::read(&path).expect("original");
    let mut w = Window::new(Drawn::Native, None);
    w.open(&path);
    let early = crate::cuts::tests::session_apply::selected(&reading, 0).feature;
    let asked = choose(&mut w, early, ModelEdit::Cut);
    assert!(open_action(&mut w, asked));
    let ready = crate::can_apply_cut(
        &w.creates,
        &Loads::default(),
        &Exports::default(),
        &w.edits,
        &w.sessions,
    );
    w.creates.sketch.cuts.set_session(true, ready, false);
    let ctx = egui::Context::default();
    for _ in 0..3 {
        crate::cuts::tests::frame(&ctx, &mut w.creates.sketch.cuts, false);
    }
    crate::cuts::tests::enter_field(&ctx, &mut w.creates.sketch.cuts, "Radius (mm):", "1.625");
    crate::cuts::tests::click(&ctx, &mut w.creates.sketch.cuts, "Apply cut");
    let r = w
        .creates
        .sketch
        .cuts
        .take_apply_request()
        .expect("real editor request");
    assert_eq!(r.cut, early);
    assert_eq!(r.expected, source(&w).version);
    let request = r.clone();
    w.accept(move |ticket, cancel, tx| {
        spawn_apply_cut(
            ticket,
            request.cut,
            request.edit,
            request.expected,
            cancel.clone(),
            move |v| tx.send(v).expect("edit"),
        )
    });
    let accepted = w.sessions.export_path().expect("accepted");
    assert_eq!(
        source(&w).version,
        w.sessions.export_source().expect("snapshot").version()
    );
    let now = crate::cuts::tests::session_apply::selected(source(&w), 0);
    assert_eq!(now.radius_mm, 1.625);
    assert_eq!(
        w.tabs.model_navigation(w.sessions.tab()).selected,
        Some(asked.row)
    );
    let peer = root.path().join("cli.fcad");
    crate::sessions::tests::cut::peer(&path, &r, root.path(), &peer);
    crate::sessions::tests::cut::same_sql(&accepted, &peer, &path);
    let exports = export_bytes(&accepted, &path, root.path(), "unsaved");
    assert_eq!(exports, peer_bytes(&peer, root.path(), "cli"));
    let volume = crate::sessions::tests::cut::cold(&accepted);
    assert_eq!(volume, crate::sessions::tests::cut::cold(&peer));
    assert_eq!(std::fs::read(&path).expect("unsaved source"), original);
    w.step(true);
    export_bytes(
        &w.sessions.export_path().expect("Undo"),
        &path,
        root.path(),
        "undo",
    );
    assert_eq!(
        crate::cuts::tests::session_apply::selected(source(&w), 0).radius_mm,
        1.375
    );
    assert!(!open_action(&mut w, asked), "stale pre-Apply answer");
    w.step(false);
    assert_eq!(
        crate::cuts::tests::session_apply::selected(source(&w), 0).radius_mm,
        1.625
    );
    assert!(w.save(SaveTarget::InPlace).published);
    let before = root.path().join("original.fcad");
    std::fs::write(&before, &original).expect("original copy");
    crate::sessions::tests::cut::same_sql(&path, &peer, &before);
    if let Some(dir) = std::env::var_os("FCAD_31A_ARTIFACTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("artifacts");
        std::fs::write(dir.join("early-cut.stl"), exports.0).expect("STL");
        std::fs::write(dir.join("early-cut.fbx"), exports.1).expect("FBX");
    }
    compare_cut_files(
        &before,
        &path,
        &root.path().join("unsaved.stl"),
        &root.path().join("unsaved.fbx"),
        &root.path().join("undo.stl"),
    );
    // Negative controls use copies of actual headless owner outputs. These
    // are not passed off as GUI evidence and never fill a window output.
    let damaged = root.path().join("damaged.fcad");
    std::fs::copy(&path, &damaged).expect("copy");
    rusqlite::Connection::open(&damaged)
        .expect("SQL")
        .execute("UPDATE objects SET name='damaged'", [])
        .expect("alter");
    let compare = |saved: &Path, stl: &Path, fbx: &Path| {
        compare_cut_files(&before, saved, stl, fbx, &root.path().join("undo.stl"))
    };
    let catches = |f: &dyn Fn()| {
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err(),
            "corrupt output accepted"
        )
    };
    let peers = cli_runs();
    catches(&|| {
        compare(
            &path,
            &root.path().join("unsaved.stl"),
            &root.path().join("absent.fbx"),
        )
    });
    assert_eq!(cli_runs(), peers, "peer ran before preflight");
    catches(&|| {
        compare(
            &damaged,
            &root.path().join("unsaved.stl"),
            &root.path().join("unsaved.fbx"),
        )
    });
    catches(&|| {
        compare(
            &path,
            &root.path().join("undo.stl"),
            &root.path().join("unsaved.fbx"),
        )
    });
    w.checkpoint(CheckpointAction::Create("three cuts".to_owned()));
    let checkpoint = w.listed()[0].id;
    let reading = source(&w).clone();
    let body = reading.cut_bodies[0].body;
    let expected = reading.version;
    w.accept(move |ticket, cancel, tx| {
        crate::sessions::spawn_add_cut(
            ticket,
            body,
            ferritecad_document::CircularCut {
                center_mm: [20., 20.],
                radius_mm: 1.,
                extent: ferritecad_document::CutExtent::ThroughAll,
            },
            expected,
            cancel.clone(),
            move |v| tx.send(v).expect("Add"),
        )
    });
    let count = |w: &Window| {
        source(w)
            .model_tree
            .rows
            .iter()
            .filter(|r| r.kind == "Cut")
            .count()
    };
    assert_eq!(
        count(&w),
        4,
        "accepted Add must update the tree, never retain the saved projection"
    );
    let added = source(&w)
        .model_tree
        .rows
        .iter()
        .rfind(|r| r.kind == "Cut")
        .expect("last")
        .id
        .0;
    choose(&mut w, added, ModelEdit::Cut);
    w.step(true);
    assert_eq!(count(&w), 3);
    assert_eq!(w.tabs.model_navigation(w.sessions.tab()).selected, None);
    w.step(false);
    assert_eq!(count(&w), 4);
    w.checkpoint(CheckpointAction::Restore(checkpoint));
    assert_eq!(count(&w), 3);
    println!("early_uuid={early} volume={volume:.6}\nFCAD_31A_NATIVE_EXECUTED all_SQL_cells=true");
}

#[test]
fn native_fillet_chamfer_revolve_and_sketch_open_their_distinct_existing_routes() {
    if !native() {
        return;
    }
    let (fr, fp, fs) = crate::fillets::tests::session_apply::fixture(2);
    let (cr, cp, cs) = crate::chamfers::tests::session_apply::fixture();
    let rr = tempfile::tempdir().expect("root");
    let rp = rr.path().join("revolve.fcad");
    let revolve = crate::constraints::tests::revolve::write_sector(
        &rp,
        &[[2., 0.], [8., 0.], [8., 10.], [2., 10.]],
        210.,
        None,
    );
    let rs = read_extrude_source(&rp).expect("reading");
    for (path, id, edit) in [
        (&fp, fs.fillet_features[0].feature, ModelEdit::Fillet),
        (&cp, cs.chamfer_features[0].feature, ModelEdit::Chamfer),
        (&rp, revolve, ModelEdit::RevolveAngle),
        (&rp, rs.sketches[0].sketch, ModelEdit::Vertices),
        (
            &rp,
            rs.constraint_sketches[0].sketch,
            ModelEdit::Constraints,
        ),
    ] {
        let mut w = Window::new(Drawn::Native, None);
        w.open(path);
        let asked = choose(&mut w, id, edit);
        assert!(
            actions(source(&w), asked.row).contains(&edit),
            "typed route {edit:?}"
        );
        assert!(
            open_action(&mut w, asked),
            "must open actual {edit:?} owner"
        );
        assert!(w.creates.sketch.saved_object_open());
        assert!(
            !open_action(&mut w, asked),
            "duplicate must not replace the form"
        );
        w.creates.sketch.dismiss();
        assert!(open_action(&mut w, asked));
    }
    drop((fr, cr));
    println!("\nFCAD_31A_NATIVE_ROUTES_EXECUTED routes=5");
}

/// This comparison consumes existing output files; it creates only CLI peers
/// in a separate directory. It is shared by owner evidence and window review.
fn compare_cut_files(before: &Path, saved: &Path, stl: &Path, fbx: &Path, undo: &Path) {
    for path in [before, saved, stl, fbx, undo] {
        assert!(path.is_file(), "missing actual output: {}", path.display());
    }
    let peer_root = tempfile::tempdir().expect("peer");
    let reading = read_extrude_source(before).expect("source reading");
    let early = crate::cuts::tests::session_apply::selected(&reading, 0);
    let request = ferritecad_jobs::EditCircularCutRequest {
        source: before.to_path_buf(),
        expected: reading.version,
        cut: early.feature,
        edit: ferritecad_document::CircularCutEdit {
            tool_curve: early.tool_curve,
            center_mm: early.center_mm,
            radius_mm: 1.625,
            extent: early.extent,
            vocabulary: ferritecad_document::ExtentVocabulary::BlindOrThroughAll,
        },
        destination: PathBuf::new(),
    };
    let peer = peer_root.path().join("early.fcad");
    crate::sessions::tests::cut::peer(before, &request, peer_root.path(), &peer);
    crate::sessions::tests::cut::same_sql(saved, &peer, before);
    let actual = (
        std::fs::read(stl).expect("STL"),
        std::fs::read(fbx).expect("FBX"),
    );
    assert!(
        actual == peer_bytes(&peer, peer_root.path(), "correct"),
        "unsaved exports must match the selected early UUID"
    );
    assert_eq!(actual, peer_bytes(saved, peer_root.path(), "saved"));
    assert_eq!(
        std::fs::read(undo).expect("Undo export"),
        peer_bytes(before, peer_root.path(), "undo").0,
        "Undo must export original model"
    );
    crate::sessions::tests::cut::cold(saved);
}

fn compare_window(root: &Path) {
    for name in [
        "inputs/cuts.fcad",
        "work/cuts.fcad",
        "unsaved.stl",
        "unsaved.fbx",
        "undo.stl",
        "tabs/last-window",
    ] {
        assert!(root.join(name).is_file(), "missing real GUI output: {name}");
    }
    for name in ["copy", "fillets", "chamfer", "revolve"] {
        assert_eq!(
            std::fs::read(root.join(format!("inputs/{name}.fcad"))).expect("input"),
            std::fs::read(root.join(format!("work/{name}.fcad"))).expect("output"),
            "navigation changed {name}"
        );
    }
    compare_cut_files(
        &root.join("inputs/cuts.fcad"),
        &root.join("work/cuts.fcad"),
        &root.join("unsaved.stl"),
        &root.join("unsaved.fbx"),
        &root.join("undo.stl"),
    );
    let list = crate::last_tabs::Folder::at(&root.join("tabs"))
        .expect("folder")
        .read()
        .expect("read")
        .expect("published");
    let actual: Vec<_> = list
        .paths
        .iter()
        .map(|p| std::fs::canonicalize(p).expect("listed file"))
        .collect();
    // Open inserts after the active tab. The recipe returns to cuts before
    // opening the other three models, leaving copy at the end.
    let expected: Vec<_> = ["cuts", "fillets", "chamfer", "revolve", "copy"]
        .iter()
        .map(|n| std::fs::canonicalize(root.join(format!("work/{n}.fcad"))).expect("work file"))
        .collect();
    assert_eq!(
        actual, expected,
        "final LastTabs must name the actual files in order (aliases allowed)"
    );
    assert_eq!(list.active, Some(0));
    let records = RecoveryStore::open(&root.join("recovery"))
        .expect("store")
        .list()
        .expect("records");
    assert_eq!(records.active, 0);
    assert_eq!(records.recoverable().count(), 0);
}

#[test]
fn native_compare_real_model_tree_window_outputs() {
    let Some(root) = std::env::var_os("FCAD_31A_GUI_DIR") else {
        return;
    };
    assert!(native(), "window comparator requires OCCT");
    let root = Path::new(&root);
    compare_window(root);
    let controls = [
        ("missing FBX", "missing real GUI output: unsaved.fbx", 0),
        ("altered SQL", "every SQL cell", 1),
        ("stale export", "unsaved exports must match", 2),
    ];
    for (name, reason, kind) in controls {
        super::super::control_of(compare_window, root, name, reason, &|r| match kind {
            0 => std::fs::remove_file(r.join("unsaved.fbx")).expect("remove output"),
            1 => {
                rusqlite::Connection::open(r.join("work/cuts.fcad"))
                    .expect("SQL")
                    .execute("UPDATE objects SET name='damaged'", [])
                    .expect("alter");
            }
            _ => {
                std::fs::copy(r.join("undo.stl"), r.join("unsaved.stl")).expect("replace");
            }
        });
    }
    println!("\nFCAD_31A_GUI_COMPARE_OK all_SQL_cells=true controls=3");
}
