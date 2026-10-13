//! The guard on every string endexd puts on D-Bus. A string with a NUL (U+0000) makes the whole
//! message invalid, and the bus disconnects the sender. Each NUL becomes U+FFFD instead: one
//! character for one, so highlight spans (counted in characters) still line up.

use std::collections::HashMap;

use zbus::zvariant::{Array, Dict, OwnedValue, Signature, StructureBuilder, Value};

/// Inserts `value` into a reply dictionary, with every string in it made safe.
pub fn put<'a>(d: &mut HashMap<String, OwnedValue>, key: &str, value: impl Into<Value<'a>>) {
    let value = OwnedValue::try_from(safe(value.into())).expect("no file descriptors");
    d.insert(text(key.to_owned()), value);
}

pub fn text(text: String) -> String {
    if text.contains('\0') {
        text.replace('\0', "\u{FFFD}")
    } else {
        text
    }
}

fn safe<'a>(value: Value<'a>) -> Value<'a> {
    let copy = |v: &Value<'a>| safe(v.try_clone().expect("no file descriptors"));
    match value {
        Value::Str(s) if s.contains('\0') => Value::from(text(s.to_string())),
        Value::Value(inner) => Value::Value(Box::new(safe(*inner))),
        Value::Array(array) => {
            let mut out = Array::new(array.element_signature());
            for item in array.iter() {
                out.append(copy(item)).expect("same signature");
            }
            out.into()
        }
        Value::Dict(dict) => {
            let Signature::Dict { key, value } = dict.signature() else {
                unreachable!("a dict signature")
            };
            let mut out = Dict::new(key, value);
            for (k, v) in dict.iter() {
                out.append(copy(k), copy(v)).expect("same signature");
            }
            out.into()
        }
        Value::Structure(s) => {
            let fields = s.into_fields().into_iter().map(safe);
            let builder = fields.fold(StructureBuilder::new(), |b, f| b.append_field(f));
            builder.build().expect("a structure has fields").into()
        }
        other => other,
    }
}
