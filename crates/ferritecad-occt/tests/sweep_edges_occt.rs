// SPDX-License-Identifier: MIT
//! The edges running along an extrusion, against the kernel that ships.
//!
//! A plate has twelve topological edges. Eight of them are where a cap meets a
//! swept face and already carry durable names; the other four run along the
//! sweep, one at each corner of the profile. This is about those four.
//!
//! A corner belongs to the two segments that meet there and to neither of them
//! alone, so the name is the unordered pair. Everything here is held to that:
//! walking the profile from elsewhere, sweeping it the other way, or editing a
//! part of it that does not touch the corner must all leave the name alone.

// A test asserting the shape of a value has nowhere to return an error to.
#![allow(clippy::panic)]

#[path = "../../ferritecad-fixtures/tests/support/naming.rs"]
mod naming;

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::PI;

use ferritecad_document::{Document, ObjectPayload, SweptEdge};
use ferritecad_eval::rebuild_cold;
use ferritecad_fixtures::{drop_segment, open_plate};
use ferritecad_kernel::{
    CancelToken, ExtrudeExtent, ExtrudeRequest, GeometryKernel, OperationContext, PlanarPoint,
    Profile, ProfileLoop, ProfileSegment, SegmentGeometry, ShapeHandle, SketchPlane,
    SubShapeHandle, SubShapeKind, TessellationParams,
};
use ferritecad_occt::{OcctKernel, is_available};
use ferritecad_topology::{ModelNaming, archive_feature, restore_feature};
use ferritecad_types::{ErrorKind, ObjectId, ProfileJoint, Result, StableEntityId, Tolerance};

macro_rules! kernel_or_skip {
    () => {
        if !is_available() {
            ferritecad_fixtures::native_dependencies(&[(false, "OCCT")]);
            return;
        }
    };
}

/// A rectangular plate, four labelled straight segments.
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

/// Three segments, two straight and one curved, so a joint between two lines
/// and a joint between a line and an arc are both present and distinguishable.
///
/// The arc runs the long way round from (10, 0) to (-10, 0), and the two lines
/// close the shape through (0, -20).
fn arc_profile() -> Result<(ExtrudeRequest, Vec<StableEntityId>)> {
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
            ExtrudeExtent::blind(5.0)?,
            false,
        ),
        labels.to_vec(),
    ))
}

struct Built {
    kernel: OcctKernel,
    map: ModelNaming,
    feature: ObjectId,
    shape: ShapeHandle,
}

fn build(request: &ExtrudeRequest) -> Built {
    let feature = ObjectId::new();
    let mut kernel = OcctKernel::new().expect("opens");
    let result = kernel
        .extrude(request, &OperationContext::default())
        .expect("builds");
    let map =
        naming::bind(&mut kernel, feature, request.profile(), &result).expect("complete naming");
    let shape = result.shape;
    Built {
        kernel,
        map,
        feature,
        shape,
    }
}

fn sweep_reference(feature: ObjectId, joint: ProfileJoint) -> SweptEdge {
    SweptEdge { feature, joint }
}
fn resolve_sweep(map: &ModelNaming, edge: &SweptEdge) -> Result<Vec<SubShapeHandle>> {
    Ok(vec![map.swept_edge(edge.feature, *edge)?])
}

fn cap_edge_name(
    map: &ModelNaming,
    feature: ObjectId,
    end: bool,
    segment: StableEntityId,
) -> ferritecad_topology::generative::Name {
    let plane = naming::plane_id();
    naming::name_at_incidence(
        map,
        feature,
        SubShapeKind::Edge,
        [
            naming::side_name(feature, plane, segment),
            naming::cap_name(feature, plane, end),
        ],
    )
    .expect("one cap boundary")
}

fn side_name(
    _map: &ModelNaming,
    feature: ObjectId,
    segment: StableEntityId,
) -> ferritecad_topology::generative::Name {
    naming::side_name(feature, naming::plane_id(), segment)
}

/// The corner where the segment before `index` meets the segment at `index`.
fn joint_at(labels: &[StableEntityId], index: usize) -> ProfileJoint {
    ProfileJoint::new(
        labels[(index + labels.len() - 1) % labels.len()],
        labels[index],
    )
    .expect("two different segments")
}

/// The one edge a joint resolves to.
fn sweep_edge_of(built: &Built, joint: ProfileJoint) -> Result<SubShapeHandle> {
    let resolved = resolve_sweep(&built.map, &sweep_reference(built.feature, joint))?;
    assert_eq!(resolved.len(), 1, "one edge, not {}", resolved.len());
    Ok(resolved[0])
}

#[test]
fn a_profile_joint_derives_the_edge_running_along_the_sweep() {
    kernel_or_skip!();

    // A temporary copy of the committed plate. The reference is written into
    // the copy: the committed fixture keeps naming exactly what it named.
    let dir = tempfile::tempdir().expect("temp dir");
    let document = open_plate(dir.path()).expect("the fixture opens");
    let segments = segments_of(&document).expect("reads the sketch");
    let feature = extrusion_of(&document);

    let joint = ProfileJoint::new(segments[0], segments[1]).expect("two different segments");
    let reference = sweep_reference(feature, joint);

    let mut kernel = OcctKernel::new().expect("opens");
    let built = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("Open CASCADE rebuilds the stored plate");

    let resolved = resolve_sweep(built.topology(), &reference)
        .unwrap_or_else(|error| panic!("{joint} names no edge: {error}"));
    assert_eq!(resolved.len(), 1, "one edge, not {}", resolved.len());
    assert_eq!(resolved[0].kind(), SubShapeKind::Edge);

    let named = built.topology().feature(feature).expect("named feature");
    let name = named.entities[&resolved[0]];
    assert_eq!(named.resolve(name).expect("primary name"), resolved[0]);
    let written = built
        .topology()
        .describe(feature, name)
        .expect("complete description");
    for forbidden in [
        "SessionId",
        "ShapeHandle",
        "SubShapeHandle",
        "index",
        "ordinal",
        "session",
    ] {
        assert!(
            !written.contains(forbidden),
            "a stored joint mentions {forbidden}: {written}"
        );
    }
    for segment in joint.segments() {
        assert!(written.contains(&segment.to_string()));
    }

    built.release_all(&mut kernel);
}

#[test]
fn the_four_joints_and_the_eight_cap_edges_cover_the_plate_exactly_once() {
    kernel_or_skip!();

    let (request, labels) = plate(10.0).expect("a valid plate");
    let mut built = build(&request);

    let mut swept = BTreeSet::new();
    for index in 0..labels.len() {
        let joint = joint_at(&labels, index);
        let edge = sweep_edge_of(&built, joint)
            .unwrap_or_else(|error| panic!("{joint} names no edge: {error}"));
        assert_eq!(edge.kind(), SubShapeKind::Edge);
        assert_eq!(edge.shape(), built.shape);
        assert!(swept.insert(edge), "{edge} was named by two joints");
    }
    assert_eq!(swept.len(), 4, "a plate has four corners");

    let mut capped = BTreeSet::new();
    for segment in &labels {
        for side in [false, true] {
            let resolved = naming::resolve(
                &built.map,
                built.feature,
                &cap_edge_name(&built.map, built.feature, side, *segment),
            )
            .expect("the cap edges are still named");
            assert_eq!(resolved.len(), 1);
            assert!(capped.insert(resolved[0]));
        }
    }
    assert_eq!(capped.len(), 8);

    // No edge answers to both kinds of name, and together they are the whole
    // shape rather than most of it.
    assert!(
        swept.is_disjoint(&capped),
        "an edge is named both along the sweep and at a cap"
    );
    let drawn = drawn_edges(&mut built);
    assert_eq!(drawn.len(), 12, "a plate has twelve topological edges");
    let named: BTreeSet<SubShapeHandle> = swept.union(&capped).copied().collect();
    assert_eq!(named, drawn, "the names do not cover the plate exactly");

    built.kernel.release(built.shape);
}

#[test]
fn each_joint_names_the_edge_where_its_own_two_swept_faces_meet() {
    kernel_or_skip!();

    let (request, labels) = plate(10.0).expect("a valid plate");
    let mut built = build(&request);
    let bounding = edge_faces(&mut built);

    for index in 0..labels.len() {
        let joint = joint_at(&labels, index);
        let edge = sweep_edge_of(&built, joint).expect("named");

        let mut expected = BTreeSet::new();
        for segment in joint.segments() {
            let faces = naming::resolve(
                &built.map,
                built.feature,
                &side_name(&built.map, built.feature, segment),
            )
            .expect("the swept faces are named");
            expected.extend(faces);
        }
        assert_eq!(expected.len(), 2, "two segments raise two faces");

        let found = bounding.get(&edge).expect("the edge is drawn");
        assert_eq!(
            found, &expected,
            "{joint} names an edge bounded by the wrong faces"
        );
    }

    built.kernel.release(built.shape);
}

#[test]
fn every_way_of_sweeping_one_profile_names_the_same_joints() {
    kernel_or_skip!();

    let (base, labels) = plate(10.0).expect("a valid plate");
    let profile = base.profile().clone();

    // Which joints are named, and how many edges each names, must not depend
    // on which way or how far the profile was swept. The handles differ
    // between builds and are deliberately not compared.
    let mut answers = Vec::new();
    for (what, extent, reversed) in [
        ("blind", ExtrudeExtent::blind(10.0).expect("valid"), false),
        ("reversed", ExtrudeExtent::blind(10.0).expect("valid"), true),
        (
            "symmetric",
            ExtrudeExtent::symmetric(10.0).expect("valid"),
            false,
        ),
        (
            "reversed-symmetric",
            ExtrudeExtent::symmetric(10.0).expect("valid"),
            true,
        ),
    ] {
        let request = ExtrudeRequest::new(profile.clone(), extent, reversed);
        let mut built = build(&request);
        let mut named = BTreeSet::new();
        for index in 0..labels.len() {
            let joint = joint_at(&labels, index);
            let edge = sweep_edge_of(&built, joint)
                .unwrap_or_else(|error| panic!("{what}: {joint} names no edge: {error}"));
            assert_eq!(edge.shape(), built.shape);
            named.insert(joint);
        }
        built.kernel.release(built.shape);
        answers.push((what, named));
    }

    let (_, first) = &answers[0];
    assert_eq!(first.len(), 4);
    for (what, named) in &answers {
        assert_eq!(named, first, "{what} named a different set of joints");
    }
}

#[test]
fn a_joint_between_two_lines_is_not_confused_with_one_against_the_arc() {
    kernel_or_skip!();

    let (request, labels) = arc_profile().expect("a valid profile");
    let mut built = build(&request);

    // labels[0] is the arc. The joint of the two lines is the only one that
    // touches neither end of it.
    let between_lines = ProfileJoint::new(labels[1], labels[2]).expect("two segments");
    let against_arc = [
        ProfileJoint::new(labels[0], labels[1]).expect("two segments"),
        ProfileJoint::new(labels[2], labels[0]).expect("two segments"),
    ];

    let straight = sweep_edge_of(&built, between_lines).expect("named");
    let curved: Vec<SubShapeHandle> = against_arc
        .iter()
        .map(|joint| sweep_edge_of(&built, *joint).expect("named"))
        .collect();
    assert!(!curved.contains(&straight), "two joints named one edge");
    assert_ne!(curved[0], curved[1]);

    // And the geometry agrees about which is which: the corner between the two
    // straight segments is the one whose two faces are both planar, so it is
    // the only sweep edge of this profile bounded by no curved face. Asked
    // through the drawn segment counts, which the arc makes plainly different.
    let drawn = drawn_segment_counts(&mut built);
    let arc_faces: BTreeSet<SubShapeHandle> = naming::resolve(
        &built.map,
        built.feature,
        &side_name(&built.map, built.feature, labels[0]),
    )
    .expect("the arc raised a face")
    .into_iter()
    .collect();
    let bounding = edge_faces(&mut built);
    assert!(
        bounding
            .get(&straight)
            .expect("drawn")
            .is_disjoint(&arc_faces),
        "the joint of the two lines was given an edge of the curved face"
    );
    for edge in &curved {
        assert!(
            !bounding.get(edge).expect("drawn").is_disjoint(&arc_faces),
            "a joint against the arc was given an edge that never touches it"
        );
    }
    assert!(drawn.values().all(|count| *count >= 1));

    built.kernel.release(built.shape);
}

#[test]
fn a_pair_that_meets_nowhere_and_a_joint_of_another_feature_each_name_nothing() {
    kernel_or_skip!();

    let (request, labels) = plate(10.0).expect("a valid plate");
    let mut built = build(&request);

    // Opposite sides of a rectangle share no corner.
    let apart = ProfileJoint::new(labels[0], labels[2]).expect("two segments");
    let apart_refusal = sweep_edge_of(&built, apart).expect_err("no corner is theirs");
    assert_eq!(apart_refusal.kind(), ErrorKind::Topology);
    assert!(
        apart_refusal.to_string().contains("has 0 descendants"),
        "{apart_refusal}"
    );

    // A joint of segments this feature never swept.
    let foreign = ProfileJoint::new(StableEntityId::new(), StableEntityId::new()).expect("two");
    let foreign_refusal = sweep_edge_of(&built, foreign).expect_err("not this profile's corner");
    assert_eq!(foreign_refusal.kind(), ErrorKind::Topology);
    assert!(
        foreign_refusal.to_string().contains("has 0 descendants"),
        "{foreign_refusal}"
    );

    // A reference to a feature that produced nothing at all.
    let elsewhere = sweep_reference(ObjectId::new(), joint_at(&labels, 1));
    let absent = resolve_sweep(&built.map, &elsewhere).expect_err("another feature");
    assert_eq!(absent.kind(), ErrorKind::Input);
    assert!(absent.to_string().contains("naming root has no plane"));

    // Three refusals, three different reasons, each naming what it refused.
    let reasons = BTreeSet::from([
        apart_refusal.to_string(),
        foreign_refusal.to_string(),
        absent.to_string(),
    ]);
    assert_eq!(reasons.len(), 3, "two refusals read the same");

    built.kernel.release(built.shape);
}

#[test]
fn a_sweep_edge_digest_cannot_resolve_as_a_face() {
    kernel_or_skip!();
    let (request, labels) = plate(10.0).expect("a valid plate");
    let mut built = build(&request);
    let edge = sweep_edge_of(&built, joint_at(&labels, 1)).expect("edge");
    let named = built.map.feature(built.feature).expect("feature");
    let mut name = named.entities[&edge];
    name.kind = SubShapeKind::Face;
    assert_eq!(
        named.resolve(name).expect_err("wrong kind").kind(),
        ErrorKind::Topology
    );
    built.kernel.release(built.shape);
}

#[test]
fn a_profile_whose_two_segments_meet_at_two_corners_names_neither() {
    kernel_or_skip!();

    // A half disc: the arc and the chord meet twice, so the pair of them
    // names two different corners. There is no honest way to say which is
    // meant, and both are therefore left unnamed rather than one chosen.
    let arc_label = StableEntityId::new();
    let chord_label = StableEntityId::new();
    let request = ExtrudeRequest::new(
        Profile::new(
            SketchPlane::world_xy(),
            ProfileLoop::new(vec![
                ProfileSegment::new(
                    arc_label,
                    SegmentGeometry::arc(PlanarPoint::ORIGIN, 10.0, 0.0, PI).expect("valid"),
                ),
                ProfileSegment::new(
                    chord_label,
                    SegmentGeometry::line(
                        PlanarPoint::new(-10.0, 0.0).expect("valid"),
                        PlanarPoint::new(10.0, 0.0).expect("valid"),
                    )
                    .expect("valid"),
                ),
            ])
            .expect("a closed loop"),
            Vec::new(),
        )
        .expect("a valid profile"),
        ExtrudeExtent::blind(5.0).expect("valid"),
        false,
    );
    let mut built = build(&request);

    let ambiguous = ProfileJoint::new(arc_label, chord_label).expect("two segments");
    let refusal = sweep_edge_of(&built, ambiguous).expect_err("this pair names two corners");
    assert_eq!(refusal.kind(), ErrorKind::Topology);
    assert!(
        refusal.to_string().contains("has 2 descendants"),
        "{refusal}"
    );

    // And the cap edges of the same shape are still named, so the refusal is
    // about this pair and not about the shape being unnameable.
    for side in [false, true] {
        let resolved = naming::resolve(
            &built.map,
            built.feature,
            &cap_edge_name(&built.map, built.feature, side, arc_label),
        )
        .expect("the cap edges are named as before");
        assert_eq!(resolved.len(), 1);
    }

    built.kernel.release(built.shape);
}

#[test]
fn two_plates_of_the_same_shape_do_not_share_the_names_of_their_corners() {
    kernel_or_skip!();

    let (first, first_labels) = plate(10.0).expect("a valid plate");
    let (second, second_labels) = plate(10.0).expect("an identical plate");
    let mut one = build(&first);
    let mut other = build(&second);

    // Same geometry, different segments, so a joint of one names nothing in
    // the other even though a plate corner is there to be found.
    let joint = joint_at(&first_labels, 1);
    assert!(sweep_edge_of(&one, joint).is_ok());
    let refusal = resolve_sweep(&other.map, &sweep_reference(other.feature, joint))
        .expect_err("another plate's corner");
    assert_eq!(refusal.kind(), ErrorKind::Topology);

    // Nor does naming the right pair against the wrong producer work.
    let borrowed = sweep_reference(one.feature, joint_at(&second_labels, 1));
    assert!(resolve_sweep(&one.map, &borrowed).is_err());

    one.kernel.release(one.shape);
    other.kernel.release(other.shape);
}

#[test]
fn editing_a_part_of_the_profile_that_is_not_the_corner_leaves_its_name_alone() {
    kernel_or_skip!();

    // The committed plate, then a copy of it with one segment gone. The corner
    // between two segments that both survive keeps its name, and it names the
    // same edge of the new shape rather than a nearby one.
    let dir = tempfile::tempdir().expect("temp dir");
    let mut document = open_plate(dir.path()).expect("the fixture opens");
    let segments = segments_of(&document).expect("reads the sketch");
    let feature = extrusion_of(&document);

    // drop_segment removes the last curve and extends its predecessor, so the
    // corner between the first two segments is untouched by the edit.
    let kept = ProfileJoint::new(segments[0], segments[1]).expect("two segments");
    let reference = sweep_reference(feature, kept);

    let mut kernel = OcctKernel::new().expect("opens");
    let before = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("rebuilds the plate");
    let named_before = resolve_sweep(before.topology(), &reference).expect("the corner is named");
    assert_eq!(named_before.len(), 1);
    let kept_name = before.topology().feature(feature).expect("named").entities[&named_before[0]];
    before.release_all(&mut kernel);

    let gone = drop_segment(&mut document).expect("removes a segment");
    assert!(
        !kept.touches(gone),
        "this edit must not touch the corner under test"
    );

    let after = rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("a three-sided profile still rebuilds");
    let named_after = naming::resolve(after.topology(), feature, &kept_name)
        .expect("the surviving corner keeps its name");
    assert_eq!(named_after.len(), 1);
    assert_eq!(named_after[0].kind(), SubShapeKind::Edge);

    // And it is that corner rather than merely some edge: the two faces it
    // lies between are the ones raised from its own two segments. Resolving to
    // a neighbour would satisfy the count and nothing else.
    let extrusion = extrusion_of(&document);
    let shape = after.shape(extrusion).expect("the extrusion made a solid");
    let mut probe = Built {
        kernel,
        map: ModelNaming::new(),
        feature: extrusion,
        shape,
    };
    let bounding = edge_faces(&mut probe);
    let plane = document
        .objects()
        .expect("objects")
        .iter()
        .find_map(|o| match &o.payload {
            ObjectPayload::Sketch(s) => Some(s.plane),
            _ => None,
        })
        .expect("sketch plane");
    let mut expected = BTreeSet::new();
    for segment in kept.segments() {
        expected.extend(
            naming::resolve(
                after.topology(),
                extrusion,
                &naming::side_name(extrusion, plane, segment),
            )
            .expect("the swept faces are named"),
        );
    }
    assert_eq!(expected.len(), 2, "two segments raise two faces");
    assert_eq!(
        bounding.get(&named_after[0]).expect("the edge is drawn"),
        &expected,
        "the surviving corner was given another corner's edge"
    );
    let mut kernel = probe.kernel;

    // The corner that the removed segment was half of is gone, and says so
    // rather than borrowing a neighbour.
    let lost = sweep_reference(feature, ProfileJoint::new(gone, segments[0]).expect("two"));
    assert!(
        resolve_sweep(after.topology(), &lost).is_err(),
        "a lost corner must not resolve"
    );

    after.release_all(&mut kernel);
}

#[test]
fn a_named_corner_is_an_edge_the_tessellation_draws() {
    kernel_or_skip!();

    let (request, labels) = plate(10.0).expect("a valid plate");
    let mut built = build(&request);

    let mut named = Vec::new();
    for index in 0..labels.len() {
        named.push(sweep_edge_of(&built, joint_at(&labels, index)).expect("named"));
    }
    let drawn = drawn_edges(&mut built);
    for edge in &named {
        // Handle equality, not position: the drawn edge and the named one are
        // the same sub-shape or the name means nothing.
        assert!(drawn.contains(edge), "{edge} is named but never drawn");
    }

    built.kernel.release(built.shape);
}

/// The plate's profile segments, in the order its sketch stores them.
fn segments_of(document: &Document) -> Result<Vec<StableEntityId>> {
    let record = document
        .objects()?
        .into_iter()
        .find(|object| matches!(object.payload, ObjectPayload::Sketch(_)))
        .expect("the fixture has a sketch");
    let ObjectPayload::Sketch(sketch) = record.payload else {
        panic!("the sketch is not a sketch");
    };
    Ok(sketch.curves.iter().map(|curve| curve.id).collect())
}

/// The plate's extrusion.
fn extrusion_of(document: &Document) -> ObjectId {
    document
        .objects()
        .expect("reads objects")
        .into_iter()
        .find(|object| matches!(object.payload, ObjectPayload::Extrude(_)))
        .expect("the fixture has an extrusion")
        .id
}

/// Every topological edge the tessellation reports for the built shape.
fn drawn_edges(built: &mut Built) -> BTreeSet<SubShapeHandle> {
    let mesh = built
        .kernel
        .tessellate(
            built.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes");
    mesh.edges
        .as_ref()
        .expect("the association is there")
        .ranges
        .iter()
        .map(|range| range.edge)
        .collect()
}

/// How many segments each topological edge is drawn with.
fn drawn_segment_counts(built: &mut Built) -> BTreeMap<SubShapeHandle, u32> {
    let mesh = built
        .kernel
        .tessellate(
            built.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes");
    let mut counts = BTreeMap::new();
    for range in &mesh.edges.as_ref().expect("association").ranges {
        *counts.entry(range.edge).or_insert(0) += range.segment_count;
    }
    counts
}

/// Which faces each topological edge lies on, read from the mesh.
///
/// Every face carries its own copy of the vertices it uses, so a vertex index
/// belongs to exactly one face, and the faces an edge bounds are the faces its
/// segments' vertices belong to. One range of an edge spans both face-side
/// representations, which is why this counts vertices rather than testing a
/// range against one face. Nothing here compares coordinates: it is index
/// membership and handle equality throughout.
fn edge_faces(built: &mut Built) -> BTreeMap<SubShapeHandle, BTreeSet<SubShapeHandle>> {
    let mesh = built
        .kernel
        .tessellate(
            built.shape,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("meshes");

    let mut face_of_vertex: BTreeMap<u32, SubShapeHandle> = BTreeMap::new();
    for range in &mesh.faces {
        let first = range.first_index as usize;
        let last = first + range.index_count as usize;
        for vertex in &mesh.indices[first..last] {
            face_of_vertex.insert(*vertex, range.face);
        }
    }

    let edges = mesh.edges.as_ref().expect("association");
    let mut found: BTreeMap<SubShapeHandle, BTreeSet<SubShapeHandle>> = BTreeMap::new();
    for range in &edges.ranges {
        let first = range.first_segment as usize * 2;
        let last = first + range.segment_count as usize * 2;
        for vertex in &edges.segments[first..last] {
            let face = face_of_vertex
                .get(vertex)
                .expect("every drawn vertex belongs to a meshed face");
            found.entry(range.edge).or_default().insert(*face);
        }
    }
    found
}

#[test]
fn the_names_of_corners_survive_a_file_and_a_new_session_but_their_handles_do_not() {
    kernel_or_skip!();

    let (request, labels) = plate(10.0).expect("a valid plate");
    let feature = ObjectId::new();
    let joints: Vec<ProfileJoint> = (0..labels.len())
        .map(|index| joint_at(&labels, index))
        .collect();

    let (archived, before, names) = {
        let mut writer = OcctKernel::new().expect("opens");
        let result = writer
            .extrude(&request, &OperationContext::default())
            .expect("Open CASCADE builds the plate");
        let map = naming::bind(&mut writer, feature, request.profile(), &result)
            .expect("complete naming");

        let mut named = Vec::new();
        for joint in &joints {
            let found = resolve_sweep(&map, &sweep_reference(feature, *joint)).expect("named");
            assert_eq!(found.len(), 1);
            named.push(found[0]);
        }

        // What travels is a table of slots, not a handle. Nothing session-local
        // may appear in it, and its own Debug is where that would show.
        let archived = archive_feature(&mut writer, &map, feature).expect("archives");
        let written = format!("{archived:?}");
        for forbidden in ["SessionId", "ShapeHandle", "SubShapeHandle", "session"] {
            assert!(
                !written.contains(forbidden),
                "the archive mentions {forbidden}: {written}"
            );
        }
        let names: Vec<_> = named
            .iter()
            .map(|edge| map.feature(feature).expect("named").entities[edge])
            .collect();
        writer.release(result.shape);
        (archived, named, names)
    };

    // Encoded and read back, which is the form a cache entry actually takes.
    let bytes = archived.encode().expect("encodes");
    let mut reader = OcctKernel::new().expect("opens");
    let decoded = ferritecad_topology::ArchivedFeature::decode(&bytes, feature, reader.identity())
        .expect("decodes");
    assert_eq!(decoded, archived);

    let mut restored = naming::prepared(feature, request.profile(), false).expect("model roots");
    restore_feature(&mut reader, &decoded, &mut restored).expect("restores");
    let shape = restored
        .feature(feature)
        .and_then(|names| names.shape())
        .expect("the restore produced a shape");

    let mut after = Vec::new();
    for name in names {
        let found = naming::resolve(&restored, feature, &name)
            .unwrap_or_else(|error| panic!("{name:?} lost its name across the file: {error}"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind(), SubShapeKind::Edge);
        after.push(found[0]);
    }

    // Four names, four distinct edges, and every one of them belongs to the
    // session that restored it rather than the session that wrote it.
    let distinct: BTreeSet<SubShapeHandle> = after.iter().copied().collect();
    assert_eq!(distinct.len(), 4);
    for edge in &after {
        assert_eq!(edge.shape(), shape);
        assert!(
            !before.contains(edge),
            "a restored handle belongs to the new session"
        );
    }

    reader.release(shape);
    assert_eq!(reader.live_shape_count(), 0);
}

#[test]
fn an_extrusion_that_is_cancelled_leaves_no_shape_behind() {
    kernel_or_skip!();

    let mut kernel = OcctKernel::new().expect("opens");
    assert_eq!(kernel.live_shape_count(), 0);

    let (request, labels) = plate(10.0).expect("a valid plate");
    let cancel = CancelToken::new();
    cancel.cancel();
    let context = OperationContext::new(Tolerance::default()).with_cancel(cancel);

    let refusal = kernel
        .extrude(&request, &context)
        .expect_err("a cancelled sweep produces no solid");
    assert_eq!(refusal.kind(), ErrorKind::Cancellation);
    assert_eq!(
        kernel.live_shape_count(),
        0,
        "a cancelled extrusion left a shape behind"
    );

    // And the session is still usable afterwards, so nothing was left half
    // built: the same request now succeeds and names its four corners.
    let mut built = build(&request);
    for index in 0..labels.len() {
        assert!(sweep_edge_of(&built, joint_at(&labels, index)).is_ok());
    }
    built.kernel.release(built.shape);
    assert_eq!(built.kernel.live_shape_count(), 0);
}
