//! JSON text of stored values, as `serde_json` writes the value the store
//! was built from.

use super::format::Node;
use super::format::StringId;
use super::format::Tables;
use super::format::Value;
use super::format::ValueRef;
use crate::error::StoreDamage;

/// Appends the compact JSON text of `root` to `output`. Walks with an
/// explicit stack, since values nest deeper than is safe to recurse.
pub(crate) fn write_json(tables: Tables<'_>, root: ValueRef, output: &mut Vec<u8>) -> Result<(), StoreDamage> {
    struct Frame<'a> {
        node: Node<'a>,
        object: bool,
        next: usize,
    }

    let mut frames: Vec<Frame<'_>> = Vec::new();
    let mut pending = Some(tables.value(root)?);
    loop {
        if let Some(value) = pending.take() {
            match value {
                Value::Array(node) => {
                    output.push(b'[');
                    frames.push(Frame { node, object: false, next: 0 });
                }
                Value::Object(node) => {
                    output.push(b'{');
                    frames.push(Frame { node, object: true, next: 0 });
                }
                scalar => write_scalar(scalar, output),
            }
        }

        let Some(frame) = frames.last_mut() else {
            return Ok(());
        };
        let step = if frame.object { 2 } else { 1 };
        if frame.next < frame.node.slot_count() {
            if frame.next > 0 {
                output.push(b',');
            }
            if frame.object {
                let key = tables.string(StringId(frame.node.slot(frame.next)))?;
                write_scalar(Value::Str(key), output);
                output.push(b':');
            }
            let child = ValueRef(frame.node.slot(frame.next + step - 1));
            frame.next += step;
            pending = Some(tables.child(frame.node.id(), child)?);
            continue;
        }

        output.push(if frame.object { b'}' } else { b']' });
        frames.pop();
    }
}

fn write_scalar(value: Value<'_>, output: &mut Vec<u8>) {
    // Writing to a Vec cannot fail, and every stored float is finite: the
    // values came from JSON.
    let written = match value {
        Value::Null => serde_json::to_writer(&mut *output, &()),
        Value::Bool(value) => serde_json::to_writer(&mut *output, &value),
        Value::Int(value) => serde_json::to_writer(&mut *output, &value),
        Value::Float(value) => serde_json::to_writer(&mut *output, &value),
        Value::Str(value) => serde_json::to_writer(&mut *output, value),
        Value::Array(_) | Value::Object(_) => unreachable!("nodes are written by write_json"),
    };
    written.expect("JSON scalars serialize into a Vec");
}
