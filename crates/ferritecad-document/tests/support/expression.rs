// SPDX-License-Identifier: MIT
use ferritecad_document::Expression;

pub fn unchecked(value: f64) -> Expression {
    let wire = ciborium::value::Value::Map(vec![
        ("source".into(), "invalid".into()),
        ("value".into(), ciborium::value::Value::Float(value)),
    ]);
    let mut bytes = Vec::new();
    ciborium::into_writer(&wire, &mut bytes).expect("encode test payload");
    ciborium::from_reader(bytes.as_slice()).expect("decode test payload")
}
