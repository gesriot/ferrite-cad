// SPDX-License-Identifier: MIT
//! §29A: what `chamfer-edge-copy` and `edit with chamfer_distance` would accept,
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
