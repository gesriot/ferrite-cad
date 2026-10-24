// SPDX-License-Identifier: MIT
use crate::TopologyMap;
use ferritecad_document::TopologyRef;
use ferritecad_kernel::SubShapeHandle;
use ferritecad_types::{CadError, Result};

pub fn resolve(map: &TopologyMap, reference: &TopologyRef) -> Result<Vec<SubShapeHandle>> {
    map.resolve_legacy(reference)
        .map(|set| set.into_iter().collect())
        .map_err(|error| CadError::Context {
            kind: error.kind(),
            message: format!(
                "topology reference {} of feature {} ({:?}): {error}",
                reference.id, reference.producer_feature, reference.output_role
            ),
            source: Box::new(error),
        })
}
