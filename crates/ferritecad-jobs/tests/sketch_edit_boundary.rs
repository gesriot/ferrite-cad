// SPDX-License-Identifier: MIT
use ferritecad_document::{Document, Expression, ObjectPayload, SketchGeometry};
use ferritecad_eval::{RebuildStatus, rebuild_cold};
use ferritecad_kernel::OperationContext;

#[test]
fn fillet_rebuild_distinguishes_vertex_distortion_from_rectangle_resize() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let root = tempfile::tempdir().expect("directory");
    let source = root.path().join("source.fcad");
    let chain = ferritecad_fixtures::write_fillet_chain(&source, 8.0).expect("fixture");
    let before = std::fs::read(&source).expect("source bytes");
    let context = OperationContext::default();
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let original = Document::open_read_only(&source).expect("source");
    let baseline = rebuild_cold(&original, &mut kernel, &context).expect("baseline");
    assert!(baseline.all_built(), "{:?}", baseline.report());
    baseline.release_all(&mut kernel);

    for (both_vertices, radius, refusal) in [
        (false, 8.0, Some("not an axis-aligned rectangle")),
        (false, 2.0, Some("not an axis-aligned rectangle")),
        (true, 8.0, Some("too large")),
        (true, 2.0, None),
    ] {
        let path = root
            .path()
            .join(format!("candidate-{both_vertices}-{radius}.fcad"));
        std::fs::copy(&source, &path).expect("candidate copy");
        let mut document = Document::open(&path).expect("candidate");
        let mut profile = document
            .object(chain.sketch)
            .expect("read")
            .expect("sketch");
        let ObjectPayload::Sketch(sketch) = &mut profile.payload else {
            unreachable!("fixture sketch")
        };
        for curve in &mut sketch.curves {
            let SketchGeometry::Line { start, end } = &mut curve.geometry else {
                unreachable!("fixture rectangle")
            };
            for point in [start, end] {
                if point.x == 60.0 && (both_vertices || point.y == 0.0) {
                    point.x = 10.0;
                }
            }
        }
        let mut rounded = document.object(chain.first).expect("read").expect("fillet");
        let ObjectPayload::Fillet(fillet) = &mut rounded.payload else {
            unreachable!("fixture fillet")
        };
        fillet.radius_mm = Expression::constant(radius).expect("radius");
        // Isolate the rebuild decision from every edit preparation check.
        document
            .write(|writer| {
                for object in [&profile, &rounded] {
                    writer.put_object(
                        object.id,
                        object.parent,
                        object.ordinal,
                        object.name.as_deref(),
                        &object.payload,
                    )?;
                }
                Ok(())
            })
            .expect("valid payloads in one transaction");
        document.close().expect("close");
        let reopened = Document::open_read_only(&path).expect("reopen");
        let built = rebuild_cold(&reopened, &mut kernel, &context).expect("cold rebuild");
        assert!(matches!(
            built.status(chain.sketch),
            Some(RebuildStatus::Built)
        ));
        assert!(matches!(
            built.status(chain.extrude),
            Some(RebuildStatus::Built)
        ));
        if let Some(reason) = refusal {
            let report = built.report();
            let failure = report
                .failed
                .iter()
                .find(|f| f.id == chain.first)
                .expect("Fillet failure");
            assert!(failure.message.contains(reason), "{}", failure.message);
            assert!(matches!(
                built.status(chain.second),
                Some(RebuildStatus::Skipped { cause }) if *cause == chain.first
            ));
            eprintln!(
                "both_vertices={both_vertices}, radius={radius}: {}",
                failure.message
            );
        } else {
            assert!(built.all_built(), "{:?}", built.report());
            for reference in original.topology_refs().expect("saved names") {
                assert!(
                    !built
                        .resolve(&reference)
                        .expect("same name resolves")
                        .is_empty()
                );
            }
        }
        built.release_all(&mut kernel);
    }
    assert_eq!(std::fs::read(&source).expect("unchanged source"), before);
}
