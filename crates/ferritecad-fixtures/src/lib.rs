// SPDX-License-Identifier: MIT
//! A stored document, and what its names are supposed to resolve to.
//!
//! The tests elsewhere in this workspace build their documents in memory, so
//! they prove today's writer agrees with today's reader and nothing more. This
//! crate holds a document that was written once and committed: a real `.fcad`
//! file, checked in, with no cache sidecar and no B-Rep anywhere in it. What a
//! rebuild makes of it checks geometry and naming.
//!
//! # The manifest names nothing that changes
//!
//! [`render_manifest`] writes each derived face name, its full derivation and
//! its cardinality, and never
//! a handle, a face index or a session id. Those differ on every run by design,
//! so recording them would turn the gate into noise and, worse, would make the
//! file look like an authority on which face is which.
//!
//! # A bijection is not identity
//!
//! Cardinality plus the final distinct-face count catches lost, ambiguous and
//! collapsed names, but cannot catch a one-to-one permutation of existing
//! faces. [`render_manifest`] therefore also measures each resolved face from
//! the triangles filed under that same handle. Area and centroid distinguish
//! every face in the committed plate without persisting session-local topology;
//! swapping caps or rotating side-face history changes the golden text.
//!
//! # Nothing opens the committed file in place
//!
//! Every entry point copies the fixture
//! into a directory the caller owns, so a test run leaves the checkout as it
//! found it.

mod edit_request;
pub use edit_request::edit_request_json;

pub mod datum_planes;
pub mod feature_changes;
pub mod up_to_end;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ferritecad_document::{
    Document, EndCondition, Expression, ObjectPayload, Point2, Sketch, SketchCurve, SketchGeometry,
};
use ferritecad_eval::RebuildResult;
use ferritecad_kernel::{
    GeometryKernel, Mesh, OperationContext, SubShapeHandle, TessellationParams,
};
use ferritecad_types::{CadError, Result, StableEntityId};

/// The committed plate: 60 x 40 x 10, one datum, one four-segment sketch, one
/// extrusion, one body, and the six references a model would store about it.
pub fn plate_source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("plate")
        .join("plate.fcad")
}

/// The manifest the plate is expected to produce.
pub fn plate_manifest_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("plate")
        .join("manifest.txt")
}

/// What the committed manifest says, with line endings normalised.
pub fn plate_manifest() -> Result<String> {
    std::fs::read_to_string(plate_manifest_path())
        .map(|text| text.replace("\r\n", "\n"))
        .map_err(|e| CadError::io("reading the plate manifest", e))
}

/// Opens a private copy of the plate inside `directory`.
///
/// A copy rather than the original: opening a document for writing sets
/// persistent pragmas, and a test that dirtied the checkout would be found out
/// by `git diff` long after the run that did it.
pub fn open_plate(directory: &Path) -> Result<Document> {
    let target = directory.join("plate.fcad");
    std::fs::copy(plate_source(), &target)
        .map_err(|e| CadError::io("copying the plate fixture", e))?;
    Document::open(target)
}

/// Writes the resolution cardinality of every reference the document stores.
///
/// Deterministic and free of anything session-local, so two runs of the same
/// build — and a run on another machine, or against another kernel — produce
/// the same text or a readable difference. Area and centroid come from the
/// triangles assigned to each resolved face, independently anchoring the name
/// to geometry rather than to a kernel's traversal index.
pub fn render_manifest(
    document: &Document,
    built: &RebuildResult,
    kernel: &mut dyn GeometryKernel,
) -> Result<String> {
    let mesh = draw(document, built, kernel)?;
    let mesh = &mesh;
    let mut out = String::new();
    out.push_str(
        "# What the committed plate resolves to.\n\
         #\n\
         # Regenerate with:\n\
         #   cargo test -p ferritecad-fixtures -- --ignored regenerate\n\
         #\n\
         # No handles, face indices or session ids appear here on purpose: they\n\
         # differ every run, and a fixture that recorded them would be asserting\n\
         # something it has no business knowing.\n\
         #\n\
         # Each face is measured from its own triangles instead. Counts alone\n\
         # could not tell a build that exchanged two names from a correct one,\n\
         # because six names still reached six faces; an area and a middle are\n\
         # properties of the geometry, so exchanging two names moves two of\n\
         # these numbers.\n\n",
    );

    let objects = document.objects()?;
    writeln!(out, "objects {}", objects.len()).expect("writing to a String cannot fail");
    let mut faces = BTreeSet::new();
    let mut blocks = Vec::new();
    for (feature, names) in &built.topology().features {
        for (name, found) in &names.bindings {
            if name.kind != ferritecad_kernel::SubShapeKind::Face {
                continue;
            }
            let mut block = format!("{}\n", built.topology().describe(*feature, *name)?);
            writeln!(block, "    {} face(s)", found.len())
                .expect("writing to a String cannot fail");
            for face in found {
                faces.insert(*face);
                match measure(mesh, *face) {
                    Some((area, centroid)) => writeln!(
                        block,
                        "    area {:.3} mm^2   middle ({:.3}, {:.3}, {:.3})",
                        area, centroid[0], centroid[1], centroid[2]
                    ),
                    None => writeln!(block, "    not drawn"),
                }
                .expect("writing to a String cannot fail");
            }
            blocks.push(block);
        }
    }
    blocks.sort();
    for block in blocks {
        out.push_str(&block);
    }

    writeln!(out, "\ndistinct faces {}", faces.len()).expect("writing to a String cannot fail");
    Ok(out)
}

/// Changes the plate's extrusion distance, leaving every name alone.
pub fn set_height(document: &mut Document, height: f64) -> Result<()> {
    let record = document
        .objects()?
        .into_iter()
        .find(|object| matches!(object.payload, ObjectPayload::Extrude(_)))
        .ok_or_else(|| CadError::input("the fixture has no extrusion to resize"))?;
    let ObjectPayload::Extrude(mut feature) = record.payload else {
        return Err(CadError::input("the extrusion is not an extrusion"));
    };

    feature.end_condition = EndCondition::Blind {
        distance: Expression::constant(height)?,
    };
    document.write(|w| {
        w.put_object(
            record.id,
            record.parent,
            record.ordinal,
            record.name.as_deref(),
            &ObjectPayload::Extrude(feature),
        )?;
        Ok(())
    })
}

/// Removes one sketch segment, closing the loop over the gap it leaves.
///
/// A segment cannot simply be deleted from a closed profile — the result would
/// not be a profile at all, and the rebuild would fail for a reason that has
/// nothing to do with naming. Its predecessor is extended to the corner it fed
/// instead, which is what a user editing a sketch would end up with.
///
/// Returns the segment that is gone.
pub fn drop_segment(document: &mut Document) -> Result<StableEntityId> {
    let record = document
        .objects()?
        .into_iter()
        .find(|object| matches!(object.payload, ObjectPayload::Sketch(_)))
        .ok_or_else(|| CadError::input("the fixture has no sketch to edit"))?;
    let ObjectPayload::Sketch(mut sketch) = record.payload else {
        return Err(CadError::input("the sketch is not a sketch"));
    };

    if sketch.curves.len() < 4 {
        return Err(CadError::input(
            "dropping a segment needs a profile that is still closed afterwards",
        ));
    }

    let removed = sketch
        .curves
        .pop()
        .ok_or_else(|| CadError::input("the sketch reported curves and then produced none"))?;
    let SketchGeometry::Line { end, .. } = removed.geometry else {
        return Err(CadError::input("this fixture's last segment is not a line"));
    };

    let previous = sketch
        .curves
        .last_mut()
        .ok_or_else(|| CadError::input("nothing is left to close the loop"))?;
    let SketchGeometry::Line { start, .. } = previous.geometry else {
        return Err(CadError::input("this fixture's segments are not all lines"));
    };
    previous.geometry = SketchGeometry::Line { start, end };

    let curves = sketch.curves.clone();
    document.write(|w| {
        w.put_object(
            record.id,
            record.parent,
            record.ordinal,
            record.name.as_deref(),
            &ObjectPayload::Sketch(Sketch {
                plane: sketch.plane,
                curves,
                constraints: Vec::new(),
            }),
        )?;
        Ok(())
    })?;
    Ok(removed.id)
}

/// Builds the plate from nothing. Used only to regenerate the committed file.
pub fn write_plate(path: &Path) -> Result<()> {
    use ferritecad_document::{
        Body, DatumPlane, Dependency, DependencyRole, Extrude, SolidOperation,
    };
    use ferritecad_types::{ObjectId, Transform};

    let plane = ObjectId::new();
    let sketch = ObjectId::new();
    let extrude = ObjectId::new();
    let body = ObjectId::new();
    let segments: Vec<StableEntityId> = (0..4).map(|_| StableEntityId::new()).collect();

    let corners = [(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)];
    let mut curves = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let (sx, sy) = corners[index];
        let (ex, ey) = corners[(index + 1) % corners.len()];
        curves.push(SketchCurve {
            id: *segment,
            construction: false,
            geometry: SketchGeometry::Line {
                start: Point2::new(sx, sy)?,
                end: Point2::new(ex, ey)?,
            },
        });
    }

    let mut document = Document::create(path)?;
    document.write(|w| {
        w.put_object(
            plane,
            None,
            0,
            Some("XY"),
            &ObjectPayload::DatumPlane(DatumPlane::Fixed {
                placement: Transform::IDENTITY,
            }),
        )?;
        w.put_object(
            sketch,
            None,
            1,
            Some("Profile"),
            &ObjectPayload::Sketch(Sketch {
                plane,
                curves,
                constraints: Vec::new(),
            }),
        )?;
        w.add_dependency(Dependency {
            dependent: sketch,
            dependency: plane,
            role: DependencyRole::Plane,
        })?;
        w.put_object(
            extrude,
            None,
            2,
            Some("Extrude1"),
            &ObjectPayload::Extrude(Extrude {
                regions: Vec::new(),
                profile: sketch,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(10.0)?,
                },
                reversed: false,
                operation: SolidOperation::NewBody,
                previous: None,
            }),
        )?;
        w.add_dependency(Dependency {
            dependent: extrude,
            dependency: sketch,
            role: DependencyRole::Profile,
        })?;
        w.put_object(
            body,
            None,
            3,
            Some("Plate"),
            &ObjectPayload::Body(Body {
                tip_feature: Some(extrude),
            }),
        )?;
        w.add_dependency(Dependency {
            dependent: body,
            dependency: extrude,
            role: DependencyRole::BodyTip,
        })?;

        Ok(())
    })?;
    document.close()
}

/// Draws every solid this rebuild produced, into one mesh.
///
/// Part of rendering the manifest rather than something a caller supplies, so
/// the two gates cannot measure faces with different settings and then compare
/// the results.
fn draw(
    document: &Document,
    built: &RebuildResult,
    kernel: &mut dyn GeometryKernel,
) -> Result<Mesh> {
    let mut combined = Mesh::default();
    for object in document.objects()? {
        if !matches!(object.payload, ObjectPayload::Extrude(_)) {
            continue;
        }
        let Some(shape) = built.shape(object.id) else {
            continue;
        };

        let mesh = kernel.tessellate(
            shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )?;
        let vertex_offset = u32::try_from(combined.positions.len() / 3)
            .map_err(|_| CadError::kernel("this rebuild drew more vertices than can be indexed"))?;
        let index_offset = u32::try_from(combined.indices.len()).map_err(|_| {
            CadError::kernel("this rebuild drew more triangles than can be indexed")
        })?;

        combined.positions.extend_from_slice(&mesh.positions);
        combined.normals.extend_from_slice(&mesh.normals);
        combined
            .indices
            .extend(mesh.indices.iter().map(|index| index + vertex_offset));
        combined
            .faces
            .extend(mesh.faces.into_iter().map(|mut range| {
                range.first_index += index_offset;
                range
            }));
    }
    Ok(combined)
}

/// One face's triangle area, and the area-weighted middle of those triangles.
///
/// Measured from the mesh rather than asked of the kernel: the point is to
/// check that the triangles filed under a name are the ones that name means,
/// and a kernel's own answer about a face it also chose would not test that.
fn measure(mesh: &Mesh, face: SubShapeHandle) -> Option<(f64, [f64; 3])> {
    let range = mesh.faces.iter().find(|range| range.face == face)?;
    let point = |index: u32| -> [f64; 3] {
        let at = index as usize * 3;
        [
            f64::from(mesh.positions[at]),
            f64::from(mesh.positions[at + 1]),
            f64::from(mesh.positions[at + 2]),
        ]
    };

    let mut area = 0.0;
    let mut middle = [0.0f64; 3];
    let first = range.first_index as usize;
    for triangle in mesh.indices[first..first + range.index_count as usize].chunks_exact(3) {
        let (a, b, c) = (point(triangle[0]), point(triangle[1]), point(triangle[2]));
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let piece = (cross[0].powi(2) + cross[1].powi(2) + cross[2].powi(2)).sqrt() / 2.0;
        area += piece;
        for axis in 0..3 {
            middle[axis] += (a[axis] + b[axis] + c[axis]) / 3.0 * piece;
        }
    }

    if area <= 0.0 {
        return None;
    }
    for value in &mut middle {
        *value /= area;
    }
    Some((area, middle))
}

/// Check each native dependency separately before a test may skip it.
#[track_caller]
pub fn native_dependencies(dependencies: &[(bool, &str)]) -> bool {
    let mut available = true;
    for &(present, dependency) in dependencies {
        if !present {
            let requirement = format!("FERRITECAD_REQUIRE_{dependency}");
            assert_ne!(
                std::env::var(&requirement).as_deref(),
                Ok("1"),
                "{requirement}=1 but {dependency} is unavailable"
            );
            eprintln!("skipped: {dependency} is unavailable");
            available = false;
        }
    }
    available
}

/// A repairable plate history, stored directly so evaluation can reject a radius.
#[derive(Debug, Clone, Copy)]
pub struct FilletChain {
    pub sketch: ferritecad_types::ObjectId,
    pub extrude: ferritecad_types::ObjectId,
    pub first: ferritecad_types::ObjectId,
    pub second: ferritecad_types::ObjectId,
    pub body: ferritecad_types::ObjectId,
}
pub fn write_fillet_chain(path: &Path, radius: f64) -> Result<FilletChain> {
    use ferritecad_document::{Body, Dependency, DependencyRole, Fillet};
    use ferritecad_types::ObjectId;
    write_plate(path)?;
    let mut document = Document::open(path)?;
    let objects = document.objects()?;
    let sketch = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .expect("plate sketch");
    let extrude = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
        .expect("plate extrusion")
        .id;
    let body = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .expect("plate body")
        .id;
    let ObjectPayload::Sketch(profile) = &sketch.payload else {
        unreachable!()
    };
    let segments: Vec<_> = profile.curves.iter().map(|c| c.id).collect();
    let first = ObjectId::new();
    let second = ObjectId::new();
    document.write(|w| {
        for (id, previous, edge, r, name) in [
            (
                first,
                extrude,
                feature_changes::extruded_fixture_edge(
                    extrude,
                    profile.plane,
                    segments[3],
                    segments[0],
                ),
                radius,
                "Fillet1",
            ),
            (
                second,
                first,
                feature_changes::extruded_fixture_edge(
                    extrude,
                    profile.plane,
                    segments[1],
                    segments[2],
                ),
                3.0,
                "Fillet2",
            ),
        ] {
            w.put_object(
                id,
                None,
                4,
                Some(name),
                &ObjectPayload::Fillet(Fillet {
                    previous,
                    edges: vec![edge],
                    radius_mm: ferritecad_document::Expression::constant(r)?,
                }),
            )?;
            w.add_dependency(Dependency {
                dependent: id,
                dependency: previous,
                role: DependencyRole::Predecessor,
            })?;
        }
        w.put_object(
            body,
            None,
            3,
            Some("Plate"),
            &ObjectPayload::Body(Body {
                tip_feature: Some(second),
            }),
        )?;
        w.remove_dependency(Dependency {
            dependent: body,
            dependency: extrude,
            role: DependencyRole::BodyTip,
        })?;
        w.add_dependency(Dependency {
            dependent: body,
            dependency: second,
            role: DependencyRole::BodyTip,
        })
    })?;
    document.close()?;
    Ok(FilletChain {
        sketch: sketch.id,
        extrude,
        first,
        second,
        body,
    })
}

/// Stored profile coordinates remain an initial guess; dimensions drive the part.
pub fn write_constrained_fillet_chain(
    path: &Path,
    radius: f64,
    width: f64,
    depth: f64,
) -> Result<FilletChain> {
    use ferritecad_document::{
        SketchConstraint, SketchConstraintRule, SketchPointRef, SketchPointSelector,
    };
    let chain = write_fillet_chain(path, radius)?;
    let mut document = Document::open(path)?;
    let object = document
        .objects()?
        .into_iter()
        .find(|o| o.id == chain.sketch)
        .expect("fixture sketch");
    let ObjectPayload::Sketch(mut sketch) = object.payload else {
        unreachable!()
    };
    let edges: Vec<_> = sketch.curves.iter().map(|c| c.id).collect();
    let start = |i| SketchPointRef::new(edges[i], SketchPointSelector::Start);
    let end = |i| SketchPointRef::new(edges[i], SketchPointSelector::End);
    let mut rules = Vec::new();
    for i in 0..4 {
        rules.push(SketchConstraintRule::Coincident {
            a: end(i),
            b: start((i + 1) % 4),
        });
    }
    rules.extend([
        SketchConstraintRule::Fixed {
            point: start(0),
            x: ferritecad_document::Expression::constant(0.0)?,
            y: ferritecad_document::Expression::constant(0.0)?,
        },
        SketchConstraintRule::Horizontal {
            a: start(0),
            b: end(0),
        },
        SketchConstraintRule::Vertical {
            a: start(1),
            b: end(1),
        },
        SketchConstraintRule::Horizontal {
            a: start(2),
            b: end(2),
        },
        SketchConstraintRule::Vertical {
            a: start(3),
            b: end(3),
        },
        SketchConstraintRule::Distance {
            a: start(0),
            b: end(0),
            distance: ferritecad_document::Expression::constant(width)?,
        },
        SketchConstraintRule::Distance {
            a: start(1),
            b: end(1),
            distance: ferritecad_document::Expression::constant(depth)?,
        },
    ]);
    sketch.constraints = rules
        .into_iter()
        .map(|rule| SketchConstraint {
            id: StableEntityId::new(),
            rule,
        })
        .collect();
    document.write(|w| {
        w.put_object(
            object.id,
            object.parent,
            object.ordinal,
            object.name.as_deref(),
            &ObjectPayload::Sketch(sketch),
        )
    })?;
    document.close()?;
    Ok(chain)
}

/// Find the first slot of a known fixture without depending on catalogue row order.
pub fn expression_slot(
    source: &ferritecad_document::ExtrudeEditSource,
    slot: ferritecad_document::ExpressionSlot,
) -> &ferritecad_document::ExpressionSlotValue {
    source
        .expression_slots
        .iter()
        .find(|value| value.slot == slot)
        .expect("fixture expression slot")
}

pub fn constraint_changes(
    document: &ferritecad_document::Document,
    id: ferritecad_types::ObjectId,
    edits: &ferritecad_document::SketchConstraintEdits,
) -> ferritecad_types::Result<Vec<ferritecad_document::EditChange>> {
    let object = document
        .object(id)?
        .ok_or_else(|| ferritecad_types::CadError::input("fixture sketch is missing"))?;
    let ferritecad_document::ObjectPayload::Sketch(sketch) = object.payload else {
        return Err(ferritecad_types::CadError::input(
            "fixture object is not a sketch",
        ));
    };
    ferritecad_document::sketch_constraint_changes(id, &sketch, edits)
}
pub fn added_constraints(
    changes: &[ferritecad_document::EditChange],
) -> Vec<&ferritecad_document::SketchConstraint> {
    changes
        .iter()
        .filter_map(|c| match c {
            ferritecad_document::EditChange::AddConstraint { constraint, .. } => Some(constraint),
            _ => None,
        })
        .collect()
}

/// Read the single sketch of a fixture without an editor catalogue.
pub fn sketch(
    document: &ferritecad_document::Document,
) -> ferritecad_types::Result<(ferritecad_types::ObjectId, Sketch)> {
    document
        .objects()?
        .into_iter()
        .find_map(|o| match o.payload {
            ObjectPayload::Sketch(sketch) => Some((o.id, sketch)),
            _ => None,
        })
        .ok_or_else(|| ferritecad_types::CadError::input("fixture has no sketch"))
}
pub fn vertex_changes(
    document: &ferritecad_document::Document,
    id: ferritecad_types::ObjectId,
    vertices: &[ferritecad_document::SketchVertex],
) -> ferritecad_types::Result<Vec<ferritecad_document::EditChange>> {
    let object = document
        .object(id)?
        .ok_or_else(|| ferritecad_types::CadError::input("fixture sketch is missing"))?;
    let ObjectPayload::Sketch(sketch) = object.payload else {
        return Err(ferritecad_types::CadError::input(
            "fixture object is not a sketch",
        ));
    };
    ferritecad_document::move_sketch_vertices(id, &sketch, vertices)
}

/// Unordered labels for fixture incidence assertions, independent of entity references.
pub fn ordered_labels(one: StableEntityId, other: StableEntityId) -> [StableEntityId; 2] {
    let mut labels = [one, other];
    labels.sort_unstable();
    labels
}

pub mod result_sets;
