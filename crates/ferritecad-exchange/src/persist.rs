// SPDX-License-Identifier: MIT
//! The half of a scene that outlives the session that read it.
//!
//! A [`Scene`] holds [`ShapeHandle`][ferritecad_kernel::ShapeHandle]s, which
//! mean nothing outside the kernel session that issued them. Everything else a
//! file said — the units, the schema, the keys, the names, the tree, the
//! placements and the colours — is portable, and that projection is what a
//! document can keep.
//!
//! These are separate types rather than `Scene` with `Serialize` derived and
//! the handles skipped. A `#[serde(skip)]` is one careless field away from
//! writing a session-local integer into a file that outlives the session, and
//! the compiler cannot warn about it. Here the compiler is the check: nothing
//! in this module can hold a handle, because no handle type implements
//! serialisation and no persisted type has a field that could carry one.
//!
//! # Binding verifies; it never matches
//!
//! [`PersistedScene::bind`] does not look for the definition that a stored one
//! most resembles. It projects the freshly imported scene, requires the two to
//! agree in full, and only then hands back that scene and its own fresh
//! handles. Anything that differs at all is refused, because the alternative —
//! picking the nearest plausible definition — hands a caller the geometry of a
//! part it did not ask for and says nothing about having done so.
//!
//! # What identifies a definition, what identifies a placement, and what each
//! version changed
//!
//! A stored scene has to say which definition each placement refers to. Version
//! 1 said it by position, because position was all it had. It therefore refused
//! a file whose definitions came back in another order, and was right to: a
//! reordered version 1 scene has changed everything it knew about itself.
//!
//! Version 2 stores the key the source file gave each definition. Identity now
//! survives reordering, so a permutation binds, while a definition that has
//! gone, one that is new, and two that claim one identity are all refused. All
//! three versions run through one comparison and the whole of their difference
//! is which identity a definition is given — see [`crate::project`].
//!
//! A key is local to its source. `step.product_definition#31` names something
//! within one STEP file and nothing at all between two, so a durable reference
//! has to carry the identity of the source alongside one.
//!
//! Version 3 gives every *placement* an [`OccurrenceId`] of its own. A key
//! identifies a part; nothing in a source file identifies one of the places
//! that part appears, and until now nothing in a document did either. Two
//! placements of one definition share their key, their name, their solid count
//! and often their transform, so what told them apart was the position they
//! were stored at — and the §22B-1e1 and §22B-1e2a measurements recorded
//! exactly what that costs downstream: inserting an unrelated sibling moved a
//! reference from one part to another, silently.
//!
//! The identity is minted once, by [`Scene::persist`], when a placement is
//! first written down. Every later reading takes it from the payload and from
//! nowhere else. That is the whole of the contract, and it is why there is no
//! constructor here that produces one from a scene that came out of a kernel:
//! a fresh reading knows the geometry and knows nothing about which occurrence
//! the document decided each place was.
//!
//! # Why a version and not a capability
//!
//! Both exist and they answer different questions. A capability says a reader
//! must understand a *vocabulary* — a role, a constraint family — whose bytes
//! are laid out exactly as before. A version says the bytes themselves are laid
//! out differently. This adds a field to every stored placement, so a reader
//! that has never heard of it cannot parse the payload at all, which is a
//! version. `exchange.step.imported.v1` already answers the only capability
//! question there is here: whether the reader understands imported STEP objects
//! and their source-of-truth bytes. Naming a second one would be counting
//! rather than saying something.

use std::collections::BTreeMap;

use ferritecad_types::{CadError, OccurrenceId, Result};
use serde::{Deserialize, Serialize};

use crate::project::{Identify, Placed, Projection, normalise, require_finite};
use crate::{ColourSource, KeyedScene, LegacyScene, Scene};

/// A definition as a document keeps it: what identifies it, what it was called
/// and how much solid geometry it turned out to hold. The shape itself is not
/// here and cannot be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedDefinition {
    /// What identifies this definition inside its source. Never empty.
    pub key: String,
    pub name: String,
    pub solids: u32,
}

/// A placement as a document keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedInstance {
    /// What durably identifies *this place*, as opposed to the part in it.
    ///
    /// Required rather than optional, and that is what makes this a version of
    /// its own. A layout in which the identity could be absent would be a
    /// layout in which "the document never recorded one" and "something dropped
    /// it" are the same value, and every reader would have to guess which. Here
    /// the older layouts are the ones with no field, they are read as
    /// themselves, and they say so — see [`StoredScene::occurrences`].
    ///
    /// Minted once by [`Scene::persist`] and afterwards only ever read back.
    /// Nothing derives it from the ordinal, the parent, the name, the transform
    /// or the definition key beside it, because every one of those changes when
    /// an unrelated part of the assembly changes.
    pub occurrence: OccurrenceId,
    /// The key — not the position — of the definition placed here.
    ///
    /// Stored as an identity rather than as an index on purpose: the claim this
    /// whole format rests on is that a position is not an identity, and a scene
    /// that recorded a position would have to be read as one.
    pub definition: String,
    /// The instance this sits inside, or `None` at the top of the scene.
    ///
    /// A position, and legitimately so. An instance is not something the source
    /// file names; it is a place, and the order of the places is the tree.
    pub parent: Option<u32>,
    pub name: String,
    /// Row-major 3x4, local to the parent — exactly [`crate::Instance::placement`].
    pub placement: [f64; 12],
    pub colour_source: ColourSource,
    /// Linear RGB, meaningless when the source is [`ColourSource::None`].
    pub colour: [f64; 3],
}

/// Everything one file turned out to contain, minus the session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedScene {
    pub source_unit: String,
    pub schema: String,
    pub definitions: Vec<PersistedDefinition>,
    /// In document order, parents before children.
    pub instances: Vec<PersistedInstance>,
}

/// What a stored scene can say about the durable identity of its placements.
///
/// Two states and no third, because there is no honest third. Either the
/// document wrote an identity down for every placement or it wrote none at all:
/// a layout does not have some.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StoredOccurrences {
    /// This scene was stored at a layout written before placements carried
    /// identities.
    ///
    /// Not lost and not missing: never recorded. The distinction is the reason
    /// this is a variant rather than an empty list — a caller handed an empty
    /// list would go looking for what went wrong, and nothing did.
    Unrecorded,
    /// One identity per instance, in the order the instances are stored.
    ///
    /// Positional alignment, and it is sound for exactly one reason: a binding
    /// has already required the fresh reading to have the same instances in the
    /// same order as the stored scene, in full. Without that check this would be
    /// an index dressed as an identity.
    Recorded(Vec<OccurrenceId>),
}

/// A stored scene, at whichever layout it was written with.
///
/// Version 1 and version 2 documents keep working. They were written under
/// weaker guarantees and they still get exactly those, rather than being
/// refused for having been written first. What they cannot have is anything
/// that needs an identity they never recorded — see [`Self::keys`] and
/// [`Self::occurrences`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum StoredScene {
    /// Written before definitions carried identities. Binds by position.
    V1(LegacyScene),
    /// Definitions carry the keys their source gave them; placements carry
    /// nothing of their own.
    V2(KeyedScene),
    /// Placements carry their own durable identities as well.
    V3(PersistedScene),
}

impl StoredScene {
    /// Attaches this stored scene to a freshly imported one.
    ///
    /// See [`PersistedScene::bind`]. Version 1 differs only in identifying its
    /// definitions by position, which is what it was stored with; version 2
    /// differs only in having no placement identities to hand on afterwards.
    pub fn bind(&self, current: Scene) -> Result<Scene> {
        match self {
            Self::V1(scene) => scene.bind(current),
            Self::V2(scene) => scene.bind(current),
            Self::V3(scene) => scene.bind(current),
        }
    }

    /// The durable identity of every placement, in stored order, or the fact
    /// that this layout recorded none.
    ///
    /// Read from the payload and computed from nothing. There is deliberately
    /// no path here that mints, hashes, enumerates or otherwise produces an
    /// identity: a caller that needs one and is told
    /// [`StoredOccurrences::Unrecorded`] must say so rather than fill it in.
    pub fn occurrences(&self) -> StoredOccurrences {
        match self {
            Self::V1(_) | Self::V2(_) => StoredOccurrences::Unrecorded,
            Self::V3(scene) => StoredOccurrences::Recorded(
                scene
                    .instances
                    .iter()
                    .map(|instance| instance.occurrence)
                    .collect(),
            ),
        }
    }

    /// The identity of every definition, in stored order, or `None` when this
    /// scene predates identities.
    ///
    /// `None` is what anything needing a durable reference has to refuse on.
    /// Synthesising keys from positions would produce something that looks like
    /// an identity and behaves like an index, which is the one failure this
    /// format exists to prevent.
    pub fn keys(&self) -> Option<Vec<&str>> {
        match self {
            Self::V1(_) => None,
            Self::V2(scene) => Some(keys_of(&scene.definitions)),
            Self::V3(scene) => Some(keys_of(&scene.definitions)),
        }
    }

    /// The layout this was stored at, which decides how it is written back.
    pub fn version(&self) -> u32 {
        match self {
            Self::V1(_) => 1,
            Self::V2(_) => 2,
            Self::V3(_) => 3,
        }
    }

    pub fn source_unit(&self) -> &str {
        match self {
            Self::V1(scene) => &scene.source_unit,
            Self::V2(scene) => &scene.source_unit,
            Self::V3(scene) => &scene.source_unit,
        }
    }

    pub fn schema(&self) -> &str {
        match self {
            Self::V1(scene) => &scene.schema,
            Self::V2(scene) => &scene.schema,
            Self::V3(scene) => &scene.schema,
        }
    }

    pub fn definition_count(&self) -> usize {
        match self {
            Self::V1(scene) => scene.definitions.len(),
            Self::V2(scene) => scene.definitions.len(),
            Self::V3(scene) => scene.definitions.len(),
        }
    }

    pub fn instance_count(&self) -> usize {
        match self {
            Self::V1(scene) => scene.instances.len(),
            Self::V2(scene) => scene.instances.len(),
            Self::V3(scene) => scene.instances.len(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::V1(scene) => scene.validate(),
            Self::V2(scene) => scene.validate(),
            Self::V3(scene) => scene.validate(),
        }
    }
}

fn keys_of(definitions: &[PersistedDefinition]) -> Vec<&str> {
    definitions
        .iter()
        .map(|definition| definition.key.as_str())
        .collect()
}

impl Scene {
    /// Projects out everything a document can keep, minting one durable
    /// identity for each placement.
    ///
    /// Fails rather than storing a scene that could not be compared later: a
    /// non-finite placement has no reliable equality, a definition with no key
    /// could never be found again, and two sharing one could never be told
    /// apart.
    ///
    /// # This is the only place an occurrence identity is created
    ///
    /// And it is called when a file is first brought into a document, never
    /// when one is read back: reading goes through [`StoredScene::bind`], which
    /// hands on what the payload already says. A second minting site would be a
    /// second answer to "which placement is this", and the reading is the half
    /// that would be wrong — a fresh import knows the geometry and knows
    /// nothing about the decisions a document made years ago.
    pub fn persist(&self) -> Result<PersistedScene> {
        let definitions = self
            .definitions
            .iter()
            .map(|definition| PersistedDefinition {
                key: definition.key.clone(),
                name: definition.name.clone(),
                solids: definition.solids,
            })
            .collect();

        let mut instances = Vec::with_capacity(self.instances.len());
        for (index, instance) in self.instances.iter().enumerate() {
            let definition = self
                .definitions
                .get(instance.definition)
                .ok_or_else(|| {
                    CadError::input(format!(
                        "instance {index} places definition {}, and there are {}",
                        instance.definition,
                        self.definitions.len()
                    ))
                })?
                .key
                .clone();
            let parent = instance
                .parent
                .map(|parent| {
                    u32::try_from(parent).map_err(|_| {
                        CadError::input(format!(
                            "parent index {parent} is beyond what this format addresses"
                        ))
                    })
                })
                .transpose()?;

            instances.push(PersistedInstance {
                // One per place, and this is the moment that place is first
                // written down. Time-ordered, so the stored order and the
                // identifiers agree on the way in; that is a property of when
                // they were made and never something a reader may rely on to
                // find one.
                occurrence: OccurrenceId::new(),
                definition,
                parent,
                name: instance.name.clone(),
                placement: normalise(&instance.placement, "placement")?,
                colour_source: instance.colour_source,
                colour: normalise(&instance.colour, "colour")?,
            });
        }

        let scene = PersistedScene {
            source_unit: self.source_unit.clone(),
            schema: self.schema.clone(),
            definitions,
            instances,
        };
        scene.validate()?;
        Ok(scene)
    }
}

/// One stored placement of either keyed layout, reduced to what makes it a
/// placement at all.
///
/// Borrowed rather than owned, and deliberately without the occurrence
/// identity: this is the vocabulary the two layouts share, and the identity is
/// exactly what they do not. What is checked about the identity is checked by
/// the layout that has one.
pub(crate) struct Placement<'a> {
    pub(crate) definition: &'a str,
    pub(crate) parent: Option<u32>,
    pub(crate) name: &'a str,
    pub(crate) placement: &'a [f64; 12],
    pub(crate) colour_source: ColourSource,
    pub(crate) colour: &'a [f64; 3],
}

/// Checks what makes a keyed scene a scene, at either layout that has keys.
///
/// Written once because it is one question. Version 2 and version 3 differ in
/// whether a placement carries an identity of its own and in nothing else, so a
/// second copy of these checks would be a second answer that could drift — and
/// the drift would land on the older layout, which is the one nobody is
/// looking at.
pub(crate) fn validate_keyed<'a>(
    definitions: &[PersistedDefinition],
    instances: impl ExactSizeIterator<Item = Placement<'a>>,
) -> Result<()> {
    // This path runs whenever an imported object is decoded or written. Index
    // once rather than scanning all earlier definitions for every key, then
    // scanning all definitions again for every placement: real assemblies can
    // contain thousands of both.
    let mut definition_keys = BTreeMap::new();
    for (index, definition) in definitions.iter().enumerate() {
        if definition.key.is_empty() {
            return Err(CadError::input(format!(
                "definition {index} has no identity, and a part that cannot be \
                 named could never be found again"
            )));
        }
        if let Some(earlier) = definition_keys.insert(definition.key.as_str(), index) {
            return Err(CadError::input(format!(
                "definitions {earlier} and {index} both claim the identity {}, \
                 so a reference to it would resolve to whichever was looked up \
                 first",
                definition.key
            )));
        }
    }

    u32::try_from(instances.len()).map_err(|_| {
        CadError::input(format!(
            "a scene of {} instances is beyond what this format addresses",
            instances.len()
        ))
    })?;

    for (index, instance) in instances.enumerate() {
        if !definition_keys.contains_key(instance.definition) {
            return Err(CadError::input(format!(
                "instance {index} places {}, which this scene does not describe",
                instance.definition
            )));
        }
        // Parents come before children, so a forward reference is a malformed
        // tree rather than one this reader cannot follow.
        if let Some(parent) = instance.parent
            && parent as usize >= index
        {
            return Err(CadError::input(format!(
                "instance {index} claims instance {parent} as its parent, which \
                 does not come before it"
            )));
        }
        require_finite(instance.placement, index, "placement")?;
        require_finite(instance.colour, index, "colour")?;
    }
    Ok(())
}

/// Reduces a stored keyed scene to what a comparison may look at.
///
/// Shared for the same reason [`validate_keyed`] is, and it carries no
/// occurrence identity for the same reason too: a binding compares what the
/// source file said, and the identity is not something a source file says. It
/// is attached afterwards, by the caller that already proved the two agree.
pub(crate) fn project_keyed<'a>(
    unit: &str,
    schema: &str,
    definitions: &[PersistedDefinition],
    instances: impl ExactSizeIterator<Item = Placement<'a>>,
) -> Result<Projection> {
    let projected_definitions = definitions
        .iter()
        .map(|definition| {
            (
                definition.key.clone(),
                definition.name.clone(),
                definition.solids,
            )
        })
        .collect();

    let mut placed = Vec::with_capacity(instances.len());
    for instance in instances {
        placed.push(Placed {
            definition: instance.definition.to_owned(),
            parent: instance.parent,
            name: instance.name.to_owned(),
            placement: normalise(instance.placement, "placement")?,
            colour_source: instance.colour_source,
            colour: normalise(instance.colour, "colour")?,
        });
    }

    Ok(Projection {
        unit: unit.to_owned(),
        schema: schema.to_owned(),
        definitions: projected_definitions,
        instances: placed,
    })
}

impl PersistedInstance {
    /// This placement, reduced to what a comparison may look at.
    fn placed(&self) -> Placement<'_> {
        Placement {
            definition: &self.definition,
            parent: self.parent,
            name: &self.name,
            placement: &self.placement,
            colour_source: self.colour_source,
            colour: &self.colour,
        }
    }
}

impl PersistedScene {
    /// Checks everything that must hold for this to describe a scene at all.
    ///
    /// Run on the way in and on the way out. Deserialisation constructs these
    /// types without going through [`Scene::persist`], so a stored scene is not
    /// trustworthy merely because it decoded.
    ///
    /// On top of what every keyed scene is held to, this requires that the
    /// placement identities are all different. A malformed one cannot reach
    /// here — [`OccurrenceId`] refuses anything that is not a UUIDv7 while the
    /// payload is still being decoded — and a missing one cannot either,
    /// because at this layout the field is not optional. What is left is the
    /// one damaged state the types permit: two placements answering to one
    /// identity, which would make a reference to it resolve to whichever was
    /// looked at first.
    pub fn validate(&self) -> Result<()> {
        validate_keyed(&self.definitions, self.instances.iter().map(Self::placed))?;

        let mut seen: BTreeMap<OccurrenceId, usize> = BTreeMap::new();
        for (index, instance) in self.instances.iter().enumerate() {
            if let Some(earlier) = seen.insert(instance.occurrence, index) {
                return Err(CadError::input(format!(
                    "instances {earlier} and {index} are both the occurrence {}, so a \
                     reference to that placement would resolve to whichever was looked \
                     up first",
                    instance.occurrence
                )));
            }
        }
        Ok(())
    }

    /// One placement, as the shared checks want it.
    ///
    /// A free-standing associated function rather than a closure so both
    /// call sites read the same and neither can quietly look at a different
    /// field.
    fn placed(instance: &PersistedInstance) -> Placement<'_> {
        instance.placed()
    }

    /// Attaches this stored scene to a freshly imported one.
    ///
    /// `current` must have come from importing the exact bytes this scene was
    /// saved from. Definitions are matched by the identity their source gave
    /// them, so a file whose parts come back in another order binds while one
    /// whose parts changed does not. On the first difference the import is
    /// refused and nothing is bound. What comes back is `current` itself, so the
    /// only handles a caller can reach are the ones its own session issued.
    pub fn bind(&self, current: Scene) -> Result<Scene> {
        // Public fields and serde can construct a PersistedScene without ever
        // going through Scene::persist. Binding is the last boundary before
        // session-local handles become visible, so it must not rely on a caller
        // having remembered to validate the stored half first.
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
}

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;

    use ferritecad_kernel::{SessionId, ShapeHandle};

    use super::*;
    use crate::{Definition, Instance, KeyedInstance, LegacyDefinition, LegacyInstance};

    /// Answers "does this type implement `Serialize`" as a `const bool`.
    ///
    /// An inherent associated constant wins over a trait one, so the inherent
    /// impl below applies exactly when the bound is satisfied and the blanket
    /// trait impl answers otherwise. Rust has no negative bound to say this
    /// directly, and the property is worth a little machinery: the whole
    /// persistence contract rests on a handle being impossible to write down.
    struct Probe<T>(PhantomData<T>);

    trait NotSerialisable {
        const SERIALISABLE: bool = false;
    }

    impl<T> NotSerialisable for Probe<T> {}

    impl<T: Serialize> Probe<T> {
        const SERIALISABLE: bool = true;
    }

    // Gates on the build rather than on a test run: a handle that became
    // serialisable must stop compiling, not fail once and be re-run.
    const _: () = assert!(
        !Probe::<ShapeHandle>::SERIALISABLE,
        "ShapeHandle became serialisable; a session-local integer can now reach a file"
    );
    const _: () = assert!(
        !Probe::<Scene>::SERIALISABLE,
        "Scene became serialisable, and it holds handles"
    );
    const _: () = assert!(
        !Probe::<Definition>::SERIALISABLE,
        "Definition became serialisable, and it holds a handle"
    );
    // The projections, in contrast, are exactly what a document stores.
    const _: () = assert!(
        Probe::<PersistedScene>::SERIALISABLE,
        "the persisted projection must be storable"
    );
    const _: () = assert!(
        Probe::<LegacyScene>::SERIALISABLE,
        "a version 1 scene must still be readable from a document"
    );
    // And the versioned wrapper is not: which layout a scene is at is said by
    // the envelope around it, not by a tag inside it.
    const _: () = assert!(
        !Probe::<StoredScene>::SERIALISABLE,
        "StoredScene became serialisable, which would put the layout in two places"
    );

    const PLATE: &str = "step.product_definition#5";
    const BOLT: &str = "step.product_definition#31";

    const IDENTITY: [f64; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

    fn moved(x: f64) -> [f64; 12] {
        let mut placement = IDENTITY;
        placement[3] = x;
        placement
    }

    fn scene(session: SessionId) -> Scene {
        Scene {
            source_unit: "MM".to_owned(),
            schema: "AP242".to_owned(),
            definitions: vec![
                Definition {
                    shape: ShapeHandle::new(session, 11),
                    name: "Plate".to_owned(),
                    solids: 1,
                    key: PLATE.to_owned(),
                },
                Definition {
                    shape: ShapeHandle::new(session, 12),
                    name: "Bolt".to_owned(),
                    solids: 2,
                    key: BOLT.to_owned(),
                },
            ],
            instances: vec![
                Instance {
                    definition: 0,
                    parent: None,
                    name: "Assembly".to_owned(),
                    placement: IDENTITY,
                    colour_source: ColourSource::None,
                    colour: [0.0; 3],
                },
                Instance {
                    definition: 1,
                    parent: Some(0),
                    name: "Bolt/1".to_owned(),
                    placement: moved(3.0),
                    colour_source: ColourSource::Instance,
                    colour: [0.25, 0.5, 0.75],
                },
            ],
        }
    }

    #[test]
    fn a_scene_keeps_all_of_its_portable_meaning() {
        let persisted = scene(SessionId::new()).persist().expect("projects");

        assert_eq!(persisted.source_unit, "MM");
        assert_eq!(persisted.schema, "AP242");
        assert_eq!(
            persisted.definitions,
            vec![
                PersistedDefinition {
                    key: PLATE.to_owned(),
                    name: "Plate".to_owned(),
                    solids: 1
                },
                PersistedDefinition {
                    key: BOLT.to_owned(),
                    name: "Bolt".to_owned(),
                    solids: 2
                },
            ]
        );
        assert_eq!(persisted.instances.len(), 2);
        // The placement names the part it places, not where that part happens
        // to be written down.
        assert_eq!(persisted.instances[1].definition, BOLT);
        assert_eq!(persisted.instances[1].parent, Some(0));
        assert_eq!(persisted.instances[1].name, "Bolt/1");
        assert_eq!(persisted.instances[1].placement, moved(3.0));
        assert_eq!(persisted.instances[1].colour_source, ColourSource::Instance);
        assert_eq!(persisted.instances[1].colour, [0.25, 0.5, 0.75]);
    }

    #[test]
    fn another_session_binds_and_hands_back_its_own_handles() {
        let stored = scene(SessionId::new()).persist().expect("projects");

        let later = SessionId::new();
        let mut current = scene(later);
        current.definitions[0].shape = ShapeHandle::new(later, 900);
        current.definitions[1].shape = ShapeHandle::new(later, 901);

        let bound = stored.bind(current).expect("an equivalent scene binds");
        let shapes: Vec<ShapeHandle> = bound.shapes().collect();
        assert_eq!(shapes[0], ShapeHandle::new(later, 900));
        assert_eq!(shapes[1], ShapeHandle::new(later, 901));
        assert!(shapes.iter().all(|shape| shape.session() == later));
    }

    #[test]
    fn a_reordered_import_binds_because_identity_is_not_position() {
        let stored = scene(SessionId::new()).persist().expect("projects");

        let later = SessionId::new();
        let mut swapped = scene(later);
        swapped.definitions.swap(0, 1);
        for instance in &mut swapped.instances {
            instance.definition = 1 - instance.definition;
        }

        let bound = stored
            .bind(swapped)
            .expect("the same parts in another order are the same parts");

        // Each placement still holds the part it named, which is the whole
        // point of storing an identity rather than an index.
        let placed = |instance: usize| &bound.definitions[bound.instances[instance].definition];
        assert_eq!(placed(0).key, PLATE);
        assert_eq!(placed(0).solids, 1);
        assert_eq!(placed(1).key, BOLT);
        assert_eq!(placed(1).solids, 2);
    }

    /// Runs `damage` on a fresh scene and requires that binding refuses it.
    fn refuses(what: &str, damage: impl FnOnce(&mut Scene)) {
        let stored = scene(SessionId::new()).persist().expect("projects");
        let mut current = scene(SessionId::new());
        damage(&mut current);
        let error = stored
            .bind(current)
            .expect_err(&format!("{what} should not have bound"));
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input, "{what}");
    }

    #[test]
    fn nothing_portable_may_change_under_a_binding() {
        refuses("a renamed definition", |scene| {
            scene.definitions[0].name = "Plate B".to_owned();
        });
        refuses("a different solid count", |scene| {
            scene.definitions[0].solids = 2;
        });
        refuses("a different unit", |scene| {
            scene.source_unit = "INCH".to_owned();
        });
        refuses("a different schema", |scene| {
            scene.schema = "AP203".to_owned();
        });
        refuses("a reparented instance", |scene| {
            scene.instances[1].parent = None;
        });
        refuses("a retargeted instance", |scene| {
            scene.instances[1].definition = 0;
        });
        refuses("a renamed instance", |scene| {
            scene.instances[1].name = "Bolt/2".to_owned();
        });
        refuses("a moved instance", |scene| {
            scene.instances[1].placement = moved(3.000_000_1);
        });
        refuses("a different colour source", |scene| {
            scene.instances[1].colour_source = ColourSource::Definition;
        });
        refuses("a different colour", |scene| {
            scene.instances[1].colour = [0.25, 0.5, 0.76];
        });
        refuses("an extra instance", |scene| {
            let extra = scene.instances[1].clone();
            scene.instances.push(extra);
        });
    }

    #[test]
    fn the_set_of_identities_must_match_exactly() {
        refuses("a definition that has gone", |scene| {
            scene.definitions.pop();
            scene.instances[1].definition = 0;
        });
        refuses("a definition that is new", |scene| {
            let mut extra = scene.definitions[1].clone();
            extra.key = "step.product_definition#99".to_owned();
            scene.definitions.push(extra);
        });
        refuses("a definition whose identity changed", |scene| {
            scene.definitions[1].key = "step.product_definition#99".to_owned();
        });
        refuses("two definitions claiming one identity", |scene| {
            scene.definitions[1].key = PLATE.to_owned();
        });
    }

    #[test]
    fn a_scene_that_cannot_be_compared_is_not_stored() {
        let mut broken = scene(SessionId::new());
        broken.instances[1].placement[3] = f64::NAN;
        assert!(broken.persist().is_err());

        let mut infinite = scene(SessionId::new());
        infinite.instances[1].colour[0] = f64::INFINITY;
        assert!(infinite.persist().is_err());

        let mut nameless = scene(SessionId::new());
        nameless.definitions[1].key = String::new();
        assert!(nameless.persist().is_err());

        let mut collided = scene(SessionId::new());
        collided.definitions[1].key = PLATE.to_owned();
        assert!(collided.persist().is_err());
    }

    #[test]
    fn signed_zero_does_not_make_two_readings_disagree() {
        let mut negative = scene(SessionId::new());
        negative.instances[0].placement[3] = -0.0;
        let stored = negative.persist().expect("projects");

        let mut positive = scene(SessionId::new());
        positive.instances[0].placement[3] = 0.0;
        stored
            .bind(positive)
            .expect("a signed zero is the same place");
    }

    #[test]
    fn a_decoded_scene_is_not_trusted_merely_for_decoding() {
        let mut unknown = scene(SessionId::new()).persist().expect("projects");
        unknown.instances[1].definition = "step.product_definition#404".to_owned();
        assert!(unknown.validate().is_err());

        let mut forward = scene(SessionId::new()).persist().expect("projects");
        forward.instances[0].parent = Some(0);
        assert!(forward.validate().is_err());

        let mut broken = scene(SessionId::new()).persist().expect("projects");
        broken.instances[1].placement[0] = f64::NAN;
        assert!(broken.validate().is_err());

        let mut collided = scene(SessionId::new()).persist().expect("projects");
        collided.definitions[1].key = PLATE.to_owned();
        assert!(collided.validate().is_err());
    }

    #[test]
    fn binding_validates_the_stored_half_it_was_given() {
        let mut stored = scene(SessionId::new()).persist().expect("projects");
        stored.definitions[1].key = String::new();
        assert!(stored.bind(scene(SessionId::new())).is_err());
    }

    /// The same assembly as [`scene`], as version 1 recorded it.
    fn legacy() -> LegacyScene {
        LegacyScene {
            source_unit: "MM".to_owned(),
            schema: "AP242".to_owned(),
            definitions: vec![
                LegacyDefinition {
                    name: "Plate".to_owned(),
                    solids: 1,
                },
                LegacyDefinition {
                    name: "Bolt".to_owned(),
                    solids: 2,
                },
            ],
            instances: vec![
                LegacyInstance {
                    definition: 0,
                    parent: None,
                    name: "Assembly".to_owned(),
                    placement: IDENTITY,
                    colour_source: ColourSource::None,
                    colour: [0.0; 3],
                },
                LegacyInstance {
                    definition: 1,
                    parent: Some(0),
                    name: "Bolt/1".to_owned(),
                    placement: moved(3.0),
                    colour_source: ColourSource::Instance,
                    colour: [0.25, 0.5, 0.75],
                },
            ],
        }
    }

    #[test]
    fn a_version_1_scene_binds_by_position_and_says_it_has_no_keys() {
        let stored = StoredScene::V1(legacy());
        assert_eq!(stored.version(), 1);
        assert!(
            stored.keys().is_none(),
            "a scene stored without identities must not offer any"
        );

        let later = SessionId::new();
        stored
            .bind(scene(later))
            .expect("an unchanged scene binds by position");

        // The reordering version 2 accepts is refused here, and rightly:
        // position was the only thing this scene ever recorded.
        let mut swapped = scene(SessionId::new());
        swapped.definitions.swap(0, 1);
        for instance in &mut swapped.instances {
            instance.definition = 1 - instance.definition;
        }
        assert!(stored.bind(swapped).is_err());
    }

    /// The same assembly as [`scene`], as version 2 recorded it.
    fn keyed() -> KeyedScene {
        let current = scene(SessionId::new()).persist().expect("projects");
        KeyedScene {
            source_unit: current.source_unit,
            schema: current.schema,
            definitions: current.definitions,
            instances: current
                .instances
                .into_iter()
                .map(|instance| KeyedInstance {
                    definition: instance.definition,
                    parent: instance.parent,
                    name: instance.name,
                    placement: instance.placement,
                    colour_source: instance.colour_source,
                    colour: instance.colour,
                })
                .collect(),
        }
    }

    #[test]
    fn a_version_2_scene_offers_its_keys_and_says_it_has_no_placement_identities() {
        let stored = StoredScene::V2(keyed());
        assert_eq!(stored.version(), 2);
        assert_eq!(stored.keys(), Some(vec![PLATE, BOLT]));
        assert_eq!(stored.definition_count(), 2);
        assert_eq!(stored.instance_count(), 2);
        assert_eq!(stored.source_unit(), "MM");
        assert_eq!(stored.schema(), "AP242");
        assert_eq!(
            stored.occurrences(),
            StoredOccurrences::Unrecorded,
            "a scene stored before placements had identities must not offer any"
        );
        // And it still binds, unchanged and by key, which is the guarantee it
        // was written under.
        stored
            .bind(scene(SessionId::new()))
            .expect("an unchanged version 2 scene still binds");
    }

    #[test]
    fn a_version_1_scene_has_no_placement_identities_either() {
        assert_eq!(
            StoredScene::V1(legacy()).occurrences(),
            StoredOccurrences::Unrecorded
        );
    }

    #[test]
    fn a_current_scene_offers_one_identity_per_placement_in_stored_order() {
        let persisted = scene(SessionId::new()).persist().expect("projects");
        let stored = StoredScene::V3(persisted.clone());
        assert_eq!(stored.version(), 3);
        assert_eq!(stored.keys(), Some(vec![PLATE, BOLT]));

        let StoredOccurrences::Recorded(recorded) = stored.occurrences() else {
            unreachable!("the current layout records placement identities")
        };
        assert_eq!(recorded.len(), 2);
        assert_eq!(
            recorded,
            vec![
                persisted.instances[0].occurrence,
                persisted.instances[1].occurrence
            ],
            "the identities offered are not the ones stored, in the order stored"
        );
        assert_ne!(
            recorded[0], recorded[1],
            "two placements of one scene were given one identity"
        );
    }

    #[test]
    fn two_placements_of_one_definition_are_two_identities() {
        let mut twice = scene(SessionId::new());
        // A second placement of the bolt that agrees with the first about
        // everything a file records: same definition, same parent, same name,
        // same transform, same colour. Nothing but the identity tells them
        // apart, which is the case this whole layout exists for.
        let repeat = twice.instances[1].clone();
        twice.instances.push(repeat);

        let persisted = twice.persist().expect("projects");
        assert_eq!(persisted.instances[1].definition, BOLT);
        assert_eq!(
            persisted.instances[1].definition,
            persisted.instances[2].definition
        );
        assert_eq!(persisted.instances[1].name, persisted.instances[2].name);
        assert_eq!(
            persisted.instances[1].placement,
            persisted.instances[2].placement
        );
        assert_ne!(
            persisted.instances[1].occurrence, persisted.instances[2].occurrence,
            "two indistinguishable placements were given one identity"
        );
    }

    #[test]
    fn one_scene_persisted_twice_mints_two_sets_of_identities() {
        // Persisting is the moment a place is first written down, so two
        // separate documents holding the same bytes are two separate sets of
        // places. An identity derived from the definition key, the ordinal, the
        // name or the transform would make these equal, and two unrelated
        // documents would then claim the same placements.
        let source = scene(SessionId::new());
        let one = source.persist().expect("projects");
        let other = source.persist().expect("projects");
        for (left, right) in one.instances.iter().zip(&other.instances) {
            assert_eq!(left.definition, right.definition);
            assert_eq!(left.placement, right.placement);
            assert_ne!(
                left.occurrence, right.occurrence,
                "the identity was derived from something the two persistings share"
            );
        }
    }

    #[test]
    fn binding_does_not_change_the_stored_identities() {
        // Binding proves a fresh reading is the same scene. It has no business
        // replacing what the document recorded about it, and the fresh reading
        // has nothing to replace it with.
        let stored = scene(SessionId::new()).persist().expect("projects");
        let before: Vec<_> = stored
            .instances
            .iter()
            .map(|instance| instance.occurrence)
            .collect();
        stored
            .bind(scene(SessionId::new()))
            .expect("an equivalent scene binds");
        let after: Vec<_> = stored
            .instances
            .iter()
            .map(|instance| instance.occurrence)
            .collect();
        assert_eq!(before, after);
    }

    #[test]
    fn two_placements_claiming_one_identity_are_refused_before_anything_binds() {
        let mut collided = scene(SessionId::new()).persist().expect("projects");
        collided.instances[1].occurrence = collided.instances[0].occurrence;
        let refusal = collided
            .validate()
            .expect_err("one identity cannot name two placements");
        assert!(refusal.to_string().contains("occurrence"), "{refusal}");
        assert!(
            collided.bind(scene(SessionId::new())).is_err(),
            "binding accepted a stored scene whose identities collide"
        );
    }

    #[test]
    fn a_malformed_identity_never_decodes_at_all() {
        // Not a check this layout has to make: an identity that is not a UUIDv7
        // is refused while the payload is still being read, so nothing
        // downstream ever sees one.
        assert!(
            "550e8400-e29b-41d4-a716-446655440000"
                .parse::<OccurrenceId>()
                .is_err(),
            "a version 4 UUID was accepted as a placement identity"
        );
        assert!("not-a-uuid".parse::<OccurrenceId>().is_err());
    }
}
