use crate::kv::error::{position, ParseError, ParseErrorKind};
use crate::kv::lexer::Tok;
use crate::kv::node::Node;
use logos::Logos;
use std::ops::Range;

/// Platform symbols considered defined when evaluating `[$NAME]` conditionals.
#[derive(Clone, Debug)]
pub struct Conditions {
    pub defined: Vec<String>,
}

impl Default for Conditions {
    fn default() -> Self {
        Conditions { defined: vec!["WIN32".into(), "WINDOWS".into()] }
    }
}

impl Conditions {
    /// Evaluates `[$A && !$B || $C]` (`&&` binds tighter than `||`). `None` if malformed.
    fn eval(&self, cond: &str) -> Option<bool> {
        let inner = cond.strip_prefix('[')?.strip_suffix(']')?;
        let mut any = false;
        for or_part in inner.split("||") {
            let mut all = true;
            for term in or_part.split("&&") {
                let term = term.trim();
                let (neg, name) = match term.strip_prefix('!') {
                    Some(r) => (true, r.trim()),
                    None => (false, term),
                };
                let name = name.strip_prefix('$')?;
                if name.is_empty() {
                    return None;
                }
                let set = self.defined.iter().any(|d| d.eq_ignore_ascii_case(name));
                all &= set != neg;
            }
            any |= all;
        }
        Some(any)
    }
}

struct Parser<'a> {
    src: &'a str,
    toks: Vec<(Result<Tok<'a>, ()>, Range<usize>)>,
    i: usize,
    cond: &'a Conditions,
}

pub fn parse(text: &str) -> Result<Vec<Node>, ParseError> {
    parse_with(text, &Conditions::default())
}

pub fn parse_with(text: &str, cond: &Conditions) -> Result<Vec<Node>, ParseError> {
    let src = text.strip_prefix('\u{feff}').unwrap_or(text);
    let toks = Tok::lexer(src).spanned().collect();
    let mut p = Parser { src, toks, i: 0, cond };
    p.block(true)
}

impl<'a> Parser<'a> {
    fn err(&self, kind: ParseErrorKind, at: usize) -> ParseError {
        let (line, col) = position(self.src, at);
        ParseError { kind, line, col }
    }

    fn eof_pos(&self) -> usize {
        self.src.len()
    }

    /// Next token with its span; lexer failures become errors here.
    fn next(&mut self) -> Result<Option<(Tok<'a>, Range<usize>)>, ParseError> {
        let Some((t, span)) = self.toks.get(self.i).cloned() else { return Ok(None) };
        self.i += 1;
        match t {
            Ok(Tok::Unterminated) => Err(self.err(ParseErrorKind::UnterminatedString, span.start)),
            Ok(t) => Ok(Some((t, span))),
            Err(()) => Err(self.err(ParseErrorKind::InvalidToken, span.start)),
        }
    }

    /// Consumes an optional `[$COND]`; returns whether the node it guards is kept.
    fn condition(&mut self, keep: &mut bool) -> Result<(), ParseError> {
        while let Some((Ok(Tok::Condition(c)), span)) = self.toks.get(self.i).cloned() {
            self.i += 1;
            match self.cond.eval(c) {
                Some(v) => *keep &= v,
                None => return Err(self.err(ParseErrorKind::BadCondition(c.to_string()), span.start)),
            }
        }
        Ok(())
    }

    fn block(&mut self, top: bool) -> Result<Vec<Node>, ParseError> {
        let mut out = Vec::new();
        loop {
            let (key, key_pos) = match self.next()? {
                Some((Tok::Quoted(k) | Tok::Bare(k), span)) => (k.to_string(), span.start),
                Some((Tok::Close, span)) => {
                    if top {
                        return Err(self.err(ParseErrorKind::UnexpectedCloseBrace, span.start));
                    }
                    return Ok(out);
                }
                Some((Tok::Open, span)) => return Err(self.err(ParseErrorKind::UnexpectedOpenBrace, span.start)),
                Some((Tok::Condition(c), span)) => {
                    return Err(self.err(ParseErrorKind::BadCondition(c.to_string()), span.start))
                }
                Some((Tok::Unterminated, _)) => unreachable!("handled in next()"),
                None => {
                    if top {
                        return Ok(out);
                    }
                    return Err(self.err(ParseErrorKind::UnexpectedEof, self.eof_pos()));
                }
            };
            let mut keep = true;
            self.condition(&mut keep)?;
            let node = match self.next()? {
                Some((Tok::Quoted(v) | Tok::Bare(v), _)) => Node::str(key, v),
                Some((Tok::Open, _)) => Node::block(key, self.block(false)?),
                Some((_, span)) => {
                    return Err(self.err(ParseErrorKind::MissingValue { key }, span.start));
                }
                None => return Err(self.err(ParseErrorKind::MissingValue { key }, key_pos)),
            };
            self.condition(&mut keep)?;
            if keep {
                out.push(node);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv::node::{NodeList, Value};

    #[test]
    fn nested_and_comments() {
        let n = parse("// c\n\"a\" \"1\"\nb\n{\n  \"c\" \"d e\"\n}\n").unwrap();
        assert_eq!(n[0], Node::str("a", "1"));
        assert_eq!(n[1].children().get_str("c"), Some("d e"));
    }

    #[test]
    fn comment_without_space() {
        let n = parse("//c
a \"1\" //trailing
//x\"y
b \"2\"").unwrap();
        assert_eq!(n.len(), 2);
    }

    #[test]
    fn raw_backslashes_and_bare_tokens() {
        let n = parse("k \"C:\\dir\\file\" bare value").unwrap();
        assert_eq!(n[0].as_str(), Some("C:\\dir\\file"));
        assert_eq!(n[1], Node::str("bare", "value"));
    }

    #[test]
    fn conditionals() {
        let n = parse("a \"1\" [$X360]\nb \"2\" [!$X360]\nc \"3\" [$WIN32 && !$X360]\n").unwrap();
        let keys: Vec<_> = n.iter().map(|n| n.key.as_str()).collect();
        assert_eq!(keys, ["b", "c"]);
    }

    #[test]
    fn errors_have_positions() {
        let e = parse("a\n{\n  \"b\" \"c\"\n").unwrap_err();
        assert_eq!(e.kind, ParseErrorKind::UnexpectedEof);
        let e = parse("a \"1\"\n}").unwrap_err();
        assert_eq!((e.line, e.col, e.kind), (2, 1, ParseErrorKind::UnexpectedCloseBrace));
        let e = parse("k \"oops").unwrap_err();
        assert_eq!(e.kind, ParseErrorKind::UnterminatedString);
        assert!(matches!(parse("k }").unwrap_err().kind, ParseErrorKind::MissingValue { .. }));
    }

    #[test]
    fn roundtrip() {
        let src = "\"a\" \"1\"\nb\n{\n\t\"c\" \"d\"\n}\n";
        let n = parse(src).unwrap();
        let again = parse(&crate::kv::to_string(&n)).unwrap();
        assert_eq!(n, again);
        assert!(matches!(n[1].value, Value::Block(_)));
    }
}
