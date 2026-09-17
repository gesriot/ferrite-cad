// SPDX-License-Identifier: MIT
//! Centre/radius-only editing of the persisted pair of analytic Circles.
//!
//! The same document class the circle editor beside it demands — one XY plane,
//! one Sketch, one forward Blind Extrude/NewBody and its Body, checked once by
//! [`frame`] — differing only in what the Sketch holds: exactly two
//! unconstrained `Circle`, concentric, one inside the other. Both stay analytic
//! here as they do everywhere else; nothing in this module can turn either into
//! a polygon, mint a new identity, or exchange their roles.
//!
//! # Roles come from the radii
//!
//! Which circle bounds the part and which is the bore is read from the stored
//! radii, exactly as the evaluator reads it — never from the order the two
//! curves happen to sit in. A document whose curves were written the other way
//! round is the same drawing, and editing it names the same two UUIDs.
use ferritecad_types::{CadError, ObjectId, Result, StableEntityId};

use crate::{
    AnnularExtrusion, Document, ObjectPayload, ObjectRecord, Point2, SketchGeometry,
    circle_edit::analytic_circle, sketch_edit::frame,
};

/// The two saved circles a copy edit can change, exactly as stored.
///
/// Both centres are reported rather than one. Concentric means "within the
/// kernel's linear tolerance", not "bit-identical", so a document may store two
/// centres that differ in the last places; saying so is honest, and it is also
/// the one fact that explains why an accepted edit leaves them exactly equal.
///
/// The height is reported because it decides what the numbers mean, not because
/// this edit can change it: a request carries no height, and the saved one is
/// what the new centre and radii are validated against.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedAnnulus {
    pub outer_curve_id: StableEntityId,
    pub inner_curve_id: StableEntityId,
    /// The stored centre of the bounding circle.
    pub center_mm: [f64; 2],
    /// The stored centre of the bore, within `CONCENTRIC_MM` of `center_mm`.
    pub inner_center_mm: [f64; 2],
    pub outer_radius_mm: f64,
    pub inner_radius_mm: f64,
    pub height_mm: f64,
}

/// A row in objects() order. Refusal is local; copy_access still takes priority.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnulusChoice {
    pub sketch: ObjectId,
    pub name: Option<String>,
    /// None for an unsupported Sketch, never an invented pair of circles.
    pub annulus: Option<SavedAnnulus>,
    pub refusal: Option<String>,
}

/// What one edit asks for: the two circles it names, and where they should be.
///
/// Both UUIDs are part of the request rather than inferred, and each is named
/// in the role it must already hold. An edit prepared against one reading can
/// therefore never be applied to the other circle of some other document, and
/// a request that swaps the two roles is refused rather than silently reading
/// the radii back the other way round.
///
/// One centre, not two: the class this edit serves is concentric, so a pair of
/// centres would be a request that could contradict itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnnulusEdit {
    pub outer_curve_id: StableEntityId,
    pub inner_curve_id: StableEntityId,
    pub center_mm: [f64; 2],
    pub outer_radius_mm: f64,
    pub inner_radius_mm: f64,
}

pub fn annulus_choices(document: &Document, objects: &[ObjectRecord]) -> Vec<AnnulusChoice> {
    objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .map(|o| match supported(document, objects, o) {
            Ok(annulus) => AnnulusChoice {
                sketch: o.id,
                name: o.name.clone(),
                annulus: Some(annulus),
                refusal: None,
            },
            Err(error) => AnnulusChoice {
                sketch: o.id,
                name: o.name.clone(),
                annulus: None,
                refusal: Some(error.to_string()),
            },
        })
        .collect()
}

fn unsupported(message: &str) -> CadError {
    CadError::unsupported(message)
}

pub(crate) fn supported(
    document: &Document,
    objects: &[ObjectRecord],
    object: &ObjectRecord,
) -> Result<SavedAnnulus> {
    let (sketch, height) = frame(document, objects, object)?;
    // Circle constraints are not part of *this* editor, so a constrained pair
    // is refused rather than edited around. The constraint editor manages that
    // case through the same reading below; see [`saved_pair`].
    if !sketch.constraints.is_empty() || sketch.curves.len() != 2 {
        return Err(unsupported(
            "annulus edit requires exactly two unconstrained Circles",
        ));
    }
    saved_pair(sketch, height)
}

/// The saved annular profile a sketch holds, judged by the one numeric policy.
///
/// Shared with the constraint editor, which manages the same two circles and
/// must read the same roles out of them. Only the reading is shared: whether a
/// Sketch may carry constraints is each editor's own question, asked before
/// this one.
pub(crate) fn saved_pair(sketch: &crate::Sketch, height: f64) -> Result<SavedAnnulus> {
    let (outer, inner) = pair_in_roles(sketch)?;
    // What is already stored has to be inside the policy an edit is judged by,
    // or the first accepted edit would silently narrow the document.
    AnnularExtrusion::new([outer.1.x, outer.1.y], outer.2, inner.2, height)
        .map_err(|e| unsupported(&format!("saved annulus is outside edit policy: {e}")))?;
    Ok(SavedAnnulus {
        outer_curve_id: outer.0,
        inner_curve_id: inner.0,
        center_mm: [outer.1.x, outer.1.y],
        inner_center_mm: [inner.1.x, inner.1.y],
        outer_radius_mm: outer.2,
        inner_radius_mm: inner.2,
        height_mm: height,
    })
}

/// The two analytic circles of an annular profile, each in the role it holds.
///
/// The one place a stored pair is read, so the annulus editor, the constraint
/// editor, the catalogue and the writer's re-derivation cannot disagree about
/// which circle is which.
pub(crate) fn pair_in_roles(sketch: &crate::Sketch) -> Result<(ReadCircle, ReadCircle)> {
    let [first, second] = sketch.curves.as_slice() else {
        return Err(unsupported("an annular profile is exactly two Circles"));
    };
    let first = analytic_circle(first)?;
    let second = analytic_circle(second)?;
    // Two rows that answer to one UUID would make the request ambiguous, and
    // no honest reading could say which curve an edit meant.
    if first.0 == second.0 {
        return Err(unsupported(
            "annulus edit requires two distinct Circle UUIDs",
        ));
    }
    let (outer, inner) = roles(first, second);
    if !AnnularExtrusion::concentric(outer.1, inner.1) {
        return Err(unsupported(
            "annulus edit requires the two Circles to share a centre",
        ));
    }
    Ok((outer, inner))
}

/// Which of two read circles bounds the region, by radius alone.
///
/// The one place the roles are decided, so the catalogue, the request check and
/// the writer's re-derivation cannot disagree about them. Equal radii are left
/// to the numeric policy, which refuses a hole that is not inside its boundary.
pub(crate) type ReadCircle = (StableEntityId, Point2, f64);
pub(crate) fn roles(first: ReadCircle, second: ReadCircle) -> (ReadCircle, ReadCircle) {
    if first.2 >= second.2 {
        (first, second)
    } else {
        (second, first)
    }
}

/// Prepare only the selected payload. Both Sketch and Circle IDs are retained,
/// and so is the order the two curves are stored in.
pub fn replace_annulus_geometry(
    document: &Document,
    selected: ObjectId,
    edit: &AnnulusEdit,
) -> Result<ObjectRecord> {
    let objects = document.objects()?;
    let mut object = objects
        .iter()
        .find(|o| o.id == selected)
        .cloned()
        .ok_or_else(|| CadError::input("selected Sketch UUID does not exist"))?;
    let saved = supported(document, &objects, &object)?;
    let checked = validate(&saved, edit)?;
    let ObjectPayload::Sketch(sketch) = &mut object.payload else {
        unreachable!("checked Sketch")
    };
    // Each curve is found by its own UUID, never by position: the stored order
    // is presentation order, and writing the bore's radius into whichever row
    // happens to come first is exactly the mistake the roles exist to prevent.
    for (curve_id, radius) in [
        (edit.outer_curve_id, checked.outer_radius_mm()),
        (edit.inner_curve_id, checked.inner_radius_mm()),
    ] {
        let curve = sketch
            .curves
            .iter_mut()
            .find(|c| c.id == curve_id)
            .ok_or_else(|| CadError::input("selected Circle UUID does not exist"))?;
        curve.geometry = SketchGeometry::Circle {
            center: checked.center(),
            radius,
        };
    }
    Ok(object)
}

impl AnnulusChoice {
    /// The same draft check used before copying. No SQLite or kernel work.
    pub fn validate_annulus(&self, edit: &AnnulusEdit) -> Result<AnnularExtrusion> {
        let saved = self
            .annulus
            .as_ref()
            .ok_or_else(|| unsupported("unsupported annulus choice"))?;
        validate(saved, edit)
    }
}

/// The one numeric policy, applied to the request against the saved height.
///
/// The height is the document's, not the request's: an edit that could change
/// it would be an extrusion edit wearing a profile's name, and there is already
/// a command for that.
fn validate(saved: &SavedAnnulus, edit: &AnnulusEdit) -> Result<AnnularExtrusion> {
    if edit.outer_curve_id == edit.inner_curve_id {
        return Err(CadError::input(
            "request must name two different Circle UUIDs",
        ));
    }
    if edit.outer_curve_id != saved.outer_curve_id || edit.inner_curve_id != saved.inner_curve_id {
        return Err(CadError::input(
            "request must name the saved bounding and bore Circle UUIDs in those roles",
        ));
    }
    AnnularExtrusion::new(
        edit.center_mm,
        edit.outer_radius_mm,
        edit.inner_radius_mm,
        saved.height_mm,
    )
}

/// The stored pair of a prepared payload, for a writer that must not trust its
/// caller about which rows it is replacing.
///
/// The roles are read back out of the prepared numbers by the same rule the
/// catalogue used, so a payload whose radii were exchanged describes a request
/// naming the two UUIDs the other way round — which the check against the saved
/// document then refuses.
pub(crate) fn prepared_annulus(object: &ObjectRecord) -> Result<AnnulusEdit> {
    let ObjectPayload::Sketch(sketch) = &object.payload else {
        return Err(CadError::input("annulus preparation must contain a Sketch"));
    };
    let [first, second] = sketch.curves.as_slice() else {
        return Err(CadError::input("annulus preparation must hold two curves"));
    };
    if !sketch.constraints.is_empty() {
        return Err(CadError::input("annulus editing stores no constraints"));
    }
    let read = |curve: &crate::SketchCurve| -> Result<ReadCircle> {
        if curve.construction {
            return Err(CadError::input(
                "annulus preparation must hold model Circles",
            ));
        }
        let SketchGeometry::Circle { center, radius } = curve.geometry else {
            return Err(CadError::input("annulus preparation must hold Circles"));
        };
        Ok((curve.id, center, radius))
    };
    let (outer, inner) = roles(read(first)?, read(second)?);
    // One centre is what a request carries, so a preparation whose two circles
    // sit at different points cannot have come from one.
    if outer.1 != inner.1 {
        return Err(CadError::input(
            "annulus preparation must place both Circles at one centre",
        ));
    }
    Ok(AnnulusEdit {
        outer_curve_id: outer.0,
        inner_curve_id: inner.0,
        center_mm: [outer.1.x, outer.1.y],
        outer_radius_mm: outer.2,
        inner_radius_mm: inner.2,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Body, DatumPlane, Dependency, DependencyRole, EndCondition, Expression, Extrude, Sketch,
        SketchCurve, SolidOperation,
    };
    use ferritecad_types::Transform;

    /// The smallest document this editor accepts, built the way creation does.
    ///
    /// `swapped` writes the bore first, which is a document creation never
    /// makes and the roles must still read correctly from.
    struct Fixture {
        _root: tempfile::TempDir,
        document: Document,
        sketch: ObjectId,
        outer: StableEntityId,
        inner: StableEntityId,
    }

    fn fixture(outer_radius: f64, inner_radius: f64, swapped: bool) -> Fixture {
        fixture_at(outer_radius, inner_radius, swapped, [12., -7.], [12., -7.])
    }

    fn fixture_at(
        outer_radius: f64,
        inner_radius: f64,
        swapped: bool,
        outer_center: [f64; 2],
        inner_center: [f64; 2],
    ) -> Fixture {
        let root = tempfile::tempdir().expect("dir");
        let mut d = Document::create(root.path().join("source.fcad")).expect("document");
        let [plane, sketch, extrude, body] = std::array::from_fn(|_| ObjectId::new());
        let outer = StableEntityId::new();
        let inner = StableEntityId::new();
        let circle = |id, center: [f64; 2], radius| -> Result<SketchCurve> {
            Ok(SketchCurve {
                id,
                construction: false,
                geometry: SketchGeometry::Circle {
                    center: Point2::new(center[0], center[1])?,
                    radius,
                },
            })
        };
        d.write(|w| {
            w.put_object(
                plane,
                None,
                0,
                Some("XY"),
                &ObjectPayload::DatumPlane(DatumPlane {
                    placement: Transform::IDENTITY,
                }),
            )?;
            let mut curves = vec![
                circle(outer, outer_center, outer_radius)?,
                circle(inner, inner_center, inner_radius)?,
            ];
            if swapped {
                curves.reverse();
            }
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
            w.put_object(
                extrude,
                None,
                2,
                Some("Extrude1"),
                &ObjectPayload::Extrude(Extrude {
                    profile: sketch,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(15.)?,
                    },
                    reversed: false,
                    operation: SolidOperation::NewBody,
                    target_body: None,
                }),
            )?;
            w.put_object(
                body,
                None,
                3,
                Some("Body"),
                &ObjectPayload::Body(Body {
                    tip_feature: Some(extrude),
                }),
            )?;
            for (dependent, dependency, role) in [
                (sketch, plane, DependencyRole::Plane),
                (extrude, sketch, DependencyRole::Profile),
                (body, extrude, DependencyRole::BodyTip),
            ] {
                w.add_dependency(Dependency {
                    dependent,
                    dependency,
                    role,
                })?;
            }
            Ok(())
        })
        .expect("fixture");
        Fixture {
            _root: root,
            document: d,
            sketch,
            outer,
            inner,
        }
    }

    fn ask(f: &Fixture, center: [f64; 2], outer: f64, inner: f64) -> AnnulusEdit {
        AnnulusEdit {
            outer_curve_id: f.outer,
            inner_curve_id: f.inner,
            center_mm: center,
            outer_radius_mm: outer,
            inner_radius_mm: inner,
        }
    }

    #[test]
    fn a_saved_pair_is_discovered_by_radius_in_either_stored_order() {
        for swapped in [false, true] {
            let f = fixture(10., 4., swapped);
            let objects = f.document.objects().expect("objects");
            let choices = annulus_choices(&f.document, &objects);
            assert_eq!(choices.len(), 1, "one Sketch, one row");
            assert_eq!(choices[0].sketch, f.sketch);
            assert_eq!(choices[0].refusal, None, "swapped={swapped}");
            assert_eq!(
                choices[0].annulus,
                Some(SavedAnnulus {
                    outer_curve_id: f.outer,
                    inner_curve_id: f.inner,
                    center_mm: [12., -7.],
                    inner_center_mm: [12., -7.],
                    outer_radius_mm: 10.,
                    inner_radius_mm: 4.,
                    height_mm: 15.,
                }),
                "the stored order decided a role (swapped={swapped})"
            );
            // The two editors beside this one refuse the same Sketch, as they
            // always did, and invent nothing about it.
            let lines = crate::sketch_choices(&f.document, &objects);
            assert!(lines[0].vertices.is_none() && lines[0].refusal.is_some());
            let circles = crate::circle_choices(&f.document, &objects);
            assert!(circles[0].circle.is_none());
            assert!(
                circles[0]
                    .refusal
                    .as_deref()
                    .is_some_and(|r| r.contains("exactly one unconstrained Circle")),
                "{:?}",
                circles[0].refusal
            );
        }
    }

    #[test]
    fn only_the_two_named_circles_and_only_their_geometry_is_rewritten() {
        for swapped in [false, true] {
            let f = fixture(10., 4., swapped);
            let before = f.document.object(f.sketch).expect("row").expect("record");
            let edit = ask(&f, [-3.5, 4.25], 6.75, 2.125);
            let prepared =
                replace_annulus_geometry(&f.document, f.sketch, &edit).expect("prepared");
            assert_eq!(prepared.id, before.id);
            assert_eq!(prepared.parent, before.parent);
            assert_eq!(prepared.ordinal, before.ordinal);
            assert_eq!(prepared.name, before.name);

            let ObjectPayload::Sketch(after) = &prepared.payload else {
                unreachable!("Sketch")
            };
            let ObjectPayload::Sketch(original) = &before.payload else {
                unreachable!("Sketch")
            };
            assert_eq!(after.plane, original.plane);
            assert!(after.constraints.is_empty());
            // Stored order and both identities survive, and each curve got its
            // own new radius rather than the row position's.
            assert_eq!(
                after.curves.iter().map(|c| c.id).collect::<Vec<_>>(),
                original.curves.iter().map(|c| c.id).collect::<Vec<_>>(),
                "stored order moved (swapped={swapped})"
            );
            for curve in &after.curves {
                assert!(!curve.construction);
                let SketchGeometry::Circle { center, radius } = curve.geometry else {
                    unreachable!("Circle")
                };
                assert_eq!((center.x, center.y), (-3.5, 4.25));
                let expected = if curve.id == f.outer { 6.75 } else { 2.125 };
                assert_eq!(radius, expected, "curve {} got {radius}", curve.id);
            }
            // And the preparation reads back as exactly the request it came
            // from, which is what the writer re-derives against.
            assert_eq!(prepared_annulus(&prepared).expect("pair"), edit);
        }
    }

    #[test]
    fn each_of_the_three_numbers_can_move_on_its_own() {
        let f = fixture(10., 4., false);
        for (what, edit) in [
            ("centre only", ask(&f, [-3.5, 4.25], 10., 4.)),
            ("outer only", ask(&f, [12., -7.], 6.75, 4.)),
            ("inner only", ask(&f, [12., -7.], 10., 2.125)),
        ] {
            let prepared = replace_annulus_geometry(&f.document, f.sketch, &edit).expect(what);
            assert_eq!(prepared_annulus(&prepared).expect("pair"), edit, "{what}");
        }
    }

    #[test]
    fn the_annulus_writer_refuses_forged_payloads_and_stale_preparations() {
        let f = fixture(10., 4., false);
        let mut d = f.document;
        let before = d.content_version().expect("version");
        let edit = ask(
            &Fixture {
                _root: tempfile::tempdir().expect("dir"),
                document: Document::open_read_only(d.path()).expect("reopen"),
                sketch: f.sketch,
                outer: f.outer,
                inner: f.inner,
            },
            [-3.5, 4.25],
            6.75,
            2.125,
        );
        let prepared = replace_annulus_geometry(&d, f.sketch, &edit).expect("prepare");

        for case in [
            "outer identity",
            "inner identity",
            "swapped radii",
            "plane",
            "construction",
            "radius",
            "third curve",
            "one curve",
            "constraints",
            "two centres",
        ] {
            let mut forged = prepared.clone();
            let ObjectPayload::Sketch(s) = &mut forged.payload else {
                unreachable!()
            };
            let outer_at = s
                .curves
                .iter()
                .position(|c| c.id == f.outer)
                .expect("outer row");
            let inner_at = 1 - outer_at;
            match case {
                "outer identity" => s.curves[outer_at].id = StableEntityId::new(),
                "inner identity" => s.curves[inner_at].id = StableEntityId::new(),
                // The geometry stays a valid annulus; only which curve owns
                // which radius is exchanged. Nothing about the payload looks
                // wrong on its own, and the roles are what catch it.
                "swapped radii" => {
                    let outer_geometry = s.curves[outer_at].geometry.clone();
                    s.curves[outer_at].geometry = s.curves[inner_at].geometry.clone();
                    s.curves[inner_at].geometry = outer_geometry;
                }
                "plane" => s.plane = ObjectId::new(),
                "construction" => s.curves[inner_at].construction = true,
                "radius" => {
                    s.curves[inner_at].geometry = SketchGeometry::Circle {
                        center: Point2::new(-3.5, 4.25).expect("finite"),
                        radius: -1.,
                    }
                }
                "third curve" => {
                    let extra = s.curves[inner_at].clone();
                    s.curves.push(SketchCurve {
                        id: StableEntityId::new(),
                        ..extra
                    });
                }
                "one curve" => {
                    s.curves.remove(inner_at);
                }
                "constraints" => {
                    s.constraints = vec![crate::SketchConstraint {
                        id: StableEntityId::new(),
                        rule: crate::SketchConstraintRule::Fixed {
                            point: crate::SketchPointRef::new(
                                s.curves[outer_at].id,
                                crate::SketchPointSelector::At,
                            ),
                            x: 0.,
                            y: 0.,
                        },
                    }]
                }
                // A hole that is no longer concentric with its boundary.
                "two centres" => {
                    s.curves[inner_at].geometry = SketchGeometry::Circle {
                        center: Point2::new(0., 0.).expect("finite"),
                        radius: 2.125,
                    }
                }
                _ => unreachable!(),
            }
            assert!(
                d.write_annulus_geometry(&forged).is_err(),
                "writer accepted forged {case}"
            );
            assert_eq!(d.content_version().expect("unchanged"), before, "{case}");
        }

        d.write_annulus_geometry(&prepared)
            .expect("valid geometry edit");
        let current = d.object(f.sketch).expect("row").expect("object");
        assert_eq!(current.payload, prepared.payload);
        let edited = d.content_version().expect("version");
        assert!(
            d.write_annulus_geometry(&prepared).is_err(),
            "stale preparation"
        );
        assert_eq!(d.content_version().expect("unchanged"), edited);
    }

    #[test]
    fn a_request_naming_the_wrong_curves_or_an_unstorable_number_is_refused() {
        let f = fixture(10., 4., false);
        let other = StableEntityId::new();
        for (edit, what) in [
            (
                AnnulusEdit {
                    outer_curve_id: other,
                    ..ask(&f, [0., 0.], 10., 4.)
                },
                "a foreign boundary UUID",
            ),
            (
                AnnulusEdit {
                    inner_curve_id: other,
                    ..ask(&f, [0., 0.], 10., 4.)
                },
                "a foreign bore UUID",
            ),
            (
                AnnulusEdit {
                    outer_curve_id: f.inner,
                    inner_curve_id: f.outer,
                    ..ask(&f, [0., 0.], 10., 4.)
                },
                "the two roles exchanged",
            ),
            (
                AnnulusEdit {
                    inner_curve_id: f.outer,
                    ..ask(&f, [0., 0.], 10., 4.)
                },
                "one UUID named twice",
            ),
            (ask(&f, [0., 0.], 10., 10.), "a hole as big as its boundary"),
            (ask(&f, [0., 0.], 10., 12.), "a hole bigger than it"),
            (ask(&f, [0., 0.], 10., 9.9999), "a wall below the minimum"),
            (ask(&f, [0., 0.], 10., 0.), "a bore of nothing"),
            (ask(&f, [0., 0.], 10., -1.), "a negative bore"),
            (ask(&f, [0., 0.], 0., 4.), "a boundary of nothing"),
            (
                ask(&f, [f64::NAN, 0.], 10., 4.),
                "a centre that is not a number",
            ),
            (
                ask(&f, [0., 0.], f64::INFINITY, 4.),
                "an unbounded boundary",
            ),
            (
                ask(&f, [0., 0.], 10., f64::NAN),
                "a bore that is not a number",
            ),
            (ask(&f, [2e6, 0.], 10., 4.), "a centre outside the range"),
            (ask(&f, [0., 0.], 2e6, 4.), "a boundary outside the range"),
        ] {
            assert!(
                replace_annulus_geometry(&f.document, f.sketch, &edit).is_err(),
                "{what} was accepted"
            );
        }
        // And a Sketch UUID that is not in this document.
        assert!(
            replace_annulus_geometry(&f.document, ObjectId::new(), &ask(&f, [0., 0.], 10., 4.))
                .is_err()
        );
    }

    #[test]
    fn a_sketch_this_editor_does_not_own_is_refused_rather_than_rewritten() {
        // A pair whose stored geometry is already outside the policy, and a
        // Sketch holding something other than two model circles, are each
        // refused with their own reason rather than edited around.
        for (outer, inner, expect) in [
            (10., 9.99995, "outside edit policy"),
            (10., 10., "outside edit policy"),
        ] {
            let f = fixture(outer, inner, false);
            let objects = f.document.objects().expect("objects");
            let error = supported(
                &f.document,
                &objects,
                &f.document.object(f.sketch).expect("row").expect("record"),
            )
            .expect_err("outside the class");
            assert!(error.to_string().contains(expect), "{error} != {expect}");
            assert!(annulus_choices(&f.document, &objects)[0].annulus.is_none());
        }
        // Two circles that do not share a centre are a class this slice does
        // not edit, and it says which fact it refused on.
        let f = fixture_at(10., 4., false, [12., -7.], [12.5, -7.]);
        let objects = f.document.objects().expect("objects");
        let error = supported(
            &f.document,
            &objects,
            &f.document.object(f.sketch).expect("row").expect("record"),
        )
        .expect_err("not concentric");
        assert!(error.to_string().contains("share a centre"), "{error}");

        // Within the kernel's own tolerance the two are one point, so the same
        // document with a nudge far below it is still editable.
        let nudge = AnnularExtrusion::CONCENTRIC_MM / 4.;
        let f = fixture_at(10., 4., false, [12., -7.], [12. + nudge, -7.]);
        let objects = f.document.objects().expect("objects");
        let saved = supported(
            &f.document,
            &objects,
            &f.document.object(f.sketch).expect("row").expect("record"),
        )
        .expect("one point as far as the kernel is concerned");
        assert_ne!(
            saved.center_mm, saved.inner_center_mm,
            "both stored centres are reported, not one of them twice"
        );
    }

    #[test]
    fn a_sketch_with_the_wrong_number_or_kind_of_curve_is_refused() {
        let f = fixture(10., 4., false);
        let mut d = f.document;
        let circle = |radius| SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Circle {
                center: Point2::new(12., -7.).expect("finite"),
                radius,
            },
        };
        for (curves, expect) in [
            (vec![circle(10.)], "exactly two"),
            (vec![circle(10.), circle(6.), circle(2.)], "exactly two"),
            (
                vec![
                    circle(10.),
                    SketchCurve {
                        geometry: SketchGeometry::Line {
                            start: Point2::ORIGIN,
                            end: Point2::new(4., 0.).expect("finite"),
                        },
                        ..circle(4.)
                    },
                ],
                "only a Circle",
            ),
            (
                vec![
                    circle(10.),
                    SketchCurve {
                        construction: true,
                        ..circle(4.)
                    },
                ],
                "construction",
            ),
        ] {
            let mut object = d.object(f.sketch).expect("row").expect("record");
            let ObjectPayload::Sketch(s) = &mut object.payload else {
                unreachable!("Sketch")
            };
            s.curves = curves;
            d.write(|w| {
                w.put_object(
                    object.id,
                    object.parent,
                    object.ordinal,
                    object.name.as_deref(),
                    &object.payload,
                )
            })
            .expect("store");
            let objects = d.objects().expect("objects");
            let error = supported(
                &d,
                &objects,
                &d.object(f.sketch).expect("row").expect("record"),
            )
            .expect_err("outside the class");
            assert!(error.to_string().contains(expect), "{error} != {expect}");
            assert!(annulus_choices(&d, &objects)[0].annulus.is_none());
        }
    }
}
