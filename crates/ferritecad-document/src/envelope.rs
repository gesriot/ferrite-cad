use ferritecad_types::{CadError, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// The self-describing wrapper around every stored object payload.
///
/// The payload is a *nested* CBOR byte string rather than an inline structure.
/// That costs a few bytes per object and buys the property the format depends
/// on: an object of an unknown type can be carried through a load-and-save
/// cycle without ever being decoded, so nothing can be lost in a round trip
/// through a build that did not understand it.
#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    /// Discriminates the payload, e.g. `"sketch"` or `"feature.extrude"`.
    pub type_name: String,
    /// Version of the payload's own layout, independent of the file schema.
    pub schema_version: u32,
    /// Capabilities a reader must implement to modify this object safely.
    pub required_capabilities: Vec<String>,
    /// CBOR encoding of the payload itself.
    pub payload: Vec<u8>,
}

impl Envelope {
    pub fn new(
        type_name: impl Into<String>,
        schema_version: u32,
        required_capabilities: Vec<String>,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            type_name: type_name.into(),
            schema_version,
            required_capabilities,
            payload,
        }
    }

    /// Wraps a typed payload.
    pub fn encode<T: Serialize>(
        type_name: impl Into<String>,
        schema_version: u32,
        required_capabilities: Vec<String>,
        payload: &T,
    ) -> Result<Self> {
        let mut bytes = Vec::new();
        ciborium::into_writer(payload, &mut bytes)
            .map_err(|e| CadError::input_because("encoding object payload", e))?;
        Ok(Self::new(
            type_name,
            schema_version,
            required_capabilities,
            bytes,
        ))
    }

    /// Unwraps the payload as `T`.
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T> {
        ciborium::from_reader(self.payload.as_slice()).map_err(|e| {
            CadError::input_because(
                format!(
                    "decoding {} payload (schema v{})",
                    self.type_name, self.schema_version
                ),
                e,
            )
        })
    }

    /// Serialises the envelope for storage.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let raw = RawEnvelope {
            type_name: self.type_name.clone(),
            schema_version: self.schema_version,
            required_capabilities: self.required_capabilities.clone(),
            payload: serde_bytes::ByteBuf::from(self.payload.clone()),
        };
        let mut bytes = Vec::new();
        ciborium::into_writer(&raw, &mut bytes)
            .map_err(|e| CadError::input_because("encoding object envelope", e))?;
        Ok(bytes)
    }

    /// Reads an envelope written by [`Envelope::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let raw: RawEnvelope = ciborium::from_reader(bytes)
            .map_err(|e| CadError::input_because("decoding object envelope", e))?;
        Ok(Self {
            type_name: raw.type_name,
            schema_version: raw.schema_version,
            required_capabilities: raw.required_capabilities,
            payload: raw.payload.into_vec(),
        })
    }
}

/// An object whose type this build does not implement.
///
/// The original envelope bytes are kept exactly as they were read and written
/// back verbatim. Nothing is re-encoded, so no encoder detail — map ordering,
/// integer width, float representation — can perturb a payload we never
/// understood in the first place.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownObject {
    pub type_name: String,
    pub schema_version: u32,
    pub required_capabilities: Vec<String>,
    raw_envelope: Vec<u8>,
}

impl UnknownObject {
    pub(crate) fn new(envelope: Envelope, raw_envelope: Vec<u8>) -> Self {
        Self {
            type_name: envelope.type_name,
            schema_version: envelope.schema_version,
            required_capabilities: envelope.required_capabilities,
            raw_envelope,
        }
    }

    /// The bytes to write back, unchanged.
    pub fn raw_envelope(&self) -> &[u8] {
        &self.raw_envelope
    }
}

#[derive(Serialize, Deserialize)]
struct RawEnvelope {
    #[serde(rename = "type")]
    type_name: String,
    schema_version: u32,
    required_capabilities: Vec<String>,
    payload: serde_bytes::ByteBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        height: f64,
        label: String,
    }

    #[test]
    fn typed_payload_round_trips() {
        let sample = Sample {
            height: 12.5,
            label: "boss".to_owned(),
        };
        let envelope = Envelope::encode(
            "feature.extrude",
            1,
            vec!["core.part.v1".to_owned()],
            &sample,
        )
        .expect("encodes");

        let bytes = envelope.to_bytes().expect("serialises");
        let read = Envelope::from_bytes(&bytes).expect("deserialises");

        assert_eq!(read, envelope);
        assert_eq!(read.decode::<Sample>().expect("decodes"), sample);
    }

    #[test]
    fn payload_is_a_cbor_byte_string_not_an_array() {
        let envelope = Envelope::new("t", 1, Vec::new(), vec![0xde, 0xad]);
        let bytes = envelope.to_bytes().expect("serialises");

        // A two-byte CBOR byte string is 0x42 0xde 0xad. Encoding the payload
        // as an array of integers instead would defeat verbatim preservation.
        let needle = [0x42u8, 0xde, 0xad];
        assert!(
            bytes.windows(needle.len()).any(|w| w == needle),
            "payload was not encoded as a byte string"
        );
    }

    #[test]
    fn an_unknown_object_keeps_its_exact_bytes() {
        let original = Envelope::new(
            "feature.loft",
            7,
            vec!["future.loft.v1".to_owned()],
            vec![1, 2, 3, 4],
        )
        .to_bytes()
        .expect("serialises");

        let envelope = Envelope::from_bytes(&original).expect("header is readable");
        let unknown = UnknownObject::new(envelope, original.clone());

        assert_eq!(unknown.type_name, "feature.loft");
        assert_eq!(unknown.schema_version, 7);
        assert_eq!(unknown.raw_envelope(), original.as_slice());
    }

    #[test]
    fn a_truncated_envelope_is_an_input_error() {
        let err = Envelope::from_bytes(&[0xa4, 0x64]).expect_err("truncated CBOR must fail");
        assert_eq!(err.kind(), ferritecad_types::ErrorKind::Input);
    }
}
