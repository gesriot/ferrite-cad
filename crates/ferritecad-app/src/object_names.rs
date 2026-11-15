// SPDX-License-Identifier: MIT
//! Smallest unused numbered object name, shared by creation forms.
use ferritecad_document::ObjectRecord;

pub(crate) fn next(objects: &[ObjectRecord], prefix: &str) -> String {
    (1..)
        .map(|n| format!("{prefix} {n}"))
        .find(|name| !objects.iter().any(|o| o.name.as_ref() == Some(name)))
        .expect("a finite document has an unused name")
}
