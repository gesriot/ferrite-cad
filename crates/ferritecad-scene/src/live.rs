// SPDX-License-Identifier: MIT
use super::*;

#[derive(Clone, Debug)]
pub(super) struct BodyNames {
    pub faces: Vec<Vec<BoundMeaning>>,
    pub edges: Vec<Vec<BoundMeaning>>,
    pub vertices: Vec<Vec<BoundMeaning>>,
}

#[derive(Debug)]
pub(super) struct BodyDrawing {
    pub mesh: Mesh,
    pub names: BodyNames,
    pub key: ContentHash,
    pub solids: u32,
}

/// Local topology partitions stay with the mesh. Packing assigns fresh offsets in
/// document order, since earlier bodies may have changed the number of pick IDs.
#[derive(Default, Debug)]
pub struct BodyMeshes {
    pub(super) entries: BTreeMap<ObjectId, Arc<BodyDrawing>>,
    params: Option<TessellationParams>,
}

impl BodyMeshes {
    pub(super) fn lookup(
        &self,
        id: ObjectId,
        key: Option<ContentHash>,
    ) -> Option<Arc<BodyDrawing>> {
        self.entries
            .get(&id)
            .filter(|d| Some(d.key) == key)
            .cloned()
    }
}

pub fn snapshot_from_revision<K: GeometryKernel>(
    document: &Document,
    built: &ferritecad_eval::RebuildResult,
    kernel: &mut K,
    read_step: impl FnMut(&mut K, &[u8]) -> Result<Import>,
    params: &TessellationParams,
    context: &OperationContext,
    accepted: &BodyMeshes,
) -> Result<(LoadedScene, BodyMeshes)> {
    let empty = BodyMeshes::default();
    let prior = if accepted.params == Some(*params) {
        accepted
    } else {
        &empty
    };
    let mut next = BodyMeshes {
        params: Some(*params),
        ..BodyMeshes::default()
    };
    let mut picture = Picture::new();
    picture.meshes = Some((prior, &mut next));
    let scene =
        prepare::from_rebuild(document, built, kernel, read_step, params, context, picture)?;
    context.check_cancelled()?;
    Ok((scene, next))
}
