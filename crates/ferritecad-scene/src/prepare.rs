// SPDX-License-Identifier: MIT
//! The one way a stored document becomes geometry.
//!
//! Two things are built from a document: the picture a viewport draws and the
//! scene an interchange writer is handed. They want different values out of
//! the same work, and the work is the expensive and delicate half: one
//! read-only open, one cold rebuild, one reading of each stored STEP source,
//! one canonical identity per definition, one tessellation per definition
//! however many places it appears, one policy about which tessellation refusal
//! may become an explicit omission, and one release of every shape on success,
//! failure and cancellation.
//!
//! Writing that twice would be writing two answers to the same question, and
//! the second would drift. So it is written here once, and what differs is
//! only what each caller keeps: [`LoadSink`] is handed the results and decides
//! what to make of them.
//!
//! # What this deliberately does not decide
//!
//! Packing, picking identity, naming, colour defaults, hierarchy flattening
//! and validity for a particular file format. Every one of those is a property
//! of the thing being built rather than of the document being read.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use ferritecad_document::{
    Document, ImportedDefinitionRef, ObjectPayload, ObjectRecord, StepImporter,
};
use ferritecad_eval::{RebuildResult, rebuild_cold};
use ferritecad_exchange::{
    ColourSource, Diagnostic, Import, Scene, Severity, Stage, StoredOccurrences,
};
use ferritecad_kernel::{
    GeometryKernel, KernelIdentity, Mesh, OperationContext, ProgressSink, ShapeHandle,
    TessellationParams, TessellationRefusal,
};
use ferritecad_types::{CadError, ImportedSourceId, ObjectId, OccurrenceId, Result, Transform};

use crate::{GeometryOmission, SceneItem};

/// How much of a load is building geometry rather than drawing it.
///
/// A guess, and the honest kind: nothing here can know the ratio for a
/// particular document, and any number would be wrong for some of them. What
/// it must not do is reach the end before the work does.
pub(crate) const BUILDING: f64 = 0.75;

/// What one sighting of a definition said about it.
#[derive(Debug, Default)]
pub(crate) struct Seen {
    pub(crate) name: Option<String>,
    pub(crate) source_file: Option<String>,
    pub(crate) solids: Option<u32>,
    pub(crate) source_unit: Option<String>,
    pub(crate) schema: Option<String>,
}

/// What repeated sightings of one display fact add up to.
///
/// Three states rather than two. "Nobody said" and "two sources said
/// different things" are different situations, and collapsing them would let
/// a third sighting refill a fact that had already been found ambiguous.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fact {
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

/// One definition while the load is still going on.
#[derive(Debug)]
struct Growing {
    item: SceneItem,
    name: Fact,
    source_file: Fact,
    source_unit: Fact,
    schema: Fact,
    solids: Option<u32>,
    omission: Option<GeometryOmission>,
    structural: bool,
    /// Whether this load has already said what this definition holds.
    ///
    /// Not the same as having been met. A definition met first as an assembly
    /// frame and used as a part further down is settled where the part is,
    /// which is where the meshing order of a picture has always been decided.
    reported: bool,
}

/// One definition, once every sighting of it has been read.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedDefinition {
    /// What this is, in terms a document could store.
    pub(crate) item: SceneItem,
    /// What the document or the file called it, when they agreed.
    pub(crate) name: Option<String>,
    /// The file an imported definition came from, by name.
    pub(crate) source_file: Option<String>,
    /// The unit the source file declared, as it declared it.
    pub(crate) source_unit: Option<String>,
    /// The schema the source file declared.
    pub(crate) schema: Option<String>,
    pub(crate) solids: Option<u32>,
    /// Why this definition has no triangles, when its retained topology could
    /// not be meshed by this build and the document knew it was invalid.
    pub(crate) omission: Option<GeometryOmission>,
    /// Whether this definition is structure that carries no geometry of its
    /// own. Distinct from an omission, and distinct from an empty mesh.
    pub(crate) structural: bool,
}

/// Every definition of one load, one entry per portable identity.
///
/// Canonical across the whole document rather than within one imported
/// object. Two objects can store the same bytes, and what they then hold is
/// the same definition: meshing it twice would give it two identities, and
/// choosing one of them would leave half of its placements pointing at the
/// other.
///
/// Keyed by [`SceneItem`] and by nothing else. A name, a file name, a solid
/// count, a position in a file and an object's place in the document are all
/// things two different definitions can share and one definition can be
/// described by differently.
#[derive(Debug, Default)]
pub(crate) struct Registry {
    entries: Vec<Growing>,
    /// Where each identity is. Never iterated: it is a lookup, and what is
    /// ordered is the entries beside it.
    known: HashMap<SceneItem, usize>,
}

impl Registry {
    /// The index of this identity, adding it when it is new.
    ///
    /// Reports whether this sighting is the first, which is what stops one
    /// definition from being tessellated twice merely because two imported
    /// objects both refer to it.
    pub(crate) fn register(&mut self, item: SceneItem, seen: Seen) -> Result<(usize, bool)> {
        if let Some(&index) = self.known.get(&item) {
            let entry = &mut self.entries[index];
            entry.name.seen(seen.name);
            entry.source_file.seen(seen.source_file);
            entry.source_unit.seen(seen.source_unit);
            entry.schema.seen(seen.schema);
            match (entry.solids, seen.solids) {
                (Some(known), Some(now)) if known != now => {
                    // Not two definitions, and not a number to pick between:
                    // one identity has one geometry, so two counts mean the
                    // document and the file disagree about what this is.
                    return Err(CadError::topology(format!(
                        "{item:?} was read as {known} solids and again as {now}; one durable \
                         definition cannot be two shapes"
                    )));
                }
                (None, now) => entry.solids = now,
                _ => {}
            }
            return Ok((index, false));
        }

        let index = self.entries.len();
        let mut entry = Growing {
            item: item.clone(),
            name: Fact::Unknown,
            source_file: Fact::Unknown,
            source_unit: Fact::Unknown,
            schema: Fact::Unknown,
            solids: seen.solids,
            omission: None,
            structural: false,
            reported: false,
        };
        entry.name.seen(seen.name);
        entry.source_file.seen(seen.source_file);
        entry.source_unit.seen(seen.source_unit);
        entry.schema.seen(seen.schema);
        self.entries.push(entry);
        self.known.insert(item, index);
        Ok((index, true))
    }

    /// Whether this load has already said what this definition holds.
    fn is_reported(&self, definition: usize) -> bool {
        self.entries
            .get(definition)
            .is_some_and(|entry| entry.reported)
    }

    fn growing(&mut self, definition: usize) -> Result<&mut Growing> {
        self.entries.get_mut(definition).ok_or_else(|| {
            CadError::topology(format!(
                "definition {definition} was settled before it was registered"
            ))
        })
    }

    fn meshed(&mut self, definition: usize) -> Result<()> {
        self.growing(definition)?.reported = true;
        Ok(())
    }

    fn omit(&mut self, definition: usize, omission: GeometryOmission) -> Result<()> {
        let entry = self.growing(definition)?;
        entry.omission = Some(omission);
        entry.reported = true;
        Ok(())
    }

    fn structural(&mut self, definition: usize) -> Result<()> {
        let entry = self.growing(definition)?;
        entry.structural = true;
        entry.reported = true;
        Ok(())
    }

    pub(crate) fn finish(self) -> Vec<PreparedDefinition> {
        self.entries
            .into_iter()
            .map(|entry| PreparedDefinition {
                item: entry.item,
                name: entry.name.into_option(),
                source_file: entry.source_file.into_option(),
                source_unit: entry.source_unit.into_option(),
                schema: entry.schema.into_option(),
                solids: entry.solids,
                omission: entry.omission,
                structural: entry.structural,
            })
            .collect()
    }
}

/// What a definition turned out to hold.
#[derive(Debug)]
pub(crate) enum Geometry<'a> {
    /// Triangles, in the kernel session this load owns.
    Mesh(&'a Mesh),
    /// Retained topology this build could not turn into triangles, and the
    /// typed refusal that says so. The persisted finding is on the definition;
    /// both observations that permitted this are already checked by the time
    /// it is reported.
    Omitted(TessellationRefusal),
    /// Structure with no geometry of its own: an assembly frame whose parts
    /// are separate definitions placed inside it.
    Structural,
}

/// What one placement durably is, in terms a document stores.
///
/// Three states, and the third is not an absence to be filled in. A document
/// written before placements carried identities has none, and the only honest
/// thing to hand on is that fact: a value invented here would be indexed by
/// whatever this load happened to traverse, and would look like an identity
/// while behaving like a position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeIdentity {
    /// A native body, identified by the object that holds it.
    Object(ObjectId),
    /// One placement of an imported scene, read back from the stored payload.
    Occurrence(OccurrenceId),
    /// The stored layout this placement came from predates placement identity.
    Unrecorded,
}

/// One place a definition appears, exactly as the source recorded it.
#[derive(Debug, Clone)]
pub(crate) struct PreparedNode {
    /// Which definition this places, by its index in the registry.
    pub(crate) definition: usize,
    /// The node this sits inside, by its index among the nodes already
    /// reported, or `None` at the top of a scene.
    pub(crate) parent: Option<usize>,
    /// Local to the parent, never accumulated.
    pub(crate) local: Transform,
    /// The same placement with its parents multiplied in, for a consumer that
    /// draws rather than writes a hierarchy.
    pub(crate) world: Transform,
    /// Whether this placement holds other placements. Such a node is the
    /// frame its children sit in; drawing it as well as them would draw the
    /// same solids twice.
    pub(crate) structural: bool,
    /// What the document or the file called this placement.
    pub(crate) name: Option<String>,
    /// What this placement durably is, taken from what the document stored and
    /// from nowhere else.
    pub(crate) identity: NodeIdentity,
    pub(crate) colour_source: ColourSource,
    /// Linear RGB. Meaningless when the source is [`ColourSource::None`].
    pub(crate) colour: [f64; 3],
}

/// What one caller keeps from a load.
///
/// The events arrive in a fixed order: [`opened`][Self::opened] once, then for
/// each object of the document its definitions and then its nodes, then
/// [`finish`][Self::finish]. Definitions are reported the moment their
/// geometry is decided, and never twice for one identity; nodes are reported
/// in source order with parents before children.
pub(crate) trait LoadSink {
    type Output;

    /// The single read-only open and the single cold rebuild, before any
    /// object is looked at.
    ///
    /// `objects` is the one read of the object table this load performs.
    fn opened(
        &mut self,
        document: &Document,
        objects: &[ObjectRecord],
        built: &RebuildResult,
    ) -> Result<()>;

    /// One definition, identified by its registry index.
    ///
    /// The indices do not necessarily arrive in order: a definition that only
    /// ever appears as an assembly frame is settled as soon as it is met,
    /// while one that carries geometry is settled where that geometry is
    /// built.
    fn definition(&mut self, definition: usize, geometry: Geometry<'_>) -> Result<()>;

    /// One placement.
    fn node(&mut self, node: &PreparedNode) -> Result<()>;

    /// Everything the load settled, with every sighting of every display fact
    /// already merged.
    fn finish(self, definitions: &[PreparedDefinition]) -> Result<Self::Output>;
}

/// Reads a document and reports what it holds.
///
/// Cancellation is checked between objects and between definitions as well as
/// inside the rebuild, so a document whose geometry takes a while can be
/// abandoned without waiting for it to finish. Every shape this obtains is
/// released before it returns, on the path that succeeds and on every path
/// that does not.
pub(crate) fn load<K, S>(
    path: &Path,
    kernel: &mut K,
    mut read_step: impl FnMut(&mut K, &[u8]) -> Result<Import>,
    params: &TessellationParams,
    context: &OperationContext,
    mut sink: S,
) -> Result<S::Output>
where
    K: GeometryKernel + ?Sized,
    S: LoadSink,
{
    // Opening is read-only, which neither migrates a schema nor changes a
    // persistent pragma. Anything that quietly rewrote the file it was asked
    // to look at would be the worst kind of surprise: the change would be
    // invisible, and it would happen to the one copy the user has.
    let document = Document::open_read_only(path)?;

    // Two phases of one job, so they share one scale. Building the geometry
    // is the slow half and gets most of it; the rest is describing what was
    // built. A bar that reached the end when the rebuild did would sit at
    // "finished" for the whole of the meshing.
    let building = phase(context, 0.0, BUILDING);
    let describing = phase(context, BUILDING, 1.0);

    // Cold on purpose, as everywhere else a result must be right rather than
    // quick: consulting a cache would make what comes out depend on the state
    // of a sidecar that exists only to save time.
    let built = rebuild_cold(&document, kernel, &building)?;

    // Handles this function obtained itself, as opposed to the ones the
    // rebuild owns. Filled as it goes so that a failure halfway through an
    // assembly still gives back what had already been read.
    let mut imported: Vec<ShapeHandle> = Vec::new();

    // Everything that can fail happens in here, so the shapes can be handed
    // back in one place whatever the outcome.
    let output = (|| -> Result<S::Output> {
        let objects = document.objects()?;
        sink.opened(&document, &objects, &built)?;

        let mut registry = Registry::default();
        let mut nodes = 0usize;

        // Counted before anything is described, so each one can say what
        // fraction of the work it is. An object that holds nothing is not part
        // of the count: the bar would stall on it and then jump.
        let holds_geometry = objects.iter().filter(|object| draws(object)).count();
        let mut done = 0usize;

        for object in &objects {
            context.check_cancelled()?;
            let scoped = phase(
                &describing,
                done as f64 / holds_geometry.max(1) as f64,
                (done + 1) as f64 / holds_geometry.max(1) as f64,
            );
            if draws(object) {
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
                    let (definition, first) = registry.register(
                        SceneItem::Body(object.id),
                        Seen {
                            name: object.name.clone(),
                            ..Seen::default()
                        },
                    )?;
                    if first {
                        let mesh = kernel.tessellate(shape, params, &scoped)?;
                        registry.meshed(definition)?;
                        sink.definition(definition, Geometry::Mesh(&mesh))?;
                    }
                    sink.node(&PreparedNode {
                        definition,
                        parent: None,
                        local: Transform::IDENTITY,
                        world: Transform::IDENTITY,
                        structural: false,
                        name: object.name.clone(),
                        // The object that holds the body, and no second
                        // identifier over it. An `OccurrenceId` minted here
                        // would be a new value every export, which is the one
                        // thing a durable identity must never be; and a stored
                        // one beside the object identifier would be two names
                        // for one thing that nothing could later be sure were
                        // the same. A body is placed exactly once, so the
                        // object is already the place.
                        identity: NodeIdentity::Object(object.id),
                        colour_source: ColourSource::None,
                        colour: [0.0; 3],
                    })?;
                    nodes += 1;
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
                    // An omitted definition needs two observations of the same
                    // thing: the diagnostic persisted with the document and the
                    // one made by this fresh reading. A historical warning
                    // cannot excuse an unrelated current failure, and a current
                    // finding cannot silently rewrite history.
                    let omittable: BTreeMap<String, Diagnostic> = reopened
                        .diagnostics_at_import
                        .iter()
                        .filter(|diagnostic| {
                            is_topology_failure(diagnostic)
                                && reopened.diagnostics_now.iter().any(|current| {
                                    is_topology_failure(current)
                                        && current.entity == diagnostic.entity
                                })
                        })
                        .map(|diagnostic| (diagnostic.entity.clone(), diagnostic.clone()))
                        .collect();
                    read_imported(
                        &mut registry,
                        &mut sink,
                        &mut nodes,
                        kernel,
                        Reading {
                            source: reopened.source(),
                            occurrences: reopened.occurrences(),
                            file: source_file,
                            omittable: &omittable,
                            scene: &reopened.scene,
                            params,
                            context: &scoped,
                        },
                    )?;
                }

                _ => continue,
            }
        }

        sink.finish(&registry.finish())
    })();

    for shape in imported.into_iter().rev() {
        kernel.release(shape);
    }
    built.release_all(kernel);
    output
}

/// One stored imported scene, and everything needed to read it.
struct Reading<'a> {
    /// The bytes this reading was verified against.
    source: ImportedSourceId,
    /// What the document stored as the durable identity of each placement, or
    /// the fact that its layout stored none. Positionally aligned with
    /// `scene.instances`, which the binding above has already required to be
    /// the same instances in the same order.
    occurrences: &'a StoredOccurrences,
    /// What to call the file it came from. A name to read, never a path to
    /// open: the bytes in the document are the source, and the place they were
    /// read from years ago may hold something else entirely by now.
    file: Option<String>,
    /// Persisted validation failures confirmed by the current reader.
    omittable: &'a BTreeMap<String, Diagnostic>,
    scene: &'a Scene,
    params: &'a TessellationParams,
    /// This object's slice of the whole load.
    context: &'a OperationContext,
}

/// Reports one stored imported scene.
///
/// # Only some instances carry geometry
///
/// An assembly arrives as both: a definition whose shape is the whole
/// assembly, and separate instances of the parts inside it. Building every
/// instance would build the same solids twice — once through the assembly's
/// own compound and once through each component — so an instance that has
/// children is structure and is not meshed. Its placement still counts: it is
/// what its children sit in.
fn read_imported<K, S>(
    registry: &mut Registry,
    sink: &mut S,
    nodes: &mut usize,
    kernel: &mut K,
    from: Reading<'_>,
) -> Result<()>
where
    K: GeometryKernel + ?Sized,
    S: LoadSink,
{
    let Reading {
        scene,
        params,
        context,
        ..
    } = from;
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

    // Which definitions of this scene anything asks for geometry from. A
    // definition every one of whose instances holds other instances is an
    // assembly frame and is never meshed.
    let wants_geometry: BTreeSet<usize> = scene
        .instances
        .iter()
        .enumerate()
        .filter(|(index, _)| !structural[*index])
        .map(|(_, instance)| instance.definition)
        .collect();

    // The caller gives this object one slice of the load; divide that slice
    // among the unique definitions it meshes. A definition another object
    // already meshed is not meshed again. Reusing the object's context for
    // every definition would make progress run from the beginning to the end
    // of the same slice once per part, going backwards between parts and
    // announcing completion more than once. If canonicalisation skips some or
    // all meshes, the explicit report at the end closes the part of this
    // object's slice no kernel call could report.
    let definitions_to_mesh = wants_geometry.len();
    let mut definitions_meshed = 0usize;

    // Registry indices for this scene's definitions, assigned in the order the
    // instances first mention them, so the export's order is the source's.
    let mut registered: BTreeMap<usize, usize> = BTreeMap::new();
    for (index, instance) in scene.instances.iter().enumerate() {
        context.check_cancelled()?;
        let definition = scene.definitions.get(instance.definition).ok_or_else(|| {
            CadError::input(format!(
                "instance {index} names definition {}, which this scene does not have",
                instance.definition
            ))
        })?;

        // The file's own name for this definition, kept beside the source it
        // belongs to. `#31` in one file is not `#31` in another, which is why
        // neither half travels alone — and why two objects storing the same
        // bytes name the same definition when they use the same key.
        let item = SceneItem::Imported(ImportedDefinitionRef::new(
            from.source,
            definition.key.clone(),
        )?);
        let seen = Seen {
            name: Some(definition.name.clone()),
            source_file: from.file.clone(),
            solids: Some(definition.solids),
            source_unit: Some(scene.source_unit.clone()),
            schema: Some(scene.schema.clone()),
        };
        let (registry_index, _) = registry.register(item, seen)?;
        registered.insert(instance.definition, registry_index);
        if registry.is_reported(registry_index) {
            continue;
        }

        if !wants_geometry.contains(&instance.definition) {
            registry.structural(registry_index)?;
            sink.definition(registry_index, Geometry::Structural)?;
            continue;
        }
        if structural[index] {
            // This definition does carry geometry, but not here. It is settled
            // at the first instance that actually asks for it, which is where
            // the meshing order of a picture has always been decided.
            continue;
        }

        let scoped = phase(
            context,
            definitions_meshed as f64 / definitions_to_mesh.max(1) as f64,
            (definitions_meshed + 1) as f64 / definitions_to_mesh.max(1) as f64,
        );
        match kernel.tessellate(definition.shape, params, &scoped) {
            Ok(mesh) => {
                definitions_meshed += 1;
                registry.meshed(registry_index)?;
                sink.definition(registry_index, Geometry::Mesh(&mesh))?;
            }
            Err(reason) => {
                let (Some(refusal), Some(diagnostic)) = (
                    face_tessellation_refusal(&reason),
                    from.omittable.get(&definition.key),
                ) else {
                    return Err(reason);
                };
                registry.omit(
                    registry_index,
                    GeometryOmission {
                        diagnostic: diagnostic.clone(),
                        reason: reason.to_string(),
                    },
                )?;
                definitions_meshed += 1;
                sink.definition(registry_index, Geometry::Omitted(refusal))?;
            }
        }
    }

    // One identity per placement or none at all, checked before a single node
    // is reported. Two lists of different lengths cannot be aligned, and the
    // failure of aligning them anyway is silent: every placement after the
    // first missing one would carry its neighbour's identity.
    if let StoredOccurrences::Recorded(recorded) = from.occurrences
        && recorded.len() != scene.instances.len()
    {
        return Err(CadError::input(format!(
            "the document stored {} placement identities for a scene of {} placements, so \
             they cannot be the identities of these placements",
            recorded.len(),
            scene.instances.len()
        )));
    }

    let base = *nodes;
    for (index, instance) in scene.instances.iter().enumerate() {
        let definition = *registered.get(&instance.definition).ok_or_else(|| {
            CadError::topology(format!(
                "instance {index} places definition {}, which this load never settled",
                instance.definition
            ))
        })?;
        // Read out of the stored payload, positionally, and derived from
        // nothing. There is no arm here that falls back to the index, the
        // parent, the name or the key: a placement the document never gave an
        // identity says so all the way to the export boundary.
        let identity = match from.occurrences {
            StoredOccurrences::Unrecorded => NodeIdentity::Unrecorded,
            StoredOccurrences::Recorded(recorded) => {
                NodeIdentity::Occurrence(*recorded.get(index).ok_or_else(|| {
                    CadError::input(format!(
                        "instance {index} has no stored placement identity among the {} this \
                         document recorded",
                        recorded.len()
                    ))
                })?)
            }
            // A stored layout this build has not been measured against is not
            // silently treated as having no identity: that would turn a
            // document that recorded one into one that appears not to have.
            _ => {
                return Err(CadError::unsupported(
                    "this document records placement identity in a way this build does not \
                     know how to read",
                ));
            }
        };
        sink.node(&PreparedNode {
            definition,
            parent: instance.parent.map(|parent| base + parent),
            local: placement_of(&instance.placement)?,
            world: world[index],
            structural: structural[index],
            name: Some(instance.name.clone()).filter(|name| !name.trim().is_empty()),
            identity,
            colour_source: instance.colour_source,
            colour: instance.colour,
        })?;
    }
    *nodes += scene.instances.len();

    if definitions_to_mesh == 0 || definitions_meshed < definitions_to_mesh {
        context.progress().report(1.0);
    }
    Ok(())
}

fn is_topology_failure(diagnostic: &Diagnostic) -> bool {
    diagnostic.stage == Stage::Validation && diagnostic.severity == Severity::Fail
}

/// The narrow failure an invalid retained definition may turn into an
/// explicit empty definition, when there is one.
///
/// Cancellation, allocation, malformed mesh data and every other loader error
/// still refuse the load. No healing, sewing, tolerance change or topology
/// edit is attempted here.
fn face_tessellation_refusal(reason: &CadError) -> Option<TessellationRefusal> {
    // Named rather than passed through, so a refusal this policy has not been
    // measured against cannot become a silent omission by having been added to
    // the vocabulary.
    match TessellationRefusal::of(reason)? {
        TessellationRefusal::IncompleteFace => Some(TessellationRefusal::IncompleteFace),
        _ => None,
    }
}

/// Whether this object holds geometry at all.
///
/// A body with nothing built into it yet holds none, and neither does
/// anything that is not geometry to begin with.
fn draws(object: &ObjectRecord) -> bool {
    match &object.payload {
        ObjectPayload::Body(body) => body.tip_feature.is_some(),
        ObjectPayload::ImportedStep(_) => true,
        _ => false,
    }
}

/// A slice of the whole load, as its own `0..1`.
///
/// The phases below report how far through themselves they are; a caller
/// wants to know how far through the load it is. Composing that here means
/// neither phase has to know what else the load does.
pub(crate) fn phase(context: &OperationContext, from: f64, to: f64) -> OperationContext {
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

/// The last component of whatever provenance the document recorded.
///
/// The field is a hint for a person and nothing opens it, but a document
/// written elsewhere may hold a whole path in it, and neither a viewport nor
/// an exported file is the place to repeat one.
pub(crate) fn file_name_of(recorded: Option<&str>) -> Option<String> {
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
