// SPDX-License-Identifier: MIT
#![allow(dead_code, clippy::panic, reason = "shared test support")]
use ferritecad_document::{Document, ObjectPayload};
use ferritecad_eval::RebuildResult;
use ferritecad_kernel::{
    NamingInput, NamingOperation, OperationContext, SubShapeHandle, SubShapeKind,
};
use ferritecad_topology::generative::{Name, cap_root, curve_root, generated};
use ferritecad_types::ObjectId;

pub type Names = Vec<(ObjectId, Name)>;

pub fn face_count(names: &Names) -> usize {
    names
        .iter()
        .filter(|(_, name)| name.kind == SubShapeKind::Face)
        .count()
}

pub fn printed_face_descriptions(text: &str) -> Vec<&str> {
    text.lines()
        .zip(text.lines().skip(1))
        .filter_map(|(header, description)| header.starts_with("    face ").then_some(description))
        .collect()
}

pub fn snapshot(document: &Document) -> Names {
    if ferritecad_occt::is_available() {
        snapshot_with(
            document,
            &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        )
    } else {
        snapshot_with(document, &mut ferritecad_kernel::mock::MockKernel::new())
    }
}
fn snapshot_with(
    document: &Document,
    kernel: &mut impl ferritecad_kernel::GeometryKernel,
) -> Names {
    let built = ferritecad_eval::rebuild_cold(document, kernel, &OperationContext::default())
        .expect("rebuild names");
    assert!(built.all_built(), "{:?}", built.report());
    let names = primary(&built);
    built.release_all(kernel);
    names
}
pub fn primary(built: &RebuildResult) -> Names {
    built
        .topology()
        .features
        .iter()
        .flat_map(|(id, feature)| feature.bindings.keys().map(|name| (*id, *name)))
        .collect()
}
pub fn assert_primary(built: &RebuildResult) {
    for (feature, name) in primary(built) {
        let entities = &built.topology().features[&feature].bindings[&name];
        assert!(!entities.is_empty());
        for entity in entities {
            assert_eq!(entity.kind(), name.kind);
            assert_eq!(Some(entity.shape()), built.shape(feature));
            assert_eq!(
                built.topology().features[&feature].entities.get(entity),
                Some(&name)
            );
        }
        assert!(
            !built
                .topology()
                .describe(feature, name)
                .expect("description")
                .is_empty()
        );
    }
}
pub fn root_faces(
    document: &Document,
    built: &RebuildResult,
) -> Vec<(ObjectId, NamingInput, SubShapeHandle)> {
    let objects = document.objects().expect("objects");
    let mut faces = Vec::new();
    for object in &objects {
        let (profile, operation, caps) = match &object.payload {
            ObjectPayload::Extrude(e) if e.previous.is_none() => {
                (e.profile, NamingOperation::Extrude, true)
            }
            ObjectPayload::Revolve(r) => (
                r.profile,
                NamingOperation::Revolve,
                matches!(r.extent, ferritecad_document::RevolveExtent::Partial { .. }),
            ),
            _ => continue,
        };
        let sketch = objects.iter().find(|o| o.id == profile).expect("profile");
        let ObjectPayload::Sketch(sketch) = &sketch.payload else {
            panic!("sketch")
        };
        let names = &built.topology().features[&object.id];
        let roots = sketch
            .curves
            .iter()
            .filter(|c| !c.construction)
            .map(|c| {
                (
                    NamingInput::Curve(c.id),
                    curve_root(object.id, c.id, Some(sketch.plane)),
                )
            })
            .chain([false, true].into_iter().filter(|_| caps).map(|end| {
                (
                    NamingInput::Cap { end },
                    cap_root(object.id, end, Some(sketch.plane)),
                )
            }));
        for (input, root) in roots {
            let name = generated(object.id, operation, root, SubShapeKind::Face);
            if let Some(entities) = names.bindings.get(&name) {
                faces.extend(entities.iter().map(|entity| (object.id, input, *entity)));
            }
        }
    }
    faces
}

pub fn assert_root_faces(document: &Document, names: &Names) {
    let objects = document.objects().expect("objects");
    for object in &objects {
        let ObjectPayload::Extrude(extrude) = &object.payload else {
            continue;
        };
        if extrude.previous.is_some() {
            continue;
        }
        let ObjectPayload::Sketch(sketch) = &objects
            .iter()
            .find(|o| o.id == extrude.profile)
            .expect("profile")
            .payload
        else {
            panic!("sketch")
        };
        for root in sketch
            .curves
            .iter()
            .filter(|c| !c.construction)
            .map(|c| curve_root(object.id, c.id, Some(sketch.plane)))
            .chain([false, true].map(|end| cap_root(object.id, end, Some(sketch.plane))))
        {
            assert!(
                names.contains(&(
                    object.id,
                    generated(
                        object.id,
                        NamingOperation::Extrude,
                        root,
                        SubShapeKind::Face
                    )
                )),
                "a root-generated face is missing"
            );
        }
    }
}

pub fn root_name(
    document: &Document,
    feature: ObjectId,
    input: NamingInput,
    operation: NamingOperation,
) -> Name {
    let object = document
        .object(feature)
        .expect("read feature")
        .expect("feature");
    let profile = match object.payload {
        ObjectPayload::Extrude(e) => e.profile,
        ObjectPayload::Revolve(r) => r.profile,
        _ => panic!("sweep feature"),
    };
    let ObjectPayload::Sketch(sketch) = document
        .object(profile)
        .expect("read profile")
        .expect("profile")
        .payload
    else {
        panic!("sketch")
    };
    let root = match input {
        NamingInput::Curve(curve) => curve_root(feature, curve, Some(sketch.plane)),
        NamingInput::Cap { end } => cap_root(feature, end, Some(sketch.plane)),
        _ => panic!("sweep input"),
    };
    generated(feature, operation, root, SubShapeKind::Face)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceInput {
    Curve(ferritecad_types::StableEntityId),
    Cap(bool),
    Edge(NamingOperation),
}
#[derive(Debug)]
pub struct GeneratedFace {
    pub at: ObjectId,
    pub origin: ObjectId,
    pub input: FaceInput,
    pub seed: Name,
    pub entities: Vec<SubShapeHandle>,
}

// Independent expressions built from the fixture's inputs. Geometry assertions use these
// ancestry queries, while snapshot() compares the complete primary output names.
pub fn generated_faces(document: &Document, built: &RebuildResult) -> Vec<GeneratedFace> {
    let mut seeds = Vec::new();
    for object in document.objects().expect("objects") {
        match object.payload {
            ObjectPayload::Extrude(e) => {
                let ObjectPayload::Sketch(sketch) = document
                    .object(e.profile)
                    .expect("profile")
                    .expect("row")
                    .payload
                else {
                    panic!("sketch")
                };
                for input in sketch
                    .curves
                    .iter()
                    .filter(|c| !c.construction)
                    .map(|c| NamingInput::Curve(c.id))
                    .chain([false, true].map(|end| NamingInput::Cap { end }))
                {
                    let key = match input {
                        NamingInput::Curve(curve) => FaceInput::Curve(curve),
                        NamingInput::Cap { end } => FaceInput::Cap(end),
                        _ => unreachable!(),
                    };
                    seeds.push((
                        object.id,
                        key,
                        root_name(document, object.id, input, NamingOperation::Extrude),
                    ));
                }
            }
            ObjectPayload::Fillet(f) => {
                let edge = built
                    .topology()
                    .swept_edges(f.previous, f.edge)
                    .expect("consumed edge");
                let input =
                    built.topology().features[&f.previous].entities[edge.first().expect("edge")];
                seeds.push((
                    object.id,
                    FaceInput::Edge(NamingOperation::Fillet),
                    generated(
                        object.id,
                        NamingOperation::Fillet,
                        input,
                        SubShapeKind::Face,
                    ),
                ));
            }
            ObjectPayload::Chamfer(c) => {
                let edge = built
                    .topology()
                    .swept_edges(c.previous, c.edge)
                    .expect("consumed edge");
                let input =
                    built.topology().features[&c.previous].entities[edge.first().expect("edge")];
                seeds.push((
                    object.id,
                    FaceInput::Edge(NamingOperation::Chamfer),
                    generated(
                        object.id,
                        NamingOperation::Chamfer,
                        input,
                        SubShapeKind::Face,
                    ),
                ));
            }
            _ => {}
        }
    }
    let mut result = Vec::new();
    for (at, feature) in &built.topology().features {
        for (origin, input, seed) in &seeds {
            if let Some(entities) = feature.ancestry.get(seed)
                && !entities.is_empty()
            {
                result.push(GeneratedFace {
                    at: *at,
                    origin: *origin,
                    input: *input,
                    seed: *seed,
                    entities: entities.iter().copied().collect(),
                });
            }
        }
    }
    result
}

impl GeneratedFace {
    pub fn label(&self) -> String {
        let input = match self.input {
            FaceInput::Curve(_) => "curve".to_owned(),
            FaceInput::Cap(end) => format!("{} cap", if end { "end" } else { "start" }),
            FaceInput::Edge(operation) => format!("generated {operation:?}"),
        };
        if self.at == self.origin {
            input
        } else {
            format!("inherited {input}")
        }
    }
}

pub fn assert_added(before: &Names, after: &Names, feature: ObjectId, faces: usize) {
    assert!(
        before.iter().all(|name| after.contains(name)),
        "an earlier name was lost"
    );
    let added: Vec<_> = after.iter().filter(|name| !before.contains(name)).collect();
    assert!(added.iter().all(|(at, _)| *at == feature));
    assert_eq!(
        added
            .iter()
            .filter(|(_, name)| name.kind == SubShapeKind::Face)
            .count(),
        faces
    );
}

pub fn floor_names(document: &Document, origin: ObjectId) -> Names {
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let built = ferritecad_eval::rebuild_cold(document, &mut kernel, &OperationContext::default())
        .expect("rebuild");
    assert!(built.all_built(), "{:?}", built.report());
    let mut names = Vec::new();
    for face in generated_faces(document, &built) {
        if face.origin == origin && face.input == FaceInput::Cap(true) {
            for entity in face.entities {
                names.push((
                    face.at,
                    built.topology().features[&face.at].entities[&entity],
                ));
            }
        }
    }
    names.sort();
    names.dedup();
    built.release_all(&mut kernel);
    names
}
