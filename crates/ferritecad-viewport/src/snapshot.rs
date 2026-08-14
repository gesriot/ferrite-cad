// SPDX-License-Identifier: MIT
//! A picture of a model, complete and unchanging.
//!
//! A snapshot is built once from meshes and placements and is then read-only:
//! no public field, no `&mut` method, nothing to invalidate. A renderer handed
//! one can upload it, draw it and pick against it without asking whether the
//! model moved underneath, because it cannot have. Replacing a snapshot is how
//! the picture changes, and that is one atomic swap rather than a set of edits
//! whose intermediate states are drawable.
//!
//! # A pick names a definition, not a placement
//!
//! Four bolts in one plate are one definition and four placements, and clicking
//! any of them yields the same [`PickId`]. That is not a limitation being worked
//! around; it is the whole of what this build can honestly say. A definition has
//! an identity its source file wrote down, so a reference to one survives being
//! saved and re-imported. An occurrence has only its position in a tree, and a
//! reference to *that* would look durable while resting on an index that the
//! next import is free to renumber.
//!
//! So the information needed to tell two placements apart never reaches a pick
//! result. Not filtered out at the end – never carried, so no later change can
//! start leaking it by accident.

use ferritecad_kernel::Mesh;
use ferritecad_types::{CadError, CanonicalHasher, ContentHash, Result, Transform};

/// Floats per packed vertex: three of position, three of normal.
pub const VERTEX_FLOATS: usize = 6;

/// What a pick can identify.
///
/// Transient by construction: it indexes into the snapshot that produced it and
/// means nothing against any other. Deliberately not serialisable – see the
/// module documentation for why a durable pick would have to name a definition
/// through the document rather than through a picture of one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PickId {
    raw: u32,
    snapshot: ContentHash,
}

impl PickId {
    /// What the background reads as, so an empty pick is not definition zero.
    pub const NOTHING: Self = Self {
        raw: 0,
        snapshot: ContentHash::from_bytes([0; 32]),
    };

    /// The value a pick buffer stores.
    pub fn to_raw(self) -> u32 {
        self.raw
    }

    /// Reads a value back out of a pick buffer.
    ///
    /// A value naming no definition in `snapshot` reads as
    /// [`NOTHING`][Self::NOTHING]: a pick buffer is written by a GPU and read
    /// back over a bus, and a value outside this snapshot's definition range
    /// must land on the background rather than on whichever definition it
    /// happens to number. The caller must decode a readback against the exact
    /// snapshot that rendered it: an in-range integer carries no generation.
    pub fn from_raw(raw: u32, snapshot: &RenderSnapshot) -> Self {
        match (raw as usize).checked_sub(1) {
            Some(definition) if definition < snapshot.meshes.len() => Self {
                raw,
                snapshot: snapshot.identity,
            },
            _ => Self::NOTHING,
        }
    }

    fn unbound(raw: u32) -> Self {
        Self {
            raw,
            snapshot: ContentHash::from_bytes([0; 32]),
        }
    }
}

/// One definition's triangles, in the form a vertex buffer wants them.
///
/// Interleaved rather than parallel: one buffer, one stride, one upload. The
/// mesh this came from keeps its own parallel arrays, which are the right shape
/// for the kernel to produce and the wrong shape to draw from.
#[derive(Debug, Clone, PartialEq)]
pub struct PackedMesh {
    vertices: Vec<f32>,
    indices: Vec<u32>,
    /// Which face each vertex belongs to, as an identity of this snapshot.
    ///
    /// The kernel says which triangles make up which face, and that is the
    /// only place that knowledge exists: once a shape is released, the handles
    /// it named are gone, and a picture that had kept them would be holding
    /// numbers belonging to a session nobody can ask any more. What is kept is
    /// the partition itself, renumbered for this snapshot alone.
    ///
    /// Per vertex rather than per triangle because a fragment can be told
    /// which vertex it came from anywhere, and which *triangle* only where the
    /// adapter offers a capability that not every one does. A tessellation
    /// gives each face its own vertices – checked when packing, not assumed –
    /// so the two are the same statement about the same partition.
    face_of_vertex: Vec<u32>,
    /// Index counts of the kernel's face ranges, in range order.
    ///
    /// Kept separately from the vertex attribute because face ranges divide
    /// the index buffer, not the order vertices happen to be stored in. It is
    /// the exact partition hashed into the snapshot identity and the direct
    /// answer to how many faces this mesh has.
    face_index_counts: Vec<u32>,
    min: [f32; 3],
    max: [f32; 3],
}

impl PackedMesh {
    /// Interleaved position and normal, [`VERTEX_FLOATS`] floats per vertex.
    pub fn vertices(&self) -> &[f32] {
        &self.vertices
    }

    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len() / VERTEX_FLOATS
    }

    /// How many faces the kernel divided this mesh into.
    pub fn face_count(&self) -> usize {
        self.face_index_counts.len()
    }

    /// The identity of the face each vertex belongs to, in vertex order.
    ///
    /// For a renderer to hand to a shader as an attribute. The values mean
    /// nothing outside the snapshot that issued them, which is why nothing
    /// here hands out a raw number without one.
    pub fn faces_of_vertices(&self) -> &[u32] {
        &self.face_of_vertex
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The corners of this mesh's own bounding box, before any placement.
    ///
    /// Both are zero for an empty mesh, which is the only answer that is not a
    /// lie about geometry that is not there.
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        (self.min, self.max)
    }
}

/// A box being grown to hold everything put into it.
///
/// One implementation of "where is this in the world", used for the whole
/// picture and for any part of it. Two of these would be two answers to one
/// question, and the one nobody exercised would be the wrong one.
#[derive(Debug, Default)]
struct Extent {
    min: [f32; 3],
    max: [f32; 3],
    holds_anything: bool,
}

impl Extent {
    /// Adds one placement of one mesh.
    ///
    /// A mesh with no triangles is not somewhere: it has no corners to place,
    /// and treating its empty bounds as a point would put the middle of a
    /// picture wherever an empty definition happened to sit.
    fn include(&mut self, mesh: &PackedMesh, item: &DrawItem) {
        if mesh.indices.is_empty() {
            return;
        }

        let (low, high) = mesh.bounds();
        // Every corner, not just the two: a rotated box's extent is not the
        // transform of its extent.
        for corner in 0..8 {
            let point = [
                if corner & 1 == 0 { low[0] } else { high[0] },
                if corner & 2 == 0 { low[1] } else { high[1] },
                if corner & 4 == 0 { low[2] } else { high[2] },
            ];
            self.grow(apply(&item.transform, point));
        }
    }

    /// Adds one placement of the part of a mesh belonging to one face.
    ///
    /// Its own vertices, every one of them, rather than the corners of a box
    /// around the whole mesh. The same arithmetic as [`Self::include`] –
    /// transform, then grow – because two ways of placing a point are two
    /// answers to one question.
    fn include_face(&mut self, mesh: &PackedMesh, item: &DrawItem, face: u32) {
        for (vertex, owner) in mesh.face_of_vertex.iter().enumerate() {
            if *owner != face {
                continue;
            }
            let at = vertex * VERTEX_FLOATS;
            let Some(position) = mesh.vertices.get(at..at + 3) else {
                continue;
            };
            self.grow(apply(
                &item.transform,
                [position[0], position[1], position[2]],
            ));
        }
    }

    /// Grows the box to hold one placed point.
    fn grow(&mut self, placed: [f32; 3]) {
        if !self.holds_anything {
            self.min = [f32::INFINITY; 3];
            self.max = [f32::NEG_INFINITY; 3];
            self.holds_anything = true;
        }
        for (axis, value) in placed.into_iter().enumerate() {
            self.min[axis] = self.min[axis].min(value);
            self.max[axis] = self.max[axis].max(value);
        }
    }

    /// What was put in, or nothing if nothing was.
    fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        self.holds_anything.then_some((self.min, self.max))
    }
}

/// One face of one definition, for as long as this picture is on screen.
///
/// Like [`PickId`] and for the same reasons: it is bound to the snapshot that
/// issued it, it carries no number anyone outside can read, and it is not
/// serialisable. This layer has no durable name for a face: a document-aware
/// caller may resolve this value to a topology reference, but this is not one.
/// It says only "the face the pointer is over, in the picture on screen".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FacePickId {
    raw: u32,
    snapshot: ContentHash,
}

impl FacePickId {
    /// What the background and every unfaced pixel read as.
    pub const NOTHING: Self = Self {
        raw: 0,
        snapshot: ContentHash::from_bytes([0; 32]),
    };

    /// The value a face buffer stores.
    pub fn to_raw(self) -> u32 {
        self.raw
    }

    /// Reads a value back out of a face buffer.
    ///
    /// A number naming no face of `snapshot` reads as
    /// [`NOTHING`][Self::NOTHING]. The caller must decode against the exact
    /// snapshot that rendered it: an in-range integer carries no generation.
    pub fn from_raw(raw: u32, snapshot: &RenderSnapshot) -> Self {
        match (raw as usize).checked_sub(1) {
            Some(face) if face < snapshot.face_owner.len() => Self {
                raw,
                snapshot: snapshot.identity,
            },
            _ => Self::NOTHING,
        }
    }
}

/// What one mark on the picture is on, transiently.
///
/// Three states rather than two identities, because "no face and definition
/// three" and "face seven, whose definition is three" are different things to
/// draw and would otherwise have to be told apart by which of two fields
/// happened to be set. Nothing here is a row number or a face ordinal: both
/// arms carry an identity bound to the picture that issued it.
///
/// One type for what is chosen and for what is under the pointer, because the
/// question "which part of this picture" has one shape and one answer, and the
/// rule that an identity of another picture marks nothing is the same rule
/// twice. What differs is how each is drawn, which is the renderer's business.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Marked {
    #[default]
    Nothing,
    /// A whole definition, as a list of definitions can say.
    Definition(PickId),
    /// One face, as only a pixel can say.
    Face(FacePickId),
}

impl Marked {
    /// The same mark, resolved against the picture about to be drawn.
    ///
    /// An identity of another picture, of a picture that has been replaced, or
    /// of nothing at all is [`Nothing`][Self::Nothing] here. One place, so a
    /// renderer, a reducer and an inspector cannot each decide differently
    /// what a stale identity means.
    pub fn known_to(self, snapshot: &RenderSnapshot) -> Self {
        match self {
            Self::Nothing => Self::Nothing,
            Self::Definition(pick) => match snapshot.definition(pick) {
                Some(_) => self,
                None => Self::Nothing,
            },
            Self::Face(face) => match snapshot.definition_of_face(face) {
                Some(_) => self,
                None => Self::Nothing,
            },
        }
    }
}

/// Which definitions of one picture are drawn, for as long as it is on screen.
///
/// Per definition and not per placement: a definition drawn four times is one
/// thing, and hiding one of its four placements would be hiding something the
/// document does not describe.
///
/// Bound to the picture it was made for, like every other transient identity
/// here. A mask from another picture – including one drawn from geometry that
/// looks identical but means something else – applies to nothing, so a
/// replaced document cannot arrive with parts already missing.
///
/// Not serialisable, and not a document fact: what is hidden is what this
/// window is not showing at the moment, and reopening the document shows
/// everything again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visibility {
    /// Indexed by definition. Empty means every definition is drawn, which is
    /// what a picture nobody has hidden anything in looks like.
    hidden: Vec<bool>,
    snapshot: ContentHash,
}

impl Default for Visibility {
    /// A mask belonging to no picture, which hides nothing in any of them.
    ///
    /// What a window holds before a document has been read.
    fn default() -> Self {
        Self {
            hidden: Vec::new(),
            snapshot: ContentHash::from_bytes([0; 32]),
        }
    }
}

impl Visibility {
    /// Everything drawn, in this picture.
    pub fn new(snapshot: &RenderSnapshot) -> Self {
        Self {
            hidden: vec![false; snapshot.meshes.len()],
            snapshot: snapshot.identity,
        }
    }

    /// Which definitions this picture is not drawing.
    ///
    /// Empty for a picture this mask was not made for, which is the safe
    /// answer in both directions: nothing is hidden that cannot be verified,
    /// and nothing is revealed that this mask has no business revealing.
    ///
    /// One rule, read by every path that draws and by the one that measures,
    /// so a hidden definition cannot be missing from a window and present in
    /// the extent a camera is framed to.
    pub fn hidden_in<'a>(&'a self, snapshot: &RenderSnapshot) -> &'a [bool] {
        if self.snapshot == snapshot.identity {
            &self.hidden
        } else {
            &[]
        }
    }

    /// Whether this definition of this picture is drawn.
    pub fn shows(&self, definition: usize, snapshot: &RenderSnapshot) -> bool {
        !self
            .hidden_in(snapshot)
            .get(definition)
            .copied()
            .unwrap_or(false)
    }

    /// Whether anything is hidden at all.
    pub fn anything_hidden(&self) -> bool {
        self.hidden.iter().any(|hidden| *hidden)
    }

    /// Whether this mark names a definition that currently draws geometry and
    /// can therefore be hidden.
    ///
    /// A definition may have a catalogue row and a transient pick while its
    /// mesh has no triangles or it has no placements. Such a definition is
    /// already nowhere in the picture. Calling that a successful Hide would
    /// change only the row label and enable Show all while changing no pixel.
    pub fn can_hide(&self, mark: Marked, snapshot: &RenderSnapshot) -> bool {
        self.hideable_definition(mark, snapshot).is_some()
    }

    /// Stops drawing whatever this mark is on, and says whether that changed
    /// anything.
    ///
    /// A face hides the definition it belongs to, not itself: this slice hides
    /// definitions, and hiding the one face a person could see of a part would
    /// leave the part on screen looking like a different part.
    ///
    /// Resolved through the picture, so nothing, a mark from a replaced
    /// picture and a mark from another picture all hide nothing. Hiding what
    /// is already hidden, or a definition that draws no geometry, is not a
    /// change.
    pub fn hide(&mut self, mark: Marked, snapshot: &RenderSnapshot) -> bool {
        let Some(definition) = self.hideable_definition(mark, snapshot) else {
            return false;
        };
        self.hidden[definition] = true;
        true
    }

    /// Whether Isolate would remove any geometry from this picture.
    ///
    /// True only when this mark names something still drawn and there is
    /// something else still drawn beside it. On its own, a definition is
    /// already isolated, and offering the action would be offering a press
    /// that changes no pixel.
    pub fn can_isolate(&self, mark: Marked, snapshot: &RenderSnapshot) -> bool {
        self.others_to_hide(mark, snapshot)
            .is_some_and(|others| !others.is_empty())
    }

    /// Stops drawing everything except whatever this mark is on, and says
    /// whether that changed anything.
    ///
    /// The same resolution as [`Self::hide`], applied the other way round: the
    /// mark names a definition that is still drawn, and every *other*
    /// definition still drawing geometry stops. A face isolates the part it is
    /// on, for the same reason it hides the part it is on.
    ///
    /// One way only. What was already hidden stays hidden – isolating is a way
    /// of removing distractions, not a way of revealing something – and
    /// definitions that draw nothing are already nowhere, so they are not
    /// newly marked as hidden. [`Self::show_all`] is how everything comes
    /// back.
    pub fn isolate(&mut self, mark: Marked, snapshot: &RenderSnapshot) -> bool {
        let Some(others) = self.others_to_hide(mark, snapshot) else {
            return false;
        };
        if others.is_empty() {
            return false;
        }
        for definition in others {
            self.hidden[definition] = true;
        }
        true
    }

    /// Draws everything again, and says whether that changed anything.
    pub fn show_all(&mut self) -> bool {
        if !self.anything_hidden() {
            return false;
        }
        self.hidden.fill(false);
        true
    }

    /// Where everything still drawn is, taken together.
    ///
    /// The same arithmetic as [`RenderSnapshot::bounds`], over fewer
    /// definitions. A model whose parts are all hidden is nowhere rather than
    /// at the origin, exactly as an empty picture is.
    pub fn bounds(&self, snapshot: &RenderSnapshot) -> Option<([f32; 3], [f32; 3])> {
        let hidden = self.hidden_in(snapshot);
        let mut extent = Extent::default();
        for item in &snapshot.items {
            if hidden.get(item.mesh).copied().unwrap_or(false) {
                continue;
            }
            extent.include(&snapshot.meshes[item.mesh], item);
        }
        extent.bounds()
    }

    /// Resolves one hide request all the way to geometry that is still drawn.
    fn hideable_definition(&self, mark: Marked, snapshot: &RenderSnapshot) -> Option<usize> {
        if self.snapshot != snapshot.identity {
            return None;
        }
        let definition = match mark.known_to(snapshot) {
            Marked::Nothing => return None,
            Marked::Definition(pick) => snapshot.definition(pick),
            Marked::Face(face) => snapshot.definition_of_face(face),
        }?;
        self.draws(definition, snapshot).then_some(definition)
    }

    /// What Isolate would stop drawing, for a mark that names something drawn.
    ///
    /// `None` when the mark itself resolves to nothing still drawn, which is
    /// the same refusal [`Self::hide`] makes and for the same reasons. An
    /// empty list means the mark is the only thing on screen already.
    fn others_to_hide(&self, mark: Marked, snapshot: &RenderSnapshot) -> Option<Vec<usize>> {
        let keep = self.hideable_definition(mark, snapshot)?;
        Some(
            (0..self.hidden.len())
                .filter(|definition| *definition != keep && self.draws(*definition, snapshot))
                .collect(),
        )
    }

    /// Whether this definition is currently putting anything on screen.
    ///
    /// Drawn, and with something to draw. A definition whose mesh has no
    /// triangles, or which is placed nowhere, is already absent from every
    /// pixel: hiding it would change a row label and nothing else, so no
    /// operation here counts it as something that can be removed.
    fn draws(&self, definition: usize, snapshot: &RenderSnapshot) -> bool {
        if self.hidden.get(definition).copied().unwrap_or(true) {
            return false;
        }
        snapshot
            .pick_of(definition)
            .and_then(|pick| snapshot.bounds_of(pick))
            .is_some()
    }
}

/// One placement of one definition, ready to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawItem {
    /// Which packed mesh to draw.
    pub mesh: usize,
    /// The composed world placement, column-major, as a GPU expects it.
    pub transform: [f32; 16],
    /// Linear RGB and alpha. Linear because that is what the importer read out
    /// of the file; converting it here would guess at a transfer function.
    pub colour: [f32; 4],
    /// What clicking this draws identifies, which is its definition.
    pub pick: PickId,
}

/// Everything needed to draw one view of a model, and nothing that can change.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderSnapshot {
    meshes: Vec<PackedMesh>,
    items: Vec<DrawItem>,
    /// Which definition each face belongs to, indexed by identity minus one.
    face_owner: Vec<usize>,
    min: [f32; 3],
    max: [f32; 3],
    has_geometry: bool,
    identity: ContentHash,
}

impl RenderSnapshot {
    pub fn meshes(&self) -> &[PackedMesh] {
        &self.meshes
    }

    /// The draw list, in the order the placements were added.
    ///
    /// That order is the caller's – document order, in practice – and is kept
    /// rather than sorted. Two builds of the same input produce the same list,
    /// which is what lets one frame be compared with another.
    pub fn draws(&self) -> &[DrawItem] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        !self.has_geometry
    }

    /// The world-space bounds of everything drawn, or `None` when nothing is.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        self.has_geometry.then_some((self.min, self.max))
    }

    /// The identity of one of this snapshot's definitions.
    ///
    /// The way anything other than a click asks for a definition: a list can
    /// say which row was pressed, and this turns that into the same kind of
    /// value a pick buffer yields. Checked against this snapshot for the same
    /// reason [`Self::definition`] is – a number naming a definition of some
    /// other picture would otherwise resolve here into whatever occupies it.
    ///
    /// What comes back is bound to this snapshot and outlives nothing. The
    /// index does not appear in it and cannot be recovered from it by anything
    /// but this snapshot, which is what keeps a row's position out of every
    /// durable thing a selection can become.
    pub fn pick_of(&self, definition: usize) -> Option<PickId> {
        if definition >= self.meshes.len() {
            return None;
        }
        // The same numbering `definition` reads back, and the reason zero is
        // the background rather than the first definition.
        let raw = u32::try_from(definition.checked_add(1)?).ok()?;
        Some(PickId {
            raw,
            snapshot: self.identity,
        })
    }

    /// Where everything drawn as this definition is, taken together.
    ///
    /// Every placement of it and no other definition, which is what a
    /// selection means: a pick names a definition, so what it covers is
    /// wherever that definition appears. One placement of four is not the
    /// answer, and neither is the whole picture.
    ///
    /// Resolved through this snapshot alone. Nothing, a pick from a picture
    /// that has been replaced and a pick from another picture all name no
    /// definition here, and none of them is a place. A definition with no
    /// triangles is nowhere rather than at the origin.
    pub fn bounds_of(&self, pick: PickId) -> Option<([f32; 3], [f32; 3])> {
        let definition = self.definition(pick)?;
        let mut extent = Extent::default();
        for item in self.items.iter().filter(|item| item.mesh == definition) {
            extent.include(&self.meshes[definition], item);
        }
        extent.bounds()
    }

    /// The definition a face belongs to, if this picture issued that face.
    ///
    /// Resolved here and nowhere else, for the same reason a definition is: a
    /// number that named a face of another picture would land on whichever
    /// face occupies it in this one.
    pub fn definition_of_face(&self, face: FacePickId) -> Option<usize> {
        if face.snapshot != self.identity {
            return None;
        }
        let index = (face.raw as usize).checked_sub(1)?;
        self.face_owner.get(index).copied()
    }

    /// How many faces this picture divides its definitions into.
    pub fn face_count(&self) -> usize {
        self.face_owner.len()
    }

    /// The identity this picture gave one face of one definition.
    ///
    /// The inverse of [`Self::definition_of_face`], and the only way to learn
    /// this picture's numbering from outside. A loader that knows what the
    /// kernel called each face of a mesh needs it to say the same thing about
    /// the picture; computing the number itself would be a second account of
    /// the numbering, and the one nobody exercised would drift.
    pub fn face_of(&self, definition: usize, ordinal: usize) -> Option<FacePickId> {
        let mesh = self.meshes.get(definition)?;
        if ordinal >= mesh.face_count() {
            return None;
        }
        // Faces are numbered in packing order, so a definition's own run
        // begins after every face packed before it.
        let before: usize = self.meshes[..definition]
            .iter()
            .map(PackedMesh::face_count)
            .sum();
        let raw = u32::try_from(before.checked_add(ordinal)?.checked_add(1)?).ok()?;
        Some(FacePickId {
            raw,
            snapshot: self.identity,
        })
    }

    /// Where one face of one definition is, in every placement of it.
    ///
    /// The face's own triangles rather than its definition's box: selecting a
    /// face and being shown the whole part would be showing something else.
    /// Every placement, for the same reason [`Self::bounds_of`] covers every
    /// placement – a face belongs to a definition, so it is wherever that
    /// definition appears.
    ///
    /// A face of another picture is nowhere, exactly as a pick from another
    /// picture names nothing.
    pub fn bounds_of_face(&self, face: FacePickId) -> Option<([f32; 3], [f32; 3])> {
        let definition = self.definition_of_face(face)?;
        let mesh = &self.meshes[definition];
        let mut extent = Extent::default();
        for item in self.items.iter().filter(|item| item.mesh == definition) {
            extent.include_face(mesh, item, face.raw);
        }
        extent.bounds()
    }

    /// The definition a pick identifies, if it identifies one.
    pub fn definition(&self, pick: PickId) -> Option<usize> {
        (pick.snapshot == self.identity)
            .then(|| (pick.raw as usize).checked_sub(1))
            .flatten()
            .filter(|index| *index < self.meshes.len())
    }
}

/// Collects meshes and placements into a snapshot.
///
/// Placements are added parent-first, each naming its parent by the value
/// [`place`][Self::place] returned for it, and world transforms are composed as
/// they arrive. A forward reference to a parent not yet added is refused rather
/// than deferred: a tree that has to be resolved in a second pass is a tree that
/// can contain a cycle.
#[derive(Debug, Default)]
pub struct SnapshotBuilder {
    meshes: Vec<PackedMesh>,
    items: Vec<DrawItem>,
    /// Opaque, deterministic meaning supplied by the layer interpreting the
    /// picture. It changes transient identities without putting that meaning
    /// into the snapshot itself.
    identity_context: Option<ContentHash>,
    /// The composed world transform of each placement, kept as `Transform` so
    /// composition stays in `f64` until the last moment.
    world: Vec<Transform>,
    /// The last face identity handed out. Numbered across the whole picture,
    /// so one face of one definition is one identity wherever it is placed.
    next_face: u32,
    /// Which definition each face belongs to, indexed by identity minus one.
    face_owner: Vec<usize>,
}

impl SnapshotBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds this picture's transient identities to an interpreted context.
    ///
    /// Geometry alone is not always the whole meaning of a pixel. A document
    /// may attach a durable name to one face while another document draws the
    /// same triangles without that name. The layer that knows those names can
    /// supply their deterministic digest here; the viewport retains only the
    /// digest and can then refuse a pick decoded for the other interpretation.
    pub fn bind_identities_to(&mut self, context: ContentHash) -> Result<()> {
        if self.identity_context.is_some() {
            return Err(CadError::input(
                "a picture's transient identities cannot be bound twice",
            ));
        }
        self.identity_context = Some(context);
        Ok(())
    }

    /// Packs one definition's mesh and returns its index.
    ///
    /// The mesh is validated first. A renderer cannot check an index against a
    /// vertex count on the GPU, and the failure looks like a driver fault
    /// rather than like the mesh it came from.
    pub fn add_mesh(&mut self, mesh: &Mesh) -> Result<usize> {
        mesh.validate()?;

        let vertex_count = mesh.vertex_count();
        let mut vertices = Vec::with_capacity(vertex_count * VERTEX_FLOATS);
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];

        for vertex in 0..vertex_count {
            for axis in 0..3 {
                let value = mesh.positions[vertex * 3 + axis];
                if !value.is_finite() {
                    return Err(CadError::input(format!(
                        "vertex {vertex} has a position that is not finite, and a \
                         viewport cannot frame a model whose extent is unknown"
                    )));
                }
                min[axis] = min[axis].min(value);
                max[axis] = max[axis].max(value);
                vertices.push(value);
            }
            for axis in 0..3 {
                let value = mesh.normals[vertex * 3 + axis];
                if !value.is_finite() {
                    return Err(CadError::input(format!(
                        "vertex {vertex} has a normal that is not finite"
                    )));
                }
                vertices.push(value);
            }
        }

        if vertex_count == 0 {
            min = [0.0; 3];
            max = [0.0; 3];
        }

        // The kernel guarantees the ranges are contiguous, non-overlapping and
        // cover every triangle, and refuses the mesh otherwise. What is
        // recorded is that partition and nothing about the session that
        // computed it.
        //
        // Written per vertex, which is exact only if no vertex is shared by
        // two faces. A tessellation gives each face its own nodes, so it is;
        // and it is checked here rather than believed, because a mesh that
        // broke it would draw one face in another's colour and the cause
        // would be nowhere near the symptom.
        let added_faces = u32::try_from(mesh.faces.len())
            .map_err(|_| CadError::input("a picture cannot hold that many faces"))?;
        let next_face = self
            .next_face
            .checked_add(added_faces)
            .ok_or_else(|| CadError::input("a picture cannot hold that many faces"))?;
        let mut face_of_vertex = vec![0u32; vertex_count];
        for (ordinal, range) in mesh.faces.iter().enumerate() {
            // The final value was checked above, so each value on the way to
            // it is representable as well. Kept local until every vertex has
            // been checked: a refused mesh changes no builder state.
            let id = self.next_face + ordinal as u32 + 1;
            let first = range.first_index as usize;
            let end = first + range.index_count as usize;
            for index in &mesh.indices[first..end] {
                let vertex = *index as usize;
                let claimed = &mut face_of_vertex[vertex];
                if *claimed != 0 && *claimed != id {
                    return Err(CadError::input(
                        "a mesh whose faces share a vertex cannot be pictured face by face",
                    ));
                }
                *claimed = id;
            }
        }
        let face_index_counts = mesh.faces.iter().map(|range| range.index_count).collect();
        // Ordinals are per snapshot, so the same face of one definition is one
        // identity however many times the definition is placed.
        self.face_owner
            .resize(next_face as usize, self.meshes.len());
        self.next_face = next_face;

        self.meshes.push(PackedMesh {
            vertices,
            indices: mesh.indices.clone(),
            face_of_vertex,
            face_index_counts,
            min,
            max,
        });
        Ok(self.meshes.len() - 1)
    }

    /// Places a definition, returning the index other placements name as parent.
    ///
    /// `local` is relative to `parent`, exactly as an imported scene records it.
    /// Composition happens here so a renderer never has to walk a tree, and so
    /// a placement's world transform is settled before anything can draw it.
    pub fn place(
        &mut self,
        mesh: usize,
        parent: Option<usize>,
        local: &Transform,
        colour: [f64; 3],
    ) -> Result<usize> {
        if mesh >= self.meshes.len() {
            return Err(CadError::input(format!(
                "placement names mesh {mesh}, and {} have been added",
                self.meshes.len()
            )));
        }

        let world = match parent {
            None => *local,
            Some(parent) => {
                let outer = self.world.get(parent).ok_or_else(|| {
                    CadError::input(format!(
                        "placement names parent {parent}, which has not been placed \
                         yet; parents are added before their children"
                    ))
                })?;
                local.then(outer)?
            }
        };

        let mut linear = [0.0f32; 4];
        for (slot, value) in linear.iter_mut().zip(colour) {
            if !value.is_finite() {
                return Err(CadError::input(
                    "a placement colour must be finite; a channel that is not \
                     would be uploaded as whatever the driver made of it",
                ));
            }
            let value = value as f32;
            if !value.is_finite() {
                return Err(CadError::input(
                    "a placement colour is outside the range a GPU can represent",
                ));
            }
            *slot = value;
        }
        linear[3] = 1.0;

        let transform = column_major(&world)?;
        let raw_pick = u32::try_from(mesh)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| {
                CadError::input("there are too many definitions for a u32 pick buffer")
            })?;

        if !self.meshes[mesh].indices.is_empty() {
            ensure_placeable(&transform, &self.meshes[mesh])?;
        }

        self.items.push(DrawItem {
            mesh,
            transform,
            colour: linear,
            // The pick identifies the definition and has no way to say which
            // placement of it this is. See the module documentation.
            pick: PickId::unbound(raw_pick),
        });
        self.world.push(world);
        Ok(self.items.len() - 1)
    }

    /// Freezes what has been collected.
    pub fn build(mut self) -> RenderSnapshot {
        let mut extent = Extent::default();
        for item in &self.items {
            extent.include(&self.meshes[item.mesh], item);
        }
        let (min, max) = extent
            .bounds()
            .unwrap_or(([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]));
        let has_geometry = extent.bounds().is_some();

        let identity = snapshot_identity(&self.meshes, &self.items, self.identity_context.as_ref());
        for item in &mut self.items {
            item.pick.snapshot = identity;
        }

        RenderSnapshot {
            meshes: self.meshes,
            items: self.items,
            face_owner: self.face_owner,
            min,
            max,
            has_geometry,
            identity,
        }
    }
}

/// A deterministic generation for CPU pick values.
///
/// It is deliberately absent from the raw GPU value: a pick target there must
/// stay a u32. A readback therefore retains the snapshot used for the draw,
/// while a `PickId` already decoded on the CPU refuses to resolve against a
/// different picture instead of silently keeping the same integer meaning.
fn snapshot_identity(
    meshes: &[PackedMesh],
    items: &[DrawItem],
    context: Option<&ContentHash>,
) -> ContentHash {
    let mut hasher = CanonicalHasher::new("ferritecad.render-snapshot");
    // Four: version two added faces but accidentally hashed runs in vertex
    // storage order, version three fixed that, and version four binds the
    // transient identities to any opaque interpretation supplied by the
    // layer that knows what the picture means.
    hasher.algorithm_version(4);
    hasher.field("identity_context");
    match context {
        Some(context) => {
            hasher.hash(context);
        }
        None => {
            hasher.str("none");
        }
    }
    hasher.field("meshes").u64(meshes.len() as u64);
    for mesh in meshes {
        hasher.field("vertices").u64(mesh.vertices.len() as u64);
        for value in &mesh.vertices {
            hasher.u64(u64::from(canonical_f32_bits(*value)));
        }
        hasher.field("indices").u64(mesh.indices.len() as u64);
        for index in &mesh.indices {
            hasher.u64(u64::from(*index));
        }
        // How many indices each face owns, in order. That is the partition
        // itself: where every boundary falls and how many there are. The
        // kernel's names for the faces are not hashed, because a handle
        // belongs to the session that issued it and two identical pictures
        // built twice would otherwise differ.
        hasher.field("faces").u64(mesh.face_count() as u64);
        for index_count in &mesh.face_index_counts {
            hasher.u64(u64::from(*index_count));
        }
    }
    hasher.field("items").u64(items.len() as u64);
    for item in items {
        hasher.u64(item.mesh as u64);
        for value in item.transform.iter().chain(item.colour.iter()) {
            hasher.u64(u64::from(canonical_f32_bits(*value)));
        }
        hasher.u64(u64::from(item.pick.raw));
    }
    hasher.finish()
}

fn canonical_f32_bits(value: f32) -> u32 {
    if value == 0.0 { 0 } else { value.to_bits() }
}

/// A 3x4 row-major transform as the 4x4 column-major matrix a GPU wants.
fn column_major(transform: &Transform) -> Result<[f32; 16]> {
    let rows = transform.rows();
    let mut out = [0.0f32; 16];
    for column in 0..4 {
        for row in 0..3 {
            let value = rows[row][column] as f32;
            if !value.is_finite() {
                return Err(CadError::input(
                    "a placement transform is outside the range a GPU can represent",
                ));
            }
            out[column * 4 + row] = value;
        }
    }
    out[15] = 1.0;
    Ok(out)
}

/// Checks the same corner arithmetic a vertex shader will perform.
fn ensure_placeable(matrix: &[f32; 16], mesh: &PackedMesh) -> Result<()> {
    let (low, high) = mesh.bounds();
    for corner in 0..8 {
        let point = [
            if corner & 1 == 0 { low[0] } else { high[0] },
            if corner & 2 == 0 { low[1] } else { high[1] },
            if corner & 4 == 0 { low[2] } else { high[2] },
        ];
        if apply(matrix, point).iter().any(|value| !value.is_finite()) {
            return Err(CadError::input(
                "a placement would overflow while a GPU transforms its vertices",
            ));
        }
    }
    Ok(())
}

/// Applies a column-major matrix to a point.
fn apply(matrix: &[f32; 16], point: [f32; 3]) -> [f32; 3] {
    let mut out = [0.0f32; 3];
    for (row, value) in out.iter_mut().enumerate() {
        *value = matrix[row] * point[0]
            + matrix[4 + row] * point[1]
            + matrix[8 + row] * point[2]
            + matrix[12 + row];
    }
    out
}
