// SPDX-License-Identifier: MIT
//! The §22B-1e3b identity wire contract: how a durable identity is spelled in
//! a file, and nothing else.
//!
//! Two functions of one value each. Everything this module can see is the
//! identity it was handed: it cannot reach a node, a definition, an ordinal, a
//! parent, a display name, a transform, a material or a scene, so a value it
//! produces cannot depend on any of them. That containment is checked
//! mechanically by [`tools/check-export-boundary.sh`], because none of those
//! dependencies would be a compile error.
//!
//! # Version 1
//!
//! ```text
//! value       = "fcad1" ":" domain ":" kind *( ":" field )
//! domain      = "def" / "occ"
//! kind        = "object" / "source" / "place"
//! field       = *( unreserved / pct-encoded )
//! unreserved  = ALPHA / DIGIT / "-" / "." / "_" / "~"
//! pct-encoded = "%" UPPER-HEX UPPER-HEX          ; one byte of UTF-8
//!
//! definition, native body   fcad1:def:object:<object id>
//! definition, imported      fcad1:def:source:<source id>:<definition key>
//! placement,  native body   fcad1:occ:object:<object id>
//! placement,  imported      fcad1:occ:place:<occurrence id>
//! ```
//!
//! The version is the first field of the value rather than a property of its
//! own, so every value says which contract it was written under even when it
//! is read alone, copied out of a file or pasted into a bug report.
//!
//! # Two domains that never meet
//!
//! A definition identity and a placement identity are different questions, and
//! a reader that joined across them would join a part to a place. They are
//! therefore separate properties whose values carry separate domain tags, and
//! the tag comes before anything a caller supplies. A native body is the case
//! that makes this matter: the same [`ObjectId`] is both its definition
//! identity and its placement identity, because a body is placed exactly once —
//! and the two values are still different strings, so nothing can compare them
//! and conclude they are one thing.
//!
//! # Injective, so there is no collision policy
//!
//! Every field is percent-escaped down to an unreserved alphabet before it is
//! joined with `:`. No field can therefore contain a `:`, the separator is
//! unambiguous, and the whole value can be split and unescaped back to exactly
//! what went in. Nothing is truncated and nothing is hashed, so two different
//! identities cannot produce one value and there is no collision to have a
//! policy about. §22B-1e2a measured what the alternative costs: two durable
//! identities deliberately collided onto one token merged two materials with
//! different colours into one object, with one warning and no refusal.
//!
//! # Absence is the only way to say "never recorded"
//!
//! There is no spelling inside a value for an identity a document does not
//! have, and no `unknown` kind. A layout written before identities simply
//! carries no property, because a value that could say "none" would be a value
//! invented for a document that recorded nothing — and the point of
//! [`ExportOccurrence::Unrecorded`] is that no such value exists.

use crate::scene::{ExportDefinitionIdentity, ExportOccurrence};

/// The property a definition identity is written to.
pub(super) const DEFINITION_PROPERTY: &str = "FerriteCADDefinitionId";
/// The property a placement identity is written to.
pub(super) const OCCURRENCE_PROPERTY: &str = "FerriteCADOccurrenceId";

/// The wire contract version every value begins with.
const VERSION: &str = "fcad1";
/// The two domains. Disjoint by construction: no value of one can be read as a
/// value of the other, whatever its fields hold.
const DEFINITION_DOMAIN: &str = "def";
const OCCURRENCE_DOMAIN: &str = "occ";
/// What kind of thing the identity names inside its domain.
const OBJECT_KIND: &str = "object";
const SOURCE_KIND: &str = "source";
const PLACE_KIND: &str = "place";

/// How a definition identity is spelled, or `None` when the document recorded
/// none.
///
/// `None` is the whole of what a legacy layout gets. It is not an empty value
/// and not a placeholder: the caller writes no property at all.
pub(super) fn definition(identity: &ExportDefinitionIdentity) -> Option<String> {
    match identity {
        ExportDefinitionIdentity::Object(object) => Some(value(
            DEFINITION_DOMAIN,
            OBJECT_KIND,
            &[&object.to_string()],
        )),
        ExportDefinitionIdentity::Source {
            source,
            definition_key,
        } => Some(value(
            DEFINITION_DOMAIN,
            SOURCE_KIND,
            &[&source.to_string(), definition_key],
        )),
        ExportDefinitionIdentity::Unrecorded => None,
    }
}

/// How a placement identity is spelled, or `None` when the document recorded
/// none.
pub(super) fn occurrence(occurrence: &ExportOccurrence) -> Option<String> {
    match occurrence {
        ExportOccurrence::Object(object) => Some(value(
            OCCURRENCE_DOMAIN,
            OBJECT_KIND,
            &[&object.to_string()],
        )),
        ExportOccurrence::Occurrence(place) => {
            Some(value(OCCURRENCE_DOMAIN, PLACE_KIND, &[&place.to_string()]))
        }
        ExportOccurrence::Unrecorded => None,
    }
}

/// Joins the version, the domain, the kind and the escaped fields.
fn value(domain: &str, kind: &str, fields: &[&str]) -> String {
    let mut out = String::with_capacity(VERSION.len() + domain.len() + kind.len() + 48);
    out.push_str(VERSION);
    out.push(':');
    out.push_str(domain);
    out.push(':');
    out.push_str(kind);
    for field in fields {
        out.push(':');
        escape_into(field, &mut out);
    }
    out
}

/// Percent-escapes one field down to the unreserved alphabet.
///
/// Over the UTF-8 bytes of the text rather than over its characters, so every
/// script is expressible and the result is plain ASCII. Uppercase hexadecimal
/// so each input has exactly one encoding: `%3a` and `%3A` would be two
/// spellings of one identity, and canonical means one.
///
/// No normalisation is applied. Two strings that differ only in Unicode
/// normal form are two different keys here, exactly as §22B-1e2a measured them
/// to be two different objects in the target program; silently folding them
/// would merge two definitions a source deliberately kept apart.
fn escape_into(field: &str, out: &mut String) {
    for byte in field.bytes() {
        if is_unreserved(byte) {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[usize::from(byte >> 4)]);
            out.push(HEX[usize::from(byte & 0x0f)]);
        }
    }
}

const HEX: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F',
];

const fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

#[cfg(test)]
#[allow(clippy::panic, reason = "a gate that cannot fail is not a gate")]
mod tests {
    use super::*;
    use ferritecad_types::{ImportedSourceId, ObjectId, OccurrenceId};

    /// Fixed identifiers, spelled out. A test that minted its own would assert
    /// against a value it could not name, and the golden vectors below would
    /// be different on every run.
    const SOURCE: &str = "019ffc72-1e3b-7000-8000-0000000000a1";
    const OBJECT: &str = "019ffc72-1e3b-7000-8000-0000000000b2";
    const PLACE: &str = "019ffc72-1e3b-7000-8000-0000000000c3";

    fn source() -> ImportedSourceId {
        SOURCE.parse().expect("a fixed UUIDv7")
    }

    fn object() -> ObjectId {
        OBJECT.parse().expect("a fixed UUIDv7")
    }

    fn place() -> OccurrenceId {
        PLACE.parse().expect("a fixed UUIDv7")
    }

    fn escape(field: &str) -> String {
        let mut out = String::new();
        escape_into(field, &mut out);
        out
    }

    /// The golden vectors of the escaping rule. Every one of them is a case
    /// that would make the separator ambiguous, the encoding non-canonical or
    /// the mapping non-injective if it were got wrong.
    #[test]
    fn the_escaping_rule_has_exactly_these_golden_vectors() {
        for (input, expected) in [
            ("step.product_definition#42", "step.product_definition%2342"),
            ("", ""),
            ("~-._", "~-._"),
            // The separator itself, and the escape character itself.
            ("a:b", "a%3Ab"),
            ("100%", "100%25"),
            ("%3A", "%253A"),
            // Whitespace, which a name may carry and a value may not.
            ("a b", "a%20b"),
            ("a\tb", "a%09b"),
            ("a\nb", "a%0Ab"),
            // One two-byte, one three-byte and one four-byte code point.
            ("\u{03a9}", "%CE%A9"),
            ("\u{2014}", "%E2%80%94"),
            ("\u{1f9f2}", "%F0%9F%A7%B2"),
            // Composed and decomposed spellings of one grapheme stay two
            // different keys, because the source that wrote them meant two.
            ("\u{00e9}", "%C3%A9"),
            ("e\u{0301}", "e%CC%81"),
        ] {
            assert_eq!(escape(input), expected, "escaping {input:?}");
        }
    }

    /// And the golden vectors of the whole values, domain tags included.
    #[test]
    fn the_wire_contract_has_exactly_these_golden_values() {
        assert_eq!(
            definition(&ExportDefinitionIdentity::Source {
                source: source(),
                definition_key: "step.product_definition#42".to_owned(),
            })
            .expect("a recorded identity"),
            format!("fcad1:def:source:{SOURCE}:step.product_definition%2342")
        );
        assert_eq!(
            definition(&ExportDefinitionIdentity::Object(object())).expect("a recorded identity"),
            format!("fcad1:def:object:{OBJECT}")
        );
        assert_eq!(
            occurrence(&ExportOccurrence::Occurrence(place())).expect("a recorded identity"),
            format!("fcad1:occ:place:{PLACE}")
        );
        assert_eq!(
            occurrence(&ExportOccurrence::Object(object())).expect("a recorded identity"),
            format!("fcad1:occ:object:{OBJECT}")
        );
    }

    /// A layout that recorded nothing gets no value, in either domain.
    #[test]
    fn an_unrecorded_identity_has_no_spelling_at_all() {
        assert_eq!(definition(&ExportDefinitionIdentity::Unrecorded), None);
        assert_eq!(occurrence(&ExportOccurrence::Unrecorded), None);
    }

    /// The same object identifier is both halves of a native body's identity,
    /// and the two values are still different strings.
    #[test]
    fn one_object_gives_two_values_that_cannot_be_confused() {
        let definition = definition(&ExportDefinitionIdentity::Object(object()))
            .expect("a body has a definition identity");
        let placement =
            occurrence(&ExportOccurrence::Object(object())).expect("a body has a placement");
        assert_ne!(definition, placement);
        assert!(definition.starts_with("fcad1:def:"));
        assert!(placement.starts_with("fcad1:occ:"));
    }

    /// Two sources that gave one definition the same local key stay two
    /// identities. This is the `ambiguous_join` §22B-1e2a measured, expressed
    /// as a refusal to produce one value.
    #[test]
    fn two_sources_with_one_local_key_are_two_values() {
        let other: ImportedSourceId = "019ffc72-1e3b-7000-8000-0000000000a2"
            .parse()
            .expect("a fixed UUIDv7");
        let key = "step.product_definition#42".to_owned();
        let first = definition(&ExportDefinitionIdentity::Source {
            source: source(),
            definition_key: key.clone(),
        });
        let second = definition(&ExportDefinitionIdentity::Source {
            source: other,
            definition_key: key,
        });
        assert_ne!(first, second);
    }

    /// Every produced value is ASCII and carries nothing that could be read as
    /// a separator, whatever went into it.
    #[test]
    fn no_field_can_smuggle_a_separator_into_a_value() {
        for key in [
            "a:b",
            "fcad1:occ:place:019ffc72-1e3b-7000-8000-0000000000c3",
            "%",
            "\u{2014}:\u{2014}",
            "\n\t\r",
        ] {
            let produced = definition(&ExportDefinitionIdentity::Source {
                source: source(),
                definition_key: key.to_owned(),
            })
            .expect("a recorded identity");
            assert!(produced.is_ascii(), "{produced} is not ASCII");
            // Version, domain, kind, source, key: five parts and no more,
            // however many colons the key was written with.
            assert_eq!(produced.split(':').count(), 5, "{produced}");
        }
    }

    /// Different keys give different values, including keys that differ only
    /// in a character the escaping touches.
    #[test]
    fn the_encoding_is_injective_over_the_keys_it_is_given() {
        let keys = [
            "a:b", "a%3Ab", "a%253Ab", "ab", "a b", "a%20b", "", "%", "%25",
        ];
        let mut produced: Vec<String> = keys
            .iter()
            .map(|key| {
                definition(&ExportDefinitionIdentity::Source {
                    source: source(),
                    definition_key: (*key).to_owned(),
                })
                .expect("a recorded identity")
            })
            .collect();
        produced.sort();
        let before = produced.len();
        produced.dedup();
        assert_eq!(produced.len(), before, "two keys produced one value");
    }
}
