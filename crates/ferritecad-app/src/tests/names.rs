// SPDX-License-Identifier: MIT
#![allow(clippy::panic, reason = "test fixture invariants")]
#[path = "../../../ferritecad-fixtures/tests/support/model_names.rs"]
mod model_names;

pub(super) fn names(
    document: &ferritecad_document::Document,
) -> std::collections::BTreeMap<
    ferritecad_types::ObjectId,
    std::collections::BTreeSet<(String, ferritecad_types::ContentHash)>,
> {
    if ferritecad_occt::is_available() {
        model_names::names_with(
            document,
            &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        )
    } else {
        model_names::names(document)
    }
}

pub(super) fn assert_names(
    built: &ferritecad_eval::RebuildResult,
    expected: &std::collections::BTreeMap<
        ferritecad_types::ObjectId,
        std::collections::BTreeSet<(String, ferritecad_types::ContentHash)>,
    >,
) {
    let actual: std::collections::BTreeMap<_, _> = built
        .topology()
        .features
        .iter()
        .map(|(id, feature)| {
            (
                *id,
                feature
                    .bindings
                    .keys()
                    .map(|n| (n.kind.to_string(), n.digest))
                    .collect(),
            )
        })
        .collect();
    assert_eq!(&actual, expected, "same primary names after rebuild");
}

pub(super) fn root_faces(
    document: &ferritecad_document::Document,
    built: &ferritecad_eval::RebuildResult,
) -> Vec<(
    ferritecad_kernel::NamingInput,
    ferritecad_kernel::SubShapeHandle,
)> {
    use ferritecad_document::ObjectPayload;
    use ferritecad_kernel::{NamingInput, NamingOperation, SubShapeKind};
    use ferritecad_topology::generative::{cap_root, curve_root, generated};
    let objects = document.objects().expect("objects");
    let mut faces = Vec::new();
    for object in &objects {
        let ObjectPayload::Extrude(extrude) = &object.payload else {
            continue;
        };
        if extrude.previous.is_some() {
            continue;
        }
        let sketch = objects
            .iter()
            .find(|o| o.id == extrude.profile)
            .expect("profile");
        let ObjectPayload::Sketch(sketch) = &sketch.payload else {
            panic!("sketch");
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
            .chain([false, true].map(|end| {
                (
                    NamingInput::Cap { end },
                    cap_root(object.id, end, Some(sketch.plane)),
                )
            }));
        for (input, root) in roots {
            let name = generated(
                object.id,
                NamingOperation::Extrude,
                root,
                SubShapeKind::Face,
            );
            faces.push((input, names.resolve(name).expect("root-generated face")));
        }
    }
    faces
}

pub(super) type FlatNames = Vec<(
    ferritecad_types::ObjectId,
    String,
    ferritecad_types::ContentHash,
)>;
pub(super) fn flat_names(path: &std::path::Path) -> FlatNames {
    names(&ferritecad_document::Document::open_read_only(path).expect("document"))
        .into_iter()
        .flat_map(|(feature, names)| {
            names
                .into_iter()
                .map(move |(kind, digest)| (feature, kind, digest))
        })
        .collect()
}
pub(super) fn assert_flat_names(built: &ferritecad_eval::RebuildResult, expected: &FlatNames) {
    let mut actual: FlatNames = built
        .topology()
        .features
        .iter()
        .flat_map(|(feature, names)| {
            names
                .bindings
                .keys()
                .map(|n| (*feature, n.kind.to_string(), n.digest))
        })
        .collect();
    actual.sort();
    assert_eq!(&actual, expected);
}
