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

mod drawing;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::path::Path;

pub use drawing::{CIRCLE_SEGMENTS, sketch_drawing, sketch_drawings};
use ferritecad_document::{
    Document, EntityKind, ImportedDefinitionRef, ObjectPayload, ObjectRecord, SelectionRule,
    SemanticRole, Sketch, SketchConstraintRule, StepImporter, TopologyRef,
};
use ferritecad_eval::rebuild_cold;
pub use ferritecad_eval::{
    ConflictingConstraint, PresentedCurve, SketchConflict, SketchPresentation, SketchSolveReport,
};
use ferritecad_exchange::{ColourSource, Import, Scene};
use ferritecad_kernel::{
    GeometryKernel, KernelIdentity, OperationContext, ProgressSink, ShapeHandle, TessellationParams,
};
use ferritecad_types::{
    CadError, CanonicalHasher, ContentHash, ImportedSourceId, ObjectId, Result, StableEntityId,
    Transform,
};
use ferritecad_viewport::{
    EdgePickId, FacePickId, PickId, RenderSnapshot, SnapshotBuilder, VertexPickId,
};
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
    /// What the document durably calls the faces of this picture, if
    /// anything does.
    pub faces: FaceNames,
    /// What it durably calls the topological edges of this picture.
    pub edges: EdgeNames,
    /// What it durably calls the topological vertices of this picture.
    pub vertices: VertexNames,
    /// What the solve of each constrained sketch found out, in document order.
    ///
    /// A third kind of answer, beside the picture and what a click means: not
    /// about anything drawn, because a sketch is not drawn here, and not about
    /// anything clickable. It rides along because the rebuild that produced
    /// the picture is the one solve that could have said it, and asking again
    /// afterwards would be solving the same sketch twice.
    pub sketch_solves: Vec<SketchSolveFacts>,
    /// Every sketch of the document, at the coordinates its profile was built
    /// from, in document order.
    ///
    /// A fourth answer, and the only one that is geometry without being part
    /// of the picture: nothing here is packed, placed, tessellated, picked or
    /// counted, and the snapshot beside it is byte for byte what it was before
    /// this existed. It rides along for the same reason the solve facts do —
    /// the rebuild that drew the solids is the one evaluation that could have
    /// said where the drawing behind them ended up.
    ///
    /// One entry per sketch, however many extrudes or bodies read it, and
    /// unlike [`sketch_solves`][Self::sketch_solves] an unconstrained sketch
    /// has one too: it was never solved, and it was still drawn.
    pub sketch_presentations: Vec<SketchPresentation>,
}

/// What one solved sketch of this document turned out to be, in the
/// document's own words.
///
/// Assembled from the [`ObjectRecord`] and the rebuild that solved it, and
/// from nothing else: no second read of the file, and no second solve. One
/// entry per sketch that carried constraints, however many bodies,
/// definitions, placements or downstream features read it — a sketch is one
/// sketch, and how often it was used is not a fact about how it solved.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchSolveFacts {
    /// The sketch this is about.
    sketch: ObjectId,
    /// What the document calls it, when it calls it anything.
    name: Option<String>,
    /// What the solve found out.
    report: SketchSolveReport,
    /// What each constraint the solve called redundant actually says.
    ///
    /// Not a second list beside [`SketchSolveReport::redundant`]: it is that
    /// list, walked once in its own order, with the rule this sketch stores
    /// under each identifier carried alongside it. The two cannot disagree
    /// because only one of them is read to build the other, and neither is
    /// ever written by hand.
    redundant: Vec<RedundantConstraint>,
}

impl SketchSolveFacts {
    /// The sketch this account belongs to.
    pub fn sketch(&self) -> ObjectId {
        self.sketch
    }

    /// What the document calls the sketch, when it calls it anything.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// What the one rebuild-time solve found out.
    pub fn report(&self) -> &SketchSolveReport {
        &self.report
    }

    /// The stored rule behind each identifier the report calls redundant.
    ///
    /// The fields of this type are private so this list cannot be replaced
    /// independently of [`Self::report`]. Only the join in [`snapshot_of`]
    /// constructs the pair.
    pub fn redundant(&self) -> &[RedundantConstraint] {
        &self.redundant
    }
}

/// One repeated constraint, named durably and said in the document's words.
///
/// The identifier is the solve's own answer and the rule is what the document
/// stores under exactly that identifier – nothing here was decided by where a
/// constraint sits in a list, by what a solver numbered it, or by two rules
/// happening to say the same thing. Two identical rules stored under two
/// identifiers are two of these, because they are two constraints.
///
/// The rule is carried whole rather than turned into a sentence: a scene has
/// no business choosing anybody's words, and the document's own vocabulary is
/// the smallest durable thing that can be explained later.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RedundantConstraint {
    /// The identifier the document stores this constraint under.
    id: StableEntityId,
    /// What it says.
    rule: SketchConstraintRule,
}

impl RedundantConstraint {
    /// The identifier the document stores this constraint under.
    pub fn id(&self) -> StableEntityId {
        self.id
    }

    /// The exact rule stored under that identifier.
    pub fn rule(&self) -> &SketchConstraintRule {
        &self.rule
    }
}

/// The stored constraint behind each identifier a solve called redundant.
///
/// `reported` is [`SketchSolveReport::redundant`] and nothing else; it arrives
/// as the identifiers themselves because that is all of a report this has any
/// business reading, and because a function over two document values can be
/// held to its contract without a solver in the room.
///
/// Joined by [`StableEntityId`] alone. Not by position, because the report's
/// order is the document's order and the sketch's order is the document's
/// order and matching them up by index would pass every test that ever
/// compared two lists of the same length; not by what a rule says, because two
/// constraints are allowed to say the same thing; and not by anything a solver
/// numbered, because none of that survives the call it was made in.
///
/// An identifier this sketch does not store is refused rather than skipped or
/// blamed on a neighbour. It means the report and the sketch are not about the
/// same drawing, and a window that quietly explained the constraint next to it
/// would be confidently wrong.
fn redundant_constraints(
    sketch: &Sketch,
    reported: &[StableEntityId],
) -> Result<Vec<RedundantConstraint>> {
    reported
        .iter()
        .map(|id| {
            sketch
                .constraints
                .iter()
                .find(|constraint| constraint.id == *id)
                .map(|constraint| RedundantConstraint {
                    id: *id,
                    rule: constraint.rule,
                })
                .ok_or_else(|| {
                    CadError::constraint(format!(
                        "the solve called {id} redundant, and this sketch stores no constraint \
                         under that identifier"
                    ))
                })
        })
        .collect()
}

/// Every durable name this document has for the edges of one picture.
///
/// Built while the rebuild's topology map and the tessellation's edge handles
/// are both in hand, which is the only moment they can be joined, and holding
/// neither afterwards. An edge nothing names has an empty list rather than an
/// invented entry: a name that was not stored is not a name.
#[derive(Clone, PartialEq)]
pub struct EdgeNames {
    /// One pick from the snapshot whose semantic context produced this, used
    /// only as a binding check and deliberately absent from `Debug`.
    picture: PickId,
    /// Indexed by the picture's own edge identity, minus one.
    by_edge: Vec<Vec<EdgeMeaning>>,
}

impl Default for EdgeNames {
    fn default() -> Self {
        Self {
            picture: PickId::NOTHING,
            by_edge: Vec::new(),
        }
    }
}

impl fmt::Debug for EdgeNames {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EdgeNames")
            .field("by_edge", &self.by_edge)
            .finish()
    }
}

impl EdgeNames {
    /// What the document calls this edge, or nothing.
    ///
    /// Answered through the picture that issued the edge, so an edge of a
    /// picture that has been replaced names nothing here however plausible
    /// its number looks.
    pub fn of(&self, edge: EdgePickId, snapshot: &RenderSnapshot) -> &[EdgeMeaning] {
        if snapshot.definition(self.picture).is_none()
            || snapshot.definition_of_edge(edge).is_none()
        {
            return &[];
        }
        match (edge.to_raw() as usize).checked_sub(1) {
            Some(index) => self.by_edge.get(index).map_or(&[], Vec::as_slice),
            None => &[],
        }
    }
}

/// What a document durably calls one entity, in the document's own words.
///
/// Every field is a portable term the document already stores. There is no
/// handle here, no session, no ordinal and no traversal position: this is what
/// a stored [`TopologyRef`] says, minus the geometric fallback, which is a hint
/// for a person and never an identity.
///
/// One type for a face, an edge and a vertex. What a document stores about any
/// of them is the same six fields, and three structures would be three places
/// for the same rule to drift; which kind is meant is already in
/// `expected_kind` and in the role. [`FaceMeaning`], [`EdgeMeaning`] and
/// [`VertexMeaning`] name it where a reader expects one kind in particular.
#[derive(Debug, Clone, PartialEq)]
pub struct PortableMeaning {
    /// The stored reference itself.
    pub reference: StableEntityId,
    /// The object holding the reference.
    pub owner: ObjectId,
    /// The feature whose output is named.
    pub producer_feature: ObjectId,
    /// What kind of entity the reference expects.
    pub expected_kind: EntityKind,
    /// What the named entity is, semantically.
    pub output_role: SemanticRole,
    /// How many entities the reference selects, and which.
    pub selection: SelectionRule,
}

/// What a document calls one face. See [`PortableMeaning`].
pub type FaceMeaning = PortableMeaning;

/// What a document calls one edge. See [`PortableMeaning`].
pub type EdgeMeaning = PortableMeaning;

/// What a document calls one topological vertex. See [`PortableMeaning`].
pub type VertexMeaning = PortableMeaning;

impl PortableMeaning {
    fn of(reference: &TopologyRef) -> Self {
        Self {
            reference: reference.id,
            owner: reference.owner,
            producer_feature: reference.producer_feature,
            expected_kind: reference.expected_kind,
            output_role: reference.output_role.clone(),
            selection: reference.selection.clone(),
        }
    }
}

/// A portable meaning while it is still paired with the deterministic digest
/// used to bind the picture's transient identities to it.
#[derive(Clone)]
struct BoundMeaning {
    meaning: PortableMeaning,
    identity: ContentHash,
}

/// Every durable name this document has for the topological vertices of one
/// picture.
///
/// Built while the rebuild's topology map and the tessellation's vertex
/// handles are both in hand, which is the only moment they can be joined, and
/// holding neither afterwards. A corner nothing names has an empty list rather
/// than an invented entry: a name that was not stored is not a name.
///
/// One entry per topological vertex, never per occurrence. A corner of a box
/// is drawn three times, once for each face meeting there, and a plate placed
/// twice is drawn twice again; all of those are one corner and answer to one
/// identity with one list of names.
#[derive(Clone, PartialEq)]
pub struct VertexNames {
    /// One pick from the snapshot whose semantic context produced this, used
    /// only as a binding check and deliberately absent from `Debug`.
    picture: PickId,
    /// Indexed by the picture's own vertex identity, minus one.
    by_vertex: Vec<Vec<VertexMeaning>>,
}

impl Default for VertexNames {
    fn default() -> Self {
        Self {
            picture: PickId::NOTHING,
            by_vertex: Vec::new(),
        }
    }
}

impl fmt::Debug for VertexNames {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VertexNames")
            .field("by_vertex", &self.by_vertex)
            .finish()
    }
}

impl VertexNames {
    /// What the document calls this topological vertex, or nothing.
    ///
    /// Answered through the picture that issued the corner, so a corner of a
    /// picture that has been replaced names nothing here however plausible its
    /// number looks.
    pub fn of(&self, vertex: VertexPickId, snapshot: &RenderSnapshot) -> &[VertexMeaning] {
        if snapshot.definition(self.picture).is_none()
            || snapshot.definition_of_vertex(vertex).is_none()
        {
            return &[];
        }
        match (vertex.to_raw() as usize).checked_sub(1) {
            Some(index) => self.by_vertex.get(index).map_or(&[], Vec::as_slice),
            None => &[],
        }
    }
}

/// Every durable name this document has for the faces of one picture.
///
/// Built while the rebuild's topology map and the tessellation's face handles
/// are both in hand, which is the only moment they can be joined, and holding
/// neither afterwards. A face nothing names has an empty list rather than an
/// invented entry: a name that was not stored is not a name.
#[derive(Clone, PartialEq)]
pub struct FaceNames {
    /// One pick from the snapshot whose semantic context produced `by_face`.
    /// It is only a binding check and is deliberately absent from `Debug`:
    /// transient identity is not part of what a face is called.
    picture: PickId,
    /// Indexed by the picture's own face identity, minus one.
    by_face: Vec<Vec<FaceMeaning>>,
}

impl Default for FaceNames {
    fn default() -> Self {
        Self {
            picture: PickId::NOTHING,
            by_face: Vec::new(),
        }
    }
}

impl fmt::Debug for FaceNames {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FaceNames")
            .field("by_face", &self.by_face)
            .finish()
    }
}

impl FaceNames {
    /// What the document calls this face, or nothing.
    ///
    /// Answered through the picture that issued the face, so a face of a
    /// picture that has been replaced names nothing here however plausible
    /// its number looks.
    pub fn of(&self, face: FacePickId, snapshot: &RenderSnapshot) -> &[FaceMeaning] {
        if snapshot.definition(self.picture).is_none()
            || snapshot.definition_of_face(face).is_none()
        {
            return &[];
        }
        match (face.to_raw() as usize).checked_sub(1) {
            Some(index) => self.by_face.get(index).map_or(&[], Vec::as_slice),
            None => &[],
        }
    }
}

/// What is chosen in one picture, as one state.
///
/// Five states, not a collection of fields that can disagree. A face, edge or
/// vertex selection carries the subshape, the definition it belongs to and
/// what the document calls it, all decided together by [`Selection::at`]; the
/// fields are private, so a caller cannot assemble a subshape beside a
/// definition it does not belong to.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Selection {
    #[default]
    Nothing,
    Definition(PickId),
    Face(SelectedFace),
    Edge(SelectedEdge),
    Vertex(SelectedVertex),
}

/// One chosen topological vertex: which corner, of what, and what it is
/// called.
///
/// Private fields, exactly as [`SelectedFace`] and [`SelectedEdge`] have them,
/// and for the same reason: a caller outside this crate cannot put together a
/// corner that belongs to one definition beside a definition it does not
/// belong to, or a corner with no durable name at all. [`Selection::at`]
/// decides all three together or produces no vertex.
///
/// The identity is transient and the meanings are portable, which is the whole
/// arrangement: what is chosen right now is a number only this picture can
/// read, and what it *is* survives the picture entirely.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectedVertex {
    vertex: VertexPickId,
    definition: PickId,
    meanings: Vec<VertexMeaning>,
}

impl SelectedVertex {
    /// The transient identity of the corner, for this picture only.
    pub fn vertex(&self) -> VertexPickId {
        self.vertex
    }

    /// The definition it belongs to, in this picture.
    pub fn definition(&self) -> PickId {
        self.definition
    }

    /// Every stored reference that resolves exactly to this corner, in the
    /// order the document stores them. All of them, for the reason a face and
    /// an edge keep all of theirs: choosing the first would present storage
    /// order as a decision about which name is right.
    pub fn meanings(&self) -> &[VertexMeaning] {
        &self.meanings
    }
}

/// One chosen topological edge: which edge, of what, and what it is called.
///
/// Private fields, exactly as [`SelectedFace`] has them, and for the same
/// reason: a caller outside this crate cannot put together an edge that
/// belongs to one definition beside a definition it does not belong to, or an
/// edge with no durable name at all. [`Selection::at`] decides all three
/// together or produces no edge.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectedEdge {
    edge: EdgePickId,
    definition: PickId,
    meanings: Vec<EdgeMeaning>,
}

impl SelectedEdge {
    /// The transient identity of the edge, for this picture only.
    pub fn edge(&self) -> EdgePickId {
        self.edge
    }

    /// The definition it belongs to, in this picture.
    pub fn definition(&self) -> PickId {
        self.definition
    }

    /// Every stored reference that resolves exactly to this edge, in the order
    /// the document stores them. All of them, for the reason a face keeps all
    /// of its: choosing the first would present storage order as a decision
    /// about which name is right.
    pub fn meanings(&self) -> &[EdgeMeaning] {
        &self.meanings
    }
}

/// One chosen face: which face, of what, and what it is called.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectedFace {
    face: FacePickId,
    definition: PickId,
    meanings: Vec<FaceMeaning>,
}

impl SelectedFace {
    /// The transient identity of the face, for this picture only.
    pub fn face(&self) -> FacePickId {
        self.face
    }

    /// The definition it belongs to, in this picture.
    pub fn definition(&self) -> PickId {
        self.definition
    }

    /// Every stored reference that resolves exactly to this face, in the order
    /// the document stores them. All of them: several references may name one
    /// face, and choosing the first would be presenting storage order as a
    /// decision about which name is right.
    pub fn meanings(&self) -> &[FaceMeaning] {
        &self.meanings
    }
}

impl Selection {
    /// What a click on one pixel chooses.
    ///
    /// Most particular first: an exactly named corner, then an exactly named
    /// edge, then an exactly named face, then the definition, then nothing.
    /// Each is chosen only when it is coherent with everything else read from
    /// the same pixel and the document has an exact durable name for it. An
    /// imported or unnamed subshape, and one whose references select a family,
    /// falls back to the most specific thing the document can say honestly.
    /// Nothing that resolves in this picture chooses nothing.
    // Four identities read from one pixel, the picture that issued them, and
    // what the document calls each of the three kinds. They are one decision
    // about one sample, and a struct built at the single call site would hide
    // the count rather than reduce it.
    #[expect(clippy::too_many_arguments, reason = "see above")]
    pub fn at(
        definition: PickId,
        face: FacePickId,
        edge: EdgePickId,
        vertex: VertexPickId,
        snapshot: &RenderSnapshot,
        names: &FaceNames,
        edges: &EdgeNames,
        vertices: &VertexNames,
    ) -> Self {
        // The corner first, because a person who aimed at a point meant the
        // point rather than the line or the surface it sits on. It is a choice
        // only where the document names it exactly and where every other
        // answer about this pixel agrees that the corner is there: the corner
        // must belong to the definition under the sample, must touch the face
        // under it where the sample has one, and must be an end of the edge
        // under it where the sample has one. Adjacency is read out of the
        // packed partitions, never from ordinals, coordinates, traversal
        // positions or occurrence indices.
        //
        // Both are stated as "where the sample has one" because that is what
        // the pixel reports. A hit that answers about a corner always answers
        // about the face it touches as well, so on that route both hold; the
        // conditional form is what keeps this honest for any other caller
        // rather than silently requiring a face the pixel never claimed.
        //
        // An aperture reaches a few pixels past the point it is drawn for, so
        // without those checks a corner would be selectable over the neighbour
        // it merely overlaps.
        let vertex_owner = snapshot.definition_of_vertex(vertex);
        let vertex_meanings = vertices.of(vertex, snapshot);
        if !vertex_meanings.is_empty()
            && vertex_owner.is_some()
            && vertex_owner == snapshot.definition(definition)
            && (face == FacePickId::NOTHING || snapshot.vertex_touches_face(vertex, face))
            && (edge == EdgePickId::NOTHING || snapshot.vertex_ends_edge(vertex, edge))
        {
            return Self::Vertex(SelectedVertex {
                vertex,
                definition,
                meanings: vertex_meanings.to_vec(),
            });
        }

        // Then the edge, and only where the document can say what the thing
        // is. An edge the document names beats a face it also names, because a
        // person who aimed at a line meant the line; an edge nobody named is
        // not a lesser edge, it is not a choice at all, and falls through to
        // whatever this picture can honestly say instead.
        let edge_owner = snapshot.definition_of_edge(edge);
        let edge_meanings = edges.of(edge, snapshot);
        if !edge_meanings.is_empty()
            && edge_owner.is_some()
            && edge_owner == snapshot.definition(definition)
            && snapshot.edge_bounds_face(edge, face)
        {
            return Self::Edge(SelectedEdge {
                edge,
                definition,
                meanings: edge_meanings.to_vec(),
            });
        }

        let owner = snapshot.definition_of_face(face);
        let meanings = names.of(face, snapshot);
        // The pick and the face must be two statements about one pixel. They
        // are read from one frame, so they already are; a disagreement means
        // one of them is stale, and a selection built from the two would be
        // about neither.
        if !meanings.is_empty() && owner.is_some() && owner == snapshot.definition(definition) {
            return Self::Face(SelectedFace {
                face,
                definition,
                meanings: meanings.to_vec(),
            });
        }
        Self::definition(definition, snapshot)
    }

    /// The definition a pick names, or nothing.
    ///
    /// What a list row chooses, and what a click on something with no durable
    /// vertex, edge or face name falls back to.
    pub fn definition(pick: PickId, snapshot: &RenderSnapshot) -> Self {
        match snapshot.definition(pick) {
            Some(_) => Self::Definition(pick),
            None => Self::Nothing,
        }
    }

    /// Which definition is chosen, whichever way it was chosen.
    pub fn owning_definition(&self, snapshot: &RenderSnapshot) -> Option<usize> {
        match self {
            Self::Nothing => None,
            Self::Definition(pick) => snapshot.definition(*pick),
            Self::Face(chosen) => snapshot.definition(chosen.definition),
            Self::Edge(chosen) => snapshot.definition(chosen.definition),
            Self::Vertex(chosen) => snapshot.definition(chosen.definition),
        }
    }

    /// What the renderer must mark, which is the transient half of this.
    pub fn marked(&self) -> ferritecad_viewport::Marked {
        match self {
            Self::Nothing => ferritecad_viewport::Marked::Nothing,
            Self::Definition(pick) => ferritecad_viewport::Marked::Definition(*pick),
            Self::Face(chosen) => ferritecad_viewport::Marked::Face(chosen.face),
            Self::Edge(chosen) => ferritecad_viewport::Marked::Edge(chosen.edge),
            Self::Vertex(chosen) => ferritecad_viewport::Marked::Vertex(chosen.vertex),
        }
    }

    /// Where what is chosen is, in every placement of it.
    ///
    /// A face is its own triangles, an edge its own segments, a corner its own
    /// occurrences, and a definition all of it. One question, answered by the
    /// picture that issued the choice.
    pub fn bounds(&self, snapshot: &RenderSnapshot) -> Option<([f32; 3], [f32; 3])> {
        match self {
            Self::Nothing => None,
            Self::Definition(pick) => snapshot.bounds_of(*pick),
            Self::Face(chosen) => snapshot.bounds_of_face(chosen.face),
            Self::Edge(chosen) => snapshot.bounds_of_edge(chosen.edge),
            Self::Vertex(chosen) => snapshot.bounds_of_vertex(chosen.vertex),
        }
    }
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
                        "{item:?} was read as {known} solids and again as {now}; one durable \
                         definition cannot be two shapes"
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
                "a definition was packed as {index} while the catalogue held {}; a click \
                 resolves through both, so they cannot disagree",
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
        // Every stored reference that names exactly one entity of this rebuild,
        // paired with the handle it named. Resolved once, in the order the
        // document stores its references, so what an entity is called does not
        // depend on the order faces, edges or vertices happen to be tessellated
        // in.
        //
        // A reference that resolves to several entities is not a name for
        // whichever one was clicked, so it is not here; one that resolves to
        // none, or that this build cannot resolve at all, names nothing and is
        // not here either.
        // No failure is reported: a lost reference is a document-level fact, and
        // a viewer that refused to draw a model because one name no longer
        // resolves would be useless exactly when it is needed.
        let named: Vec<(ferritecad_kernel::SubShapeHandle, BoundMeaning)> = document
            .topology_refs()?
            .iter()
            .filter_map(|reference| match built.resolve(reference) {
                Ok(found) => match found.as_slice() {
                    [entity] => Some((
                        *entity,
                        BoundMeaning {
                            meaning: PortableMeaning::of(reference),
                            identity: reference.meaning_hash(),
                        },
                    )),
                    _ => None,
                },
                Err(_) => None,
            })
            .collect();

        let mut builder = SnapshotBuilder::new();
        // One catalogue for the whole document, not one per imported object:
        // two objects can store the same bytes, and what they then draw is the
        // same definition.
        let mut catalogue = Catalogue::default();
        // What each face of each packed definition is called, by the ordinal
        // the kernel listed it under while packing. Turned into the picture's
        // own face identities once the picture exists, because the picture is
        // what numbers them.
        let mut names: BTreeMap<usize, Vec<Vec<BoundMeaning>>> = BTreeMap::new();
        // The same, for the topological edges the kernel named. A definition
        // whose mesh carries no edge association contributes no entry at all,
        // which is what keeps "nothing is known" from becoming "named nothing".
        let mut edge_named: BTreeMap<usize, Vec<Vec<BoundMeaning>>> = BTreeMap::new();
        // And the same for the topological vertices. One entry per corner the
        // kernel identified, never per occurrence: how often a corner is drawn
        // is a fact about triangles, and what it is called is not.
        let mut vertex_named: BTreeMap<usize, Vec<Vec<BoundMeaning>>> = BTreeMap::new();
        let objects = document.objects()?;

        // Read from the records already in hand and the rebuild that has
        // already happened. Document order is the order `objects` is in, and
        // the join is by the sketch's own identifier, so a report belongs to
        // the sketch it was filed under and to no neighbour of it. An imported
        // object is never a sketch and was never solved, so it is never here.
        //
        // This is the one moment a report and the sketch it is about are both
        // in hand, so it is where each identifier the report named is turned
        // back into the constraint the document stores under it. Nothing is
        // asked of a solver and nothing is read from the file again: both
        // halves are already here.
        //
        // The drawings themselves are read out of the same rebuild in the
        // same walk, and for the same reasons: document order because that is
        // the order the person who drew them sees, by identifier because a
        // sketch two extrudes read is one sketch, and from the rebuild because
        // it is the only thing that knows where a solve left the curves.
        let mut sketch_solves: Vec<SketchSolveFacts> = Vec::new();
        let mut sketch_presentations: Vec<SketchPresentation> = Vec::new();
        for object in &objects {
            if let Some(presentation) = built.sketch_presentation(object.id) {
                sketch_presentations.push(presentation.clone());
            }
            let Some(report) = built.solve_report(object.id) else {
                continue;
            };
            // Only a sketch carries constraints and only a sketch is solved,
            // so a report filed under anything else is a report about a
            // different drawing. Refused rather than described from whatever
            // record happens to be here.
            let ObjectPayload::Sketch(sketch) = &object.payload else {
                return Err(CadError::constraint(format!(
                    "a solve was reported for {}, which this document stores as {} rather than \
                     a sketch",
                    object.id,
                    object.payload.type_name()
                )));
            };
            sketch_solves.push(SketchSolveFacts {
                sketch: object.id,
                name: object.name.clone(),
                redundant: redundant_constraints(sketch, report.redundant())?,
                report: report.clone(),
            });
        }

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
                            // Joined by the handle the kernel gave the face,
                            // which is the only thing that says the stored
                            // name and this triangle range are the same face.
                            // Not by ordinal, not by geometry, and not by
                            // name: two faces of one body can be congruent,
                            // and traversal order is the kernel's business.
                            // The edges, joined the same way and for the same
                            // reason: the handle the kernel gave the edge is
                            // the only thing that says a stored name and this
                            // run of segments are the same edge. A handle
                            // carries its kind, so a face's name can never
                            // match an edge's range however the two are
                            // numbered.
                            let named_edges: Vec<Vec<BoundMeaning>> = mesh
                                .edges
                                .as_ref()
                                .map(|edges| {
                                    edges
                                        .ranges
                                        .iter()
                                        .map(|range| {
                                            named
                                                .iter()
                                                .filter(|(handle, _)| *handle == range.edge)
                                                .map(|(_, meaning)| meaning.clone())
                                                .collect::<Vec<BoundMeaning>>()
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            // The corners, joined the same way and for the
                            // same reason. `range.vertex` is the handle the
                            // kernel gave the topological vertex, and equality
                            // with it is the only thing that says a stored name
                            // and this corner are the same point. Not the
                            // ordinal, not the coordinates, not which
                            // occurrence came first and not how many there are:
                            // one range is one corner however often it is drawn.
                            let named_vertices: Vec<Vec<BoundMeaning>> = mesh
                                .topological_vertices
                                .as_ref()
                                .map(|corners| {
                                    corners
                                        .ranges
                                        .iter()
                                        .map(|range| {
                                            named
                                                .iter()
                                                .filter(|(handle, _)| *handle == range.vertex)
                                                .map(|(_, meaning)| meaning.clone())
                                                .collect::<Vec<BoundMeaning>>()
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            let named: Vec<Vec<BoundMeaning>> = mesh
                                .faces
                                .iter()
                                .map(|range| {
                                    named
                                        .iter()
                                        .filter(|(handle, _)| *handle == range.face)
                                        .map(|(_, meaning)| meaning.clone())
                                        .collect()
                                })
                                .collect();
                            let definition = builder.add_mesh(&mesh)?;
                            names.insert(definition, named);
                            if !named_edges.is_empty() {
                                edge_named.insert(definition, named_edges);
                            }
                            // A mesh with no vertex association, and one whose
                            // association is provably empty, both contribute
                            // nothing. Either way this picture knows of no
                            // corner here, which is not the same as knowing
                            // there is a corner nobody named.
                            if !named_vertices.is_empty() {
                                vertex_named.insert(definition, named_vertices);
                            }
                            Ok(definition)
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
        builder.bind_identities_to(semantic_context_identity(
            &names,
            &edge_named,
            &vertex_named,
        ))?;
        let snapshot = builder.build();
        Ok(LoadedScene {
            faces: face_names(&snapshot, names)?,
            edges: edge_names(&snapshot, edge_named)?,
            vertices: vertex_names(&snapshot, vertex_named)?,
            snapshot,
            catalogue: catalogue.finish(),
            sketch_solves,
            sketch_presentations,
        })
    })();

    for shape in imported.into_iter().rev() {
        kernel.release(shape);
    }
    built.release_all(kernel);
    snapshot
}

/// Lays what each definition's faces are called out by the picture's own
/// numbering.
///
/// The picture is asked where each face ended up rather than told: computing
/// the identity here would be a second account of a numbering that already
/// exists, and the two would drift the first time either changed.
fn face_names(
    snapshot: &RenderSnapshot,
    named: BTreeMap<usize, Vec<Vec<BoundMeaning>>>,
) -> Result<FaceNames> {
    let mut by_face = vec![Vec::new(); snapshot.face_count()];
    for (definition, per_ordinal) in named {
        for (ordinal, bound) in per_ordinal.into_iter().enumerate() {
            let face = snapshot.face_of(definition, ordinal).ok_or_else(|| {
                CadError::topology(format!(
                    "definition {definition} was packed with more faces than the picture numbered"
                ))
            })?;
            let at = (face.to_raw() as usize)
                .checked_sub(1)
                .ok_or_else(|| CadError::topology("a face of a picture is never numbered zero"))?;
            by_face[at] = bound.into_iter().map(|named| named.meaning).collect();
        }
    }
    Ok(FaceNames {
        picture: snapshot.pick_of(0).unwrap_or(PickId::NOTHING),
        by_face,
    })
}

/// Lays what each definition's edges are called out by the picture's own
/// numbering.
///
/// The mirror of [`face_names`], and asking the picture for the same reason:
/// the numbering exists already, and computing it a second time here would be
/// a second account that drifts the first time either changes.
fn edge_names(
    snapshot: &RenderSnapshot,
    named: BTreeMap<usize, Vec<Vec<BoundMeaning>>>,
) -> Result<EdgeNames> {
    let mut by_edge = vec![Vec::new(); snapshot.edge_count()];
    for (definition, per_ordinal) in named {
        for (ordinal, bound) in per_ordinal.into_iter().enumerate() {
            let edge = snapshot.edge_of(definition, ordinal).ok_or_else(|| {
                CadError::topology(format!(
                    "definition {definition} was packed with more edges than the picture numbered"
                ))
            })?;
            let at = (edge.to_raw() as usize)
                .checked_sub(1)
                .ok_or_else(|| CadError::topology("an edge of a picture is never numbered zero"))?;
            by_edge[at] = bound.into_iter().map(|named| named.meaning).collect();
        }
    }
    Ok(EdgeNames {
        picture: snapshot.pick_of(0).unwrap_or(PickId::NOTHING),
        by_edge,
    })
}

/// Lays what each definition's corners are called out by the picture's own
/// numbering.
///
/// The mirror of [`edge_names`], and asking the picture for the same reason:
/// the numbering exists already, and computing it a second time here would be a
/// second account that drifts the first time either changes.
fn vertex_names(
    snapshot: &RenderSnapshot,
    named: BTreeMap<usize, Vec<Vec<BoundMeaning>>>,
) -> Result<VertexNames> {
    let mut by_vertex = vec![Vec::new(); snapshot.vertex_count()];
    for (definition, per_ordinal) in named {
        for (ordinal, bound) in per_ordinal.into_iter().enumerate() {
            let vertex = snapshot.vertex_of(definition, ordinal).ok_or_else(|| {
                CadError::topology(format!(
                    "definition {definition} was packed with more vertices than the picture \
                     numbered"
                ))
            })?;
            let at = (vertex.to_raw() as usize).checked_sub(1).ok_or_else(|| {
                CadError::topology("a vertex of a picture is never numbered zero")
            })?;
            by_vertex[at] = bound.into_iter().map(|named| named.meaning).collect();
        }
    }
    Ok(VertexNames {
        picture: snapshot.pick_of(0).unwrap_or(PickId::NOTHING),
        by_vertex,
    })
}

/// The exact portable meaning assigned to every packed face, edge and corner.
///
/// One digest for the whole interpretation rather than one per kind. What
/// binds a picture's transient identities is what the document says about it,
/// and that is faces, edges and corners together: moving one stored name from
/// one face to another, from one edge to another, or from one corner to
/// another, must make every identity issued under the old reading stale, and so
/// must adding a corner name to a picture whose triangles did not change.
///
/// Positions are hashed because they are what a name is attached to, and the
/// three domains are separated explicitly so a face meaning, an edge meaning
/// and a vertex meaning at the same position cannot produce the same digest.
/// The viewport sees only the result.
///
/// # Why the version moved to 2
///
/// This is a change to what the digest covers, not a fix to how it is
/// computed. A build that hashed faces and edges only would give the same
/// answer for two pictures that differ solely in what their corners are called,
/// and the transient identities bound under the old reading would go on looking
/// current. The version says the two readings are different readings rather
/// than leaving them to collide. It is local to this digest: nothing durable is
/// addressed by it, and [`RenderSnapshot`]'s own geometric algorithm version is
/// untouched because the packing did not change.
fn semantic_context_identity(
    faces: &BTreeMap<usize, Vec<Vec<BoundMeaning>>>,
    edges: &BTreeMap<usize, Vec<Vec<BoundMeaning>>>,
    vertices: &BTreeMap<usize, Vec<Vec<BoundMeaning>>>,
) -> ContentHash {
    let mut hasher = CanonicalHasher::new("ferritecad.scene.semantic-context");
    hasher.algorithm_version(2);
    for (domain, named) in [("faces", faces), ("edges", edges), ("vertices", vertices)] {
        hasher.field("domain").str(domain);
        hasher.field("definitions").u64(named.len() as u64);
        for (definition, positions) in named {
            hasher.field("definition").u64(*definition as u64);
            hasher.field("positions").u64(positions.len() as u64);
            for (position, meanings) in positions.iter().enumerate() {
                hasher.field("position").u64(position as u64);
                hasher.field("meanings").u64(meanings.len() as u64);
                for meaning in meanings {
                    hasher.hash(&meaning.identity);
                }
            }
        }
    }
    hasher.finish()
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
    // meshed again. Reusing the object's context for every definition would
    // make progress run from the beginning to the end of the same slice once
    // per part, going backwards between parts and announcing completion more
    // than once. If canonicalisation skips some or all meshes, the explicit
    // report at the end closes the part of this object's slice no kernel call
    // could report.
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

    if definitions_to_mesh == 0 || definitions_meshed < definitions_to_mesh {
        context.progress().report(1.0);
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
#[allow(clippy::panic, reason = "a gate that cannot fail is not a gate")]
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

    /// One reference and the digest it contributes.
    fn bound(role: SemanticRole) -> BoundMeaning {
        let reference = TopologyRef {
            id: StableEntityId::new(),
            owner: ObjectId::new(),
            producer_feature: ObjectId::new(),
            expected_kind: EntityKind::Edge,
            output_role: role,
            selection: SelectionRule::Exact,
            fallback_signature: None,
        };
        BoundMeaning {
            meaning: PortableMeaning::of(&reference),
            identity: reference.meaning_hash(),
        }
    }

    fn a_cap_edge(side: ferritecad_document::CapSide) -> SemanticRole {
        SemanticRole::ExtrudeCapEdge {
            side,
            profile_segment: StableEntityId::new(),
        }
    }

    fn a_cap_vertex(side: ferritecad_document::CapSide) -> SemanticRole {
        SemanticRole::ExtrudeCapVertex {
            side,
            joint: ferritecad_types::ProfileJoint::new(
                StableEntityId::new(),
                StableEntityId::new(),
            )
            .expect("two different segments"),
        }
    }

    #[test]
    fn what_binds_a_picture_covers_its_corners_as_well_as_its_edges_and_faces() {
        use ferritecad_document::CapSide;

        // One meaning per domain, deliberately built from the same role shape
        // so what tells the three apart is the domain and not the contents.
        let faces: BTreeMap<usize, Vec<Vec<BoundMeaning>>> =
            [(0usize, vec![vec![bound(a_cap_edge(CapSide::Start))]])]
                .into_iter()
                .collect();
        let edges: BTreeMap<usize, Vec<Vec<BoundMeaning>>> =
            [(0usize, vec![vec![bound(a_cap_edge(CapSide::End))]])]
                .into_iter()
                .collect();
        let vertices: BTreeMap<usize, Vec<Vec<BoundMeaning>>> =
            [(0usize, vec![vec![bound(a_cap_vertex(CapSide::Start))]])]
                .into_iter()
                .collect();
        let nothing = BTreeMap::new();

        let of = |f: &BTreeMap<usize, Vec<Vec<BoundMeaning>>>,
                  e: &BTreeMap<usize, Vec<Vec<BoundMeaning>>>,
                  v: &BTreeMap<usize, Vec<Vec<BoundMeaning>>>| {
            semantic_context_identity(f, e, v)
        };

        let bare = of(&nothing, &nothing, &nothing);
        let with_faces = of(&faces, &nothing, &nothing);
        let with_edges = of(&nothing, &edges, &nothing);
        let with_vertices = of(&nothing, &nothing, &vertices);
        let with_all = of(&faces, &edges, &vertices);

        // Adding a name of any kind changes what the picture means, and the
        // three are told apart: one meaning filed as a face, as an edge and as
        // a corner are three interpretations of the same geometry.
        for (what, digest) in [
            ("faces", with_faces),
            ("edges", with_edges),
            ("vertices", with_vertices),
            ("all three", with_all),
        ] {
            assert_ne!(bare, digest, "adding {what} left the identity alone");
        }
        for (one, other) in [
            (with_faces, with_edges),
            (with_faces, with_vertices),
            (with_edges, with_vertices),
        ] {
            assert_ne!(one, other, "the three domains are not separated");
        }
        for domain in [with_faces, with_edges, with_vertices] {
            assert_ne!(with_all, domain);
        }

        // Adding only corner names to a picture whose faces and edges are
        // already named changes it too, which is the case a digest covering
        // two domains would miss.
        assert_ne!(
            of(&faces, &edges, &nothing),
            of(&faces, &edges, &vertices),
            "naming the corners of an already-named picture left the identity alone"
        );

        // And the position a name sits at is part of it, in the new domain as
        // in the old ones. Both readings below have two corners and one name,
        // so the counts are identical and only where the name sits differs.
        let one = vertices[&0][0].clone();
        let first: BTreeMap<usize, Vec<Vec<BoundMeaning>>> =
            [(0usize, vec![one.clone(), Vec::new()])]
                .into_iter()
                .collect();
        let second: BTreeMap<usize, Vec<Vec<BoundMeaning>>> =
            [(0usize, vec![Vec::new(), one])].into_iter().collect();
        assert_ne!(
            of(&nothing, &nothing, &first),
            of(&nothing, &nothing, &second),
            "moving a name to another corner left the identity alone"
        );

        // The same for edges, unchanged from before corners existed.
        let one = edges[&0][0].clone();
        let first: BTreeMap<usize, Vec<Vec<BoundMeaning>>> =
            [(0usize, vec![one.clone(), Vec::new()])]
                .into_iter()
                .collect();
        let second: BTreeMap<usize, Vec<Vec<BoundMeaning>>> =
            [(0usize, vec![Vec::new(), one])].into_iter().collect();
        assert_ne!(
            of(&nothing, &first, &nothing),
            of(&nothing, &second, &nothing),
            "moving a name to another edge left the identity alone"
        );
    }

    #[test]
    fn an_edge_name_of_another_picture_names_nothing_here() {
        let shape = ShapeHandle::new(ferritecad_kernel::SessionId::new(), 1);
        let mesh = |ordinal: u64| Mesh {
            topological_vertices: None,
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![ferritecad_kernel::MeshFaceRange {
                face: SubShapeHandle::new(shape, ferritecad_kernel::SubShapeKind::Face, 0),
                first_index: 0,
                index_count: 3,
            }],
            edges: Some(ferritecad_kernel::MeshEdges {
                segments: vec![0, 1],
                ranges: vec![ferritecad_kernel::MeshEdgeRange {
                    edge: SubShapeHandle::new(
                        shape,
                        ferritecad_kernel::SubShapeKind::Edge,
                        ordinal,
                    ),
                    first_segment: 0,
                    segment_count: 1,
                }],
            }),
        };

        // Two pictures of the same geometry under different interpretations.
        let picture = |role: SemanticRole| {
            let mut builder = SnapshotBuilder::new();
            let definition = builder.add_mesh(&mesh(0)).expect("packs");
            builder
                .place(definition, None, &Transform::IDENTITY, BODY_COLOUR)
                .expect("places");
            let named: BTreeMap<usize, Vec<Vec<BoundMeaning>>> =
                [(0usize, vec![vec![bound(role)]])].into_iter().collect();
            builder
                .bind_identities_to(semantic_context_identity(
                    &BTreeMap::new(),
                    &named,
                    &BTreeMap::new(),
                ))
                .expect("binds");
            let snapshot = builder.build();
            let names = edge_names(&snapshot, named).expect("lays out");
            (snapshot, names)
        };

        use ferritecad_document::CapSide;
        let (mine, my_names) = picture(a_cap_edge(CapSide::Start));
        let (theirs, their_names) = picture(a_cap_edge(CapSide::End));

        let my_edge = mine.edge_of(0, 0).expect("numbered");
        let their_edge = theirs.edge_of(0, 0).expect("numbered");
        assert_eq!(my_edge.to_raw(), their_edge.to_raw(), "the same raw value");
        assert_ne!(my_edge, their_edge, "and different pictures");

        assert_eq!(my_names.of(my_edge, &mine).len(), 1);
        assert_eq!(their_names.of(their_edge, &theirs).len(), 1);
        // In range and from another picture: still nothing.
        assert!(my_names.of(their_edge, &mine).is_empty());
        assert!(their_names.of(my_edge, &theirs).is_empty());
        // And a name table of one picture answers nothing about another.
        assert!(my_names.of(their_edge, &theirs).is_empty());
    }

    /// One triangle whose corners the kernel reports, or does not.
    ///
    /// `corners` is how many topological vertices the association claims;
    /// `None` is a mesh with no association at all, which is a different
    /// statement from an association that names nothing.
    fn a_mesh_with_corners(shape: ShapeHandle, corners: Option<usize>) -> Mesh {
        Mesh {
            topological_vertices: corners.map(|count| ferritecad_kernel::MeshVertices {
                occurrences: (0..count as u32).collect(),
                ranges: (0..count)
                    .map(|ordinal| ferritecad_kernel::MeshVertexRange {
                        vertex: SubShapeHandle::new(
                            shape,
                            ferritecad_kernel::SubShapeKind::Vertex,
                            ordinal as u64,
                        ),
                        first_occurrence: ordinal as u32,
                        occurrence_count: 1,
                    })
                    .collect(),
            }),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![ferritecad_kernel::MeshFaceRange {
                face: SubShapeHandle::new(shape, ferritecad_kernel::SubShapeKind::Face, 0),
                first_index: 0,
                index_count: 3,
            }],
            edges: None,
        }
    }

    #[test]
    fn a_corner_name_of_another_picture_names_nothing_here() {
        use ferritecad_document::CapSide;

        let shape = ShapeHandle::new(ferritecad_kernel::SessionId::new(), 1);
        // Two pictures of the same geometry under different interpretations.
        let picture = |role: SemanticRole| {
            let mut builder = SnapshotBuilder::new();
            let definition = builder
                .add_mesh(&a_mesh_with_corners(shape, Some(1)))
                .expect("packs");
            builder
                .place(definition, None, &Transform::IDENTITY, BODY_COLOUR)
                .expect("places");
            let named: BTreeMap<usize, Vec<Vec<BoundMeaning>>> =
                [(0usize, vec![vec![bound(role)]])].into_iter().collect();
            builder
                .bind_identities_to(semantic_context_identity(
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                    &named,
                ))
                .expect("binds");
            let snapshot = builder.build();
            let names = vertex_names(&snapshot, named).expect("lays out");
            (snapshot, names)
        };

        let (mine, my_names) = picture(a_cap_vertex(CapSide::Start));
        let (theirs, their_names) = picture(a_cap_vertex(CapSide::End));

        let my_corner = mine.vertex_of(0, 0).expect("numbered");
        let their_corner = theirs.vertex_of(0, 0).expect("numbered");
        assert_eq!(
            my_corner.to_raw(),
            their_corner.to_raw(),
            "the same raw value"
        );
        assert_ne!(my_corner, their_corner, "and different pictures");

        assert_eq!(my_names.of(my_corner, &mine).len(), 1);
        assert_eq!(their_names.of(their_corner, &theirs).len(), 1);
        // In range and from another picture: still nothing.
        assert!(my_names.of(their_corner, &mine).is_empty());
        assert!(their_names.of(my_corner, &theirs).is_empty());
        // And a name table of one picture answers nothing about another.
        assert!(my_names.of(their_corner, &theirs).is_empty());
        // Nothing is nothing, in either picture.
        assert!(my_names.of(VertexPickId::NOTHING, &mine).is_empty());
    }

    #[test]
    fn a_mesh_that_names_no_corner_gets_no_invented_one() {
        use ferritecad_document::CapSide;

        let shape = ShapeHandle::new(ferritecad_kernel::SessionId::new(), 1);
        // No association at all, and an association that is provably empty.
        // Both are "this picture knows of no corner here", and neither is "a
        // corner nobody named".
        for corners in [None, Some(0)] {
            let mut builder = SnapshotBuilder::new();
            let definition = builder
                .add_mesh(&a_mesh_with_corners(shape, corners))
                .expect("packs");
            builder
                .place(definition, None, &Transform::IDENTITY, BODY_COLOUR)
                .expect("places");
            builder
                .bind_identities_to(semantic_context_identity(
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                ))
                .expect("binds");
            let snapshot = builder.build();
            assert_eq!(
                snapshot.vertex_count(),
                0,
                "{corners:?}: the picture numbered a corner nobody reported"
            );
            assert!(snapshot.vertex_of(0, 0).is_none(), "{corners:?}");

            let names = vertex_names(&snapshot, BTreeMap::new()).expect("lays out");
            assert!(
                names.of(VertexPickId::NOTHING, &snapshot).is_empty(),
                "{corners:?}"
            );

            // And a name table built for a picture that numbers no corner has
            // nothing to hand back however it is asked.
            let named: BTreeMap<usize, Vec<Vec<BoundMeaning>>> =
                [(0usize, vec![vec![bound(a_cap_vertex(CapSide::Start))]])]
                    .into_iter()
                    .collect();
            assert!(
                vertex_names(&snapshot, named).is_err(),
                "{corners:?}: a name was laid out against a corner the picture never numbered"
            );
        }
    }

    #[test]
    fn a_corner_of_the_second_definition_is_not_the_second_corner_of_the_picture() {
        use ferritecad_document::CapSide;

        // Two definitions, three corners each. The second definition's first
        // corner is the picture's fourth, so a table that used the ordinal as
        // the picture-wide number would name the wrong one.
        let first = ShapeHandle::new(ferritecad_kernel::SessionId::new(), 1);
        let second = ShapeHandle::new(ferritecad_kernel::SessionId::new(), 2);

        let mut builder = SnapshotBuilder::new();
        let one = builder
            .add_mesh(&a_mesh_with_corners(first, Some(3)))
            .expect("packs");
        let other = builder
            .add_mesh(&a_mesh_with_corners(second, Some(3)))
            .expect("packs");
        builder
            .place(one, None, &Transform::IDENTITY, BODY_COLOUR)
            .expect("places");
        builder
            .place(other, None, &Transform::IDENTITY, BODY_COLOUR)
            .expect("places");

        // Only the second definition's first corner is named.
        let named: BTreeMap<usize, Vec<Vec<BoundMeaning>>> = [(
            other,
            vec![
                vec![bound(a_cap_vertex(CapSide::Start))],
                Vec::new(),
                Vec::new(),
            ],
        )]
        .into_iter()
        .collect();
        builder
            .bind_identities_to(semantic_context_identity(
                &BTreeMap::new(),
                &BTreeMap::new(),
                &named,
            ))
            .expect("binds");
        let snapshot = builder.build();
        assert_eq!(snapshot.vertex_count(), 6);

        let names = vertex_names(&snapshot, named).expect("lays out");
        let theirs = snapshot.vertex_of(other, 0).expect("numbered");
        assert_eq!(
            theirs.to_raw(),
            4,
            "the second definition's first corner is the picture's fourth"
        );
        assert_eq!(names.of(theirs, &snapshot).len(), 1);
        for ordinal in 0..3 {
            let mine = snapshot.vertex_of(one, ordinal).expect("numbered");
            assert!(
                names.of(mine, &snapshot).is_empty(),
                "corner {ordinal} of the first definition took the second's name"
            );
        }
    }

    #[test]
    fn every_occurrence_and_every_placement_of_one_corner_is_one_name() {
        use ferritecad_document::CapSide;

        // One topological vertex drawn at three packed positions, in a
        // definition placed twice. That is one corner, one identity and one
        // list of names however many times it appears.
        let shape = ShapeHandle::new(ferritecad_kernel::SessionId::new(), 1);
        let mesh = Mesh {
            topological_vertices: Some(ferritecad_kernel::MeshVertices {
                occurrences: vec![0, 1, 2],
                ranges: vec![ferritecad_kernel::MeshVertexRange {
                    vertex: SubShapeHandle::new(shape, ferritecad_kernel::SubShapeKind::Vertex, 0),
                    first_occurrence: 0,
                    occurrence_count: 3,
                }],
            }),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![ferritecad_kernel::MeshFaceRange {
                face: SubShapeHandle::new(shape, ferritecad_kernel::SubShapeKind::Face, 0),
                first_index: 0,
                index_count: 3,
            }],
            edges: None,
        };

        let mut builder = SnapshotBuilder::new();
        let definition = builder.add_mesh(&mesh).expect("packs");
        for _ in 0..2 {
            builder
                .place(definition, None, &Transform::IDENTITY, BODY_COLOUR)
                .expect("places");
        }
        let named: BTreeMap<usize, Vec<Vec<BoundMeaning>>> =
            [(0usize, vec![vec![bound(a_cap_vertex(CapSide::Start))]])]
                .into_iter()
                .collect();
        builder
            .bind_identities_to(semantic_context_identity(
                &BTreeMap::new(),
                &BTreeMap::new(),
                &named,
            ))
            .expect("binds");
        let snapshot = builder.build();

        assert_eq!(
            snapshot.vertex_count(),
            1,
            "three occurrences and two placements are still one corner"
        );
        assert_eq!(snapshot.draws().len(), 2);
        let corner = snapshot.vertex_of(0, 0).expect("numbered");
        assert_eq!(
            snapshot.occurrences_of_vertex(corner).map(<[u32]>::len),
            Some(3)
        );
        let names = vertex_names(&snapshot, named).expect("lays out");
        assert_eq!(names.of(corner, &snapshot).len(), 1);
    }

    #[test]
    fn naming_only_the_corners_makes_every_older_identity_stale() {
        use ferritecad_document::CapSide;

        // The same triangles, the same faces, the same edges and the same
        // corners; only what the document calls one corner differs. Every
        // identity the picture issues must change, because a face value, an
        // edge value and a corner value carried over from the old reading
        // would all now point into a picture that means something else.
        let shape = ShapeHandle::new(ferritecad_kernel::SessionId::new(), 1);
        let mesh = Mesh {
            topological_vertices: Some(ferritecad_kernel::MeshVertices {
                occurrences: vec![0, 1],
                ranges: (0..2)
                    .map(|ordinal| ferritecad_kernel::MeshVertexRange {
                        vertex: SubShapeHandle::new(
                            shape,
                            ferritecad_kernel::SubShapeKind::Vertex,
                            ordinal,
                        ),
                        first_occurrence: ordinal as u32,
                        occurrence_count: 1,
                    })
                    .collect(),
            }),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![ferritecad_kernel::MeshFaceRange {
                face: SubShapeHandle::new(shape, ferritecad_kernel::SubShapeKind::Face, 0),
                first_index: 0,
                index_count: 3,
            }],
            edges: Some(ferritecad_kernel::MeshEdges {
                segments: vec![0, 1],
                ranges: vec![ferritecad_kernel::MeshEdgeRange {
                    edge: SubShapeHandle::new(shape, ferritecad_kernel::SubShapeKind::Edge, 0),
                    first_segment: 0,
                    segment_count: 1,
                }],
            }),
        };

        let faces: BTreeMap<usize, Vec<Vec<BoundMeaning>>> =
            [(0usize, vec![vec![bound(a_cap_edge(CapSide::Start))]])]
                .into_iter()
                .collect();
        let edges: BTreeMap<usize, Vec<Vec<BoundMeaning>>> =
            [(0usize, vec![vec![bound(a_cap_edge(CapSide::End))]])]
                .into_iter()
                .collect();

        let picture = |corner: SemanticRole| {
            let mut builder = SnapshotBuilder::new();
            let definition = builder.add_mesh(&mesh).expect("packs");
            builder
                .place(definition, None, &Transform::IDENTITY, BODY_COLOUR)
                .expect("places");
            let vertices: BTreeMap<usize, Vec<Vec<BoundMeaning>>> =
                [(0usize, vec![vec![bound(corner)], Vec::new()])]
                    .into_iter()
                    .collect();
            builder
                .bind_identities_to(semantic_context_identity(&faces, &edges, &vertices))
                .expect("binds");
            builder.build()
        };

        let before = picture(a_cap_vertex(CapSide::Start));
        let after = picture(a_cap_vertex(CapSide::End));

        // The geometry really is identical, so nothing but the reading can be
        // telling the two apart.
        assert_eq!(before.meshes().len(), after.meshes().len());
        assert_eq!(before.face_count(), after.face_count());
        assert_eq!(before.edge_count(), after.edge_count());
        assert_eq!(before.vertex_count(), after.vertex_count());

        let old_face = before.face_of(0, 0).expect("numbered");
        let old_edge = before.edge_of(0, 0).expect("numbered");
        let old_corner = before.vertex_of(0, 0).expect("numbered");
        assert!(
            after.definition_of_face(old_face).is_none(),
            "a face identity of the old reading still resolves"
        );
        assert!(
            after.definition_of_edge(old_edge).is_none(),
            "an edge identity of the old reading still resolves"
        );
        assert!(
            after.definition_of_vertex(old_corner).is_none(),
            "a corner identity of the old reading still resolves"
        );

        // And each still resolves in the picture that issued it, so the
        // refusals above are about the reading and not about the values.
        assert!(before.definition_of_face(old_face).is_some());
        assert!(before.definition_of_edge(old_edge).is_some());
        assert!(before.definition_of_vertex(old_corner).is_some());
    }

    #[test]
    fn a_document_whose_kernel_names_no_edges_loads_and_names_none() {
        // The mock reports no edge association at all, which is the honest
        // thing for it to say. The loader must carry that through: no picture
        // edge, no entry, and no failure.
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("plate.fcad");
        std::fs::copy(ferritecad_fixtures::plate_source(), &path).expect("copies the fixture");

        let mut kernel = MockKernel::new();
        let scene = snapshot_of(
            &path,
            &mut kernel,
            no_imports,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("the plate loads through the mock");

        assert_eq!(
            scene.snapshot.edge_count(),
            0,
            "the mock named no topological edge, so the picture numbers none"
        );
        assert!(scene.snapshot.edge_of(0, 0).is_none());
        assert!(
            scene
                .edges
                .of(EdgePickId::NOTHING, &scene.snapshot)
                .is_empty()
        );

        // The same for the corners, and asked through the loader rather than
        // through `vertex_names` directly: a picture nobody associated must
        // come out of a real load with nothing invented, not merely out of the
        // layout function.
        assert_eq!(
            scene.snapshot.vertex_count(),
            0,
            "the mock named no topological vertex, so the picture numbers none"
        );
        assert!(scene.snapshot.vertex_of(0, 0).is_none());
        assert!(
            scene
                .vertices
                .of(VertexPickId::NOTHING, &scene.snapshot)
                .is_empty()
        );

        // And the faces are still named exactly as before this slice.
        assert!(
            scene.snapshot.face_count() > 0,
            "the faces of the plate are still numbered"
        );
    }

    /// A picture with one edge the document names, and one it does not.
    ///
    /// Two edges of one definition: the first carries a durable name, the
    /// second carries none. The face carries one too, so "the edge wins" is a
    /// statement with content rather than the only answer available.
    fn a_named_edge() -> (RenderSnapshot, FaceNames, EdgeNames) {
        use ferritecad_document::CapSide;

        let shape = ShapeHandle::new(ferritecad_kernel::SessionId::new(), 1);
        let mesh = Mesh {
            topological_vertices: None,
            positions: vec![
                0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 2.0, 0.0, 0.0, 3.0, 0.0, 0.0, 2.0,
                1.0, 0.0,
            ],
            normals: vec![
                0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0,
                0.0, 1.0,
            ],
            indices: vec![0, 1, 2, 3, 4, 5],
            faces: (0..2)
                .map(|ordinal| ferritecad_kernel::MeshFaceRange {
                    face: SubShapeHandle::new(
                        shape,
                        ferritecad_kernel::SubShapeKind::Face,
                        ordinal,
                    ),
                    first_index: ordinal as u32 * 3,
                    index_count: 3,
                })
                .collect(),
            edges: Some(ferritecad_kernel::MeshEdges {
                segments: vec![0, 1, 1, 2],
                ranges: (0..2)
                    .map(|ordinal| ferritecad_kernel::MeshEdgeRange {
                        edge: SubShapeHandle::new(
                            shape,
                            ferritecad_kernel::SubShapeKind::Edge,
                            ordinal,
                        ),
                        first_segment: ordinal as u32,
                        segment_count: 1,
                    })
                    .collect(),
            }),
        };

        let mut builder = SnapshotBuilder::new();
        let definition = builder.add_mesh(&mesh).expect("packs");
        builder
            .place(definition, None, &Transform::IDENTITY, BODY_COLOUR)
            .expect("places");
        // Three stored names for the first edge, none for the second.
        let three: Vec<BoundMeaning> = (0..3).map(|_| bound(a_cap_edge(CapSide::Start))).collect();
        let edge_named: BTreeMap<usize, Vec<Vec<BoundMeaning>>> =
            [(definition, vec![three, Vec::new()])]
                .into_iter()
                .collect();
        let face_named: BTreeMap<usize, Vec<Vec<BoundMeaning>>> = [(
            definition,
            vec![vec![bound(a_cap_edge(CapSide::End))], Vec::new()],
        )]
        .into_iter()
        .collect();
        builder
            .bind_identities_to(semantic_context_identity(
                &face_named,
                &edge_named,
                &BTreeMap::new(),
            ))
            .expect("binds");
        let snapshot = builder.build();
        let faces = face_names(&snapshot, face_named).expect("lays out");
        let edges = edge_names(&snapshot, edge_named).expect("lays out");
        (snapshot, faces, edges)
    }

    /// A picture with two faces, two edges and three corners, of which two
    /// carry exact durable names.
    ///
    /// Corner 0 is drawn twice - once in each face, as a corner two faces meet
    /// at is - touches both faces, ends the first edge and not the second, and
    /// carries three stored names. Corner 1 is unnamed on purpose. Corner 2
    /// touches only the second face, ends no edge, and carries one name, which
    /// is what lets a corner be offered against a face it does not touch.
    ///
    /// The definition is placed twice so extents have both occurrences and
    /// both placements to cover.
    fn a_named_vertex() -> (RenderSnapshot, FaceNames, EdgeNames, VertexNames) {
        use ferritecad_document::CapSide;

        let shape = ShapeHandle::new(ferritecad_kernel::SessionId::new(), 1);
        let mesh = Mesh {
            // Positions 0 and 3 are one model point drawn in both faces.
            positions: vec![
                0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 2.0,
                1.0, 0.0,
            ],
            normals: vec![
                0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0,
                0.0, 1.0,
            ],
            indices: vec![0, 1, 2, 3, 4, 5],
            faces: (0..2)
                .map(|ordinal| ferritecad_kernel::MeshFaceRange {
                    face: SubShapeHandle::new(
                        shape,
                        ferritecad_kernel::SubShapeKind::Face,
                        ordinal,
                    ),
                    first_index: ordinal as u32 * 3,
                    index_count: 3,
                })
                .collect(),
            edges: Some(ferritecad_kernel::MeshEdges {
                segments: vec![0, 1, 1, 2],
                ranges: (0..2)
                    .map(|ordinal| ferritecad_kernel::MeshEdgeRange {
                        edge: SubShapeHandle::new(
                            shape,
                            ferritecad_kernel::SubShapeKind::Edge,
                            ordinal,
                        ),
                        first_segment: ordinal as u32,
                        segment_count: 1,
                    })
                    .collect(),
            }),
            topological_vertices: Some(ferritecad_kernel::MeshVertices {
                occurrences: vec![0, 3, 2, 5],
                ranges: vec![
                    ferritecad_kernel::MeshVertexRange {
                        vertex: SubShapeHandle::new(
                            shape,
                            ferritecad_kernel::SubShapeKind::Vertex,
                            0,
                        ),
                        first_occurrence: 0,
                        occurrence_count: 2,
                    },
                    ferritecad_kernel::MeshVertexRange {
                        vertex: SubShapeHandle::new(
                            shape,
                            ferritecad_kernel::SubShapeKind::Vertex,
                            1,
                        ),
                        first_occurrence: 2,
                        occurrence_count: 1,
                    },
                    ferritecad_kernel::MeshVertexRange {
                        vertex: SubShapeHandle::new(
                            shape,
                            ferritecad_kernel::SubShapeKind::Vertex,
                            2,
                        ),
                        first_occurrence: 3,
                        occurrence_count: 1,
                    },
                ],
            }),
        };

        let mut builder = SnapshotBuilder::new();
        let definition = builder.add_mesh(&mesh).expect("packs");
        for x in [0.0, 10.0] {
            builder
                .place(
                    definition,
                    None,
                    &Transform::from_translation(
                        ferritecad_types::Vec3::new(x, 0.0, 0.0).expect("finite"),
                    )
                    .expect("finite"),
                    BODY_COLOUR,
                )
                .expect("places");
        }
        // Three stored names for the first corner, none for the second, one
        // for the third.
        let three: Vec<BoundMeaning> = (0..3)
            .map(|_| bound(a_cap_vertex(CapSide::Start)))
            .collect();
        let vertex_named: BTreeMap<usize, Vec<Vec<BoundMeaning>>> = [(
            definition,
            vec![three, Vec::new(), vec![bound(a_cap_vertex(CapSide::End))]],
        )]
        .into_iter()
        .collect();
        let edge_named: BTreeMap<usize, Vec<Vec<BoundMeaning>>> = [(
            definition,
            vec![vec![bound(a_cap_edge(CapSide::Start))], Vec::new()],
        )]
        .into_iter()
        .collect();
        let face_named: BTreeMap<usize, Vec<Vec<BoundMeaning>>> = [(
            definition,
            vec![vec![bound(a_cap_edge(CapSide::End))], Vec::new()],
        )]
        .into_iter()
        .collect();
        builder
            .bind_identities_to(semantic_context_identity(
                &face_named,
                &edge_named,
                &vertex_named,
            ))
            .expect("binds");
        let snapshot = builder.build();
        let faces = face_names(&snapshot, face_named).expect("lays out");
        let edges = edge_names(&snapshot, edge_named).expect("lays out");
        let vertices = vertex_names(&snapshot, vertex_named).expect("lays out");
        (snapshot, faces, edges, vertices)
    }

    #[test]
    fn a_named_corner_is_chosen_before_the_edge_and_the_face_it_lies_on() {
        let (snapshot, faces, edges, vertices) = a_named_vertex();
        let definition = snapshot.pick_of(0).expect("drawn");
        let face = snapshot.face_of(0, 0).expect("numbered");
        let edge = snapshot.edge_of(0, 0).expect("numbered");
        let named = snapshot.vertex_of(0, 0).expect("numbered");
        let unnamed = snapshot.vertex_of(0, 1).expect("numbered");
        assert!(
            !faces.of(face, &snapshot).is_empty() && !edges.of(edge, &snapshot).is_empty(),
            "the edge and the face beneath the corner are named too"
        );
        assert!(snapshot.vertex_touches_face(named, face));
        assert!(snapshot.vertex_ends_edge(named, edge));

        // The corner wins over both.
        let chosen = Selection::at(
            definition, face, edge, named, &snapshot, &faces, &edges, &vertices,
        );
        let Selection::Vertex(corner) = &chosen else {
            panic!("a named corner did not win over a named edge and face: {chosen:?}")
        };
        assert_eq!(corner.vertex(), named);
        assert_eq!(corner.definition(), definition);
        // All three names, in the order they were stored.
        assert_eq!(corner.meanings().len(), 3);
        assert_eq!(corner.meanings(), vertices.of(named, &snapshot));
        // And the three answers about one choice agree.
        assert_eq!(chosen.marked(), ferritecad_viewport::Marked::Vertex(named));
        assert_eq!(chosen.owning_definition(&snapshot), Some(0));
        assert_eq!(chosen.bounds(&snapshot), snapshot.bounds_of_vertex(named));
        assert_ne!(chosen.bounds(&snapshot), snapshot.bounds_of_edge(edge));
        assert_ne!(chosen.bounds(&snapshot), snapshot.bounds_of_face(face));
        assert_ne!(chosen.bounds(&snapshot), snapshot.bounds_of(definition));

        // A corner nobody named is not a lesser corner; it is not a choice,
        // and the named edge beneath it is.
        assert!(snapshot.vertex_ends_edge(unnamed, snapshot.edge_of(0, 1).expect("numbered")));
        let fallback = Selection::at(
            definition, face, edge, unnamed, &snapshot, &faces, &edges, &vertices,
        );
        assert!(
            matches!(fallback, Selection::Edge(_)),
            "an unnamed corner chose {fallback:?}"
        );
        // With no named edge under it either, the named face.
        let onto_face = Selection::at(
            definition,
            face,
            snapshot.edge_of(0, 1).expect("numbered"),
            unnamed,
            &snapshot,
            &faces,
            &edges,
            &vertices,
        );
        assert!(
            matches!(onto_face, Selection::Face(_)),
            "an unnamed corner over an unnamed edge chose {onto_face:?}"
        );
        // And with nothing named at all, the part.
        let bare = Selection::at(
            definition,
            face,
            snapshot.edge_of(0, 1).expect("numbered"),
            unnamed,
            &snapshot,
            &FaceNames::default(),
            &edges,
            &vertices,
        );
        assert!(matches!(bare, Selection::Definition(_)), "{bare:?}");
        // A named corner with no names in hand falls through the same way,
        // which is what an imported or family-named corner produces.
        let nameless = Selection::at(
            definition,
            face,
            edge,
            named,
            &snapshot,
            &faces,
            &edges,
            &VertexNames::default(),
        );
        assert!(
            matches!(nameless, Selection::Edge(_)),
            "a corner with no durable name chose {nameless:?}"
        );
    }

    #[test]
    fn a_corner_that_contradicts_its_pixel_chooses_no_corner() {
        let (snapshot, faces, edges, vertices) = a_named_vertex();
        let definition = snapshot.pick_of(0).expect("drawn");
        let face = snapshot.face_of(0, 0).expect("numbered");
        let other_face = snapshot.face_of(0, 1).expect("numbered");
        let edge = snapshot.edge_of(0, 0).expect("numbered");
        let other_edge = snapshot.edge_of(0, 1).expect("numbered");
        let named = snapshot.vertex_of(0, 0).expect("numbered");
        // The third corner is named and touches only the second face.
        let elsewhere = snapshot.vertex_of(0, 2).expect("numbered");
        assert!(!vertices.of(elsewhere, &snapshot).is_empty());
        assert!(!snapshot.vertex_touches_face(elsewhere, face));
        assert!(!snapshot.vertex_ends_edge(named, other_edge));

        // A second picture whose raw values are in range here.
        let (other, _, _, other_vertices) = a_named_vertex();
        let foreign = other.vertex_of(0, 0).expect("numbered");
        assert_eq!(foreign.to_raw(), named.to_raw(), "the same raw value");
        assert!(
            !other_vertices.of(foreign, &other).is_empty(),
            "and named in its own picture"
        );

        for (what, definition, face, edge, vertex) in [
            (
                "a corner of another picture",
                definition,
                face,
                edge,
                foreign,
            ),
            (
                "nothing at all",
                definition,
                face,
                edge,
                VertexPickId::NOTHING,
            ),
            (
                "a definition of nothing",
                PickId::NOTHING,
                face,
                edge,
                named,
            ),
            (
                "a corner that does not touch the pixel's face",
                definition,
                face,
                EdgePickId::NOTHING,
                elsewhere,
            ),
            (
                "a corner that does not end the pixel's edge",
                definition,
                face,
                other_edge,
                named,
            ),
        ] {
            let chosen = Selection::at(
                definition, face, edge, vertex, &snapshot, &faces, &edges, &vertices,
            );
            assert!(
                !matches!(chosen, Selection::Vertex(_)),
                "{what} assembled a corner: {chosen:?}"
            );
        }

        // The third corner is chosen where the pixel really is on its face.
        let honest = Selection::at(
            definition,
            other_face,
            EdgePickId::NOTHING,
            elsewhere,
            &snapshot,
            &faces,
            &edges,
            &vertices,
        );
        assert!(
            matches!(honest, Selection::Vertex(_)),
            "a coherent corner was refused: {honest:?}"
        );
    }

    #[test]
    fn a_corner_of_the_second_definition_is_chosen_by_the_pictures_numbering() {
        use ferritecad_document::CapSide;

        // Two definitions, the first with three corners and the second with
        // one. The second definition's only corner is the picture's fourth, so
        // a rule using the ordinal within the definition would choose the
        // first definition's first corner instead.
        let (first_snapshot, ..) = a_named_vertex();
        let _ = first_snapshot;

        let shape = ShapeHandle::new(ferritecad_kernel::SessionId::new(), 1);
        let mesh = |corners: usize| Mesh {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![ferritecad_kernel::MeshFaceRange {
                face: SubShapeHandle::new(shape, ferritecad_kernel::SubShapeKind::Face, 0),
                first_index: 0,
                index_count: 3,
            }],
            edges: None,
            topological_vertices: Some(ferritecad_kernel::MeshVertices {
                occurrences: (0..corners as u32).collect(),
                ranges: (0..corners)
                    .map(|ordinal| ferritecad_kernel::MeshVertexRange {
                        vertex: SubShapeHandle::new(
                            shape,
                            ferritecad_kernel::SubShapeKind::Vertex,
                            ordinal as u64,
                        ),
                        first_occurrence: ordinal as u32,
                        occurrence_count: 1,
                    })
                    .collect(),
            }),
        };

        let mut builder = SnapshotBuilder::new();
        let first = builder.add_mesh(&mesh(3)).expect("packs");
        let second = builder.add_mesh(&mesh(1)).expect("packs");
        for definition in [first, second] {
            builder
                .place(definition, None, &Transform::IDENTITY, BODY_COLOUR)
                .expect("places");
        }
        let named: BTreeMap<usize, Vec<Vec<BoundMeaning>>> = [
            (first, vec![Vec::new(), Vec::new(), Vec::new()]),
            (second, vec![vec![bound(a_cap_vertex(CapSide::End))]]),
        ]
        .into_iter()
        .collect();
        builder
            .bind_identities_to(semantic_context_identity(
                &BTreeMap::new(),
                &BTreeMap::new(),
                &named,
            ))
            .expect("binds");
        let snapshot = builder.build();
        assert_eq!(snapshot.vertex_count(), 4);

        let vertices = vertex_names(&snapshot, named).expect("lays out");
        let theirs = snapshot.vertex_of(second, 0).expect("numbered");
        let their_face = snapshot.face_of(second, 0).expect("numbered");
        let their_pick = snapshot.pick_of(second).expect("drawn");
        let chosen = Selection::at(
            their_pick,
            their_face,
            EdgePickId::NOTHING,
            theirs,
            &snapshot,
            &FaceNames::default(),
            &EdgeNames::default(),
            &vertices,
        );
        let Selection::Vertex(corner) = &chosen else {
            panic!("the second definition's corner was not chosen: {chosen:?}")
        };
        assert_eq!(corner.vertex(), theirs);
        assert_eq!(chosen.owning_definition(&snapshot), Some(second));

        // And the first definition's corners are not chooseable through it.
        for ordinal in 0..3 {
            let mine = snapshot.vertex_of(first, ordinal).expect("numbered");
            assert!(
                vertices.of(mine, &snapshot).is_empty(),
                "a name landed on the first definition's corner {ordinal}"
            );
        }
    }

    #[test]
    fn what_a_chosen_corner_is_cannot_be_taken_apart_or_reassembled() {
        let (snapshot, faces, edges, vertices) = a_named_vertex();
        let definition = snapshot.pick_of(0).expect("drawn");
        let face = snapshot.face_of(0, 0).expect("numbered");
        let edge = snapshot.edge_of(0, 0).expect("numbered");
        let named = snapshot.vertex_of(0, 0).expect("numbered");
        let chosen = Selection::at(
            definition, face, edge, named, &snapshot, &faces, &edges, &vertices,
        );
        let Selection::Vertex(corner) = &chosen else {
            panic!("{chosen:?}")
        };

        // The three parts are one decision: the corner really belongs to the
        // definition beside it, and the meanings really are that corner's.
        assert_eq!(
            snapshot.definition_of_vertex(corner.vertex()),
            snapshot.definition(corner.definition()),
            "a chosen corner and its definition disagree"
        );
        assert_eq!(corner.meanings(), vertices.of(corner.vertex(), &snapshot));
        assert!(!corner.meanings().is_empty());

        // There is no way to build one out of parts that disagree: the fields
        // are private and `Selection::at` is the only constructor. That is a
        // property of the type rather than of a run, so what is checked here
        // is that the only route refuses every incoherent tuple - which is
        // what `a_corner_that_contradicts_its_pixel_chooses_no_corner` states
        // case by case.
        //
        // What is transient stays out of what it is called: a portable meaning
        // holds no identity of this picture.
        let shown = format!("{:?}", corner.meanings());
        for leak in [
            "VertexPickId",
            "FacePickId",
            "EdgePickId",
            "PickId",
            "SubShapeHandle",
            "ShapeHandle",
            "SessionId",
        ] {
            assert!(
                !shown.contains(leak),
                "{leak} reached what a corner is called"
            );
        }
    }

    #[test]
    fn a_named_edge_is_chosen_before_the_face_it_lies_on() {
        let (snapshot, faces, edges) = a_named_edge();
        let definition = snapshot.pick_of(0).expect("drawn");
        let face = snapshot.face_of(0, 0).expect("numbered");
        let named = snapshot.edge_of(0, 0).expect("numbered");
        let unnamed = snapshot.edge_of(0, 1).expect("numbered");
        assert!(
            !faces.of(face, &snapshot).is_empty(),
            "the face is named too"
        );

        // The edge wins.
        let chosen = Selection::at(
            definition,
            face,
            named,
            VertexPickId::NOTHING,
            &snapshot,
            &faces,
            &edges,
            &VertexNames::default(),
        );
        let Selection::Edge(edge) = &chosen else {
            panic!("a named edge did not win over a named face: {chosen:?}")
        };
        assert_eq!(edge.edge(), named);
        assert_eq!(edge.definition(), definition);
        // All three names, in the order they were stored.
        assert_eq!(edge.meanings().len(), 3);
        assert_eq!(edge.meanings(), edges.of(named, &snapshot));
        // And the three answers about one choice agree.
        assert_eq!(chosen.marked(), ferritecad_viewport::Marked::Edge(named));
        assert_eq!(chosen.owning_definition(&snapshot), Some(0));
        assert_eq!(chosen.bounds(&snapshot), snapshot.bounds_of_edge(named));

        // An edge nobody named is not a lesser edge; it is not a choice, and
        // the named face beneath it is.
        let fallback = Selection::at(
            definition,
            face,
            unnamed,
            VertexPickId::NOTHING,
            &snapshot,
            &faces,
            &edges,
            &VertexNames::default(),
        );
        assert!(
            matches!(fallback, Selection::Face(_)),
            "an unnamed edge chose {fallback:?}"
        );
        // With no named face either, the part.
        let bare = Selection::at(
            definition,
            face,
            unnamed,
            VertexPickId::NOTHING,
            &snapshot,
            &FaceNames::default(),
            &edges,
            &VertexNames::default(),
        );
        assert!(matches!(bare, Selection::Definition(_)), "{bare:?}");
    }

    #[test]
    fn an_edge_that_contradicts_its_pixel_chooses_no_edge() {
        let (snapshot, faces, edges) = a_named_edge();
        let definition = snapshot.pick_of(0).expect("drawn");
        let face = snapshot.face_of(0, 0).expect("numbered");
        let unrelated_face = snapshot.face_of(0, 1).expect("numbered");
        let named = snapshot.edge_of(0, 0).expect("numbered");
        assert!(!snapshot.edge_bounds_face(named, unrelated_face));

        // A second picture whose raw values are in range here.
        let (other, _, other_edges) = a_named_edge();
        let foreign = other.edge_of(0, 0).expect("numbered");
        assert_eq!(foreign.to_raw(), named.to_raw(), "the same raw value");
        assert!(
            !other_edges.of(foreign, &other).is_empty(),
            "and named in its own picture"
        );

        for (what, definition, face, edge) in [
            ("an edge of another picture", definition, face, foreign),
            ("nothing at all", definition, face, EdgePickId::NOTHING),
            ("a definition of nothing", PickId::NOTHING, face, named),
            (
                "an edge that does not bound the pixel's face",
                definition,
                unrelated_face,
                named,
            ),
        ] {
            let chosen = Selection::at(
                definition,
                face,
                edge,
                VertexPickId::NOTHING,
                &snapshot,
                &faces,
                &edges,
                &VertexNames::default(),
            );
            assert!(
                !matches!(chosen, Selection::Edge(_)),
                "{what} assembled an edge: {chosen:?}"
            );
        }
    }

    #[test]
    fn a_second_definitions_edges_are_numbered_by_the_picture() {
        use ferritecad_document::CapSide;

        let shape = ShapeHandle::new(ferritecad_kernel::SessionId::new(), 1);
        let mesh = |edges: u64| Mesh {
            topological_vertices: None,
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![ferritecad_kernel::MeshFaceRange {
                face: SubShapeHandle::new(shape, ferritecad_kernel::SubShapeKind::Face, 0),
                first_index: 0,
                index_count: 3,
            }],
            edges: Some(ferritecad_kernel::MeshEdges {
                segments: (0..edges).flat_map(|_| [0u32, 1]).collect(),
                ranges: (0..edges)
                    .map(|ordinal| ferritecad_kernel::MeshEdgeRange {
                        edge: SubShapeHandle::new(
                            shape,
                            ferritecad_kernel::SubShapeKind::Edge,
                            ordinal,
                        ),
                        first_segment: ordinal as u32,
                        segment_count: 1,
                    })
                    .collect(),
            }),
        };

        // Two definitions: the first with three edges, the second with one.
        // The second definition's only edge is the picture's fourth, so a
        // layout using the ordinal within the definition would file its name
        // against the first definition's first edge instead.
        let mut builder = SnapshotBuilder::new();
        let first = builder.add_mesh(&mesh(3)).expect("packs");
        let second = builder.add_mesh(&mesh(1)).expect("packs");
        for definition in [first, second] {
            builder
                .place(definition, None, &Transform::IDENTITY, BODY_COLOUR)
                .expect("places");
        }
        let named: BTreeMap<usize, Vec<Vec<BoundMeaning>>> = [
            (first, vec![Vec::new(), Vec::new(), Vec::new()]),
            (second, vec![vec![bound(a_cap_edge(CapSide::End))]]),
        ]
        .into_iter()
        .collect();
        builder
            .bind_identities_to(semantic_context_identity(
                &BTreeMap::new(),
                &named,
                &BTreeMap::new(),
            ))
            .expect("binds");
        let snapshot = builder.build();
        assert_eq!(snapshot.edge_count(), 4);

        let names = edge_names(&snapshot, named).expect("lays out");
        let theirs = snapshot.edge_of(second, 0).expect("numbered");
        assert_eq!(
            names.of(theirs, &snapshot).len(),
            1,
            "the second definition's edge lost its name"
        );
        for ordinal in 0..3 {
            let mine = snapshot.edge_of(first, ordinal).expect("numbered");
            assert!(
                names.of(mine, &snapshot).is_empty(),
                "a name landed on the first definition's edge {ordinal}"
            );
        }
    }

    #[test]
    fn a_mesh_with_no_edge_association_is_given_no_names() {
        let shape = ShapeHandle::new(ferritecad_kernel::SessionId::new(), 1);
        let base = Mesh {
            topological_vertices: None,
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            faces: vec![ferritecad_kernel::MeshFaceRange {
                face: SubShapeHandle::new(shape, ferritecad_kernel::SubShapeKind::Face, 0),
                first_index: 0,
                index_count: 3,
            }],
            edges: None,
        };

        for (what, edges) in [
            ("nothing is known", None),
            (
                "there are none",
                Some(ferritecad_kernel::MeshEdges::default()),
            ),
        ] {
            let mut builder = SnapshotBuilder::new();
            let definition = builder
                .add_mesh(&Mesh {
                    edges,
                    ..base.clone()
                })
                .expect("packs");
            builder
                .place(definition, None, &Transform::IDENTITY, BODY_COLOUR)
                .expect("places");
            builder
                .bind_identities_to(semantic_context_identity(
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                ))
                .expect("binds");
            let snapshot = builder.build();
            let names = edge_names(&snapshot, BTreeMap::new()).expect("lays out");
            assert_eq!(snapshot.edge_count(), 0, "{what}");
            assert!(
                snapshot.edge_of(0, 0).is_none(),
                "{what}: a picture invented an edge to name"
            );
            assert!(
                names.of(EdgePickId::NOTHING, &snapshot).is_empty(),
                "{what}"
            );
        }
    }

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
                        &ObjectPayload::Sketch(Sketch {
                            plane,
                            curves,
                            constraints: Vec::new(),
                        }),
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
    fn a_reused_definition_still_finishes_one_monotonic_load_progress() {
        let directory = tempfile::tempdir().expect("a temporary directory is available");
        let path = directory.path().join("twice.fcad");
        let mut setup = MockKernel::new();
        document_with_the_same_file_twice(&path, &mut setup, ["part.step"; 2]);

        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = std::sync::Arc::clone(&seen);
        let context =
            OperationContext::default().with_progress(ProgressSink::new(move |fraction| {
                record
                    .lock()
                    .expect("no test thread panicked")
                    .push(fraction);
            }));

        let mut kernel = MockKernel::new();
        snapshot_of(
            &path,
            &mut kernel,
            |kernel, _| {
                Ok(Import::Imported {
                    scene: one_part(kernel, "Bracket"),
                    diagnostics: Vec::new(),
                })
            },
            &params(),
            &context,
        )
        .expect("both imports reopen");

        let seen = seen.lock().expect("no test thread panicked").clone();
        assert!(
            seen.windows(2).all(|pair| pair[0] <= pair[1]),
            "progress went backwards when a definition was reused: {seen:?}"
        );
        assert_eq!(
            seen.iter().filter(|fraction| **fraction >= 1.0).count(),
            1,
            "the load reported itself finished the wrong number of times: {seen:?}"
        );
        let last = seen.last().copied().expect("the load reported progress");
        assert!(
            (last - 1.0).abs() < 1e-6,
            "a successful load stopped at {last}: {seen:?}"
        );
        assert_eq!(kernel.live_shape_count(), 0);
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

    /// What the plate's rebuild says each of its faces is called, and the mesh
    /// those faces were named in.
    ///
    /// One kernel session for both halves, because a handle means nothing
    /// outside the session that issued it: two sessions would agree about
    /// nothing and the comparison would be vacuous.
    fn plate_by_hand(
        path: &Path,
        kernel: &mut MockKernel,
    ) -> (
        Vec<(
            ferritecad_kernel::SubShapeHandle,
            ferritecad_types::StableEntityId,
        )>,
        Mesh,
    ) {
        let document = Document::open_read_only(path).expect("opens");
        let built = ferritecad_eval::rebuild_cold(&document, kernel, &OperationContext::default())
            .expect("rebuilds");
        let named = document
            .topology_refs()
            .expect("reads references")
            .iter()
            .filter_map(|reference| match built.resolve(reference) {
                Ok(found) => match found.as_slice() {
                    [face] => Some((*face, reference.id)),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        let body = document
            .objects()
            .expect("reads objects")
            .into_iter()
            .find(|object| matches!(object.payload, ObjectPayload::Body(_)))
            .expect("the plate has a body");
        let shape = built.shape(body.id).expect("the body was built");
        let mesh = kernel
            .tessellate(
                shape,
                &TessellationParams::default(),
                &OperationContext::default(),
            )
            .expect("meshes");
        built.release_all(kernel);
        (named, mesh)
    }

    #[test]
    fn every_named_face_of_the_plate_carries_the_reference_that_resolves_to_it() {
        let (_directory, path) = plate();
        let (by_hand, mesh) = plate_by_hand(&path, &mut MockKernel::new());
        assert_eq!(by_hand.len(), 6, "the committed plate names six faces");

        let scene = snapshot_of(
            &path,
            &mut MockKernel::new(),
            no_imports,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("loads");

        let mut named = 0usize;
        for ordinal in 0..scene.snapshot.meshes()[0].face_count() {
            let face = scene
                .snapshot
                .face_of(0, ordinal)
                .expect("the picture numbered that face");
            let meanings = scene.faces.of(face, &scene.snapshot);
            assert_eq!(meanings.len(), 1, "each face of the plate is named once");
            named += 1;

            // Compared against the handle the kernel gave the triangles at
            // this ordinal, not against the ordinal: that is the whole claim.
            let handle = mesh.faces[ordinal].face;
            let expected: Vec<_> = by_hand
                .iter()
                .filter(|(named, _)| *named == handle)
                .map(|(_, reference)| *reference)
                .collect();
            assert_eq!(
                meanings.iter().map(|m| m.reference).collect::<Vec<_>>(),
                expected,
                "face {ordinal} of the plate is called something else"
            );
        }
        assert_eq!(named, 6);
    }

    /// A kernel that hands its faces over in the opposite order.
    ///
    /// Conforming: the ranges stay contiguous and cover every triangle, they
    /// are simply listed the other way round. A loader that joined names to
    /// faces by ordinal would name every face of this mesh wrongly.
    struct ReversesFaces {
        inner: MockKernel,
    }

    impl GeometryKernel for ReversesFaces {
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
            let mesh = self.inner.tessellate(shape, params, context)?;
            let mut reversed = Mesh {
                topological_vertices: None,
                positions: mesh.positions.clone(),
                normals: mesh.normals.clone(),
                indices: Vec::with_capacity(mesh.indices.len()),
                faces: Vec::with_capacity(mesh.faces.len()),
                edges: None,
            };
            for range in mesh.faces.iter().rev() {
                let first = range.first_index as usize;
                let end = first + range.index_count as usize;
                let at = reversed.indices.len() as u32;
                reversed
                    .indices
                    .extend_from_slice(&mesh.indices[first..end]);
                reversed.faces.push(ferritecad_kernel::MeshFaceRange {
                    face: range.face,
                    first_index: at,
                    index_count: range.index_count,
                });
            }
            reversed.validate()?;
            Ok(reversed)
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
    fn a_kernel_that_lists_its_faces_backwards_names_the_same_faces() {
        let (_directory, path) = plate();
        let forwards = snapshot_of(
            &path,
            &mut MockKernel::new(),
            no_imports,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("loads");
        let backwards = snapshot_of(
            &path,
            &mut ReversesFaces {
                inner: MockKernel::new(),
            },
            no_imports,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("loads");

        // Same faces, listed in the opposite order, so the face at ordinal n
        // of one is the face at ordinal 5 - n of the other. What each is
        // called must follow the face, not the position.
        let count = forwards.snapshot.meshes()[0].face_count();
        assert_eq!(count, backwards.snapshot.meshes()[0].face_count());
        for ordinal in 0..count {
            let one = forwards.snapshot.face_of(0, ordinal).expect("numbered");
            let other = backwards
                .snapshot
                .face_of(0, count - 1 - ordinal)
                .expect("numbered");
            assert_eq!(
                forwards
                    .faces
                    .of(one, &forwards.snapshot)
                    .iter()
                    .map(|m| m.reference)
                    .collect::<Vec<_>>(),
                backwards
                    .faces
                    .of(other, &backwards.snapshot)
                    .iter()
                    .map(|m| m.reference)
                    .collect::<Vec<_>>(),
                "reversing the kernel's face order retargeted a name"
            );
        }
    }

    /// One square body, and the pieces a topology reference is written from.
    ///
    /// Returned rather than looked up again, because a test that had to find
    /// the extrusion by searching would be testing the search.
    fn a_named_body(
        path: &Path,
        write: impl FnOnce(ObjectId, ObjectId, &[ferritecad_types::StableEntityId]) -> Vec<TopologyRef>,
    ) -> (ObjectId, Vec<TopologyRef>) {
        a_named_body_with(path, write)
    }

    fn a_named_body_with(
        path: &Path,
        write: impl FnOnce(ObjectId, ObjectId, &[ferritecad_types::StableEntityId]) -> Vec<TopologyRef>,
    ) -> (ObjectId, Vec<TopologyRef>) {
        use ferritecad_document::{
            Body, DatumPlane, Dependency, DependencyRole, EndCondition, Expression, Extrude,
            Point2, Sketch, SketchCurve, SketchGeometry, SolidOperation,
        };
        use ferritecad_types::StableEntityId;

        let (plane, sketch, extrude, body) = (
            ObjectId::new(),
            ObjectId::new(),
            ObjectId::new(),
            ObjectId::new(),
        );
        let corners = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let segments: Vec<StableEntityId> =
            (0..corners.len()).map(|_| StableEntityId::new()).collect();
        let written = write(extrude, body, &segments);

        let mut document = Document::create(path).expect("creates a document");
        let stored = written.clone();
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
                let mut curves = Vec::new();
                for (corner, id) in segments.iter().enumerate() {
                    let (sx, sy) = corners[corner];
                    let (ex, ey) = corners[(corner + 1) % corners.len()];
                    curves.push(SketchCurve {
                        id: *id,
                        construction: false,
                        geometry: SketchGeometry::Line {
                            start: Point2::new(sx, sy)?,
                            end: Point2::new(ex, ey)?,
                        },
                    });
                }
                w.put_object(
                    sketch,
                    None,
                    1,
                    None,
                    &ObjectPayload::Sketch(Sketch {
                        plane,
                        curves,
                        constraints: Vec::new(),
                    }),
                )?;
                w.add_dependency(Dependency {
                    dependent: sketch,
                    dependency: plane,
                    role: DependencyRole::Plane,
                })?;
                w.put_object(
                    extrude,
                    None,
                    2,
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
                w.put_object(
                    body,
                    None,
                    3,
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
                for reference in &stored {
                    w.put_topology_ref(reference)?;
                    w.add_dependency(Dependency {
                        dependent: reference.owner,
                        dependency: reference.producer_feature,
                        role: DependencyRole::TopologyReference,
                    })?;
                }
                Ok(())
            })
            .expect("writes the document");
        (body, written)
    }

    fn face_reference(
        owner: ObjectId,
        producer: ObjectId,
        role: SemanticRole,
        selection: SelectionRule,
    ) -> TopologyRef {
        TopologyRef {
            id: ferritecad_types::StableEntityId::new(),
            owner,
            producer_feature: producer,
            expected_kind: EntityKind::Face,
            output_role: role,
            selection,
            fallback_signature: None,
        }
    }

    /// A kernel whose extrusion raises two faces from the first segment.
    ///
    /// What a real kernel does when a face is split: the history says both
    /// came from one input, so a family reference to that input names two
    /// faces and an exact one names none.
    struct RaisesTwoFaces {
        inner: MockKernel,
    }

    impl RaisesTwoFaces {
        fn new() -> Self {
            Self {
                inner: MockKernel::new(),
            }
        }
    }

    impl GeometryKernel for RaisesTwoFaces {
        fn identity(&self) -> &KernelIdentity {
            self.inner.identity()
        }

        fn extrude(
            &mut self,
            request: &ExtrudeRequest,
            context: &OperationContext,
        ) -> Result<ExtrudeResult> {
            let mut result = self.inner.extrude(request, context)?;
            let first = request
                .profile()
                .outer()
                .segments()
                .first()
                .expect("a profile has segments")
                .label;
            // The start cap, recorded a second time as though it too had been
            // raised from that segment.
            if let Some(extra) = result.start_cap.first().copied() {
                result
                    .history
                    .record_generated(ferritecad_kernel::HistoryInput::Segment(first), extra);
            }
            Ok(result)
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
            self.inner.tessellate(shape, params, context)
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

    fn load_with<K: GeometryKernel + ?Sized>(path: &Path, kernel: &mut K) -> LoadedScene {
        snapshot_of(
            path,
            kernel,
            no_imports,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("loads")
    }

    fn load(path: &Path) -> LoadedScene {
        snapshot_of(
            path,
            &mut MockKernel::new(),
            no_imports,
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("loads")
    }

    /// Every face of the one definition, and what the document calls it.
    fn meanings(scene: &LoadedScene) -> Vec<Vec<ferritecad_types::StableEntityId>> {
        (0..scene.snapshot.meshes()[0].face_count())
            .map(|ordinal| {
                let face = scene.snapshot.face_of(0, ordinal).expect("numbered");
                scene
                    .faces
                    .of(face, &scene.snapshot)
                    .iter()
                    .map(|meaning| meaning.reference)
                    .collect()
            })
            .collect()
    }

    #[test]
    fn a_reference_naming_a_family_of_faces_names_none_of_them_in_particular() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("family.fcad");
        // One segment id on two sides of the square, so everything raised from
        // it is two faces. A family selection is the right way to say that,
        // and it is not a name for whichever of the two was clicked: the two
        // are indistinguishable through it, and choosing one would be choosing
        // by traversal order.
        let (_body, written) = a_named_body(&path, |extrude, body, segments| {
            vec![face_reference(
                body,
                extrude,
                SemanticRole::ExtrudeSide {
                    profile_segment: segments[0],
                },
                SelectionRule::AllDerivedFrom {
                    ancestor: segments[0],
                },
            )]
        });
        assert_eq!(written.len(), 1);

        let scene = load_with(&path, &mut RaisesTwoFaces::new());
        assert!(
            meanings(&scene).iter().all(Vec::is_empty),
            "a reference naming two faces was accepted as a name for one"
        );
    }

    #[test]
    fn a_shared_segment_still_names_the_face_an_exact_reference_reaches() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("shared.fcad");
        // The same document, and beside the family reference an exact one to a
        // cap. Refusing the ambiguous name must not refuse the unambiguous one
        // in the same document.
        let (_body, _written) = a_named_body(&path, |extrude, body, segments| {
            vec![
                face_reference(
                    body,
                    extrude,
                    SemanticRole::ExtrudeSide {
                        profile_segment: segments[0],
                    },
                    SelectionRule::AllDerivedFrom {
                        ancestor: segments[0],
                    },
                ),
                face_reference(
                    body,
                    extrude,
                    SemanticRole::ExtrudeCap {
                        side: ferritecad_document::CapSide::Start,
                    },
                    SelectionRule::Exact,
                ),
            ]
        });

        let scene = load_with(&path, &mut RaisesTwoFaces::new());
        let named: usize = meanings(&scene).iter().filter(|m| !m.is_empty()).count();
        assert_eq!(named, 1, "the exact name was lost with the ambiguous one");
    }

    #[test]
    fn several_exact_references_to_one_face_are_all_kept_in_the_order_they_are_stored() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("twice.fcad");
        let (_body, written) = a_named_body(&path, |extrude, body, _segments| {
            // Two references, both naming the end cap exactly. Both are true
            // of that face, and which of them is "the" name is not this
            // loader's decision.
            vec![
                face_reference(
                    body,
                    extrude,
                    SemanticRole::ExtrudeCap {
                        side: ferritecad_document::CapSide::End,
                    },
                    SelectionRule::Exact,
                ),
                face_reference(
                    body,
                    extrude,
                    SemanticRole::ExtrudeCap {
                        side: ferritecad_document::CapSide::End,
                    },
                    SelectionRule::Exact,
                ),
            ]
        });

        let scene = load(&path);
        let both: Vec<_> = meanings(&scene)
            .into_iter()
            .filter(|m| !m.is_empty())
            .collect();
        assert_eq!(both.len(), 1, "both references name the same one face");
        let mut expected: Vec<_> = written.iter().map(|reference| reference.id).collect();
        // The document hands its references back in a defined order, and that
        // is the order they are kept in.
        expected.sort();
        assert_eq!(both[0], expected);

        // Loading twice gives the same order, which is what "deterministic"
        // has to mean for something a person will read.
        assert_eq!(meanings(&load(&path)), meanings(&scene));
    }

    #[test]
    fn a_body_nobody_named_has_no_face_meanings_at_all() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("unnamed.fcad");
        a_named_body(&path, |_, _, _| Vec::new());

        let scene = load(&path);
        assert!(
            meanings(&scene).iter().all(Vec::is_empty),
            "a face nobody named was given a name"
        );
    }

    #[test]
    fn an_imported_definition_has_no_face_meanings_at_all() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("imported.fcad");
        let mut kernel = MockKernel::new();
        document_with_import(&path, &mut kernel);

        let scene = snapshot_of(
            &path,
            &mut kernel,
            |kernel: &mut MockKernel, _: &[u8]| {
                Ok(Import::Imported {
                    scene: nested_assembly(kernel),
                    diagnostics: Vec::new(),
                })
            },
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("loads");

        assert!(scene.snapshot.face_count() > 0, "the import draws faces");
        for definition in 0..scene.snapshot.meshes().len() {
            for ordinal in 0..scene.snapshot.meshes()[definition].face_count() {
                let face = scene
                    .snapshot
                    .face_of(definition, ordinal)
                    .expect("numbered");
                assert!(
                    scene.faces.of(face, &scene.snapshot).is_empty(),
                    "an imported face was given a durable name it does not have"
                );
            }
        }
    }

    #[test]
    fn what_a_face_is_called_holds_no_transient_identity() {
        let (_directory, path) = plate();
        let scene = load(&path);
        let shown = format!("{:?}", scene.faces);
        assert!(shown.contains("ExtrudeCap"), "the plate names its caps");

        // What a face is called is what the document stores about it. A
        // renderer's own number for the face is true of one picture and of
        // nothing else, so it is not part of what the face *is* – and a
        // meaning that carried one could be written down and found to mean
        // nothing an hour later.
        for word in [
            "FacePickId",
            "PickId",
            "ShapeHandle",
            "SubShapeHandle",
            "SessionId",
            "raw",
        ] {
            assert!(
                !shown.contains(word),
                "a durable face meaning carried a {word}: {shown}"
            );
        }
    }

    #[test]
    fn what_is_handed_over_holds_no_kernel_handle_of_any_kind() {
        let (_directory, path) = plate();
        let scene = load(&path);
        let shown = format!("{scene:?}");
        for word in [
            "ShapeHandle",
            "SubShapeHandle",
            "SessionId",
            "SubShapeKind",
            "TopologyMap",
        ] {
            assert!(
                !shown.contains(word),
                "the handed-over scene holds a {word}"
            );
        }
    }

    #[test]
    fn clicking_a_named_face_chooses_the_face_and_everything_else_the_definition() {
        let (_directory, path) = plate();
        let scene = load(&path);
        let snapshot = &scene.snapshot;
        let pick = snapshot.pick_of(0).expect("the plate is drawn");
        let named = snapshot.face_of(0, 0).expect("numbered");
        assert!(!scene.faces.of(named, snapshot).is_empty());

        // A face the document names is chosen as that face, and carries what
        // the document calls it.
        let chosen = Selection::at(
            pick,
            named,
            EdgePickId::NOTHING,
            VertexPickId::NOTHING,
            snapshot,
            &scene.faces,
            &EdgeNames::default(),
            &VertexNames::default(),
        );
        let Selection::Face(face) = &chosen else {
            panic!("a named face was not chosen as a face: {chosen:?}");
        };
        assert_eq!(face.face(), named);
        assert_eq!(face.definition(), pick);
        assert_eq!(face.meanings().len(), 1);
        assert_eq!(chosen.owning_definition(snapshot), Some(0));
        assert_eq!(chosen.marked(), ferritecad_viewport::Marked::Face(named));

        // Nothing at all is nothing at all.
        assert_eq!(
            Selection::at(
                PickId::NOTHING,
                FacePickId::NOTHING,
                EdgePickId::NOTHING,
                VertexPickId::NOTHING,
                snapshot,
                &scene.faces,
                &EdgeNames::default(),
                &VertexNames::default()
            ),
            Selection::Nothing
        );
    }

    #[test]
    fn a_face_the_document_does_not_name_chooses_the_definition_it_is_on() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("unnamed.fcad");
        a_named_body(&path, |_, _, _| Vec::new());
        let scene = load(&path);
        let snapshot = &scene.snapshot;

        let pick = snapshot.pick_of(0).expect("the body is drawn");
        let face = snapshot.face_of(0, 0).expect("numbered");
        assert!(scene.faces.of(face, snapshot).is_empty());

        // The honest answer, and the one this application could already give:
        // the part, not an invented name for one of its faces.
        assert_eq!(
            Selection::at(
                pick,
                face,
                EdgePickId::NOTHING,
                VertexPickId::NOTHING,
                snapshot,
                &scene.faces,
                &EdgeNames::default(),
                &VertexNames::default()
            ),
            Selection::Definition(pick)
        );
    }

    #[test]
    fn a_face_of_an_imported_definition_chooses_the_definition() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("imported.fcad");
        let mut kernel = MockKernel::new();
        document_with_import(&path, &mut kernel);
        let scene = snapshot_of(
            &path,
            &mut kernel,
            |kernel: &mut MockKernel, _: &[u8]| {
                Ok(Import::Imported {
                    scene: nested_assembly(kernel),
                    diagnostics: Vec::new(),
                })
            },
            &TessellationParams::default(),
            &OperationContext::default(),
        )
        .expect("loads");

        let snapshot = &scene.snapshot;
        let pick = snapshot.pick_of(0).expect("the import is drawn");
        let face = snapshot.face_of(0, 0).expect("numbered");
        assert_eq!(
            Selection::at(
                pick,
                face,
                EdgePickId::NOTHING,
                VertexPickId::NOTHING,
                snapshot,
                &scene.faces,
                &EdgeNames::default(),
                &VertexNames::default()
            ),
            Selection::Definition(pick),
            "an imported face has no durable name and must not be chosen as one"
        );
    }

    #[test]
    fn a_face_and_a_definition_that_do_not_belong_together_are_not_a_selection() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("two.fcad");
        // Two bodies, the second one named, so there is a face of one
        // definition and a pick of another in the same picture.
        let (first, _) = a_named_body(&path, |_, _, _| Vec::new());
        let _ = first;
        let scene = load(&path);
        let snapshot = &scene.snapshot;
        let pick = snapshot.pick_of(0).expect("drawn");

        // A face of a picture that has been replaced, beside a live pick.
        let (_other_directory, other_path) = plate();
        let other = load(&other_path);
        let stale = other.snapshot.face_of(0, 0).expect("numbered");

        assert_eq!(
            Selection::at(
                pick,
                stale,
                EdgePickId::NOTHING,
                VertexPickId::NOTHING,
                snapshot,
                &scene.faces,
                &EdgeNames::default(),
                &VertexNames::default()
            ),
            Selection::Definition(pick),
            "a face from another picture must not attach itself to this one"
        );
        assert_eq!(
            Selection::at(
                PickId::NOTHING,
                stale,
                EdgePickId::NOTHING,
                VertexPickId::NOTHING,
                snapshot,
                &scene.faces,
                &EdgeNames::default(),
                &VertexNames::default()
            ),
            Selection::Nothing
        );
    }

    #[test]
    fn a_stale_face_names_nothing_after_the_picture_is_replaced() {
        let (_directory, path) = plate();
        let before = load(&path);
        let after = load(&path);
        let face = before.snapshot.face_of(0, 0).expect("numbered");

        // The same document, loaded twice. The two pictures are the same
        // picture by content, so this is the strongest form of the question:
        // what stops the old identity is the identity itself.
        assert!(!before.faces.of(face, &before.snapshot).is_empty());
        assert_eq!(
            after.faces.of(face, &after.snapshot).len(),
            before.faces.of(face, &before.snapshot).len(),
            "the same picture names the same faces"
        );

        // And a face of a genuinely different picture names nothing here –
        // in a picture that has names of its own, so an unchecked lookup by
        // number would find one and hand it over as though it were this
        // face's.
        let directory = tempfile::tempdir().expect("a temporary directory");
        let elsewhere = directory.path().join("other.fcad");
        a_named_body(&elsewhere, |extrude, body, segments| {
            // Named at the same position the stale identity would land on, so
            // a lookup that trusted the number would find this and hand it
            // over as though it were the plate's face.
            vec![face_reference(
                body,
                extrude,
                SemanticRole::ExtrudeSide {
                    profile_segment: segments[0],
                },
                SelectionRule::Exact,
            )]
        });
        let other = load(&elsewhere);
        assert!(
            !other
                .faces
                .of(
                    other.snapshot.face_of(0, 0).expect("numbered"),
                    &other.snapshot
                )
                .is_empty(),
            "the second picture must name the face the stale number reaches"
        );
        assert_eq!(face.to_raw(), 1, "the stale identity is the first face");
        assert_eq!(other.snapshot.definition_of_face(face), None);
        assert!(
            other.faces.of(face, &other.snapshot).is_empty(),
            "a face of the replaced picture was answered with a name from this one"
        );
    }

    #[test]
    fn face_names_cannot_be_borrowed_by_an_identical_picture_with_different_meaning() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let named_path = directory.path().join("named.fcad");
        let unnamed_path = directory.path().join("unnamed.fcad");
        a_named_body(&named_path, |extrude, body, _| {
            vec![face_reference(
                body,
                extrude,
                SemanticRole::ExtrudeCap {
                    side: ferritecad_document::CapSide::Start,
                },
                SelectionRule::Exact,
            )]
        });
        a_named_body(&unnamed_path, |_, _, _| Vec::new());

        let named = load(&named_path);
        let unnamed = load(&unnamed_path);
        assert_eq!(
            named.snapshot.meshes(),
            unnamed.snapshot.meshes(),
            "the gate needs byte-identical geometry and face partitions"
        );
        let drawn = |scene: &LoadedScene| {
            scene
                .snapshot
                .draws()
                .iter()
                .map(|item| (item.mesh, item.transform, item.colour))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            drawn(&named),
            drawn(&unnamed),
            "the gate needs identical placements and appearance"
        );
        assert_ne!(
            named.snapshot, unnamed.snapshot,
            "different portable face meanings must make old picks stale"
        );

        let ordinal = meanings(&named)
            .iter()
            .position(|meanings| !meanings.is_empty())
            .expect("the first document names one face");
        let face = unnamed
            .snapshot
            .face_of(0, ordinal)
            .expect("the identical picture has the same face position");
        let pick = unnamed.snapshot.pick_of(0).expect("the body is drawn");

        assert!(
            named.faces.of(face, &unnamed.snapshot).is_empty(),
            "a name stored only in the first document leaked into the second"
        );
        assert_eq!(
            Selection::at(
                pick,
                face,
                EdgePickId::NOTHING,
                VertexPickId::NOTHING,
                &unnamed.snapshot,
                &named.faces,
                &EdgeNames::default(),
                &VertexNames::default(),
            ),
            Selection::Definition(pick),
            "a face the current document does not name became selectable as a face"
        );
    }

    // -----------------------------------------------------------------------
    // Turning what a solve called redundant back into what the document says
    // -----------------------------------------------------------------------
    //
    // The join itself, held to its contract without a solver in the room. A
    // report says which identifiers repeat; only the sketch it is about knows
    // what those identifiers stand for, and putting the two together wrongly
    // is the one mistake here that produces a confident, readable, wrong
    // sentence.

    /// A sketch with two lines and the constraints handed in.
    fn constrained(constraints: Vec<ferritecad_document::SketchConstraint>) -> Sketch {
        use ferritecad_document::{Point2, SketchCurve, SketchGeometry};
        use ferritecad_types::StableEntityId;

        let corners = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        let curves = (0..2)
            .map(|index| SketchCurve {
                id: StableEntityId::new(),
                construction: false,
                geometry: SketchGeometry::Line {
                    start: Point2::new(corners[index].0, corners[index].1).expect("finite"),
                    end: Point2::new(corners[index + 1].0, corners[index + 1].1).expect("finite"),
                },
            })
            .collect();
        Sketch {
            plane: ObjectId::new(),
            curves,
            constraints,
        }
    }

    /// One rule, given an identity of its own.
    fn stored(rule: SketchConstraintRule) -> ferritecad_document::SketchConstraint {
        ferritecad_document::SketchConstraint {
            id: ferritecad_types::StableEntityId::new(),
            rule,
        }
    }

    /// A relationship between the two ends of the sketch's first curve.
    fn level(sketch: &Sketch) -> SketchConstraintRule {
        use ferritecad_document::{SketchPointRef, SketchPointSelector};
        let curve = sketch.curves[0].id;
        SketchConstraintRule::Horizontal {
            a: SketchPointRef::new(curve, SketchPointSelector::Start),
            b: SketchPointRef::new(curve, SketchPointSelector::End),
        }
    }

    #[test]
    fn each_reported_identifier_is_answered_with_the_rule_stored_under_it() {
        let mut sketch = constrained(Vec::new());
        let rule = level(&sketch);
        let first = stored(rule);
        let second = stored(rule);
        // A third that says something else, and that no report names: it is
        // here so that answering with a neighbour, or with everything the
        // sketch holds, shows up as a difference.
        let third = stored(SketchConstraintRule::Fixed {
            point: ferritecad_document::SketchPointRef::new(
                sketch.curves[1].id,
                ferritecad_document::SketchPointSelector::Start,
            ),
            x: 1.0,
            y: 2.0,
        });
        sketch.constraints = vec![first, third, second];

        // Two constraints that say exactly the same thing, stored under two
        // identifiers, reported in the order the document stores them. They
        // are two constraints, and each is answered with its own entry.
        let joined = redundant_constraints(&sketch, &[first.id, second.id])
            .expect("both identifiers are stored in this sketch");
        assert_eq!(
            joined,
            vec![
                RedundantConstraint {
                    id: first.id,
                    rule: first.rule,
                },
                RedundantConstraint {
                    id: second.id,
                    rule: second.rule,
                },
            ],
            "two identical rules under two identifiers are not two entries in the report's order"
        );
        assert_ne!(joined[0].id, joined[1].id, "one entry answered for both");
    }

    #[test]
    fn the_answer_follows_the_identifier_and_not_the_position() {
        let mut sketch = constrained(Vec::new());
        let level = stored(level(&sketch));
        let pinned = stored(SketchConstraintRule::Fixed {
            point: ferritecad_document::SketchPointRef::new(
                sketch.curves[1].id,
                ferritecad_document::SketchPointSelector::End,
            ),
            x: 3.0,
            y: 4.0,
        });
        sketch.constraints = vec![level, pinned];

        // The second constraint reported first. Answering by position would
        // hand back the first, which says something else entirely and would
        // read perfectly.
        let joined = redundant_constraints(&sketch, &[pinned.id])
            .expect("the identifier is stored in this sketch");
        assert_eq!(
            joined,
            vec![RedundantConstraint {
                id: pinned.id,
                rule: pinned.rule
            }]
        );
    }

    #[test]
    fn an_identifier_this_sketch_does_not_store_is_refused() {
        // The neighbour case, which is the one that happens: two sketches of
        // one document, and a report about the first read against the second.
        // Every identifier here is real and none of them is in this sketch.
        let mut first = constrained(Vec::new());
        first.constraints = vec![stored(level(&first))];
        let mut second = constrained(Vec::new());
        second.constraints = vec![stored(level(&second))];
        let reported = first.constraints[0].id;

        let refused = redundant_constraints(&second, &[reported])
            .expect_err("a report about another sketch must not be answered from this one");
        assert_eq!(
            refused.kind(),
            ferritecad_types::ErrorKind::Constraint,
            "the mismatch was reported as something other than a constraint problem: {refused}"
        );
        assert!(
            refused.to_string().contains(&reported.to_string()),
            "the refusal does not say which identifier could not be found: {refused}"
        );
        // And not quietly answered with whatever this sketch does hold.
        assert!(
            !refused
                .to_string()
                .contains(&second.constraints[0].id.to_string()),
            "the refusal named this sketch's own constraint as though it were the one \
             reported: {refused}"
        );
    }

    #[test]
    fn a_report_naming_nothing_joins_to_nothing() {
        let mut sketch = constrained(Vec::new());
        sketch.constraints = vec![stored(level(&sketch))];
        assert_eq!(
            redundant_constraints(&sketch, &[]).expect("nothing to look up"),
            Vec::new(),
            "a sketch that repeats nothing was given an entry anyway"
        );
    }
}
