// SPDX-License-Identifier: MIT
use super::*;

pub(crate) fn open(path: &Path) -> Result<DocumentSession> {
    open_in(
        &std::env::temp_dir(),
        path,
        ferritecad_jobs::HistoryLimits::default(),
    )
}
pub(crate) fn open_in(
    root: &Path,
    path: &Path,
    limits: ferritecad_jobs::HistoryLimits,
) -> Result<DocumentSession> {
    let session = DocumentSession::open_in(root, path, limits)?;
    let snapshot = session.current();
    let params = TessellationParams::default();
    let context = OperationContext::default();
    if ferritecad_occt::is_available() {
        snapshot.load_scene(
            &mut ferritecad_occt::OcctKernel::new()?,
            |k, bytes| k.import_step(bytes),
            &params,
            &context,
        )?;
    } else {
        snapshot.load_scene(
            &mut ferritecad_kernel::mock::MockKernel::new(),
            |_, _| Err(CadError::unsupported("no imports in this test")),
            &params,
            &context,
        )?;
    }
    Ok(session)
}
