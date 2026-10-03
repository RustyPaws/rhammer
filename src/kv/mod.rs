//! Valve KeyValues (VDF) library: lexer, parser and writer that preserve order and
//! duplicate keys. Used for VMF, gameinfo.txt, VMT and rhammer's own config file.

mod error;
mod lexer;
mod node;
mod parser;
mod writer;

pub use error::{ParseError, ParseErrorKind};
pub use node::{Node, NodeList, Value};
pub use parser::{parse, parse_with, Conditions};
pub use writer::{to_string, write_nodes};
