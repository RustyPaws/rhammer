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

/// Helpers on a list of nodes (keys compare case-insensitively).
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
