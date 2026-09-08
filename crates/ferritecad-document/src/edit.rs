// SPDX-License-Identifier: MIT
//! The stored facts on which a bounded extrusion edit can be confirmed.

use std::{cell::LazyCell, collections::HashSet};

use ferritecad_types::{CadError, ContentHash, DocumentId, ObjectId, Result};

use crate::{Access, DependencyRole, Document, EndCondition, ObjectPayload, ObjectRecord};

/// Identity and complete content version of one consistent document reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentVersion {
    pub document_id: DocumentId,
    pub content: ContentHash,
}

/// One explicitly addressable native extrusion, including why it is unavailable.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtrudeChoice {
    pub feature: ObjectId,
    pub name: Option<String>,
    pub distance_mm: Option<f64>,
    pub refusal: Option<String>,
}

/// Facts carried alongside the accepted picture, never re-read by a form.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtrudeEditSource {
    pub version: DocumentVersion,
    pub features: Vec<ExtrudeChoice>,
    pub refusal: Option<String>,
}

impl ExtrudeEditSource {
    pub fn read(document: &Document) -> Result<Self> {
        Self::read_with_parameter_dependents(document, || parameter_dependents(document))
    }

    fn read_with_parameter_dependents(
        document: &Document,
        load: impl FnOnce() -> Result<HashSet<ObjectId>>,
    ) -> Result<Self> {
        let refusal = match document.copy_access()? {
            Access::ReadOnly { reason } => Some(reason),
            _ => None,
        };
        // Parameter membership is loaded at most once, and only if a Blind
        // literal still needs that check. Cache errors as well as the set:
        // unreadable dependencies must not cause another query per feature.
        let parameterized = LazyCell::new(load);
        let features = document
            .objects()?
            .iter()
            .filter_map(|object| {
                let ObjectPayload::Extrude(extrude) = &object.payload else {
                    return None;
                };
                let distance_mm = match &extrude.end_condition {
                    EndCondition::Blind { distance } | EndCondition::Symmetric { distance } => {
                        Some(distance.value())
                    }
                    _ => None,
                };
                Some(ExtrudeChoice {
                    feature: object.id,
                    name: object.name.clone(),
                    distance_mm,
                    refusal: blind_literal_distance(object)
                        .map_err(|e| e.to_string())
                        .and_then(|distance| {
                            let set = parameterized.as_ref().map_err(|e| e.to_string())?;
                            without_parameter_dependency(distance, set.contains(&object.id))
                                .map_err(|e| e.to_string())
                        })
                        .err(),
                })
            })
            .collect();
        Ok(Self {
            version: DocumentVersion {
                document_id: document.meta().document_id,
                content: document.content_version()?,
            },
            features,
            refusal,
        })
    }

    pub fn unavailable_reason(&self) -> Option<&str> {
        self.refusal.as_deref().or_else(|| {
            if self.features.is_empty() {
                Some("This document has no native extrusions.")
            } else if self.features.iter().all(|f| f.refusal.is_some()) {
                Some("No supported extrusion: a constant Blind distance is required.")
            } else {
                None
            }
        })
    }
}

/// Only a matching numeric source and stored value is a literal. No evaluator
/// is implied: expressions currently store source text and their last value.
/// A parameter edge is refused even if the text happens to look numeric.
pub fn editable_extrude(document: &Document, object: &ObjectRecord) -> Result<f64> {
    let distance = blind_literal_distance(object)?;
    // One selected feature needs only the original scan, not a new index.
    let parameterized = document
        .dependencies()?
        .iter()
        .any(|dep| dep.dependent == object.id && dep.role == DependencyRole::Parameter);
    without_parameter_dependency(distance, parameterized)
}

fn parameter_dependents(document: &Document) -> Result<HashSet<ObjectId>> {
    Ok(document
        .dependencies()?
        .into_iter()
        .filter(|dep| dep.role == DependencyRole::Parameter)
        .map(|dep| dep.dependent)
        .collect())
}

fn blind_literal_distance(object: &ObjectRecord) -> Result<f64> {
    let ObjectPayload::Extrude(extrude) = &object.payload else {
        return Err(CadError::unsupported(format!(
            "object {} is {}, not a native extrusion",
            object.id,
            object.payload.type_name()
        )));
    };
    let EndCondition::Blind { distance } = &extrude.end_condition else {
        return Err(CadError::unsupported(
            "only Blind extrusions can be edited; Symmetric and ThroughAll are unsupported",
        ));
    };
    let literal = distance.source.trim().parse::<f64>().ok();
    if !literal.is_some_and(|v| v.is_finite() && v == distance.value()) {
        return Err(formula_refusal());
    }
    Ok(distance.value())
}

fn without_parameter_dependency(distance: f64, parameterized: bool) -> Result<f64> {
    if parameterized {
        Err(formula_refusal())
    } else {
        Ok(distance)
    }
}

fn formula_refusal() -> CadError {
    CadError::unsupported(
        "extrusion distance is a formula or parameter dependency; only a numeric literal can be edited",
    )
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::{
        Body, DatumPlane, Dependency, Expression, Extrude, Parameter, Point2, Sketch, SketchCurve,
        SketchGeometry, SolidOperation,
    };
    use ferritecad_types::{Dimension, Transform};
    use rusqlite::params;

    const FORMULA: &str = "unsupported: extrusion distance is a formula or parameter dependency; only a numeric literal can be edited";
    const NOT_BLIND: &str = "unsupported: only Blind extrusions can be edited; Symmetric and ThroughAll are unsupported";

    struct MixedCatalog {
        _dir: tempfile::TempDir,
        document: Document,
        plane: ObjectId,
        sketch: ObjectId,
        height: ObjectId,
        other_body: ObjectId,
        first: ObjectId,
        parameterized: ObjectId,
        symmetric: ObjectId,
        also: ObjectId,
        through: ObjectId,
        formula: ObjectId,
    }

    fn blind(profile: ObjectId, source: &str, value: f64) -> ObjectPayload {
        ObjectPayload::Extrude(Extrude {
            profile,
            end_condition: EndCondition::Blind {
                distance: Expression::new(source, value).expect("distance"),
            },
            reversed: false,
            operation: SolidOperation::NewBody,
            target_body: None,
        })
    }

    fn mixed_catalog() -> MixedCatalog {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut document = Document::create(dir.path().join("mixed.fcad")).expect("create");
        let plane = ObjectId::new();
        let sketch = ObjectId::new();
        let height = ObjectId::new();
        let other_body = ObjectId::new();
        let first = ObjectId::new();
        let parameterized = ObjectId::new();
        let symmetric = ObjectId::new();
        let also = ObjectId::new();
        let through = ObjectId::new();
        let formula = ObjectId::new();
        document
            .write(|w| {
                w.put_object(
                    plane,
                    None,
                    0,
                    Some("XY"),
                    &ObjectPayload::DatumPlane(DatumPlane {
                        placement: Transform::IDENTITY,
                    }),
                )?;
                w.put_object(first, None, 1, Some("First"), &blind(sketch, "8", 8.0))?;
                w.put_object(
                    sketch,
                    None,
                    2,
                    Some("Profile"),
                    &ObjectPayload::Sketch(Sketch {
                        plane,
                        curves: vec![SketchCurve {
                            id: ferritecad_types::StableEntityId::new(),
                            construction: false,
                            geometry: SketchGeometry::Line {
                                start: Point2::ORIGIN,
                                end: Point2::new(20.0, 0.0)?,
                            },
                        }],
                        constraints: Vec::new(),
                    }),
                )?;
                w.put_object(
                    parameterized,
                    None,
                    3,
                    Some("Param"),
                    &blind(sketch, "11", 11.0),
                )?;
                w.put_object(
                    height,
                    None,
                    4,
                    Some("Height"),
                    &ObjectPayload::Parameter(Parameter {
                        name: "height".into(),
                        dimension: Dimension::Length,
                        expression: Expression::constant(11.0)?,
                    }),
                )?;
                w.put_object(
                    symmetric,
                    None,
                    5,
                    Some("Sym"),
                    &ObjectPayload::Extrude(Extrude {
                        profile: sketch,
                        end_condition: EndCondition::Symmetric {
                            distance: Expression::constant(9.0)?,
                        },
                        reversed: false,
                        operation: SolidOperation::NewBody,
                        target_body: None,
                    }),
                )?;
                w.put_object(
                    other_body,
                    None,
                    6,
                    Some("Other"),
                    &ObjectPayload::Body(Body {
                        tip_feature: Some(first),
                    }),
                )?;
                w.put_object(also, None, 7, Some("Also"), &blind(sketch, " 11 ", 11.0))?;
                w.put_object(
                    through,
                    None,
                    8,
                    Some("Thru"),
                    &ObjectPayload::Extrude(Extrude {
                        profile: sketch,
                        end_condition: EndCondition::ThroughAll,
                        reversed: false,
                        operation: SolidOperation::NewBody,
                        target_body: None,
                    }),
                )?;
                w.put_object(
                    formula,
                    None,
                    9,
                    Some("Expr"),
                    &blind(sketch, "height * 2", 15.0),
                )?;
                w.add_dependency(Dependency {
                    dependent: sketch,
                    dependency: plane,
                    role: DependencyRole::Plane,
                })?;
                for extrude in [first, parameterized, symmetric, also, through, formula] {
                    w.add_dependency(Dependency {
                        dependent: extrude,
                        dependency: sketch,
                        role: DependencyRole::Profile,
                    })?;
                }
                w.add_dependency(Dependency {
                    dependent: parameterized,
                    dependency: height,
                    role: DependencyRole::Parameter,
                })?;
                w.add_dependency(Dependency {
                    dependent: other_body,
                    dependency: height,
                    role: DependencyRole::Parameter,
                })?;
                w.add_dependency(Dependency {
                    dependent: other_body,
                    dependency: first,
                    role: DependencyRole::BodyTip,
                })?;
                Ok(())
            })
            .expect("populate");
        MixedCatalog {
            _dir: dir,
            document,
            plane,
            sketch,
            height,
            other_body,
            first,
            parameterized,
            symmetric,
            also,
            through,
            formula,
        }
    }

    fn choice(reading: &ExtrudeEditSource, id: ObjectId) -> &ExtrudeChoice {
        reading
            .features
            .iter()
            .find(|feature| feature.feature == id)
            .expect("catalog row")
    }

    #[test]
    fn mixed_catalog_matches_public_editable_check_and_preserves_order() {
        let mixed = mixed_catalog();
        let mut reads = 0;
        let reading = ExtrudeEditSource::read_with_parameter_dependents(&mixed.document, || {
            reads += 1;
            parameter_dependents(&mixed.document)
        })
        .expect("catalog");
        assert_eq!(reads, 1, "one SQL reading for all eligible features");
        assert_eq!(reading.refusal, None);
        assert_eq!(reading.unavailable_reason(), None);
        assert_eq!(
            reading
                .features
                .iter()
                .map(|feature| (
                    feature.feature,
                    feature.name.as_deref(),
                    feature.distance_mm,
                    feature.refusal.as_deref()
                ))
                .collect::<Vec<_>>(),
            vec![
                (mixed.first, Some("First"), Some(8.0), None),
                (
                    mixed.parameterized,
                    Some("Param"),
                    Some(11.0),
                    Some(FORMULA)
                ),
                (mixed.symmetric, Some("Sym"), Some(9.0), Some(NOT_BLIND)),
                (mixed.also, Some("Also"), Some(11.0), None),
                (mixed.through, Some("Thru"), None, Some(NOT_BLIND)),
                (mixed.formula, Some("Expr"), Some(15.0), Some(FORMULA)),
            ]
        );
        assert_eq!(
            editable_extrude(
                &mixed.document,
                &mixed
                    .document
                    .object(mixed.first)
                    .expect("read")
                    .expect("first")
            )
            .expect("literal"),
            8.0
        );
        assert_eq!(
            editable_extrude(
                &mixed.document,
                &mixed
                    .document
                    .object(mixed.also)
                    .expect("read")
                    .expect("also")
            )
            .expect("trimmed literal"),
            11.0
        );
        for object in mixed.document.objects().expect("objects") {
            let classified = editable_extrude(&mixed.document, &object);
            match &object.payload {
                ObjectPayload::Extrude(_) => {
                    let row = choice(&reading, object.id);
                    match classified {
                        Ok(distance) => {
                            assert_eq!(row.refusal, None, "{}", object.id);
                            assert_eq!(row.distance_mm, Some(distance));
                        }
                        Err(error) => {
                            assert_eq!(row.refusal.as_deref(), Some(error.to_string().as_str()));
                        }
                    }
                }
                _ => {
                    assert!(
                        reading
                            .features
                            .iter()
                            .all(|feature| feature.feature != object.id),
                        "{} leaked into the extrusion catalog",
                        object.id
                    );
                    let error = classified.expect_err("non-extrude");
                    assert_eq!(
                        error.to_string(),
                        format!(
                            "unsupported: object {} is {}, not a native extrusion",
                            object.id,
                            object.payload.type_name()
                        )
                    );
                }
            }
        }
        assert_eq!(
            mixed.plane,
            mixed.document.objects().expect("objects")[0].id
        );
        assert_eq!(
            mixed.sketch,
            mixed.document.objects().expect("objects")[2].id
        );
        assert_eq!(
            mixed.height,
            mixed.document.objects().expect("objects")[4].id
        );
        assert_eq!(
            mixed.other_body,
            mixed.document.objects().expect("objects")[6].id
        );
    }

    #[test]
    fn empty_and_all_unsupported_catalogs_keep_their_explanations() {
        let dir = tempfile::tempdir().expect("temp dir");
        let empty = Document::create(dir.path().join("empty.fcad")).expect("empty");
        let empty_reading = ExtrudeEditSource::read_with_parameter_dependents(&empty, || {
            panic!("empty catalogs must not load the Parameter index")
        })
        .expect("empty catalog");
        assert!(empty_reading.features.is_empty());
        assert_eq!(empty_reading.refusal, None);
        assert_eq!(
            empty_reading.unavailable_reason(),
            Some("This document has no native extrusions.")
        );

        let mut unsupported =
            Document::create(dir.path().join("unsupported.fcad")).expect("create");
        let sketch = ObjectId::new();
        unsupported
            .write(|w| {
                w.put_object(
                    sketch,
                    None,
                    0,
                    Some("Profile"),
                    &ObjectPayload::Sketch(Sketch {
                        plane: ObjectId::new(),
                        curves: Vec::new(),
                        constraints: Vec::new(),
                    }),
                )?;
                w.put_object(
                    ObjectId::new(),
                    None,
                    1,
                    Some("Sym"),
                    &ObjectPayload::Extrude(Extrude {
                        profile: sketch,
                        end_condition: EndCondition::Symmetric {
                            distance: Expression::constant(9.0)?,
                        },
                        reversed: false,
                        operation: SolidOperation::NewBody,
                        target_body: None,
                    }),
                )?;
                for source in ["height * 2", "12", "NaN", "inf"] {
                    w.put_object(
                        ObjectId::new(),
                        None,
                        2,
                        Some("Expr"),
                        &blind(sketch, source, 15.0),
                    )?;
                }
                Ok(())
            })
            .expect("populate");
        let reading = ExtrudeEditSource::read_with_parameter_dependents(&unsupported, || {
            panic!("unsupported literals must not load the Parameter index")
        })
        .expect("unsupported catalog");
        assert_eq!(reading.features.len(), 5);
        assert!(
            reading
                .features
                .iter()
                .all(|feature| feature.refusal.is_some())
        );
        assert_eq!(
            reading.unavailable_reason(),
            Some("No supported extrusion: a constant Blind distance is required.")
        );
        assert_eq!(reading.features[0].refusal.as_deref(), Some(NOT_BLIND));
        assert!(
            reading.features[1..]
                .iter()
                .all(|f| f.refusal.as_deref() == Some(FORMULA))
        );
    }

    #[test]
    fn a_dependency_read_error_refuses_only_blind_literals_that_need_membership() {
        let mixed = mixed_catalog();
        let path = mixed.document.path().to_path_buf();
        mixed.document.close().expect("close");
        let connection = rusqlite::Connection::open(&path).expect("raw");
        connection
            .execute(
                "INSERT INTO deps (dependent_id, dependency_id, role) VALUES (?1, ?2, ?3)",
                params![
                    mixed.first.to_bytes().as_slice(),
                    mixed.height.to_bytes().as_slice(),
                    "bogus",
                ],
            )
            .expect("corrupt role");
        drop(connection);
        let document = Document::open(&path).expect("reopen");
        let mut reads = 0;
        let reading = ExtrudeEditSource::read_with_parameter_dependents(&document, || {
            reads += 1;
            parameter_dependents(&document)
        })
        .expect("catalog still builds");
        assert_eq!(
            reads, 1,
            "a failed SQL reading must not be retried per feature"
        );
        let invalid = document.dependencies().expect_err("bogus role").to_string();
        assert_eq!(
            choice(&reading, mixed.symmetric).refusal.as_deref(),
            Some(NOT_BLIND)
        );
        assert_eq!(
            choice(&reading, mixed.through).refusal.as_deref(),
            Some(NOT_BLIND)
        );
        assert_eq!(
            choice(&reading, mixed.formula).refusal.as_deref(),
            Some(FORMULA)
        );
        assert_eq!(
            choice(&reading, mixed.first).refusal.as_deref(),
            Some(invalid.as_str())
        );
        assert_eq!(
            choice(&reading, mixed.also).refusal.as_deref(),
            Some(invalid.as_str())
        );
        assert_eq!(
            choice(&reading, mixed.parameterized).refusal.as_deref(),
            Some(invalid.as_str())
        );
    }

    fn scaled_catalog(
        features: usize,
        extra_deps: usize,
    ) -> (tempfile::TempDir, Document, usize, u64) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("scaled.fcad");
        let mut document = Document::create(&path).expect("create");
        let plane = ObjectId::new();
        let sketch = ObjectId::new();
        let height = ObjectId::new();
        let extrudes: Vec<ObjectId> = (0..features).map(|_| ObjectId::new()).collect();
        document
            .write(|w| {
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
                        curves: [
                            (Point2::ORIGIN, Point2::new(20.0, 0.0)?),
                            (Point2::new(20.0, 0.0)?, Point2::new(20.0, 10.0)?),
                            (Point2::new(20.0, 10.0)?, Point2::new(0.0, 10.0)?),
                            (Point2::new(0.0, 10.0)?, Point2::ORIGIN),
                        ]
                        .into_iter()
                        .map(|(start, end)| SketchCurve {
                            id: ferritecad_types::StableEntityId::new(),
                            construction: false,
                            geometry: SketchGeometry::Line { start, end },
                        })
                        .collect(),
                        constraints: Vec::new(),
                    }),
                )?;
                w.put_object(
                    height,
                    None,
                    2,
                    Some("Height"),
                    &ObjectPayload::Parameter(Parameter {
                        name: "height".into(),
                        dimension: Dimension::Length,
                        expression: Expression::constant(10.0)?,
                    }),
                )?;
                w.add_dependency(Dependency {
                    dependent: sketch,
                    dependency: plane,
                    role: DependencyRole::Plane,
                })?;
                for (index, id) in extrudes.iter().enumerate() {
                    w.put_object(
                        *id,
                        None,
                        i64::try_from(index + 3).expect("ordinal"),
                        Some("Blind"),
                        &blind(sketch, "10", 10.0),
                    )?;
                    w.add_dependency(Dependency {
                        dependent: *id,
                        dependency: sketch,
                        role: DependencyRole::Profile,
                    })?;
                }
                if features != 0 {
                    w.add_dependency(Dependency {
                        dependent: extrudes[0],
                        dependency: height,
                        role: DependencyRole::Parameter,
                    })?;
                }
                for index in 0..extra_deps {
                    let filler = ObjectId::new();
                    w.put_object(
                        filler,
                        None,
                        i64::try_from(features + index + 3).expect("ordinal"),
                        None,
                        &ObjectPayload::Body(Body { tip_feature: None }),
                    )?;
                    w.add_dependency(Dependency {
                        dependent: filler,
                        dependency: height,
                        role: DependencyRole::Parameter,
                    })?;
                }
                Ok(())
            })
            .expect("populate");
        let report = document.validate().expect("validate benchmark fixture");
        assert!(report.is_ok(), "{report:?}");
        document.close().expect("close writer");
        let document =
            Document::open_read_only(&path).expect("one pinned reading for both implementations");
        let deps = document.dependencies().expect("deps").len();
        let bytes = std::fs::metadata(&path).expect("size").len();
        (dir, document, deps, bytes)
    }

    fn median_ns(samples: &mut [u128]) -> u128 {
        samples.sort_unstable();
        samples[samples.len() / 2]
    }

    fn time_ns(repeats: usize, mut run: impl FnMut()) -> u128 {
        for _ in 0..3 {
            run();
        }
        let mut samples = Vec::with_capacity(repeats);
        for _ in 0..repeats {
            let start = std::time::Instant::now();
            run();
            samples.push(start.elapsed().as_nanos());
        }
        median_ns(&mut samples)
    }

    // Reference catalog traversal from main@5b20719. The public single-feature
    // check still uses the original dependency scan; only the catalog indexes it.
    fn legacy_catalog_read(document: &Document) -> Result<ExtrudeEditSource> {
        let refusal = match document.copy_access()? {
            Access::ReadOnly { reason } => Some(reason),
            _ => None,
        };
        let features = document
            .objects()?
            .iter()
            .filter_map(|object| {
                let ObjectPayload::Extrude(extrude) = &object.payload else {
                    return None;
                };
                let distance_mm = match &extrude.end_condition {
                    EndCondition::Blind { distance } | EndCondition::Symmetric { distance } => {
                        Some(distance.value())
                    }
                    _ => None,
                };
                Some(ExtrudeChoice {
                    feature: object.id,
                    name: object.name.clone(),
                    distance_mm,
                    refusal: editable_extrude(document, object)
                        .err()
                        .map(|e| e.to_string()),
                })
            })
            .collect();
        Ok(ExtrudeEditSource {
            version: DocumentVersion {
                document_id: document.meta().document_id,
                content: document.content_version()?,
            },
            features,
            refusal,
        })
    }

    fn paired_catalog_ns(document: &Document, repeats: usize) -> (u128, u128) {
        let mut before = Vec::with_capacity(repeats);
        let mut after = Vec::with_capacity(repeats);
        for round in 0..(3 + repeats) {
            // Alternate order so one implementation is not always the warm one.
            for legacy in if round % 2 == 0 {
                [true, false]
            } else {
                [false, true]
            } {
                let start = std::time::Instant::now();
                let reading = if legacy {
                    legacy_catalog_read(document)
                } else {
                    ExtrudeEditSource::read(document)
                };
                std::hint::black_box(reading.expect("catalog"));
                let elapsed = start.elapsed().as_nanos();
                if round >= 3 {
                    if legacy {
                        before.push(elapsed);
                    } else {
                        after.push(elapsed);
                    }
                }
            }
        }
        (median_ns(&mut before), median_ns(&mut after))
    }

    #[test]
    #[ignore = "manual ExtrudeEditSource::read timing; not a CI assertion"]
    fn measure_extrude_catalog_read() {
        const REPEATS: usize = 21;
        println!(
            "arch={} repeats={REPEATS} warmup=3 debug_assertions={}",
            std::env::consts::ARCH,
            cfg!(debug_assertions)
        );
        println!(
            "F D objects bytes copy_access_ns objects_ns dependencies_ns content_version_ns before_ns after_ns"
        );
        for (features, extra_deps) in [(1, 0), (200, 200), (1000, 3000), (2000, 8000)] {
            let (_dir, document, deps, bytes) = scaled_catalog(features, extra_deps);
            let objects = document.objects().expect("objects").len();
            let copy_access = time_ns(REPEATS, || {
                document.copy_access().expect("access");
            });
            let objects_ns = time_ns(REPEATS, || {
                document.objects().expect("objects");
            });
            let dependencies_ns = time_ns(REPEATS, || {
                document.dependencies().expect("deps");
            });
            let content_version_ns = time_ns(REPEATS, || {
                document.content_version().expect("version");
            });
            let baseline = legacy_catalog_read(&document).expect("original catalog");
            let (before_ns, after_ns) = paired_catalog_ns(&document, REPEATS);
            let reading = ExtrudeEditSource::read(&document).expect("catalog");
            assert_eq!(reading.features.len(), features);
            assert_eq!(
                reading, baseline,
                "all fields and version on the same pinned reading"
            );
            println!(
                "{features} {deps} {objects} {bytes} {copy_access} {objects_ns} {dependencies_ns} {content_version_ns} {before_ns} {after_ns}"
            );
        }
    }
}
