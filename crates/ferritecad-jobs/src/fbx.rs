// SPDX-License-Identifier: MIT
//! One document, one FBX, however it was asked for.
//!
//! The whole route from a stored document to a file another program opens as a
//! model: [`export_scene`] keeps the hierarchy, the placements and the
//! definitions that a picture throws away, [`write_fbx_ascii_7400`] is handed
//! that and a byte sink and nothing else, and the result reaches its
//! destination in one step or not at all.
//!
//! # Nothing here is a second opinion
//!
//! One read of the document, one cold rebuild, one reading of each stored STEP
//! source and one call to the writer. This module never reopens the document,
//! never asks the kernel for more geometry, never touches the STEP file the
//! document was imported from — it no longer has to exist — and never works
//! out for itself what the export left behind. That last one matters most:
//! what was left out is [`FbxWriteReport::omissions`] and nothing else, so
//! there is no second list that could disagree with the file that was
//! published.
//!
//! # A partial export is a file, and says so
//!
//! A definition this build cannot turn into triangles keeps its place in the
//! hierarchy and the export is published anyway, because refusing to publish
//! would throw away every definition that was fine. The outcome says which
//! kind of export it was; what that is worth — an exit code, a line in a
//! window — is the caller's to decide.
//!
//! # Giving up
//!
//! Both halves are cancellable, not just the long one. Building the scene
//! takes the cancellation through the [`OperationContext`] the kernel already
//! reads; writing it takes it through a sink that refuses the next block once
//! the request has been withdrawn. And the last thing before publication is
//! one more check, because everything up to that point can still be thrown
//! away and nothing after it can.

use std::io::Write;
use std::path::{Path, PathBuf};

use ferritecad_exchange::Import;
use ferritecad_export::{ExportScene, FbxWriteReport, write_fbx_ascii_7400};
use ferritecad_kernel::{CancelToken, GeometryKernel, OperationContext, TessellationParams};
use ferritecad_scene::export_scene;
use ferritecad_types::{CadError, Result};

use crate::publish::{Existing, Temporary, refuse_source_as_destination};

/// Why a document may not be its own FBX, in the words both interfaces use.
///
/// One sentence, because it is one refusal. A window and a command line
/// disagree about almost everything they say to a person; they do not get to
/// disagree about whether this happened.
pub const SOURCE_IS_DESTINATION: &str = "the native document cannot also be the FBX output";

/// What one export was asked to do.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct FbxExportRequest<'a> {
    /// The stored document. Opened read-only and never written to.
    pub document: &'a Path,
    /// Where the finished file goes, and the only path this touches.
    pub destination: &'a Path,
    /// What to do about anything already at the destination.
    pub existing: Existing<'a>,
}

impl<'a> FbxExportRequest<'a> {
    /// An export that refuses to overwrite anything, with the sentence the
    /// caller's own user would read if it did.
    pub fn new(document: &'a Path, destination: &'a Path, existing: Existing<'a>) -> Self {
        Self {
            document,
            destination,
            existing,
        }
    }
}

/// A published FBX, and what the writer said about it.
///
/// Deliberately not an exit code, a status line or a sentence. It is what
/// happened: where the file is, how big it is, what is in it, and every
/// definition that could not be given triangles. A command line turns this
/// into a number and some text on standard error; a window turns it into rows
/// on a panel; neither of those belongs to the work itself.
#[derive(Debug)]
pub struct FbxExport {
    destination: PathBuf,
    report: FbxWriteReport,
}

impl FbxExport {
    /// Where the file is. The path that was asked for, published.
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    /// The writer's own record of what it wrote.
    pub fn report(&self) -> &FbxWriteReport {
        &self.report
    }

    /// Whether the file is the whole document.
    ///
    /// False means the file is real and published, and something the document
    /// holds has no triangles in it. It never means the file is missing.
    pub fn is_complete(&self) -> bool {
        self.report.is_complete()
    }
}

/// Writes `request.document` out as FBX 7.4 ASCII and publishes it.
///
/// `kernel` is handed in rather than opened here, so the session belongs to
/// whichever thread is doing this work and ends with it. `read_step` is how
/// that kernel reads a STEP source the document stores; the bytes come from
/// the document, so nothing external is opened and a document exports the same
/// scene years after the file it was imported from is gone.
///
/// The destination is written or it is not: a failure anywhere — before the
/// first byte, in the middle of the write, at publication — leaves nothing at
/// the destination, leaves whatever was there untouched, and leaves no scratch
/// file behind.
pub fn export_document_as_fbx<K>(
    request: FbxExportRequest<'_>,
    kernel: &mut K,
    read_step: impl FnMut(&mut K, &[u8]) -> Result<Import>,
    params: &TessellationParams,
    context: &OperationContext,
) -> Result<FbxExport>
where
    K: GeometryKernel + ?Sized,
{
    // First, and before any work: a document that is its own destination is
    // refused whatever the caller has already agreed to. There is no state of
    // mind in which overwriting the file being read is what somebody meant.
    refuse_source_as_destination(request.document, request.destination, SOURCE_IS_DESTINATION)?;

    // Cold on purpose, and the same reading a picture is built from with the
    // hierarchy kept. Every shape this makes is released before it returns,
    // whatever happens.
    let scene = export_scene(request.document, kernel, read_step, params, context)?;

    let report = write_and_publish(&request, &scene, context.cancel())?;
    Ok(FbxExport {
        destination: request.destination.to_path_buf(),
        report,
    })
}

/// Writes the scene into a scratch file beside the destination and publishes
/// it once, after the writer has finished.
///
/// See [`crate::publish`] for why the scratch file lives where it does and what
/// makes the last step atomic. Streamed rather than built in memory first: the
/// complex assembly's FBX is hundreds of megabytes, almost all of it vertex and
/// normal arrays, and holding a second copy of that would be the difference
/// between a large export and one that cannot be done at all.
fn write_and_publish(
    request: &FbxExportRequest<'_>,
    scene: &ExportScene,
    cancel: &CancelToken,
) -> Result<FbxWriteReport> {
    let temporary = Temporary::beside(request.destination)?;

    // `create_new`, so this cannot open something already at that name and
    // cannot follow a symlink to somewhere else.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary.path())
        .map_err(|e| CadError::io(format!("creating {}", temporary.path().display()), e))?;

    let mut sink = std::io::BufWriter::with_capacity(1 << 20, file);
    let report = write_scene(scene, &mut sink, cancel)?;

    // `into_inner` flushes what the buffer still holds and hands back the
    // error rather than swallowing it in a drop.
    let file = sink.into_inner().map_err(|error| {
        CadError::io(
            format!("writing {}", temporary.path().display()),
            error.into_error(),
        )
    })?;
    file.sync_all()
        .map_err(|e| CadError::io(format!("syncing {}", temporary.path().display()), e))?;
    drop(file);

    publish_if_still_wanted(temporary, request.destination, request.existing, cancel)?;
    Ok(report)
}

/// The one call to the writer, through a sink that can be told to stop.
///
/// The writer itself knows nothing about cancellation and must not: it is
/// handed a scene and a byte sink, and that is the whole reason its output is
/// a function of the scene. Giving up is therefore expressed where the bytes
/// go — the next block is refused — and the refusal is turned back into the
/// one cancellation error every other part of this system uses.
fn write_scene(
    scene: &ExportScene,
    sink: &mut impl Write,
    cancel: &CancelToken,
) -> Result<FbxWriteReport> {
    let mut guarded = Cancellable {
        inner: sink,
        cancel,
    };
    match write_fbx_ascii_7400(scene, &mut guarded) {
        Ok(report) => Ok(report),
        // A write refused because the request was withdrawn is a cancellation
        // and not an I/O failure, and saying so is what lets an interface tell
        // "the user changed their mind" from "the disk is full".
        Err(error) => {
            cancel.check()?;
            Err(error)
        }
    }
}

/// Publishes what was written, unless the request has been withdrawn.
///
/// The last decision, and the only one that cannot be taken back. Everything
/// before this line can be thrown away — the scratch file goes with the guard
/// and the destination is untouched — and nothing after it can: once the file
/// is at the destination it is a real file that somebody may already be
/// reading, so a cancellation that arrives afterwards is simply late.
fn publish_if_still_wanted(
    temporary: Temporary,
    destination: &Path,
    existing: Existing<'_>,
    cancel: &CancelToken,
) -> Result<()> {
    cancel.check()?;
    temporary.publish(destination, existing)
}

/// A byte sink that stops accepting once the work has been given up on.
///
/// Checked per write rather than per buffer flush: the writer emits a line at
/// a time into a large buffer, so this is where a cancellation is noticed
/// promptly, and the buffer underneath still turns those into few, large
/// writes to the file.
struct Cancellable<'a, W> {
    inner: W,
    cancel: &'a CancelToken,
}

impl<W: Write> Write for Cancellable<'_, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.cancel.is_cancelled() {
            // Deliberately not `ErrorKind::Interrupted`, which is the kind
            // this most resembles and the one thing it must not be:
            // `Write::write_all` treats an interrupted write as a write to
            // retry, so a sink that refused with it would be asked again, and
            // again, for as long as the process lived. A refusal that means
            // "stop" has to be a kind nothing retries.
            return Err(std::io::Error::other("the export was cancelled"));
        }
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
#[allow(clippy::panic, reason = "a gate that cannot fail is not a gate")]
mod tests {
    use super::*;
    use ferritecad_exchange::{Diagnostic, Severity, Stage};
    use ferritecad_export::{
        ExportColourOrigin, ExportGeometry, ExportMaterial, ExportMesh, ExportOccurrence,
        ExportOmission, ExportProvenance, ExportSceneBuilder, ExportSource, ExportTransform,
    };
    use ferritecad_kernel::TessellationRefusal;
    use ferritecad_types::ObjectId;

    const ADVICE: &str = "pass --force to replace it";

    fn keep() -> Existing<'static> {
        Existing::Keep { advice: ADVICE }
    }

    /// One triangle, one slot, in whatever colour is asked for.
    fn mesh(colour: [f64; 3]) -> ExportMesh {
        ExportMesh::new(
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![[0.0, 0.0, 1.0]; 3],
            vec![[0, 1, 2]],
            vec![0],
            vec![
                ExportMaterial::new("slot".to_owned(), colour, ExportColourOrigin::Default)
                    .expect("a material in range"),
            ],
        )
        .expect("a mesh that holds together")
    }

    fn omission() -> ExportGeometry {
        ExportGeometry::Omitted(ExportOmission::new(
            Diagnostic {
                stage: Stage::Validation,
                severity: Severity::Warning,
                entity: "step.product_definition#2583".to_owned(),
                message: "the solid is not valid".to_owned(),
            },
            TessellationRefusal::IncompleteFace,
        ))
    }

    /// A scene of one definition with one placement, called `name`.
    fn one_node(geometry: ExportGeometry, name: &str) -> ExportScene {
        let mut builder = ExportSceneBuilder::new();
        let definition = builder
            .definition(
                ExportSource::Body {
                    object: ObjectId::new(),
                },
                Some("part".to_owned()),
                ExportProvenance::default(),
                geometry,
            )
            .expect("a definition");
        builder
            .node(
                None,
                definition,
                ExportTransform::IDENTITY,
                Some(name.to_owned()),
                None,
                ExportOccurrence::Unrecorded,
            )
            .expect("a placement");
        builder.finish().expect("a scene")
    }

    fn leftovers(directory: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(directory)
            .expect("lists the directory")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.to_string_lossy().contains(".partial"))
            .collect()
    }

    /// The scene, the scratch file and the publication, with no kernel in it.
    fn publish(destination: &Path, scene: &ExportScene, existing: Existing<'_>) -> Result<()> {
        let request = FbxExportRequest::new(Path::new("document.fcad"), destination, existing);
        write_and_publish(&request, scene, &CancelToken::new()).map(|_| ())
    }

    #[test]
    fn a_writer_that_fails_after_it_has_started_publishes_nothing() {
        let directory = tempfile::tempdir().expect("temp dir");
        let destination = directory.path().join("part.fbx");

        // A name FBX ASCII cannot spell. It is refused while the objects are
        // being written, which is well after the header, the settings and the
        // definitions have gone into the scratch file — so this is a failure
        // with a half-written file behind it, not a refusal before any byte.
        let scene = one_node(ExportGeometry::Mesh(mesh([0.5, 0.5, 0.5])), "bad\u{7}name");
        let error = publish(&destination, &scene, keep())
            .expect_err("a name the format cannot spell must stop the write");

        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Unsupported);
        assert!(
            !destination.exists(),
            "a failed write published a destination"
        );
        assert!(
            leftovers(directory.path()).is_empty(),
            "a failed write left scratch space behind: {:?}",
            leftovers(directory.path())
        );
    }

    #[test]
    fn a_colour_the_format_cannot_record_is_refused_before_anything_is_published() {
        let directory = tempfile::tempdir().expect("temp dir");
        let destination = directory.path().join("part.fbx");

        // A linear intensity the export model accepts and FBX has no way of
        // recording. The writer refuses it while working out what the file will
        // say, which is before the scratch file is given a single byte.
        let scene = one_node(ExportGeometry::Mesh(mesh([2.0, 0.0, 0.0])), "unremarkable");
        let error = publish(&destination, &scene, keep())
            .expect_err("a colour outside the measured range must stop the export");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Unsupported);

        assert!(!destination.exists());
        assert!(leftovers(directory.path()).is_empty());
    }

    /// A shear cannot even become a scene, so it can never reach the writer.
    #[test]
    fn a_placement_no_static_hierarchy_can_express_never_becomes_a_scene() {
        let sheared = ExportTransform::new([
            [1.0, 0.3, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]);
        assert!(sheared.is_err(), "a shear was accepted as a placement");
    }

    /// The early check an interface makes is a courtesy; this is the decision.
    ///
    /// A file that appears between whatever the caller asked and the end of a
    /// long tessellation must not be overwritten, and the export that lost the
    /// race must leave nothing of itself behind.
    #[test]
    fn a_destination_that_appears_while_the_writer_works_is_not_overwritten() {
        let directory = tempfile::tempdir().expect("temp dir");
        let destination = directory.path().join("part.fbx");
        std::fs::write(&destination, b"arrived during the export").expect("writes");

        let scene = one_node(ExportGeometry::Mesh(mesh([0.5, 0.5, 0.5])), "part");
        let error = publish(&destination, &scene, keep())
            .expect_err("publishing without replacement must not replace anything");

        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input);
        // And in the words of the interface that asked, which is what the
        // advice carried by the request is for.
        assert!(error.to_string().contains(ADVICE), "{error}");
        assert_eq!(
            std::fs::read(&destination).expect("the other file remains"),
            b"arrived during the export"
        );
        assert!(leftovers(directory.path()).is_empty());
    }

    /// And an authorised replacement replaces the whole file rather than the
    /// part of it the new one happens to reach.
    #[test]
    fn replacing_a_destination_leaves_none_of_the_old_file_behind() {
        let directory = tempfile::tempdir().expect("temp dir");
        let destination = directory.path().join("part.fbx");
        let stale = vec![b'x'; 1 << 20];
        std::fs::write(&destination, &stale).expect("writes");

        let scene = one_node(ExportGeometry::Mesh(mesh([0.5, 0.5, 0.5])), "part");
        publish(&destination, &scene, Existing::Replace).expect("an authorised replacement");

        let published = std::fs::read(&destination).expect("reads the replacement");
        assert!(published.starts_with(b"; FBX 7.4.0 project file"));
        assert!(published.len() < stale.len(), "the old tail survived");
        assert!(leftovers(directory.path()).is_empty());
    }

    #[test]
    fn a_partial_export_is_published_and_says_it_is_not_the_whole_document() {
        let directory = tempfile::tempdir().expect("temp dir");
        let destination = directory.path().join("part.fbx");

        let scene = one_node(omission(), "frame");
        let request = FbxExportRequest::new(Path::new("document.fcad"), &destination, keep());
        let report = write_and_publish(&request, &scene, &CancelToken::new())
            .expect("an omission is still writable");

        assert!(destination.exists(), "a partial export published nothing");
        assert!(std::fs::metadata(&destination).expect("stats").len() > 0);
        assert!(!report.is_complete());
        assert_eq!(report.omissions().len(), 1);
        assert!(leftovers(directory.path()).is_empty());
    }

    /// Serialisation stops when the request is withdrawn, and stops as a
    /// cancellation rather than as a disk that went wrong.
    #[test]
    fn a_write_that_is_given_up_on_stops_and_says_it_was_cancelled() {
        let scene = one_node(ExportGeometry::Mesh(mesh([0.5, 0.5, 0.5])), "part");
        let cancel = CancelToken::new();
        cancel.cancel();

        let mut sink = Vec::new();
        let error = write_scene(&scene, &mut sink, &cancel)
            .expect_err("a withdrawn request must not be written out");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Cancellation);
        assert!(sink.is_empty(), "a cancelled write produced bytes");
    }

    /// And it stops in the middle, not only before it starts.
    #[test]
    fn a_write_given_up_on_part_way_through_stops_where_it_was() {
        /// A sink that withdraws the request once it has seen enough.
        struct GiveUpAfter<'a> {
            seen: usize,
            limit: usize,
            cancel: &'a CancelToken,
            written: Vec<u8>,
        }
        impl Write for GiveUpAfter<'_> {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.written.extend_from_slice(buf);
                self.seen += buf.len();
                if self.seen >= self.limit {
                    self.cancel.cancel();
                }
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let scene = one_node(ExportGeometry::Mesh(mesh([0.5, 0.5, 0.5])), "part");
        let cancel = CancelToken::new();
        let mut sink = GiveUpAfter {
            seen: 0,
            limit: 64,
            cancel: &cancel,
            written: Vec::new(),
        };
        let error = write_scene(&scene, &mut sink, &cancel)
            .expect_err("a request withdrawn mid-write must not finish");

        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Cancellation);
        assert!(sink.seen >= 64, "the sink never saw anything");
        // Stopped where it was, rather than having gone on to the end.
        let whole = {
            let mut all = Vec::new();
            write_scene(&scene, &mut all, &CancelToken::new()).expect("writes");
            all
        };
        assert!(
            sink.written.len() < whole.len(),
            "a cancelled write produced the whole file anyway"
        );
    }

    /// A refusal that means "stop" must be one nothing retries.
    ///
    /// This is the shape of a defect rather than a hypothetical: spelling the
    /// refusal `Interrupted` is the obvious thing to do — it is exactly what
    /// happened here — and it makes a cancelled export spin for ever instead
    /// of stopping, because [`Write::write_all`] treats an interrupted write
    /// as one to try again. Asserted on the kind rather than by writing and
    /// seeing what happens, because what happens is that nothing happens.
    #[test]
    fn a_withdrawn_request_is_refused_with_a_kind_nothing_retries() {
        let cancel = CancelToken::new();
        cancel.cancel();
        let mut written = Vec::new();
        let mut sink = Cancellable {
            inner: &mut written,
            cancel: &cancel,
        };

        let error = sink
            .write(b"anything at all")
            .expect_err("a withdrawn request must be refused");
        assert_ne!(
            error.kind(),
            std::io::ErrorKind::Interrupted,
            "write_all retries an interrupted write, so this refusal never ends"
        );
        // And the whole-buffer path, which is what the writer actually calls,
        // gives up rather than asking again.
        assert!(sink.write_all(b"anything at all").is_err());
        assert!(written.is_empty(), "a refused sink wrote something");
    }

    /// The last check, and what it is worth.
    ///
    /// A cancellation that arrives while the finished bytes are still in the
    /// scratch file publishes nothing: no destination appears, whatever was
    /// there stays, and the scratch space goes.
    #[test]
    fn a_cancellation_that_arrives_before_publication_publishes_nothing() {
        let directory = tempfile::tempdir().expect("temp dir");
        let destination = directory.path().join("part.fbx");
        let temporary = Temporary::beside(&destination).expect("reserves scratch space");
        std::fs::write(temporary.path(), b"a finished file").expect("writes the scratch file");

        let cancel = CancelToken::new();
        cancel.cancel();
        let error = publish_if_still_wanted(temporary, &destination, keep(), &cancel)
            .expect_err("a withdrawn request must not publish");

        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Cancellation);
        assert!(!destination.exists(), "a cancelled export published a file");
        assert!(leftovers(directory.path()).is_empty());
    }

    /// And an existing file is left exactly as it was.
    #[test]
    fn a_cancellation_before_publication_leaves_the_old_file_alone() {
        let directory = tempfile::tempdir().expect("temp dir");
        let destination = directory.path().join("part.fbx");
        std::fs::write(&destination, b"the file that was already there").expect("writes");
        let temporary = Temporary::beside(&destination).expect("reserves scratch space");
        std::fs::write(temporary.path(), b"a finished replacement").expect("writes");

        let cancel = CancelToken::new();
        cancel.cancel();
        let error = publish_if_still_wanted(temporary, &destination, Existing::Replace, &cancel)
            .expect_err("a withdrawn request must not replace anything either");

        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Cancellation);
        assert_eq!(
            std::fs::read(&destination).expect("the old file remains"),
            b"the file that was already there"
        );
        assert!(leftovers(directory.path()).is_empty());
    }

    /// A request that has not been withdrawn publishes, which is what makes
    /// the check above a check rather than a refusal.
    #[test]
    fn a_request_still_wanted_publishes() {
        let directory = tempfile::tempdir().expect("temp dir");
        let destination = directory.path().join("part.fbx");
        let temporary = Temporary::beside(&destination).expect("reserves scratch space");
        std::fs::write(temporary.path(), b"a finished file").expect("writes");

        publish_if_still_wanted(temporary, &destination, keep(), &CancelToken::new())
            .expect("nothing was withdrawn");
        assert_eq!(
            std::fs::read(&destination).expect("the published file"),
            b"a finished file"
        );
    }
}
