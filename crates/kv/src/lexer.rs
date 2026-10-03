use logos::Logos;

/// KeyValues tokens. Quoted strings are raw (backslashes are not escapes), matching VMF/VMT.
#[derive(Logos, Clone, Debug, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip(r"//[^\n]*", allow_greedy = true))]
pub enum Tok<'a> {
    #[token("{")]
    Open,
    #[token("}")]
    Close,
    #[regex(r#""[^"]*""#, |lex| &lex.slice()[1..lex.slice().len() - 1])]
    Quoted(&'a str),
    /// `[$WIN32]`, `[!$X360]`, `[$A && $B]`: platform conditionals after a value.
    #[regex(r"\[!?\$[^\]\r\n]*\]", |lex| lex.slice(), priority = 10)]
    Condition(&'a str),
    #[regex(r#"[^\s{}"]+"#, |lex| lex.slice())]
    Bare(&'a str),
    /// A quote that is never closed (reported as an error by the parser).
    #[regex(r#""[^"]*"#, priority = 1)]
    Unterminated,
}
