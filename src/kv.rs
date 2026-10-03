//! Valve KeyValues (VDF) parser/writer that preserves order and duplicate keys.
//! Used for VMF, gameinfo.txt and VMT files.

use std::fmt::Write;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Block(Vec<Node>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub key: String,
    pub value: Value,
}

impl Node {
    pub fn str(key: impl Into<String>, v: impl Into<String>) -> Node {
        Node { key: key.into(), value: Value::Str(v.into()) }
    }
    pub fn block(key: impl Into<String>, children: Vec<Node>) -> Node {
        Node { key: key.into(), value: Value::Block(children) }
    }
    pub fn as_str(&self) -> Option<&str> {
        match &self.value {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn children(&self) -> &[Node] {
        match &self.value {
            Value::Block(c) => c,
            _ => &[],
        }
    }
}

/// Helpers on a list of nodes.
pub trait NodeList {
    fn get_str(&self, key: &str) -> Option<&str>;
    fn get_block(&self, key: &str) -> Option<&[Node]>;
    fn blocks<'a>(&'a self, key: &'a str) -> Box<dyn Iterator<Item = &'a Node> + 'a>;
}

impl NodeList for [Node] {
    fn get_str(&self, key: &str) -> Option<&str> {
        self.iter()
            .find(|n| n.key.eq_ignore_ascii_case(key) && matches!(n.value, Value::Str(_)))
            .and_then(|n| n.as_str())
    }
    fn get_block(&self, key: &str) -> Option<&[Node]> {
        self.iter()
            .find(|n| n.key.eq_ignore_ascii_case(key) && matches!(n.value, Value::Block(_)))
            .map(|n| n.children())
    }
    fn blocks<'a>(&'a self, key: &'a str) -> Box<dyn Iterator<Item = &'a Node> + 'a> {
        Box::new(
            self.iter()
                .filter(move |n| n.key.eq_ignore_ascii_case(key) && matches!(n.value, Value::Block(_))),
        )
    }
}

struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
}

#[derive(Debug, PartialEq)]
enum Tok {
    Str(String),
    Open,
    Close,
    Eof,
}

impl<'a> Lexer<'a> {
    fn next(&mut self) -> Tok {
        loop {
            while self.i < self.s.len() && (self.s[self.i] as char).is_ascii_whitespace() {
                self.i += 1;
            }
            if self.i + 1 < self.s.len() && self.s[self.i] == b'/' && self.s[self.i + 1] == b'/' {
                while self.i < self.s.len() && self.s[self.i] != b'\n' {
                    self.i += 1;
                }
                continue;
            }
            break;
        }
        if self.i >= self.s.len() {
            return Tok::Eof;
        }
        match self.s[self.i] {
            b'{' => {
                self.i += 1;
                Tok::Open
            }
            b'}' => {
                self.i += 1;
                Tok::Close
            }
            b'"' => {
                self.i += 1;
                // VMF/VMT strings are raw: backslashes are not escapes.
                let start = self.i;
                while self.i < self.s.len() && self.s[self.i] != b'"' {
                    self.i += 1;
                }
                let tok = Tok::Str(String::from_utf8_lossy(&self.s[start..self.i]).into_owned());
                self.i += 1;
                tok
            }
            _ => {
                let start = self.i;
                while self.i < self.s.len() {
                    let c = self.s[self.i];
                    if c.is_ascii_whitespace() || c == b'{' || c == b'}' || c == b'"' {
                        break;
                    }
                    self.i += 1;
                }
                Tok::Str(String::from_utf8_lossy(&self.s[start..self.i]).into_owned())
            }
        }
    }
}

fn parse_block(lx: &mut Lexer, top: bool) -> anyhow::Result<Vec<Node>> {
    let mut out = Vec::new();
    loop {
        let key = match lx.next() {
            Tok::Str(k) => k,
            Tok::Close => {
                if top {
                    anyhow::bail!("unexpected '}}'");
                }
                return Ok(out);
            }
            Tok::Eof => {
                if top {
                    return Ok(out);
                }
                anyhow::bail!("unexpected end of file");
            }
            Tok::Open => anyhow::bail!("unexpected '{{'"),
        };
        match lx.next() {
            Tok::Str(v) => out.push(Node::str(key, v)),
            Tok::Open => {
                let ch = parse_block(lx, false)?;
                out.push(Node::block(key, ch));
            }
            t => anyhow::bail!("unexpected token after key {key:?}: {t:?}"),
        }
    }
}

pub fn parse(text: &str) -> anyhow::Result<Vec<Node>> {
    let bytes = text.strip_prefix('\u{feff}').unwrap_or(text).as_bytes();
    let mut lx = Lexer { s: bytes, i: 0 };
    parse_block(&mut lx, true)
}

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
