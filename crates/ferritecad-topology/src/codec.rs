// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_kernel::{ArchiveSlot, BrepBlob, KernelIdentity, SubShapeKind};
use ferritecad_types::{CadError, ContentHash, ObjectId, Result};

use crate::{ArchivedFeature, generative::Name};

pub const ARCHIVE_CACHE_KIND: &str = "brep.generative.v1";

impl ArchivedFeature {
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut out = self.producer.to_bytes().to_vec();
        for part in [
            self.blob.kernel().id().as_bytes(),
            self.blob.kernel().version().as_bytes(),
            self.blob.kernel().build().as_bytes(),
            self.blob.bytes(),
        ] {
            put_bytes(&mut out, part)?;
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
    CadError::topology("invalid generative archive; discard the complete entry")
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
