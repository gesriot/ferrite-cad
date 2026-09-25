// SPDX-License-Identifier: MIT
//! §26I: the same bounded Cut history in a simple Line polygon — a concave L,
//! sloped triangles and quadrilaterals, either winding, far from the origin.
//!
//! Containment is judged against the saved Lines, never their bounding box;
//! every rule runs in the catalogue, preparation and the writer's transaction.
use super::*;
use ferritecad_document::{
    CircularCutEdit, CutBoundary, SavedCircularCut, SketchVertex, prepare_circular_cut,
    prepare_cut_parameters, prepare_extrude_height, replace_sketch_coordinates,
};
use ferritecad_types::{ObjectId, StableEntityId};

/// The acceptance L: 1600 mm², notch at the top right, reflex vertex (20, 20).
pub(super) const L: [[f64; 2]; 6] = [
    [0., 0.],
    [60., 0.],
    [60., 20.],
    [20., 20.],
    [20., 40.],
    [0., 40.],
];
const H: f64 = 10.;
const EDIT: &str = "edit-circular-cut";

/// Sixteen separated disks inside the L, in both branches and the corner
/// they share; slot 15 crosses y = 20 left of the reflex vertex, which is
/// inside the part although it is a side of the lower branch's box.
const SLOTS: [[f64; 2]; 16] = [
    [26.125, 5.625],
    [33.125, 5.625],
    [40.125, 5.625],
    [47.125, 5.625],
    [54.125, 5.625],
    [26.125, 13.625],
    [33.125, 13.625],
    [40.125, 13.625],
    [47.125, 13.625],
    [54.125, 13.625],
    [6.125, 25.625],
    [14.125, 25.625],
    [6.125, 33.625],
    [14.125, 33.625],
    [10.125, 9.625],
    [10.125, 18.625],
];

/// History order is not geometric order; ThroughAll, Blind-through and
/// pockets alternate. The first two links are in different branches.
pub(super) fn tools(n: usize) -> Vec<CircularCut> {
    (0..n)
        .map(|i| CircularCut {
            center_mm: SLOTS[(i * 11) % 16],
            radius_mm: 1.5 + (i % 4) as f64 * 0.25,
            extent: match i % 3 {
                0 => CutExtent::ThroughAll,
                1 => CutExtent::Blind { depth_mm: H },
                _ => CutExtent::Blind {
                    depth_mm: 3.5 + (i % 7) as f64,
                },
            },
        })
        .collect()
}

fn reach(tools: &[CircularCut], height: f64) -> f64 {
    tools
        .iter()
        .map(|t| PI * t.radius_mm.powi(2) * t.extent.reach_mm(height))
        .sum()
}

fn extent_json(extent: CutExtent) -> Value {
    match extent {
        CutExtent::Blind { depth_mm } => json!({"kind":"blind","depth_mm":depth_mm}),
        CutExtent::ThroughAll => json!({"kind":"through_all"}),
    }
}

/// A part and its history written without a kernel, through the document's
/// own preparation and writer.
fn fixture(root: &Path, points: &[[f64; 2]], tools: &[CircularCut]) -> PathBuf {
    let path = root.join("polygon.fcad");
    write_polygon_document(&path, points, H);
    let mut d = Document::open(&path).expect("doc");
    let (body, _) = tip(&d);
    for tool in tools {
        let p = prepare_circular_cut(&d, body, tool).expect("prepare link");
        d.write_circular_cut(&p).expect("write link");
    }
    d.close().expect("close");
    path
}

fn ordered(path: &Path) -> Vec<SavedCircularCut> {
    super::history::ordered(path)
}

/// The base Sketch's saved Lines, by identity and in stored order.
fn base_lines(path: &Path) -> Vec<(StableEntityId, [f64; 2], [f64; 2])> {
    let d = Document::open_read_only(path).expect("doc");
    let saved = &ordered(path)[0];
    let lines = d
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| o.id == saved.profile_sketch)
        .map(|o| match o.payload {
            ObjectPayload::Sketch(s) => s
                .curves
                .iter()
                .map(|c| match c.geometry {
                    SketchGeometry::Line { start, end } => {
                        (c.id, [start.x, start.y], [end.x, end.y])
                    }
                    _ => panic!("a Line"),
                })
                .collect(),
            _ => panic!("the base Sketch"),
        })
        .expect("base");
    d.close().expect("close");
    lines
}

fn curve_ids(boundary: &Value) -> Vec<String> {
    boundary["segments"]
        .as_array()
        .expect("segments")
        .iter()
        .map(|s| s["curve_id"].as_str().expect("id").to_owned())
        .collect()
}

/// A `_v3` boundary is the saved Lines, by identity, in order, and nothing else.
fn check_boundary(path: &Path, block: &Value, points: &[[f64; 2]]) {
    let lines = base_lines(path);
    let boundary = &block["boundary"];
    assert_eq!(boundary["kind"], "line_polygon");
    assert_eq!(
        curve_ids(boundary),
        lines.iter().map(|l| l.0.to_string()).collect::<Vec<_>>()
    );
    for (segment, line) in boundary["segments"]
        .as_array()
        .expect("s")
        .iter()
        .zip(&lines)
    {
        assert_eq!(segment["start_mm"], json!(line.1));
        assert_eq!(segment["end_mm"], json!(line.2));
    }
    let area = boundary["area_mm2"].as_f64().expect("area");
    assert!((area - polygon_area(points)).abs() < 1e-6, "{area}");
    let twice: f64 = (0..points.len())
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            a[0] * b[1] - a[1] * b[0]
        })
        .sum();
    assert_eq!(
        boundary["orientation"],
        if twice > 0. {
            "counter_clockwise"
        } else {
            "clockwise"
        }
    );
    let xs = points.iter().map(|p| p[0]);
    let ys = points.iter().map(|p| p[1]);
    assert_eq!(
        block["bounds_mm"],
        json!([
            [
                xs.clone().fold(f64::INFINITY, f64::min),
                ys.clone().fold(f64::INFINITY, f64::min)
            ],
            [
                xs.fold(f64::NEG_INFINITY, f64::max),
                ys.fold(f64::NEG_INFINITY, f64::max)
            ]
        ])
    );
    assert!(block.get("extents_mm").is_none(), "_v3 never says extents");
}

/// A consumer written against the rectangle blocks: `extents_mm` is the part.
mod old_rectangle {
    use serde::Deserialize;
    #[derive(Deserialize)]
    pub struct Part {
        pub extents_mm: [[f64; 2]; 2],
    }
    #[derive(Deserialize)]
    pub struct Add {
        pub refusal: Option<String>,
        pub target: Option<Part>,
    }
    #[derive(Deserialize)]
    pub struct Edit {
        pub refusal: Option<String>,
        pub saved: Option<Part>,
    }
    #[derive(Deserialize)]
    pub struct History {
        pub tools: Vec<serde_json::Value>,
    }
    #[derive(Deserialize)]
    pub struct Feature {
        pub circular_cut_edit: Edit,
        pub circular_cut_edit_v2: Edit,
        pub base_height_edit: Option<Part>,
        pub base_height_edit_v2: Option<Part>,
    }
    #[derive(Deserialize)]
    pub struct Sketch {
        pub cut_history: Option<History>,
        pub cut_history_v2: Option<History>,
    }
    #[derive(Deserialize)]
    pub struct Body {
        pub cut_edit: Add,
        pub cut_edit_v2: Add,
    }
    #[derive(Deserialize)]
    pub struct Inspection {
        pub features: Vec<Feature>,
        pub bodies: Vec<Body>,
        pub sketches: Vec<Sketch>,
    }

    /// Every rectangle this consumer would believe, and every reason given.
    pub fn read(value: &serde_json::Value) -> (Vec<[[f64; 2]; 2]>, Vec<String>, usize) {
        let old: Inspection = serde_json::from_value(value.clone()).expect("old DTO reads");
        let mut parts = Vec::new();
        let mut reasons = Vec::new();
        for b in &old.bodies {
            for add in [&b.cut_edit, &b.cut_edit_v2] {
                parts.extend(add.target.as_ref().map(|t| t.extents_mm));
                reasons.extend(add.refusal.clone());
            }
        }
        for f in &old.features {
            for edit in [&f.circular_cut_edit, &f.circular_cut_edit_v2] {
                parts.extend(edit.saved.as_ref().map(|t| t.extents_mm));
                reasons.extend(edit.refusal.clone());
            }
            for h in [&f.base_height_edit, &f.base_height_edit_v2] {
                parts.extend(h.as_ref().map(|t| t.extents_mm));
            }
        }
        let histories = old
            .sketches
            .iter()
            .flat_map(|s| [&s.cut_history, &s.cut_history_v2])
            .filter(|h| h.as_ref().is_some_and(|h| !h.tools.is_empty()))
            .count();
        (parts, reasons, histories)
    }
}

#[test]
fn polygon_discovery_refusals_and_writer_checks_without_kernel() {
    for n in [1, 2, 16] {
        let root = tempfile::tempdir().expect("root");
        let tools = tools(n);
        let path = fixture(root.path(), &L, &tools);
        let before = std::fs::read(&path).expect("bytes");
        let catalog = inspect(&path);
        let lines = base_lines(&path);
        assert_eq!(lines.len(), 6);

        // `_v3` states the real boundary everywhere a Cut can be changed.
        let add = &catalog["bodies"][0]["cut_edit_v3"];
        assert_eq!(add["available"], json!(n < 16), "{add}");
        if n < 16 {
            let target = &add["target"];
            check_boundary(&path, target, &L);
            assert_eq!(target["request_versions"], json!([1, 2]));
            assert_eq!(target["tools"].as_array().expect("tools").len(), n);
            for (tool, listed) in tools.iter().zip(target["tools"].as_array().expect("t")) {
                assert_eq!(listed["center_mm"], json!(tool.center_mm));
                assert_eq!(listed["extent"], extent_json(tool.extent));
            }
        } else {
            assert!(add["target"].is_null());
            assert!(add["refusal"].as_str().expect("why").contains("16"));
        }
        let features = catalog["features"].as_array().expect("features");
        let saved: Vec<_> = features
            .iter()
            .filter(|f| !f["circular_cut_edit_v3"]["saved"].is_null())
            .collect();
        assert_eq!(saved.len(), n, "every link is editable");
        for f in &saved {
            let s = &f["circular_cut_edit_v3"]["saved"];
            check_boundary(&path, s, &L);
            assert!(s["extent"].is_object() && s.get("depth_mm").is_none());
        }
        let height = features
            .iter()
            .find(|f| f["base_height_edit_v3"].is_object())
            .expect("base height")["base_height_edit_v3"]
            .clone();
        check_boundary(&path, &height, &L);
        let sketch = catalog["sketches"]
            .as_array()
            .expect("sketches")
            .iter()
            .find(|s| s["cut_history_v3"].is_object())
            .expect("base Sketch")
            .clone();
        check_boundary(&path, &sketch["cut_history_v3"], &L);
        assert_eq!(sketch["editable"], json!(true));
        assert_eq!(
            sketch["vertices"]
                .as_array()
                .expect("vertices")
                .iter()
                .map(|v| v["curve_id"].as_str().expect("id").to_owned())
                .collect::<Vec<_>>(),
            curve_ids(&sketch["cut_history_v3"]["boundary"])
        );

        // An old rectangle consumer is told nothing it could take as a
        // rectangle: every block that meant one is unavailable, with a reason
        // that names the block that can say it.
        let (parts, reasons, histories) = old_rectangle::read(&catalog);
        assert!(
            parts.is_empty(),
            "a bounding box posed as the part: {parts:?}"
        );
        assert_eq!(histories, 0);
        // Both add blocks (unless the 16-link limit speaks first) and both
        // edit blocks of every link point at `_v3`; the base's own refusal
        // is the NewBody one it always was.
        assert_eq!(
            reasons.iter().filter(|r| r.contains("_v3")).count(),
            if n < 16 { 2 } else { 0 } + 2 * n,
            "{reasons:?}"
        );

        // Every Cut names every saved side by its own UUID.
        let d = Document::open_read_only(&path).expect("doc");
        let refs = d.topology_refs().expect("refs");
        for saved in ordered(&path) {
            let sides: std::collections::BTreeSet<_> = refs
                .iter()
                .filter(|r| r.owner == saved.feature)
                .filter_map(|r| match r.output_role {
                    SemanticRole::CarriedSide { profile_segment }
                    | SemanticRole::OriginSide {
                        profile_segment, ..
                    } if lines.iter().any(|l| l.0 == profile_segment) => Some(profile_segment),
                    _ => None,
                })
                .collect();
            assert_eq!(sides, lines.iter().map(|l| l.0).collect(), "real names");
        }
        d.close().expect("close");
        let valid = reply(
            cli()
                .arg("validate")
                .arg(&path)
                .arg("--json")
                .output()
                .expect("validate"),
            "validate",
            0,
        );
        assert_eq!(valid["result"]["valid"], json!(true));
        assert_eq!(std::fs::read(&path).expect("bytes"), before);
    }

    // Refusals by the one rule, from the catalogue and from preparation.
    let root = tempfile::tempdir().expect("root");
    let path = fixture(root.path(), &L, &tools(2));
    let d = Document::open_read_only(&path).expect("doc");
    let objects = d.objects().expect("objects");
    let choice = ferritecad_document::cut_choices(&d, &objects).remove(0);
    let (body, _) = tip(&d);
    let blind = CutExtent::Blind { depth_mm: 4. };
    for (center, radius, why) in [
        ([40., 30.], 3., "inside the bounding box, in the notch"),
        ([17., 17.], 4.3, "over the reflex vertex"),
        ([40., 17.], 4., "across the concave edge"),
        ([40., 10.], 10., "exactly at the clearance"),
        ([30., 10.], 10., "exactly at two walls"),
        ([-1., 10.], 0.5, "outside, beside the part"),
    ] {
        let cut = CircularCut {
            center_mm: center,
            radius_mm: radius,
            extent: blind,
        };
        let draft = choice.validate_cut(&cut).expect_err(why);
        let prepared = prepare_circular_cut(&d, body, &cut).expect_err(why);
        assert_eq!(draft.to_string(), prepared.to_string(), "{why}");
        assert!(
            draft.to_string().contains("stay inside the part"),
            "{why}: {draft}"
        );
    }
    for (center, radius, why) in [
        (
            [17., 17.],
            4.2,
            "clear of the reflex vertex, though not of its lines",
        ),
        ([40., 10.], 10. - 1e-3, "a hair inside the clearance"),
        (
            [10.125, 18.625 + 1.],
            2.25,
            "across y = 20 where it is not a wall",
        ),
    ] {
        let cut = CircularCut {
            center_mm: center,
            radius_mm: radius,
            extent: blind,
        };
        // Only the disk rule is asked here; the saved tools are elsewhere.
        let other = choice.validate_cut(&cut);
        assert!(
            other.is_ok()
                || other
                    .as_ref()
                    .is_err_and(|e| e.to_string().contains("conflicts")),
            "{why}: {other:?}"
        );
    }
    d.close().expect("close");

    // Base edits check every tool, the far ones included, and name the Cut.
    let root = tempfile::tempdir().expect("root");
    let tools16 = tools(16);
    let path = fixture(root.path(), &L, &tools16);
    let saved = ordered(&path);
    let lines = base_lines(&path);
    let vertices = |points: &[[f64; 2]]| -> Vec<SketchVertex> {
        lines
            .iter()
            .zip(points)
            .map(|(l, p)| SketchVertex {
                curve_id: l.0,
                start_mm: *p,
            })
            .collect()
    };
    let moved = |x: f64| {
        let mut p = L;
        p[1][0] = x;
        p[2][0] = x;
        p
    };
    // Slot 9 is link 11 (r 2.25 at x 54.125); the right wall at 56 is 0.375
    // short. Every other link, including slot 4 at r 1.5, is clear.
    let far = &saved[11];
    assert_eq!(far.center_mm, SLOTS[9]);
    let d = Document::open_read_only(&path).expect("doc");
    let error = replace_sketch_coordinates(&d, saved[0].profile_sketch, &vertices(&moved(56.)))
        .expect_err("the far tool");
    let message = error.to_string();
    assert!(message.contains(&far.feature.to_string()), "{message}");
    for other in saved.iter().filter(|s| s.feature != far.feature) {
        assert!(!message.contains(&other.feature.to_string()), "{message}");
    }
    replace_sketch_coordinates(&d, saved[0].profile_sketch, &vertices(&moved(57.)))
        .expect("clear of every tool");
    // Into the notch: the L's reflex vertex moves up and right, over slot 13.
    let mut notch = L;
    notch[3] = [15., 20.];
    notch[4] = [15., 40.];
    let error = replace_sketch_coordinates(&d, saved[0].profile_sketch, &vertices(&notch))
        .expect_err("a moved concave wall");
    assert!(error.to_string().contains("Cut "), "{error}");
    // Order, identities and winding are not coordinates.
    let mut reversed: Vec<_> = vertices(&L);
    reversed.reverse();
    assert!(replace_sketch_coordinates(&d, saved[0].profile_sketch, &reversed).is_err());
    let mut wound = L;
    wound.reverse();
    assert!(replace_sketch_coordinates(&d, saved[0].profile_sketch, &vertices(&wound)).is_err());
    // A height edit re-checks the same rule and keeps the boundary.
    let base = saved[0].base_feature;
    prepare_extrude_height(&d, base, 13.).expect("taller");
    d.close().expect("close");

    // The writer's transaction repeats the rule against the document it
    // writes to: a base edit prepared before a far tool moved is refused.
    let late = root.path().join("late.fcad");
    std::fs::copy(&path, &late).expect("copy");
    let mut d = Document::open(&late).expect("doc");
    let prepared = replace_sketch_coordinates(&d, saved[0].profile_sketch, &vertices(&moved(57.)))
        .expect("valid when prepared");
    let tool = CircularCutEdit {
        tool_curve: far.tool_curve,
        center_mm: [55., far.center_mm[1]],
        radius_mm: far.radius_mm,
        extent: far.extent,
        vocabulary: ExtentVocabulary::BlindOrThroughAll,
    };
    let edit = prepare_cut_parameters(&d, far.feature, &tool).expect("valid in the old base");
    d.write_cut_parameters(&edit).expect("the far tool moves");
    let version = d.content_version().expect("version");
    let error = d
        .write_sketch_geometry(&prepared)
        .expect_err("stale against a far tool");
    assert!(
        error.to_string().contains(&far.feature.to_string()),
        "{error}"
    );
    assert_eq!(d.content_version().expect("version"), version, "atomic");
    // And an add prepared before the base moved.
    let (body, _) = tip(&d);
    let add = prepare_circular_cut(
        &d,
        body,
        &CircularCut {
            center_mm: [6.125, 39. - 0.75 - 0.5],
            radius_mm: 0.5,
            extent: blind,
        },
    );
    // 16 is the limit; the add is refused on count before geometry.
    assert!(add.is_err());
    d.close().expect("close");

    let root = tempfile::tempdir().expect("root");
    let path = fixture(root.path(), &L, &tools(2));
    let mut d = Document::open(&path).expect("doc");
    let (body, _) = tip(&d);
    let lines = base_lines(&path);
    let saved = ordered(&path);
    let prepared = prepare_circular_cut(
        &d,
        body,
        &CircularCut {
            center_mm: [6., 37.],
            radius_mm: 1.,
            extent: blind,
        },
    )
    .expect("valid when prepared");
    let mut lower = L;
    lower[4][1] = 37.5;
    lower[5][1] = 37.5;
    d.write_sketch_coordinates(
        saved[0].profile_sketch,
        &lines
            .iter()
            .zip(lower)
            .map(|(l, p)| SketchVertex {
                curve_id: l.0,
                start_mm: p,
            })
            .collect::<Vec<_>>(),
    )
    .expect("the top wall moves down");
    let version = d.content_version().expect("version");
    let error = d
        .write_circular_cut(&prepared)
        .expect_err("stale against the moved wall");
    assert!(
        error.to_string().contains("stay inside the part"),
        "{error}"
    );
    assert_eq!(d.content_version().expect("version"), version, "atomic");
    d.close().expect("close");
}

#[test]
fn a_rectangle_keeps_every_older_block_and_gains_v3() {
    let root = tempfile::tempdir().expect("root");
    let tools = super::history::tools(3);
    let path = super::history::fixture(root.path(), &tools);
    let catalog = inspect(&path);
    let (parts, reasons, histories) = old_rectangle::read(&catalog);
    // Two add blocks, two edit blocks per Cut, two height blocks.
    assert_eq!(parts.len(), 2 + 2 * 3 + 2);
    assert!(parts.iter().all(|p| *p == [[0., 0.], [SIZE[0], SIZE[1]]]));
    assert!(reasons.iter().all(|r| !r.contains("_v3")), "{reasons:?}");
    assert_eq!(histories, 2);
    let rectangle = [[0., 0.], [SIZE[0], 0.], [SIZE[0], SIZE[1]], [0., SIZE[1]]];
    check_boundary(
        &path,
        &catalog["bodies"][0]["cut_edit_v3"]["target"],
        &rectangle,
    );
    // `_v3` is `_v2` with the part stated as a boundary rather than extents.
    let strip = |mut v: Value, key: &str| {
        let o = v.as_object_mut().expect("object");
        o.remove(key);
        o.remove("bounds_mm");
        o.remove("boundary");
        v
    };
    let body = &catalog["bodies"][0];
    let mut v2 = body["cut_edit_v2"].clone();
    let mut v3 = body["cut_edit_v3"].clone();
    v2["target"] = strip(v2["target"].take(), "extents_mm");
    v3["target"] = strip(v3["target"].take(), "extents_mm");
    assert_eq!(v2, v3);
    // Nothing distinguishes a rectangle's sloped twin in the older blocks.
    let skew_root = tempfile::tempdir().expect("root");
    let skew = fixture(
        skew_root.path(),
        &[[0., 0.], [SIZE[0], 0.5], [SIZE[0], SIZE[1]], [0., SIZE[1]]],
        &self::tools(1),
    );
    let (parts, _, _) = old_rectangle::read(&inspect(&skew));
    assert!(
        parts.is_empty(),
        "a sloped quadrilateral is not a rectangle"
    );
}

/// The analytic volume of the L with its tools, and the mesh the CLI exports.
///
/// A part with no Cut yet is measured by its mesh only: its creation names
/// only some of its faces, and the name checks here are about Cut histories.
fn measured(path: &Path, tools: &[CircularCut], points: &[[f64; 2]], height: f64) -> f64 {
    let m = mesh(path, &path.with_extension("stl"));
    check_part_mesh_at(&m, tools, points, height);
    if tools.is_empty() {
        return m.volume;
    }
    let volume = measure_part_at(path, tools, points, height, None);
    let exact = polygon_area(points) * height - reach(tools, height);
    assert!((volume - exact).abs() < 1e-6, "{volume} != {exact}");
    volume
}

fn add(source: &Path, destination: &Path, cut: CircularCut, code: i32) -> Value {
    let catalog = inspect(source);
    let request = source.with_extension("add.json");
    write(
        &request,
        &json!({"request_version":2,"center_mm":cut.center_mm,"radius_mm":cut.radius_mm,
                "extent":extent_json(cut.extent)}),
    );
    reply(
        cli()
            .arg(OP)
            .arg(source)
            .args([
                "--body",
                catalog["bodies"][0]["body_id"].as_str().expect("body"),
                "--expect-version",
                catalog["content_version"].as_str().expect("version"),
                "--request",
            ])
            .arg(request)
            .arg("-o")
            .arg(destination)
            .arg("--json")
            .output()
            .expect("cut"),
        OP,
        code,
    )
}

fn edit(
    source: &Path,
    destination: &Path,
    saved: &SavedCircularCut,
    cut: CircularCut,
    code: i32,
) -> Value {
    let catalog = inspect(source);
    let request = source.with_extension("edit.json");
    write(
        &request,
        &json!({"request_version":2,"tool_curve_id":saved.tool_curve,
            "center_mm":cut.center_mm,"radius_mm":cut.radius_mm,"extent":extent_json(cut.extent)}),
    );
    reply(
        cli()
            .arg(EDIT)
            .arg(source)
            .args(["--feature", &saved.feature.to_string(), "--expect-version"])
            .arg(catalog["content_version"].as_str().expect("version"))
            .arg("--request")
            .arg(request)
            .arg("-o")
            .arg(destination)
            .arg("--json")
            .output()
            .expect("edit"),
        EDIT,
        code,
    )
}

fn height(source: &Path, destination: &Path, height: f64, code: i32) -> Value {
    let catalog = inspect(source);
    let base = catalog["features"]
        .as_array()
        .expect("features")
        .iter()
        .find(|f| f["base_height_edit_v3"].is_object())
        .expect("base")["feature_id"]
        .as_str()
        .expect("id")
        .to_owned();
    reply(
        cli()
            .arg("edit-extrude")
            .arg(source)
            .args(["--feature", &base, "--expect-version"])
            .arg(catalog["content_version"].as_str().expect("version"))
            .args(["--distance-mm", &height.to_string(), "-o"])
            .arg(destination)
            .arg("--json")
            .output()
            .expect("height"),
        "edit-extrude",
        code,
    )
}

/// Coordinates for the base Sketch, from its `_v3` boundary: same UUIDs,
/// same order, only numbers.
fn sketch(source: &Path, destination: &Path, points: &[[f64; 2]], code: i32) -> Value {
    let catalog = inspect(source);
    let base = catalog["sketches"]
        .as_array()
        .expect("sketches")
        .iter()
        .find(|s| s["cut_history_v3"].is_object())
        .expect("base Sketch")
        .clone();
    let ids = curve_ids(&base["cut_history_v3"]["boundary"]);
    assert_eq!(ids.len(), points.len());
    let vertices: Vec<Value> = ids
        .iter()
        .zip(points)
        .map(|(id, p)| json!({"curve_id":id,"start_mm":p}))
        .collect();
    let request = source.with_extension("sketch.json");
    write(&request, &json!({"request_version":1,"vertices":vertices}));
    reply(
        cli()
            .arg("edit-sketch-copy")
            .arg(source)
            .args(["--sketch", base["sketch_id"].as_str().expect("id")])
            .args([
                "--expect-version",
                catalog["content_version"].as_str().expect("v"),
            ])
            .arg("--request")
            .arg(&request)
            .arg("-o")
            .arg(destination)
            .arg("--json")
            .output()
            .expect("sketch"),
        "edit-sketch-copy",
        code,
    )
}

/// A part made by the shipped creation command.
fn created(root: &Path, points: &[[f64; 2]], name: &str) -> PathBuf {
    let path = root.join(format!("{name}.fcad"));
    let request = root.join(format!("{name}.create.json"));
    write(
        &request,
        &json!({"request_version":1,"points_mm":points,"height_mm":H}),
    );
    reply(
        cli()
            .arg("create-sketch-extrude")
            .arg(&request)
            .arg("-o")
            .arg(&path)
            .arg("--json")
            .output()
            .expect("create"),
        "create-sketch-extrude",
        0,
    );
    path
}

/// The SQL allowlist of one Cut parameter edit: the tool Sketch and Cut
/// payload/hash (and the Cut's layout column when its end changes form), added
/// refs and `meta.modified_at`. Every other cell, rowid and byte is kept.
fn only_the_cut(source: &Path, copy: &Path, saved: &SavedCircularCut, added: usize) {
    use rusqlite::types::Value as Sql;
    let a = tables(source);
    let b = tables(copy);
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    for (name, (columns, rows)) in &a {
        let (cols, now) = &b[name];
        assert_eq!(columns, cols);
        if name == "topology_refs" {
            assert_eq!(now.len(), rows.len() + added, "added refs");
            assert!(rows.iter().all(|r| now.contains(r)), "a saved ref moved");
            continue;
        }
        assert_eq!(rows.len(), now.len(), "{name} rows");
        for (old, new) in rows.iter().zip(now) {
            for (i, (x, y)) in old.iter().zip(new).enumerate() {
                if x == y {
                    continue;
                }
                let id = columns.iter().position(|c| c == "id");
                let row =
                    |v: ObjectId| id.is_some_and(|id| old[id] == Sql::Blob(v.to_bytes().to_vec()));
                let allowed = match name.as_str() {
                    "meta" => columns[i] == "modified_at",
                    "objects" => {
                        (row(saved.feature) || row(saved.tool_sketch))
                            && matches!(columns[i].as_str(), "payload" | "payload_hash")
                            || row(saved.feature) && columns[i] == "schema_version"
                    }
                    _ => false,
                };
                assert!(
                    allowed,
                    "unexpected cell {name}.{}: {x:?} -> {y:?}",
                    columns[i]
                );
            }
        }
    }
}

/// The L with 1, 2 and 16 Cuts: add, edit the first, a middle and the last
/// link, base height and base coordinates, and refusals on the notch, the
/// reflex vertex and a far wall — every result measured, meshed and read back.
#[test]
fn native_l_profile_history_matrix() {
    if !native() {
        return;
    }
    use CacheOutcome::{Hit, Miss};
    let mut artifact = 0;
    for n in [1, 2, 16] {
        let root = tempfile::tempdir().expect("root");
        let tools = tools(n);
        let mut source = created(root.path(), &L, "L");
        let plate = measured(&source, &[], &L, H);
        assert!((plate - 16000.).abs() < 1e-3, "{plate}");
        for (i, tool) in tools.iter().enumerate() {
            let next = root.path().join(format!("link-{}.fcad", i + 1));
            let out = add(&source, &next, *tool, 0);
            assert_eq!(out["result"]["extent"], extent_json(tool.extent));
            source = next;
        }
        let original = std::fs::read(&source).expect("bytes");
        let cold = measured(&source, &tools, &L, H);
        assert!((cold - (16000. - reach(&tools, H))).abs() < 1e-6);
        assert_eq!(
            cold,
            measure_part_at(&source, &tools, &L, H, Some(&vec![Miss; n + 1]))
        );
        assert_eq!(
            cold,
            measure_part_at(&source, &tools, &L, H, Some(&vec![Hit; n + 1]))
        );
        let ordered = ordered(&source);
        let never = root.path().join("never.fcad");

        // Refused by the real boundary, atomically, whatever the box says.
        if n < 16 {
            for (center, radius) in [([40., 30.], 3.), ([17., 17.], 4.3), ([40., 17.], 4.)] {
                let out = add(
                    &source,
                    &never,
                    CircularCut {
                        center_mm: center,
                        radius_mm: radius,
                        extent: CutExtent::ThroughAll,
                    },
                    2,
                );
                let message = out["error"]["message"].as_str().expect("message");
                assert!(message.contains("stay inside the part"), "{message}");
                assert!(!never.exists());
            }
        }

        // Edit the first, a middle and the last link.
        for index in std::collections::BTreeSet::from([0, n / 2, n - 1]) {
            let saved = &ordered[index];
            let tool = tools[index];
            let copy = root.path().join(format!("edit-{index}.fcad"));
            let changed = CircularCut {
                center_mm: [tool.center_mm[0] - 0.25, tool.center_mm[1] + 0.125],
                radius_mm: tool.radius_mm - 0.125,
                ..tool
            };
            edit(&source, &copy, saved, changed, 0);
            only_the_cut(&source, &copy, saved, 0);
            let mut all = tools.clone();
            all[index] = changed;
            measured(&copy, &all, &L, H);
            // Into the notch: refused, and names nothing it did not check.
            let out = edit(
                &source,
                &never,
                saved,
                CircularCut {
                    center_mm: [40., 30.],
                    ..tool
                },
                2,
            );
            let message = out["error"]["message"].as_str().expect("message");
            assert!(message.contains("outside the part"), "{message}");
            assert!(!never.exists());
        }

        // Height: ThroughAll follows, Blind stays absolute.
        let grown = root.path().join("grown.fcad");
        height(&source, &grown, 13., 0);
        measured(&grown, &tools, &L, 13.);
        assert_eq!(
            measure_part_at(&grown, &tools, &L, 13., Some(&vec![Miss; n + 1])),
            measure_part_at(&grown, &tools, &L, 13., Some(&vec![Hit; n + 1]))
        );

        // Coordinates: the L widens and its notch moves, every tool clear.
        let wider = [
            [-2., -1.5],
            [61.25, 0.],
            [61.25, 21.],
            [20.5, 21.],
            [20.5, 41.],
            [0., 40.],
        ];
        let moved = root.path().join("moved.fcad");
        sketch(&grown, &moved, &wider, 0);
        measured(&moved, &tools, &wider, 13.);
        let reopened = inspect(&moved);
        let base = reopened["features"]
            .as_array()
            .expect("features")
            .iter()
            .find(|f| f["base_height_edit_v3"].is_object())
            .expect("base")["base_height_edit_v3"]
            .clone();
        check_boundary(&moved, &base, &wider);
        if n == 2 && artifact < 2 {
            for path in [&grown, &moved] {
                if let Some(dir) = std::env::var_os("FCAD_POLYGON_CUT_ARTIFACTS") {
                    let dir = Path::new(&dir);
                    std::fs::create_dir_all(dir).expect("artifacts");
                    reply(
                        cli()
                            .arg("export-fbx")
                            .arg(path)
                            .arg("-o")
                            .arg(dir.join(format!("polygon-{artifact}.fbx")))
                            .arg("--json")
                            .output()
                            .expect("fbx"),
                        "export-fbx",
                        0,
                    );
                }
                artifact += 1;
            }
        }
        if n == 16 {
            // A far wall: the right side at 56 misses link 11 (slot 9) only.
            let mut near = L;
            near[1][0] = 56.;
            near[2][0] = 56.;
            let out = sketch(&source, &never, &near, 2);
            let message = out["error"]["message"].as_str().expect("message");
            assert!(
                message.contains(&ordered[11].feature.to_string()),
                "{message}"
            );
            assert!(!never.exists());
        }
        assert_eq!(std::fs::read(&source).expect("source"), original);
    }
    assert_eq!(artifact, 2);
}

/// Sloped convex parts, both windings, far from the origin: every wall is
/// found by its saved UUID and faces out of the part.
#[test]
fn native_sloped_polygons_both_windings_translated() {
    if !native() {
        return;
    }
    let shift = [1000.25, -250.5];
    let triangle = [[0., 0.], [50., 5.], [10., 40.]];
    let quad = [[0., 0.], [50., 10.], [45., 45.], [-5., 35.]];
    for (name, shape, inside, over) in [
        (
            "triangle",
            &triangle[..],
            [[20., 15.], [15., 28.]],
            [30., 22.],
        ),
        ("quad", &quad[..], [[15., 12.], [30., 30.]], [44., 30.]),
    ] {
        for reversed in [false, true] {
            let root = tempfile::tempdir().expect("root");
            let mut points: Vec<_> = shape
                .iter()
                .map(|p| [p[0] + shift[0], p[1] + shift[1]])
                .collect();
            if reversed {
                points.reverse();
            }
            let tools: Vec<_> = inside
                .iter()
                .zip([CutExtent::ThroughAll, CutExtent::Blind { depth_mm: 4.5 }])
                .map(|(c, extent)| CircularCut {
                    center_mm: [c[0] + shift[0], c[1] + shift[1]],
                    radius_mm: 3.,
                    extent,
                })
                .collect();
            let mut source = created(root.path(), &points, name);
            for (i, tool) in tools.iter().enumerate() {
                let next = root.path().join(format!("{name}-{i}.fcad"));
                add(&source, &next, *tool, 0);
                source = next;
            }
            measured(&source, &tools, &points, H);
            let target = &inspect(&source)["bodies"][0]["cut_edit_v3"]["target"];
            check_boundary(&source, target, &points);
            let boundary: CutBoundary = ordered(&source).remove(0).boundary;
            assert_eq!(boundary.segments().len(), shape.len());
            assert_eq!(boundary.rectangle_mm(), None);
            // Inside the bounding box, over a sloped wall: refused.
            let never = root.path().join("never.fcad");
            let out = add(
                &source,
                &never,
                CircularCut {
                    center_mm: [over[0] + shift[0], over[1] + shift[1]],
                    radius_mm: 4.,
                    extent: CutExtent::ThroughAll,
                },
                2,
            );
            assert!(
                out["error"]["message"]
                    .as_str()
                    .expect("message")
                    .contains("stay inside the part")
            );
            assert!(!never.exists());
        }
    }
}
