// SPDX-License-Identifier: MIT
//! §29A: what `chamfer-edge-copy` and `edit-chamfer-distance` would accept,
//! from the same pinned reading `inspect --json` gives everything else.
//! Additive: nothing here changes a field, type or code of an older answer.
use ferritecad_types::ObjectId;
use serde::Serialize;

use super::FilletEdgeDto;

/// What `chamfer-edge-copy` would accept about one Body.
#[derive(Serialize)]
pub(super) struct ChamferDiscovery {
    available: bool,
    refusal: Option<String>,
    document_refusal: Option<String>,
    target: Option<ChamferTarget>,
}

#[derive(Serialize)]
struct ChamferTarget {
    body_id: ObjectId,
    /// The feature both consumed and cut: the plate's base Extrude.
    base_feature_id: ObjectId,
    profile_sketch_id: ObjectId,
    height_mm: f64,
    request_versions: &'static [u32],
    /// The unit of every distance below: millimetres, measured from the edge
    /// along **each** of the two adjacent faces — not the slanted flat's width,
    /// which is `distance_mm * sqrt(2)`.
    distance_unit: &'static str,
    min_distance_mm: f64,
    /// How much of each adjacent face a chamfer leaves, at least.
    min_flat_mm: f64,
    candidates: Vec<ChamferCandidate>,
}

/// One edge, by its meaning. `edge` is the identity a request repeats; the
/// rest are labels recomputed from the saved profile.
#[derive(Serialize)]
struct ChamferCandidate {
    edge: FilletEdgeDto,
    label: String,
    corner_mm: [f64; 2],
    adjacent_lengths_mm: [f64; 2],
    /// The largest distance this build accepts here (inclusive); below
    /// `min_distance_mm` when the corner's sides leave none, and then
    /// `offerable` is `false`.
    max_distance_mm: f64,
    offerable: bool,
}

impl ChamferDiscovery {
    pub(super) fn new(
        choice: &ferritecad_document::ChamferChoice,
        document_refusal: Option<String>,
    ) -> Self {
        let target = choice.target.as_ref().map(|t| ChamferTarget {
            body_id: t.body,
            base_feature_id: t.base_feature,
            profile_sketch_id: t.profile,
            height_mm: t.height_mm,
            request_versions: &[1],
            distance_unit: "mm",
            min_distance_mm: ferritecad_document::MIN_DISTANCE_MM,
            min_flat_mm: ferritecad_document::MIN_FLAT_MM,
            candidates: t
                .corners
                .iter()
                .map(|c| {
                    let [a, b] = c.joint.segments();
                    ChamferCandidate {
                        edge: FilletEdgeDto {
                            feature_id: c.feature,
                            joint: c.joint.segments(),
                        },
                        label: format!(
                            "vertical edge at ({}, {}) mm between Lines {a} and {b}",
                            c.corner_mm[0], c.corner_mm[1]
                        ),
                        corner_mm: c.corner_mm,
                        adjacent_lengths_mm: c.adjacent_lengths_mm,
                        max_distance_mm: c.max_distance_mm,
                        offerable: c.is_offerable(),
                    }
                })
                .collect(),
        });
        Self {
            available: choice.refusal.is_none() && document_refusal.is_none(),
            refusal: choice.refusal.clone(),
            document_refusal,
            target,
        }
    }
}

/// One saved Chamfer (§29A), as the pinned reading found it, and what
/// `edit-chamfer-distance` would accept about it. The edge and distance are the
/// stored ones even when the edit is refused. Not a `features` entry, for the
/// reason `fillets` is not.
#[derive(Serialize)]
pub(super) struct ChamferFeatureDiscovery {
    feature_id: ObjectId,
    name: Option<String>,
    body_id: Option<ObjectId>,
    previous_feature_id: ObjectId,
    edge: FilletEdgeDto,
    /// The corner of the cut edge; `null` when the frame is refused.
    corner_mm: Option<[f64; 2]>,
    adjacent_lengths_mm: Option<[f64; 2]>,
    distance_mm: f64,
    distance_unit: &'static str,
    distance_edit: ChamferDistanceEditDiscovery,
}

/// `available` folds in the document-wide refusal, which keeps its priority;
/// `refusal` is this Chamfer's own reason and `document_refusal` the shared
/// one. The bounds are the domain's, both inclusive, for this corner; the
/// maximum is `null` when the frame is refused.
#[derive(Serialize)]
struct ChamferDistanceEditDiscovery {
    available: bool,
    refusal: Option<String>,
    document_refusal: Option<String>,
    request_versions: &'static [u32],
    min_distance_mm: f64,
    max_distance_mm: Option<f64>,
}

impl ChamferFeatureDiscovery {
    pub(super) fn new(
        choice: ferritecad_document::ChamferDistanceChoice,
        document_refusal: Option<String>,
    ) -> Self {
        let saved = choice.saved.as_ref();
        Self {
            feature_id: choice.feature,
            name: choice.name,
            body_id: saved.map(|s| s.body),
            previous_feature_id: choice.stored.previous,
            edge: FilletEdgeDto {
                feature_id: choice.stored.edge.feature,
                joint: choice.stored.edge.joint.segments(),
            },
            corner_mm: saved.map(|s| s.corner.corner_mm),
            adjacent_lengths_mm: saved.map(|s| s.corner.adjacent_lengths_mm),
            distance_mm: choice.stored.distance_mm,
            distance_unit: "mm",
            distance_edit: ChamferDistanceEditDiscovery {
                available: choice.refusal.is_none() && document_refusal.is_none(),
                refusal: choice.refusal,
                document_refusal,
                request_versions: &[1],
                min_distance_mm: ferritecad_document::MIN_DISTANCE_MM,
                max_distance_mm: saved.map(|s| s.corner.max_distance_mm),
            },
        }
    }
}
