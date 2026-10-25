// SPDX-License-Identifier: MIT
use std::collections::BTreeSet;
use std::fmt::Write;

use ferritecad_kernel::NamingOperation;
use ferritecad_types::{CadError, ObjectId, Result};

use crate::derivation::Derivation;
use crate::generative::{ModelNaming, Name};

impl ModelNaming {
    /// Describes a name and every reachable operand without session-local identities.
    pub fn describe(&self, at: ObjectId, name: Name) -> Result<String> {
        let feature = self
            .feature(at)
            .ok_or_else(|| CadError::topology("description has no feature result"))?;
        if !feature.bindings.contains_key(&name) {
            return Err(CadError::topology("description has no output binding"));
        }
        let operation = self
            .operations
            .get(&at)
            .ok_or_else(|| CadError::topology("description has no feature operation"))?;
        let mut text = format!("feature {at}, operation {operation:?}");
        let mut pending = vec![name];
        let mut seen = BTreeSet::new();
        while let Some(input) = pending.pop() {
            if !seen.insert(input) {
                continue;
            }
            let node = feature
                .derivations
                .get(&input.digest)
                .ok_or_else(|| CadError::topology("description has a missing derivation"))?;
            write!(text, "; {} = ", input.digest).expect("writing to a String cannot fail");
            match &**node {
                Derivation::Curve(owner, curve, plane) => {
                    write!(text, "curve {curve}, producer {owner}")
                        .expect("writing to a String cannot fail");
                    if let Some(plane) = plane {
                        write!(text, ", plane {plane}").expect("writing to a String cannot fail");
                    }
                }
                Derivation::Cap(owner, end, plane) => {
                    let side = if *end { "end" } else { "start" };
                    write!(text, "{side} cap, producer {owner}")
                        .expect("writing to a String cannot fail");
                    if let Some(plane) = plane {
                        write!(text, ", plane {plane}").expect("writing to a String cannot fail");
                    }
                }
                Derivation::Expression {
                    tag,
                    feature,
                    inputs,
                    extra,
                    ..
                } => {
                    text.push_str(tag);
                    if matches!(*tag, "generated" | "modified") {
                        let operation = NamingOperation::from_tag(*extra)
                            .ok_or_else(|| CadError::topology("unknown derivation operation"))?;
                        write!(text, " {operation:?}").expect("writing to a String cannot fail");
                    } else if *tag == "curve-end" {
                        text.push_str(if *extra == 0 { " start" } else { " end" });
                    }
                    if let Some(owner) = feature {
                        write!(text, ", producer {owner}")
                            .expect("writing to a String cannot fail");
                    }
                    text.push_str(", inputs [");
                    for (index, parent) in inputs.iter().enumerate() {
                        if index != 0 {
                            text.push_str(", ");
                        }
                        write!(text, "{} {}", parent.kind, parent.digest)
                            .expect("writing to a String cannot fail");
                    }
                    text.push(']');
                    pending.extend(inputs.iter().rev().copied());
                }
            }
        }
        Ok(text)
    }
}
