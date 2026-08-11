// SPDX-License-Identifier: MIT
//! The half of a scene that outlives the session that read it.
//!
//! A [`Scene`] holds [`ShapeHandle`][ferritecad_kernel::ShapeHandle]s, which
//! mean nothing outside the kernel session that issued them. Everything else a
//! file said — the units, the schema, the names, the tree, the placements and
//! the colours — is portable, and that projection is what a document can keep.
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
//! most resembles. It projects the freshly imported scene, compares the whole
//! projection position by position, and only then hands back that scene and
//! its own fresh handles. Two definitions may legitimately share a name, so a
//! name is not an identity; position is, and position is decided by source
//! bytes the caller has already verified against a content hash. Anything that
//! differs at all is refused, because the alternative — picking the nearest
//! plausible definition — hands a caller the geometry of a part it did not ask
//! for and says nothing about having done so.
//!
//! What this rests on is stated rather than assumed: identical bytes read by
//! the same importer yield the same scene in the same order. Where that does
//! not hold — a different kernel that orders an assembly differently, say —
//! the comparison fails and the caller is told, which is the intended outcome.
//!
//! There is one deliberate boundary to that statement. Two definitions with
//! identical persisted fields and perfectly symmetric occurrences are not
//! distinguishable by this projection; swapping both the definitions and
//! those indistinguishable occurrences leaves no portable value changed. No
//! durable reference points into an imported assembly yet. Before one does, a
//! source-stable definition key from the STEP/XDE layer is required — adding a
//! geometric guess here would only rename silent retargeting.

use ferritecad_types::{CadError, Result, normalize_f64};
use serde::{Deserialize, Serialize};

use crate::{ColourSource, Instance, Scene};

/// A definition as a document keeps it: what it was called and how much solid
/// geometry it turned out to hold. The shape itself is not here and cannot be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedDefinition {
    pub name: String,
    pub solids: u32,
}

/// A placement as a document keeps it.
///
/// `definition` and `parent` are positions in [`PersistedScene`], the same
/// references [`Instance`] carries, narrowed to the width they are stored at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedInstance {
    pub definition: u32,
    /// The instance this sits inside, or `None` at the top of the scene.
    pub parent: Option<u32>,
    pub name: String,
    /// Row-major 3x4, local to the parent — exactly [`Instance::placement`].
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

impl Scene {
    /// Projects out everything a document can keep.
    ///
    /// Fails rather than storing a scene that could not be compared later: a
    /// non-finite placement has no reliable equality, and an index that names
    /// no definition describes no tree.
    pub fn persist(&self) -> Result<PersistedScene> {
        let definitions = self
            .definitions
            .iter()
            .map(|definition| PersistedDefinition {
                name: definition.name.clone(),
                solids: definition.solids,
            })
            .collect();

        let mut instances = Vec::with_capacity(self.instances.len());
        for instance in &self.instances {
            instances.push(persist_instance(instance)?);
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
    /// types without going through [`Scene::persist`], so a stored scene is
    /// not trustworthy merely because it decoded.
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
                    "instance {index} refers to definition {}, and there are {definition_count}",
                    instance.definition
                )));
            }
            // Parents come before children, so a forward reference is a
            // malformed tree rather than one this reader cannot follow.
            if let Some(parent) = instance.parent
                && parent as usize >= index
            {
                return Err(CadError::input(format!(
                    "instance {index} claims instance {parent} as its parent, which does not come \
                     before it"
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
    /// saved from. Its whole portable projection is compared with this one
    /// before anything is returned; on the first difference the import is
    /// refused and nothing is bound. What comes back is `current` itself, so
    /// the only handles a caller can reach are the ones its own session issued.
    pub fn bind(&self, current: Scene) -> Result<Scene> {
        // Public fields and serde can construct a PersistedScene without ever
        // going through Scene::persist. Binding is the last boundary before
        // session-local handles become visible, so it must not rely on a
        // caller having remembered to validate the stored half first.
        self.validate()?;
        let fresh = current.persist()?;
        self.require_same_as(&fresh)?;
        Ok(current)
    }

    fn require_same_as(&self, fresh: &Self) -> Result<()> {
        require_same("unit", &self.source_unit, &fresh.source_unit)?;
        require_same("schema", &self.schema, &fresh.schema)?;

        if self.definitions.len() != fresh.definitions.len() {
            return Err(differs(format!(
                "it defined {} shape(s) and now defines {}",
                self.definitions.len(),
                fresh.definitions.len()
            )));
        }
        for (index, (stored, now)) in self.definitions.iter().zip(&fresh.definitions).enumerate() {
            require_same(
                &format!("the name of definition {index}"),
                &stored.name,
                &now.name,
            )?;
            require_same(
                &format!("the solid count of definition {index}"),
                &stored.solids,
                &now.solids,
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
                &format!("the definition of instance {index}"),
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

fn persist_instance(instance: &Instance) -> Result<PersistedInstance> {
    let definition = u32::try_from(instance.definition).map_err(|_| {
        CadError::input(format!(
            "definition index {} is beyond what this format addresses",
            instance.definition
        ))
    })?;
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

    Ok(PersistedInstance {
        definition,
        parent,
        name: instance.name.clone(),
        placement: normalise(&instance.placement, "placement")?,
        colour_source: instance.colour_source,
        colour: normalise(&instance.colour, "colour")?,
    })
}

/// Refuses anything that cannot be compared reproducibly, and collapses
/// `-0.0`, whose bits differ from `0.0` although the two compare equal.
fn normalise<const N: usize>(values: &[f64; N], what: &str) -> Result<[f64; N]> {
    let mut out = [0.0; N];
    for (slot, value) in out.iter_mut().zip(values) {
        *slot = normalize_f64(*value).map_err(|_| {
            CadError::input(format!("an imported {what} is not finite, found {value}"))
        })?;
    }
    Ok(out)
}

fn require_finite<const N: usize>(values: &[f64; N], index: usize, what: &str) -> Result<()> {
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
fn require_same<T: PartialEq + std::fmt::Display>(what: &str, stored: &T, fresh: &T) -> Result<()> {
    if stored != fresh {
        return Err(differs(format!("{what} was {stored} and is now {fresh}")));
    }
    Ok(())
}

fn differs(detail: String) -> CadError {
    CadError::input(format!(
        "this file does not import as the scene stored beside it, so nothing was bound: {detail}"
    ))
}

/// Display shims, so the comparison reports what differed rather than only
/// that something did.
struct Row<'a, const N: usize>(&'a [f64; N]);

impl<const N: usize> std::fmt::Display for Row<'_, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

impl std::fmt::Display for OptionalIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

impl std::fmt::Display for Colour {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;

    use ferritecad_kernel::{SessionId, ShapeHandle};

    use super::*;
    use crate::Definition;

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
    // The projection, in contrast, is exactly what a document stores.
    const _: () = assert!(
        Probe::<PersistedScene>::SERIALISABLE,
        "the persisted projection must be storable"
    );

    fn scene(session: SessionId) -> Scene {
        Scene {
            source_unit: "MM".to_owned(),
            schema: "AP242".to_owned(),
            definitions: vec![
                Definition {
                    shape: ShapeHandle::new(session, 11),
                    name: "Plate".to_owned(),
                    solids: 1,
                },
                Definition {
                    shape: ShapeHandle::new(session, 12),
                    name: "Bolt".to_owned(),
                    solids: 2,
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

    const IDENTITY: [f64; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

    fn moved(x: f64) -> [f64; 12] {
        let mut placement = IDENTITY;
        placement[3] = x;
        placement
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
                    name: "Plate".to_owned(),
                    solids: 1
                },
                PersistedDefinition {
                    name: "Bolt".to_owned(),
                    solids: 2
                },
            ]
        );
        assert_eq!(persisted.instances.len(), 2);
        assert_eq!(persisted.instances[1].definition, 1);
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
        // A different session, and different slots within it.
        current.definitions[0].shape = ShapeHandle::new(later, 900);
        current.definitions[1].shape = ShapeHandle::new(later, 901);

        let bound = stored.bind(current).expect("an equivalent scene binds");
        let shapes: Vec<ShapeHandle> = bound.shapes().collect();
        assert_eq!(shapes[0], ShapeHandle::new(later, 900));
        assert_eq!(shapes[1], ShapeHandle::new(later, 901));
        assert!(shapes.iter().all(|shape| shape.session() == later));
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
        refuses("a lost definition", |scene| {
            scene.definitions.pop();
            scene.instances[1].definition = 0;
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
    fn definitions_may_not_be_reordered_under_a_binding() {
        let stored = scene(SessionId::new()).persist().expect("projects");

        let later = SessionId::new();
        let mut swapped = scene(later);
        swapped.definitions.swap(0, 1);
        // The instances still describe the same assembly, so only the order of
        // the definitions themselves gives the swap away.
        swapped.instances[0].definition = 1;
        swapped.instances[1].definition = 0;

        let error = stored
            .bind(swapped)
            .expect_err("a permutation must not bind");
        assert!(
            error.to_string().contains("definition 0"),
            "the refusal should say what differed: {error}"
        );
    }

    #[test]
    fn two_definitions_of_the_same_name_are_told_apart_by_position() {
        let session = SessionId::new();
        let mut original = scene(session);
        original.definitions[1].name = "Plate".to_owned();
        let stored = original.persist().expect("projects");

        // Same names, so nothing but position distinguishes them, and the two
        // hold different geometry: one solid against two.
        let mut swapped = scene(SessionId::new());
        swapped.definitions[1].name = "Plate".to_owned();
        swapped.definitions.swap(0, 1);
        swapped.instances[0].definition = 1;
        swapped.instances[1].definition = 0;

        let error = stored
            .bind(swapped)
            .expect_err("a duplicate name must not license a swap");
        assert!(
            error.to_string().contains("solid count"),
            "the refusal should name the evidence: {error}"
        );
    }

    #[test]
    fn a_scene_that_cannot_be_compared_is_not_stored() {
        let mut broken = scene(SessionId::new());
        broken.instances[1].placement[3] = f64::NAN;
        assert!(broken.persist().is_err());

        let mut infinite = scene(SessionId::new());
        infinite.instances[1].colour[0] = f64::INFINITY;
        assert!(infinite.persist().is_err());
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
        let mut scene = scene(SessionId::new()).persist().expect("projects");
        scene.instances[1].definition = 7;
        assert!(scene.validate().is_err());

        let mut forward = scene.clone();
        forward.instances[1].definition = 0;
        forward.instances[0].parent = Some(0);
        assert!(forward.validate().is_err());

        let mut broken = scene.clone();
        broken.instances[1].definition = 0;
        broken.instances[1].placement[0] = f64::NAN;
        assert!(broken.validate().is_err());
    }

    #[test]
    fn binding_validates_the_stored_half_it_was_given() {
        let current = scene(SessionId::new());
        let mut stored = current.persist().expect("projects");
        stored.instances[1].definition = 99;

        stored
            .bind(current)
            .expect_err("a malformed stored scene must not reach its handles");
    }
}
