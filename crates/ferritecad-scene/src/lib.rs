// SPDX-License-Identifier: MIT
//! Turning a stored document into a picture of what it describes.
//!
//! One direction only: a document is read, rebuilt, tessellated and packed
//! into a [`RenderSnapshot`]. Nothing here writes, and nothing here draws.
//!
//! # The document is not touched
//!
//! Opening is [`Document::open_read_only`], which neither migrates a schema
//! nor changes a persistent pragma. A viewer that quietly rewrote the file it
//! was asked to look at would be the worst kind of surprise: the change would
//! be invisible, and it would happen to the one copy the user has.
//!
//! # A kernel is handed in
//!
//! So this can be exercised against the mock, with no Open CASCADE anywhere,
//! and so the caller decides which session the shapes belong to. Every shape
//! this makes is released before it returns, on the path that succeeds and on
//! every path that does not: a viewer that leaked a session's worth of solids
//! per failed load would run out of memory while showing an error message.
//!
//! # Two kinds of geometry, one session
//!
//! A native body is rebuilt from its features. An imported STEP object is not
//! built at all: its geometry comes from bytes the document stores, and
//! reading them again needs an importer. Both must end up in the same kernel
//! session, because both are drawn in the same picture and released by the
//! same session at the end.
//!
//! That is why reading a STEP file arrives as a function rather than as a
//! second object: `GeometryKernel` is the kernel's contract and `StepImporter`
//! is the document's, and nothing in the shipped graph points from the kernel
//! adapter back at the document. Passing the kernel to the function instead of
//! capturing it is what lets one `&mut` satisfy both.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use ferritecad_document::{
    Document, ImportedDefinitionRef, ObjectPayload, ObjectRecord, StepImporter,
};
use ferritecad_eval::rebuild_cold;
use ferritecad_exchange::{ColourSource, Import, Scene};
use ferritecad_kernel::{
    GeometryKernel, KernelIdentity, OperationContext, ProgressSink, ShapeHandle, TessellationParams,
};
use ferritecad_types::{CadError, ImportedSourceId, ObjectId, Result, Transform};
use ferritecad_viewport::{RenderSnapshot, SnapshotBuilder};
use serde::{Deserialize, Serialize};

/// A picture, and what each part of it is.
///
/// The two halves answer different questions and are kept apart on purpose.
/// [`RenderSnapshot`] is what a GPU draws and holds nothing that outlives the
/// session that made it; the catalogue is what a click *means*, in terms a
/// document can store.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedScene {
    pub snapshot: RenderSnapshot,
    /// What each packed mesh is, indexed the way the snapshot indexes them.
    pub catalogue: Vec<CatalogueEntry>,
}

/// One drawn definition: what it is, and what to call it.
///
/// The two are not the same thing and are not stored the same way. `item` is
/// the identity and is the only part that serialises; everything beside it was
/// read while loading so a person can be told what they chose, and none of it
/// may be matched on. Names are not unique – two bodies may be called the same
/// thing, and two files may each contain a `Part` – so a viewer that found a
/// definition by its name would sometimes find the wrong one.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogueEntry {
    /// What this is, in terms a document could store.
    pub item: SceneItem,
    /// What the document or the file called it. Empty names are dropped
    /// rather than shown as blank, and nothing is invented for the rest.
    pub name: Option<String>,
    /// The file an imported definition came from, by name.
    ///
    /// A name to read, never a path to open: the bytes in the document are the
    /// source, and the place they were read from years ago may hold something
    /// else entirely by now. Reduced to its last component even when the
    /// document recorded more, so a window cannot put somebody's home
    /// directory on screen.
    pub source_file: Option<String>,
    /// How many solids the definition holds, as the importer counted them.
    pub solids: Option<u32>,
}

/// One drawn definition while the load is still going on.
///
/// The same identity can be met more than once: a document may hold two
/// imported objects storing the same bytes, and the document layer gives
/// identical bytes one source identity, so the two objects describe the same
/// definitions. What is drawn must be one definition all the same, and what is
/// said about it must not depend on which object was read first.
#[derive(Debug)]
struct Drawn {
    item: SceneItem,
    name: Fact,
    source_file: Fact,
    solids: Option<u32>,
}

/// What repeated sightings of one display fact add up to.
///
/// Three states rather than two. "Nobody said" and "two sources said
/// different things" are different situations, and collapsing them would let
/// a third sighting refill a fact that had already been found ambiguous.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Fact {
    Unknown,
    Known(String),
    /// Two sightings disagreed. Nothing is shown rather than one of them
    /// chosen by which import happened to be read first: display facts are
    /// not identity, and a window must not present document order as though
    /// it were a decision about which name is right.
    Ambiguous,
}

impl Fact {
    fn seen(&mut self, value: Option<String>) {
        let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
            return;
        };
        *self = match std::mem::replace(self, Self::Unknown) {
            Self::Unknown => Self::Known(value),
            Self::Known(known) if known == value => Self::Known(known),
            Self::Known(_) | Self::Ambiguous => Self::Ambiguous,
        };
    }

    fn into_option(self) -> Option<String> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown | Self::Ambiguous => None,
        }
    }
}

/// What one sighting of a definition said about it.
#[derive(Debug, Default)]
struct Seen {
    name: Option<String>,
    source_file: Option<String>,
    solids: Option<u32>,
}

/// Every definition drawn in one load, one entry per portable identity.
///
/// Canonical across the whole document rather than within one imported
/// object. Two objects storing the same bytes share a source identity, so a
/// definition they both draw is one definition: packing it twice would give it
/// two identities on the GPU, and choosing one of them would highlight half of
/// its placements.
///
/// Keyed by [`SceneItem`] and by nothing else. A name, a file name, a solid
/// count, a position in the file and an object's place in the document are all
/// things two different definitions can share and one definition can be
/// described by differently.
#[derive(Debug, Default)]
struct Catalogue {
    entries: Vec<Drawn>,
    /// Where each identity was packed. Never iterated – it is a lookup, and
    /// what is ordered is the entries beside it.
    packed: HashMap<SceneItem, usize>,
}

impl Catalogue {
    /// The definition this identity is drawn as, packing it if it is new.
    ///
    /// `pack` is called only for an identity nothing has drawn yet, which is
    /// what stops one definition from being tessellated twice merely because
    /// two imported objects both refer to it.
    fn definition(
        &mut self,
        item: SceneItem,
        seen: Seen,
        pack: impl FnOnce() -> Result<usize>,
    ) -> Result<usize> {
        if let Some(&index) = self.packed.get(&item) {
            let entry = &mut self.entries[index];
            entry.name.seen(seen.name);
            entry.source_file.seen(seen.source_file);
            match (entry.solids, seen.solids) {
                (Some(known), Some(now)) if known != now => {
                    // Not two definitions, and not a number to pick between:
                    // one identity has one geometry, so two counts mean the
                    // document and the file disagree about what this is.
                    return Err(CadError::topology(format!(
                        "{item:?} was read as {known} solids and again as {now}; one durable                          definition cannot be two shapes"
                    )));
                }
                (None, now) => entry.solids = now,
                _ => {}
            }
            return Ok(index);
        }

        let index = pack()?;
        if index != self.entries.len() {
            return Err(CadError::topology(format!(
                "a definition was packed as {index} while the catalogue held {}; a click                  resolves through both, so they cannot disagree",
                self.entries.len()
            )));
        }

        let mut entry = Drawn {
            item: item.clone(),
            name: Fact::Unknown,
            source_file: Fact::Unknown,
            solids: seen.solids,
        };
        entry.name.seen(seen.name);
        entry.source_file.seen(seen.source_file);
        self.entries.push(entry);
        self.packed.insert(item, index);
        Ok(index)
    }

    /// What the load is handing over, in the order the snapshot draws it.
    fn finish(self) -> Vec<CatalogueEntry> {
        self.entries
            .into_iter()
            .map(|entry| CatalogueEntry {
                item: entry.item,
                name: entry.name.into_option(),
                source_file: entry.source_file.into_option(),
                solids: entry.solids,
            })
            .collect()
    }
}

/// The last component of whatever provenance the document recorded.
///
/// The field is a hint for a person and nothing opens it, but a document
/// written elsewhere may hold a whole path in it, and a viewport is not the
/// place to display one.
fn file_name_of(recorded: Option<&str>) -> Option<String> {
    let recorded = recorded?.trim();
    if recorded.is_empty() {
        return None;
    }
    let name = recorded
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(recorded)
        .trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// What one drawn definition is, in terms that outlive this reading.
///
/// A pick names a definition inside one snapshot and means nothing outside it
/// – that is what makes it safe to throw away. This is the other half: the
/// same definition said in a way a document could store, so a selection can
/// become something durable without a viewport ever holding a durable name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneItem {
    /// A body of this document, named by the object that holds it.
    Body(ObjectId),
    /// A definition inside an imported file, named by the file and the key
    /// that file gave it. Never by where it sits in the assembly: an
    /// occurrence has only a position, and a position is renumbered by the
    /// next import.
    Imported(ImportedDefinitionRef),
}

/// What every body is drawn in.
///
/// One colour for all of them, because a document records no appearance and
/// inventing one per body would be presenting a decision nobody made as
/// something the file said. Appearance is a document feature that does not
/// exist yet; when it does, it will arrive here as data rather than as a
/// palette.
const BODY_COLOUR: [f64; 3] = [0.62, 0.66, 0.70];

/// Reads a document and builds a picture of it.
///
/// Native bodies and imported scenes, in document order. A body is tessellated
/// once and placed at the origin; an imported scene is re-read from the bytes
/// the document stores, and every definition that is actually drawn is
/// tessellated once however many places it appears in.
///
/// `read_step` is how this asks the kernel to read a stored STEP file again.
/// It takes the kernel rather than holding it, so the same session builds
/// both kinds of geometry; a document with no imports never calls it, which is
/// why a caller with no importer can pass one that refuses.
///
/// Cancellation is checked between objects and between definitions as well as
/// inside the rebuild, so a document whose geometry takes a while can be
/// abandoned without waiting for it to finish.
pub fn snapshot_of<K>(
    path: &Path,
    kernel: &mut K,
    mut read_step: impl FnMut(&mut K, &[u8]) -> Result<Import>,
    params: &TessellationParams,
    context: &OperationContext,
) -> Result<LoadedScene>
where
    K: GeometryKernel + ?Sized,
{
    let document = Document::open_read_only(path)?;

    // Two phases of one job, so they share one scale. Building the geometry
    // is the slow half and gets most of it; the rest is drawing what was
    // built. A bar that reached the end when the rebuild did would sit at
    // "finished" for the whole of the meshing.
    let building = phase(context, 0.0, BUILDING);
    let drawing = phase(context, BUILDING, 1.0);

    // Cold on purpose, as everywhere else a result must be right rather than
    // quick: consulting a cache would make what is on screen depend on the
    // state of a sidecar that exists only to save time.
    let built = rebuild_cold(&document, kernel, &building)?;

    // Handles this function obtained itself, as opposed to the ones the
    // rebuild owns. Filled as it goes so that a failure halfway through an
    // assembly still gives back what had already been read.
    let mut imported: Vec<ShapeHandle> = Vec::new();

    // Everything that can fail happens in here, so that the shapes can be
    // handed back in one place whatever the outcome.
    let snapshot = (|| -> Result<LoadedScene> {
        let mut builder = SnapshotBuilder::new();
        // One catalogue for the whole document, not one per imported object:
        // two objects can store the same bytes, and what they then draw is the
        // same definition.
        let mut catalogue = Catalogue::default();
        let objects = document.objects()?;

        // Counted before anything is drawn, so each one can say what fraction
        // of the drawing it is. An object that draws nothing is not part of
        // the count: the bar would stall on it and then jump.
        let drawable = objects.iter().filter(|object| draws(object)).count();
        let mut done = 0usize;

        for object in objects {
            context.check_cancelled()?;
            // This object's share of the drawing phase. The kernel reports
            // that it finished a mesh, and that report arrives as the part of
            // the load it actually is.
            let scoped = phase(
                &drawing,
                done as f64 / drawable.max(1) as f64,
                (done + 1) as f64 / drawable.max(1) as f64,
            );
            if draws(&object) {
                done += 1;
            }

            match &object.payload {
                ObjectPayload::Body(body) => {
                    // A body with no tip feature is empty by definition rather
                    // than broken: nothing has been built into it yet.
                    if body.tip_feature.is_none() {
                        continue;
                    }
                    let shape = built.shape(object.id).ok_or_else(|| {
                        CadError::topology(format!(
                            "body {} names a feature but the rebuild produced no geometry for it",
                            object.id
                        ))
                    })?;
                    // Two bodies are two definitions however alike they look:
                    // the object that holds one is what it is, and a document
                    // may give two of them one name.
                    let definition = catalogue.definition(
                        SceneItem::Body(object.id),
                        Seen {
                            name: object.name.clone(),
                            ..Seen::default()
                        },
                        || {
                            let mesh = kernel.tessellate(shape, params, &scoped)?;
                            builder.add_mesh(&mesh)
                        },
                    )?;
                    builder.place(definition, None, &Transform::IDENTITY, BODY_COLOUR)?;
                }

                ObjectPayload::ImportedStep(stored) => {
                    // Read from the record already in hand rather than by
                    // asking the document again: the other route would fetch
                    // the whole source blob to learn one short string.
                    let source_file = file_name_of(stored.source_name.as_deref());
                    // Scoped so the borrow ends before the kernel is needed
                    // for meshing. A refusal here releases what it built; what
                    // it returns is this function's to give back.
                    let reopened = {
                        let mut reader = Reader {
                            kernel: &mut *kernel,
                            read: &mut read_step,
                        };
                        document.reopen_step_import(object.id, &mut reader)?
                    };
                    imported.extend(reopened.scene.shapes());
                    draw_scene(
                        &mut builder,
                        &mut catalogue,
                        kernel,
                        Provenance {
                            source: reopened.source(),
                            file: source_file,
                        },
                        &reopened.scene,
                        params,
                        &scoped,
                    )?;
                }

                _ => continue,
            }
        }
        Ok(LoadedScene {
            snapshot: builder.build(),
            catalogue: catalogue.finish(),
        })
    })();

    for shape in imported.into_iter().rev() {
        kernel.release(shape);
    }
    built.release_all(kernel);
    snapshot
}

/// Where an imported scene came from: its identity, and what to call it.
struct Provenance {
    source: ImportedSourceId,
    file: Option<String>,
}

/// Adds an imported scene to the picture being built.
///
/// # Only the leaves carry geometry
///
/// An assembly arrives as both: a definition whose shape is the whole
/// assembly, and separate instances of the parts inside it. Drawing every
/// instance would draw the same solids twice – once through the assembly's own
/// compound and once through each component – so an instance that has children
/// is structure and is not drawn. Its placement still counts: it is what its
/// children sit in.
///
/// # Composed here
///
/// A file records each placement relative to its parent, which is the file's
/// own structure and worth keeping in the document. A picture needs world
/// positions, so the chain is multiplied out once, here, where the tree is
/// still in hand.
fn draw_scene<K: GeometryKernel + ?Sized>(
    builder: &mut SnapshotBuilder,
    catalogue: &mut Catalogue,
    kernel: &mut K,
    from: Provenance,
    scene: &Scene,
    params: &TessellationParams,
    context: &OperationContext,
) -> Result<()> {
    let mut structural = vec![false; scene.instances.len()];
    for (index, instance) in scene.instances.iter().enumerate() {
        let Some(parent) = instance.parent else {
            continue;
        };
        let holds = structural.get_mut(parent).ok_or_else(|| {
            CadError::input(format!(
                "instance {index} sits inside {parent}, which this scene does not have"
            ))
        })?;
        *holds = true;
    }

    // Parents come before children, so one pass composes the whole tree.
    let mut world: Vec<Transform> = Vec::with_capacity(scene.instances.len());
    for (index, instance) in scene.instances.iter().enumerate() {
        let local = placement_of(&instance.placement)?;
        let placed = match instance.parent {
            None => local,
            Some(parent) => {
                let outer = world.get(parent).ok_or_else(|| {
                    CadError::input(format!(
                        "instance {index} sits inside {parent}, which the scene lists after it"
                    ))
                })?;
                local.then(outer)?
            }
        };
        world.push(placed);
    }

    // One imported object can hold many definitions. The caller gives the
    // object one slice of the load; divide that slice among the unique leaf
    // definitions it draws. A definition another object already drew is not
    // meshed again, so an object that adds nothing new advances the count by
    // less than its whole slice. Reusing the object's context for
    // every definition would make progress run from the beginning to the end
    // of the same slice once per part, going backwards between parts and
    // announcing completion more than once.
    let definitions_to_mesh = scene
        .instances
        .iter()
        .enumerate()
        .filter(|(index, _)| !structural[*index])
        .map(|(_, instance)| instance.definition)
        .collect::<BTreeSet<_>>()
        .len();
    let mut definitions_meshed = 0usize;

    for (index, instance) in scene.instances.iter().enumerate() {
        if structural[index] {
            continue;
        }
        context.check_cancelled()?;

        let definition = scene.definitions.get(instance.definition).ok_or_else(|| {
            CadError::input(format!(
                "instance {index} draws definition {}, which this scene does not have",
                instance.definition
            ))
        })?;

        // The file's own name for this definition, kept beside the source it
        // belongs to. `#31` in one file is not `#31` in another, which is why
        // neither half travels alone – and why two objects storing the same
        // bytes name the same definition when they use the same key.
        let item = SceneItem::Imported(ImportedDefinitionRef::new(
            from.source,
            definition.key.clone(),
        )?);
        let seen = Seen {
            name: Some(definition.name.clone()),
            source_file: from.file.clone(),
            solids: Some(definition.solids),
        };

        let mesh = catalogue.definition(item, seen, || {
            let scoped = phase(
                context,
                definitions_meshed as f64 / definitions_to_mesh.max(1) as f64,
                (definitions_meshed + 1) as f64 / definitions_to_mesh.max(1) as f64,
            );
            let mesh = kernel.tessellate(definition.shape, params, &scoped)?;
            definitions_meshed += 1;
            builder.add_mesh(&mesh)
        })?;

        // Linear RGB as the importer read it out of the file. Where the file
        // said nothing, the same neutral colour a native body gets: inventing
        // one per part would present a decision nobody made as something the
        // file recorded.
        // Any source at all means the number beside it came from the file.
        // Written this way rather than by naming the two known sources: a
        // third would be another place a colour can come from, not a reason to
        // stop using it.
        let colour = match instance.colour_source {
            ColourSource::None => BODY_COLOUR,
            _ => instance.colour,
        };
        builder.place(mesh, None, &world[index], colour)?;
    }
    Ok(())
}

/// Whether this object puts anything on screen.
///
/// A body with nothing built into it yet draws nothing, and neither does
/// anything that is not geometry at all.
fn draws(object: &ObjectRecord) -> bool {
    match &object.payload {
        ObjectPayload::Body(body) => body.tip_feature.is_some(),
        ObjectPayload::ImportedStep(_) => true,
        _ => false,
    }
}

/// How much of a load is building geometry rather than drawing it.
///
/// A guess, and the honest kind: nothing here can know the ratio for a
/// particular document, and any number would be wrong for some of them. What
/// it must not do is reach the end before the work does.
const BUILDING: f64 = 0.75;

/// A slice of the whole load, as its own `0..1`.
///
/// The phases below report how far through themselves they are; a caller
/// wants to know how far through the load it is. Composing that here means
/// neither phase has to know what else the load does.
fn phase(context: &OperationContext, from: f64, to: f64) -> OperationContext {
    let outer = context.progress().clone();
    OperationContext::new(context.tolerance())
        .with_cancel(context.cancel().clone())
        .with_progress(ProgressSink::new(move |fraction| {
            outer.report(from + fraction * (to - from));
        }))
}

/// Turns a scene's row-major 3x4 placement into a transform.
fn placement_of(placement: &[f64; 12]) -> Result<Transform> {
    Transform::from_rows([
        [placement[0], placement[1], placement[2], placement[3]],
        [placement[4], placement[5], placement[6], placement[7]],
        [placement[8], placement[9], placement[10], placement[11]],
    ])
}

/// A kernel, behind the contract the document uses to re-read a STEP file.
///
/// Holds the kernel for exactly as long as one reopening takes. Identity and
/// release come from the kernel itself, so the only thing a caller has to
/// supply is how this particular kernel reads STEP bytes.
struct Reader<'a, K: ?Sized, F> {
    kernel: &'a mut K,
    read: F,
}

impl<K, F> StepImporter for Reader<'_, K, F>
where
    K: GeometryKernel + ?Sized,
    F: FnMut(&mut K, &[u8]) -> Result<Import>,
{
    fn identity(&self) -> &KernelIdentity {
        self.kernel.identity()
    }

    fn import(&mut self, source: &[u8]) -> Result<Import> {
        (self.read)(self.kernel, source)
    }

    fn release(&mut self, shape: ShapeHandle) {
        self.kernel.release(shape);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ferritecad_document::StepImportRequest;
    use ferritecad_exchange::{Definition, Instance};
    use ferritecad_kernel::mock::MockKernel;
    use ferritecad_kernel::{
        ArchiveSlot, BrepBlob, CancelToken, ExtrudeRequest, ExtrudeResult, KernelIdentity, Mesh,
        OperationResult, ShapeHandle, SubShapeHandle,
    };
    use ferritecad_types::ObjectId;

    /// The committed plate, copied somewhere the test owns.
    ///
    /// What a caller with no importer passes.
    ///
    /// A document with no imports never asks, so this refusing before it can
    /// do anything is also the check that it never asked.
    fn no_imports<K: ?Sized>(_: &mut K, _: &[u8]) -> Result<Import> {
        Err(CadError::unsupported(
            "this test opened a document that was supposed to hold no imports",
        ))
    }

    /// Copied rather than opened in place because a test that touched the
    /// checkout would be the very thing this crate promises not to do.
    fn plate() -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("plate.fcad");
        std::fs::copy(ferritecad_fixtures::plate_source(), &path).expect("copies the fixture");
        (directory, path)
    }

    /// A document of `count` separate square bodies, ten apart along x.
    ///
    /// The committed plate is one body, which cannot show that a second one is
    /// drawn, ordered or released. This is the smallest document that can.
    fn several_bodies(path: &Path, count: usize) -> Vec<ferritecad_types::ObjectId> {
        use ferritecad_document::{
            Body, DatumPlane, Dependency, DependencyRole, EndCondition, Expression, Extrude,
            Point2, Sketch, SketchCurve, SketchGeometry, SolidOperation,
        };
        use ferritecad_types::{ObjectId, StableEntityId};

        let plane = ObjectId::new();
        let mut bodies = Vec::new();
        let mut document = Document::create(path).expect("creates a document");
        document
            .write(|w| {
                w.put_object(
                    plane,
                    None,
                    0,
                    Some("XY"),
                    &ObjectPayload::DatumPlane(DatumPlane {
                        placement: Transform::IDENTITY,
                    }),
                )?;

                for index in 0..count {
                    let (sketch, extrude, body) =
                        (ObjectId::new(), ObjectId::new(), ObjectId::new());
                    let left = index as f64 * 10.0;
                    let corners = [
                        (left, 0.0),
                        (left + 5.0, 0.0),
                        (left + 5.0, 5.0),
                        (left, 5.0),
                    ];
                    let mut curves = Vec::new();
                    for corner in 0..corners.len() {
                        let (sx, sy) = corners[corner];
                        let (ex, ey) = corners[(corner + 1) % corners.len()];
                        curves.push(SketchCurve {
                            id: StableEntityId::new(),
                            construction: false,
                            geometry: SketchGeometry::Line {
                                start: Point2::new(sx, sy)?,
                                end: Point2::new(ex, ey)?,
                            },
                        });
                    }

                    let ordinal = index as i64 * 3;
                    w.put_object(
                        sketch,
                        None,
                        ordinal + 1,
                        None,
                        &ObjectPayload::Sketch(Sketch { plane, curves }),
                    )?;
                    w.add_dependency(Dependency {
                        dependent: sketch,
                        dependency: plane,
                        role: DependencyRole::Plane,
                    })?;
                    w.put_object(
                        extrude,
                        None,
                        ordinal + 2,
                        None,
                        &ObjectPayload::Extrude(Extrude {
                            profile: sketch,
                            end_condition: EndCondition::Blind {
                                distance: Expression::constant(2.0)?,
                            },
                            reversed: false,
                            operation: SolidOperation::NewBody,
                            target_body: None,
                        }),
                    )?;
                    w.add_dependency(Dependency {
                        dependent: extrude,
                        dependency: sketch,
                        role: DependencyRole::Profile,
                    })?;
                    // Every one of them called the same thing. A document is
                    // entitled to allow that, so the tests here are entitled
                    // to depend on it.
                    w.put_object(
                        body,
                        None,
                        ordinal + 3,
                        Some("Plate"),
                        &ObjectPayload::Body(Body {
                            tip_feature: Some(extrude),
                        }),
                    )?;
                    w.add_dependency(Dependency {
                        dependent: body,
                        dependency: extrude,
                        role: DependencyRole::BodyTip,
                    })?;
                    bodies.push(body);
                }
                Ok(())
            })
            .expect("writes the document");
        bodies
    }

    fn params() -> TessellationParams {
        TessellationParams::new(
            TessellationParams::DEFAULT_LINEAR,
            TessellationParams::DEFAULT_ANGULAR,
            false,
        )
        .expect("the defaults are valid")
    }

    /// One mock solid, so a fabricated scene refers to geometry that exists.
    fn solid(kernel: &mut MockKernel) -> ShapeHandle {
        use ferritecad_kernel::{
            ExtrudeExtent, PlanarPoint, Profile, ProfileLoop, ProfileSegment, SegmentGeometry,
            SketchPlane,
        };
        use ferritecad_types::StableEntityId;

        let corners = [
            PlanarPoint::new(0.0, 0.0),
            PlanarPoint::new(10.0, 0.0),
            PlanarPoint::new(10.0, 10.0),
            PlanarPoint::new(0.0, 10.0),
        ]
        .map(|corner| corner.expect("finite"));
        let segments = corners
            .iter()
            .enumerate()
            .map(|(index, start)| {
                ProfileSegment::new(
                    StableEntityId::new(),
                    SegmentGeometry::line(*start, corners[(index + 1) % corners.len()])
                        .expect("distinct"),
                )
            })
            .collect();
        let profile = Profile::new(
            SketchPlane::world_xy(),
            ProfileLoop::new(segments).expect("closes"),
            Vec::new(),
        )
        .expect("valid");
        let request = ExtrudeRequest::new(
            profile,
            ExtrudeExtent::blind(10.0).expect("positive"),
            false,
        );

        kernel
            .extrude(&request, &OperationContext::default())
            .expect("the mock builds a solid")
            .shape
    }

    fn definition(kernel: &mut MockKernel, name: &str, solids: u32, key: &str) -> Definition {
        Definition {
            shape: solid(kernel),
            name: name.to_owned(),
            solids,
            key: key.to_owned(),
        }
    }

    fn instance(
        definition: usize,
        parent: Option<usize>,
        at: [f64; 3],
        colour_source: ColourSource,
        colour: [f64; 3],
    ) -> Instance {
        Instance {
            definition,
            parent,
            name: String::new(),
            placement: [
                1.0, 0.0, 0.0, at[0], 0.0, 1.0, 0.0, at[1], 0.0, 0.0, 1.0, at[2],
            ],
            colour_source,
            colour,
        }
    }

    /// The shape of `fixtures/step/canonical/03-nested-assembly.step`.
    ///
    /// Measured from the real import rather than imagined: two groups of two
    /// cubes inside an outer group, where every placement is relative to its
    /// parent and the two group definitions carry the whole compound of what
    /// is inside them. That last part is what makes drawing every instance
    /// wrong, so a made-up scene that left it out would agree with a wrong
    /// implementation.
    fn nested_assembly(kernel: &mut MockKernel) -> Scene {
        Scene {
            source_unit: "MILLIMETRE".to_owned(),
            schema: "AP214".to_owned(),
            definitions: vec![
                definition(kernel, "OuterGroup", 4, "step.product_definition#5"),
                definition(kernel, "InnerGroup", 2, "step.product_definition#31"),
                definition(kernel, "Cube", 1, "step.product_definition#58"),
            ],
            instances: vec![
                instance(0, None, [0.0, 0.0, 0.0], ColourSource::None, [0.0; 3]),
                instance(1, Some(0), [0.0, 0.0, 0.0], ColourSource::None, [0.0; 3]),
                instance(
                    2,
                    Some(1),
                    [0.0, 0.0, 0.0],
                    ColourSource::Definition,
                    [0.1, 0.2, 0.3],
                ),
                instance(
                    2,
                    Some(1),
                    [30.0, 0.0, 0.0],
                    ColourSource::Definition,
                    [0.1, 0.2, 0.3],
                ),
                instance(1, Some(0), [0.0, 40.0, 0.0], ColourSource::None, [0.0; 3]),
                instance(
                    2,
                    Some(4),
                    [0.0, 0.0, 0.0],
                    ColourSource::Definition,
                    [0.1, 0.2, 0.3],
                ),
                instance(
                    2,
                    Some(4),
                    [30.0, 0.0, 0.0],
                    ColourSource::Instance,
                    [0.9, 0.1, 0.1],
                ),
            ],
        }
    }

    /// A document holding one stored import of `scene`.
    ///
    /// The bytes are not a STEP file and never need to be: the document stores
    /// whatever it was handed and hands the same back, and what reads them is
    /// the importer this test supplies.
    fn document_with_import(path: &Path, kernel: &mut MockKernel) -> ObjectId {
        let scene = nested_assembly(kernel);
        let import = Import::Imported {
            scene,
            diagnostics: Vec::new(),
        };
        let object = ObjectId::new();
        let mut document = Document::create(path).expect("creates a document");
        document
            .store_step_import(StepImportRequest {
                object,
                name: Some("Assembly"),
                source: SOURCE,
                source_name: Some("03-nested-assembly.step"),
                import: &import,
                importer: kernel.identity(),
            })
            .expect("stores the import");
        for shape in import
            .scene()
            .expect("this import produced a scene")
            .shapes()
        {
            kernel.release(shape);
        }
        object
    }

    const SOURCE: &[u8] = b"ISO-10303-21; this is what the document stores";

    #[test]
    fn a_stored_assembly_is_drawn_once_per_place_it_appears() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("assembly.fcad");
        let mut kernel = MockKernel::new();
        document_with_import(&path, &mut kernel);
        assert_eq!(kernel.live_shape_count(), 0, "the setup kept shapes");

        let loaded = snapshot_of(
            &path,
            &mut kernel,
            // Reading the file again produces the same scene with new handles,
            // which is what a second kernel session does.
            |kernel, source| {
                assert_eq!(source, SOURCE, "the document handed over other bytes");
                Ok(Import::Imported {
                    scene: nested_assembly(kernel),
                    diagnostics: Vec::new(),
                })
            },
            &params(),
            &OperationContext::default(),
        )
        .expect("the stored assembly reopens");
        let snapshot = loaded.snapshot;

        // One mesh: four cubes in four places are one definition, and the two
        // groups are structure. A loader that tessellated every definition
        // would report three, and one that drew every instance would put the
        // whole assembly on screen twice.
        assert_eq!(snapshot.meshes().len(), 1, "definitions were meshed twice");
        assert_eq!(snapshot.draws().len(), 4, "one draw per cube, and no more");
        for item in snapshot.draws() {
            assert_eq!(item.mesh, 0);
        }

        // Where each cube ended up: the inner placement composed with the
        // group it sits in. A loader that ignored the tree would put all four
        // at two positions.
        let mut corners: Vec<[i64; 3]> = snapshot
            .draws()
            .iter()
            .map(|item| {
                // Column-major, so the translation is the last column.
                [
                    item.transform[12].round() as i64,
                    item.transform[13].round() as i64,
                    item.transform[14].round() as i64,
                ]
            })
            .collect();
        corners.sort_unstable();
        assert_eq!(
            corners,
            vec![[0, 0, 0], [0, 40, 0], [30, 0, 0], [30, 40, 0]]
        );

        assert_eq!(
            kernel.live_shape_count(),
            0,
            "the imported shapes were never given back"
        );
    }

    #[test]
    fn every_mesh_says_what_it_is_in_terms_a_document_could_store() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("assembly.fcad");
        let mut kernel = MockKernel::new();
        let object = document_with_import(&path, &mut kernel);

        let loaded = snapshot_of(
            &path,
            &mut kernel,
            |kernel, _| {
                Ok(Import::Imported {
                    scene: nested_assembly(kernel),
                    diagnostics: Vec::new(),
                })
            },
            &params(),
            &OperationContext::default(),
        )
        .expect("the stored assembly reopens");

        // One entry per mesh, and the same index: a click gives a mesh index
        // and this is what turns it into something that outlives the session.
        assert_eq!(loaded.catalogue.len(), loaded.snapshot.meshes().len());

        // Four cubes in four places share one definition, so the four draws
        // resolve to the same catalogue entry. That is what makes selecting
        // one of them select the definition rather than the placement.
        let entries: Vec<&SceneItem> = loaded
            .snapshot
            .draws()
            .iter()
            .map(|item| &loaded.catalogue[item.mesh].item)
            .collect();
        assert_eq!(entries.len(), 4);
        assert!(entries.windows(2).all(|pair| pair[0] == pair[1]));

        // And what it says is the file's own name for that definition, beside
        // the source it belongs to. `#58` in another file is another thing.
        let SceneItem::Imported(reference) = entries[0] else {
            unreachable!("an imported definition was catalogued as a native body")
        };
        assert_eq!(reference.definition_key(), "step.product_definition#58");

        // Beside the identity, what a person needs to recognise it: the file's
        // own name for the definition, the file it came from by name, and how
        // many solids it holds. None of these is matched on – all four cubes
        // are called `Cube`, and so might a definition in another file be.
        let facts = &loaded.catalogue[loaded.snapshot.draws()[0].mesh];
        assert_eq!(facts.name.as_deref(), Some("Cube"));
        assert_eq!(
            facts.source_file.as_deref(),
            Some("03-nested-assembly.step")
        );
        assert_eq!(facts.solids, Some(1));

        // The document that stored the import is the source it names, so a
        // reference taken from a picture resolves in the document it came from.
        let stored = Document::open_read_only(&path).expect("reopens");
        let import = stored
            .step_import(object)
            .expect("reads")
            .expect("the import is there");
        assert_eq!(reference.source(), import.imported.source);

        assert_eq!(kernel.live_shape_count(), 0);
    }

    #[test]
    fn a_native_body_is_catalogued_by_the_object_that_holds_it() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("three.fcad");
        let bodies = several_bodies(&path, 3);

        let mut kernel = MockKernel::new();
        let loaded = snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect("the document loads");

        let named: Vec<SceneItem> = loaded
            .snapshot
            .draws()
            .iter()
            .map(|item| loaded.catalogue[item.mesh].item.clone())
            .collect();
        assert_eq!(
            named,
            bodies.into_iter().map(SceneItem::Body).collect::<Vec<_>>(),
            "the catalogue does not name the bodies that were drawn"
        );

        // A body's display facts are the document's own name for it and
        // nothing else: it came from no file and holds no counted solids.
        let facts = &loaded.catalogue[0];
        assert_eq!(facts.name.as_deref(), Some("Plate"));
        assert_eq!(facts.source_file, None);
        assert_eq!(facts.solids, None);
    }

    /// A document holding two imports whose files reuse the same key.
    ///
    /// Not a contrivance: `#31` is a position in a file, and the corpus's own
    /// `01-single-part.step` and `02-flat-assembly.step` both contain
    /// `step.product_definition#5`.
    fn document_with_two_sources(path: &Path, kernel: &mut MockKernel) -> (ObjectId, ObjectId) {
        let mut document = Document::create(path).expect("creates a document");
        let mut store = |name: &str, bytes: &[u8], part: &str| {
            let import = Import::Imported {
                scene: one_part(kernel, part),
                diagnostics: Vec::new(),
            };
            let object = ObjectId::new();
            document
                .store_step_import(StepImportRequest {
                    object,
                    name: Some(part),
                    source: bytes,
                    source_name: Some(name),
                    import: &import,
                    importer: kernel.identity(),
                })
                .expect("stores the import");
            for shape in import.scene().expect("a scene was stored").shapes() {
                kernel.release(shape);
            }
            object
        };

        // One of them recorded with the whole path it was read from, which a
        // document written by another tool is entitled to do, and which a
        // window must not put on screen.
        let first = store(
            "/home/someone/models/left.step",
            b"ISO-10303-21; the first file",
            "Bracket",
        );
        let second = store("right.step", b"ISO-10303-21; the second file", "Bracket");
        (first, second)
    }

    /// One definition, one placement, under a key every file numbers alike.
    fn one_part(kernel: &mut MockKernel, name: &str) -> Scene {
        Scene {
            source_unit: "MILLIMETRE".to_owned(),
            schema: "AP214".to_owned(),
            definitions: vec![definition(kernel, name, 1, "step.product_definition#5")],
            instances: vec![instance(
                0,
                None,
                [0.0, 0.0, 0.0],
                ColourSource::None,
                [0.0; 3],
            )],
        }
    }

    /// Two imported objects storing exactly the same bytes.
    ///
    /// The document layer gives identical bytes one source identity, so the
    /// two objects draw the same definitions – the case a viewer meets when
    /// somebody imports the same file twice.
    fn document_with_the_same_file_twice(
        path: &Path,
        kernel: &mut MockKernel,
        recorded_as: [&str; 2],
    ) -> [ObjectId; 2] {
        let mut document = Document::create(path).expect("creates a document");
        let mut objects = Vec::new();
        for name in recorded_as {
            let import = Import::Imported {
                scene: one_part(kernel, "Bracket"),
                diagnostics: Vec::new(),
            };
            let object = ObjectId::new();
            document
                .store_step_import(StepImportRequest {
                    object,
                    name: Some("Imported"),
                    source: SOURCE,
                    source_name: Some(name),
                    import: &import,
                    importer: kernel.identity(),
                })
                .expect("stores the import");
            for shape in import.scene().expect("a scene was stored").shapes() {
                kernel.release(shape);
            }
            objects.push(object);
        }
        [objects[0], objects[1]]
    }

    #[test]
    fn one_definition_stored_twice_is_drawn_once_and_placed_twice() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("twice.fcad");
        let mut kernel = MockKernel::new();
        let objects = document_with_the_same_file_twice(&path, &mut kernel, ["part.step"; 2]);

        // The document really did give both objects one source: that is what
        // makes their definitions one definition rather than two alike.
        let stored = Document::open_read_only(&path).expect("reopens");
        let sources: Vec<_> = objects
            .iter()
            .map(|object| {
                stored
                    .step_import(*object)
                    .expect("reads")
                    .expect("the import is there")
                    .imported
                    .source
            })
            .collect();
        assert_eq!(sources[0], sources[1], "identical bytes were stored twice");
        drop(stored);

        let loaded = snapshot_of(
            &path,
            &mut kernel,
            |kernel, _| {
                Ok(Import::Imported {
                    scene: one_part(kernel, "Bracket"),
                    diagnostics: Vec::new(),
                })
            },
            &params(),
            &OperationContext::default(),
        )
        .expect("both imports reopen");

        // One identity, one packed mesh, one catalogue entry – and both
        // placements, because each object still contributes its own.
        assert_eq!(loaded.catalogue.len(), 1, "one definition was packed twice");
        assert_eq!(loaded.snapshot.meshes().len(), 1);
        assert_eq!(loaded.snapshot.draws().len(), 2, "an occurrence was lost");

        // And every placement is the same definition, so choosing any of them
        // chooses all of them: the picture cannot highlight half of it.
        let picks: Vec<_> = loaded
            .snapshot
            .draws()
            .iter()
            .map(|draw| draw.pick)
            .collect();
        assert_eq!(
            picks[0], picks[1],
            "two placements of one definition differ"
        );
        assert_eq!(loaded.snapshot.definition(picks[0]), Some(0));

        assert_eq!(kernel.live_shape_count(), 0);
    }

    #[test]
    fn the_same_bytes_recorded_under_two_names_are_still_one_definition() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("twice.fcad");
        let mut kernel = MockKernel::new();
        document_with_the_same_file_twice(&path, &mut kernel, ["left.step", "right.step"]);

        let loaded = snapshot_of(
            &path,
            &mut kernel,
            |kernel, _| {
                Ok(Import::Imported {
                    scene: one_part(kernel, "Bracket"),
                    diagnostics: Vec::new(),
                })
            },
            &params(),
            &OperationContext::default(),
        )
        .expect("both imports reopen");

        // The bytes decide, not what somebody called the file when they
        // imported it. One identity, one entry, both placements.
        assert_eq!(loaded.catalogue.len(), 1);
        assert_eq!(loaded.snapshot.draws().len(), 2);

        // Two file names for one identity is a disagreement, and neither is
        // more right than the other. Nothing is shown rather than whichever
        // object happened to be read first.
        assert_eq!(
            loaded.catalogue[0].source_file, None,
            "one of two recorded names was presented as the answer"
        );
        // What both sightings agreed on is still said.
        assert_eq!(loaded.catalogue[0].name.as_deref(), Some("Bracket"));
        assert_eq!(loaded.catalogue[0].solids, Some(1));
    }

    #[test]
    fn a_definition_two_objects_share_is_meshed_once() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("twice.fcad");
        let mut setup = MockKernel::new();
        document_with_the_same_file_twice(&path, &mut setup, ["part.step"; 2]);

        // A kernel that answers every question and counts them. Packing once
        // is not the same as meshing once: a loader could tessellate twice and
        // then throw one away, and only the count would say so.
        let mut kernel = StopsAfterBuilding::new(Stop::Nothing);
        let loaded = snapshot_of(
            &path,
            &mut kernel,
            |kernel, _| {
                Ok(Import::Imported {
                    scene: one_part(&mut kernel.inner, "Bracket"),
                    diagnostics: Vec::new(),
                })
            },
            &params(),
            &OperationContext::default(),
        )
        .expect("both imports reopen");

        assert_eq!(loaded.snapshot.draws().len(), 2);
        assert_eq!(
            kernel.meshed, 1,
            "one definition was tessellated {} times because two objects refer to it",
            kernel.meshed
        );
        assert_eq!(
            kernel.inner.live_shape_count(),
            0,
            "the shapes of the second reading were never given back"
        );
    }

    #[test]
    fn one_key_in_two_files_names_two_different_things() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("two-sources.fcad");
        let mut kernel = MockKernel::new();
        document_with_two_sources(&path, &mut kernel);

        let loaded = snapshot_of(
            &path,
            &mut kernel,
            |kernel, _| {
                // Both files call their part `Bracket` and number it `#5`,
                // exactly as two real files may. Nothing here distinguishes
                // them, which is the point: only the source does.
                Ok(Import::Imported {
                    scene: one_part(kernel, "Bracket"),
                    diagnostics: Vec::new(),
                })
            },
            &params(),
            &OperationContext::default(),
        )
        .expect("both imports reopen");

        assert_eq!(loaded.catalogue.len(), 2, "two files, two definitions");
        let keys: Vec<&str> = loaded
            .catalogue
            .iter()
            .map(|entry| match &entry.item {
                SceneItem::Imported(reference) => reference.definition_key(),
                SceneItem::Body(_) => unreachable!("both entries came from a file"),
            })
            .collect();
        assert_eq!(keys[0], keys[1], "the two files really do reuse one key");

        // And the entries are still different things, because a key belongs to
        // the file that issued it. Nothing here compares names: both parts are
        // called `Bracket`, which is legal and says nothing.
        assert_ne!(
            loaded.catalogue[0].item, loaded.catalogue[1].item,
            "one key in two files was taken for one definition"
        );
        assert_eq!(loaded.catalogue[0].name, loaded.catalogue[1].name);
        assert_eq!(
            loaded.catalogue[0].source_file.as_deref(),
            Some("left.step")
        );
        assert_eq!(
            loaded.catalogue[1].source_file.as_deref(),
            Some("right.step")
        );

        assert_eq!(kernel.live_shape_count(), 0);
    }

    #[test]
    fn two_bodies_may_share_a_name_and_are_still_two_bodies() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("twins.fcad");
        let bodies = several_bodies(&path, 2);

        assert_eq!(bodies.len(), 2);

        let mut kernel = MockKernel::new();
        let loaded = snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect("the document loads");

        assert_eq!(
            loaded.catalogue[0].name.as_deref(),
            Some("Plate"),
            "the name the document gave it was not carried"
        );
        assert_eq!(loaded.catalogue[0].name, loaded.catalogue[1].name);
        assert_ne!(
            loaded.catalogue[0].item, loaded.catalogue[1].item,
            "two bodies with one name were taken for one body"
        );
    }

    /// One identity seen twice, with whatever each sighting said about it.
    fn twice(first: Seen, second: Seen) -> Result<Vec<CatalogueEntry>> {
        let item = SceneItem::Body(ObjectId::new());
        let mut catalogue = Catalogue::default();
        catalogue.definition(item.clone(), first, || Ok(0))?;
        catalogue.definition(item, second, || {
            unreachable!("an identity already drawn was packed a second time")
        })?;
        Ok(catalogue.finish())
    }

    #[test]
    fn a_fact_nobody_gave_is_filled_by_whoever_did() {
        let entries = twice(
            Seen {
                name: None,
                source_file: Some("part.step".to_owned()),
                solids: None,
            },
            Seen {
                name: Some("Bracket".to_owned()),
                source_file: None,
                solids: Some(2),
            },
        )
        .expect("two sightings of one definition");

        // Neither sighting is preferred; between them they said three things,
        // and all three are known.
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name.as_deref(), Some("Bracket"));
        assert_eq!(entries[0].source_file.as_deref(), Some("part.step"));
        assert_eq!(entries[0].solids, Some(2));
    }

    #[test]
    fn two_answers_to_one_question_are_no_answer() {
        let entries = twice(
            Seen {
                name: Some("Bracket".to_owned()),
                source_file: Some("left.step".to_owned()),
                solids: Some(1),
            },
            Seen {
                name: Some("Support".to_owned()),
                source_file: Some("right.step".to_owned()),
                solids: Some(1),
            },
        )
        .expect("two sightings of one definition");

        // One identity described two ways. Showing either would be presenting
        // document order as a decision about which description is right, and
        // splitting the identity would be worse: they are the same definition.
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, None);
        assert_eq!(entries[0].source_file, None);
        assert_eq!(entries[0].solids, Some(1));
    }

    #[test]
    fn a_third_sighting_does_not_settle_what_two_left_open() {
        let item = SceneItem::Body(ObjectId::new());
        let mut catalogue = Catalogue::default();
        let named = |name: &str| Seen {
            name: Some(name.to_owned()),
            ..Seen::default()
        };

        catalogue
            .definition(item.clone(), named("Bracket"), || Ok(0))
            .expect("packs");
        catalogue
            .definition(item.clone(), named("Support"), || Ok(0))
            .expect("merges");
        catalogue
            .definition(item, named("Bracket"), || Ok(0))
            .expect("merges");

        // Two names disagreed, so the question is settled as unanswerable. A
        // third voice does not carry it: "nobody said" and "they disagreed"
        // are different states, and only the first can still be filled in.
        assert_eq!(catalogue.finish()[0].name, None);
    }

    #[test]
    fn one_identity_cannot_be_two_shapes() {
        let error = twice(
            Seen {
                solids: Some(1),
                ..Seen::default()
            },
            Seen {
                solids: Some(4),
                ..Seen::default()
            },
        )
        .expect_err("two solid counts for one definition is not a thing to average");

        // Not two definitions, and not a number to choose between: a durable
        // identity names one shape, so this is the document and the file
        // disagreeing about what that shape is.
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Topology);
        assert!(
            error.to_string().contains("cannot be two shapes"),
            "{error}"
        );
    }

    #[test]
    fn a_catalogue_that_lost_step_with_the_picture_refuses() {
        let mut catalogue = Catalogue::default();

        // The catalogue is indexed the way the snapshot is, and a click is
        // resolved through both. A packer that returned some other index would
        // make a click mean whatever happened to sit there.
        let error = catalogue
            .definition(SceneItem::Body(ObjectId::new()), Seen::default(), || Ok(3))
            .expect_err("a definition packed out of step must not be catalogued");
        assert!(error.to_string().contains("cannot disagree"), "{error}");
    }

    #[test]
    fn what_a_click_means_is_durable_and_what_it_was_is_not() {
        // The catalogue survives being written down, which is the whole point
        // of it: a selection becomes something a document can hold. Its
        // companion cannot – a `PickId` is bound to one snapshot's identity
        // and implements no serialisation at all, so there is no accident by
        // which a click's transient half could be stored beside its durable
        // one.
        let entry = SceneItem::Body(ObjectId::new());
        let written = serde_json::to_string(&entry).expect("a catalogue entry can be written down");
        let read: SceneItem = serde_json::from_str(&written).expect("and read back");
        assert_eq!(read, entry);
    }

    #[test]
    fn what_the_file_said_about_colour_is_what_is_drawn() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("assembly.fcad");
        let mut kernel = MockKernel::new();
        document_with_import(&path, &mut kernel);

        let loaded = snapshot_of(
            &path,
            &mut kernel,
            |kernel, _| {
                Ok(Import::Imported {
                    scene: nested_assembly(kernel),
                    diagnostics: Vec::new(),
                })
            },
            &params(),
            &OperationContext::default(),
        )
        .expect("the stored assembly reopens");
        let snapshot = loaded.snapshot;

        // Three cubes take their definition's colour and one is painted over
        // it. Linear RGB, straight from the file: converting here would guess
        // at a transfer function the importer deliberately did not apply.
        let mut colours: Vec<[u32; 3]> = snapshot
            .draws()
            .iter()
            .map(|item| {
                [
                    (item.colour[0] * 1000.0).round() as u32,
                    (item.colour[1] * 1000.0).round() as u32,
                    (item.colour[2] * 1000.0).round() as u32,
                ]
            })
            .collect();
        colours.sort_unstable();
        assert_eq!(
            colours,
            vec![
                [100, 200, 300],
                [100, 200, 300],
                [100, 200, 300],
                [900, 100, 100]
            ]
        );
        for item in snapshot.draws() {
            assert_eq!(item.colour[3], 1.0, "an imported part is not transparent");
        }
    }

    #[test]
    fn an_import_that_cannot_be_reopened_keeps_nothing() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("assembly.fcad");
        let mut kernel = MockKernel::new();
        document_with_import(&path, &mut kernel);

        // The file reads, and describes something else. Binding refuses, and
        // the shapes it built are the importer's to take back – which is the
        // document's contract, checked here because this is the caller that
        // would otherwise be holding them.
        let error = snapshot_of(
            &path,
            &mut kernel,
            |kernel, _| {
                let mut scene = nested_assembly(kernel);
                scene.definitions[2].name = "Cuboid".to_owned();
                Ok(Import::Imported {
                    scene,
                    diagnostics: Vec::new(),
                })
            },
            &params(),
            &OperationContext::default(),
        )
        .expect_err("a scene that is not what was stored must be refused");
        assert!(error.to_string().contains("Cuboid"), "{error}");
        assert_eq!(kernel.live_shape_count(), 0);

        // And a reading that fails outright.
        let error = snapshot_of(
            &path,
            &mut kernel,
            |_, _| Err(CadError::kernel("the file could not be read again")),
            &params(),
            &OperationContext::default(),
        )
        .expect_err("a reading that failed is not a picture");
        assert!(error.to_string().contains("could not be read again"));
        assert_eq!(kernel.live_shape_count(), 0);
    }

    #[test]
    fn cancelling_between_the_parts_of_an_assembly_gives_them_all_back() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("assembly.fcad");
        let mut kernel = MockKernel::new();
        document_with_import(&path, &mut kernel);

        // Cancelled the moment the scene has been read and bound, with every
        // imported solid live and none of them drawn yet.
        let token = CancelToken::new();
        let context = OperationContext::default().with_cancel(token.clone());
        let error = snapshot_of(
            &path,
            &mut kernel,
            |kernel, _| {
                let scene = nested_assembly(kernel);
                token.cancel();
                Ok(Import::Imported {
                    scene,
                    diagnostics: Vec::new(),
                })
            },
            &params(),
            &context,
        )
        .expect_err("a cancelled load must not produce a picture");

        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Cancellation);
        assert_eq!(
            kernel.live_shape_count(),
            0,
            "cancelling left the imported assembly in the session"
        );
    }

    #[test]
    fn the_committed_plate_becomes_something_to_draw() {
        let (_directory, path) = plate();
        let mut kernel = MockKernel::new();

        let loaded = snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect("the plate loads");
        let snapshot = loaded.snapshot;

        assert_eq!(snapshot.meshes().len(), 1, "the plate is one body");
        assert_eq!(snapshot.draws().len(), 1);
        assert!(snapshot.meshes()[0].triangle_count() > 0, "it has no faces");

        // 60 x 40 x 10, which is what the fixture is and what a viewer must
        // frame. Checked here so a loader that dropped the placement or the
        // extrusion height would not merely produce fewer triangles.
        let (min, max) = snapshot.bounds().expect("something is drawn");
        let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
        assert!((size[0] - 60.0).abs() < 1e-3, "{size:?}");
        assert!((size[1] - 40.0).abs() < 1e-3, "{size:?}");
        assert!((size[2] - 10.0).abs() < 1e-3, "{size:?}");
    }

    #[test]
    fn a_loader_accepts_the_kernel_contract_without_knowing_its_implementation() {
        let (_directory, path) = plate();
        let mut implementation = MockKernel::new();
        let kernel: &mut dyn GeometryKernel = &mut implementation;

        let loaded = snapshot_of(
            &path,
            kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect("the contract is enough to load a native document");

        assert_eq!(loaded.snapshot.meshes().len(), 1);
        assert_eq!(implementation.live_shape_count(), 0);
    }

    #[test]
    fn a_load_reports_how_far_through_it_is() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("three.fcad");
        several_bodies(&path, 3);

        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = std::sync::Arc::clone(&seen);
        let context = OperationContext::default().with_progress(
            ferritecad_kernel::ProgressSink::new(move |fraction| {
                record
                    .lock()
                    .expect("no test thread panicked")
                    .push(fraction);
            }),
        );

        let mut kernel = MockKernel::new();
        snapshot_of(&path, &mut kernel, no_imports, &params(), &context)
            .expect("the document loads");

        let seen = seen.lock().expect("no test thread panicked").clone();
        assert!(!seen.is_empty(), "a load reported no progress at all");
        assert!(
            seen.windows(2).all(|pair| pair[0] <= pair[1]),
            "progress went backwards: {seen:?}"
        );
        assert!(
            seen.iter().all(|fraction| (0.0..=1.0).contains(fraction)),
            "progress left the scale: {seen:?}"
        );

        // The two halves are one scale. Building reports below the split and
        // drawing above it, so a bar fed from this does not reach the end
        // while the model is still being meshed.
        assert!(
            seen.iter().any(|fraction| *fraction < BUILDING),
            "nothing was reported while the geometry was being built: {seen:?}"
        );
        assert!(
            seen.iter().any(|fraction| *fraction > BUILDING),
            "nothing was reported while the model was being drawn: {seen:?}"
        );

        // And it ends at the end, once. Three bodies, each reporting when its
        // own mesh is done: a loader that passed the kernel's own numbers
        // through would announce a finished load three times, the first of
        // them with two thirds of the drawing still to do.
        let finished = seen.iter().filter(|fraction| **fraction >= 1.0).count();
        assert_eq!(
            finished, 1,
            "the load reported itself finished {finished} times: {seen:?}"
        );
        let last = seen.last().copied().expect("something was reported");
        assert!((last - 1.0).abs() < 1e-6, "a finished load reported {last}");
    }

    #[test]
    fn an_imported_assembly_reports_one_monotonic_drawing_phase() {
        let mut kernel = MockKernel::new();
        let scene = Scene {
            source_unit: "MILLIMETRE".to_owned(),
            schema: "AP214".to_owned(),
            definitions: vec![
                definition(&mut kernel, "Assembly", 2, "step.product_definition#1"),
                definition(&mut kernel, "First", 1, "step.product_definition#2"),
                definition(&mut kernel, "Second", 1, "step.product_definition#3"),
            ],
            instances: vec![
                instance(0, None, [0.0, 0.0, 0.0], ColourSource::None, [0.0; 3]),
                instance(
                    1,
                    Some(0),
                    [0.0, 0.0, 0.0],
                    ColourSource::Definition,
                    [0.1, 0.2, 0.3],
                ),
                instance(
                    2,
                    Some(0),
                    [20.0, 0.0, 0.0],
                    ColourSource::Definition,
                    [0.3, 0.2, 0.1],
                ),
            ],
        };

        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = std::sync::Arc::clone(&seen);
        let context = OperationContext::default().with_progress(
            ferritecad_kernel::ProgressSink::new(move |fraction| {
                record
                    .lock()
                    .expect("no test thread panicked")
                    .push(fraction);
            }),
        );

        let mut builder = SnapshotBuilder::new();
        let mut catalogue = Catalogue::default();
        draw_scene(
            &mut builder,
            &mut catalogue,
            &mut kernel,
            Provenance {
                source: ImportedSourceId::new(),
                file: None,
            },
            &scene,
            &params(),
            &context,
        )
        .expect("the assembly draws");
        let snapshot = builder.build();
        assert_eq!(snapshot.meshes().len(), 2, "a leaf definition was missed");

        let seen = seen.lock().expect("no test thread panicked").clone();
        assert!(
            seen.windows(2).all(|pair| pair[0] <= pair[1]),
            "progress went backwards between imported definitions: {seen:?}"
        );
        assert_eq!(
            seen.iter().filter(|fraction| **fraction >= 1.0).count(),
            1,
            "the imported object announced completion once per definition: {seen:?}"
        );
        assert!(
            seen.iter().any(|fraction| (0.0..1.0).contains(fraction)),
            "the first definition consumed the whole drawing phase: {seen:?}"
        );

        for shape in scene.shapes() {
            kernel.release(shape);
        }
        assert_eq!(kernel.live_shape_count(), 0, "the test kept its shapes");
    }

    #[test]
    fn every_definition_can_be_named_and_told_apart() {
        let (_directory, path) = plate();
        let mut kernel = MockKernel::new();
        let loaded = snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect("the plate loads");
        let snapshot = loaded.snapshot;

        // The viewport's own rule, met here rather than discovered on the GPU.
        for (index, item) in snapshot.draws().iter().enumerate() {
            assert_eq!(
                snapshot.definition(item.pick),
                Some(item.mesh),
                "draw {index} picks something other than what it draws"
            );
        }
    }

    #[test]
    fn looking_at_a_document_does_not_change_it() {
        let (_directory, path) = plate();
        let before = std::fs::read(&path).expect("reads");

        let mut kernel = MockKernel::new();
        snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect("the plate loads");

        // Byte for byte. `Document::open` would have migrated the schema and
        // set persistent pragmas, and either would be an edit to a file the
        // user only asked to look at.
        assert_eq!(std::fs::read(&path).expect("reads"), before);
        assert!(
            !path.with_extension("fcad-cache").exists(),
            "looking at a document left a cache sidecar beside it"
        );
    }

    #[test]
    fn a_document_another_program_left_in_wal_mode_is_refused_untouched() {
        let (_directory, path) = plate();
        {
            let connection = rusqlite::Connection::open(&path).expect("opens the copy");
            let mode: String = connection
                .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
                .expect("switches journal mode");
            assert_eq!(mode, "wal");
        }
        let before = std::fs::read(&path).expect("reads");

        let mut kernel = MockKernel::new();
        let error = snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect_err("a WAL document must be refused rather than converted");
        assert!(
            error.to_string().contains("WAL"),
            "the refusal does not say what is wrong: {error}"
        );

        // The point of refusing. Reading a document must never rewrite its
        // journal mode or leave `-wal` and `-shm` beside it: that is an edit to
        // a file the user only asked to look at, and it happens behind a
        // program that may still have the document open.
        assert_eq!(std::fs::read(&path).expect("reads"), before);
        for sidecar in ["fcad-wal", "fcad-shm", "fcad-cache"] {
            assert!(
                !path.with_extension(sidecar).exists(),
                "looking at a document left a .{sidecar} beside it"
            );
        }
    }

    #[test]
    fn a_load_that_succeeds_keeps_no_shapes() {
        let (_directory, path) = plate();
        let mut kernel = MockKernel::new();

        snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect("the plate loads");
        assert_eq!(
            kernel.live_shape_count(),
            0,
            "the snapshot is packed and the shapes were still kept"
        );
    }

    #[test]
    fn a_load_that_fails_keeps_no_shapes_either() {
        let (directory, _) = plate();
        let mut kernel = MockKernel::new();

        // A file that is not a document at all: nothing is built, so nothing
        // can be leaked, but the path has to be exercised to say so.
        let rubbish = directory.path().join("not-a-document.fcad");
        std::fs::write(&rubbish, b"this is not a SQLite file").expect("writes");
        assert!(
            snapshot_of(
                &rubbish,
                &mut kernel,
                no_imports,
                &params(),
                &OperationContext::default()
            )
            .is_err()
        );
        assert_eq!(kernel.live_shape_count(), 0);

        // And one that does not exist.
        assert!(
            snapshot_of(
                &directory.path().join("absent.fcad"),
                &mut kernel,
                no_imports,
                &params(),
                &OperationContext::default()
            )
            .is_err()
        );
        assert_eq!(kernel.live_shape_count(), 0);
    }

    #[test]
    fn cancelling_before_anything_is_built_produces_no_picture() {
        let (_directory, path) = plate();
        let mut kernel = MockKernel::new();

        let token = CancelToken::new();
        token.cancel();
        let context = OperationContext::default().with_cancel(token);

        let error = snapshot_of(&path, &mut kernel, no_imports, &params(), &context)
            .expect_err("a cancelled load must not produce a picture");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Cancellation);
        assert_eq!(kernel.live_shape_count(), 0);
    }

    /// What makes a kernel stop once the geometry already exists.
    enum Stop {
        /// Nothing at all: the kernel answers, and the count is the point.
        Nothing,
        /// The user changed their mind while the model was being meshed.
        Cancelled(CancelToken),
        /// Meshing itself failed.
        Failed,
        /// The user changed their mind between one body and the next, and the
        /// kernel noticed nothing: it was asked for one mesh and gave one.
        BetweenBodies(CancelToken),
    }

    /// A kernel that lets the rebuild finish and then refuses.
    ///
    /// This is the only arrangement in which a leak is possible at all. A
    /// loader that released shapes only on its way out of a successful load
    /// would pass every other test in this file: before the rebuild there is
    /// nothing to leak, and after the snapshot is packed there is nothing left
    /// to go wrong.
    struct StopsAfterBuilding {
        inner: MockKernel,
        stop: Stop,
        /// How many times a mesh was asked for, which is how a definition
        /// tessellated twice is told from one packed twice.
        meshed: usize,
    }

    impl StopsAfterBuilding {
        fn new(stop: Stop) -> Self {
            Self {
                inner: MockKernel::new(),
                stop,
                meshed: 0,
            }
        }
    }

    impl GeometryKernel for StopsAfterBuilding {
        fn identity(&self) -> &KernelIdentity {
            self.inner.identity()
        }

        fn extrude(
            &mut self,
            request: &ExtrudeRequest,
            context: &OperationContext,
        ) -> Result<ExtrudeResult> {
            self.inner.extrude(request, context)
        }

        fn transform(
            &mut self,
            shape: ShapeHandle,
            transform: &Transform,
            context: &OperationContext,
        ) -> Result<OperationResult> {
            self.inner.transform(shape, transform, context)
        }

        fn tessellate(
            &mut self,
            shape: ShapeHandle,
            params: &TessellationParams,
            context: &OperationContext,
        ) -> Result<Mesh> {
            self.meshed += 1;
            match &self.stop {
                Stop::Nothing => self.inner.tessellate(shape, params, context),
                Stop::Cancelled(token) => {
                    // Cancelled at the moment the picture was about to be
                    // built, with every solid of the model live.
                    token.cancel();
                    Err(ferritecad_types::CadError::Cancelled)
                }
                Stop::Failed => Err(CadError::topology("this shape cannot be meshed")),
                Stop::BetweenBodies(token) => {
                    // Deliberately meshed under a context that is not
                    // cancelled: the kernel contract says cancelling is a
                    // request, and that some algorithms finish the unit of work
                    // they are in. Such a kernel is conforming, and answering
                    // one more question correctly must not turn into meshing
                    // the rest of a model nobody is waiting for.
                    let uninterrupted = OperationContext::new(context.tolerance());
                    let mesh = self.inner.tessellate(shape, params, &uninterrupted)?;
                    token.cancel();
                    Ok(mesh)
                }
            }
        }

        fn encode_shape_with(
            &mut self,
            shape: ShapeHandle,
            sub_shapes: &[SubShapeHandle],
        ) -> Result<(BrepBlob, Vec<ArchiveSlot>)> {
            self.inner.encode_shape_with(shape, sub_shapes)
        }

        fn decode_shape_with(
            &mut self,
            blob: &BrepBlob,
            slots: &[ArchiveSlot],
        ) -> Result<(ShapeHandle, Vec<SubShapeHandle>)> {
            self.inner.decode_shape_with(blob, slots)
        }

        fn encode_shape(&mut self, shape: ShapeHandle) -> Result<BrepBlob> {
            self.inner.encode_shape(shape)
        }

        fn decode_shape(&mut self, blob: &BrepBlob) -> Result<ShapeHandle> {
            self.inner.decode_shape(blob)
        }

        fn release(&mut self, shape: ShapeHandle) {
            self.inner.release(shape);
        }
    }

    #[test]
    fn cancelling_after_the_model_is_built_gives_every_shape_back() {
        let (_directory, path) = plate();
        let token = CancelToken::new();
        let mut kernel = StopsAfterBuilding::new(Stop::Cancelled(token.clone()));
        let context = OperationContext::default().with_cancel(token);

        let error = snapshot_of(&path, &mut kernel, no_imports, &params(), &context)
            .expect_err("a cancelled load must not produce a picture");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Cancellation);

        // The model really was built first, so there was something to leak.
        assert!(kernel.inner.extrude_count() > 0, "nothing was ever built");
        assert_eq!(
            kernel.inner.live_shape_count(),
            0,
            "cancelling left the session holding solids"
        );
    }

    #[test]
    fn failing_after_the_model_is_built_gives_every_shape_back() {
        let (_directory, path) = plate();
        let mut kernel = StopsAfterBuilding::new(Stop::Failed);

        let error = snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect_err("meshing failed, so there is no picture");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Topology);

        assert!(kernel.inner.extrude_count() > 0, "nothing was ever built");
        assert_eq!(
            kernel.inner.live_shape_count(),
            0,
            "a failed load left the session holding solids"
        );
    }

    #[test]
    fn every_body_becomes_its_own_drawing_in_document_order() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("three.fcad");
        let bodies = several_bodies(&path, 3);
        assert_eq!(bodies.len(), 3);

        let mut kernel = MockKernel::new();
        let loaded = snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect("the document loads");
        let snapshot = loaded.snapshot;

        assert_eq!(snapshot.meshes().len(), 3, "bodies were merged or dropped");
        assert_eq!(snapshot.draws().len(), 3);

        // Each draw names its own mesh, and the order follows the document
        // rather than whatever order the rebuild happened to finish in.
        let named: Vec<usize> = snapshot.draws().iter().map(|item| item.mesh).collect();
        assert_eq!(named, vec![0, 1, 2]);

        // And they really are three separate squares ten apart, not one square
        // drawn three times: the whole thing is 25 wide.
        let (min, max) = snapshot.bounds().expect("something is drawn");
        assert!((max[0] - min[0] - 25.0).abs() < 1e-3, "{min:?} {max:?}");
    }

    #[test]
    fn a_body_with_nothing_in_it_yet_is_not_a_failure() {
        use ferritecad_document::{Body, DatumPlane};
        use ferritecad_types::ObjectId;

        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("empty-body.fcad");
        let mut document = Document::create(&path).expect("creates a document");
        document
            .write(|w| {
                w.put_object(
                    ObjectId::new(),
                    None,
                    0,
                    Some("XY"),
                    &ObjectPayload::DatumPlane(DatumPlane {
                        placement: Transform::IDENTITY,
                    }),
                )?;
                w.put_object(
                    ObjectId::new(),
                    None,
                    1,
                    Some("Body1"),
                    &ObjectPayload::Body(Body { tip_feature: None }),
                )
            })
            .expect("writes the document");
        drop(document);

        // A body nothing has been built into is empty, not broken, and a
        // viewer that refused to open such a document would refuse the first
        // document anyone makes.
        let mut kernel = MockKernel::new();
        let loaded = snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &params(),
            &OperationContext::default(),
        )
        .expect("a document with an empty body still opens");
        let snapshot = loaded.snapshot;
        assert!(snapshot.draws().is_empty());
        assert!(snapshot.bounds().is_none(), "empty geometry has no extent");
    }

    #[test]
    fn cancelling_between_two_bodies_gives_every_shape_back() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("two.fcad");
        several_bodies(&path, 2);

        // The kernel answers every question it is asked, correctly: the first
        // mesh comes back whole. Only the loader is in a position to notice
        // that the user has since asked it to stop, and it must, rather than
        // meshing the rest of a model nobody is waiting for.
        let token = CancelToken::new();
        let mut kernel = StopsAfterBuilding::new(Stop::BetweenBodies(token.clone()));
        let context = OperationContext::default().with_cancel(token);

        let error = snapshot_of(&path, &mut kernel, no_imports, &params(), &context)
            .expect_err("a cancelled load must not produce a picture");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Cancellation);
        assert_eq!(
            kernel.inner.live_shape_count(),
            0,
            "cancelling between bodies left the session holding solids"
        );
    }

    #[test]
    fn provenance_reaches_the_viewer_as_a_file_name_on_either_platform() {
        assert_eq!(
            file_name_of(Some("/home/someone/models/plate.step")).as_deref(),
            Some("plate.step")
        );
        assert_eq!(
            file_name_of(Some(r"C:\Users\Someone\Models\plate.step")).as_deref(),
            Some("plate.step")
        );
        assert_eq!(file_name_of(Some("  ")), None);
        assert_eq!(file_name_of(None), None);
    }
}
