// SPDX-License-Identifier: MIT
//! A stored document, and what its names are supposed to resolve to.
//!
//! The tests elsewhere in this workspace build their documents in memory, so
//! they prove today's writer agrees with today's reader and nothing more. This
//! crate holds a document that was written once and committed: a real `.fcad`
//! file, checked in, with no cache sidecar and no B-Rep anywhere in it. What a
//! rebuild makes of it is the compatibility gate.
//!
//! # The manifest names nothing that changes
//!
//! [`render_manifest`] writes whether each stored reference resolved and with
//! what cardinality — the role, the selection rule, how many faces — and never
//! a handle, a face index or a session id. Those differ on every run by design,
//! so recording them would turn the gate into noise and, worse, would make the
//! file look like an authority on which face is which.
//!
//! # Current limit: a bijection is not identity
//!
//! Cardinality plus the final distinct-face count catches lost, ambiguous and
//! collapsed names. It cannot catch a one-to-one permutation of existing
//! faces: swapping start and end caps would render the same text. The gate must
//! not claim otherwise. Closing that gap needs a kernel-neutral geometric
//! fingerprint for each resolved face; the planned face-associated
//! tessellation can supply one without persisting session-local topology.
//!
//! # Nothing opens the committed file in place
//!
//! [`Document::open`][ferritecad_document::Document::open] migrates and sets
//! pragmas, which would rewrite the fixture. Every entry point here copies it
//! into a directory the caller owns, so a test run leaves the checkout as it
//! found it.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ferritecad_document::{
    CapSide, Document, EndCondition, Expression, ObjectPayload, Point2, SelectionRule,
    SemanticRole, Sketch, SketchCurve, SketchGeometry,
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
/// A copy rather than the original: opening a document migrates it and sets
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
/// the same text or a readable difference. This deliberately does not identify
/// the geometry of a resolved face; see the crate-level limitation.
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
    let references = document.topology_refs()?;
    writeln!(out, "objects {}", objects.len()).expect("writing to a String cannot fail");
    writeln!(out, "references {}\n", references.len()).expect("writing to a String cannot fail");

    let mut faces = BTreeSet::new();
    let mut blocks = Vec::new();
    for reference in &references {
        let mut block = format!("{}\n", describe_role(&reference.output_role));
        writeln!(block, "    {}", describe_selection(&reference.selection))
            .expect("writing to a String cannot fail");

        match built.resolve(reference) {
            Ok(found) => {
                faces.extend(found.iter().copied());
                writeln!(
                    block,
                    "    {} face{}",
                    found.len(),
                    if found.len() == 1 { "" } else { "s" }
                )
                .expect("writing to a String cannot fail");
                for face in found {
                    match measure(mesh, face) {
                        Some((area, centroid)) => writeln!(
                            block,
                            "    area {:.3} mm^2   middle ({:.3}, {:.3}, {:.3})",
                            area, centroid[0], centroid[1], centroid[2]
                        ),
                        None => writeln!(block, "    not drawn"),
                    }
                    .expect("writing to a String cannot fail");
                }
            }
            Err(error) => writeln!(
                block,
                "    unresolved: {}",
                first_sentence(&error.to_string())
            )
            .expect("writing to a String cannot fail"),
        }
        blocks.push(block);
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
            }),
        )?;
        Ok(())
    })?;
    Ok(removed.id)
}

/// Builds the plate from nothing. Used only to regenerate the committed file.
pub fn write_plate(path: &Path) -> Result<()> {
    use ferritecad_document::{
        Body, DatumPlane, Dependency, DependencyRole, EntityKind, Extrude, SolidOperation,
        TopologyRef,
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
            &ObjectPayload::DatumPlane(DatumPlane {
                placement: Transform::IDENTITY,
            }),
        )?;
        w.put_object(
            sketch,
            None,
            1,
            Some("Profile"),
            &ObjectPayload::Sketch(Sketch { plane, curves }),
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
                profile: sketch,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(10.0)?,
                },
                reversed: false,
                operation: SolidOperation::NewBody,
                target_body: None,
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

        for side in [CapSide::Start, CapSide::End] {
            w.put_topology_ref(&TopologyRef {
                id: StableEntityId::new(),
                owner: extrude,
                producer_feature: extrude,
                expected_kind: EntityKind::Face,
                output_role: SemanticRole::ExtrudeCap { side },
                selection: SelectionRule::Exact,
                fallback_signature: None,
            })?;
        }
        for segment in &segments {
            w.put_topology_ref(&TopologyRef {
                id: StableEntityId::new(),
                owner: extrude,
                producer_feature: extrude,
                expected_kind: EntityKind::Face,
                output_role: SemanticRole::ExtrudeSide {
                    profile_segment: *segment,
                },
                selection: SelectionRule::AllDerivedFrom { ancestor: *segment },
                fallback_signature: None,
            })?;
        }
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

fn describe_role(role: &SemanticRole) -> String {
    match role {
        SemanticRole::ExtrudeCap { side } => format!("cap {}", describe_cap(*side)),
        SemanticRole::ExtrudeSide { profile_segment } => format!("side {profile_segment}"),
        SemanticRole::SketchSegment { segment } => format!("sketch segment {segment}"),
        SemanticRole::FilletFace { source_edge } => format!("fillet face from {source_edge}"),
        other => format!("{other:?}"),
    }
}

fn describe_cap(side: CapSide) -> &'static str {
    match side {
        CapSide::Start => "start",
        CapSide::End => "end",
        // A side this build does not know is written as such rather than
        // folded into one of the two it does.
        _ => "unknown",
    }
}

fn describe_selection(selection: &SelectionRule) -> String {
    match selection {
        SelectionRule::Exact => "exact".to_owned(),
        SelectionRule::AllDerivedFrom { ancestor } => format!("all derived from {ancestor}"),
        other => format!("{other:?}"),
    }
}

/// Keeps a failure's first clause, so the manifest records why without
/// recording a message that may be reworded.
fn first_sentence(message: &str) -> String {
    message
        .split([';', ':'])
        .next()
        .unwrap_or(message)
        .trim()
        .to_owned()
}
