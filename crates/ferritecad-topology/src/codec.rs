// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_kernel::{ArchiveSlot, BrepBlob, KernelIdentity, SubShapeKind};
use ferritecad_types::{CadError, ContentHash, ObjectId, Result};

use crate::{ArchivedFeature, generative::Name};

pub const ARCHIVE_CACHE_KIND: &str = "brep.generative.v2";

impl ArchivedFeature {
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut out = self.producer.to_bytes().to_vec();
        for part in [
            self.blob.kernel().id().as_bytes(),
            self.blob.kernel().version().as_bytes(),
            self.blob.kernel().build().as_bytes(),
            self.blob.bytes(),
        ] {
            put_bytes(&mut out, part)?;
        }
        put_count(&mut out, self.derivations.len())?;
        for (digest, node) in &self.derivations {
            out.extend_from_slice(digest.as_bytes());
            put_derivation(&mut out, node)?;
        }
        put_count(&mut out, self.entities.len())?;
        for (slot, name) in &self.entities {
            put_slot(&mut out, *slot);
            put_name(&mut out, *name);
        }
        put_count(&mut out, self.ancestry.len())?;
        for (name, slots) in &self.ancestry {
            put_name(&mut out, *name);
            put_slots(&mut out, slots)?;
        }
        put_count(&mut out, self.incidence.len())?;
        for (slot, faces) in &self.incidence {
            put_slot(&mut out, *slot);
            put_slots(&mut out, faces)?;
        }
        let hash = ContentHash::of_bytes(&out);
        out.extend_from_slice(hash.as_bytes());
        Ok(out)
    }

    pub fn decode(bytes: &[u8], producer: ObjectId, kernel: &KernelIdentity) -> Result<Self> {
        let end = bytes.len().checked_sub(32).ok_or_else(malformed)?;
        let (payload, hash) = bytes.split_at(end);
        if ContentHash::of_bytes(payload).as_bytes() != hash {
            return Err(malformed());
        }
        let mut reader = Reader(payload);
        if ObjectId::from_slice(reader.take(16)?)? != producer {
            return Err(malformed());
        }
        for part in [
            kernel.id().as_bytes(),
            kernel.version().as_bytes(),
            kernel.build().as_bytes(),
        ] {
            if reader.bytes()? != part {
                return Err(malformed());
            }
        }
        let blob = BrepBlob::new(kernel.clone(), reader.bytes()?.to_vec());
        let mut derivations = crate::derivation::Derivations::new();
        for _ in 0..reader.count()? {
            let digest = ContentHash::from_slice(reader.take(32)?)?;
            let node = std::sync::Arc::new(reader.derivation()?);
            if derivations.insert(digest, node).is_some() {
                return Err(malformed());
            }
        }
        let mut entities = BTreeMap::new();
        for _ in 0..reader.count()? {
            let slot = reader.slot()?;
            if entities.insert(slot, reader.name()?).is_some() {
                return Err(malformed());
            }
        }
        let mut ancestry = BTreeMap::new();
        for _ in 0..reader.count()? {
            let name = reader.name()?;
            if ancestry.insert(name, reader.slots()?).is_some() {
                return Err(malformed());
            }
        }
        let mut incidence = BTreeMap::new();
        for _ in 0..reader.count()? {
            let slot = reader.slot()?;
            if incidence.insert(slot, reader.slots()?).is_some() {
                return Err(malformed());
            }
        }
        if !reader.0.is_empty() {
            return Err(malformed());
        }
        let archived = Self {
            derivations,
            producer,
            blob,
            entities,
            ancestry,
            incidence,
        };
        archived.validate()?;
        Ok(archived)
    }
}

fn malformed() -> CadError {
    CadError::input("invalid generative archive; discard the complete entry")
}
fn put_count(out: &mut Vec<u8>, count: usize) -> Result<()> {
    out.extend_from_slice(&u32::try_from(count).map_err(|_| malformed())?.to_le_bytes());
    Ok(())
}
fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    put_count(out, bytes.len())?;
    out.extend_from_slice(bytes);
    Ok(())
}
fn put_slot(out: &mut Vec<u8>, slot: ArchiveSlot) {
    out.extend_from_slice(&slot.index().to_le_bytes());
}
fn put_slots(out: &mut Vec<u8>, slots: &BTreeSet<ArchiveSlot>) -> Result<()> {
    put_count(out, slots.len())?;
    for slot in slots {
        put_slot(out, *slot);
    }
    Ok(())
}
fn put_name(out: &mut Vec<u8>, name: Name) {
    out.push(match name.kind {
        SubShapeKind::Face => 0,
        SubShapeKind::Edge => 1,
        SubShapeKind::Vertex => 2,
        _ => 255,
    });
    out.extend_from_slice(name.digest.as_bytes());
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let part = self.0.get(..n).ok_or_else(malformed)?;
        self.0 = &self.0[n..];
        Ok(part)
    }
    fn count(&mut self) -> Result<usize> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().map_err(|_| malformed())?) as usize)
    }
    fn bytes(&mut self) -> Result<&'a [u8]> {
        let n = self.count()?;
        self.take(n)
    }
    fn slot(&mut self) -> Result<ArchiveSlot> {
        Ok(ArchiveSlot::new(
            u32::try_from(self.count()?).map_err(|_| malformed())?,
        ))
    }
    fn slots(&mut self) -> Result<BTreeSet<ArchiveSlot>> {
        let n = self.count()?;
        let mut set = BTreeSet::new();
        for _ in 0..n {
            if !set.insert(self.slot()?) {
                return Err(malformed());
            }
        }
        Ok(set)
    }
    fn name(&mut self) -> Result<Name> {
        let kind = match self.take(1)?[0] {
            0 => SubShapeKind::Face,
            1 => SubShapeKind::Edge,
            2 => SubShapeKind::Vertex,
            _ => return Err(malformed()),
        };
        Ok(Name {
            kind,
            digest: ContentHash::from_slice(self.take(32)?)?,
        })
    }
}

const EXPRESSION_TAGS: [&str; 7] = [
    "curve-end",
    "generated",
    "modified",
    "merge",
    "split-boundary-history",
    "incident-faces",
    "boundary",
];
fn put_optional_id(out: &mut Vec<u8>, id: Option<ObjectId>) {
    out.push(u8::from(id.is_some()));
    if let Some(id) = id {
        out.extend_from_slice(&id.to_bytes());
    }
}
fn put_derivation(out: &mut Vec<u8>, node: &crate::derivation::Derivation) -> Result<()> {
    use crate::derivation::Derivation;
    match node {
        Derivation::Curve(feature, curve, plane) => {
            out.push(0);
            out.extend_from_slice(&feature.to_bytes());
            out.extend_from_slice(&curve.to_bytes());
            put_optional_id(out, *plane);
        }
        Derivation::Cap(feature, end, plane) => {
            out.push(1);
            out.extend_from_slice(&feature.to_bytes());
            out.push(u8::from(*end));
            put_optional_id(out, *plane);
        }
        Derivation::Expression {
            kind,
            tag,
            feature,
            inputs,
            extra,
        } => {
            out.push(2);
            out.push(
                EXPRESSION_TAGS
                    .iter()
                    .position(|t| t == tag)
                    .ok_or_else(malformed)? as u8,
            );
            out.push(match kind {
                SubShapeKind::Face => 0,
                SubShapeKind::Edge => 1,
                SubShapeKind::Vertex => 2,
                _ => return Err(malformed()),
            });
            put_optional_id(out, *feature);
            out.push(*extra);
            put_count(out, inputs.len())?;
            for input in inputs {
                put_name(out, *input);
            }
        }
    }
    Ok(())
}
impl Reader<'_> {
    fn boolean(&mut self) -> Result<bool> {
        match self.take(1)?[0] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(malformed()),
        }
    }
    fn optional_id(&mut self) -> Result<Option<ObjectId>> {
        if self.boolean()? {
            Ok(Some(ObjectId::from_slice(self.take(16)?)?))
        } else {
            Ok(None)
        }
    }
    fn derivation(&mut self) -> Result<crate::derivation::Derivation> {
        use crate::derivation::Derivation;
        Ok(match self.take(1)?[0] {
            0 => Derivation::Curve(
                ObjectId::from_slice(self.take(16)?)?,
                ferritecad_types::StableEntityId::from_slice(self.take(16)?)?,
                self.optional_id()?,
            ),
            1 => Derivation::Cap(
                ObjectId::from_slice(self.take(16)?)?,
                self.boolean()?,
                self.optional_id()?,
            ),
            2 => {
                let tag = *EXPRESSION_TAGS
                    .get(self.take(1)?[0] as usize)
                    .ok_or_else(malformed)?;
                let kind = match self.take(1)?[0] {
                    0 => SubShapeKind::Face,
                    1 => SubShapeKind::Edge,
                    2 => SubShapeKind::Vertex,
                    _ => return Err(malformed()),
                };
                let feature = self.optional_id()?;
                let extra = self.take(1)?[0];
                let mut inputs = BTreeSet::new();
                for _ in 0..self.count()? {
                    if !inputs.insert(self.name()?) {
                        return Err(malformed());
                    }
                }
                Derivation::Expression {
                    kind,
                    tag,
                    feature,
                    inputs,
                    extra,
                }
            }
            _ => return Err(malformed()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferritecad_kernel::GeometryKernel;

    #[test]
    fn duplicate_rows_and_unknown_tags_are_refused_even_with_a_valid_checksum() {
        let (mut kernel, model, feature, _) = crate::archive::tests::plate();
        let archive = crate::archive_feature(&mut kernel, &model, feature).expect("archive");
        let bytes = archive.encode().expect("encode");
        let payload = &bytes[..bytes.len() - 32];
        let mut reader = Reader(payload);
        reader.take(16).expect("producer");
        for _ in 0..4 {
            reader.bytes().expect("identity and B-Rep");
        }
        let mut malformed = Vec::new();
        for table in 0..4 {
            let count_at = payload.len() - reader.0.len();
            let count = reader.count().expect("count");
            let first = payload.len() - reader.0.len();
            let mut end = first;
            for row in 0..count {
                match table {
                    0 => {
                        reader.take(32).expect("digest");
                        reader.derivation().expect("node");
                    }
                    1 => {
                        reader.slot().expect("slot");
                        reader.name().expect("name");
                    }
                    2 => {
                        reader.name().expect("name");
                        reader.slots().expect("ancestry");
                    }
                    _ => {
                        reader.slot().expect("slot");
                        reader.slots().expect("incidence");
                    }
                }
                if row == 0 {
                    end = payload.len() - reader.0.len();
                }
            }
            let mut duplicate = payload.to_vec();
            duplicate.splice(first..first, payload[first..end].iter().copied());
            duplicate[count_at..count_at + 4].copy_from_slice(&(count as u32 + 1).to_le_bytes());
            malformed.push(duplicate);
            if table == 0 || table == 1 {
                let mut unknown = payload.to_vec();
                unknown[first + if table == 0 { 32 } else { 4 }] = 255;
                malformed.push(unknown);
            }
        }
        for mut payload in malformed {
            payload.extend_from_slice(ContentHash::of_bytes(&payload).as_bytes());
            assert!(ArchivedFeature::decode(&payload, feature, kernel.identity()).is_err());
        }
    }
}
