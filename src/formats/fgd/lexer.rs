//! FGD tokenizer (logos-based).

use logos::Logos;
use std::ops::Range;

#[derive(Debug, Clone, PartialEq)]
pub enum T {
    Id(String),
    Str(String),
    Sym(char),
}

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
enum Raw<'a> {
    #[regex(r"//[^\n]*", priority = 20, allow_greedy = true)]
    Comment,
    #[regex(r#""[^"]*""#, |lex| &lex.slice()[1..lex.slice().len() - 1])]
    Str(&'a str),
    #[regex(r#""[^"]*"#, priority = 1)]
    Unterminated,
    #[regex(r"[()\[\]{}=:,+@]", |lex| lex.slice().chars().next().unwrap())]
    Sym(char),
    #[regex(r#"[^\s()\[\]{}=:,+@"]+"#, |lex| lex.slice())]
    Id(&'a str),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub line: usize,
    pub col: usize,
    pub msg: &'static str,
}

fn position(src: &str, offset: usize) -> (usize, usize) {
    let before = &src[..offset.min(src.len())];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, col)
}

/// Tokenizes `src`. Unterminated strings are kept (up to end of file) and reported;
/// unrecognised input is skipped and reported.
pub fn tokenize(src: &str) -> (Vec<(T, Range<usize>)>, Vec<LexError>) {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let mut out = Vec::new();
    let mut errors = Vec::new();
    let mut lx = Raw::lexer(src);
    while let Some(r) = lx.next() {
        let span = lx.span();
        let mut err = |msg| {
            let (line, col) = position(src, span.start);
            errors.push(LexError { line, col, msg });
        };
        match r {
            Ok(Raw::Comment) => {}
            Ok(Raw::Str(s)) => out.push((T::Str(s.to_string()), span)),
            Ok(Raw::Unterminated) => {
                err("unterminated string");
                out.push((T::Str(lx.slice()[1..].to_string()), span));
            }
            Ok(Raw::Sym(c)) => out.push((T::Sym(c), span)),
            Ok(Raw::Id(s)) => out.push((T::Id(s.to_string()), span)),
            Err(()) => err("invalid token"),
        }
    }
    (out, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<T> {
        tokenize(s).0.into_iter().map(|(t, _)| t).collect()
    }

    #[test]
    fn basic() {
        assert_eq!(
            toks("@PointClass base(Targetname) = prop : \"Desc\" + \"more\" [ ] //c"),
            vec![
                T::Sym('@'),
                T::Id("PointClass".into()),
                T::Id("base".into()),
                T::Sym('('),
                T::Id("Targetname".into()),
                T::Sym(')'),
                T::Sym('='),
                T::Id("prop".into()),
                T::Sym(':'),
                T::Str("Desc".into()),
                T::Sym('+'),
                T::Str("more".into()),
                T::Sym('['),
                T::Sym(']'),
            ]
        );
    }

    #[test]
    fn comment_and_unterminated() {
        assert_eq!(toks("//only\nx"), vec![T::Id("x".into())]);
        let (t, e) = tokenize("a \"oops\nb");
        assert_eq!(t.len(), 2);
        assert_eq!((e[0].line, e[0].col), (1, 3));
    }
}
