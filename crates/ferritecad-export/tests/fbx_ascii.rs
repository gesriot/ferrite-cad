// SPDX-License-Identifier: MIT
//! The §22B-1a measured scene, written by the production FBX 7.4 ASCII writer.
//!
//! The values here are the ones the Unity 6000.4.10f1 measurement settled:
//! the axis and unit metadata, the coordinate map `(x, y, z) -> (x, z, -y)`
//! scaled by `0.001`, the polygon order, the authored normals, the two
//! material slots, and the hierarchy in which one geometry is placed twice.
//! Nothing here reads the committed measurement fixture: what is measured is
//! what the writer produced.

// A gate asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

mod fbx_scene;

use fbx_scene::{Fbx, escaping_scene, measured_scene, measured_scene_with_identities};

use ferritecad_exchange::{Diagnostic, Severity, Stage};
use ferritecad_export::{
    ExportColourOrigin, ExportGeometry, ExportMaterial, ExportMesh, ExportOccurrence,
    ExportOmission, ExportProvenance, ExportSceneBuilder, ExportSource, ExportTransform,
    write_fbx_ascii_7400,
};
use ferritecad_kernel::TessellationRefusal;
use ferritecad_types::{ErrorKind, ImportedSourceId, ObjectId};

fn written(scene: &ferritecad_export::ExportScene) -> (Vec<u8>, ferritecad_export::FbxWriteReport) {
    let mut bytes = Vec::new();
    let report = write_fbx_ascii_7400(scene, &mut bytes).expect("the measured scene is writable");
    (bytes, report)
}

fn parsed(bytes: &[u8]) -> Fbx {
    Fbx::parse(std::str::from_utf8(bytes).expect("the writer emits UTF-8"))
}

#[test]
fn the_file_says_it_is_fbx_7400_ascii_and_nothing_about_when_it_was_made() {
    let (bytes, _) = written(&measured_scene());
    let text = std::str::from_utf8(&bytes).expect("UTF-8");
    let file = parsed(&bytes);

    assert_eq!(
        file.at("FBXHeaderExtension/FBXVersion").number(),
        7400.0,
        "the one measured version"
    );
    assert_eq!(
        file.at("FBXHeaderExtension/FBXHeaderVersion").number(),
        1003.0
    );
    assert_eq!(file.at("FBXHeaderExtension/EncryptionType").number(), 0.0);

    // Where other writers put a clock, a host name or a random identifier,
    // this one has constants. Checked as the exact lines they are, so a value
    // derived from the machine is a failure here rather than a file that
    // differs from itself tomorrow.
    assert_eq!(
        file.at("FBXHeaderExtension/CreationTimeStamp/Year")
            .number(),
        2000.0
    );
    for (field, expected) in [
        ("Month", 1.0),
        ("Day", 1.0),
        ("Hour", 0.0),
        ("Minute", 0.0),
        ("Second", 0.0),
        ("Millisecond", 0.0),
    ] {
        assert_eq!(
            file.at(&format!("FBXHeaderExtension/CreationTimeStamp/{field}"))
                .number(),
            expected
        );
    }
    assert!(
        text.contains("CreationTime: \"2000-01-01 00:00:00:000\"\n"),
        "the creation time is not the constant it must be"
    );
    assert!(
        text.contains("FileId: \"FCAD-FBX-7400-ASCII\"\n"),
        "the file identifier is not the constant it must be"
    );
    assert_eq!(file.at("CreationTime").text(), "2000-01-01 00:00:00:000");
    assert_eq!(file.at("FileId").text(), "FCAD-FBX-7400-ASCII");

    // And no absolute path, host name or address reached the file.
    for forbidden in ["/Users/", "/home/", "C:\\", "@", "://"] {
        assert!(
            !text.contains(forbidden),
            "the file contains {forbidden}, which is not a function of the scene"
        );
    }
}

#[test]
fn the_global_settings_are_the_one_measured_axis_and_unit_contract() {
    let (bytes, _) = written(&measured_scene());
    let file = parsed(&bytes);
    let settings = file.at("GlobalSettings/Properties70");

    for (name, expected) in [
        ("CoordAxis", 0.0),
        ("CoordAxisSign", 1.0),
        ("UpAxis", 1.0),
        ("UpAxisSign", 1.0),
        ("FrontAxis", 2.0),
        ("FrontAxisSign", 1.0),
        ("OriginalUpAxis", 1.0),
        ("OriginalUpAxisSign", 1.0),
        ("UnitScaleFactor", 100.0),
        ("OriginalUnitScaleFactor", 100.0),
    ] {
        assert_eq!(
            settings.property(name).numbers()[0],
            expected,
            "{name} is not the measured value"
        );
    }
}

#[test]
fn the_geometry_is_converted_exactly_once_and_keeps_its_polygon_order() {
    let (bytes, _) = written(&measured_scene());
    let file = parsed(&bytes);
    let geometries = file.all("Objects/Geometry");
    assert_eq!(
        geometries.len(),
        1,
        "one definition with a mesh, one geometry"
    );
    let geometry = &geometries[0];

    // (x, y, z) -> (x, z, -y) * 0.001, applied once. The asymmetric 1000 /
    // 2000 / 3000 mm reference becomes 1 / 2 / 3 metres.
    assert_eq!(
        geometry.child("Vertices").numbers(),
        vec![
            0.0, 0.0, 0.0, //
            1.0, 0.0, 0.0, //
            0.0, 0.0, -2.0, //
            0.0, 3.0, 0.0,
        ]
    );

    // The terminal corner of each polygon is written as its bitwise negation,
    // and the FerriteCAD order is kept.
    assert_eq!(
        geometry.child("PolygonVertexIndex").numbers(),
        vec![
            0.0, 2.0, -2.0, //
            0.0, 1.0, -4.0, //
            0.0, 3.0, -3.0, //
            1.0, 2.0, -4.0,
        ]
    );

    let normals = geometry.child("LayerElementNormal");
    assert_eq!(
        normals.child("MappingInformationType").text(),
        "ByPolygonVertex"
    );
    assert_eq!(normals.child("ReferenceInformationType").text(), "Direct");
    // The authored normals, rotated and neither recalculated nor averaged.
    assert_eq!(
        normals.child("Normals").numbers(),
        vec![
            1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -1.0, //
            1.0, 0.0, 0.0, 0.0, 0.0, -1.0, -1.0, 0.0, 0.0, //
            1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, -1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0,
        ]
    );

    let materials = geometry.child("LayerElementMaterial");
    assert_eq!(
        materials.child("MappingInformationType").text(),
        "ByPolygon"
    );
    assert_eq!(
        materials.child("ReferenceInformationType").text(),
        "IndexToDirect"
    );
    assert_eq!(
        materials.child("Materials").numbers(),
        vec![0.0, 0.0, 1.0, 1.0],
        "the two measured slots stay two slots in source order"
    );
}

#[test]
fn every_node_is_a_model_and_the_hierarchy_is_the_scenes() {
    let (bytes, report) = written(&measured_scene());
    let file = parsed(&bytes);
    let models = file.all("Objects/Model");
    assert_eq!(models.len(), 9, "one model per node, whatever it holds");
    assert_eq!(report.models(), 9);
    assert_eq!(report.geometries(), 1);

    let names: Vec<String> = models.iter().map(|model| model.object_name()).collect();
    assert_eq!(
        names,
        vec![
            "Assembly Root",
            "Assembly Frame",
            "Repeated Part",
            "Repeated Part",
            "Omitted #2583",
            "CP Origin",
            "CP X1000",
            "CP Y2000",
            "CP Z3000",
        ],
        "the writer neither merges nor renames two siblings called the same thing"
    );

    // Identity is the number, and the number does not come from the name.
    let ids: Vec<i64> = models.iter().map(|model| model.object_id()).collect();
    let unique: std::collections::BTreeSet<i64> = ids.iter().copied().collect();
    assert_eq!(unique.len(), 9, "two models share an identity");
    assert_eq!(
        ids[2] + 1,
        ids[3],
        "identity follows scene order, not the name"
    );

    // A node with geometry is a Mesh; structure and an omission are Nulls.
    let kinds: Vec<String> = models.iter().map(|model| model.class()).collect();
    assert_eq!(
        kinds,
        vec![
            "Null", "Null", "Mesh", "Mesh", "Null", "Null", "Null", "Null", "Null"
        ]
    );

    // Parent-child connections repeat ExportNode::parent exactly.
    let parents = file.parents();
    assert_eq!(parents[&ids[0]], 0, "the scene root sits at the file root");
    assert_eq!(parents[&ids[1]], ids[0]);
    assert_eq!(parents[&ids[2]], ids[1]);
    assert_eq!(parents[&ids[3]], ids[1]);
    assert_eq!(parents[&ids[4]], ids[1]);
    for control in 5..9 {
        assert_eq!(
            parents[&ids[control]], ids[2],
            "a control point lost its parent"
        );
    }
}

#[test]
fn two_placements_of_one_definition_share_one_geometry() {
    let (bytes, _) = written(&measured_scene());
    let file = parsed(&bytes);
    let geometry = file.all("Objects/Geometry")[0].object_id();
    let models = file.all("Objects/Model");
    let first = models[2].object_id();
    let second = models[3].object_id();

    let attached = file.connections_from(geometry);
    assert_eq!(
        attached,
        vec![first, second],
        "one Geometry object is connected to both placements"
    );
}

#[test]
fn a_node_colour_override_is_a_binding_and_not_a_change_to_the_definition() {
    let (bytes, report) = written(&measured_scene());
    let file = parsed(&bytes);
    let models = file.all("Objects/Model");
    let plain = models[2].object_id();
    let overridden = models[3].object_id();

    let first: Vec<i64> = file.materials_of(plain);
    let second: Vec<i64> = file.materials_of(overridden);
    assert_eq!(
        first.len(),
        2,
        "the two slots survive on the plain placement"
    );
    assert_eq!(second.len(), 2, "and on the overridden one");
    assert_ne!(first, second, "the override did not bind its own materials");
    assert_eq!(report.materials(), 4);

    let colour_of = |id: i64| file.material(id).at_property("DiffuseColor").numbers();
    // The definition's own slots keep the colours the definition recorded.
    assert!(
        colour_of(first[0])[0] > colour_of(first[0])[2],
        "slot 0 is the red one"
    );
    assert!(
        colour_of(first[1])[2] > colour_of(first[1])[0],
        "slot 1 is the blue one"
    );
    // The overriding placement's slots are the override, in the same order.
    assert_eq!(colour_of(second[0]), colour_of(second[1]));
    assert_ne!(colour_of(second[0]), colour_of(first[0]));
}

#[test]
fn an_omitted_definition_is_a_node_with_no_geometry_and_says_why() {
    let scene = measured_scene();
    let (bytes, report) = written(&scene);
    let file = parsed(&bytes);
    let models = file.all("Objects/Model");
    let omitted = &models[4];

    assert!(
        file.connections_to(omitted.object_id())
            .iter()
            .all(|from| !file.is_geometry(*from)),
        "an omitted definition was given triangles"
    );

    let properties = omitted.user_properties();
    assert_eq!(
        properties
            .get("FerriteCADGeometryOmission")
            .map(String::as_str),
        Some("step.product_definition#2583")
    );
    assert_eq!(
        properties
            .get("FerriteCADDefinitionKey")
            .map(String::as_str),
        Some("step.product_definition#2583")
    );
    assert_eq!(
        properties
            .get("FerriteCADOmissionFinding")
            .map(String::as_str),
        Some("step.product_definition#2583")
    );
    assert_eq!(
        properties
            .get("FerriteCADOmissionRefusal")
            .map(String::as_str),
        Some("IncompleteFace"),
        "the stable name of the typed refusal, not a Debug rendering"
    );
    assert_eq!(
        properties.get("FerriteCADComplete").map(String::as_str),
        Some("0")
    );

    // Structure is not an omission and carries no marker.
    for structural in [&models[0], &models[1], &models[5]] {
        let properties = structural.user_properties();
        assert!(
            !properties.contains_key("FerriteCADGeometryOmission"),
            "{} was marked as a missing part",
            structural.object_name()
        );
        assert!(properties.contains_key("FerriteCADNodeKey"));
    }

    // And the report says the same thing the scene's completeness does.
    assert!(!report.is_complete());
    assert_eq!(
        report.omissions(),
        scene.completeness().omissions(),
        "the writer discarded part of the scene's completeness report"
    );
}

#[test]
fn two_sources_with_one_local_key_remain_distinct_in_the_report() {
    let first_source = ImportedSourceId::new();
    let second_source = ImportedSourceId::new();
    let key = "step.product_definition#31";
    let mut builder = ExportSceneBuilder::new();

    for (source, message) in [
        (first_source, "first source finding"),
        (second_source, "second source finding"),
    ] {
        let definition = builder
            .definition(
                ExportSource::Imported {
                    source,
                    definition_key: key.to_owned(),
                },
                None,
                ExportProvenance::default(),
                ExportGeometry::Omitted(ExportOmission::new(
                    Diagnostic {
                        stage: Stage::Validation,
                        severity: Severity::Fail,
                        entity: key.to_owned(),
                        message: message.to_owned(),
                    },
                    TessellationRefusal::IncompleteFace,
                )),
            )
            .expect("a source-local omission");
        builder
            .node(
                None,
                definition,
                ExportTransform::IDENTITY,
                None,
                None,
                ExportOccurrence::Unrecorded,
            )
            .expect("the omitted definition remains placed");
    }

    let scene = builder.finish().expect("two distinct source identities");
    let (_, report) = written(&scene);
    assert_eq!(report.omissions().len(), 2);
    assert_eq!(
        report.omissions()[0].source,
        ExportSource::Imported {
            source: first_source,
            definition_key: key.to_owned(),
        }
    );
    assert_eq!(
        report.omissions()[1].source,
        ExportSource::Imported {
            source: second_source,
            definition_key: key.to_owned(),
        }
    );
    assert_eq!(
        report.omissions()[0].omission.finding.message,
        "first source finding"
    );
    assert_eq!(
        report.omissions()[1].omission.finding.message,
        "second source finding"
    );
}

fn one_triangle(material: ExportMaterial) -> ExportMesh {
    ExportMesh::new(
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        vec![[0.0, 0.0, 1.0]; 3],
        vec![[0, 1, 2]],
        vec![0],
        vec![material],
    )
    .expect("one triangle")
}

fn scene_with_colour(
    material: [f64; 3],
    override_colour: Option<[f64; 3]>,
) -> ferritecad_export::ExportScene {
    let mut builder = ExportSceneBuilder::new();
    let material = ExportMaterial::new("colour", material, ExportColourOrigin::Source)
        .expect("the neutral scene can carry an HDR linear value");
    let definition = builder
        .definition(
            ExportSource::Body {
                object: ObjectId::new(),
            },
            None,
            ExportProvenance::default(),
            ExportGeometry::Mesh(one_triangle(material)),
        )
        .expect("one body");
    builder
        .node(
            None,
            definition,
            ExportTransform::IDENTITY,
            None,
            override_colour,
            ExportOccurrence::Unrecorded,
        )
        .expect("one placement");
    builder.finish().expect("one coloured triangle")
}

#[test]
fn a_colour_outside_the_measured_range_is_refused_before_any_byte_is_written() {
    for scene in [
        scene_with_colour([2.0, 0.0, 0.0], None),
        scene_with_colour([0.5, 0.5, 0.5], Some([0.0, 1.5, 0.0])),
    ] {
        let mut bytes = Vec::new();
        let error = write_fbx_ascii_7400(&scene, &mut bytes)
            .expect_err("an unmeasured HDR colour must not be clamped");
        assert_eq!(error.kind(), ErrorKind::Unsupported);
        assert!(bytes.is_empty(), "the refusal left a partial FBX behind");
    }
}

#[test]
fn the_same_scene_always_produces_the_same_bytes() {
    let scene = measured_scene();
    let (first, first_report) = written(&scene);
    let (second, second_report) = written(&scene);
    assert_eq!(first, second, "two writes of one scene differ");
    assert_eq!(first_report, second_report);
    assert_eq!(first_report.bytes(), first.len() as u64);

    // And a scene built again from the same values is the same file.
    let (again, _) = written(&measured_scene());
    assert_eq!(first, again);

    // No signed zero survives to distinguish two equal values.
    let text = std::str::from_utf8(&first).expect("UTF-8");
    assert!(!text.contains("-0.0"), "a negative zero reached the file");
    assert!(!text.contains("-0,"), "a negative zero reached an array");
}

#[test]
fn a_placement_identity_changes_nothing_the_writer_writes() {
    // §22B-1e3a puts a durable identity on every placement and stops there.
    // What Unity does with an identity is settled by what a *name* is, which
    // the §22B-1e1 and §22B-1e2a measurements established and which this slice
    // deliberately does not act on. So the writer must not have started reading
    // it: the same scene with and without identities is the same file, byte for
    // byte, and the report beside it says the same thing.
    let (without, without_report) = written(&measured_scene());
    let (with, with_report) = written(&measured_scene_with_identities());
    assert_eq!(
        with, without,
        "a placement identity reached the FBX bytes, which this slice does not do"
    );
    // The report says the same thing too. Compared field by field rather than
    // as a whole because each scene mints its own `ImportedSourceId`, which is
    // what the report names an omission by and has nothing to do with this.
    assert_eq!(with_report.bytes(), without_report.bytes());
    assert_eq!(with_report.models(), without_report.models());
    assert_eq!(with_report.geometries(), without_report.geometries());
    assert_eq!(with_report.materials(), without_report.materials());
    assert_eq!(
        with_report.omissions().len(),
        without_report.omissions().len()
    );
    for (left, right) in with_report
        .omissions()
        .iter()
        .zip(without_report.omissions())
    {
        assert_eq!(left.definition, right.definition);
        assert_eq!(left.nodes, right.nodes);
        assert_eq!(left.omission, right.omission);
    }

    // And the identities really were there, so the comparison above is between
    // two different scenes rather than two spellings of one.
    let identified = measured_scene_with_identities();
    assert!(
        identified
            .nodes()
            .iter()
            .all(|node| node.occurrence.is_recorded()),
        "the identified scene carries no identities, so this gate measures nothing"
    );
    assert!(
        measured_scene()
            .nodes()
            .iter()
            .all(|node| !node.occurrence.is_recorded()),
        "the control scene already carries identities"
    );
    // Two builds of the identified scene mint different identities, and still
    // produce the same file: the writer is a function of everything except
    // this.
    let again = measured_scene_with_identities();
    assert_ne!(
        identified
            .nodes()
            .iter()
            .map(|node| node.occurrence)
            .collect::<Vec<_>>(),
        again
            .nodes()
            .iter()
            .map(|node| node.occurrence)
            .collect::<Vec<_>>(),
    );
    assert_eq!(written(&again).0, with);
}

#[test]
fn the_local_transform_is_converted_and_never_accumulated() {
    let (bytes, _) = written(&measured_scene());
    let file = parsed(&bytes);
    let models = file.all("Objects/Model");

    // The measured assembly translation, converted once: mm to metres and
    // (x, y, z) -> (x, z, -y).
    assert_eq!(
        models[1].at_property("Lcl Translation").numbers(),
        vec![0.1, 0.3, -0.2]
    );
    // A control point 1000 mm along FerriteCAD X is one metre along FBX X,
    // and its parent's transform is not multiplied into it.
    assert_eq!(
        models[6].at_property("Lcl Translation").numbers(),
        vec![1.0, 0.0, 0.0]
    );
    assert_eq!(
        models[7].at_property("Lcl Translation").numbers(),
        vec![0.0, 0.0, -2.0]
    );
    assert_eq!(
        models[8].at_property("Lcl Translation").numbers(),
        vec![0.0, 3.0, 0.0]
    );
    for model in &models {
        assert_eq!(
            model.at_property("Lcl Scaling").numbers(),
            vec![1.0, 1.0, 1.0],
            "the conversion hid itself in a hierarchy scale"
        );
    }
    assert_eq!(
        models[0].at_property("Lcl Rotation").numbers(),
        vec![0.0, 0.0, 0.0],
        "an identity placement rotates by nothing"
    );
    assert_ne!(
        models[1].at_property("Lcl Rotation").numbers(),
        vec![0.0, 0.0, 0.0]
    );
}

#[test]
fn a_name_the_format_cannot_spell_is_refused_rather_than_quietly_changed() {
    let scene = escaping_scene();
    let (bytes, _) = written(&scene);
    let text = std::str::from_utf8(&bytes).expect("the writer emits UTF-8");

    // The FBX ASCII entities, which are what a reader turns back into the
    // original characters. A backslash is not special and stays itself.
    assert!(text.contains("a &quot;quoted&quot; name"));
    assert!(text.contains(r"back\slash"));
    assert!(text.contains("Кириллица"));
    assert!(text.contains("&lf;"));
    assert!(text.contains("&cr;"));
    assert!(
        !text.contains("\\\""),
        "backslash escaping is not this format"
    );

    let file = parsed(&bytes);
    let names: Vec<String> = file
        .all("Objects/Model")
        .iter()
        .map(|model| model.object_name())
        .collect();
    assert!(
        names.iter().any(|name| name.is_empty()),
        "an empty name is a name"
    );
}
