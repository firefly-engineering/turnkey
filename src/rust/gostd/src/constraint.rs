//! Go's `go/build/constraint`: build constraint lines (`//go:build` and the
//! older `// +build`), parsed into boolean expressions over build tags
//!
//! A hand port of `go/build/constraint/expr.go` from Go 1.26, the toolchain
//! turnkey pins: which lines are constraints, the expression syntax with
//! its errors and its size limits, and how an expression is written back
//! (`String`, `PlusBuildLines`). Its tests are Go's `expr_test.go`.

use crate::unicode::{is_digit, is_letter, is_space};
use std::fmt;

/// A limit on the complexity of a `//go:build` expression, which keeps the
/// recursive parser from exhausting the stack
const MAX_SIZE: usize = 1000;

/// The limit on the AND and OR operators of a `// +build` line
const MAX_OLD_SIZE: usize = 100;

/// A build tag constraint expression
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// A single tag, e.g. `linux` or `cgo`
    Tag(String),
    /// `!X`
    Not(Box<Expr>),
    /// `X && Y`
    And(Box<Expr>, Box<Expr>),
    /// `X || Y`
    Or(Box<Expr>, Box<Expr>),
}

/// The tag `t`
pub fn tag(t: &str) -> Expr {
    Expr::Tag(t.to_string())
}

/// `!x`
pub fn not(x: Expr) -> Expr {
    Expr::Not(Box::new(x))
}

/// `x && y`
pub fn and(x: Expr, y: Expr) -> Expr {
    Expr::And(Box::new(x), Box::new(y))
}

/// `x || y`
pub fn or(x: Expr, y: Expr) -> Expr {
    Expr::Or(Box::new(x), Box::new(y))
}

impl Expr {
    /// Whether the expression holds, calling `ok(tag)` to find out whether
    /// a tag is set. Both sides of `&&` and `||` are evaluated, so `ok`
    /// sees every tag.
    pub fn eval(&self, ok: &mut impl FnMut(&str) -> bool) -> bool {
        match self {
            Expr::Tag(t) => ok(t),
            Expr::Not(x) => !x.eval(ok),
            Expr::And(x, y) => {
                let xok = x.eval(ok);
                let yok = y.eval(ok);
                xok && yok
            }
            Expr::Or(x, y) => {
                let xok = x.eval(ok);
                let yok = y.eval(ok);
                xok || yok
            }
        }
    }
}

impl fmt::Display for Expr {
    /// The expression in `//go:build` syntax, parenthesized where needed
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Tag(t) => f.write_str(t),
            Expr::Not(x) => match **x {
                Expr::And(..) | Expr::Or(..) => write!(f, "!({x})"),
                _ => write!(f, "!{x}"),
            },
            Expr::And(x, y) => {
                let arg = |e: &Expr| match e {
                    Expr::Or(..) => format!("({e})"),
                    _ => e.to_string(),
                };
                write!(f, "{} && {}", arg(x), arg(y))
            }
            Expr::Or(x, y) => {
                let arg = |e: &Expr| match e {
                    Expr::And(..) => format!("({e})"),
                    _ => e.to_string(),
                };
                write!(f, "{} || {}", arg(x), arg(y))
            }
        }
    }
}

/// A syntax error in a build expression
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    /// The byte offset in the expression where it was detected
    pub offset: usize,
    /// What is wrong
    pub err: String,
}

impl SyntaxError {
    fn new(offset: usize, err: impl Into<String>) -> Self {
        SyntaxError {
            offset,
            err: err.into(),
        }
    }
}

/// Why a line is no build constraint, or one that doesn't parse
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The expression's syntax is wrong
    Syntax(SyntaxError),
    /// The line is neither a `//go:build` nor a `// +build` line
    NotConstraint,
    /// The `// +build` line has more than 100 operators
    Complex,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Syntax(e) => f.write_str(&e.err),
            Error::NotConstraint => f.write_str("not a build constraint"),
            Error::Complex => f.write_str("expression too complex for // +build lines"),
        }
    }
}

impl std::error::Error for Error {}

/// `constraint.Parse`: the expression of a single `//go:build ...` or
/// `// +build ...` line
pub fn parse(line: &str) -> Result<Expr, Error> {
    if let Some(text) = split_go_build(line) {
        return parse_expr(text).map_err(Error::Syntax);
    }
    if let Some(text) = split_plus_build(line) {
        return parse_plus_build_expr(text);
    }
    Err(Error::NotConstraint)
}

/// `constraint.IsGoBuild`: whether the line is a `//go:build` constraint,
/// whether or not its expression parses
pub fn is_go_build(line: &str) -> bool {
    split_go_build(line).is_some()
}

/// `constraint.IsPlusBuild`: whether the line is a `// +build` constraint,
/// whether or not its expression parses
pub fn is_plus_build(line: &str) -> bool {
    split_plus_build(line).is_some()
}

/// `strings.TrimSpace`
fn trim_space(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// The line without one trailing newline, unless it has another
fn single_line(line: &str) -> Option<&str> {
    let line = line.strip_suffix('\n').unwrap_or(line);
    (!line.contains('\n')).then_some(line)
}

/// The expression of a `//go:build` line
fn split_go_build(line: &str) -> Option<&str> {
    let line = single_line(line)?;
    if !line.starts_with("//go:build") {
        return None;
    }
    let line = &trim_space(line)["//go:build".len()..];
    // The prefix must be followed by a space (which the trim finds), or
    // nothing: "//go:buildsomething" is no //go:build line
    let trim = trim_space(line);
    if line.len() == trim.len() && !line.is_empty() {
        return None;
    }
    Some(trim)
}

/// The expression of a `// +build` line (the space is optional)
fn split_plus_build(line: &str) -> Option<&str> {
    let line = single_line(line)?;
    let line = trim_space(line.strip_prefix("//")?);
    let line = line.strip_prefix("+build")?;
    let trim = trim_space(line);
    if line.len() == trim.len() && !line.is_empty() {
        return None;
    }
    Some(trim)
}

/// The state of parsing a `//go:build` expression
struct ExprParser<'a> {
    /// The input
    s: &'a str,
    /// The next read location in `s`
    i: usize,
    /// The last token read, "" at the end
    tok: &'a str,
    is_tag: bool,
    /// The start of the last token
    pos: usize,
    size: usize,
}

/// Parses a `//go:build` expression
pub fn parse_expr(text: &str) -> Result<Expr, SyntaxError> {
    let mut p = ExprParser {
        s: text,
        i: 0,
        tok: "",
        is_tag: false,
        pos: 0,
        size: 0,
    };
    let x = p.or()?;
    if !p.tok.is_empty() {
        return Err(SyntaxError::new(
            p.pos,
            format!("unexpected token {}", p.tok),
        ));
    }
    Ok(x)
}

impl<'a> ExprParser<'a> {
    /// A sequence of `||` expressions. On entry the next token has not been
    /// lexed; on exit it has, and is in `tok`.
    fn or(&mut self) -> Result<Expr, SyntaxError> {
        let mut x = self.and()?;
        while self.tok == "||" {
            x = or(x, self.and()?);
        }
        Ok(x)
    }

    /// A sequence of `&&` expressions, lexed as `or` lexes
    fn and(&mut self) -> Result<Expr, SyntaxError> {
        let mut x = self.not()?;
        while self.tok == "&&" {
            x = and(x, self.not()?);
        }
        Ok(x)
    }

    /// A `!` expression, lexed as `or` lexes
    fn not(&mut self) -> Result<Expr, SyntaxError> {
        self.size += 1;
        if self.size > MAX_SIZE {
            return Err(SyntaxError::new(self.pos, "build expression too large"));
        }
        self.lex()?;
        if self.tok == "!" {
            self.lex()?;
            if self.tok == "!" {
                return Err(SyntaxError::new(self.pos, "double negation not allowed"));
            }
            return Ok(not(self.atom()?));
        }
        self.atom()
    }

    /// A tag or a parenthesized expression. On entry the next token has
    /// been lexed; on exit, the one after it has.
    fn atom(&mut self) -> Result<Expr, SyntaxError> {
        if self.tok == "(" {
            let pos = self.pos;
            let x = self.or().map_err(|mut e| {
                // An end inside the parentheses is a missing close paren,
                // however deep
                if e.err == "unexpected end of expression" {
                    e.err = "missing close paren".to_string();
                }
                e
            })?;
            if self.tok != ")" {
                return Err(SyntaxError::new(pos, "missing close paren"));
            }
            self.lex()?;
            return Ok(x);
        }

        if !self.is_tag {
            if self.tok.is_empty() {
                return Err(SyntaxError::new(self.pos, "unexpected end of expression"));
            }
            return Err(SyntaxError::new(
                self.pos,
                format!("unexpected token {}", self.tok),
            ));
        }
        let t = self.tok;
        self.lex()?;
        Ok(tag(t))
    }

    /// Consumes the next token: `tok` is its text ("" at the end), `is_tag`
    /// whether it is a tag, and `pos` its offset
    fn lex(&mut self) -> Result<(), SyntaxError> {
        self.is_tag = false;
        let b = self.s.as_bytes();
        while self.i < b.len() && (b[self.i] == b' ' || b[self.i] == b'\t') {
            self.i += 1;
        }
        if self.i >= b.len() {
            self.tok = "";
            self.pos = self.i;
            return Ok(());
        }
        match b[self.i] {
            b'(' | b')' | b'!' => {
                self.pos = self.i;
                self.i += 1;
                self.tok = &self.s[self.pos..self.i];
                return Ok(());
            }
            c @ (b'&' | b'|') => {
                if self.i + 1 >= b.len() || b[self.i + 1] != c {
                    return Err(SyntaxError::new(
                        self.i,
                        format!("invalid syntax at {}", c as char),
                    ));
                }
                self.pos = self.i;
                self.i += 2;
                self.tok = &self.s[self.pos..self.i];
                return Ok(());
            }
            _ => {}
        }

        let rest = &self.s[self.i..];
        let len = rest
            .char_indices()
            .find(|&(_, c)| !is_tag_char(c))
            .map_or(rest.len(), |(i, _)| i);
        if len == 0 {
            let c = rest.chars().next().expect("not at the end");
            return Err(SyntaxError::new(self.i, format!("invalid syntax at {c}")));
        }
        self.pos = self.i;
        self.i += len;
        self.tok = &self.s[self.pos..self.i];
        self.is_tag = true;
        Ok(())
    }
}

/// Whether a character can be part of a tag: a letter, a digit, `_` or `.`
fn is_tag_char(c: char) -> bool {
    is_letter(c) || is_digit(c) || c == '_' || c == '.'
}

/// `isValidTag`: whether the word is a valid build tag. Unlike Go
/// identifiers, tags may start with a digit (e.g. `386`).
fn is_valid_tag(word: &str) -> bool {
    !word.is_empty() && word.chars().all(is_tag_char)
}

/// Parses a `// +build` expression: spaces separate the ORed clauses,
/// commas the ANDed terms of a clause, and `!` negates a term. An invalid
/// term is the tag `ignore`.
fn parse_plus_build_expr(text: &str) -> Result<Expr, Error> {
    let mut size = 0;
    let mut x: Option<Expr> = None;
    for clause in text.split(is_space).filter(|f| !f.is_empty()) {
        let mut y: Option<Expr> = None;
        for lit in clause.split(',') {
            let z = if lit.starts_with("!!") || lit == "!" {
                tag("ignore")
            } else {
                let (neg, lit) = match lit.strip_prefix('!') {
                    Some(rest) => (true, rest),
                    None => (false, lit),
                };
                let z = if is_valid_tag(lit) {
                    tag(lit)
                } else {
                    tag("ignore")
                };
                if neg { not(z) } else { z }
            };
            y = Some(match y {
                None => z,
                Some(y) => {
                    size += 1;
                    if size > MAX_OLD_SIZE {
                        return Err(Error::Complex);
                    }
                    and(y, z)
                }
            });
        }
        let y = y.expect("a clause has a term");
        x = Some(match x {
            None => y,
            Some(x) => {
                size += 1;
                if size > MAX_OLD_SIZE {
                    return Err(Error::Complex);
                }
                or(x, y)
            }
        });
    }
    Ok(x.unwrap_or_else(|| tag("ignore")))
}

/// `constraint.PlusBuildLines`: `// +build` lines that together evaluate
/// to `x`, or [`Error::Complex`] when it can't be written as such lines
pub fn plus_build_lines(x: &Expr) -> Result<Vec<String>, Error> {
    // Push the negations down to the tags, so that !(x && y) is !x || !y
    let x = push_not(x, false);

    // An AND of ORs of ANDs of literals (a tag or its negation)
    let mut split: Vec<Vec<Vec<&Expr>>> = Vec::new();
    for or in split_and(&x) {
        let mut ands = Vec::new();
        for and in split_or(or) {
            let mut lits = Vec::new();
            for lit in split_and(and) {
                match lit {
                    Expr::Tag(_) | Expr::Not(_) => lits.push(lit),
                    _ => return Err(Error::Complex),
                }
            }
            ands.push(lits);
        }
        split.push(ands);
    }

    // With no actual ORs, the top-level ANDs go to the bottom level: one
    // line instead of many
    if split.iter().map(Vec::len).max().unwrap_or(0) == 1 {
        let lits = split.iter().flat_map(|or| or[0].iter().copied()).collect();
        split = vec![vec![lits]];
    }

    Ok(split
        .iter()
        .map(|or| {
            let mut line = String::from("// +build");
            for and in or {
                line.push(' ');
                let lits: Vec<String> = and.iter().map(|lit| lit.to_string()).collect();
                line.push_str(&lits.join(","));
            }
            line
        })
        .collect())
}

/// De Morgan's laws applied to push negations down to the tags
fn push_not(x: &Expr, negate: bool) -> Expr {
    match x {
        Expr::Not(inner) => {
            if matches!(**inner, Expr::Tag(_)) && !negate {
                return x.clone();
            }
            push_not(inner, !negate)
        }
        Expr::Tag(_) => {
            if negate {
                not(x.clone())
            } else {
                x.clone()
            }
        }
        Expr::And(a, b) => {
            let (a, b) = (push_not(a, negate), push_not(b, negate));
            if negate { or(a, b) } else { and(a, b) }
        }
        Expr::Or(a, b) => {
            let (a, b) = (push_not(a, negate), push_not(b, negate));
            if negate { and(a, b) } else { or(a, b) }
        }
    }
}

/// `x`'s top-level `&&` operands, in order
fn split_and(x: &Expr) -> Vec<&Expr> {
    match x {
        Expr::And(a, b) => {
            let mut list = split_and(a);
            list.extend(split_and(b));
            list
        }
        _ => vec![x],
    }
}

/// `x`'s top-level `||` operands, in order
fn split_or(x: &Expr) -> Vec<&Expr> {
    match x {
        Expr::Or(a, b) => {
            let mut list = split_or(a);
            list.extend(split_or(b));
            list
        }
        _ => vec![x],
    }
}

#[cfg(test)]
mod tests {
    //! Go 1.26's go/build/constraint/expr_test.go

    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn expr_string() {
        for (x, out) in [
            (tag("abc"), "abc"),
            (not(tag("abc")), "!abc"),
            (not(and(tag("abc"), tag("def"))), "!(abc && def)"),
            (
                and(tag("abc"), or(tag("def"), tag("ghi"))),
                "abc && (def || ghi)",
            ),
            (
                or(and(tag("abc"), tag("def")), tag("ghi")),
                "(abc && def) || ghi",
            ),
        ] {
            assert_eq!(x.to_string(), out);
        }
    }

    #[test]
    fn lex() {
        for (input, want) in [
            ("", ""),
            ("x", "x"),
            ("x.y", "x.y"),
            ("x_y", "x_y"),
            ("αx", "αx"),
            ("αx²", "αx err: invalid syntax at ²"),
            ("go1.2", "go1.2"),
            ("x y", "x y"),
            ("x!y", "x ! y"),
            ("&&||!()xy yx ", "&& || ! ( ) xy yx"),
            ("x~", "x err: invalid syntax at ~"),
            ("x ~", "x err: invalid syntax at ~"),
            ("x &", "x err: invalid syntax at &"),
            ("x &y", "x err: invalid syntax at &"),
        ] {
            let mut p = ExprParser {
                s: input,
                i: 0,
                tok: "",
                is_tag: false,
                pos: 0,
                size: 0,
            };
            let mut out = String::new();
            loop {
                let res = p.lex();
                if res.is_ok() && p.tok.is_empty() {
                    break;
                }
                if !out.is_empty() {
                    out.push(' ');
                }
                match res {
                    Err(e) => {
                        out.push_str(&format!("err: {}", e.err));
                        break;
                    }
                    Ok(()) => out.push_str(p.tok),
                }
            }
            assert_eq!(out, want, "lex({input:?})");
        }
    }

    #[test]
    fn parse_expr_cases() {
        for (input, x) in [
            ("x", tag("x")),
            ("x&&y", and(tag("x"), tag("y"))),
            ("x||y", or(tag("x"), tag("y"))),
            ("(x)", tag("x")),
            ("x||y&&z", or(tag("x"), and(tag("y"), tag("z")))),
            ("x&&y||z", or(and(tag("x"), tag("y")), tag("z"))),
            ("x&&(y||z)", and(tag("x"), or(tag("y"), tag("z")))),
            ("(x||y)&&z", and(or(tag("x"), tag("y")), tag("z"))),
            ("!(x&&y)", not(and(tag("x"), tag("y")))),
        ] {
            assert_eq!(parse_expr(input).unwrap(), x, "parse_expr({input:?})");
        }
    }

    #[test]
    fn parse_error() {
        for (input, offset, err) in [
            ("x && ", 5, "unexpected end of expression"),
            ("x && (", 6, "missing close paren"),
            ("x && ||", 5, "unexpected token ||"),
            ("x && !", 6, "unexpected end of expression"),
            ("x && !!", 6, "double negation not allowed"),
            ("x !", 2, "unexpected token !"),
            ("x && (y", 5, "missing close paren"),
        ] {
            assert_eq!(
                parse_expr(input),
                Err(SyntaxError::new(offset, err)),
                "parse_expr({input:?})"
            );
        }
    }

    #[test]
    fn expr_eval() {
        for (input, want, want_tags) in [
            ("x", false, "x"),
            ("x && y", false, "x y"),
            ("x || y", false, "x y"),
            ("!x && yes", true, "x yes"),
            ("yes || y", true, "y yes"),
        ] {
            let x = parse_expr(input).unwrap();
            let mut tags = BTreeSet::new();
            let ok = x.eval(&mut |t: &str| {
                tags.insert(t.to_string());
                t == "yes"
            });
            let want_tags: BTreeSet<String> = want_tags.split(' ').map(str::to_string).collect();
            assert_eq!((ok, tags), (want, want_tags), "eval({input:?})");
        }
    }

    #[test]
    fn parse_plus_build() {
        for (input, x) in [
            ("x", tag("x")),
            ("x,y", and(tag("x"), tag("y"))),
            ("x y", or(tag("x"), tag("y"))),
            ("x y,z", or(tag("x"), and(tag("y"), tag("z")))),
            ("x,y z", or(and(tag("x"), tag("y")), tag("z"))),
            ("x,!y !z", or(and(tag("x"), not(tag("y"))), not(tag("z")))),
            ("!! x", or(tag("ignore"), tag("x"))),
            ("!!x", tag("ignore")),
            ("!x", not(tag("x"))),
            ("!", tag("ignore")),
            ("", tag("ignore")),
        ] {
            assert_eq!(
                parse_plus_build_expr(input).unwrap().to_string(),
                x.to_string(),
                "parse_plus_build_expr({input:?})"
            );
        }
    }

    #[test]
    fn parse_lines() {
        for (input, x, err) in [
            ("//+build !", Some(tag("ignore")), ""),
            ("//+build", Some(tag("ignore")), ""),
            ("//+build x y", Some(or(tag("x"), tag("y"))), ""),
            ("// +build x y \n", Some(or(tag("x"), tag("y"))), ""),
            ("// +build x y \n ", None, "not a build constraint"),
            ("// +build x y \nmore", None, "not a build constraint"),
            (" //+build x y", None, "not a build constraint"),
            ("//go:build x && y", Some(and(tag("x"), tag("y"))), ""),
            ("//go:build x && y\n", Some(and(tag("x"), tag("y"))), ""),
            ("//go:build x && y\n ", None, "not a build constraint"),
            ("//go:build x && y\nmore", None, "not a build constraint"),
            (" //go:build x && y", None, "not a build constraint"),
            ("//go:build\n", None, "unexpected end of expression"),
        ] {
            match (parse(input), x) {
                (Ok(got), Some(x)) => assert_eq!(got.to_string(), x.to_string(), "{input:?}"),
                (Err(e), None) => assert!(e.to_string().contains(err), "{input:?}: {e}"),
                (got, _) => panic!("parse({input:?}) = {got:?}"),
            }
        }
        assert!(is_go_build("//go:build x") && !is_go_build("//go:buildx"));
        assert!(is_plus_build("// +build x") && !is_plus_build("// +buildx"));
    }

    #[test]
    fn plus_build_lines_cases() {
        for (input, out) in [
            ("x", Some(vec!["x"])),
            ("x && !y", Some(vec!["x,!y"])),
            ("x || y", Some(vec!["x y"])),
            ("x && (y || z)", Some(vec!["x", "y z"])),
            ("!(x && y)", Some(vec!["!x !y"])),
            ("x || (y && z)", Some(vec!["x y,z"])),
            ("w && (x || (y && z))", Some(vec!["w", "x y,z"])),
            ("v || (w && (x || (y && z)))", None),
        ] {
            let x = parse_expr(input).unwrap();
            let got = plus_build_lines(&x);
            match out {
                Some(lines) => {
                    let want: Vec<String> =
                        lines.iter().map(|l| format!("// +build {l}")).collect();
                    assert_eq!(got, Ok(want), "{input:?}");
                }
                None => assert_eq!(got, Err(Error::Complex), "{input:?}"),
            }
        }
    }

    #[test]
    fn size_limits() {
        for expr in [
            "a || ".repeat(MAX_SIZE + 2),
            "a && ".repeat(MAX_SIZE + 2),
            "(a &&".repeat(MAX_SIZE + 2),
            "(a ||".repeat(MAX_SIZE + 2),
        ] {
            match parse(&format!("//go:build {expr}")) {
                Err(Error::Syntax(e)) => assert_eq!(e.err, "build expression too large"),
                got => panic!("{got:?}"),
            }
        }
    }

    #[test]
    fn plus_size_limits() {
        for expr in ["a ".repeat(MAX_OLD_SIZE + 2), "a,".repeat(MAX_OLD_SIZE + 2)] {
            assert_eq!(parse(&format!("// +build {expr}")), Err(Error::Complex));
        }
    }
}
