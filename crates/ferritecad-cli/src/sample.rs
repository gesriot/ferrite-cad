// SPDX-License-Identifier: MIT
//! The smallest model that exercises the whole document layer.
//!
//! A datum plane, a rectangular profile on it, and an extrusion producing a
//! body — plus the topology references naming that extrusion's two caps. It
//! exists so the format can be tested end to end before any geometry kernel is
//! wired up, and so `inspect` has something to show.

use ferritecad_document::{
    Body, CapSide, DatumPlane, Dependency, DependencyRole, Document, EndCondition, EntityKind,
    Expression, Extrude, ObjectPayload, Point2, SelectionRule, SemanticRole, Sketch, SketchCurve,
    SketchGeometry, SolidOperation, TopologyRef,
};
use ferritecad_types::{ObjectId, Result, StableEntityId, Transform};

/// Adds the sample part to an empty document.
pub fn populate(document: &mut Document, width: f64, depth: f64, height: f64) -> Result<()> {
    let plane = ObjectId::new();
    let sketch = ObjectId::new();
    let extrude = ObjectId::new();
    let body = ObjectId::new();

    let corners = [
        Point2::new(0.0, 0.0)?,
        Point2::new(width, 0.0)?,
        Point2::new(width, depth)?,
        Point2::new(0.0, depth)?,
    ];

    let mut curves = Vec::with_capacity(corners.len());
    for (index, start) in corners.iter().enumerate() {
        curves.push(SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Line {
                start: *start,
                end: corners[(index + 1) % corners.len()],
            },
        });
    }
    let first_segment = curves[0].id;

    document.write(|writer| {
        writer.put_object(
            plane,
            None,
            0,
            Some("XY"),
            &ObjectPayload::DatumPlane(DatumPlane {
                placement: Transform::IDENTITY,
            }),
        )?;
        writer.put_object(
            sketch,
            None,
            1,
            Some("Profile"),
            &ObjectPayload::Sketch(Sketch {
                plane,
                curves: curves.clone(),
            }),
        )?;
        writer.add_dependency(Dependency {
            dependent: sketch,
            dependency: plane,
            role: DependencyRole::Plane,
        })?;

        writer.put_object(
            body,
            None,
            3,
            Some("Plate"),
            &ObjectPayload::Body(Body {
                tip_feature: Some(extrude),
            }),
        )?;

        writer.put_object(
            extrude,
            None,
            2,
            Some("Extrude1"),
            &ObjectPayload::Extrude(Extrude {
                profile: sketch,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(height)?,
                },
                reversed: false,
                operation: SolidOperation::NewBody,
                target_body: None,
            }),
        )?;
        writer.add_dependency(Dependency {
            dependent: extrude,
            dependency: sketch,
            role: DependencyRole::Profile,
        })?;
        writer.add_dependency(Dependency {
            dependent: body,
            dependency: extrude,
            role: DependencyRole::BodyTip,
        })?;

        // Both caps are named up front. Nothing resolves them yet — that is
        // the kernel's job in a later stage — but the contract they express is
        // part of the model, not of the rebuild.
        for side in [CapSide::Start, CapSide::End] {
            writer.put_topology_ref(&TopologyRef {
                id: StableEntityId::new(),
                owner: extrude,
                producer_feature: extrude,
                expected_kind: EntityKind::Face,
                output_role: SemanticRole::ExtrudeCap { side },
                selection: SelectionRule::Exact,
                fallback_signature: None,
            })?;
        }

        // "Every face raised from this profile segment" stays correct when the
        // segment is split or the count changes; a face index would not.
        writer.put_topology_ref(&TopologyRef {
            id: StableEntityId::new(),
            owner: extrude,
            producer_feature: extrude,
            expected_kind: EntityKind::Face,
            output_role: SemanticRole::ExtrudeSide {
                profile_segment: first_segment,
            },
            selection: SelectionRule::AllDerivedFrom {
                ancestor: first_segment,
            },
            fallback_signature: None,
        })?;

        Ok(())
    })
}
