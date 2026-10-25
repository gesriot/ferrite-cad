// SPDX-License-Identifier: MIT
#![allow(dead_code, reason = "shared test support")]
use ferritecad_document::Document;
use ferritecad_kernel::{GeometryKernel, OperationContext, mock::MockKernel};
use ferritecad_types::{ContentHash, ObjectId};
use std::collections::{BTreeMap, BTreeSet};

pub fn names(document: &Document) -> BTreeMap<ObjectId, BTreeSet<(String, ContentHash)>> {
    names_with(document, &mut MockKernel::new())
}

pub fn names_with(
    document: &Document,
    kernel: &mut impl GeometryKernel,
) -> BTreeMap<ObjectId, BTreeSet<(String, ContentHash)>> {
    let built = ferritecad_eval::rebuild_cold(document, kernel, &OperationContext::default())
        .expect("rebuild names");
    assert!(built.report().all_built(), "{:?}", built.report());
    let names = built
        .topology()
        .features
        .iter()
        .map(|(id, feature)| {
            (
                *id,
                feature
                    .bindings
                    .keys()
                    .map(|name| (name.kind.to_string(), name.digest))
                    .collect(),
            )
        })
        .collect();
    built.release_all(kernel);
    names
}
