// SPDX-License-Identifier: MIT
//! §28L: all four vertical corners of one rectangular plate, one Fillet at a
//! time, each a real Open CASCADE fillet of its predecessor's result, and the
//! edits of a radius, the height, the coordinates and the constraints on the
//! whole history. Everything is measured: the B-Rep through the document's own
//! references, the mesh with an independent reader, the SQL cell by cell.
use super::constraints::{dimensioned, only_this_sketch_changed, rect_of, solved, solving};
use super::height::only_the_height_changed;
use super::radius::{only_the_radius_changed, stored_refs};
use super::sketch::{mapped, only_this_row_changed};
use super::*;
use ferritecad_eval::CacheOutcome;
use ferritecad_types::ObjectId;

const RECT: [f64; 4] = [X0, Y0, W, D];

fn uuid(v: &Value) -> ObjectId {
    v.as_str().expect("UUID").parse().expect("UUID")
}

/// Whether two drawn corners of a rectangle share a Line.
fn adjacent(a: usize, b: usize) -> bool {
    (a + 1) % 4 == b || (b + 1) % 4 == a
}

/// A fixture reading an existing document.
fn fixture_at(path: &Path) -> Fixture {
    let root = tempfile::tempdir().expect("directory");
    Fixture {
        request: root.path().join("request.json"),
        root,
        source: path.to_path_buf(),
        catalog: inspect(path),
    }
}

/// What one published Fillet is.
#[derive(Debug, Clone)]
struct Step {
    corner: [f64; 2],
    radius: f64,
    id: ObjectId,
}

/// The Fillets of a chain in `order` (indices into the drawn corners) with
/// `radii`, published one by one through the shipped command. Every step is
/// measured as a publication: discovery before and after, the exact SQL
/// allowlist, validation and a cold rebuild resolving every name. Returns the
/// documents (`docs[k]` carries `k` Fillets, `docs[0]` is the plate) and the
/// steps.
fn chain(
    corners: [[f64; 2]; 4],
    order: &[usize],
    radii: &[f64],
    tag: &str,
) -> (Fixture, Vec<PathBuf>, Vec<Step>) {
    let plate = Fixture::drawn(Plate::new(corners));
    let mut docs = vec![plate.source.clone()];
    let mut steps: Vec<Step> = Vec::new();
    for (k, (&at, &r)) in order.iter().zip(radii).enumerate() {
        let source = docs[k].clone();
        let g = fixture_at(&source);
        let before = std::fs::read(&source).expect("source bytes");
        let discovery = g.discovery();
        assert_eq!(discovery["available"], true, "{discovery}");
        let target = &discovery["target"];
        assert_eq!(target["candidates"].as_array().map(Vec::len), Some(4 - k));
        assert_eq!(target["fillets"].as_array().map(Vec::len), Some(k));
        // Each candidate says which saved Fillets share a Line with it: both
        // of them for a corner between two rounded ones, and then the single
        // scalar of the older form stays `null` rather than name one.
        for c in target["candidates"].as_array().expect("candidates") {
            let index = corners
                .iter()
                .position(|v| json!(v) == c["stored_corner_mm"])
                .expect("a drawn corner");
            let beside: Vec<usize> = (0..k).filter(|j| adjacent(order[*j], index)).collect();
            let listed = c["adjacent_fillets"].as_array().expect("adjacent list");
            assert_eq!(listed.len(), beside.len(), "{c}");
            for (item, j) in listed.iter().zip(&beside) {
                assert_eq!(item["feature_id"], json!(steps[*j].id.to_string()), "{c}");
                assert!(
                    item["shared_line_id"].is_string()
                        && item["stored_shared_length_mm"].is_number()
                );
            }
            assert_eq!(
                c["adjacent_fillet_feature_id"].is_string(),
                beside.len() == 1,
                "{c}"
            );
            assert_eq!(c["shared_line_id"].is_string(), beside.len() == 1, "{c}");
        }
        let candidate = g.at(corners[at]).clone();
        let out = plate.root.path().join(format!("{tag}-{k}.fcad"));
        g.ask_edge(
            &json!({"feature_id": candidate["edge"]["feature_id"],
                    "joint": [candidate["edge"]["joint"][1], candidate["edge"]["joint"][0]]}),
            r,
        );
        let v = reply(g.fillet(&out).output().expect("process"), OP, 0);
        let result = &v["result"];
        assert_eq!(
            result["references"].as_array().map(Vec::len),
            Some(7 + k),
            "{v}"
        );
        let id = uuid(&result["feature_id"]);
        assert_eq!(
            result["previous_feature_id"],
            if k == 0 {
                candidate["edge"]["feature_id"].clone()
            } else {
                json!(steps[k - 1].id.to_string())
            },
            "each Fillet rounds the result of the one before"
        );
        assert_eq!(
            std::fs::read(&source).expect("bytes"),
            before,
            "source touched"
        );
        only_a_fillet_was_added(
            &source,
            &out,
            uuid(&json!(g.body_id())),
            7 + k,
            match k {
                0 => &[
                    "feature.fillet.v1",
                    "topology.origin-face.v1",
                    "feature.predecessor.v1",
                ],
                1 => &["feature.fillet.sequential.v1"],
                _ => &[],
            },
        );
        steps.push(Step {
            corner: corners[at],
            radius: r,
            id,
        });
        let after = inspect(&out);
        // The history as the catalogue tells it.
        let fillets = after["fillets"].as_array().expect("fillets");
        assert_eq!(fillets.len(), k + 1);
        for (i, f) in fillets.iter().enumerate() {
            assert_eq!(f["feature_id"], json!(steps[i].id.to_string()));
            assert_eq!(f["history_index"], i + 1, "{f}");
            let edit = &f["radius_edit"];
            assert_eq!(edit["available"], true, "{f}");
            assert_eq!(edit["neighbours"].as_array().map(Vec::len), Some(k), "{f}");
            assert_eq!(
                edit["neighbour"].is_object(),
                k == 1,
                "the older single neighbour exists for two Fillets only: {f}"
            );
        }
        let base = after["features"]
            .as_array()
            .expect("features")
            .iter()
            .find(|f| !f["fillet_history"].is_null())
            .expect("the base Extrude row");
        assert_eq!(base["fillet_history"]["count"], k + 1);
        assert_eq!(base["fillet_base"].is_object(), k + 1 <= 2, "{base}");
        let row = &after["bodies"][0]["fillet_edge"];
        assert_eq!(row["available"], k + 1 < 4, "{row}");
        let checked = cli()
            .arg("validate")
            .arg(&out)
            .arg("--json")
            .output()
            .expect("validate");
        assert_eq!(reply(checked, "validate", 0)["result"]["valid"], true);
        let n = stored_refs(&out).len();
        let rebuilt = cli()
            .arg("rebuild")
            .arg(&out)
            .arg("--cold")
            .output()
            .expect("rebuild");
        let text = String::from_utf8(rebuilt.stdout).expect("UTF-8");
        assert!(
            text.contains(&format!("{n} of {n} stored references resolved")),
            "{text}"
        );
        docs.push(out);
    }
    (plate, docs, steps)
}

/// What one stored name resolved to on the rebuilt part.
#[derive(Debug, Clone, PartialEq)]
struct Face {
    role: String,
    owner: ObjectId,
    producer: ObjectId,
    origin: Option<ObjectId>,
    surfaces: Vec<FaceSurface>,
    axis: Option<[f64; 2]>,
    on_tip: bool,
}

#[derive(Debug, Clone)]
struct Built {
    faces: u64,
    volume: f64,
    shapes: usize,
    named: BTreeMap<String, Face>,
}
impl Built {
    fn same_as(&self, other: &Self) {
        assert_eq!(self.faces, other.faces);
        assert!((self.volume - other.volume).abs() < 1e-9 * self.volume);
        assert_eq!(self.shapes, other.shapes);
        assert_eq!(self.named.len(), other.named.len());
        for (id, mine) in &self.named {
            let theirs = &other.named[id];
            assert_eq!(
                (
                    &mine.role,
                    mine.owner,
                    mine.producer,
                    mine.origin,
                    &mine.surfaces,
                    mine.on_tip
                ),
                (
                    &theirs.role,
                    theirs.owner,
                    theirs.producer,
                    theirs.origin,
                    &theirs.surfaces,
                    theirs.on_tip
                ),
                "{id}"
            );
            match (mine.axis, theirs.axis) {
                (Some(a), Some(b)) => {
                    assert!((a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9)
                }
                (None, None) => {}
                other => panic!("{id}: {other:?}"),
            }
        }
    }
}

/// Rebuilds `path` cold or through the cache at `cache`, resolving every
/// stored reference in the kernel that built it.
fn measure_history(path: &Path, cache: Option<&Path>) -> (Built, Vec<ferritecad_eval::CacheEvent>) {
    let doc = Document::open_read_only(path).expect("reopen");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let mut events = Vec::new();
    let built = match cache {
        Some(cache_path) => {
            let mut store = ferritecad_document::CacheStore::open(
                cache_path,
                doc.meta().document_id,
                kernel.identity().id(),
                kernel.identity().version(),
            )
            .expect("cache");
            let (built, e) = ferritecad_eval::rebuild_cached(
                &doc,
                &mut kernel,
                &mut store,
                &OperationContext::default(),
            )
            .expect("cached rebuild");
            events = e;
            built
        }
        None => ferritecad_eval::rebuild_cold(&doc, &mut kernel, &OperationContext::default())
            .expect("cold rebuild"),
    };
    let objects = doc.objects().expect("objects");
    let body = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .expect("a body");
    let tip = built.shape(body.id).expect("the body was built");
    let mut named = BTreeMap::new();
    for reference in &doc.topology_refs().expect("refs") {
        let resolved = built.resolve(reference).expect("resolve");
        assert!(!resolved.is_empty(), "{} resolved to nothing", reference.id);
        let producer = built
            .shape(reference.producer_feature)
            .expect("the producer was built");
        for handle in &resolved {
            assert_eq!(handle.shape(), producer, "{} was repointed", reference.id);
        }
        let mut surfaces: Vec<FaceSurface> = resolved
            .iter()
            .map(|h| kernel.face_surface(*h).expect("surface"))
            .collect();
        surfaces.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        let axis = match (resolved.as_slice(), surfaces.as_slice()) {
            ([one], [FaceSurface::Cylinder { .. }]) => {
                let (origin, direction) = kernel.cylinder_axis(*one).expect("axis");
                assert!(direction[0].abs() < 1e-12 && direction[1].abs() < 1e-12);
                Some([origin[0], origin[1]])
            }
            _ => None,
        };
        let role = format!("{:?}", reference.output_role);
        named.insert(
            reference.id.to_string(),
            Face {
                role: role
                    .split([' ', '{'])
                    .next()
                    .expect("a role name")
                    .to_owned(),
                owner: reference.owner,
                producer: reference.producer_feature,
                origin: match &reference.output_role {
                    SemanticRole::OriginFilletFace { origin_feature, .. } => Some(*origin_feature),
                    _ => None,
                },
                surfaces,
                axis,
                on_tip: producer == tip,
            },
        );
    }
    let (faces, volume) = kernel.shape_stats(tip).expect("stats");
    let out = Built {
        faces,
        volume,
        shapes: built.shape_count(),
        named,
    };
    built.release_all(&mut kernel);
    doc.close().expect("close");
    assert_eq!(kernel.live_shape_count(), 0, "every handle was released");
    (out, events)
}

fn by_feature(events: &[ferritecad_eval::CacheEvent]) -> BTreeMap<ObjectId, Vec<CacheOutcome>> {
    let mut by: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for e in events {
        by.entry(e.feature).or_default().push(e.outcome);
    }
    by
}

/// `(W·D − (1 − π/4)·Σr²)·h`: the B-Rep's volume of a rectangle with its
/// corners rounded.
fn analytic([_, _, w, d]: [f64; 4], radii: &[f64], h: f64) -> f64 {
    (w * d - (1. - PI / 4.) * radii.iter().map(|r| r * r).sum::<f64>()) * h
}

/// A published copy against the plate its numbers describe. The copy is
/// reopened: valid; a cold rebuild, then a real cache Miss and Hit over the
/// plate and every Fillet, each equal to the cold one; the analytic volume
/// and faces; **every cylinder under its own name** — each Fillet's own face
/// and, on every later Fillet, its origin name — with its radius and its axis
/// `r` inward of its corner; and the exported mesh read independently (closed,
/// one oriented surface, an arc at every rounded corner and a vertex at no
/// other) and the FBX.
fn verify(copy: &Path, steps: &[Step], rect: [f64; 4], height: f64, name: &str) -> Built {
    let k = steps.len();
    let cold = {
        let (cold, _) = measure_history(copy, None);
        let cache = copy.with_extension("fcad-cache");
        let (miss, events) = measure_history(copy, Some(&cache));
        assert!(
            events.iter().all(|e| e.outcome == CacheOutcome::Miss),
            "{events:?}"
        );
        assert_eq!(
            by_feature(&events).len(),
            k + 1,
            "the plate and every Fillet"
        );
        let (hit, events) = measure_history(copy, Some(&cache));
        assert!(
            events.iter().all(|e| e.outcome == CacheOutcome::Hit),
            "{events:?}"
        );
        cold.same_as(&miss);
        cold.same_as(&hit);
        cold
    };
    let exact = analytic(
        rect,
        &steps.iter().map(|s| s.radius).collect::<Vec<_>>(),
        height,
    );
    assert!(
        (cold.volume - exact).abs() < 1e-9 * exact,
        "{} is not {exact}",
        cold.volume
    );
    assert_eq!(cold.faces, 6 + k as u64);
    let expect_axis = |s: &Step| inward_in([rect[0], rect[1]], s.corner, s.radius);
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9;
    for s in steps {
        let own: Vec<&Face> = cold
            .named
            .values()
            .filter(|f| f.role == "EdgeFilletFace" && f.owner == s.id)
            .collect();
        let [face] = own.as_slice() else {
            panic!("Fillet {} has {} own faces", s.id, own.len())
        };
        assert_eq!(face.surfaces, [FaceSurface::Cylinder { radius: s.radius }]);
        assert!(
            face.axis.is_some_and(|a| near(a, expect_axis(s))),
            "{face:?}"
        );
    }
    // Every later Fillet carries every earlier one's cylinder under its
    // origin name — the first one's through all the Fillets between.
    for (m, later) in steps.iter().enumerate() {
        for earlier in &steps[..m] {
            let found: Vec<&Face> = cold
                .named
                .values()
                .filter(|f| {
                    f.role == "OriginFilletFace"
                        && f.owner == later.id
                        && f.origin == Some(earlier.id)
                })
                .collect();
            let [face] = found.as_slice() else {
                panic!("{} names {} {} times", later.id, earlier.id, found.len())
            };
            assert_eq!(
                face.surfaces,
                [FaceSurface::Cylinder {
                    radius: earlier.radius
                }]
            );
            assert!(
                face.axis.is_some_and(|a| near(a, expect_axis(earlier))),
                "{face:?}"
            );
            assert_eq!(face.on_tip, m == k - 1);
        }
    }
    let m = mesh(copy, &copy.with_extension("stl"));
    check_mesh_rounded(
        &m,
        rect,
        &steps
            .iter()
            .map(|s| (s.corner, s.radius))
            .collect::<Vec<_>>(),
        height,
    );
    fbx(copy, name);
    cold
}

/// The four corners rounded one after the other, across both windings and
/// other starting Lines, in orders that put the third corner between two
/// rounded ones and the fourth between two as well, with a different radius
/// at each, on the asymmetric offset fractional plate.
#[test]
fn native_four_corners_in_several_orders_are_what_the_numbers_say() {
    if !native() {
        return;
    }
    for (name, corners, order, radii) in [
        (
            "opposite-then-between",
            CCW,
            [1, 3, 0, 2],
            [2.375, 3.0625, 1.5, 2.0],
        ),
        (
            "around",
            CW_FROM_UPPER_RIGHT,
            [0, 1, 2, 3],
            [3.5, 2.125, 1.75, 4.25],
        ),
        (
            "diagonal-start",
            CCW_FROM_THIRD,
            [2, 0, 3, 1],
            [5.0, 1.25, 2.75, 3.3125],
        ),
    ] {
        let (_plate, docs, steps) = chain(corners, &order, &radii, name);
        // The third and the fourth are the new ones; the first two are
        // earlier slices and are measured the same way.
        for k in [2usize, 3, 4] {
            verify(&docs[k], &steps[..k], RECT, H, &format!("{name}-{k}"));
        }
        // The document reopens cold, byte for byte as published: a second
        // reader opens it, and nothing needs the cache.
        let again = fixture_at(&docs[4]);
        assert_eq!(again.catalog["fillets"].as_array().map(Vec::len), Some(4));
        let row = &again.catalog["bodies"][0]["fillet_edge"];
        assert_eq!(row["available"], false, "{row}");
        assert!(
            row["refusal"]
                .as_str()
                .expect("a reason")
                .contains("every corner"),
            "{row}"
        );
    }
}

/// A fifth Fillet, a corner twice and a stale version are refused by name
/// before anything is written, and so are a source that is its own output, an
/// occupied output and an alias; a cancelled run leaves nothing; a lost report
/// keeps the published copy (exit 7).
#[test]
fn native_the_fourth_fillet_refusals_races_cancellation_and_report_loss_are_atomic() {
    if !native() {
        return;
    }
    let (plate, docs, steps) = chain(CCW, &[1, 3, 0], &[6.12, 3.0625, 1.5], "atomic");
    let g = fixture_at(&docs[3]);
    let before = std::fs::read(&g.source).expect("bytes");
    let last = g.at(CCW[2]).clone();
    g.ask_edge(&last["edge"], 2.0);
    let names = entries(g.root.path());
    let never = g.root.path().join("never.fcad");
    // The corner already rounded is not offered; naming it is refused.
    let row = g.discovery();
    assert_eq!(
        row["target"]["candidates"].as_array().map(Vec::len),
        Some(1)
    );
    let again = inspect(&docs[2]);
    let held = again["bodies"][0]["fillet_edge"]["target"]["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .find(|c| c["stored_corner_mm"] == json!(CCW[0]))
        .expect("the third corner, in the older document")
        .clone();
    g.ask_edge(&held["edge"], 1.0);
    let v = reply(g.fillet(&never).output().expect("process"), OP, 2);
    assert_eq!(refused(&v), "input", "{v}");
    assert!(v.to_string().contains("already rounded"), "{v}");
    // Too large beside the *far* neighbour, not the first one found.
    let bound = D - 6.12 - MIN_RADIUS;
    for (r, ok) in [(bound.next_up(), false), (bound, true)] {
        g.ask_edge(&last["edge"], r);
        if ok {
            continue;
        }
        let v = reply(g.fillet(&never).output().expect("process"), OP, 2);
        assert_eq!(refused(&v), "input", "{v}");
        assert!(v.to_string().contains("flat"), "{v}");
        assert!(
            v.to_string().contains(&steps[0].id.to_string()),
            "the first Fillet: {v}"
        );
    }
    assert_eq!(entries(g.root.path()), names, "a refusal left something");
    assert_eq!(std::fs::read(&g.source).expect("bytes"), before);

    // Output paths and a stale version.
    g.ask_edge(&last["edge"], 2.0);
    let taken = g.root.path().join("taken.fcad");
    std::fs::write(&taken, b"another process owns this").expect("occupied");
    let alias = g.root.path().join("alias.fcad");
    std::fs::hard_link(&g.source, &alias).expect("hard link");
    for (destination, why) in [
        (g.source.clone(), "the source is not its own output"),
        (taken.clone(), "an occupied output is not replaced"),
        (alias.clone(), "an alias of the source is the source"),
    ] {
        let v = reply(g.fillet(&destination).output().expect("process"), OP, 2);
        assert_eq!(refused(&v), "input", "{why}: {v}");
    }
    let v = reply(
        g.fillet_from(&g.source, g.body_id(), &"0".repeat(64), &never)
            .output()
            .expect("process"),
        OP,
        2,
    );
    assert_eq!(refused(&v), "input", "{v}");
    assert_eq!(
        std::fs::read(&taken).expect("taken"),
        b"another process owns this"
    );
    assert!(!never.exists());

    // Cancellation near the end publishes nothing and leaves nothing.
    let d = Document::open_read_only(&g.source).expect("source");
    let expected = ferritecad_document::DocumentVersion {
        document_id: d.meta().document_id,
        content: d.content_version().expect("version"),
    };
    d.close().expect("close");
    let destination = g.root.path().join("cancelled.fcad");
    let token = ferritecad_kernel::CancelToken::new();
    let stop = token.clone();
    let context = OperationContext::default()
        .with_cancel(token)
        .with_progress(ferritecad_kernel::ProgressSink::new(move |fraction| {
            if fraction >= 0.95 {
                stop.cancel();
            }
        }));
    let request = ferritecad_jobs::EdgeFilletRequest {
        source: g.source.clone(),
        expected,
        body: uuid(&json!(g.body_id())),
        fillet: ferritecad_document::EdgeFillet {
            edge: ferritecad_document::FilletEdge {
                feature: uuid(&last["edge"]["feature_id"]),
                joint: ferritecad_types::ProfileJoint::new(
                    last["edge"]["joint"][0]
                        .as_str()
                        .expect("a")
                        .parse()
                        .expect("UUID"),
                    last["edge"]["joint"][1]
                        .as_str()
                        .expect("b")
                        .parse()
                        .expect("UUID"),
                )
                .expect("joint"),
            },
            radius_mm: 2.0,
        },
        destination: destination.clone(),
    };
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let result = ferritecad_jobs::fillet_edge_copy(&request, &mut kernel, &context);
    assert!(result.is_err(), "{result:?}");
    assert!(!destination.exists());
    assert_eq!(kernel.live_shape_count(), 0);
    assert_eq!(entries(g.root.path()), {
        let mut n = names.clone();
        n.extend([
            taken.file_name().expect("n").to_owned(),
            alias.file_name().expect("n").to_owned(),
        ]);
        n.sort();
        n
    });

    // A lost report: the copy is published and kept, exit 7, and it is the
    // fourth Fillet.
    let lost = g.root.path().join("lost-report.fcad");
    assert_eq!(
        g.fillet(&lost)
            .stdout(pipe::closed_pipe())
            .stderr(pipe::closed_pipe())
            .status()
            .expect("pipes")
            .code(),
        Some(7)
    );
    assert!(lost.exists(), "the published copy is kept");
    let kept = inspect(&lost);
    assert_eq!(kept["fillets"].as_array().map(Vec::len), Some(4));
    assert_eq!(kept["bodies"][0]["fillet_edge"]["available"], false);
    assert_eq!(std::fs::read(&g.source).expect("bytes"), before);
    drop(plate);
}

const MIN_RADIUS: f64 = 0.01;

/// The largest radius `r_before` that leaves `r_later` its flat on a Line of
/// `length`, by brute force on the rule's own expression.
fn largest_before(length: f64, r_later: f64) -> f64 {
    let mut g = length - r_later - MIN_RADIUS;
    while r_later > length - g - MIN_RADIUS {
        g = g.next_down();
    }
    while r_later <= length - g.next_up() - MIN_RADIUS {
        g = g.next_up();
    }
    g
}

fn radius_edit(source: &Path, feature: ObjectId, r: f64, out: &Path) -> Output {
    let g = fixture_at(source);
    g.ask_radius(r);
    g.edit_from(source, &feature.to_string(), g.version(), out)
        .output()
        .expect("process")
}

/// The early, the middle and the last radius of a history of four, each
/// published by the shipped command: the named cylinder moves to its new
/// radius on its own axis, every other name and UUID is kept, the SQL change
/// is that Fillet's row alone, and the cache keeps exactly the unchanged
/// prefix. The first Fillet's largest radius is bound by the **fourth**
/// across the short Line — the far neighbour in the history — and is accepted
/// to the float; the next float above it is refused naming that Fillet.
#[test]
fn native_early_middle_and_last_radii_answer_to_every_neighbour() {
    if !native() {
        return;
    }
    // r4 = 6.12 leaves the first 12.25 - 6.12 - 0.01 = 6.12 across Line 1.
    let radii = [2.375, 3.0625, 1.5, 6.12];
    let (_plate, docs, steps) = chain(CCW, &[1, 3, 0, 2], &radii, "radii");
    let source = docs[4].clone();
    let g = fixture_at(&source);
    let refs = stored_refs(&source);
    let before = std::fs::read(&source).expect("bytes");
    let fillets = g.catalog["fillets"].as_array().expect("fillets").clone();
    // The bounds discovery reports are the minimum over every neighbour.
    let max = |i: usize| {
        fillets[i]["radius_edit"]["max_radius_mm"]
            .as_f64()
            .expect("bound")
    };
    assert_eq!(max(0), f64::min(0.5 * D, largest_before(D, radii[3])));
    assert_eq!(max(3), f64::min(0.5 * D, D - radii[0] - MIN_RADIUS));
    for (i, f) in fillets.iter().enumerate() {
        let n = f["radius_edit"]["neighbours"]
            .as_array()
            .expect("neighbours");
        assert_eq!(n.len(), 3, "{f}");
        assert!(
            f["radius_edit"]["neighbour"].is_null(),
            "not a history of two"
        );
        assert_eq!(f["history_index"], i + 1);
    }

    // Past the far neighbour: the first Fillet beside the fourth.
    let never = g.root.path().join("never.fcad");
    let names = entries(g.root.path());
    let over = radius_edit(&source, steps[0].id, max(0).next_up(), &never);
    let v = reply(over, OP_RADIUS, 2);
    assert_eq!(refused(&v), "input", "{v}");
    assert!(v.to_string().contains("flat"), "{v}");
    assert!(
        v.to_string().contains(&steps[3].id.to_string()),
        "the fourth Fillet: {v}"
    );
    assert!(!never.exists());
    assert_eq!(entries(g.root.path()), names);
    // Early, middle, last: each at its exact bound or inside it.
    let cache = source.with_extension("fcad-cache");
    let (cold, _) = measure_history(&source, Some(&cache));
    for (k, r) in [(0usize, max(0)), (1, 2.5), (3, 4.75)] {
        let out = g.root.path().join(format!("edited-{k}.fcad"));
        let v = reply(radius_edit(&source, steps[k].id, r, &out), OP_RADIUS, 0);
        assert_eq!(v["result"]["feature_id"], json!(steps[k].id.to_string()));
        assert_eq!(v["result"]["history_index"], k + 1);
        assert_eq!(v["result"]["previous_radius_mm"], radii[k]);
        assert_eq!(v["result"]["radius_mm"], r);
        assert!(only_the_radius_changed(&source, &out, &steps[k].id.to_string()) >= 2);
        assert_eq!(stored_refs(&out), refs, "every name and its UUID kept");
        assert_eq!(std::fs::read(&source).expect("bytes"), before);
        let mut edited = steps.clone();
        edited[k].radius = r;
        let built = verify(&out, &edited, RECT, H, &format!("radii-{k}"));
        // The named cylinder is the same name on a new radius.
        let id = steps[k].id;
        assert!(built.named.values().any(|f| f.role == "EdgeFilletFace"
            && f.owner == id
            && f.surfaces == [FaceSurface::Cylinder { radius: r }]));
        // The cache restored from the unchanged prefix only: the plate and
        // the Fillets before the changed one hit, it and every later one miss.
        let (_, events) = measure_history(&out, Some(&cache));
        let outcome = by_feature(&events);
        let order: Vec<ObjectId> = steps.iter().map(|s| s.id).collect();
        for (j, id) in order.iter().enumerate() {
            let want = if j < k {
                CacheOutcome::Hit
            } else {
                CacheOutcome::Miss
            };
            assert_eq!(
                outcome[id],
                vec![want],
                "Fillet {} after changing {}",
                j + 1,
                k + 1
            );
        }
        assert!(
            outcome
                .iter()
                .filter(|(id, _)| !order.contains(id))
                .all(|(_, o)| o.iter().all(|o| *o == CacheOutcome::Hit)),
            "the plate is untouched: {outcome:?}"
        );
    }
    drop(cold);
    // The bound itself, restored: the maximum is accepted and then restored.
    let out = g.root.path().join("restored.fcad");
    let v = reply(radius_edit(&source, steps[0].id, 2.375, &out), OP_RADIUS, 0);
    assert_eq!(v["result"]["radius_mm"], 2.375);
}

const OP_RADIUS: &str = "edit-fillet-radius";

/// The fourth Fillet cannot grow past the first, across the short Line the
/// perimeter closes on, though two Fillets lie between them in the history:
/// the largest radius is the first Fillet's remainder, accepted to the float,
/// and the next float is refused naming the first Fillet and that Line.
#[test]
fn native_the_last_radius_answers_to_the_first_across_the_closing_line() {
    if !native() {
        return;
    }
    let radii = [6.12, 3.0625, 1.5, 3.0];
    let (_plate, docs, steps) = chain(CCW, &[1, 3, 0, 2], &radii, "closing");
    let source = docs[4].clone();
    let g = fixture_at(&source);
    let bound = D - 6.12 - MIN_RADIUS;
    let max = g.catalog["fillets"][3]["radius_edit"]["max_radius_mm"]
        .as_f64()
        .expect("bound");
    assert_eq!(max, bound);
    let never = g.root.path().join("never.fcad");
    let names = entries(g.root.path());
    let v = reply(
        radius_edit(&source, steps[3].id, bound.next_up(), &never),
        OP_RADIUS,
        2,
    );
    assert_eq!(refused(&v), "input", "{v}");
    assert!(v.to_string().contains("flat"), "{v}");
    assert!(
        v.to_string().contains(&steps[0].id.to_string()),
        "the first Fillet: {v}"
    );
    assert!(
        !v.to_string().contains(&steps[1].id.to_string())
            && !v.to_string().contains(&steps[2].id.to_string()),
        "not the ones between: {v}"
    );
    assert!(!never.exists());
    assert_eq!(entries(g.root.path()), names);
    let out = g.root.path().join("exact.fcad");
    reply(radius_edit(&source, steps[3].id, bound, &out), OP_RADIUS, 0);
    let mut edited = steps.clone();
    edited[3].radius = bound;
    verify(&out, &edited, RECT, H, "closing-exact");
}

/// The base height, the base rectangle and the Line constraints under four
/// Fillets: each is the existing command, reads every Fillet in history
/// order, moves its own row alone, and the rebuilt plate is the plate its
/// numbers describe with all four cylinders under their names. The
/// rectangle's pair rule holds to the exact float between the first and the
/// fourth Fillet, which are not neighbours in the history.
#[test]
fn native_height_coordinates_and_constraints_keep_four_fillets() {
    if !native() {
        return;
    }
    let radii = [2.0, 2.0, 2.0, 2.0];
    let (_plate, docs, steps) = chain(CCW, &[1, 3, 0, 2], &radii, "edits");
    let source = docs[4].clone();
    let g = fixture_at(&source);
    let refs = stored_refs(&source);
    let before = std::fs::read(&source).expect("bytes");
    let base = g.base_id().to_owned();
    assert!(
        g.base_row()["fillet_base"].is_null(),
        "no older projection of four"
    );
    assert_eq!(g.base_row()["fillet_history"]["count"], 4);

    // The height.
    let raised = g.root.path().join("raised.fcad");
    let v = reply(
        g.raise(&source, &base, "11.5", &raised)
            .output()
            .expect("process"),
        "edit-extrude",
        0,
    );
    assert_eq!(v["result"]["feature_id"], json!(base));
    assert!(only_the_height_changed(&source, &raised, &base) >= 2);
    assert_eq!(stored_refs(&raised), refs);
    verify(&raised, &steps, RECT, 11.5, "edits-height");
    let cache = source.with_extension("fcad-cache");
    measure_history(&source, Some(&cache));
    let (_, events) = measure_history(&raised, Some(&cache));
    assert!(
        events.iter().all(|e| e.outcome == CacheOutcome::Miss),
        "a new height rebuilds the plate and every Fillet: {events:?}"
    );

    // The rectangle: moved and resized, and then to the exact float.
    let rectangle = [1.5, -2.25, 30.0, 9.0];
    let h = fixture_at(&source);
    h.ask_rect(rectangle);
    let moved = h.root.path().join("moved.fcad");
    let v = reply(
        h.redraw(&source, h.version(), &moved)
            .output()
            .expect("process"),
        "edit-sketch-copy",
        0,
    );
    assert_eq!(v["result"]["sketch_id"], h.sketch_id());
    assert_eq!(only_this_row_changed(&source, &moved, h.sketch_id()), 2);
    assert_eq!(stored_refs(&moved), refs);
    let at = |c: [f64; 2]| mapped(rectangle, c);
    let moved_steps: Vec<Step> = steps
        .iter()
        .map(|s| Step {
            corner: at(s.corner),
            ..s.clone()
        })
        .collect();
    verify(&moved, &moved_steps, rectangle, H, "edits-rectangle");
    // The least depth that leaves the flat 2 + 2 + 0.01 between the first
    // Fillet (corner 1) and the fourth (corner 2), and one float below it.
    let exact = {
        let mut least = 4.01_f64;
        while 2.0 <= ferritecad_document::pair_bound(least, 2.0) {
            least = least.next_down();
        }
        while 2.0 > ferritecad_document::pair_bound(least, 2.0) {
            least = least.next_up();
        }
        least
    };
    h.ask_rect([X0, Y0, W, exact]);
    let ok = h.root.path().join("exact.fcad");
    reply(
        h.redraw(&source, h.version(), &ok)
            .output()
            .expect("process"),
        "edit-sketch-copy",
        0,
    );
    h.ask_rect([X0, Y0, W, exact.next_down()]);
    let no = h.root.path().join("no.fcad");
    let names = entries(h.root.path());
    let v = reply(
        h.redraw(&source, h.version(), &no)
            .output()
            .expect("process"),
        "edit-sketch-copy",
        2,
    );
    assert_eq!(refused(&v), "input", "{v}");
    assert!(v.to_string().contains("flat"), "{v}");
    assert!(
        steps
            .iter()
            .any(|s| v.to_string().contains(&s.id.to_string())),
        "a Fillet in the way: {v}"
    );
    assert!(!no.exists());
    let mut after = entries(h.root.path());
    after.retain(|n| n != "exact.fcad");
    assert_eq!(
        after,
        names
            .iter()
            .filter(|n| *n != "exact.fcad")
            .cloned()
            .collect::<Vec<_>>()
    );
    assert_eq!(std::fs::read(&source).expect("bytes"), before);

    // The constraints (a solver is needed): the plate dimensioned to a
    // smaller solved rectangle; the Edit Sketch coordinates are refused
    // while a user constraint is there, and returned when they are removed.
    if !solving() {
        return;
    }
    let c = fixture_at(&source);
    let (at0, width, depth) = ([0.5, 1.25], 28.0, 9.5);
    let add = dimensioned(&c, at0, width, depth);
    c.ask_constraints(&[], &add);
    let dimensioned_copy = c.root.path().join("dimensioned.fcad");
    let v = reply(
        c.constrain(&dimensioned_copy).output().expect("process"),
        "edit-sketch-constraints-copy",
        0,
    );
    assert_eq!(v["result"]["sketch_id"], c.plate_sketch_id(), "{v}");
    assert_eq!(v["result"]["solve"]["degrees_of_freedom"], 0, "{v}");
    only_this_sketch_changed(&source, &dimensioned_copy, c.plate_sketch_id());
    assert_eq!(stored_refs(&dimensioned_copy), refs);
    let (starts, dof) = solved(&dimensioned_copy);
    assert_eq!(dof, 0);
    let rect = rect_of(&starts);
    assert!(
        (rect[2] - width).abs() < 1e-9 && (rect[3] - depth).abs() < 1e-9,
        "{rect:?}"
    );
    let stored = c.stored_starts();
    let solved_steps: Vec<Step> = steps
        .iter()
        .map(|s| Step {
            corner: starts[stored.iter().position(|v| *v == s.corner).expect("corner")],
            ..s.clone()
        })
        .collect();
    verify(
        &dimensioned_copy,
        &solved_steps,
        rect,
        H,
        "edits-dimensioned",
    );
    // The catalogue: all four, the older projection absent, the bound the
    // solved plate's (`null`), the coordinate editor refusing, naming the
    // constraint editor.
    let d = fixture_at(&dimensioned_copy);
    assert_eq!(
        d.catalog["sketches"][0]["editable"], false,
        "{}",
        d.catalog["sketches"][0]
    );
    assert_eq!(
        d.catalog["sketches"][0]["constraint_edit"]["fillet_history"]["count"],
        4
    );
    assert!(d.constraint_row()["fillet_base"].is_null());
    for f in d.catalog["fillets"].as_array().expect("fillets") {
        assert!(f["radius_edit"]["max_radius_mm"].is_null(), "{f}");
    }
    // Removing every user constraint leaves the closure and returns the
    // coordinate editor.
    let user: Vec<Value> = d.constraint_row()["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .filter(|c| c["rule"]["kind"] != "coincident")
        .map(|c| c["constraint_id"].clone())
        .collect();
    d.ask_constraints(&user, &[]);
    let freed = d.root.path().join("freed.fcad");
    reply(
        d.constrain(&freed).output().expect("process"),
        "edit-sketch-constraints-copy",
        0,
    );
    let f = fixture_at(&freed);
    assert_eq!(
        f.catalog["sketches"][0]["editable"], true,
        "{}",
        f.catalog["sketches"][0]
    );
    verify(&freed, &steps, RECT, H, "edits-freed");
}

/// On a dimensioned plate the radii are the solved plate's: a depth that
/// outgrew the stored bound takes a radius the stored rectangle could not,
/// and a depth that shrank refuses a radius the stored rectangle still holds
/// — each judged at every rebuild by the evaluator, not by a second check —
/// and the solver's own conflict and redundancy keep their real UUIDs.
#[test]
fn native_the_solved_plate_decides_every_radius_of_four() {
    if !solving() {
        return;
    }
    let radii = [2.0, 2.0, 2.0, 2.0];
    let (_plate, docs, steps) = chain(CCW, &[1, 3, 0, 2], &radii, "solved");
    let c = fixture_at(&docs[4]);
    // A plate twice as deep as the stored one: radius 8 mm would be refused
    // on the stored 12.25 × ½ = 6.125 mm.
    let (at0, width, depth) = ([X0, Y0], W, 20.0);
    c.ask_constraints(&[], &dimensioned(&c, at0, width, depth));
    let deep = c.root.path().join("deep.fcad");
    reply(
        c.constrain(&deep).output().expect("process"),
        "edit-sketch-constraints-copy",
        0,
    );
    let g = fixture_at(&deep);
    let id = steps[0].id;
    let out = g.root.path().join("eight.fcad");
    let v = reply(radius_edit(&deep, id, 8.0, &out), OP_RADIUS, 0);
    assert_eq!(v["result"]["radius_mm"], 8.0, "{v}");
    let (starts, _) = solved(&out);
    let stored = g.stored_starts();
    let mut edited = steps.clone();
    edited[0].radius = 8.0;
    let solved_steps: Vec<Step> = edited
        .iter()
        .map(|s| Step {
            corner: starts[stored.iter().position(|v| *v == s.corner).expect("corner")],
            ..s.clone()
        })
        .collect();
    verify(&out, &solved_steps, rect_of(&starts), H, "solved-eight");
    // Shrunk to 12 mm deep: the 8 mm radius no longer fits (bound 6), though
    // the stored rectangle would still hold it. Nothing is published.
    let h = fixture_at(&out);
    let user: Vec<Value> = h.constraint_row()["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .filter(|c| c["rule"]["kind"] == "distance")
        .map(|c| c["constraint_id"].clone())
        .collect();
    assert_eq!(user.len(), 2);
    let across = super::constraints::horizontal(&h.stored_starts());
    let v_line = across.iter().position(|a| !*a).expect("a vertical Line");
    let depth_rule = h.constraint_row()["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .find(|c| c["rule"]["kind"] == "distance" && c["rule"]["distance"] == 20.0)
        .expect("the depth")["constraint_id"]
        .clone();
    h.ask_constraints(
        std::slice::from_ref(&depth_rule),
        &[super::constraints::length(&h.line(v_line), 12.0)],
    );
    let never = h.root.path().join("never.fcad");
    let names = entries(h.root.path());
    let v = reply(
        h.constrain(&never).output().expect("process"),
        "edit-sketch-constraints-copy",
        2,
    );
    assert_eq!(refused(&v), "input", "{v}");
    assert!(
        v.to_string().contains(&steps[0].id.to_string()) || v.to_string().contains("too large"),
        "{v}"
    );
    assert!(!never.exists());
    assert_eq!(entries(h.root.path()), names);
    // The other direction: a plate 9 mm deep (solved bound 4.5) refuses the
    // radius 4.9 mm that the stored rectangle would still accept.
    let c9 = fixture_at(&docs[4]);
    c9.ask_constraints(&[], &dimensioned(&c9, [X0, Y0], W, 9.0));
    let shallow = c9.root.path().join("shallow.fcad");
    reply(
        c9.constrain(&shallow).output().expect("process"),
        "edit-sketch-constraints-copy",
        0,
    );
    let v = reply(
        radius_edit(&shallow, steps[1].id, 4.9, &c9.root.path().join("no.fcad")),
        OP_RADIUS,
        2,
    );
    assert_eq!(refused(&v), "input", "{v}");
    reply(
        radius_edit(&shallow, steps[1].id, 4.4, &c9.root.path().join("yes.fcad")),
        OP_RADIUS,
        0,
    );

    // A real conflict and a real redundancy, as before: the other horizontal
    // side dimensioned differently, and an equality that says what the
    // lengths said.
    let g = fixture_at(&deep);
    let opposite = (1..4)
        .find(|i| across[*i])
        .expect("the other horizontal Line");
    g.ask_constraints(&[], &[super::constraints::length(&g.line(opposite), 20.0)]);
    let v = reply(
        g.constrain(&never).output().expect("process"),
        "edit-sketch-constraints-copy",
        2,
    );
    assert_eq!(refused(&v), "constraint", "{v}");
    let named = v["error"]["constraint_conflict"]["constraints"]
        .as_array()
        .expect("conflict")
        .clone();
    assert!(
        !named.is_empty() && named.iter().all(|c| c["constraint_id"].is_string()),
        "{v}"
    );
    let held: Vec<Value> = g.constraint_row()["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .map(|c| c["constraint_id"].clone())
        .collect();
    assert!(
        named.iter().any(|c| held.contains(&c["constraint_id"])),
        "a real constraint: {v}"
    );
    let curves = &g.constraint_row()["curves"];
    g.ask_constraints(
        &[],
        &[json!({"rule": "equal_length",
                 "a_curve_id": curves[0]["curve_id"],
                 "b_curve_id": curves[opposite]["curve_id"]})],
    );
    let said = g.root.path().join("redundant.fcad");
    let v = reply(
        g.constrain(&said).output().expect("process"),
        "edit-sketch-constraints-copy",
        0,
    );
    let equality = v["result"]["added_constraints"][0]["constraint_id"].clone();
    assert_eq!(
        v["result"]["solve"]["redundant_constraint_ids"],
        json!([equality]),
        "{v}"
    );
    assert_eq!(v["result"]["solve"]["degrees_of_freedom"], 0);
}

/// Rounds drawn corner `at` of the plate in `f` with `r`, written without a
/// kernel by the shipped preparation and writer. Returns the new Fillet.
fn round_without_kernel(f: &Fixture, corners: [[f64; 2]; 4], at: usize, r: f64) -> ObjectId {
    let mut d = Document::open(&f.source).expect("writable");
    let objects = d.objects().expect("objects");
    let body = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .expect("a Body")
        .id;
    let choice = ferritecad_document::fillet_choices(&d, &objects)[0].clone();
    let target = choice.target.expect("a target");
    let corner = target
        .corners
        .iter()
        .find(|c| c.corner_mm == corners[at])
        .expect("a candidate corner");
    let prepared = ferritecad_document::prepare_edge_fillet(
        &d,
        body,
        &ferritecad_document::EdgeFillet {
            edge: ferritecad_document::FilletEdge {
                feature: target.base_feature,
                joint: corner.joint,
            },
            radius_mm: r,
        },
    )
    .expect("prepared");
    let id = prepared.feature().id;
    d.write_edge_fillet(&prepared).expect("written");
    d.close().expect("close");
    id
}

/// The wire for none to four Fillets, with no kernel: `history_index` runs 1
/// to 4; `neighbours` lists every other Fillet in history order with the Line
/// it shares; the older single `neighbour` is an object for a history of
/// exactly two and `null` otherwise; `fillet_base` with `second_fillet` is the
/// older projection of one or two and `null` for three or four, which
/// `fillet_history` describes whatever the count (also in the constraint
/// editor's context); the candidates list every adjacent Fillet and never
/// name one when two share a Line; a plate with four rounded corners has no
/// candidate and a typed refusal.
#[test]
fn history_discovery_and_protocol_without_native() {
    let corners = CCW_FROM_THIRD;
    let order = [2usize, 0, 3, 1];
    let radii = [5.0, 1.25, 2.75, 3.3125];
    let f = Fixture::drawn(Plate::new(corners));
    assert_eq!(f.catalog["fillets"], json!([]));
    assert!(f.catalog["features"][0]["fillet_history"].is_null());
    assert!(f.catalog["features"][0]["fillet_base"].is_null());
    let mut ids: Vec<ObjectId> = Vec::new();
    for n in 1..=4usize {
        ids.push(round_without_kernel(
            &f,
            corners,
            order[n - 1],
            radii[n - 1],
        ));
        let c = inspect(&f.source);
        let fillets = c["fillets"].as_array().expect("fillets");
        assert_eq!(fillets.len(), n);
        for (i, row) in fillets.iter().enumerate() {
            assert_eq!(row["feature_id"], json!(ids[i].to_string()));
            assert_eq!(row["history_index"], i + 1);
            assert_eq!(row["radius_mm"], radii[i]);
            let edit = &row["radius_edit"];
            assert_eq!(edit["available"], true, "{row}");
            let list = edit["neighbours"].as_array().expect("neighbours");
            assert_eq!(list.len(), n - 1);
            let want: Vec<String> = (0..n)
                .filter(|j| *j != i)
                .map(|j| ids[j].to_string())
                .collect();
            let got: Vec<&str> = list
                .iter()
                .map(|x| x["feature_id"].as_str().expect("id"))
                .collect();
            assert_eq!(got, want, "in history order");
            for x in list {
                let j = x["history_index"].as_u64().expect("index") as usize - 1;
                let shared = adjacent(order[i], order[j]);
                assert_eq!(x["shared_line_id"].is_string(), shared, "{x}");
                assert_eq!(x["stored_shared_length_mm"].is_number(), shared, "{x}");
                assert_eq!(x["radius_mm"], radii[j]);
            }
            assert_eq!(edit["neighbour"].is_object(), n == 2, "{n}: {row}");
            assert!(edit["max_radius_mm"].as_f64().expect("a bound") >= radii[i]);
        }
        // Every surface that names the history.
        let projections = |v: &Value| {
            (
                v["fillet_base"].is_object(),
                v["fillet_base"]["second_fillet"].is_object(),
                v["fillet_history"]["count"].as_u64(),
            )
        };
        let want = (n <= 2, n == 2, Some(n as u64));
        assert_eq!(projections(&c["features"][0]), want, "{}", c["features"][0]);
        assert_eq!(projections(&c["sketches"][0]), want, "{}", c["sketches"][0]);
        assert_eq!(
            projections(&c["sketches"][0]["constraint_edit"]),
            want,
            "{}",
            c["sketches"][0]["constraint_edit"]
        );
        let history = &c["features"][0]["fillet_history"]["fillets"];
        let base = &c["features"][0]["feature_id"];
        for (i, e) in history.as_array().expect("history").iter().enumerate() {
            assert_eq!(e["fillet_feature_id"], json!(ids[i].to_string()));
            assert_eq!(e["history_index"], i + 1);
            assert_eq!(
                e["previous_feature_id"],
                if i == 0 {
                    base.clone()
                } else {
                    json!(ids[i - 1].to_string())
                },
                "each rounds the result of the one before"
            );
            assert_eq!(e["edge"]["feature_id"], *base, "the edge is the plate's");
            assert_eq!(e["corner_mm"], json!(corners[order[i]]));
            assert_eq!(e["radius_mm"], radii[i]);
        }
        // The older projections agree with the history where they exist.
        if n <= 2 {
            assert_eq!(
                c["features"][0]["fillet_base"]["fillet_feature_id"],
                json!(ids[0].to_string())
            );
        }
        // The candidates, and the refusal when none is left.
        let row = &c["bodies"][0]["fillet_edge"];
        if n < 4 {
            assert_eq!(row["available"], true, "{row}");
            let candidates = row["target"]["candidates"].as_array().expect("candidates");
            assert_eq!(candidates.len(), 4 - n);
            assert_eq!(row["target"]["fillets"].as_array().map(Vec::len), Some(n));
            for k in candidates {
                let index = corners
                    .iter()
                    .position(|v| json!(v) == k["stored_corner_mm"])
                    .expect("corner");
                let beside = (0..n).filter(|j| adjacent(order[*j], index)).count();
                assert_eq!(
                    k["adjacent_fillets"].as_array().map(Vec::len),
                    Some(beside),
                    "{k}"
                );
                assert_eq!(k["shared_line_id"].is_string(), beside == 1, "{k}");
                assert!(!k["label"].as_str().expect("label").is_empty());
            }
        } else {
            assert_eq!(row["available"], false, "{row}");
            assert!(row["target"].is_null());
            assert!(
                row["refusal"]
                    .as_str()
                    .expect("a reason")
                    .contains("every corner"),
                "{row}"
            );
        }
    }
    // A fifth Fillet is refused before anything is written, whatever the
    // build can do: unsupported, typed, nothing left behind.
    let g = fixture_at(&f.source);
    let before = std::fs::read(&f.source).expect("bytes");
    let names = entries(g.root.path());
    let never = g.root.path().join("never.fcad");
    let tip = inspect(&f.source);
    let held = tip["fillets"][0]["edge"].clone();
    g.ask_edge(&held, 1.0);
    let names_with_request = {
        let mut n = names;
        n.push("request.json".into());
        n.sort();
        n
    };
    let v = reply(
        g.fillet_from(&f.source, g.body_id(), g.version(), &never)
            .output()
            .expect("process"),
        OP,
        2,
    );
    assert_eq!(refused(&v), "unsupported", "{v}");
    assert!(!never.exists());
    assert_eq!(entries(g.root.path()), names_with_request);
    assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
}
