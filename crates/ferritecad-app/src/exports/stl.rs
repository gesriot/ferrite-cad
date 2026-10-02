// SPDX-License-Identifier: MIT
//! STL-specific form and adapter over the existing export lifecycle.

use super::*;
use ferritecad_jobs::{
    BodySelection, STL_SOURCE_IS_DESTINATION, StlBody, StlExport, StlExportRequest,
    export_document_as_stl,
};
use ferritecad_types::{CadError, ObjectId};
use ferritecad_ui::{StlBodyRow, StlExportForm};

/// Captured together, from the accepted scene/form, before the Save dialog.
#[derive(Debug, Clone)]
pub(crate) struct StlIntent {
    /// What is read: the file the accepted model lives in.
    pub document: PathBuf,
    /// What the user calls the document: the dialog's suggested name and folder, and
    /// the file an export must never be written over. The same as `document`
    /// unless an open document session keeps the model in a private file.
    pub alias: PathBuf,
    pub body: ObjectId,
    pub params: TessellationParams,
}

pub(super) struct StlForm {
    document: PathBuf,
    alias: PathBuf,
    shown: StlExportForm,
}

impl Exports {
    pub(crate) fn configuring_stl(&self) -> bool {
        self.stl_form.is_some()
    }

    pub(crate) fn pending_stl(&self) -> Option<&StlIntent> {
        self.pending_stl.as_ref()
    }

    pub(crate) fn presentation(&mut self) -> (&ExportStatus, Option<&mut StlExportForm>) {
        (
            &self.status,
            self.stl_form.as_mut().map(|form| &mut form.shown),
        )
    }

    #[cfg(test)]
    pub(crate) fn ask_stl(
        &mut self,
        document: &Path,
        bodies: Vec<StlBody>,
        input: &mut ViewportInput,
    ) -> bool {
        self.ask_stl_for(document, document, bodies, input)
    }

    /// As [`Self::ask_stl`], for a document whose model is read from `document` but
    /// which the user knows by `alias`.
    pub(crate) fn ask_stl_for(
        &mut self,
        document: &Path,
        alias: &Path,
        bodies: Vec<StlBody>,
        input: &mut ViewportInput,
    ) -> bool {
        if bodies.is_empty() {
            return false;
        }
        super::leave_document(self, input);
        self.stl_form = Some(StlForm {
            document: document.to_path_buf(),
            alias: alias.to_path_buf(),
            shown: StlExportForm {
                selected: if bodies.len() == 1 {
                    Some(bodies[0].id)
                } else {
                    None
                },
                bodies: bodies
                    .into_iter()
                    .map(|body| StlBodyRow {
                        id: body.id,
                        label: body.label(),
                    })
                    .collect(),
                linear_mm: TessellationParams::DEFAULT_LINEAR.to_string(),
                angular_rad: TessellationParams::DEFAULT_ANGULAR.to_string(),
                error: None,
            },
        });
        true
    }

    pub(crate) fn stl_intent(&mut self) -> Option<StlIntent> {
        let form = self.stl_form.as_mut()?;
        let parsed: Result<_> = (|| {
            let body = form
                .shown
                .selected
                .filter(|id| form.shown.bodies.iter().any(|b| b.id == *id))
                .ok_or_else(|| CadError::input("choose one body"))?;
            let linear = form
                .shown
                .linear_mm
                .trim()
                .parse()
                .map_err(|_| CadError::input("linear deflection must be a number in mm"))?;
            let angular =
                form.shown.angular_rad.trim().parse().map_err(|_| {
                    CadError::input("angular deflection must be a number in radians")
                })?;
            Ok(StlIntent {
                document: form.document.clone(),
                alias: form.alias.clone(),
                body,
                params: TessellationParams::new(linear, angular, false)?,
            })
        })();
        match parsed {
            Ok(intent) => {
                form.shown.error = None;
                Some(intent)
            }
            Err(error) => {
                form.shown.error = Some(error.to_string());
                None
            }
        }
    }
}

pub(crate) fn begin_stl_export(
    exports: &mut Exports,
    input: &mut ViewportInput,
    intent: &StlIntent,
    chosen: Option<PathBuf>,
    spawn: impl FnOnce(&Path, bool, ExportGeneration, &CancelToken) -> JoinHandle<()>,
) -> Option<ExportGeneration> {
    let was_chosen = chosen.is_some();
    let started = begin_export_for(
        exports,
        input,
        Some(&intent.alias),
        chosen,
        STL_SOURCE_IS_DESTINATION,
        spawn,
    );
    if was_chosen && exports.pending.is_some() {
        exports.pending_stl = Some(intent.clone());
    }
    started
}

pub(crate) fn confirm_stl_export(
    exports: &mut Exports,
    input: &mut ViewportInput,
    choice: ReplaceChoice,
    spawn: impl FnOnce(&Path, bool, ExportGeneration, &CancelToken) -> JoinHandle<()>,
) -> Option<ExportGeneration> {
    let intent = exports.pending_stl.clone()?;
    confirm_export_for(
        exports,
        input,
        Some(&intent.alias),
        choice,
        STL_SOURCE_IS_DESTINATION,
        spawn,
    )
}

pub(crate) fn finish_stl_export(
    exports: &mut Exports,
    input: &mut ViewportInput,
    generation: ExportGeneration,
    result: Result<StlExport>,
) -> bool {
    finish_outcome(
        exports,
        input,
        generation,
        result.map(|exported| ExportStatus::WroteStl {
            destination: display(&exported.destination),
            body: exported.body.label(),
            triangles: exported.triangles,
            bytes: exported.bytes,
        }),
    )
}

pub(crate) fn run_stl_export(
    intent: &StlIntent,
    destination: &Path,
    replace: bool,
    context: &OperationContext,
) -> Result<StlExport> {
    export_document_as_stl(
        StlExportRequest {
            document: &intent.document,
            destination,
            body: BodySelection::Id(intent.body),
            params: intent.params,
            existing: existing(replace),
        },
        OcctKernel::new,
        context,
    )
}

#[cfg(test)]
mod tests;
