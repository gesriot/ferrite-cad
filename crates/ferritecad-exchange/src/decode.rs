// SPDX-License-Identifier: MIT
//! Reading the buffer an importer produced.
//!
//! The encoding is FerriteCAD's own and is described where it is written, in
//! `crates/ferritecad-occt-bridge/include/ferritecad_occt.h`. It exists
//! because a tree with names in it does not fit a handful of parallel arrays
//! across a flat ABI without inventing a second protocol.
//!
//! Every read is checked. A short buffer, a length that runs past the end, a
//! definition index that names nothing, a parent that is not already known —
//! each is an error, because the alternative is a scene assembled out of
//! whatever the bytes happened to say.

use ferritecad_kernel::{SessionId, ShapeHandle};
use std::collections::HashMap;

use ferritecad_types::{CadError, Result};

use crate::{ColourSource, Definition, Diagnostic, Import, Instance, Scene, Severity, Stage};

const MAGIC: &[u8; 4] = b"FCSI";
const FORMAT_VERSION: u16 = 2;
const NO_PARENT: u32 = u32::MAX;

/// Reads an import result, attaching shapes to `session`.
///
/// The session is passed in rather than encoded: a handle is only meaningful
/// against the session that issued it, and a buffer that named its own would
/// be a buffer that could forge one.
pub fn decode(bytes: &[u8], session: SessionId) -> Result<Import> {
    let mut reader = Reader { bytes, at: 0 };

    if reader.take(4, "magic")? != MAGIC {
        return Err(malformed("these bytes are not an import result"));
    }
    let version = reader.u16("format version")?;
    if version != FORMAT_VERSION {
        return Err(malformed(format!(
            "this import result is version {version} and this build reads \
             version {FORMAT_VERSION}"
        )));
    }

    let rejected = match reader.u8("status")? {
        0 => false,
        1 => true,
        other => {
            return Err(malformed(format!(
                "this import result has unknown status {other}"
            )));
        }
    };
    let source_unit = reader.text("source unit")?;
    let schema = reader.text("schema")?;

    let definition_count = reader.u32("definition count")?;
    let mut definitions = Vec::with_capacity(definition_count.min(4096) as usize);
    let mut definition_keys = HashMap::with_capacity(definition_count.min(4096) as usize);
    for index in 0..definition_count {
        let shape = reader.u64("definition shape")?;
        if shape == 0 {
            return Err(malformed(format!(
                "definition {index} has the reserved shape handle 0"
            )));
        }
        let name = reader.text("definition name")?;
        let solids = reader.u32("definition solids")?;
        let key = reader.text("definition key")?;
        // Checked here as well as in the bridge that produced it. This side
        // cannot release the shapes an importer already registered, so it is
        // the weaker of the two guards and exists to stop a malformed buffer
        // becoming a scene — not to make the bridge's check optional.
        if key.is_empty() {
            return Err(malformed(format!(
                concat!(
                    "definition {index} arrived without an identity, and a scene ",
                    "whose parts cannot be named again must not be built"
                ),
                index = index
            )));
        }
        if let Some(earlier) = definition_keys.insert(key.clone(), index) {
            return Err(malformed(format!(
                concat!(
                    "definitions {earlier} and {index} both claim the identity {key}, ",
                    "so a reference to either would resolve to whichever was looked up first"
                ),
                earlier = earlier,
                index = index,
                key = key
            )));
        }
        definitions.push(Definition {
            shape: ShapeHandle::new(session, shape),
            name,
            solids,
            key,
        });
    }

    let instance_count = reader.u32("instance count")?;
    let mut instances: Vec<Instance> = Vec::with_capacity(instance_count.min(65536) as usize);
    for index in 0..instance_count {
        let definition = reader.u32("instance definition")? as usize;
        if definition >= definitions.len() {
            return Err(malformed(format!(
                "instance {index} refers to definition {definition}, and there \
                 are {}",
                definitions.len()
            )));
        }

        let raw_parent = reader.u32("instance parent")?;
        let parent = if raw_parent == NO_PARENT {
            None
        } else {
            let parent = raw_parent as usize;
            // Parents come before children, so a forward reference is a
            // malformed tree rather than one this reader cannot follow.
            if parent >= index as usize {
                return Err(malformed(format!(
                    "instance {index} claims instance {parent} as its parent, \
                     which does not come before it"
                )));
            }
            Some(parent)
        };

        let name = reader.text("instance name")?;
        let mut placement = [0.0; 12];
        for value in &mut placement {
            *value = reader.finite_f64("placement")?;
        }

        let colour_source = match reader.u8("colour source")? {
            0 => ColourSource::None,
            1 => ColourSource::Instance,
            2 => ColourSource::Definition,
            other => {
                return Err(malformed(format!(
                    "instance {index} names colour source {other}, which this \
                     build does not know"
                )));
            }
        };
        let colour = [
            reader.finite_f64("colour")?,
            reader.finite_f64("colour")?,
            reader.finite_f64("colour")?,
        ];

        instances.push(Instance {
            definition,
            parent,
            name,
            placement,
            colour_source,
            colour,
        });
    }

    let diagnostic_count = reader.u32("diagnostic count")?;
    let mut diagnostics = Vec::with_capacity(diagnostic_count.min(65536) as usize);
    for _ in 0..diagnostic_count {
        let stage = match reader.u8("diagnostic stage")? {
            0 => Stage::Load,
            1 => Stage::Transfer,
            2 => Stage::Identity,
            3 => Stage::Validation,
            other => {
                return Err(malformed(format!(
                    "a diagnostic claims stage {other}, which this build does \
                     not know"
                )));
            }
        };
        let severity = match reader.u8("diagnostic severity")? {
            0 => Severity::Warning,
            1 => Severity::Fail,
            other => {
                return Err(malformed(format!(
                    "a diagnostic claims severity {other}, which this build \
                     does not know"
                )));
            }
        };
        diagnostics.push(Diagnostic {
            stage,
            severity,
            entity: reader.text("diagnostic entity")?,
            message: reader.text("diagnostic message")?,
        });
    }

    reader.finish()?;

    if rejected {
        if !source_unit.is_empty()
            || !schema.is_empty()
            || !definitions.is_empty()
            || !instances.is_empty()
        {
            return Err(malformed(
                "a rejected import carries scene data that cannot be returned",
            ));
        }
        return Ok(Import::Rejected { diagnostics });
    }
    Ok(Import::Imported {
        scene: Scene {
            source_unit,
            schema,
            definitions,
            instances,
        },
        diagnostics,
    })
}

fn malformed(what: impl Into<String>) -> CadError {
    CadError::kernel(what)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize, what: &str) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| {
                malformed(format!(
                    "the import result ends inside its {what}: {count} more \
                     byte(s) were needed and {} remain",
                    self.bytes.len() - self.at
                ))
            })?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self, what: &str) -> Result<[u8; N]> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N, what)?);
        Ok(out)
    }

    fn u8(&mut self, what: &str) -> Result<u8> {
        Ok(self.take(1, what)?[0])
    }
    fn u16(&mut self, what: &str) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array(what)?))
    }
    fn u32(&mut self, what: &str) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array(what)?))
    }
    fn u64(&mut self, what: &str) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array(what)?))
    }
    fn f64(&mut self, what: &str) -> Result<f64> {
        Ok(f64::from_le_bytes(self.array(what)?))
    }

    fn finite_f64(&mut self, what: &str) -> Result<f64> {
        let value = self.f64(what)?;
        if !value.is_finite() {
            return Err(malformed(format!(
                "the import result's {what} is not finite"
            )));
        }
        Ok(value)
    }

    fn text(&mut self, what: &str) -> Result<String> {
        let length = self.u32(what)? as usize;
        let bytes = self.take(length, what)?;
        // Lossless or not at all: a name that arrives as replacement
        // characters is a name that has already been lost, and the corpus
        // exists partly to prove that does not happen.
        String::from_utf8(bytes.to_vec())
            .map_err(|_| malformed(format!("the import result's {what} is not UTF-8")))
    }

    fn finish(self) -> Result<()> {
        let left = self.bytes.len() - self.at;
        if left > 0 {
            return Err(malformed(format!(
                "the import result has {left} byte(s) after its last diagnostic"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Encoded {
        bytes: Vec<u8>,
        status: usize,
        definition: usize,
        parent: usize,
        placement: usize,
    }

    /// Two definitions and one placement of each, so the tests can say
    /// something about how definitions relate to one another.
    fn two_definitions(first_key: &str, second_key: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        bytes.push(0);
        put_text(&mut bytes, "millimetre");
        put_text(&mut bytes, "AP242");

        bytes.extend_from_slice(&2u32.to_le_bytes());
        for (slot, name, key) in [(1u64, "Plate", first_key), (2, "Bolt", second_key)] {
            bytes.extend_from_slice(&slot.to_le_bytes());
            put_text(&mut bytes, name);
            bytes.extend_from_slice(&1u32.to_le_bytes());
            put_text(&mut bytes, key);
        }

        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes
    }

    fn put_text(out: &mut Vec<u8>, text: &str) {
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(text.as_bytes());
    }

    fn imported() -> Encoded {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        let status = bytes.len();
        bytes.push(0);
        put_text(&mut bytes, "millimetre");
        put_text(&mut bytes, "AP242");

        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&1u64.to_le_bytes());
        put_text(&mut bytes, "Plate");
        bytes.extend_from_slice(&1u32.to_le_bytes());
        put_text(&mut bytes, "step.product_definition#5");

        bytes.extend_from_slice(&1u32.to_le_bytes());
        let definition = bytes.len();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        let parent = bytes.len();
        bytes.extend_from_slice(&NO_PARENT.to_le_bytes());
        put_text(&mut bytes, "Plate");
        let placement = bytes.len();
        for value in [
            1.0f64, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0);
        for _ in 0..3 {
            bytes.extend_from_slice(&0.0f64.to_le_bytes());
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());

        Encoded {
            bytes,
            status,
            definition,
            parent,
            placement,
        }
    }

    fn rejected() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        bytes.push(1);
        put_text(&mut bytes, "");
        put_text(&mut bytes, "");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes
    }

    #[test]
    fn a_complete_import_decodes() {
        let encoded = imported();
        let outcome = decode(&encoded.bytes, SessionId::new()).expect("decodes");
        let scene = outcome.scene().expect("is imported");
        assert_eq!(scene.definitions.len(), 1);
        assert_eq!(scene.instances.len(), 1);
        assert_eq!(scene.instances[0].translation(), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn every_truncation_is_refused() {
        let encoded = imported().bytes;
        for length in 0..encoded.len() {
            assert!(
                decode(&encoded[..length], SessionId::new()).is_err(),
                "accepted the first {length} of {} bytes",
                encoded.len()
            );
        }
        assert!(decode(&encoded, SessionId::new()).is_ok());
    }

    #[test]
    fn status_is_an_enum_not_a_boolean() {
        let mut encoded = imported();
        encoded.bytes[encoded.status] = 2;
        assert!(decode(&encoded.bytes, SessionId::new()).is_err());
    }

    #[test]
    fn a_rejected_import_cannot_smuggle_in_a_scene() {
        let mut encoded = imported();
        encoded.bytes[encoded.status] = 1;
        assert!(decode(&encoded.bytes, SessionId::new()).is_err());
        assert!(matches!(
            decode(&rejected(), SessionId::new()).expect("valid rejection"),
            Import::Rejected { .. }
        ));
    }

    #[test]
    fn non_finite_placements_are_refused() {
        let mut encoded = imported();
        encoded.bytes[encoded.placement..encoded.placement + 8]
            .copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(decode(&encoded.bytes, SessionId::new()).is_err());
    }

    #[test]
    fn indices_must_describe_a_tree() {
        let mut unknown_definition = imported();
        unknown_definition.bytes[unknown_definition.definition..unknown_definition.definition + 4]
            .copy_from_slice(&1u32.to_le_bytes());
        assert!(decode(&unknown_definition.bytes, SessionId::new()).is_err());

        let mut forward_parent = imported();
        forward_parent.bytes[forward_parent.parent..forward_parent.parent + 4]
            .copy_from_slice(&0u32.to_le_bytes());
        assert!(decode(&forward_parent.bytes, SessionId::new()).is_err());
    }

    #[test]
    fn a_definition_without_an_identity_is_refused() {
        let error = decode(
            &two_definitions("step.product_definition#5", ""),
            SessionId::new(),
        )
        .expect_err("a nameless definition must not become a scene");
        assert!(error.to_string().contains("without an identity"), "{error}");
    }

    #[test]
    fn two_definitions_may_not_claim_one_identity() {
        let shared = "step.product_definition#31";
        let error = decode(&two_definitions(shared, shared), SessionId::new())
            .expect_err("a shared identity must not become a scene");
        assert!(error.to_string().contains("both claim"), "{error}");

        // The same buffer with distinct identities is the control: what is
        // refused above is the collision, not the shape of the message.
        let scene = decode(
            &two_definitions(shared, "step.product_definition#5"),
            SessionId::new(),
        )
        .expect("distinct identities decode");
        assert_eq!(scene.scene().expect("imported").definitions.len(), 2);
    }

    #[test]
    fn an_identity_diagnostic_survives_the_wire() {
        let mut bytes = rejected();
        // One diagnostic, stage 2, which is neither loading nor building.
        let count = bytes.len() - 4;
        bytes[count..].copy_from_slice(&1u32.to_le_bytes());
        bytes.push(2);
        bytes.push(1);
        put_text(&mut bytes, "step.product_definition#31");
        put_text(&mut bytes, "two definitions carry the same identity");

        let outcome = decode(&bytes, SessionId::new()).expect("decodes");
        let diagnostic = &outcome.diagnostics()[0];
        assert_eq!(diagnostic.stage, Stage::Identity);
        assert_eq!(diagnostic.severity, Severity::Fail);
        assert!(
            diagnostic.to_string().contains("identifying"),
            "{diagnostic}"
        );
    }

    #[test]
    fn trailing_bytes_are_refused() {
        let mut encoded = imported().bytes;
        encoded.push(0);
        assert!(decode(&encoded, SessionId::new()).is_err());
    }
}
