// SPDX-License-Identifier: MIT
//! Scenes stored after a definition had an identity and before a placement did.
//!
//! Version 2 gave every definition the key its source file wrote down, which is
//! what made a durable reference to a *part* possible. It said nothing about
//! which *placement* of that part a reference meant, because in a version 2
//! scene there is nothing to say: two placements of one definition share their
//! key, their name, their solid count and often their transform, and the only
//! thing that ever told them apart was the position they were stored at.
//!
//! Version 2 documents are still opened, still bound, still exported and still
//! written back at version 2. What they get is the guarantee they were written
//! under and no more.
//!
//! Nothing upgrades these in place, and the reason is the same one
//! [`crate::legacy`] gives for keys. An identity minted while reading would be
//! indexed by the traversal that read it: it would look durable and behave like
//! an ordinal, two readings of one file would disagree, and a document that
//! honestly has no placement identity would start claiming one. So a version 2
//! scene keeps its layout until the file it came from is imported again, and
//! anything that needs a real placement identity is told, by name, that none
//! was recorded.

use ferritecad_types::Result;
use serde::{Deserialize, Serialize};

use crate::persist::{PersistedDefinition, Placement, project_keyed, validate_keyed};
use crate::project::Identify;
use crate::{ColourSource, Scene};

/// A placement as version 2 kept it: no identity of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyedInstance {
    /// The key — not the position — of the definition placed here.
    pub definition: String,
    /// The instance this sits inside, or `None` at the top of the scene.
    pub parent: Option<u32>,
    pub name: String,
    /// Row-major 3x4, local to the parent — exactly [`crate::Instance::placement`].
    pub placement: [f64; 12],
    pub colour_source: ColourSource,
    /// Linear RGB, meaningless when the source is [`ColourSource::None`].
    pub colour: [f64; 3],
}

/// Everything one file turned out to contain, as version 2 recorded it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyedScene {
    pub source_unit: String,
    pub schema: String,
    pub definitions: Vec<PersistedDefinition>,
    /// In document order, parents before children.
    pub instances: Vec<KeyedInstance>,
}

impl KeyedScene {
    /// Checks everything that must hold for this to describe a scene at all.
    ///
    /// The same checks the current layout runs, because they are the same
    /// question: what makes a keyed scene a scene did not change when
    /// placements gained identities. What the current layout adds on top is
    /// exactly the part about those identities.
    pub fn validate(&self) -> Result<()> {
        validate_keyed(&self.definitions, self.instances.iter().map(Self::placed))
    }

    /// Attaches this stored scene to a freshly imported one.
    ///
    /// See [`crate::PersistedScene::bind`]. This differs in one thing and it is
    /// not part of the comparison: there are no placement identities to attach
    /// afterwards.
    pub fn bind(&self, current: Scene) -> Result<Scene> {
        self.validate()?;
        let fresh = current.project(Identify::Key)?;
        project_keyed(
            &self.source_unit,
            &self.schema,
            &self.definitions,
            self.instances.iter().map(Self::placed),
        )?
        .require_same_as(&fresh)?;
        Ok(current)
    }

    /// One version 2 placement, reduced to what a comparison may look at.
    fn placed(instance: &KeyedInstance) -> Placement<'_> {
        Placement {
            definition: &instance.definition,
            parent: instance.parent,
            name: &instance.name,
            placement: &instance.placement,
            colour_source: instance.colour_source,
            colour: &instance.colour,
        }
    }
}
