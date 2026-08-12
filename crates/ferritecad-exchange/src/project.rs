// SPDX-License-Identifier: MIT
//! Comparing a stored scene with a freshly imported one.
//!
//! Both stored layouts are compared by the same code, because only one thing
//! separates them and it is worth stating exactly: **what identifies a
//! definition**. Version 1 had nothing to identify one by, so its identity is
//! the position it was written at. Version 2 carries the key the file itself
//! gave it. Everything else — that names, solid counts, the instance tree,
//! placements and colours must all agree — is the same question in both.
//!
//! Saying it that way has a consequence worth having: a permutation of the
//! definitions is refused under version 1 and accepted under version 2, and
//! that falls out of the identity rather than being a rule written twice. A
//! reordered version 1 scene really has changed, because position was all it
//! ever had to go on.

use std::collections::BTreeMap;
use std::fmt;

use ferritecad_types::{CadError, Result, normalize_f64};

use crate::{ColourSource, Definition, Scene};

/// What tells one definition from another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Identify {
    /// The key the source file gave it. Survives reordering.
    Key,
    /// Where it sits in the list, which is all a version 1 scene has.
    Position,
}

impl Identify {
    pub(crate) fn of(self, index: usize, definition: &Definition) -> String {
        match self {
            Self::Key => definition.key.clone(),
            Self::Position => position(index),
        }
    }
}

/// The identity of the definition at `index` when position is all there is.
pub(crate) fn position(index: usize) -> String {
    format!("position {index}")
}

/// One placement, reduced to what a comparison may look at.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Placed {
    /// The identity — not the position — of the definition being placed.
    pub(crate) definition: String,
    pub(crate) parent: Option<u32>,
    pub(crate) name: String,
    pub(crate) placement: [f64; 12],
    pub(crate) colour_source: ColourSource,
    pub(crate) colour: [f64; 3],
}

/// A scene reduced to the things that have to agree.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Projection {
    pub(crate) unit: String,
    pub(crate) schema: String,
    /// Identity, name and solid count, in the order they were written.
    pub(crate) definitions: Vec<(String, String, u32)>,
    pub(crate) instances: Vec<Placed>,
}

impl Scene {
    /// Reduces a live scene to what can be compared, refusing anything that
    /// could not be compared reliably.
    pub(crate) fn project(&self, identify: Identify) -> Result<Projection> {
        let mut definitions = Vec::with_capacity(self.definitions.len());
        for (index, definition) in self.definitions.iter().enumerate() {
            let identity = identify.of(index, definition);
            if identity.is_empty() {
                return Err(CadError::input(format!(
                    "definition {index} has no identity, and an unidentifiable \
                     part cannot be stored or found again"
                )));
            }
            definitions.push((identity, definition.name.clone(), definition.solids));
        }

        let mut instances = Vec::with_capacity(self.instances.len());
        for (index, instance) in self.instances.iter().enumerate() {
            let definition = self
                .definitions
                .get(instance.definition)
                .map(|definition| identify.of(instance.definition, definition))
                .ok_or_else(|| {
                    CadError::input(format!(
                        "instance {index} places definition {}, and there are {}",
                        instance.definition,
                        self.definitions.len()
                    ))
                })?;
            let parent = instance
                .parent
                .map(|parent| {
                    // Parents come before children, so a forward reference is
                    // a malformed tree rather than one this cannot follow.
                    if parent >= index {
                        return Err(CadError::input(format!(
                            "instance {index} claims instance {parent} as its \
                             parent, which does not come before it"
                        )));
                    }
                    u32::try_from(parent).map_err(|_| {
                        CadError::input(format!(
                            "parent index {parent} is beyond what this format addresses"
                        ))
                    })
                })
                .transpose()?;

            instances.push(Placed {
                definition,
                parent,
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

impl Projection {
    /// Requires that a freshly imported scene says the same as this stored one.
    ///
    /// Definitions are matched by identity and never by where they sit, so
    /// under version 2 a reordered import binds and a changed one does not.
    /// Instances are compared in order, because their order *is* the tree: the
    /// parent field is a position among instances, and a scene whose placements
    /// arrived in another order is a different scene however its parts are
    /// named.
    pub(crate) fn require_same_as(&self, fresh: &Self) -> Result<()> {
        require_same("unit", &self.unit, &fresh.unit)?;
        require_same("schema", &self.schema, &fresh.schema)?;

        let stored = by_identity(&self.definitions, "this document")?;
        let now = by_identity(&fresh.definitions, "this file")?;

        for (identity, (name, _)) in &stored {
            if !now.contains_key(identity) {
                return Err(differs(format!(
                    "it no longer describes {identity}, which was stored as {name:?}"
                )));
            }
        }
        for (identity, (name, _)) in &now {
            if !stored.contains_key(identity) {
                return Err(differs(format!(
                    "it now describes {identity} as {name:?}, and nothing was \
                     stored under that identity"
                )));
            }
        }

        for (identity, (name, solids)) in &stored {
            let (fresh_name, fresh_solids) = &now[identity];
            require_same(&format!("the name of {identity}"), name, fresh_name)?;
            require_same(
                &format!("the solid count of {identity}"),
                solids,
                fresh_solids,
            )?;
        }

        if self.instances.len() != fresh.instances.len() {
            return Err(differs(format!(
                "it placed {} instance(s) and now places {}",
                self.instances.len(),
                fresh.instances.len()
            )));
        }
        for (index, (stored, now)) in self.instances.iter().zip(&fresh.instances).enumerate() {
            require_same(
                &format!("the definition placed by instance {index}"),
                &stored.definition,
                &now.definition,
            )?;
            require_same(
                &format!("the parent of instance {index}"),
                &OptionalIndex(stored.parent),
                &OptionalIndex(now.parent),
            )?;
            require_same(
                &format!("the name of instance {index}"),
                &stored.name,
                &now.name,
            )?;
            require_same(
                &format!("the placement of instance {index}"),
                &Row(&stored.placement),
                &Row(&now.placement),
            )?;
            require_same(
                &format!("the colour source of instance {index}"),
                &Colour(stored.colour_source),
                &Colour(now.colour_source),
            )?;
            require_same(
                &format!("the colour of instance {index}"),
                &Row(&stored.colour),
                &Row(&now.colour),
            )?;
        }
        Ok(())
    }
}

/// Indexes definitions by identity, refusing a repeated one.
///
/// A duplicate makes "the definition called X" a question with two answers, and
/// every later comparison would silently be about whichever came first.
/// Ordered so two runs report the same difference first.
fn by_identity(
    definitions: &[(String, String, u32)],
    whose: &str,
) -> Result<BTreeMap<String, (String, u32)>> {
    let mut found: BTreeMap<String, (String, u32)> = BTreeMap::new();
    for (identity, name, solids) in definitions {
        if found
            .insert(identity.clone(), (name.clone(), *solids))
            .is_some()
        {
            return Err(differs(format!(
                "{whose} describes {identity} more than once, so a reference to \
                 it would resolve to whichever was looked up first"
            )));
        }
    }
    Ok(found)
}

/// Refuses anything that cannot be compared reproducibly, and collapses
/// `-0.0`, whose bits differ from `0.0` although the two compare equal.
pub(crate) fn normalise<const N: usize>(values: &[f64; N], what: &str) -> Result<[f64; N]> {
    let mut out = [0.0; N];
    for (slot, value) in out.iter_mut().zip(values) {
        *slot = normalize_f64(*value).map_err(|_| {
            CadError::input(format!("an imported {what} is not finite, found {value}"))
        })?;
    }
    Ok(out)
}

pub(crate) fn require_finite<const N: usize>(
    values: &[f64; N],
    index: usize,
    what: &str,
) -> Result<()> {
    for value in values {
        if !value.is_finite() {
            return Err(CadError::input(format!(
                "the {what} of instance {index} is not finite, found {value}"
            )));
        }
    }
    Ok(())
}

/// Compared exactly, never within a tolerance.
///
/// This is an identity check on a re-reading of bytes already proven identical,
/// not a geometric comparison. A tolerance here would let a placement that
/// really did move bind to a handle anyway.
fn require_same<T: PartialEq + fmt::Display>(what: &str, stored: &T, fresh: &T) -> Result<()> {
    if stored != fresh {
        return Err(differs(format!("{what} was {stored} and is now {fresh}")));
    }
    Ok(())
}

pub(crate) fn differs(detail: String) -> CadError {
    CadError::input(format!(
        "this file does not import as the scene stored beside it, so nothing was bound: {detail}"
    ))
}

/// Display shims, so the comparison reports what differed rather than only
/// that something did.
struct Row<'a, const N: usize>(&'a [f64; N]);

impl<const N: usize> fmt::Display for Row<'_, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[")?;
        for (index, value) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{value}")?;
        }
        f.write_str("]")
    }
}

impl<const N: usize> PartialEq for Row<'_, N> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

struct OptionalIndex(Option<u32>);

impl fmt::Display for OptionalIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(index) => write!(f, "instance {index}"),
            None => f.write_str("the top of the scene"),
        }
    }
}

impl PartialEq for OptionalIndex {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

struct Colour(ColourSource);

impl fmt::Display for Colour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            ColourSource::None => "unset",
            ColourSource::Instance => "set on the placement",
            ColourSource::Definition => "taken from the definition",
        })
    }
}

impl PartialEq for Colour {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
