// SPDX-License-Identifier: MIT
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// Distinguishes one kernel session from another.
///
/// Every session gets a fresh value, so a handle issued by one session is
/// recognisably foreign to the next. Without this a stale handle would be an
/// arbitrary integer that happens to index something, and the resulting wrong
/// face would look like a naming bug rather than a lifetime bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(u64);

impl SessionId {
    /// Issues an identifier no earlier session in this process has used.
    ///
    /// Deliberately not `Default`: `default()` reads as "an empty one" but
    /// would mint a fresh identity, and two sessions believing they are one is
    /// exactly the confusion this type exists to prevent.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "session#{}", self.0)
    }
}

/// A shape held by a kernel session.
///
/// **Never persisted.** It deliberately implements no serialisation: writing one
/// into a document would record a number whose meaning ends with the process
/// that produced it, and reading it back would silently address whatever now
/// occupies that slot. What a document stores is the feature that produces a
/// shape, not the shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShapeHandle {
    session: SessionId,
    index: u64,
}

impl ShapeHandle {
    /// Called by a kernel implementation as it takes ownership of a shape.
    pub fn new(session: SessionId, index: u64) -> Self {
        Self { session, index }
    }

    pub fn session(&self) -> SessionId {
        self.session
    }

    /// The session-local slot. Meaningful only to the session that issued it.
    pub fn index(&self) -> u64 {
        self.index
    }
}

impl fmt::Display for ShapeHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/shape#{}", self.session, self.index)
    }
}

/// What sort of geometry a sub-shape is.
///
/// Deliberately not `ferritecad_document::EntityKind`. That one is part of a
/// stored format and changes on a schema version; this one is part of a call
/// into a library. They agree today and are free to stop agreeing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SubShapeKind {
    Face,
    Edge,
    Vertex,
}

impl SubShapeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Face => "face",
            Self::Edge => "edge",
            Self::Vertex => "vertex",
        }
    }
}

impl fmt::Display for SubShapeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A face, edge or vertex inside a shape held by a kernel session.
///
/// **Never persisted**, for the same reason as [`ShapeHandle`], and with more
/// at stake: an index into a shape's faces is exactly the reference that
/// silently retargets when anything upstream changes. The document stores what
/// a face *is* — the cap of this extrusion, the side raised from that profile
/// segment — and the topology layer re-derives this handle on every rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubShapeHandle {
    shape: ShapeHandle,
    kind: SubShapeKind,
    index: u64,
}

impl SubShapeHandle {
    pub fn new(shape: ShapeHandle, kind: SubShapeKind, index: u64) -> Self {
        Self { shape, kind, index }
    }

    pub fn shape(&self) -> ShapeHandle {
        self.shape
    }

    pub fn kind(&self) -> SubShapeKind {
        self.kind
    }

    /// The session-local slot. Meaningful only to the session that issued it,
    /// and meaningless after a rebuild.
    pub fn index(&self) -> u64 {
        self.index
    }
}

impl fmt::Display for SubShapeHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}#{}", self.shape, self.kind, self.index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_are_distinguishable() {
        assert_ne!(SessionId::new(), SessionId::new());
    }

    #[test]
    fn handles_from_different_sessions_differ_even_at_the_same_slot() {
        let first = ShapeHandle::new(SessionId::new(), 0);
        let second = ShapeHandle::new(SessionId::new(), 0);
        assert_ne!(first, second);
    }

    #[test]
    fn sub_shapes_of_different_kinds_are_distinct_at_the_same_slot() {
        let shape = ShapeHandle::new(SessionId::new(), 0);
        assert_ne!(
            SubShapeHandle::new(shape, SubShapeKind::Face, 3),
            SubShapeHandle::new(shape, SubShapeKind::Edge, 3)
        );
    }

    #[test]
    fn a_handle_says_where_it_came_from() {
        let shape = ShapeHandle::new(SessionId::new(), 7);
        let face = SubShapeHandle::new(shape, SubShapeKind::Face, 2);
        assert_eq!(face.shape(), shape);
        assert!(face.to_string().contains("face#2"));
    }
}
