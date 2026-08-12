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
//! # What identifies a definition, and what version 2 changed
//!
//! A stored scene has to say which definition each placement refers to. Version
//! 1 said it by position, because position was all it had. It therefore refused
//! a file whose definitions came back in another order, and was right to: a
//! reordered version 1 scene has changed everything it knew about itself.
//!
//! Version 2 stores the key the source file gave each definition. Identity now
//! survives reordering, so a permutation binds, while a definition that has
//! gone, one that is new, and two that claim one identity are all refused. Both
//! versions run through one comparison and the whole of their difference is
//! which identity it is given — see [`crate::project`].
//!
//! A key is local to its source. `step.product_definition#31` names something
//! within one STEP file and nothing at all between two, so a durable reference
//! has to carry the identity of the source alongside one.

use ferritecad_types::{CadError, Result};
use serde::{Deserialize, Serialize};

use crate::project::{Identify, Placed, Projection, normalise, require_finite};
use crate::{ColourSource, LegacyScene, Scene};

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

/// A stored scene, at whichever layout it was written with.
///
/// Version 1 documents keep working. They were written under a weaker
/// guarantee and they still get exactly that one, rather than being refused for
/// having been written first. What they cannot have is anything that needs an
/// identity — see [`Self::keys`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum StoredScene {
    /// Written before definitions carried identities. Binds by position.
    V1(LegacyScene),
    /// Definitions carry the keys their source gave them.
    V2(PersistedScene),
}

impl StoredScene {
    /// Attaches this stored scene to a freshly imported one.
    ///
    /// See [`PersistedScene::bind`]. Version 1 differs only in identifying its
    /// definitions by position, which is what it was stored with.
    pub fn bind(&self, current: Scene) -> Result<Scene> {
        match self {
            Self::V1(scene) => scene.bind(current),
            Self::V2(scene) => scene.bind(current),
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
            Self::V2(scene) => Some(
                scene
                    .definitions
                    .iter()
                    .map(|definition| definition.key.as_str())
                    .collect(),
            ),
        }
    }

    /// The layout this was stored at, which decides how it is written back.
    pub fn version(&self) -> u32 {
        match self {
            Self::V1(_) => 1,
            Self::V2(_) => 2,
        }
    }

    pub fn source_unit(&self) -> &str {
        match self {
            Self::V1(scene) => &scene.source_unit,
            Self::V2(scene) => &scene.source_unit,
        }
    }

    pub fn schema(&self) -> &str {
        match self {
            Self::V1(scene) => &scene.schema,
            Self::V2(scene) => &scene.schema,
        }
    }

    pub fn definition_count(&self) -> usize {
        match self {
            Self::V1(scene) => scene.definitions.len(),
            Self::V2(scene) => scene.definitions.len(),
        }
    }

    pub fn instance_count(&self) -> usize {
        match self {
            Self::V1(scene) => scene.instances.len(),
            Self::V2(scene) => scene.instances.len(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::V1(scene) => scene.validate(),
            Self::V2(scene) => scene.validate(),
        }
    }
}

impl Scene {
    /// Projects out everything a document can keep.
    ///
    /// Fails rather than storing a scene that could not be compared later: a
    /// non-finite placement has no reliable equality, a definition with no key
    /// could never be found again, and two sharing one could never be told
    /// apart.
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

impl PersistedScene {
    /// Checks everything that must hold for this to describe a scene at all.
    ///
    /// Run on the way in and on the way out. Deserialisation constructs these
    /// types without going through [`Scene::persist`], so a stored scene is not
    /// trustworthy merely because it decoded.
    pub fn validate(&self) -> Result<()> {
        for (index, definition) in self.definitions.iter().enumerate() {
            if definition.key.is_empty() {
                return Err(CadError::input(format!(
                    "definition {index} has no identity, and a part that cannot be \
                     named could never be found again"
                )));
            }
            if let Some(earlier) = self.definitions[..index]
                .iter()
                .position(|other| other.key == definition.key)
            {
                return Err(CadError::input(format!(
                    "definitions {earlier} and {index} both claim the identity {}, \
                     so a reference to it would resolve to whichever was looked up \
                     first",
                    definition.key
                )));
            }
        }

        u32::try_from(self.instances.len()).map_err(|_| {
            CadError::input(format!(
                "a scene of {} instances is beyond what this format addresses",
                self.instances.len()
            ))
        })?;

        for (index, instance) in self.instances.iter().enumerate() {
            if !self
                .definitions
                .iter()
                .any(|definition| definition.key == instance.definition)
            {
                return Err(CadError::input(format!(
                    "instance {index} places {}, which this scene does not describe",
                    instance.definition
                )));
            }
            // Parents come before children, so a forward reference is a
            // malformed tree rather than one this reader cannot follow.
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
        self.project()?.require_same_as(&fresh)?;
        Ok(current)
    }

    fn project(&self) -> Result<Projection> {
        let definitions = self
            .definitions
            .iter()
            .map(|definition| {
                (
                    definition.key.clone(),
                    definition.name.clone(),
                    definition.solids,
                )
            })
            .collect();

        let mut instances = Vec::with_capacity(self.instances.len());
        for instance in &self.instances {
            instances.push(Placed {
                definition: instance.definition.clone(),
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

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;

    use ferritecad_kernel::{SessionId, ShapeHandle};

    use super::*;
    use crate::{Definition, Instance, LegacyDefinition, LegacyInstance};

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

    #[test]
    fn a_version_2_scene_offers_its_keys_in_stored_order() {
        let stored = StoredScene::V2(scene(SessionId::new()).persist().expect("projects"));
        assert_eq!(stored.version(), 2);
        assert_eq!(stored.keys(), Some(vec![PLATE, BOLT]));
        assert_eq!(stored.definition_count(), 2);
        assert_eq!(stored.instance_count(), 2);
        assert_eq!(stored.source_unit(), "MM");
        assert_eq!(stored.schema(), "AP242");
    }
}
