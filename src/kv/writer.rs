use crate::kv::node::{Node, Value};
use std::fmt::Write;

/// KeyValues strings are raw, so an embedded quote cannot be escaped; it is replaced.
fn esc(s: &str) -> String {
    s.replace('"', "'")
}

pub fn write_nodes(out: &mut String, nodes: &[Node], depth: usize) {
    let ind = "\t".repeat(depth);
    for n in nodes {
        match &n.value {
            Value::Str(v) => {
                let _ = writeln!(out, "{ind}\"{}\" \"{}\"", esc(&n.key), esc(v));
            }
            Value::Block(c) => {
                let _ = writeln!(out, "{ind}{}", n.key);
                let _ = writeln!(out, "{ind}{{");
                write_nodes(out, c, depth + 1);
                let _ = writeln!(out, "{ind}}}");
            }
        }
    }
}

pub fn to_string(nodes: &[Node]) -> String {
    let mut s = String::new();
    write_nodes(&mut s, nodes, 0);
    s
}
