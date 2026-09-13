// SPDX-License-Identifier: MIT
//! Making a new native document, however it was asked for.
//!
//! One route from "a person wants a new document" to a `.fcad` file at a path
//! they chose, taken by the command line and by the window alike. What a new
//! document contains, what its display units are, what the sample plate is
//! made of and — above all — when a file appears at the destination are decided
//! here and nowhere else.
//!
//! # Two boundaries, and both are needed
//!
//! Creating a document is two writes, not one, and each has its own way of
//! being all-or-nothing.
//!
//! The first is inside the file: every object, dependency and topology
//! reference goes in through one [`Document::write`], which is a SQLite
//! transaction. A refusal part way through the model — a size that is not a
//! finite number, an expression that will not build — rolls the whole thing
//! back, so no half-built plate is ever stored.
//!
//! The second is the file itself: the document is built in a private scratch
//! directory beside the destination, its connection is closed, and only then is
//! it published in one atomic no-clobber step. That is what makes "the run
//! failed, so there is no new document" true. A transaction alone cannot say
//! it, because a rolled-back transaction leaves behind the empty database the
//! transaction was started in — which is exactly the defect this module was
//! written to remove: `create bad.fcad --sample --size NaN 50 12` used to exit
//! 2 and leave `bad.fcad` behind with nothing in it.
//!
//! # Never a replacement
//!
//! There is no way to ask this to overwrite anything. Both interfaces refuse a
//! destination that is already taken, and the publication is a no-clobber, so
//! a file that appears between the check and the last step is kept rather than
//! replaced. What each interface tells its own user to do about that is the
//! interface's sentence, carried in [`CreateDocumentRequest::advice`]; a window
//! must not print the name of a command-line flag.
//!
//! Empty/sample creation remains kernel-free. Polygon creation additionally
//! cold-checks the saved graph with a worker-owned kernel before closing and
//! publishing. Both routes use the same writer and publication; neither stores
//! a mesh or requires a window.

use std::path::{Path, PathBuf};

use crate::PolygonExtrusion;
use ferritecad_document::{
    Body, CapSide, DatumPlane, Dependency, DependencyRole, Document, EndCondition, EntityKind,
    Expression, Extrude, ObjectPayload, Point2, SelectionRule, SemanticRole, Sketch, SketchCurve,
    SketchGeometry, SolidOperation, TopologyRef,
};
use ferritecad_kernel::{GeometryKernel, OperationContext, ProgressSink};
use ferritecad_types::{DocumentId, ObjectId, Result, StableEntityId, Transform, Unit};

use crate::publish::{Existing, Temporary, path_entry_exists};

/// The size of a sample plate, in millimetres.
///
/// Millimetres because that is what a document stores, and stored values do
/// not depend on which unit an interface has been asked to show lengths in. A
/// plate asked for as 60 x 40 x 10 is the same plate whether the document
/// displays millimetres or inches.
///
/// Nothing is rejected here. What a model may hold is
/// [`ferritecad_document`]'s to say — a coordinate goes through
/// [`Point2::new`] and a height through [`Expression::constant`], both of which
/// refuse a value that is not finite — and a second opinion in this struct
/// would be a second set of rules, free to disagree with the one the document
/// actually enforces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlateSize {
    pub width: f64,
    pub depth: f64,
    pub height: f64,
}

impl PlateSize {
    /// What both interfaces offer before anybody types anything.
    ///
    /// One place, so the window and the command line cannot suggest different
    /// plates.
    pub const DEFAULT: Self = Self {
        width: 60.0,
        depth: 40.0,
        height: 10.0,
    };
}

impl Default for PlateSize {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What a new document is to contain.
///
/// Empty, sample plate, or one bounded polygon extrusion. This is a creation
/// request, not an arbitrary model or an edit to an existing document.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum NewDocument {
    /// Metadata and nothing else. No objects, no dependencies, no references.
    Empty,
    /// The sample plate: a datum plane, a rectangular profile on it, an
    /// extrusion producing a body, and the three references naming that
    /// extrusion's two caps and the faces raised from its first segment.
    SamplePlate(PlateSize),
    /// A validated XY polygon, cold-checked before publication.
    SketchExtrude(PolygonExtrusion),
}

/// What one creation was asked to do.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CreateDocumentRequest<'a> {
    /// Where the finished document goes, and the only path this publishes to.
    pub destination: &'a Path,
    /// Unit to show lengths in. Values are stored in millimetres regardless.
    pub display_length_unit: Unit,
    /// Unit to show angles in. Values are stored in radians regardless.
    pub display_angle_unit: Unit,
    /// What to put in it.
    pub content: NewDocument,
    /// How the caller's own user is told to get past a taken destination.
    ///
    /// Finishes the sentence `<path> already exists; …`. There is no flag and
    /// no button that authorises replacing a document here, so this says what
    /// the person can do instead — which for a window is not what it is for a
    /// command line, and neither may be printed by the other.
    pub advice: &'a str,
}

impl<'a> CreateDocumentRequest<'a> {
    /// A creation with the display units a document has unless asked
    /// otherwise.
    pub fn new(destination: &'a Path, content: NewDocument, advice: &'a str) -> Self {
        Self {
            destination,
            display_length_unit: Unit::Millimeter,
            display_angle_unit: Unit::Degree,
            content,
            advice,
        }
    }

    /// The same creation, showing lengths and angles in the named units.
    pub fn displaying(mut self, length: Unit, angle: Unit) -> Self {
        self.display_length_unit = length;
        self.display_angle_unit = angle;
        self
    }
}

/// A document that was created and published.
///
/// Deliberately not an exit code, a status line or a sentence: it is what
/// happened. A command line turns this into a number and a line of output; a
/// window turns it into a file to open. Neither of those belongs to the work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedDocument {
    destination: PathBuf,
    document_id: DocumentId,
}

impl CreatedDocument {
    /// Where the document is. The path that was asked for, published.
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    /// The identifier the new document was given.
    ///
    /// New on every creation, by design: two documents made from the same
    /// description are two documents, and nothing may make them one.
    pub fn document_id(&self) -> DocumentId {
        self.document_id
    }
}

/// Creates a new document and publishes it at `request.destination`.
///
/// The destination is written or it is not. A refusal anywhere — a unit that
/// does not measure what it is being used for, a size that is not a finite
/// number, a destination that is already taken, a request given up on — leaves
/// nothing at the destination, leaves whatever was there untouched, and leaves
/// no scratch file or SQLite sidecar beside it.
///
/// Giving up is possible until the publication and not after it. The last
/// cancellation check stands immediately before the one step that touches the
/// destination; once that step has succeeded the document is a real file
/// somebody may already be opening, and a cancellation that arrives then is
/// simply late.
pub fn create_document(
    request: CreateDocumentRequest<'_>,
    context: &OperationContext,
) -> Result<CreatedDocument> {
    if matches!(request.content, NewDocument::SketchExtrude(_)) {
        return Err(ferritecad_types::CadError::unsupported(
            "polygon creation requires a checked kernel route",
        ));
    }
    create_checked(request, context, |_| Ok(()))
}

/// Same transaction/publication route, with a worker-owned kernel for the new
/// polygon. The factory is called only after preflight and model construction.
pub fn create_document_with_kernel<K: GeometryKernel>(
    request: CreateDocumentRequest<'_>,
    factory: impl FnOnce() -> Result<K>,
    context: &OperationContext,
) -> Result<CreatedDocument> {
    let polygon = matches!(request.content, NewDocument::SketchExtrude(_));
    create_checked(request, context, |document| {
        if !polygon {
            return Ok(());
        }
        context.check_cancelled()?;
        let mut kernel = factory()?;
        check_polygon(document, &mut kernel, context)
    })
}

fn check_polygon(
    document: &Document,
    kernel: &mut impl GeometryKernel,
    context: &OperationContext,
) -> Result<()> {
    let progress = context.progress().clone();
    let phase = context
        .clone()
        .with_progress(ProgressSink::new(move |f| progress.report(0.1 + 0.7 * f)));
    let built = ferritecad_eval::rebuild_cold(document, kernel, &phase)?;
    context.progress().report(0.8); // geometry checked next; nothing is published yet
    let checked = (|| {
        if built.shape_count() != 1 {
            return Err(ferritecad_types::CadError::kernel(
                "polygon did not build one shape",
            ));
        }
        for reference in document.topology_refs()? {
            if built.resolve(&reference)?.is_empty() {
                return Err(ferritecad_types::CadError::topology(
                    "polygon lost a stored reference",
                ));
            }
        }
        context.check_cancelled()
    })();
    built.release_all(kernel);
    checked
}

fn create_checked(
    request: CreateDocumentRequest<'_>,
    context: &OperationContext,
    check: impl FnOnce(&Document) -> Result<()>,
) -> Result<CreatedDocument> {
    // Before any work, and cheaply. The publication below is the guarantee;
    // this is the courtesy of not building a document in order to throw it
    // away, and of saying so in the same words either way.
    if path_entry_exists(request.destination)? {
        return Err(taken(request.destination, request.advice));
    }
    context.cancel().check()?;

    let (temporary, document_id) = build_checked(&request, check)?;
    // The SQLite connection is closed; only publication remains.
    context.progress().report(0.9);

    // The last decision, and the only one that cannot be taken back.
    context.cancel().check()?;
    temporary.publish(
        request.destination,
        Existing::Keep {
            advice: request.advice,
        },
    )?;

    // Publication is the completion boundary. Do not check cancellation again.
    context.progress().report(1.0);
    Ok(CreatedDocument {
        destination: request.destination.to_path_buf(),
        document_id,
    })
}

/// Builds the whole document in a scratch file beside the destination.
///
/// The connection is closed before this returns, so what the guard holds is a
/// finished, self-contained SQLite file rather than a database somebody is
/// still writing to. Publishing an open document would publish whatever had
/// been flushed so far, and its rollback journal would be left behind beside
/// it.
///
/// Split out so that the two steps a creation is made of can be driven apart
/// in a gate: everything up to and including the close, and then the one
/// publication.
fn build_checked(
    request: &CreateDocumentRequest<'_>,
    check: impl FnOnce(&Document) -> Result<()>,
) -> Result<(Temporary, DocumentId)> {
    let temporary = Temporary::beside(request.destination)?;

    let mut document = Document::create_with(
        temporary.path(),
        request.display_length_unit,
        request.display_angle_unit,
    )?;
    let document_id = document.meta().document_id;

    // One transaction for the whole model. A refusal inside it rolls back
    // everything, and the scratch document goes with the guard either way.
    match &request.content {
        NewDocument::Empty => {}
        NewDocument::SamplePlate(size) => populate_sample_plate(&mut document, *size)?,
        NewDocument::SketchExtrude(profile) => {
            populate_profile(&mut document, profile.points(), profile.height_mm(), "Body")?
        }
    }
    check(&document)?;
    document.close()?;
    Ok((temporary, document_id))
}

/// What a caller is told about a destination that is already taken.
///
/// The refusal both interfaces make, in one place, finished with whichever
/// sentence the caller's own user can act on. The same wording the atomic
/// publication produces for a file that appears later, so a person sees one
/// refusal rather than two that mean the same thing.
fn taken(destination: &Path, advice: &str) -> ferritecad_types::CadError {
    ferritecad_types::CadError::input(format!(
        "{} already exists; {advice}",
        destination.display()
    ))
}

/// Puts the sample part into a document that has nothing in it.
///
/// The smallest model that exercises the whole document layer: a datum plane,
/// a rectangular profile on it, and an extrusion producing a body — plus the
/// topology references naming that extrusion's two caps and the faces raised
/// from the first segment of its profile.
///
/// Width and depth are the sketch's coordinates and the height is the
/// extrusion's blind distance, all in millimetres. There are no `Parameter`
/// objects: this is the shape of the model as it has always been stored, and
/// changing it is a modelling question rather than part of moving the code.
fn populate_sample_plate(document: &mut Document, size: PlateSize) -> Result<()> {
    let PlateSize {
        width,
        depth,
        height,
    } = size;

    let corners = [
        Point2::new(0.0, 0.0)?,
        Point2::new(width, 0.0)?,
        Point2::new(width, depth)?,
        Point2::new(0.0, depth)?,
    ];
    populate_profile(document, &corners, height, "Plate")
}

fn populate_profile(
    document: &mut Document,
    corners: &[Point2],
    height: f64,
    body_name: &str,
) -> Result<()> {
    let plane = ObjectId::new();
    let sketch = ObjectId::new();
    let extrude = ObjectId::new();
    let body = ObjectId::new();
    let mut curves = Vec::with_capacity(corners.len());
    for (index, start) in corners.iter().enumerate() {
        curves.push(SketchCurve {
            id: StableEntityId::new(),
            construction: false,
            geometry: SketchGeometry::Line {
                start: *start,
                end: corners[(index + 1) % corners.len()],
            },
        });
    }
    let first_segment = curves[0].id;

    document.write(|writer| {
        writer.put_object(
            plane,
            None,
            0,
            Some("XY"),
            &ObjectPayload::DatumPlane(DatumPlane {
                placement: Transform::IDENTITY,
            }),
        )?;
        writer.put_object(
            sketch,
            None,
            1,
            Some("Profile"),
            &ObjectPayload::Sketch(Sketch {
                plane,
                curves: curves.clone(),
                constraints: Vec::new(),
            }),
        )?;
        writer.add_dependency(Dependency {
            dependent: sketch,
            dependency: plane,
            role: DependencyRole::Plane,
        })?;

        writer.put_object(
            body,
            None,
            3,
            Some(body_name),
            &ObjectPayload::Body(Body {
                tip_feature: Some(extrude),
            }),
        )?;

        writer.put_object(
            extrude,
            None,
            2,
            Some("Extrude1"),
            &ObjectPayload::Extrude(Extrude {
                profile: sketch,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(height)?,
                },
                reversed: false,
                operation: SolidOperation::NewBody,
                target_body: None,
            }),
        )?;
        writer.add_dependency(Dependency {
            dependent: extrude,
            dependency: sketch,
            role: DependencyRole::Profile,
        })?;
        writer.add_dependency(Dependency {
            dependent: body,
            dependency: extrude,
            role: DependencyRole::BodyTip,
        })?;

        // Both caps are named up front. Nothing resolves them yet — that is
        // the kernel's job when the document is opened — but the contract they
        // express is part of the model, not of the rebuild.
        for side in [CapSide::Start, CapSide::End] {
            writer.put_topology_ref(&TopologyRef {
                id: StableEntityId::new(),
                owner: extrude,
                producer_feature: extrude,
                expected_kind: EntityKind::Face,
                output_role: SemanticRole::ExtrudeCap { side },
                selection: SelectionRule::Exact,
                fallback_signature: None,
            })?;
        }

        // "Every face raised from this profile segment" stays correct when the
        // segment is split or the count changes; a face index would not.
        writer.put_topology_ref(&TopologyRef {
            id: StableEntityId::new(),
            owner: extrude,
            producer_feature: extrude,
            expected_kind: EntityKind::Face,
            output_role: SemanticRole::ExtrudeSide {
                profile_segment: first_segment,
            },
            selection: SelectionRule::AllDerivedFrom {
                ancestor: first_segment,
            },
            fallback_signature: None,
        })?;

        Ok(())
    })
}

#[cfg(test)]
#[allow(clippy::panic, reason = "a gate that cannot fail is not a gate")]
mod tests {
    use super::*;

    use ferritecad_document::{ObjectKind, SelectionRule, SemanticRole};
    use ferritecad_kernel::CancelToken;
    use ferritecad_types::ErrorKind;

    /// What a caller of this module would put in front of its own user. The
    /// wording is the interface's; this module only places it.
    const ADVICE: &str = "creating would destroy it";

    /// Every entry in a directory, sorted, as text.
    ///
    /// Named entries rather than a count, because the two things that can be
    /// left behind — a private scratch directory and a stray SQLite journal —
    /// have to be nameable in the failure message when one of them is.
    fn entries(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(directory)
            .expect("lists the directory")
            .map(|entry| {
                entry
                    .expect("reads an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    fn plate(size: PlateSize) -> NewDocument {
        NewDocument::SamplePlate(size)
    }

    /// What a creation asks for, with the units a document has by default.
    fn request<'a>(destination: &'a Path, content: NewDocument) -> CreateDocumentRequest<'a> {
        CreateDocumentRequest::new(destination, content, ADVICE)
    }

    #[test]
    fn an_empty_document_is_published_and_can_be_opened() {
        let root = tempfile::tempdir().expect("temporary directory");
        let destination = root.path().join("empty.fcad");

        let created = create_document(
            request(&destination, NewDocument::Empty),
            &OperationContext::default(),
        )
        .expect("creates an empty document");

        assert_eq!(created.destination(), destination);
        assert_eq!(entries(root.path()), vec!["empty.fcad".to_owned()]);

        let document = Document::open(&destination).expect("opens what was published");
        assert_eq!(document.meta().document_id, created.document_id());
        assert_eq!(document.meta().display_length_unit, Unit::Millimeter);
        assert_eq!(document.meta().display_angle_unit, Unit::Degree);
        assert!(document.objects().expect("reads objects").is_empty());
        assert!(
            document
                .dependencies()
                .expect("reads dependencies")
                .is_empty()
        );
        assert!(
            document
                .topology_refs()
                .expect("reads references")
                .is_empty()
        );
        document.close().expect("closes");
    }

    /// Everything the plate is made of, read back out of the published file.
    #[test]
    fn a_sample_plate_is_published_whole() {
        let root = tempfile::tempdir().expect("temporary directory");
        let destination = root.path().join("plate.fcad");
        let size = PlateSize {
            width: 80.0,
            depth: 50.0,
            height: 12.0,
        };

        create_document(
            request(&destination, plate(size)),
            &OperationContext::default(),
        )
        .expect("creates the plate");
        assert_eq!(entries(root.path()), vec!["plate.fcad".to_owned()]);

        let document = Document::open(&destination).expect("opens what was published");
        let objects = document.objects().expect("reads objects");
        let mut names: Vec<&str> = objects
            .iter()
            .map(|object| object.name.as_deref().unwrap_or(""))
            .collect();
        names.sort_unstable();
        assert_eq!(names, ["Extrude1", "Plate", "Profile", "XY"]);

        let sketch = objects
            .iter()
            .find(|object| object.name.as_deref() == Some("Profile"))
            .expect("the profile is stored");
        let ObjectPayload::Sketch(stored) = &sketch.payload else {
            panic!("the profile is not a sketch: {:?}", sketch.payload.kind());
        };
        // The four corners in millimetres, in the order the rectangle is drawn.
        let corners: Vec<(f64, f64)> = stored
            .curves
            .iter()
            .map(|curve| match curve.geometry {
                SketchGeometry::Line { start, .. } => (start.x, start.y),
                ref other => panic!("the profile holds {other:?}"),
            })
            .collect();
        assert_eq!(
            corners,
            vec![(0.0, 0.0), (80.0, 0.0), (80.0, 50.0), (0.0, 50.0)]
        );

        let extrude = objects
            .iter()
            .find(|object| object.name.as_deref() == Some("Extrude1"))
            .expect("the extrusion is stored");
        let ObjectPayload::Extrude(stored) = &extrude.payload else {
            panic!("the extrusion is not one: {:?}", extrude.payload.kind());
        };
        let EndCondition::Blind { distance } = &stored.end_condition else {
            panic!("the extrusion is not blind: {:?}", stored.end_condition);
        };
        assert_eq!(distance.value(), 12.0);
        assert_eq!(stored.operation, SolidOperation::NewBody);

        let dependencies = document.dependencies().expect("reads dependencies");
        assert_eq!(dependencies.len(), 3);
        let mut roles: Vec<DependencyRole> = dependencies.iter().map(|edge| edge.role).collect();
        roles.sort_unstable_by_key(|role| format!("{role:?}"));
        assert_eq!(
            roles,
            vec![
                DependencyRole::BodyTip,
                DependencyRole::Plane,
                DependencyRole::Profile
            ]
        );

        let references = document.topology_refs().expect("reads references");
        assert_eq!(references.len(), 3);
        assert!(
            references
                .iter()
                .all(|reference| reference.expected_kind == EntityKind::Face)
        );
        // The side reference names a segment of the profile that is actually
        // in the profile. A rule naming an entity nothing draws would resolve
        // to nothing for ever, and would still be three references.
        let side = references
            .iter()
            .find(|reference| matches!(reference.output_role, SemanticRole::ExtrudeSide { .. }))
            .expect("the side reference is stored");
        let SemanticRole::ExtrudeSide { profile_segment } = side.output_role else {
            unreachable!("just matched");
        };
        assert_eq!(
            side.selection,
            SelectionRule::AllDerivedFrom {
                ancestor: profile_segment
            }
        );
        assert_eq!(profile_segment, stored_first_segment(&document));

        assert_eq!(
            objects
                .iter()
                .filter(|object| object.payload.kind() == Some(ObjectKind::Body))
                .count(),
            1
        );
        document.close().expect("closes");
    }

    /// The identifier of the profile's first segment, as the document stores
    /// it.
    fn stored_first_segment(document: &Document) -> StableEntityId {
        let objects = document.objects().expect("reads objects");
        let sketch = objects
            .iter()
            .find(|object| object.name.as_deref() == Some("Profile"))
            .expect("the profile is stored");
        let ObjectPayload::Sketch(stored) = &sketch.payload else {
            panic!("the profile is not a sketch");
        };
        stored.curves.first().expect("the profile has curves").id
    }

    /// The failing-first case, at the level the guarantee lives at.
    ///
    /// A width that is not a finite number is refused before the model reaches
    /// a transaction at all — and the point is that this makes no difference
    /// to what is on disk afterwards.
    #[test]
    fn a_size_that_is_not_a_number_publishes_nothing() {
        let root = tempfile::tempdir().expect("temporary directory");
        let destination = root.path().join("bad.fcad");

        let error = create_document(
            request(
                &destination,
                plate(PlateSize {
                    width: f64::NAN,
                    depth: 50.0,
                    height: 12.0,
                }),
            ),
            &OperationContext::default(),
        )
        .expect_err("refuses a width that is not a number");

        assert_eq!(error.kind(), ErrorKind::Input);
        assert!(
            error.to_string().contains("must be finite"),
            "the refusal does not say what is wrong: {error}"
        );
        assert!(!destination.exists(), "a refused creation left a document");
        assert!(
            entries(root.path()).is_empty(),
            "a refused creation left {:?}",
            entries(root.path())
        );
    }

    /// And the same, for a refusal that happens after the document exists and
    /// objects have already been written into it.
    ///
    /// The height is the extrusion's blind distance, which is built inside the
    /// writing transaction and after the plane, the profile and the body have
    /// gone in. Without both boundaries this is the case that leaves a file
    /// behind: the transaction rolls back and the database it rolled back
    /// inside is still a document at the destination.
    #[test]
    fn a_refusal_inside_the_transaction_publishes_nothing() {
        let root = tempfile::tempdir().expect("temporary directory");
        let destination = root.path().join("bad.fcad");

        let error = create_document(
            request(
                &destination,
                plate(PlateSize {
                    width: 60.0,
                    depth: 40.0,
                    height: f64::NAN,
                }),
            ),
            &OperationContext::default(),
        )
        .expect_err("refuses a height that is not a number");

        assert_eq!(error.kind(), ErrorKind::Input);
        assert!(!destination.exists(), "a refused creation left a document");
        assert!(
            entries(root.path()).is_empty(),
            "a refused creation left {:?}",
            entries(root.path())
        );
    }

    /// Half a plate is never stored, which is the other boundary.
    ///
    /// The scratch document is built, the transaction is refused, and what the
    /// guard holds afterwards is an empty document rather than a plate missing
    /// its extrusion. Driven through [`build`] because the whole point is what
    /// is in the file that is *not* published.
    #[test]
    fn a_refused_transaction_leaves_nothing_half_written() {
        let root = tempfile::tempdir().expect("temporary directory");
        let destination = root.path().join("bad.fcad");
        let temporary = Temporary::beside(&destination).expect("reserves scratch space");

        let mut document = Document::create_with(temporary.path(), Unit::Millimeter, Unit::Degree)
            .expect("creates the scratch document");
        populate_sample_plate(
            &mut document,
            PlateSize {
                width: 60.0,
                depth: 40.0,
                height: f64::NAN,
            },
        )
        .expect_err("refuses a height that is not a number");

        assert!(
            document.objects().expect("reads objects").is_empty(),
            "the refused transaction stored objects"
        );
        assert!(
            document
                .topology_refs()
                .expect("reads references")
                .is_empty(),
            "the refused transaction stored references"
        );
        document.close().expect("closes");
    }

    #[test]
    fn a_taken_destination_is_refused_and_left_alone() {
        let root = tempfile::tempdir().expect("temporary directory");
        let destination = root.path().join("plate.fcad");
        std::fs::write(&destination, b"somebody else's file").expect("writes the file");

        let error = create_document(
            request(&destination, plate(PlateSize::DEFAULT)),
            &OperationContext::default(),
        )
        .expect_err("refuses a destination that is taken");

        assert_eq!(error.kind(), ErrorKind::Input);
        assert!(
            error.to_string().ends_with(ADVICE),
            "the refusal does not end in the caller's own advice: {error}"
        );
        assert_eq!(
            std::fs::read(&destination).expect("reads the file"),
            b"somebody else's file"
        );
        assert_eq!(entries(root.path()), vec!["plate.fcad".to_owned()]);
    }

    /// A file that appears while the document is being built is kept.
    ///
    /// Driven through the two steps [`create_document`] itself takes, because
    /// there is no other way to be inside them: the early check cannot see a
    /// file that does not exist yet, and what has to hold is that the
    /// publication refuses anyway.
    #[test]
    fn a_file_that_appears_before_publication_is_kept() {
        let root = tempfile::tempdir().expect("temporary directory");
        let destination = root.path().join("plate.fcad");
        let asked = request(&destination, plate(PlateSize::DEFAULT));

        let racing = destination.clone();
        let context = OperationContext::default().with_progress(
            ferritecad_kernel::ProgressSink::new(move |fraction| {
                if fraction == 0.9 {
                    std::fs::write(&racing, b"somebody else's file").expect("racing writer");
                }
            }),
        );
        let error = create_document(asked, &context).expect_err("atomic no-clobber");

        assert_eq!(error.kind(), ErrorKind::Input);
        assert_eq!(
            std::fs::read(&destination).expect("reads the file"),
            b"somebody else's file"
        );
        assert_eq!(entries(root.path()), vec!["plate.fcad".to_owned()]);
    }

    #[test]
    fn a_withdrawn_request_publishes_nothing() {
        let root = tempfile::tempdir().expect("temporary directory");
        let destination = root.path().join("plate.fcad");

        let cancel = CancelToken::new();
        cancel.cancel();
        let error = create_document(
            request(&destination, plate(PlateSize::DEFAULT)),
            &OperationContext::default().with_cancel(cancel),
        )
        .expect_err("gives up");

        assert_eq!(error.kind(), ErrorKind::Cancellation);
        assert!(!destination.exists(), "a withdrawn request left a document");
        assert!(
            entries(root.path()).is_empty(),
            "a withdrawn request left {:?}",
            entries(root.path())
        );
    }

    #[test]
    fn cancellation_after_close_removes_scratch_and_sidecars() {
        let root = tempfile::tempdir().expect("directory");
        let destination = root.path().join("cancelled.fcad");
        let cancel = CancelToken::new();
        let stopped = cancel.clone();
        let parent = root.path().to_path_buf();
        let context = OperationContext::default()
            .with_cancel(cancel)
            .with_progress(ferritecad_kernel::ProgressSink::new(move |fraction| {
                if fraction == 0.9 {
                    let scratch = std::fs::read_dir(&parent)
                        .expect("directory")
                        .next()
                        .expect("scratch exists")
                        .expect("entry")
                        .path();
                    let doc =
                        Document::open(scratch.join("payload")).expect("closed document reopens");
                    assert_eq!(doc.objects().expect("objects").len(), 4);
                    doc.close().expect("close");
                    // Model sidecars left by a failed SQLite close. Cleanup owns these names.
                    for suffix in ["-wal", "-shm", "-journal"] {
                        std::fs::write(scratch.join(format!("payload{suffix}")), b"sidecar")
                            .expect("sidecar");
                    }
                    stopped.cancel();
                }
            }));
        let error = create_document(request(&destination, plate(PlateSize::DEFAULT)), &context)
            .expect_err("cancelled before publication");
        assert_eq!(error.kind(), ErrorKind::Cancellation);
        assert!(entries(root.path()).is_empty());
    }

    #[test]
    fn cancellation_after_publication_reports_the_created_file() {
        let root = tempfile::tempdir().expect("directory");
        let destination = root.path().join("complete.fcad");
        let cancel = CancelToken::new();
        let stopped = cancel.clone();
        let context = OperationContext::default()
            .with_cancel(cancel)
            .with_progress(ferritecad_kernel::ProgressSink::new(move |fraction| {
                if fraction == 1.0 {
                    stopped.cancel();
                }
            }));
        let created = create_document(request(&destination, NewDocument::Empty), &context)
            .expect("publication already succeeded");
        assert_eq!(created.destination(), destination);
        assert_eq!(entries(root.path()), vec!["complete.fcad"]);
        Document::open(&destination)
            .expect("published document")
            .close()
            .expect("close");
    }

    /// Display units are what a person is shown, and nothing else.
    ///
    /// The same plate is asked for in a document that displays inches, and the
    /// stored coordinates are the same millimetres. A creation whose stored
    /// model depended on the display unit would make two documents of one
    /// description.
    #[test]
    fn the_display_unit_does_not_change_the_stored_millimetres() {
        let root = tempfile::tempdir().expect("temporary directory");
        let size = PlateSize {
            width: 80.0,
            depth: 50.0,
            height: 12.0,
        };

        let mut stored = Vec::new();
        for (name, length, angle) in [
            ("mm.fcad", Unit::Millimeter, Unit::Degree),
            ("inch.fcad", Unit::Inch, Unit::Radian),
        ] {
            let destination = root.path().join(name);
            create_document(
                request(&destination, plate(size)).displaying(length, angle),
                &OperationContext::default(),
            )
            .expect("creates the plate");

            let document = Document::open(&destination).expect("opens it");
            assert_eq!(document.meta().display_length_unit, length);
            assert_eq!(document.meta().display_angle_unit, angle);
            let objects = document.objects().expect("reads objects");
            let sketch = objects
                .iter()
                .find(|object| object.name.as_deref() == Some("Profile"))
                .expect("the profile is stored");
            let ObjectPayload::Sketch(profile) = &sketch.payload else {
                panic!("the profile is not a sketch");
            };
            stored.push(
                profile
                    .curves
                    .iter()
                    .map(|curve| match curve.geometry {
                        SketchGeometry::Line { start, end } => (start.x, start.y, end.x, end.y),
                        ref other => panic!("the profile holds {other:?}"),
                    })
                    .collect::<Vec<_>>(),
            );
            document.close().expect("closes");
        }

        assert_eq!(stored[0], stored[1]);
        assert_eq!(stored[0][1].0, 80.0);
    }

    /// A unit that does not measure what it is being used for is refused, and
    /// refused without leaving anything behind.
    #[test]
    fn a_unit_of_the_wrong_dimension_publishes_nothing() {
        let root = tempfile::tempdir().expect("temporary directory");
        let destination = root.path().join("wrong.fcad");

        let error = create_document(
            request(&destination, NewDocument::Empty).displaying(Unit::Degree, Unit::Degree),
            &OperationContext::default(),
        )
        .expect_err("refuses a length shown in degrees");

        assert_eq!(error.kind(), ErrorKind::Input);
        assert!(!destination.exists(), "a refused creation left a document");
        assert!(entries(root.path()).is_empty());
    }

    /// Which sizes a document accepts, measured rather than assumed.
    ///
    /// Not an endorsement of any of them: it is what the document layer has
    /// always enforced, and this gate exists so that moving the code cannot
    /// quietly widen or narrow it. The height is an extrusion distance and is
    /// refused unless it is positive; the width and the depth are sketch
    /// coordinates and are refused only when they are not finite numbers.
    #[test]
    fn the_sizes_a_document_accepts_are_unchanged() {
        let root = tempfile::tempdir().expect("temporary directory");

        for (name, size) in [
            (
                "flat.fcad",
                PlateSize {
                    width: 0.0,
                    depth: 0.0,
                    height: 10.0,
                },
            ),
            (
                "negative.fcad",
                PlateSize {
                    width: -60.0,
                    depth: -40.0,
                    height: 10.0,
                },
            ),
            (
                "tiny.fcad",
                PlateSize {
                    width: 1e-9,
                    depth: 1e-9,
                    height: 1e-9,
                },
            ),
        ] {
            let destination = root.path().join(name);
            create_document(
                request(&destination, plate(size)),
                &OperationContext::default(),
            )
            .unwrap_or_else(|error| panic!("{name} was refused: {error}"));
            assert!(destination.is_file());
        }

        for (name, size) in [
            (
                "zero-height.fcad",
                PlateSize {
                    width: 60.0,
                    depth: 40.0,
                    height: 0.0,
                },
            ),
            (
                "negative-height.fcad",
                PlateSize {
                    width: 60.0,
                    depth: 40.0,
                    height: -10.0,
                },
            ),
        ] {
            let destination = root.path().join(name);
            let Err(error) = create_document(
                request(&destination, plate(size)),
                &OperationContext::default(),
            ) else {
                panic!("{name} was accepted");
            };
            assert_eq!(error.kind(), ErrorKind::Input);
            assert!(
                error.to_string().contains("must be positive"),
                "{name} was refused for another reason: {error}"
            );
            assert!(!destination.exists(), "{name} was refused and left behind");
        }
    }
    #[test]
    fn polygon_cold_check_cleanup_and_publication_boundaries() {
        use ferritecad_kernel::{ProgressSink, mock::MockKernel};
        let polygon = PolygonExtrusion::new(
            vec![
                [0., 0.],
                [60., 0.],
                [60., 20.],
                [20., 20.],
                [20., 40.],
                [0., 40.],
            ],
            10.,
        )
        .expect("polygon");
        for event in [
            "success",
            "build failure",
            "before",
            "during",
            "before publish",
            "racer",
            "late",
        ] {
            let dir = tempfile::tempdir().expect("dir");
            let out = dir.path().join("new.fcad");
            let cancel = CancelToken::new();
            let stop = cancel.clone();
            let target = out.clone();
            let ctx = OperationContext::default()
                .with_cancel(cancel.clone())
                .with_progress(ProgressSink::new(move |f| {
                    if event == "during" && (0.7..0.9).contains(&f) {
                        stop.cancel();
                    }
                    if f == 0.9 {
                        if event == "before publish" {
                            stop.cancel();
                        }
                        if event == "racer" {
                            std::fs::write(&target, b"racer").expect("racer");
                        }
                    }
                    if f == 1.0 && event == "late" {
                        stop.cancel();
                    }
                }));
            if event == "before" {
                cancel.cancel();
            }
            let mut kernel = MockKernel::new();
            let result = create_checked(
                CreateDocumentRequest::new(
                    &out,
                    NewDocument::SketchExtrude(polygon.clone()),
                    "keep",
                ),
                &ctx,
                |doc| {
                    check_polygon(doc, &mut kernel, &ctx)?;
                    if event == "build failure" {
                        return Err(ferritecad_types::CadError::kernel("injected build failure"));
                    }
                    Ok(())
                },
            );
            assert_eq!(
                kernel.live_shape_count(),
                0,
                "{event}: shapes must be freed before dropping the kernel"
            );
            assert_eq!(
                result.is_ok(),
                matches!(event, "success" | "late"),
                "{event}: {result:?}"
            );
            assert_eq!(out.exists(), matches!(event, "success" | "late" | "racer"));
            assert_eq!(
                entries(dir.path()).len(),
                usize::from(out.exists()),
                "scratch/sidecars after {event}"
            );
            if event == "racer" {
                assert_eq!(std::fs::read(&out).expect("racer"), b"racer");
            }
            if result.is_ok() {
                let d = Document::open_read_only(&out).expect("publication");
                assert_eq!(d.objects().expect("objects").len(), 4);
                d.close().expect("close");
            }
        }
    }
}
