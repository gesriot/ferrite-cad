// SPDX-License-Identifier: MIT
//! The vertex where a profile corner reaches a cap, named durably.
//!
//! A plate has eight of them, four on each cap, and each belongs to the pair
//! of profile segments meeting at that corner rather than to a position in a
//! loop. This file asks whether that name survives everything a name has to
//! survive: an archive, a new session, a file on disk and a cache hit.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::PI;

use ferritecad_document::{
    Body, CacheStore, CapSide, DatumPlane, Dependency, DependencyRole, Document, EndCondition,
    EntityKind, Expression, Extrude, ObjectPayload, Point2, SelectionRule, SemanticRole, Sketch,
    SketchCurve, SketchGeometry, SolidOperation, TopologyRef,
};
use ferritecad_eval::{CacheOutcome, rebuild_cached, rebuild_cold};
use ferritecad_kernel::{
    ExtrudeExtent, ExtrudeRequest, GeometryKernel, OperationContext, PlanarPoint, Profile,
    ProfileLoop, ProfileSegment, SegmentGeometry, ShapeHandle, SketchPlane, SubShapeHandle,
    SubShapeKind, TessellationParams,
};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_topology::{TopologyMap, archive_feature, resolve, restore_feature};
use ferritecad_types::{ErrorKind, ObjectId, ProfileJoint, Result, StableEntityId, Transform};

macro_rules! kernel_or_skip {
    () => {
        if !is_available() {
            eprintln!("skipped: this build has no Open CASCADE");
            return;
        }
    };
}

fn plate(height: f64) -> Result<(ExtrudeRequest, Vec<StableEntityId>)> {
    let corners = [(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)];
    let points: Vec<PlanarPoint> = corners
        .iter()
        .map(|(x, y)| PlanarPoint::new(*x, *y))
        .collect::<Result<_>>()?;

    let mut segments = Vec::new();
    let mut labels = Vec::new();
    for (index, start) in points.iter().enumerate() {
        let label = StableEntityId::new();
        labels.push(label);
        segments.push(ProfileSegment::new(
            label,
            SegmentGeometry::line(*start, points[(index + 1) % points.len()])?,
        ));
    }

    Ok((
        ExtrudeRequest::new(
            Profile::new(
                SketchPlane::world_xy(),
                ProfileLoop::new(segments)?,
                Vec::new(),
            )?,
            ExtrudeExtent::blind(height)?,
            false,
        ),
        labels,
    ))
}

#[test]
fn a_vertex_survives_the_named_archive_as_a_vertex_of_the_restored_shape() {
    kernel_or_skip!();

    let (request, _) = plate(10.0).expect("a valid plate");

    // The eight corner vertices the sweep named, archived beside their solid.
    let (blob, slots) = {
        let mut writer = OcctKernel::new().expect("opens");
        let result = writer
            .extrude(&request, &OperationContext::default())
            .expect("Open CASCADE builds the plate");

        let vertices: Vec<SubShapeHandle> = result
            .start_cap_vertices
            .values()
            .chain(result.end_cap_vertices.values())
            .copied()
            .collect();
        assert_eq!(vertices.len(), 8, "four corners on each cap");

        let archived = writer
            .encode_shape_with(result.shape, &vertices)
            .expect("a vertex is archivable beside its shape");
        writer.release(result.shape);
        archived
    };

    // A session that never saw the original.
    let mut reader = OcctKernel::new().expect("opens");
    let (shape, restored) = reader
        .decode_shape_with(&blob, &slots)
        .expect("the archive restores");

    assert_eq!(restored.len(), 8);
    for sub in &restored {
        assert_eq!(
            sub.kind(),
            SubShapeKind::Vertex,
            "an archived vertex comes back as a vertex"
        );
        assert_eq!(sub.shape(), shape, "and as one of the restored shape");
    }

    reader.release(shape);
}

/// A document holding one plate, and the segments its profile was drawn from.
struct StoredPlate {
    extrude: ObjectId,
    segments: Vec<StableEntityId>,
}

fn store_plate(
    document: &mut Document,
    corners: &[(f64, f64)],
    height: f64,
) -> Result<StoredPlate> {
    let plane = ObjectId::new();
    let sketch = ObjectId::new();
    let plate = StoredPlate {
        extrude: ObjectId::new(),
        segments: (0..corners.len()).map(|_| StableEntityId::new()).collect(),
    };

    let mut curves = Vec::new();
    for (index, start) in corners.iter().enumerate() {
        let end = corners[(index + 1) % corners.len()];
        curves.push(SketchCurve {
            id: plate.segments[index],
            construction: false,
            geometry: SketchGeometry::Line {
                start: Point2::new(start.0, start.1)?,
                end: Point2::new(end.0, end.1)?,
            },
        });
    }

    let body = ObjectId::new();
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
            plate.extrude,
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
        w.add_dependency(Dependency {
            dependent: plate.extrude,
            dependency: sketch,
            role: DependencyRole::Profile,
        })?;
        w.put_object(
            body,
            None,
            3,
            Some("Plate"),
            &ObjectPayload::Body(Body {
                tip_feature: Some(plate.extrude),
            }),
        )?;
        w.add_dependency(Dependency {
            dependent: body,
            dependency: plate.extrude,
            role: DependencyRole::BodyTip,
        })?;
        Ok(())
    })?;

    Ok(plate)
}

fn cap_vertex_reference(feature: ObjectId, side: CapSide, joint: ProfileJoint) -> TopologyRef {
    TopologyRef {
        id: StableEntityId::new(),
        owner: feature,
        producer_feature: feature,
        expected_kind: EntityKind::Vertex,
        output_role: SemanticRole::ExtrudeCapVertex { side, joint },
        selection: SelectionRule::Exact,
        fallback_signature: None,
    }
}

#[test]
fn a_stored_corner_and_cap_resolve_to_one_vertex_of_a_cold_rebuild() {
    kernel_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("plate.fcad");
    let mut document = Document::create(&path).expect("creates");
    let plate = store_plate(
        &mut document,
        &[(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)],
        10.0,
    )
    .expect("populates");

    // The corner where the first two segments of the profile meet, on the
    // start cap. Written into the document, so what is resolved below is a
    // stored reference and not one held in memory since it was made.
    let joint = ProfileJoint::new(plate.segments[0], plate.segments[1]).expect("two segments");
    let stored = cap_vertex_reference(plate.extrude, CapSide::Start, joint);
    document
        .write(|w| w.put_topology_ref(&stored))
        .expect("the document stores a cap-vertex reference");
    document.close().expect("closes");

    let reopened = Document::open(&path).expect("reopens");
    let reference = reopened
        .topology_refs()
        .expect("reads the stored references")
        .into_iter()
        .find(|r| r.id == stored.id)
        .expect("the reference is still there");

    let mut kernel = OcctKernel::new().expect("opens a session");
    let built = rebuild_cold(&reopened, &mut kernel, &OperationContext::default())
        .expect("the plate rebuilds through Open CASCADE");

    let found = resolve(built.topology(), &reference).expect("a stored corner names one vertex");
    assert_eq!(
        found.len(),
        1,
        "an exact reference names exactly one vertex"
    );
    assert_eq!(found[0].kind(), SubShapeKind::Vertex);
    assert_eq!(
        Some(found[0].shape()),
        built.shape(plate.extrude),
        "and it belongs to the solid the feature built"
    );

    built.release_all(&mut kernel);
    assert_eq!(kernel.live_shape_count(), 0);
}

/// A closed profile of straight segments through the given planar points.
fn loop_of(
    points: &[(f64, f64)],
    extent: ExtrudeExtent,
    reversed: bool,
) -> Result<(ExtrudeRequest, Vec<StableEntityId>)> {
    let planar: Vec<PlanarPoint> = points
        .iter()
        .map(|(x, y)| PlanarPoint::new(*x, *y))
        .collect::<Result<_>>()?;
    let mut segments = Vec::new();
    let mut labels = Vec::new();
    for (index, start) in planar.iter().enumerate() {
        let label = StableEntityId::new();
        labels.push(label);
        segments.push(ProfileSegment::new(
            label,
            SegmentGeometry::line(*start, planar[(index + 1) % planar.len()])?,
        ));
    }
    Ok((
        ExtrudeRequest::new(
            Profile::new(
                SketchPlane::world_xy(),
                ProfileLoop::new(segments)?,
                Vec::new(),
            )?,
            extent,
            reversed,
        ),
        labels,
    ))
}

/// Three segments, two straight and one curved, so a corner between two lines
/// and a corner between a line and an arc both occur and can be told apart.
fn arc_profile(extent: ExtrudeExtent) -> Result<(ExtrudeRequest, Vec<StableEntityId>)> {
    let labels = [
        StableEntityId::new(),
        StableEntityId::new(),
        StableEntityId::new(),
    ];
    let arc = ProfileSegment::new(
        labels[0],
        SegmentGeometry::arc(PlanarPoint::ORIGIN, 10.0, 0.0, PI)?,
    );
    let down = ProfileSegment::new(
        labels[1],
        SegmentGeometry::line(PlanarPoint::new(-10.0, 0.0)?, PlanarPoint::new(0.0, -20.0)?)?,
    );
    let up = ProfileSegment::new(
        labels[2],
        SegmentGeometry::line(PlanarPoint::new(0.0, -20.0)?, PlanarPoint::new(10.0, 0.0)?)?,
    );
    Ok((
        ExtrudeRequest::new(
            Profile::new(
                SketchPlane::world_xy(),
                ProfileLoop::new(vec![arc, down, up])?,
                Vec::new(),
            )?,
            extent,
            false,
        ),
        labels.to_vec(),
    ))
}

/// Two segments closing a lens: one unordered pair occurring at two corners.
fn lens() -> Result<(ExtrudeRequest, Vec<StableEntityId>)> {
    let labels = [StableEntityId::new(), StableEntityId::new()];
    let left = PlanarPoint::new(-10.0, 0.0)?;
    let right = PlanarPoint::new(10.0, 0.0)?;
    let over = ProfileSegment::new(
        labels[0],
        SegmentGeometry::arc(PlanarPoint::ORIGIN, 10.0, 0.0, PI)?,
    );
    let back = ProfileSegment::new(labels[1], SegmentGeometry::line(left, right)?);
    Ok((
        ExtrudeRequest::new(
            Profile::new(
                SketchPlane::world_xy(),
                ProfileLoop::new(vec![over, back])?,
                Vec::new(),
            )?,
            ExtrudeExtent::blind(5.0)?,
            false,
        ),
        labels.to_vec(),
    ))
}

struct Built {
    kernel: OcctKernel,
    map: TopologyMap,
    feature: ObjectId,
    shape: ShapeHandle,
}

fn build(request: &ExtrudeRequest) -> Built {
    let feature = ObjectId::new();
    let mut kernel = OcctKernel::new().expect("opens");
    let result = kernel
        .extrude(request, &OperationContext::default())
        .expect("Open CASCADE builds the profile");
    let mut map = TopologyMap::new();
    map.record_extrude(feature, request.profile(), &result)
        .expect("records");
    let shape = result.shape;
    Built {
        kernel,
        map,
        feature,
        shape,
    }
}

/// The corner where the segment before `index` meets the segment at `index`.
fn joint_at(labels: &[StableEntityId], index: usize) -> ProfileJoint {
    ProfileJoint::new(
        labels[(index + labels.len() - 1) % labels.len()],
        labels[index],
    )
    .expect("two different segments")
}

/// The one vertex a corner and a cap resolve to.
fn cap_vertex_of(built: &Built, side: CapSide, joint: ProfileJoint) -> Result<SubShapeHandle> {
    let resolved = resolve(
        &built.map,
        &cap_vertex_reference(built.feature, side, joint),
    )?;
    assert_eq!(resolved.len(), 1, "one vertex, not {}", resolved.len());
    Ok(resolved[0])
}

/// How many corners of a profile name a vertex on each cap.
fn named_counts(built: &Built, labels: &[StableEntityId]) -> (usize, usize) {
    let names = built.map.feature(built.feature).expect("recorded");
    let mut counts = (0, 0);
    for index in 0..labels.len() {
        let joint = joint_at(labels, index);
        if names
            .cap_vertex(CapSide::Start, joint)
            .expect("the start side is known")
            .count()
            == 1
        {
            counts.0 += 1;
        }
        if names
            .cap_vertex(CapSide::End, joint)
            .expect("the end side is known")
            .count()
            == 1
        {
            counts.1 += 1;
        }
    }
    counts
}

#[test]
fn a_plate_names_four_corners_on_each_cap_and_each_names_one_vertex() {
    kernel_or_skip!();

    let (request, labels) = plate(10.0).expect("a valid plate");
    let mut built = build(&request);

    assert_eq!(
        named_counts(&built, &labels),
        (4, 4),
        "four corners reach each cap"
    );

    // Every one of the eight resolves, to one vertex of this solid, and no two
    // of them to the same vertex.
    let mut seen = BTreeSet::new();
    for index in 0..labels.len() {
        let joint = joint_at(&labels, index);
        for side in [CapSide::Start, CapSide::End] {
            let vertex = cap_vertex_of(&built, side, joint).expect("a corner names one vertex");
            assert_eq!(vertex.kind(), SubShapeKind::Vertex, "{side:?} {joint}");
            assert_eq!(vertex.shape(), built.shape, "{side:?} {joint}");
            assert!(seen.insert(vertex), "{side:?} {joint} names a vertex twice");
        }
    }
    assert_eq!(seen.len(), 8, "a plate has eight corner vertices");

    // The two ends of one corner are different points. Swapping the sides
    // would leave the count above intact and this assertion failing.
    for index in 0..labels.len() {
        let joint = joint_at(&labels, index);
        assert_ne!(
            cap_vertex_of(&built, CapSide::Start, joint).expect("resolves"),
            cap_vertex_of(&built, CapSide::End, joint).expect("resolves"),
            "{joint} named one vertex on both caps"
        );
    }

    built.kernel.release(built.shape);
}

/// Which packed positions the tessellation draws each of a shape's parts at.
///
/// Everything below is handle identity and index membership. No coordinate is
/// compared anywhere, and no ordinal is matched: the question is whether the
/// vertex a durable name resolved to is a corner of the face and the edge the
/// name claims, which the partitions answer directly.
struct Drawn {
    corner_positions: BTreeMap<SubShapeHandle, Vec<u32>>,
    face_positions: BTreeMap<SubShapeHandle, BTreeSet<u32>>,
    edge_positions: BTreeMap<SubShapeHandle, BTreeSet<u32>>,
}

fn drawn(built: &mut Built) -> Drawn {
    let mesh = built
        .kernel
        .tessellate(
            built.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes");

    let corners = mesh
        .topological_vertices
        .as_ref()
        .expect("the kernel names the corners");
    let mut corner_positions = BTreeMap::new();
    for range in &corners.ranges {
        let first = range.first_occurrence as usize;
        let last = first + range.occurrence_count as usize;
        corner_positions.insert(range.vertex, corners.occurrences[first..last].to_vec());
    }

    let mut face_positions: BTreeMap<SubShapeHandle, BTreeSet<u32>> = BTreeMap::new();
    for range in &mesh.faces {
        let first = range.first_index as usize;
        let last = first + range.index_count as usize;
        face_positions
            .entry(range.face)
            .or_default()
            .extend(mesh.indices[first..last].iter().copied());
    }

    let mut edge_positions: BTreeMap<SubShapeHandle, BTreeSet<u32>> = BTreeMap::new();
    if let Some(edges) = mesh.edges.as_ref() {
        for range in &edges.ranges {
            let first = range.first_segment as usize * 2;
            let last = first + range.segment_count as usize * 2;
            edge_positions
                .entry(range.edge)
                .or_default()
                .extend(edges.segments[first..last].iter().copied());
        }
    }

    Drawn {
        corner_positions,
        face_positions,
        edge_positions,
    }
}

fn cap_reference(feature: ObjectId, side: CapSide) -> TopologyRef {
    TopologyRef {
        id: StableEntityId::new(),
        owner: feature,
        producer_feature: feature,
        expected_kind: EntityKind::Face,
        output_role: SemanticRole::ExtrudeCap { side },
        selection: SelectionRule::Exact,
        fallback_signature: None,
    }
}

fn sweep_reference(feature: ObjectId, joint: ProfileJoint) -> TopologyRef {
    TopologyRef {
        id: StableEntityId::new(),
        owner: feature,
        producer_feature: feature,
        expected_kind: EntityKind::Edge,
        output_role: SemanticRole::ExtrudeSweepEdge { joint },
        selection: SelectionRule::Exact,
        fallback_signature: None,
    }
}

#[test]
fn every_named_vertex_lies_on_the_cap_it_claims_and_ends_its_own_sweep_edge() {
    kernel_or_skip!();

    let (request, labels) = plate(10.0).expect("a valid plate");
    let mut built = build(&request);
    let picture = drawn(&mut built);

    for side in [CapSide::Start, CapSide::End] {
        // The cap this side names, taken through the same resolver, so the two
        // durable names are compared with each other rather than with an
        // index into the result.
        let cap = resolve(&built.map, &cap_reference(built.feature, side))
            .expect("a cap resolves")
            .remove(0);
        let on_cap = picture
            .face_positions
            .get(&cap)
            .expect("the cap is drawn")
            .clone();

        for index in 0..labels.len() {
            let joint = joint_at(&labels, index);
            let vertex = cap_vertex_of(&built, side, joint).expect("resolves");
            let mine = picture
                .corner_positions
                .get(&vertex)
                .expect("the corner is drawn");
            assert!(!mine.is_empty());

            assert!(
                mine.iter().any(|at| on_cap.contains(at)),
                "{side:?} {joint} names a vertex that is not on the {side:?} cap"
            );

            // And it is an end of the edge swept from its own corner, not of a
            // neighbour's: the sweep edge is resolved from the same joint.
            let swept = resolve(&built.map, &sweep_reference(built.feature, joint))
                .expect("the corner also names an edge")
                .remove(0);
            let along = picture
                .edge_positions
                .get(&swept)
                .expect("the sweep edge is drawn");
            assert!(
                mine.iter().any(|at| along.contains(at)),
                "{side:?} {joint} names a vertex that does not end the edge swept from {joint}"
            );

            // The same vertex must not be an end of a different corner's swept
            // edge, which is what shifting a name one corner along would look
            // like on a shape where every corner is drawn alike.
            for other in 0..labels.len() {
                if other == index {
                    continue;
                }
                let neighbour = joint_at(&labels, other);
                let elsewhere = resolve(&built.map, &sweep_reference(built.feature, neighbour))
                    .expect("resolves")
                    .remove(0);
                let along = picture
                    .edge_positions
                    .get(&elsewhere)
                    .expect("drawn")
                    .clone();
                assert!(
                    !mine.iter().any(|at| along.contains(at)),
                    "{side:?} {joint} names a vertex that ends {neighbour}'s swept edge instead"
                );
            }
        }
    }

    built.kernel.release(built.shape);
}

#[test]
fn a_corner_keeps_its_meaning_when_the_sweep_is_blind_reversed_or_symmetric() {
    kernel_or_skip!();

    let corners = [(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)];
    let labels: Vec<StableEntityId> = (0..4).map(|_| StableEntityId::new()).collect();

    // One profile, three sweeps of it. The segment labels are shared, so the
    // corners are literally the same corners and the question is only whether
    // the sweep changed what they name.
    let of = |extent: ExtrudeExtent, reversed: bool| -> ExtrudeRequest {
        let planar: Vec<PlanarPoint> = corners
            .iter()
            .map(|(x, y)| PlanarPoint::new(*x, *y).expect("finite"))
            .collect();
        let segments: Vec<ProfileSegment> = (0..planar.len())
            .map(|index| {
                ProfileSegment::new(
                    labels[index],
                    SegmentGeometry::line(planar[index], planar[(index + 1) % planar.len()])
                        .expect("distinct"),
                )
            })
            .collect();
        ExtrudeRequest::new(
            Profile::new(
                SketchPlane::world_xy(),
                ProfileLoop::new(segments).expect("closes"),
                Vec::new(),
            )
            .expect("valid"),
            extent,
            reversed,
        )
    };

    for (what, request) in [
        (
            "blind",
            of(ExtrudeExtent::blind(10.0).expect("positive"), false),
        ),
        (
            "reversed",
            of(ExtrudeExtent::blind(10.0).expect("positive"), true),
        ),
        (
            "symmetric",
            of(ExtrudeExtent::symmetric(5.0).expect("positive"), false),
        ),
    ] {
        let mut built = build(&request);
        assert_eq!(
            named_counts(&built, &labels),
            (4, 4),
            "{what}: four corners should reach each cap"
        );

        let mut seen = BTreeSet::new();
        for index in 0..labels.len() {
            let joint = joint_at(&labels, index);
            for side in [CapSide::Start, CapSide::End] {
                let vertex = cap_vertex_of(&built, side, joint)
                    .unwrap_or_else(|e| panic!("{what}: {side:?} {joint} should resolve: {e}"));
                assert!(
                    seen.insert(vertex),
                    "{what}: {side:?} {joint} is not its own"
                );
            }
        }
        assert_eq!(seen.len(), 8, "{what}");
        built.kernel.release(built.shape);
    }
}

#[test]
fn walking_the_profile_from_another_segment_leaves_each_corner_owning_its_vertices() {
    kernel_or_skip!();

    let corners = [(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)];
    let labels: Vec<StableEntityId> = (0..4).map(|_| StableEntityId::new()).collect();

    // The same closed loop entered at each of its four segments. A name that
    // depended on a position in the list would move; a name that is the pair
    // of segments cannot.
    let rotated = |start: usize| -> ExtrudeRequest {
        let planar: Vec<PlanarPoint> = corners
            .iter()
            .map(|(x, y)| PlanarPoint::new(*x, *y).expect("finite"))
            .collect();
        let segments: Vec<ProfileSegment> = (0..planar.len())
            .map(|step| {
                let index = (start + step) % planar.len();
                ProfileSegment::new(
                    labels[index],
                    SegmentGeometry::line(planar[index], planar[(index + 1) % planar.len()])
                        .expect("distinct"),
                )
            })
            .collect();
        ExtrudeRequest::new(
            Profile::new(
                SketchPlane::world_xy(),
                ProfileLoop::new(segments).expect("closes"),
                Vec::new(),
            )
            .expect("valid"),
            ExtrudeExtent::blind(10.0).expect("positive"),
            false,
        )
    };

    // What each corner means, by the faces it touches, so two rebuilds can be
    // compared without comparing handles from different sessions.
    let meanings = |start: usize| -> BTreeMap<(String, ProfileJoint), BTreeSet<u32>> {
        let request = rotated(start);
        let mut built = build(&request);
        let picture = drawn(&mut built);
        let mut out = BTreeMap::new();
        for index in 0..labels.len() {
            let joint = joint_at(&labels, index);
            for (name, side) in [("start", CapSide::Start), ("end", CapSide::End)] {
                let vertex = cap_vertex_of(&built, side, joint).expect("resolves");
                let mine: BTreeSet<u32> = picture
                    .corner_positions
                    .get(&vertex)
                    .expect("drawn")
                    .iter()
                    .copied()
                    .collect();
                // Which of this rebuild's own named sides the corner touches.
                // Side faces are named by segment, so this is a description in
                // durable terms and not a handle.
                let mut touching = BTreeSet::new();
                for (position, segment) in labels.iter().enumerate() {
                    let names = built.map.feature(built.feature).expect("recorded");
                    for face in names.side(*segment) {
                        if let Some(positions) = picture.face_positions.get(&face)
                            && mine.iter().any(|at| positions.contains(at))
                        {
                            touching.insert(position as u32);
                        }
                    }
                }
                out.insert((name.to_owned(), joint), touching);
            }
        }
        built.kernel.release(built.shape);
        out
    };

    let first = meanings(0);
    assert_eq!(first.len(), 8);
    for (_, touching) in first.iter() {
        assert_eq!(touching.len(), 2, "a corner of a plate touches two sides");
    }
    for start in 1..labels.len() {
        assert_eq!(
            meanings(start),
            first,
            "entering the loop at segment {start} changed what a corner names"
        );
    }
}

#[test]
fn a_triangle_names_three_corners_a_side_and_an_arc_profile_names_its_own_three() {
    kernel_or_skip!();

    let (triangle, triangle_labels) = loop_of(
        &[(0.0, 0.0), (30.0, 0.0), (15.0, 25.0)],
        ExtrudeExtent::blind(6.0).expect("positive"),
        false,
    )
    .expect("a valid triangle");
    let mut built = build(&triangle);
    assert_eq!(
        named_counts(&built, &triangle_labels),
        (3, 3),
        "a triangle has three corners"
    );
    let mut seen = BTreeSet::new();
    for index in 0..triangle_labels.len() {
        let joint = joint_at(&triangle_labels, index);
        for side in [CapSide::Start, CapSide::End] {
            assert!(
                seen.insert(
                    cap_vertex_of(&built, side, joint).expect("a triangle corner resolves")
                )
            );
        }
    }
    assert_eq!(seen.len(), 6);
    built.kernel.release(built.shape);

    // Three segments again, but one of them curved, so a corner between two
    // lines and a corner where a line meets an arc are both named and are not
    // each other.
    let (arc, arc_labels) =
        arc_profile(ExtrudeExtent::blind(5.0).expect("positive")).expect("a valid arc profile");
    let mut built = build(&arc);
    assert_eq!(named_counts(&built, &arc_labels), (3, 3));
    let mut seen = BTreeSet::new();
    for index in 0..arc_labels.len() {
        let joint = joint_at(&arc_labels, index);
        for side in [CapSide::Start, CapSide::End] {
            assert!(
                seen.insert(cap_vertex_of(&built, side, joint).expect("an arc corner resolves")),
                "{side:?} {joint} is not its own vertex"
            );
        }
    }
    assert_eq!(seen.len(), 6);
    built.kernel.release(built.shape);
}

#[test]
fn a_pair_that_meets_at_two_corners_names_neither_vertex() {
    kernel_or_skip!();

    let (request, labels) = lens().expect("a valid lens");
    let mut built = build(&request);

    // Four corner vertices in the solid, and one unordered pair to name them
    // with. The name cannot choose, so nothing is named.
    let joint = ProfileJoint::new(labels[0], labels[1]).expect("two different segments");
    let names = built.map.feature(built.feature).expect("recorded");
    for side in [CapSide::Start, CapSide::End] {
        assert_eq!(
            names
                .cap_vertex(side, joint)
                .expect("the side is known")
                .count(),
            0,
            "a pair meeting at two corners named a {side:?} vertex"
        );
    }

    for side in [CapSide::Start, CapSide::End] {
        let refusal = resolve(
            &built.map,
            &cap_vertex_reference(built.feature, side, joint),
        )
        .expect_err("an ambiguous corner is refused rather than chosen between");
        assert_eq!(refusal.kind(), ErrorKind::Topology);
        assert!(refusal.to_string().contains("produced none"), "{refusal}");
    }

    built.kernel.release(built.shape);
}

#[test]
fn corner_names_survive_an_archive_and_a_session_that_never_built_them() {
    kernel_or_skip!();

    let (request, labels) = plate(10.0).expect("a valid plate");
    let feature = ObjectId::new();

    // Everything the writing session knows, taken before it ends. The faces
    // and edges go into the same table as the vertices, so what travels below
    // is a mixed archive rather than one of vertices alone.
    let archived = {
        let mut writer = OcctKernel::new().expect("opens");
        let result = writer
            .extrude(&request, &OperationContext::default())
            .expect("builds");
        let mut map = TopologyMap::new();
        map.record_extrude(feature, request.profile(), &result)
            .expect("records");
        let archived = archive_feature(&mut writer, &map, feature).expect("archives");
        writer.release(result.shape);
        assert_eq!(writer.live_shape_count(), 0);
        archived
    };

    // Faces, edges and vertices are all in the table, each under its own kind.
    let mut kinds = BTreeMap::new();
    for (name, _) in archived.bindings() {
        *kinds.entry(name.kind()).or_insert(0usize) += 1;
    }
    assert_eq!(
        kinds.get(&SubShapeKind::Vertex).copied(),
        Some(8),
        "eight corner vertices should have been archived"
    );
    assert_eq!(
        kinds.get(&SubShapeKind::Face).copied(),
        Some(6),
        "and the faces beside them"
    );
    assert_eq!(
        kinds.get(&SubShapeKind::Edge).copied(),
        Some(12),
        "and the edges beside those"
    );

    // A session that never saw the original.
    let mut reader = OcctKernel::new().expect("opens");
    let mut restored = TopologyMap::new();
    restore_feature(&mut reader, &archived, &mut restored).expect("restores");

    let shape = restored
        .feature(feature)
        .and_then(|names| names.shape())
        .expect("the restore produced a shape");

    let mut seen = BTreeSet::new();
    for index in 0..labels.len() {
        let joint = joint_at(&labels, index);
        for side in [CapSide::Start, CapSide::End] {
            let found = resolve(&restored, &cap_vertex_reference(feature, side, joint))
                .unwrap_or_else(|e| panic!("{side:?} {joint} should resolve after restoring: {e}"));
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].kind(), SubShapeKind::Vertex);
            assert_eq!(found[0].shape(), shape, "a name of the restored shape");
            assert!(seen.insert(found[0]), "{side:?} {joint} is not its own");
        }
    }
    assert_eq!(seen.len(), 8);

    // The faces and edges are still there too: a table that gained vertices
    // must not have lost what it already carried.
    for side in [CapSide::Start, CapSide::End] {
        assert_eq!(
            resolve(&restored, &cap_reference(feature, side))
                .expect("a cap still resolves")
                .len(),
            1
        );
    }
    for index in 0..labels.len() {
        assert_eq!(
            resolve(
                &restored,
                &sweep_reference(feature, joint_at(&labels, index))
            )
            .expect("a sweep edge still resolves")
            .len(),
            1
        );
    }

    reader.release(shape);
    assert_eq!(reader.live_shape_count(), 0);
}

#[test]
fn a_named_archive_of_vertices_that_cannot_be_read_leaves_no_shape_behind() {
    kernel_or_skip!();

    let (request, _) = plate(10.0).expect("a valid plate");
    let mut writer = OcctKernel::new().expect("opens");
    let result = writer
        .extrude(&request, &OperationContext::default())
        .expect("builds");
    let vertices: Vec<SubShapeHandle> = result
        .start_cap_vertices
        .values()
        .chain(result.end_cap_vertices.values())
        .copied()
        .collect();
    let (blob, slots) = writer
        .encode_shape_with(result.shape, &vertices)
        .expect("archives");
    writer.release(result.shape);
    assert_eq!(writer.live_shape_count(), 0);

    let mut reader = OcctKernel::new().expect("opens");
    // A slot past the end of the table. The decode gets as far as reading the
    // B-Rep and must not keep the shape it built to answer with.
    let mut wrong = slots.clone();
    wrong.push(ferritecad_kernel::ArchiveSlot::new(9_999));
    reader
        .decode_shape_with(&blob, &wrong)
        .expect_err("a slot outside the archive is refused");
    assert_eq!(
        reader.live_shape_count(),
        0,
        "a decode that answered nothing left a shape behind"
    );

    // And the root slot, which is the shape and not a sub-shape.
    reader
        .decode_shape_with(&blob, &[ferritecad_kernel::ArchiveSlot::ROOT])
        .expect_err("slot zero is not a sub-shape");
    assert_eq!(reader.live_shape_count(), 0);

    // The same bytes with a table this build can read still restore, so the
    // refusals above are about the table and not about vertices.
    let (shape, restored) = reader.decode_shape_with(&blob, &slots).expect("restores");
    assert_eq!(restored.len(), 8);
    reader.release(shape);
    assert_eq!(reader.live_shape_count(), 0);
}

#[test]
fn a_cache_hit_and_a_cold_rebuild_name_the_same_corners() {
    kernel_or_skip!();

    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("plate.fcad");
    let mut document = Document::create(&path).expect("creates");
    let plate = store_plate(
        &mut document,
        &[(0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (0.0, 40.0)],
        10.0,
    )
    .expect("populates");

    // One stored reference per corner and cap, written into the document so
    // both rebuilds below answer the same eight questions.
    let mut stored = Vec::new();
    for index in 0..plate.segments.len() {
        let joint = joint_at(&plate.segments, index);
        for side in [CapSide::Start, CapSide::End] {
            stored.push(cap_vertex_reference(plate.extrude, side, joint));
        }
    }
    document
        .write(|w| {
            for reference in &stored {
                w.put_topology_ref(reference)?;
            }
            Ok(())
        })
        .expect("the document stores eight cap-vertex references");

    let document_id = document.meta().document_id;
    let cache_path = dir.path().join("plate.fcad-cache");
    let context = OperationContext::default();

    // What a corner means, said without a handle: which of this rebuild's own
    // named faces the vertex is drawn on. Two sessions cannot compare handles,
    // and that is the point of the exercise.
    let meanings = |built: &ferritecad_eval::RebuildResult,
                    kernel: &mut OcctKernel|
     -> BTreeMap<usize, BTreeSet<String>> {
        let shape = built.shape(plate.extrude).expect("a solid");
        let mesh = kernel
            .tessellate(shape, &TessellationParams::default(), &context)
            .expect("meshes");
        let corners = mesh
            .topological_vertices
            .as_ref()
            .expect("the kernel names the corners");
        let mut face_positions: BTreeMap<SubShapeHandle, BTreeSet<u32>> = BTreeMap::new();
        for range in &mesh.faces {
            let first = range.first_index as usize;
            let last = first + range.index_count as usize;
            face_positions
                .entry(range.face)
                .or_default()
                .extend(mesh.indices[first..last].iter().copied());
        }

        let mut out = BTreeMap::new();
        for (position, reference) in stored.iter().enumerate() {
            let found = resolve(built.topology(), reference).expect("a stored corner resolves");
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].kind(), SubShapeKind::Vertex);
            let range = corners
                .ranges
                .iter()
                .find(|range| range.vertex == found[0])
                .expect("the corner is drawn");
            let first = range.first_occurrence as usize;
            let last = first + range.occurrence_count as usize;
            let mine: BTreeSet<u32> = corners.occurrences[first..last].iter().copied().collect();

            // Named in durable terms: the caps by side, the sides by segment.
            let names = built
                .topology()
                .feature(plate.extrude)
                .expect("the feature is named");
            let mut on = BTreeSet::new();
            for (label, side) in [("start cap", CapSide::Start), ("end cap", CapSide::End)] {
                for face in names.cap(side).expect("the side is known") {
                    if let Some(positions) = face_positions.get(&face)
                        && mine.iter().any(|at| positions.contains(at))
                    {
                        on.insert(label.to_owned());
                    }
                }
            }
            for (index, segment) in plate.segments.iter().enumerate() {
                for face in names.side(*segment) {
                    if let Some(positions) = face_positions.get(&face)
                        && mine.iter().any(|at| positions.contains(at))
                    {
                        on.insert(format!("side {index}"));
                    }
                }
            }
            out.insert(position, on);
        }
        out
    };

    // A miss, which computes the solid and writes an archive.
    let cold = {
        let mut kernel = OcctKernel::new().expect("opens");
        let mut cache = CacheStore::open(
            &cache_path,
            document_id,
            kernel.identity().id(),
            kernel.identity().version(),
        )
        .expect("the sidecar opens");
        let (built, events) =
            rebuild_cached(&document, &mut kernel, &mut cache, &context).expect("rebuilds");
        assert_eq!(events[0].outcome, CacheOutcome::Miss);
        let answers = meanings(&built, &mut kernel);
        built.release_all(&mut kernel);
        assert_eq!(kernel.live_shape_count(), 0);
        answers
    };
    assert_eq!(cold.len(), 8);
    for (position, on) in &cold {
        assert_eq!(
            on.len(),
            3,
            "reference {position} should touch a cap and two sides, found {on:?}"
        );
    }

    // And a hit, in a session that never built this plate.
    let warm = {
        let mut kernel = OcctKernel::new().expect("opens");
        let mut cache = CacheStore::open(
            &cache_path,
            document_id,
            kernel.identity().id(),
            kernel.identity().version(),
        )
        .expect("the sidecar opens again");
        let (built, events) =
            rebuild_cached(&document, &mut kernel, &mut cache, &context).expect("rebuilds");
        assert_eq!(
            events.iter().map(|e| e.outcome).collect::<Vec<_>>(),
            vec![CacheOutcome::Hit],
            "the plate should have come out of the sidecar"
        );
        let answers = meanings(&built, &mut kernel);
        built.release_all(&mut kernel);
        assert_eq!(kernel.live_shape_count(), 0);
        answers
    };

    assert_eq!(
        warm, cold,
        "a cache hit named the corners differently from the rebuild that filled it"
    );

    // A plain cold rebuild, with no cache in play at all, agrees too.
    let mut kernel = OcctKernel::new().expect("opens");
    let built = rebuild_cold(&document, &mut kernel, &context).expect("rebuilds");
    let plain = meanings(&built, &mut kernel);
    built.release_all(&mut kernel);
    assert_eq!(plain, cold);
}
