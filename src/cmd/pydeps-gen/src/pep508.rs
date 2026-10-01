//! Python dependency specifiers (PEP 508): parsing, with markers kept as
//! written once their syntax is checked.
//!
//! It parses as src/go/pkg/pep508 does (which also evaluates markers, for
//! rules sync and the pydeps cell); both run the cases in
//! src/go/pkg/pep508/testdata/pep508-vectors.json.
//!
//! Reference: https://packaging.python.org/en/latest/specifications/dependency-specifiers/

use anyhow::{Result, anyhow, bail};

/// A parsed dependency specifier.
#[derive(Debug, Clone, PartialEq)]
pub struct Requirement {
    /// The distribution's name, normalized (PEP 503).
    pub name: String,
    /// The extras it asks for, normalized and sorted.
    pub extras: Vec<String>,
    /// The version specifier or URL, as written (not interpreted).
    pub version: String,
    /// The environment marker, if any.
    pub marker: Option<Marker>,
}

/// An environment marker, syntactically valid.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    /// The marker as written.
    pub text: String,
}

/// The marker variables PEP 508 defines.
pub const VARIABLES: &[&str] = &[
    "os_name",
    "sys_platform",
    "platform_machine",
    "platform_python_implementation",
    "platform_release",
    "platform_system",
    "platform_version",
    "python_version",
    "python_full_version",
    "implementation_name",
    "implementation_version",
    "extra",
];

/// Normalize a distribution or extra name (PEP 503).
pub fn normalize_name(name: &str) -> String {
    let mut out = String::new();
    let mut sep = false;
    for c in name.to_lowercase().chars() {
        if c == '-' || c == '_' || c == '.' {
            sep = true;
            continue;
        }
        if sep && !out.is_empty() {
            out.push('-');
        }
        sep = false;
        out.push(c);
    }
    out
}

fn is_alnum(c: u8) -> bool {
    c.is_ascii_alphanumeric()
}

fn is_name_char(c: u8) -> bool {
    is_alnum(c) || c == b'-' || c == b'_' || c == b'.'
}

/// Parse a dependency specifier:
/// `name [ "[" extra { "," extra } "]" ] [ versionspec | "@" url ] [ ";" marker ]`.
pub fn parse_requirement(s: &str) -> Result<Requirement> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    let start = i;
    while i < b.len() && is_name_char(b[i]) {
        i += 1;
    }
    if i == start || !is_alnum(b[start]) || !is_alnum(b[i - 1]) {
        bail!("{s:?}: expected a distribution name");
    }
    let name = normalize_name(&s[start..i]);
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    let mut extras = Vec::new();
    if i < b.len() && b[i] == b'[' {
        let end = s[i..]
            .find(']')
            .ok_or_else(|| anyhow!("{s:?}: unterminated extras"))?;
        for extra in s[i + 1..i + end].split(',') {
            let extra = extra.trim();
            if !extra.is_empty() {
                extras.push(normalize_name(extra));
            }
        }
        extras.sort();
        i += end + 1;
    }

    // The version spec or URL runs to the marker. A URL may hold ';' only
    // if followed by no whitespace, so a URL's marker follows " ;".
    let rest = &s[i..];
    let is_url = rest.trim_start().starts_with('@');
    let rb = rest.as_bytes();
    let semi = (0..rb.len()).find(|&j| {
        rb[j] == b';' && (!is_url || (j > 0 && (rb[j - 1] == b' ' || rb[j - 1] == b'\t')))
    });
    let (version, marker) = match semi {
        None => (rest.trim().to_string(), None),
        Some(j) => {
            let marker = parse_marker(rest[j + 1..].trim()).map_err(|e| anyhow!("{s:?}: {e}"))?;
            (rest[..j].trim().to_string(), Some(marker))
        }
    };
    Ok(Requirement {
        name,
        extras,
        version,
        marker,
    })
}

/// Parse an environment marker.
pub fn parse_marker(s: &str) -> Result<Marker> {
    let tokens = tokenize(s)?;
    let mut p = Parser { tokens, pos: 0 };
    p.or().map_err(|e| anyhow!("marker {s:?}: {e}"))?;
    if p.peek() != &Token::Eof {
        bail!("marker {s:?}: unexpected {:?}", p.peek());
    }
    Ok(Marker {
        text: s.to_string(),
    })
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Str(String),
    Op(String),
    LParen,
    RParen,
    Eof,
}

/// Recursive-descent parser over a marker's tokens:
/// `or = and { "or" and }`, `and = atom { "and" atom }`,
/// `atom = "(" or ")" | value op value`.
struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn next(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        if tok != Token::Eof {
            self.pos += 1;
        }
        tok
    }

    fn is_word(&self, word: &str) -> bool {
        matches!(self.peek(), Token::Ident(w) if w == word)
    }

    fn or(&mut self) -> Result<()> {
        self.and()?;
        while self.is_word("or") {
            self.next();
            self.and()?;
        }
        Ok(())
    }

    fn and(&mut self) -> Result<()> {
        self.atom()?;
        while self.is_word("and") {
            self.next();
            self.atom()?;
        }
        Ok(())
    }

    fn atom(&mut self) -> Result<()> {
        if self.peek() == &Token::LParen {
            self.next();
            self.or()?;
            return match self.next() {
                Token::RParen => Ok(()),
                tok => bail!("expected ), got {tok:?}"),
            };
        }
        self.value()?;
        self.op()?;
        self.value()
    }

    fn value(&mut self) -> Result<()> {
        match self.next() {
            Token::Str(_) => Ok(()),
            Token::Ident(v) if VARIABLES.contains(&v.as_str()) => Ok(()),
            Token::Ident(v) => bail!("unknown marker variable {v:?}"),
            tok => bail!("expected a variable or a string, got {tok:?}"),
        }
    }

    fn op(&mut self) -> Result<()> {
        match self.next() {
            Token::Op(_) => Ok(()),
            Token::Ident(w) if w == "in" => Ok(()),
            Token::Ident(w) if w == "not" => match self.next() {
                Token::Ident(w) if w == "in" => Ok(()),
                _ => bail!("expected in after not"),
            },
            tok => bail!("expected a comparison, got {tok:?}"),
        }
    }
}

fn tokenize(s: &str) -> Result<Vec<Token>> {
    let b = s.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b'\t' => i += 1,
            b'(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            b')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            b'\'' | b'"' => {
                let end = s[i + 1..]
                    .find(c as char)
                    .ok_or_else(|| anyhow!("marker {s:?}: unterminated string"))?;
                tokens.push(Token::Str(s[i + 1..i + 1 + end].to_string()));
                i += end + 2;
            }
            b'<' | b'>' | b'=' | b'!' | b'~' => {
                let mut j = i;
                while j < b.len() && b"<>=!~".contains(&b[j]) {
                    j += 1;
                }
                let op = &s[i..j];
                if !["<", "<=", ">", ">=", "==", "!=", "~=", "==="].contains(&op) {
                    bail!("marker {s:?}: unknown operator {op:?}");
                }
                tokens.push(Token::Op(op.to_string()));
                i = j;
            }
            c if is_alnum(c) || c == b'_' => {
                let mut j = i;
                while j < b.len() && (is_alnum(b[j]) || b[j] == b'_' || b[j] == b'.') {
                    j += 1;
                }
                tokens.push(Token::Ident(s[i..j].to_string()));
                i = j;
            }
            _ => bail!("marker {s:?}: unexpected {:?}", c as char),
        }
    }
    tokens.push(Token::Eof);
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Vectors {
        markers: Vec<MarkerCase>,
        requirements: Vec<RequirementCase>,
        invalid: Vec<String>,
    }

    #[derive(Deserialize)]
    struct MarkerCase {
        marker: String,
    }

    #[derive(Deserialize)]
    struct RequirementCase {
        requirement: String,
        want: Want,
    }

    #[derive(Deserialize)]
    struct Want {
        name: String,
        extras: Vec<String>,
        marker: String,
    }

    /// The cases src/go/pkg/pep508 runs too. testdata/ links to the file,
    /// and Buck2 maps it to the same path.
    fn vectors() -> Vectors {
        serde_json::from_str(include_str!("../testdata/pep508-vectors.json")).unwrap()
    }

    #[test]
    fn shared_markers_parse() {
        for case in &vectors().markers {
            assert!(parse_marker(&case.marker).is_ok(), "{}", case.marker);
        }
    }

    #[test]
    fn shared_requirements() {
        let v = vectors();
        for case in &v.requirements {
            let req = parse_requirement(&case.requirement).unwrap();
            let marker = req.marker.map(|m| m.text).unwrap_or_default();
            assert_eq!(
                (req.name, req.extras, marker),
                (
                    case.want.name.clone(),
                    case.want.extras.clone(),
                    case.want.marker.clone()
                ),
                "{}",
                case.requirement
            );
        }
        for s in &v.invalid {
            assert!(parse_requirement(s).is_err(), "{s:?} parsed");
        }
    }
}
