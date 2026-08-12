// SPDX-License-Identifier: MIT
//! Scenes stored before a definition had an identity.
//!
//! Version 1 documents are still opened, still bound and still re-imported.
//! What they get is the guarantee they were written under and no more: their
//! definitions are told apart by the position they were stored at, because that
//! is genuinely all a version 1 scene knows about them.
//!
//! So a file whose definitions come back in another order is refused here and
//! accepted at version 2, and that is not an inconsistency. Under version 2 the
//! reordering changed nothing the scene had recorded; under version 1 it
//! changed the only thing it had.
//!
//! Nothing upgrades these in place. A key cannot be invented from a position —
//! the result would look like an identity and behave like an index — so a
//! version 1 scene keeps its layout until the file it came from is imported
//! again, and anything that needs a real identity refuses instead.

use ferritecad_types::{CadError, Result};
use serde::{Deserialize, Serialize};

use crate::project::{Identify, Placed, Projection, normalise, position, require_finite};
use crate::{ColourSource, Scene};

/// A definition as version 1 kept it: no identity of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyDefinition {
    pub name: String,
    pub solids: u32,
}

/// A placement as version 1 kept it, naming its definition by position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegacyInstance {
    pub definition: u32,
    pub parent: Option<u32>,
    pub name: String,
    pub placement: [f64; 12],
    pub colour_source: ColourSource,
    pub colour: [f64; 3],
}

/// Everything one file turned out to contain, as version 1 recorded it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegacyScene {
    pub source_unit: String,
    pub schema: String,
    pub definitions: Vec<LegacyDefinition>,
    pub instances: Vec<LegacyInstance>,
}

impl LegacyScene {
    /// Checks everything that must hold for this to describe a scene at all.
    pub fn validate(&self) -> Result<()> {
        let definition_count = u32::try_from(self.definitions.len()).map_err(|_| {
            CadError::input(format!(
                "a scene of {} definitions is beyond what this format addresses",
                self.definitions.len()
            ))
        })?;
        u32::try_from(self.instances.len()).map_err(|_| {
            CadError::input(format!(
                "a scene of {} instances is beyond what this format addresses",
                self.instances.len()
            ))
        })?;

        for (index, instance) in self.instances.iter().enumerate() {
            if instance.definition >= definition_count {
                return Err(CadError::input(format!(
                    "instance {index} refers to definition {}, and there are \
                     {definition_count}",
                    instance.definition
                )));
            }
            if let Some(parent) = instance.parent
                && parent as usize >= index
            {
                return Err(CadError::input(format!(
                    "instance {index} claims instance {parent} as its parent, which \
                     does not come before it"
                )));
            }
            require_finite(&instance.placement, index, "placement")?;
            require_finite(&instance.colour, index, "colour")?;
        }
        Ok(())
    }

    /// Attaches this stored scene to a freshly imported one, by position.
    pub fn bind(&self, current: Scene) -> Result<Scene> {
        self.validate()?;
        let fresh = current.project(Identify::Position)?;
        self.project()?.require_same_as(&fresh)?;
        Ok(current)
    }

    fn project(&self) -> Result<Projection> {
        let definitions = self
            .definitions
            .iter()
            .enumerate()
            .map(|(index, definition)| {
                (position(index), definition.name.clone(), definition.solids)
            })
            .collect();

        let mut instances = Vec::with_capacity(self.instances.len());
        for instance in &self.instances {
            instances.push(Placed {
                definition: position(instance.definition as usize),
                parent: instance.parent,
                name: instance.name.clone(),
                placement: normalise(&instance.placement, "placement")?,
                colour_source: instance.colour_source,
                colour: normalise(&instance.colour, "colour")?,
            });
        }

        Ok(Projection {
            unit: self.source_unit.clone(),
            schema: self.schema.clone(),
            definitions,
            instances,
        })
    }
}
