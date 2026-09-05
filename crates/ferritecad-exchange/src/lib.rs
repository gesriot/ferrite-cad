// SPDX-License-Identifier: MIT
//! What an imported file means, in terms this project owns.
//!
//! An importer hands back a scene: definitions, which are the shapes a file
//! describes once, and instances, which are the places those shapes appear.
//! A part used four times is one definition and four instances, because that
//! is what the file says and because collapsing it would lose the only thing
//! that made it an assembly.
//!
//! # There is no `valid` flag
//!
//! Measured against Open CASCADE 8.0.1 on the committed corpus: of six
//! deliberately damaged files, the kernel refuses two outright and reads four.
//! Of those four, one more is refused by the importer because a definition has
//! no identity, two are read and described precisely in the diagnostics, and
//! one is read, transferred and reported clean while carrying a malformed
//! coordinate. So "nothing was noticed" is a fact about the reader, not about
//! the file, and a flag saying `valid: true` would state the second while
//! knowing only the first.
//!
//! What an [`Import`] offers instead is everything that was noticed, with the
//! stage it was noticed at. A caller decides what to do about it; this says
//! what happened.
//!
//! # What survives the session
//!
//! A [`Scene`] is session-bound and cannot be stored. [`PersistedScene`] is
//! its portable projection — everything the file said, with no handle in it —
//! and [`PersistedScene::bind`] is the only way back: it re-checks the whole
//! projection against a fresh import before letting a caller near the new
//! handles. See the [`persist`][mod@persist] module for why binding verifies
//! rather than matches.

mod decode;
mod keyed;
mod legacy;
mod persist;
mod project;

pub use decode::decode;
pub use keyed::{KeyedInstance, KeyedScene};
pub use legacy::{LegacyDefinition, LegacyInstance, LegacyScene};
pub use persist::{
    PersistedDefinition, PersistedInstance, PersistedScene, StoredOccurrences, StoredScene,
};

use std::fmt;

use ferritecad_kernel::ShapeHandle;
use serde::{Deserialize, Serialize};

/// A shape the file describes, once, however often it is placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    /// The geometry, in the kernel session that read it.
    pub shape: ShapeHandle,
    /// What the file called it, or empty when it called it nothing.
    pub name: String,
    pub solids: u32,
    /// What identifies this definition *inside its source*, never empty.
    ///
    /// The importer refuses a file rather than hand back a definition it
    /// cannot name, so a scene that exists has a key for every definition and
    /// no two alike. What the text means is the importer's business; what
    /// matters here is that it came from the file rather than from the reading.
    ///
    /// # Local to one source
    ///
    /// `step.product_definition#31` identifies something within one STEP file
    /// and nothing at all between two. A durable reference into an imported
    /// assembly must carry the identity of the source alongside this, and this
    /// alone is not one.
    pub key: String,
}

/// Where a colour came from, which is not the same as what it is.
///
/// A component may be painted over the definition it refers to. A reader that
/// reported only the final colour would make those two cases identical, and
/// an editor built on it could not tell which one the user was changing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ColourSource {
    /// Nothing said what colour this is.
    None,
    /// Set on this placement, overriding the definition.
    Instance,
    /// Taken from the definition.
    Definition,
}

/// A place where a definition appears.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub definition: usize,
    /// The instance this sits inside, or `None` at the top of the scene.
    pub parent: Option<usize>,
    pub name: String,
    /// Row-major 3x4: rotation and scale in the first three columns, the
    /// translation in the fourth. Local to the parent, not accumulated —
    /// composing them is the caller's business and doing it here would throw
    /// away the file's own structure.
    pub placement: [f64; 12],
    pub colour_source: ColourSource,
    /// Linear RGB. Meaningless when the source is [`ColourSource::None`].
    ///
    /// Linear because that is what Open CASCADE stores; a file written with
    /// sRGB (0.8, 0.2, 0.2) comes back as roughly (0.604, 0.033, 0.033), and
    /// calling that sRGB would be wrong by a whole transfer function.
    pub colour: [f64; 3],
}

impl Instance {
    /// The translation part, which is what most callers actually want.
    pub fn translation(&self) -> [f64; 3] {
        [self.placement[3], self.placement[7], self.placement[11]]
    }
}

/// Which part of the import noticed something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Stage {
    /// Parsing the file.
    Load,
    /// Building geometry from what was parsed.
    Transfer,
    /// Asking whether what was built can be told apart and found again.
    ///
    /// This one is FerriteCAD's, not the kernel's. A file can be read and
    /// transferred without complaint and still describe a definition nothing
    /// can name a second time; reporting that under [`Stage::Load`] would
    /// attribute to Open CASCADE a refusal it had no part in.
    Identity,
    /// Checking the topology Open CASCADE produced.
    ///
    /// A transferred definition can still contain an invalid solid. Keeping
    /// that finding separate from transfer makes a partial import explicit
    /// without claiming that the reader refused geometry it actually built.
    Validation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Severity {
    Warning,
    Fail,
}

/// One thing the importer noticed.
///
/// Serialisable because a document keeps what an import reported at the time
/// it happened; see [`PersistedScene`] for what may and may not be stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub stage: Stage,
    pub severity: Severity,
    /// What it was about — an entity type or identifier — or empty.
    pub entity: String,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stage = match self.stage {
            Stage::Load => "reading",
            Stage::Transfer => "building",
            Stage::Identity => "identifying",
            Stage::Validation => "validating",
        };
        let severity = match self.severity {
            Severity::Warning => "warning",
            Severity::Fail => "problem",
        };
        if self.entity.is_empty() {
            write!(f, "{severity} while {stage}: {}", self.message)
        } else {
            write!(
                f,
                "{severity} while {stage} {}: {}",
                self.entity, self.message
            )
        }
    }
}

/// Everything one file turned out to contain.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    /// The unit the file declared, as it declared it. Empty when it did not.
    pub source_unit: String,
    pub schema: String,
    pub definitions: Vec<Definition>,
    /// In document order, parents before children.
    pub instances: Vec<Instance>,
}

impl Scene {
    /// The instances with no parent.
    pub fn roots(&self) -> impl Iterator<Item = (usize, &Instance)> {
        self.instances
            .iter()
            .enumerate()
            .filter(|(_, instance)| instance.parent.is_none())
    }

    /// Every shape the scene refers to, for releasing them all at the end.
    pub fn shapes(&self) -> impl Iterator<Item = ShapeHandle> + '_ {
        self.definitions.iter().map(|definition| definition.shape)
    }
}

/// What an import produced.
///
/// Two outcomes and no third. A file either yielded a scene or did not; there
/// is no "yielded a scene, and it is fine", because nothing available can
/// establish the second half.
#[derive(Debug, Clone, PartialEq)]
pub enum Import {
    /// Nothing was built. The diagnostics say what was noticed before it
    /// stopped, which is often the most useful thing an import can offer.
    Rejected { diagnostics: Vec<Diagnostic> },
    /// A scene was built. Diagnostics may still be present and may still
    /// matter: two of the corpus's damaged files import completely and are
    /// described here in detail.
    Imported {
        scene: Scene,
        diagnostics: Vec<Diagnostic>,
    },
}

impl Import {
    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            Self::Rejected { diagnostics } | Self::Imported { diagnostics, .. } => diagnostics,
        }
    }

    pub fn scene(&self) -> Option<&Scene> {
        match self {
            Self::Imported { scene, .. } => Some(scene),
            Self::Rejected { .. } => None,
        }
    }

    /// How many diagnostics are failures rather than warnings.
    pub fn failures(&self) -> usize {
        self.diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Fail)
            .count()
    }
}
