use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum ParseErrorKind {
    UnexpectedCloseBrace,
    UnexpectedOpenBrace,
    UnexpectedEof,
    /// A key that is not followed by a value or a `{` block.
    MissingValue { key: String },
    UnterminatedString,
    BadCondition(String),
    InvalidToken,
}

/// Syntax error with a 1-based line/column position.
#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: ", self.line, self.col)?;
        match &self.kind {
            ParseErrorKind::UnexpectedCloseBrace => write!(f, "unexpected '}}'"),
            ParseErrorKind::UnexpectedOpenBrace => write!(f, "unexpected '{{'"),
            ParseErrorKind::UnexpectedEof => write!(f, "unexpected end of file (unclosed block)"),
            ParseErrorKind::MissingValue { key } => write!(f, "key {key:?} has no value"),
            ParseErrorKind::UnterminatedString => write!(f, "unterminated string"),
            ParseErrorKind::BadCondition(c) => write!(f, "bad conditional {c}"),
            ParseErrorKind::InvalidToken => write!(f, "invalid token"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Converts byte offsets to 1-based (line, column).
pub(crate) fn position(src: &str, offset: usize) -> (usize, usize) {
    let before = &src[..offset.min(src.len())];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, col)
}
