// SPDX-License-Identifier: MIT
use ferritecad_types::{CadError, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// The object type and its CBOR payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(with = "serde_bytes")]
    pub payload: Vec<u8>,
}

impl Envelope {
    pub fn new(type_name: impl Into<String>, payload: Vec<u8>) -> Self {
        Self {
            type_name: type_name.into(),
            payload,
        }
    }

    pub fn encode<T: Serialize>(type_name: impl Into<String>, payload: &T) -> Result<Self> {
        let mut bytes = Vec::new();
        ciborium::into_writer(payload, &mut bytes)
            .map_err(|e| CadError::input_because("encoding object payload", e))?;
        Ok(Self::new(type_name, bytes))
    }

    pub fn decode<T: DeserializeOwned>(&self) -> Result<T> {
        ciborium::from_reader(self.payload.as_slice())
            .map_err(|e| CadError::input_because(format!("decoding {} payload", self.type_name), e))
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        ciborium::into_writer(self, &mut bytes)
            .map_err(|e| CadError::input_because("encoding object envelope", e))?;
        Ok(bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        ciborium::from_reader(bytes)
            .map_err(|e| CadError::input_because("decoding object envelope", e))
    }
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
        let envelope = Envelope::encode("feature.extrude", &sample).expect("encodes");
        let bytes = envelope.to_bytes().expect("serialises");
        let read = Envelope::from_bytes(&bytes).expect("deserialises");
        assert_eq!(read, envelope);
        assert_eq!(read.decode::<Sample>().expect("decodes"), sample);
    }

    #[test]
    fn payload_is_a_cbor_byte_string_not_an_array() {
        let bytes = Envelope::new("t", vec![0xde, 0xad])
            .to_bytes()
            .expect("serialises");
        assert!(bytes.windows(3).any(|window| window == [0x42, 0xde, 0xad]));
    }

    #[test]
    fn a_truncated_envelope_is_an_input_error() {
        let error = Envelope::from_bytes(&[0xa4, 0x64]).expect_err("truncated CBOR");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input);
    }
}
