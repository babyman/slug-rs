//! The value JSON form defined in `docs/reference/value-json.md`.

use alloc::{string::String, vec::Vec};
use core::fmt::Write;

use crate::Value;

/// Levels below which a nested list, struct, or map is still encoded; see the
/// reference's depth limit. A `$truncated` marker takes the one level above.
const MAX_CONTAINER_DEPTH: usize = 95;

impl Value {
    /// Encodes this value in Slug's value JSON form.
    ///
    /// Encoding never fails: values nested past the depth limit become
    /// `$truncated` markers, and values without data become `$opaque`.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut output = String::new();
        encode(self, 0, &mut output);
        output
    }
}

fn encode(value: &Value, depth: usize, output: &mut String) {
    match value {
        Value::Nil => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Int(value) => write_display(output, value),
        Value::Float(value) if value.is_finite() => encode_float(*value, output),
        Value::Str(value) => encode_string(value, output),
        Value::Bytes(bytes) => {
            output.push_str("{\"$bytes\":\"");
            for byte in bytes.iter() {
                write_display(output, format_args!("{byte:02x}"));
            }
            output.push_str("\"}");
        }
        Value::List(values) => {
            if truncated(depth, 1, "list", output) {
                return;
            }
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                encode(value, depth + 1, output);
            }
            output.push(']');
        }
        Value::Map(entries) => {
            if truncated(depth, 3, "map", output) {
                return;
            }
            let mut encoded: Vec<(String, &Value)> = entries
                .iter()
                .map(|(key, value)| {
                    let mut text = String::new();
                    encode(key, depth + 3, &mut text);
                    (text, value)
                })
                .collect();
            encoded.sort_unstable_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
            output.push_str("{\"$map\":[");
            for (index, (key, value)) in encoded.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                output.push('[');
                output.push_str(key);
                output.push(',');
                encode(value, depth + 3, output);
                output.push(']');
            }
            output.push_str("]}");
        }
        Value::Struct(value) => {
            if truncated(depth, 2, "struct", output) {
                return;
            }
            output.push_str("{\"$struct\":{");
            for (index, (field, value)) in value.schema.fields.iter().zip(&value.values).enumerate()
            {
                if index > 0 {
                    output.push(',');
                }
                encode_string(&field.name, output);
                output.push(':');
                encode(value, depth + 2, output);
            }
            output.push_str("}}");
        }
        Value::Enum(value) => {
            output.push_str("{\"$enum\":");
            encode_string(&value.name, output);
            output.push_str(",\"case\":");
            encode_string(&value.case, output);
            output.push('}');
        }
        Value::Binding { cell, .. } => match cell.try_borrow() {
            Ok(value) => encode(&value, depth, output),
            Err(_) => encode_opaque("uninitialized", value, output),
        },
        Value::Float(_) => encode_opaque("num", value, output),
        Value::Uninitialized => encode_opaque("uninitialized", value, output),
        Value::StructSchema(_) => encode_opaque("schema", value, output),
        Value::Channel(_) => encode_opaque("chan", value, output),
        #[cfg(feature = "concurrency")]
        Value::Task(_) => encode_opaque("task", value, output),
        Value::NativeResource(_) => encode_opaque("native_resource", value, output),
        Value::Closure(_)
        | Value::Native(_)
        | Value::DeclaredNative { .. }
        | Value::Builtin(_)
        | Value::Overloads(_) => encode_opaque("fn", value, output),
    }
}

fn truncated(depth: usize, levels: usize, kind: &str, output: &mut String) -> bool {
    if depth + levels <= MAX_CONTAINER_DEPTH {
        return false;
    }
    output.push_str("{\"$truncated\":\"");
    output.push_str(kind);
    output.push_str("\"}");
    true
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::float_cmp,
    reason = "the range and integral-value checks make the conversion exact"
)]
fn encode_float(value: f64, output: &mut String) {
    // Integral floats share the map-key canonical form with integers, so
    // `1.0` and `-0.0` encode as `1` and `0`.
    if value >= i64::MIN as f64 && value < -(i64::MIN as f64) && (value as i64) as f64 == value {
        write_display(output, value as i64);
        return;
    }
    let magnitude = if value < 0.0 { -value } else { value };
    if (1e-6..1e21).contains(&magnitude) {
        write_display(output, value);
    } else {
        write_display(output, format_args!("{value:e}"));
    }
}

fn encode_string(value: &str, output: &mut String) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{8}' => output.push_str("\\b"),
            '\t' => output.push_str("\\t"),
            '\n' => output.push_str("\\n"),
            '\u{c}' => output.push_str("\\f"),
            '\r' => output.push_str("\\r"),
            character if character < ' ' => {
                write_display(output, format_args!("\\u{:04x}", u32::from(character)));
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

fn encode_opaque(kind: &str, value: &Value, output: &mut String) {
    output.push_str("{\"$opaque\":\"");
    output.push_str(kind);
    output.push_str("\",\"display\":");
    let mut display = String::new();
    write_display(&mut display, format_args!("{value:?}"));
    encode_string(&display, output);
    output.push('}');
}

fn write_display(output: &mut String, value: impl core::fmt::Display) {
    write!(output, "{value}").expect("writing into String cannot fail");
}
