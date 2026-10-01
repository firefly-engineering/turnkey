//! The platform specs of Cargo's `[target.'<spec>'.dependencies]` tables,
//! a `cfg()` expression or a target triple, evaluated for the platforms
//! turnkey builds for
//!
//! The Rust port of src/go/pkg/cargocfg: its grammar (Cargo's), parsed by
//! a tokenizer and a recursive-descent parser, with each predicate
//! evaluated by cfg-expr against its copy of rustc's target table, as rustc
//! sets cfgs for the triple. cfg-expr's own parser is stricter than Cargo
//! on some predicates (`cfg(feature)`, `cfg(unix = "x")`, a known key with
//! a value it doesn't know) and laxer on others (`cfg(any(unix,,))`), so it
//! only reads one predicate at a time; one it can't read is not set.
//! testdata/cfg-vectors.json holds the test cases each runs.
//!
//! Reference: https://doc.rust-lang.org/reference/conditional-compilation.html

use gostd::strconv::quote;

/// A parsed platform spec
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spec {
    /// A target triple
    Triple(String),
    /// A `cfg()` expression
    Cfg(Pred),
}

/// A node of a `cfg()` expression
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pred {
    /// `all(...)`: true if every child is, so `all()` is true
    All(Vec<Pred>),
    /// `any(...)`: true if one child is, so `any()` is false
    Any(Vec<Pred>),
    /// `not(...)`
    Not(Box<Pred>),
    /// A bare key, e.g. `unix`
    Key(String),
    /// `key = "value"`, e.g. `target_os = "linux"`
    KeyValue(String, String),
}

impl Spec {
    /// Parses a platform spec: `cfg(<predicate>)` or a target triple.
    pub fn parse(spec: &str) -> Result<Spec, String> {
        let spec = spec.trim_matches(gostd::unicode::is_space);
        if !spec.starts_with("cfg(") {
            if spec.is_empty() || spec.contains([' ', '(', ')', '=', ',', '"']) {
                return Err(format!(
                    "{} is neither cfg(...) nor a target triple",
                    quote(spec)
                ));
            }
            return Ok(Spec::Triple(spec.to_string()));
        }
        let mut p = Parser {
            tokens: tokenize(spec),
            pos: 0,
        };
        p.expect_ident("cfg")?;
        p.expect(Kind::LParen)?;
        let pred = p.predicate()?;
        p.expect(Kind::RParen)?;
        let tok = p.next();
        if tok.kind != Kind::Eof {
            return Err(format!("{spec}: unexpected {tok} after the cfg()"));
        }
        Ok(Spec::Cfg(pred))
    }

    /// Whether the spec holds for the target `triple`
    pub fn matches(&self, triple: &str) -> bool {
        match self {
            Spec::Triple(t) => t == triple,
            Spec::Cfg(pred) => match cfg_expr::targets::get_builtin_target_by_triple(triple) {
                Some(target) => pred.eval(target),
                None => false,
            },
        }
    }
}

impl Pred {
    fn eval(&self, target: &cfg_expr::targets::TargetInfo) -> bool {
        match self {
            Pred::All(children) => children.iter().all(|c| c.eval(target)),
            Pred::Any(children) => children.iter().any(|c| c.eval(target)),
            Pred::Not(child) => !child.eval(target),
            // A bare key other than a target family (test, miri,
            // debug_assertions, a custom --cfg) isn't set when a dependency
            // is built
            Pred::Key(key) => is_set(key, target),
            // A key Cargo doesn't set for the target (a custom cfg such as
            // getrandom_backend) is false
            Pred::KeyValue(key, value) => is_set(&format!("{key} = \"{value}\""), target),
        }
    }
}

/// Whether rustc sets the cfg `predicate` (`key` or `key = "value"`) for
/// `target`: only its target cfgs are, and a predicate cfg-expr can't read
/// isn't.
fn is_set(predicate: &str, target: &cfg_expr::targets::TargetInfo) -> bool {
    match cfg_expr::Expression::parse(predicate) {
        Ok(expr) => expr.eval(|pred| match pred {
            cfg_expr::Predicate::Target(tp) => tp.matches(target),
            _ => false,
        }),
        Err(_) => false,
    }
}

/// A token's kind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Eof,
    Ident,
    String,
    LParen,
    RParen,
    Comma,
    Eq,
    Invalid,
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Kind::Eof => "end of input",
            Kind::Ident => "identifier",
            Kind::String => "string",
            Kind::LParen => "(",
            Kind::RParen => ")",
            Kind::Comma => ",",
            Kind::Eq => "=",
            Kind::Invalid => "invalid character",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    kind: Kind,
    text: String,
}

impl Token {
    fn of(kind: Kind) -> Token {
        Token {
            kind,
            text: String::new(),
        }
    }
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            Kind::Ident | Kind::Invalid => write!(f, "{} {}", self.kind, quote(&self.text)),
            Kind::String => write!(f, "string {}", quote(&self.text)),
            kind => write!(f, "{kind}"),
        }
    }
}

/// A recursive-descent parser over a spec's tokens:
///
/// ```text
/// predicate = ident "(" [ predicate { "," predicate } [ "," ] ] ")"   (all, any, not)
///           | ident [ "=" string ]
/// ```
struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn next(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        if tok.kind != Kind::Eof {
            self.pos += 1;
        }
        tok
    }

    fn peek(&self) -> Kind {
        self.tokens[self.pos].kind
    }

    fn expect(&mut self, kind: Kind) -> Result<(), String> {
        let tok = self.next();
        if tok.kind != kind {
            return Err(format!("expected {kind}, got {tok}"));
        }
        Ok(())
    }

    fn expect_ident(&mut self, name: &str) -> Result<(), String> {
        let tok = self.next();
        if tok.kind != Kind::Ident || tok.text != name {
            return Err(format!("expected {name}, got {tok}"));
        }
        Ok(())
    }

    fn predicate(&mut self) -> Result<Pred, String> {
        let tok = self.next();
        if tok.kind != Kind::Ident {
            return Err(format!("expected a cfg predicate, got {tok}"));
        }
        match self.peek() {
            Kind::LParen => {
                self.next();
                let mut children = self.list()?;
                match tok.text.as_str() {
                    "all" => Ok(Pred::All(children)),
                    "any" => Ok(Pred::Any(children)),
                    "not" if children.len() == 1 => Ok(Pred::Not(Box::new(children.remove(0)))),
                    "not" => Err(format!("not() takes one predicate, got {}", children.len())),
                    op => Err(format!("unknown cfg operator {op}()")),
                }
            }
            Kind::Eq => {
                self.next();
                let value = self.next();
                if value.kind != Kind::String {
                    return Err(format!(
                        "expected a string after {} =, got {value}",
                        tok.text
                    ));
                }
                Ok(Pred::KeyValue(tok.text, value.text))
            }
            _ => Ok(Pred::Key(tok.text)),
        }
    }

    /// Predicates up to and including the closing parenthesis
    fn list(&mut self) -> Result<Vec<Pred>, String> {
        let mut preds = Vec::new();
        while self.peek() != Kind::RParen {
            preds.push(self.predicate()?);
            if self.peek() != Kind::Comma {
                break;
            }
            self.next();
        }
        self.expect(Kind::RParen)?;
        Ok(preds)
    }
}

/// A spec's tokens, ending with [`Kind::Eof`]. An unterminated string or a
/// byte no token starts with is [`Kind::Invalid`] (its text the byte read
/// as Latin-1, as Go's `string(c)` reads it).
fn tokenize(s: &str) -> Vec<Token> {
    let b = s.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'(' => {
                tokens.push(Token::of(Kind::LParen));
                i += 1;
            }
            b')' => {
                tokens.push(Token::of(Kind::RParen));
                i += 1;
            }
            b',' => {
                tokens.push(Token::of(Kind::Comma));
                i += 1;
            }
            b'=' => {
                tokens.push(Token::of(Kind::Eq));
                i += 1;
            }
            b'"' => match b[i + 1..].iter().position(|&c| c == b'"') {
                Some(end) => {
                    tokens.push(Token {
                        kind: Kind::String,
                        text: s[i + 1..i + 1 + end].to_string(),
                    });
                    i += end + 2;
                }
                None => {
                    tokens.push(Token {
                        kind: Kind::Invalid,
                        text: s[i..].to_string(),
                    });
                    break;
                }
            },
            c if is_ident_start(c) => {
                let mut j = i + 1;
                while j < b.len() && (is_ident_start(b[j]) || b[j].is_ascii_digit()) {
                    j += 1;
                }
                tokens.push(Token {
                    kind: Kind::Ident,
                    text: s[i..j].to_string(),
                });
                i = j;
            }
            c => {
                tokens.push(Token {
                    kind: Kind::Invalid,
                    text: char::from(c).to_string(),
                });
                break;
            }
        }
    }
    tokens.push(Token::of(Kind::Eof));
    tokens
}

fn is_ident_start(c: u8) -> bool {
    c == b'_' || c.is_ascii_alphabetic()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    /// The Rust triples of turnkey's platforms, in Buck2's names
    const PLATFORMS: &[(&str, &str)] = &[
        ("linux-x86_64", "x86_64-unknown-linux-gnu"),
        ("linux-arm64", "aarch64-unknown-linux-gnu"),
        ("macos-x86_64", "x86_64-apple-darwin"),
        ("macos-arm64", "aarch64-apple-darwin"),
    ];

    #[derive(Deserialize)]
    struct Vectors {
        cfg: Vec<Vector>,
        triples: Vec<Vector>,
    }

    #[derive(Deserialize)]
    struct Vector {
        spec: String,
        matches: Vec<String>,
    }

    /// The cases src/go/pkg/cargocfg runs too. testdata/ links to the
    /// file, which Buck2 maps there.
    #[test]
    fn shared_vectors() {
        let v: Vectors =
            serde_json::from_str(include_str!("../../testdata/cfg-vectors.json")).unwrap();
        for c in v.cfg.iter().chain(&v.triples) {
            let spec = Spec::parse(&c.spec).unwrap_or_else(|err| panic!("{}: {err}", c.spec));
            let mut got: Vec<&str> = PLATFORMS
                .iter()
                .filter(|(_, triple)| spec.matches(triple))
                .map(|(name, _)| *name)
                .collect();
            got.sort();
            let mut want: Vec<&str> = c.matches.iter().map(String::as_str).collect();
            want.sort();
            assert_eq!(got, want, "{}", c.spec);
        }
    }

    /// Malformed specs are errors, worded as the Go version words them
    #[test]
    fn parse_errors() {
        for (spec, want) in [
            ("cfg(", "expected a cfg predicate, got end of input"),
            (
                "cfg(target_os = )",
                "expected a string after target_os =, got )",
            ),
            ("cfg(target_os = \"linux\"", "expected ), got end of input"),
            (
                "cfg(not(unix, windows))",
                "not() takes one predicate, got 2",
            ),
            ("cfg(maybe(unix))", "unknown cfg operator maybe()"),
            (
                "cfg(unix) extra",
                "cfg(unix) extra: unexpected identifier \"extra\" after the cfg()",
            ),
            (
                "cfg(target_os = \"linux)",
                "expected a string after target_os =, got invalid character \"\\\"linux)\"",
            ),
            (
                "not a triple",
                "\"not a triple\" is neither cfg(...) nor a target triple",
            ),
            ("", "\"\" is neither cfg(...) nor a target triple"),
            ("  ", "\"\" is neither cfg(...) nor a target triple"),
            ("cfg(any(unix,,))", "expected a cfg predicate, got ,"),
            (
                "cfg($)",
                "expected a cfg predicate, got invalid character \"$\"",
            ),
            (
                "cfg(é)",
                "expected a cfg predicate, got invalid character \"Ã\"",
            ),
            (
                "cfgx(unix)",
                "\"cfgx(unix)\" is neither cfg(...) nor a target triple",
            ),
            ("cfg()", "expected a cfg predicate, got )"),
        ] {
            assert_eq!(Spec::parse(spec), Err(want.to_string()), "{spec:?}");
        }
    }

    /// Predicates cfg-expr can't read alone, but Cargo's grammar has, are
    /// not set, as rustc sets no such cfg
    #[test]
    fn predicates_rustc_never_sets() {
        for spec in [
            "cfg(feature)",
            "cfg(unix = \"x\")",
            "cfg(target_pointer_width = \"sixty\")",
            "cfg(target_endian = \"middle\")",
            "cfg(target_thread_local)",
            "cfg(target_has_atomic_load_store = \"64\")",
            "cfg(Unix)",
        ] {
            let parsed = Spec::parse(spec).unwrap();
            for (name, triple) in PLATFORMS {
                assert!(!parsed.matches(triple), "{spec} holds on {name}");
            }
        }
    }
}
