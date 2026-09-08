// SPDX-License-Identifier: MIT
//! The CLI's arguments and words over the same STL operation as the viewer.

use ferritecad_jobs::{BodySelection, StlExport, StlExportRequest, export_document_as_stl};
use ferritecad_kernel::{OperationContext, TessellationParams};
use ferritecad_occt::OcctKernel;
use ferritecad_types::Result;
use std::process::ExitCode;

use crate::{ExportStlArgs, replacing};

pub fn export_stl(args: ExportStlArgs) -> Result<ExitCode> {
    let linear = args.linear_deflection;
    let angular = args.angular_deflection;
    let exported = export_stl_result(args)?;
    println!(
        "wrote {} ({} triangles, {} bytes) from {}",
        exported.destination.display(),
        exported.triangles,
        exported.bytes,
        exported.body.label()
    );
    println!("  deflection: {linear} mm linear, {angular} rad angular");
    Ok(ExitCode::SUCCESS)
}

pub fn export_stl_result(args: ExportStlArgs) -> Result<StlExport> {
    if args.json {
        crate::json::require_utf8_path(&args.path)?;
        crate::json::require_utf8_path(&args.output)?;
    }
    let params = TessellationParams::new(args.linear_deflection, args.angular_deflection, false)?;
    export_document_as_stl(
        StlExportRequest {
            document: &args.path,
            destination: &args.output,
            body: args
                .solid
                .as_deref()
                .map(BodySelection::NameOrId)
                .unwrap_or(BodySelection::Only {
                    advice: "name one with --solid",
                }),
            params,
            existing: replacing(args.force),
        },
        OcctKernel::new,
        &OperationContext::default(),
    )
}
