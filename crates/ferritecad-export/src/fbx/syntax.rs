// SPDX-License-Identifier: MIT
//! How FBX 7.4 ASCII is spelled, in one place.
//!
//! Two rules live here and nowhere else: how a string becomes a quoted FBX
//! token, and how a number becomes text. Both exist once because both are
//! places where two spellings of one value would make the bytes stop being a
//! function of the scene.
//!
//! # The escaping rule is not backslashes
//!
//! FBX ASCII quotes strings and escapes three characters with XML-like
//! entities: `"` is `&quot;`, a carriage return is `&cr;` and a line feed is
//! `&lf;`. A backslash is an ordinary character. There is no escape for `&`
//! itself, so a name that already contains one of those three spellings
//! cannot be written and read back as itself; such a name is refused rather
//! than silently altered. Control characters other than tab have no
//! representation at all and are refused for the same reason.

use std::io::Write;

use ferritecad_types::{CadError, Result};

/// How many array values one line carries.
///
/// A mesh of a million triangles would otherwise be one line of a hundred
/// megabytes, which some readers and most editors will not accept. The
/// number is fixed rather than chosen from the data, so where the breaks fall
/// is a property of the format and not of the scene.
const VALUES_PER_LINE: usize = 12;

/// The three sequences a reader turns back into something else.
const ENTITIES: [&str; 3] = ["&quot;", "&cr;", "&lf;"];

/// One property of one node.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Value<'a> {
    Int(i64),
    Double(f64),
    /// Written quoted and escaped.
    Text(&'a str),
}

impl Value<'_> {
    pub(crate) fn bool(value: bool) -> Self {
        Self::Int(i64::from(value))
    }
}

/// Writes FBX ASCII to a sink, counting what it produced.
#[derive(Debug)]
pub(crate) struct Ascii<W: Write> {
    out: W,
    bytes: u64,
    depth: usize,
}

impl<W: Write> Ascii<W> {
    pub(crate) fn new(out: W) -> Self {
        Self {
            out,
            bytes: 0,
            depth: 0,
        }
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.bytes
    }

    fn put(&mut self, text: &str) -> Result<()> {
        self.out
            .write_all(text.as_bytes())
            .map_err(|source| CadError::io("the FBX could not be written", source))?;
        self.bytes += text.len() as u64;
        Ok(())
    }

    fn indent(&mut self) -> Result<()> {
        for _ in 0..self.depth {
            self.put("\t")?;
        }
        Ok(())
    }

    fn properties(&mut self, props: &[Value<'_>]) -> Result<()> {
        for (position, value) in props.iter().enumerate() {
            self.put(if position == 0 { " " } else { ", " })?;
            match value {
                Value::Int(number) => {
                    let text = number.to_string();
                    self.put(&text)?;
                }
                Value::Double(number) => {
                    let text = double(*number)?;
                    self.put(&text)?;
                }
                Value::Text(text) => {
                    let escaped = escape(text)?;
                    self.put("\"")?;
                    self.put(&escaped)?;
                    self.put("\"")?;
                }
            }
        }
        Ok(())
    }

    /// A comment line, which carries no data.
    pub(crate) fn comment(&mut self, text: &str) -> Result<()> {
        self.put("; ")?;
        self.put(text)?;
        self.put("\n")
    }

    pub(crate) fn blank(&mut self) -> Result<()> {
        self.put("\n")
    }

    /// A node with children.
    pub(crate) fn open(&mut self, name: &str, props: &[Value<'_>]) -> Result<()> {
        self.indent()?;
        self.put(name)?;
        self.put(":")?;
        self.properties(props)?;
        self.put(" {\n")?;
        self.depth += 1;
        Ok(())
    }

    pub(crate) fn close(&mut self) -> Result<()> {
        self.depth = self.depth.saturating_sub(1);
        self.indent()?;
        self.put("}\n")
    }

    /// A node with no children.
    pub(crate) fn leaf(&mut self, name: &str, props: &[Value<'_>]) -> Result<()> {
        self.indent()?;
        self.put(name)?;
        self.put(":")?;
        self.properties(props)?;
        self.put("\n")
    }

    /// One entry of a `Properties70` block.
    pub(crate) fn property(
        &mut self,
        name: &str,
        kind: &str,
        label: &str,
        flags: &str,
        values: &[Value<'_>],
    ) -> Result<()> {
        let mut props = vec![
            Value::Text(name),
            Value::Text(kind),
            Value::Text(label),
            Value::Text(flags),
        ];
        props.extend_from_slice(values);
        self.leaf("P", &props)
    }

    /// An array node, written as the format's `*count { a: ... }`.
    ///
    /// The values arrive lazily, because a mesh of a million triangles has
    /// several million of them and holding the text of all of them at once
    /// would cost more than the file does. The declared count is checked
    /// against what was actually produced, so a length and a payload cannot
    /// disagree in the file.
    pub(crate) fn array<I>(&mut self, name: &str, count: usize, values: I) -> Result<()>
    where
        I: IntoIterator<Item = Result<String>>,
    {
        self.indent()?;
        self.put(name)?;
        self.put(": *")?;
        self.put(&count.to_string())?;
        self.put(" {\n")?;
        self.depth += 1;

        let mut written = 0usize;
        for value in values {
            let text = value?;
            if written == 0 {
                self.indent()?;
                self.put("a: ")?;
            } else if written.is_multiple_of(VALUES_PER_LINE) {
                self.put("\n")?;
                self.indent()?;
                self.put(",")?;
            } else {
                self.put(",")?;
            }
            self.put(&text)?;
            written += 1;
        }
        if written != count {
            return Err(CadError::input(format!(
                "an FBX array declared {count} values and produced {written}"
            )));
        }
        self.put("\n")?;
        self.depth -= 1;
        self.indent()?;
        self.put("}\n")
    }
}

/// One canonical text for one double.
///
/// Refuses what no reader can use, gives zero a single representation, and
/// otherwise writes the shortest text that reads back as the same value. The
/// shortest round-trip form is computed by integer arithmetic rather than by
/// the C library, so two platforms writing the same value write the same
/// characters.
pub(crate) fn double(value: f64) -> Result<String> {
    if !value.is_finite() {
        return Err(CadError::unsupported(format!(
            "the export produced {value}, which is not a number any file can record"
        )));
    }
    // `-0.0 == 0.0`, so two equal scenes must not differ by a sign nobody can
    // see.
    let canonical = if value == 0.0 { 0.0 } else { value };
    Ok(format!("{canonical:?}"))
}

/// One canonical text for one integer.
pub(crate) fn integer(value: i64) -> Result<String> {
    Ok(value.to_string())
}

/// Escapes one string, or refuses one this format cannot spell.
pub(crate) fn escape(value: &str) -> Result<String> {
    for entity in ENTITIES {
        if value.contains(entity) {
            return Err(CadError::unsupported(format!(
                "the name {value:?} contains {entity}, which a reader turns into another \
                 character, so writing it would change the name rather than record it"
            )));
        }
    }

    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => out.push_str("&quot;"),
            '\r' => out.push_str("&cr;"),
            '\n' => out.push_str("&lf;"),
            // Tab survives quoting unchanged; the other control characters
            // have no spelling at all in this format.
            '\t' => out.push('\t'),
            _ if character.is_control() => {
                return Err(CadError::unsupported(format!(
                    "the name {value:?} contains the control character U+{:04X}, which FBX ASCII \
                     cannot represent",
                    u32::from(character)
                )));
            }
            _ => out.push(character),
        }
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::panic, reason = "a gate that cannot fail is not a gate")]
mod tests {
    use super::*;
    use ferritecad_types::ErrorKind;

    fn escaped(value: &str) -> String {
        escape(value).expect("this name is representable")
    }

    /// The reader's rule, applied backwards, so the gate checks a round trip
    /// rather than a spelling.
    fn unescape(value: &str) -> String {
        value
            .replace("&quot;", "\"")
            .replace("&cr;", "\r")
            .replace("&lf;", "\n")
    }

    #[test]
    fn the_three_escaped_characters_survive_a_round_trip() {
        for name in [
            "",
            "plain",
            "a \"quoted\" name",
            r"back\slash",
            "tab\there",
            "line\nbreak",
            "carriage\rreturn",
            "Кириллица и юникод — ok",
            "everything \" \\ \t \r \n Ω",
            "an & ampersand",
            "&q partial",
            "&cr almost",
        ] {
            let written = escaped(name);
            // Both halves are the property. What is written must be something
            // a quoted FBX string can hold at all: a raw quote ends the
            // string, and a raw newline ends the line the string is on. And
            // what a reader makes of it must be the name it started as.
            // Checking only the second half would pass on a writer that
            // escaped nothing, because unescaping nothing is also nothing.
            assert!(
                !written.contains('"'),
                "{name:?} left a raw quote in the file: {written:?}"
            );
            assert!(
                !written.contains('\r'),
                "{name:?} left a raw carriage return in the file: {written:?}"
            );
            assert!(
                !written.contains('\n'),
                "{name:?} left a raw line feed in the file: {written:?}"
            );
            assert_eq!(unescape(&written), name, "{name:?} did not survive");
        }
    }

    #[test]
    fn a_backslash_is_an_ordinary_character_and_a_quote_is_not() {
        assert_eq!(escaped(r"c:\path"), r"c:\path");
        assert_eq!(escaped("say \"hi\""), "say &quot;hi&quot;");
        assert_eq!(escaped("one\ntwo"), "one&lf;two");
        assert_eq!(escaped("one\rtwo"), "one&cr;two");
        assert_eq!(escaped(""), "");
    }

    #[test]
    fn a_name_this_format_cannot_spell_is_refused() {
        for name in [
            "already &quot; escaped",
            "already &cr; escaped",
            "already &lf; escaped",
            "a \u{7} bell",
            "a \u{0} nul",
            "a \u{1b} escape",
        ] {
            let Err(error) = escape(name) else {
                panic!("{name:?} was accepted");
            };
            assert_eq!(
                error.kind(),
                ErrorKind::Unsupported,
                "{name:?} was refused as something other than unsupported"
            );
        }
    }

    #[test]
    fn zero_has_one_spelling_and_a_non_number_has_none() {
        assert_eq!(double(0.0).expect("zero"), "0.0");
        assert_eq!(double(-0.0).expect("negative zero"), "0.0");
        assert_eq!(double(1.0).expect("one"), "1.0");
        assert_eq!(double(-2.0).expect("minus two"), "-2.0");
        assert_eq!(double(0.001).expect("a millimetre in metres"), "0.001");
        assert_eq!(double(100.0).expect("the unit scale"), "100.0");
        for refused in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                double(refused).expect_err("not a number").kind(),
                ErrorKind::Unsupported
            );
        }
    }

    #[test]
    fn every_written_double_reads_back_as_itself() {
        for value in [
            1.0,
            -1.0,
            0.1,
            1.0 / 3.0,
            1.234_567_890_123_456_7e-9,
            9.876_543_210_987_654e11,
            f64::MIN_POSITIVE,
            f64::MAX,
        ] {
            let text = double(value).expect("finite");
            let read: f64 = text.parse().expect("the writer spells a number");
            assert_eq!(read, value, "{text} is not {value}");
        }
    }

    #[test]
    fn an_array_declares_the_length_it_writes_and_wraps_its_lines() {
        let mut out = Vec::new();
        let mut ascii = Ascii::new(&mut out);
        ascii
            .array("Values", 25, (0..25).map(|value| double(f64::from(value))))
            .expect("writes");
        let text = String::from_utf8(out).expect("UTF-8");
        assert!(text.starts_with("Values: *25 {\n"));
        let payload: Vec<&str> = text
            .lines()
            .filter(|line| {
                line.trim_start().starts_with("a:") || line.trim_start().starts_with(',')
            })
            .collect();
        assert_eq!(payload.len(), 3, "25 values in lines of {VALUES_PER_LINE}");
        assert_eq!(
            text.matches(',').count(),
            24,
            "one separator between values"
        );

        // A payload that does not match the length it declared is refused
        // rather than written.
        let mut out = Vec::new();
        let mut ascii = Ascii::new(&mut out);
        assert!(
            ascii
                .array("Values", 4, (0..3).map(|value| double(f64::from(value))))
                .is_err(),
            "a declared length and its payload must agree"
        );
    }

    #[test]
    fn the_writer_counts_exactly_what_it_produced() {
        let mut out = Vec::new();
        let mut ascii = Ascii::new(&mut out);
        ascii.comment("a comment").expect("writes");
        ascii.open("Objects", &[]).expect("writes");
        ascii
            .leaf(
                "Model",
                &[Value::Int(7), Value::Text("Model::x"), Value::Double(1.5)],
            )
            .expect("writes");
        ascii.close().expect("writes");
        let counted = ascii.bytes();
        assert_eq!(counted, out.len() as u64);
        assert_eq!(
            String::from_utf8(out).expect("UTF-8"),
            "; a comment\nObjects: {\n\tModel: 7, \"Model::x\", 1.5\n}\n"
        );
    }
}
