//! pep508: Python dependency specifiers (PEP 508), parsed, and their
//! environment markers evaluated
//!
//! pydeps-gen checks the specifiers and markers it reads with it, and the
//! pydeps cell (pydeps-cell) evaluates each dependency's marker per
//! platform with [`env_for`]. It parses and evaluates as src/go/pkg/pep508
//! does; both run the cases in testdata/pep508-vectors.json.
//!
//! Reference: https://packaging.python.org/en/latest/specifications/dependency-specifiers/

use anyhow::{Result, anyhow, bail};
use conditions::{CPU, Configuration, OS};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

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

/// A parsed environment marker.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    /// The marker as written.
    pub text: String,
    expr: Expr,
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

/// Normalize a distribution or extra name (PEP 503): lowercase (as Go's
/// strings.ToLower lowers it), runs of `-`, `_` and `.` replaced by `-`.
pub fn normalize_name(name: &str) -> String {
    let mut out = String::new();
    let mut sep = false;
    for c in gostd::strings::to_lower(name).chars() {
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
    let expr = p.or().map_err(|e| anyhow!("marker {s:?}: {e}"))?;
    if p.peek() != &Token::Eof {
        bail!("marker {s:?}: unexpected {:?}", p.peek());
    }
    Ok(Marker {
        text: s.to_string(),
        expr,
    })
}

impl Marker {
    /// Whether the marker holds in `env`.
    pub fn evaluate(&self, env: &Env) -> bool {
        self.expr.eval(env)
    }

    /// The variables the marker reads, sorted.
    pub fn variables(&self) -> Vec<&'static str> {
        let mut seen = BTreeSet::new();
        self.expr.variables(&mut seen);
        seen.into_iter().collect()
    }
}

/// Whether `marker` holds in `env`; no marker always does.
pub fn holds(marker: Option<&Marker>, env: &Env) -> bool {
    marker.is_none_or(|m| m.evaluate(env))
}

/// The environment a marker is evaluated in: a value for each marker
/// variable it holds (sys_platform, python_version, extra, ...). A marker
/// reads a variable it doesn't hold as `""`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct Env(BTreeMap<String, String>);

impl Env {
    /// The value of `variable`, `""` when the environment doesn't hold it
    pub fn get(&self, variable: &str) -> &str {
        self.0.get(variable).map(String::as_str).unwrap_or("")
    }

    /// Sets `variable` to `value`
    pub fn set(&mut self, variable: &str, value: &str) {
        self.0.insert(variable.to_string(), value.to_string());
    }

    /// Whether the environment holds every variable `marker` reads, so that
    /// evaluating it doesn't depend on a value the environment doesn't
    /// know. It does for no marker.
    pub fn decides(&self, marker: Option<&Marker>) -> bool {
        marker.is_none_or(|m| m.variables().iter().all(|v| self.0.contains_key(*v)))
    }
}

impl<const N: usize> From<[(&str, &str); N]> for Env {
    fn from(values: [(&str, &str); N]) -> Self {
        Env(values
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect())
    }
}

/// The marker environment of a configuration for a CPython of the given
/// full version (e.g. `3.13.12`), with `extra`. It holds only the
/// variables it knows: the platform's when the configuration has a
/// platform, in Buck2's names (os `linux` or `macos`, cpu `x86_64` or
/// `arm64`); the Python version's when `python_version` isn't `""`; and
/// the implementation's and `extra` always. `platform_release` and
/// `platform_version`, which no configuration fixes, it never holds;
/// [`Env::decides`] tells whether a marker reads any. `None` for a
/// configuration whose platform it doesn't know.
pub fn env_for(config: &Configuration, python_version: &str, extra: &str) -> Option<Env> {
    let mut env = Env::from([
        ("implementation_name", "cpython"),
        ("platform_python_implementation", "CPython"),
        ("extra", extra),
    ]);
    if !python_version.is_empty() {
        env.set("python_full_version", python_version);
        env.set("implementation_version", python_version);
        env.set("python_version", &major_minor(python_version));
    }
    let (os, cpu) = (config.get(OS), config.get(CPU));
    if os.is_empty() && cpu.is_empty() {
        return Some(env);
    }
    let (sys_platform, system, machine) = match (os, cpu) {
        ("linux", "x86_64") => ("linux", "Linux", "x86_64"),
        ("linux", "arm64") => ("linux", "Linux", "aarch64"),
        ("macos", "x86_64") => ("darwin", "Darwin", "x86_64"),
        ("macos", "arm64") => ("darwin", "Darwin", "arm64"),
        _ => return None,
    };
    env.set("os_name", "posix");
    env.set("sys_platform", sys_platform);
    env.set("platform_system", system);
    env.set("platform_machine", machine);
    Some(env)
}

/// `3.13` for `3.13.12`
fn major_minor(version: &str) -> String {
    let mut parts = version.splitn(3, '.');
    match (parts.next(), parts.next()) {
        (Some(major), Some(minor)) => format!("{major}.{minor}"),
        _ => version.to_string(),
    }
}

/// A marker expression
#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    /// Compares two values
    Cmp {
        left: Operand,
        op: String,
        right: Operand,
    },
}

/// A comparison's side: a variable's value, or a string
#[derive(Debug, Clone, PartialEq)]
enum Operand {
    Variable(&'static str),
    Str(String),
}

impl Operand {
    fn value_in(&self, env: &Env) -> String {
        match self {
            Operand::Str(s) => s.clone(),
            Operand::Variable("extra") => normalize_name(env.get("extra")),
            Operand::Variable(v) => env.get(v).to_string(),
        }
    }

    fn is_extra(&self) -> bool {
        matches!(self, Operand::Variable("extra"))
    }
}

impl Expr {
    fn eval(&self, env: &Env) -> bool {
        match self {
            Expr::Or(x, y) => x.eval(env) || y.eval(env),
            Expr::And(x, y) => x.eval(env) && y.eval(env),
            Expr::Cmp { left, op, right } => compare(left, op, right, env),
        }
    }

    fn variables(&self, seen: &mut BTreeSet<&'static str>) {
        match self {
            Expr::Or(x, y) | Expr::And(x, y) => {
                x.variables(seen);
                y.variables(seen);
            }
            Expr::Cmp { left, right, .. } => {
                for v in [left, right] {
                    if let Operand::Variable(name) = v {
                        seen.insert(name);
                    }
                }
            }
        }
    }
}

/// Compares as PEP 508 does: versions as versions (PEP 440) when both
/// sides are, and otherwise as strings, for which only `==`, `!=`, `in`
/// and `not in` hold. An extra is compared normalized.
fn compare(left: &Operand, op: &str, right: &Operand, env: &Env) -> bool {
    let (mut l, mut r) = (left.value_in(env), right.value_in(env));
    if left.is_extra() || right.is_extra() {
        if let Operand::Str(_) = left {
            l = normalize_name(&l);
        }
        if let Operand::Str(_) = right {
            r = normalize_name(&r);
        }
    }
    match op {
        "in" => return r.contains(&l),
        "not in" => return !r.contains(&l),
        "===" => return l == r,
        _ => {}
    }
    if let Some(result) = compare_versions(&l, op, &r) {
        return result;
    }
    match op {
        "==" => l == r,
        "!=" => l != r,
        _ => false,
    }
}

/// Evaluates `left op right` on release versions (1, 3.13, 3.13.2; right
/// may end with .* for == and !=). `None` if either side isn't one.
fn compare_versions(left: &str, op: &str, right: &str) -> Option<bool> {
    let a = parse_release(left)?;
    if let Some(prefix) = right.strip_suffix(".*") {
        let b = parse_release(prefix)?;
        if op != "==" && op != "!=" {
            return None;
        }
        let matched = a.len() >= b.len() && compare_release(&a[..b.len()], &b).is_eq();
        return Some(matched == (op == "=="));
    }
    let b = parse_release(right)?;
    let c = compare_release(&a, &b);
    Some(match op {
        "==" => c.is_eq(),
        "!=" => c.is_ne(),
        "<" => c.is_lt(),
        "<=" => c.is_le(),
        ">" => c.is_gt(),
        ">=" => c.is_ge(),
        "~=" => {
            if b.len() < 2 {
                return Some(false);
            }
            let n = b.len() - 1;
            c.is_ge() && compare_release(&pad(&a, n)[..n], &b[..n]).is_eq()
        }
        _ => return None,
    })
}

/// A release version's numbers. Each is read as Go's strconv.Atoi reads
/// it on 64 bits, which is `str::parse::<i64>`: an optional sign, then
/// decimal digits, in range. Negative numbers aren't versions.
fn parse_release(s: &str) -> Option<Vec<i64>> {
    if s.is_empty() {
        return None;
    }
    s.split('.')
        .map(|p| p.parse::<i64>().ok().filter(|n| *n >= 0))
        .collect()
}

/// `v` padded with zeros to at least `n` numbers
fn pad(v: &[i64], n: usize) -> Vec<i64> {
    let mut v = v.to_vec();
    while v.len() < n {
        v.push(0);
    }
    v
}

fn compare_release(a: &[i64], b: &[i64]) -> std::cmp::Ordering {
    let n = a.len().max(b.len());
    pad(a, n).cmp(&pad(b, n))
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

    fn or(&mut self) -> Result<Expr> {
        let mut x = self.and()?;
        while self.is_word("or") {
            self.next();
            let y = self.and()?;
            x = Expr::Or(Box::new(x), Box::new(y));
        }
        Ok(x)
    }

    fn and(&mut self) -> Result<Expr> {
        let mut x = self.atom()?;
        while self.is_word("and") {
            self.next();
            let y = self.atom()?;
            x = Expr::And(Box::new(x), Box::new(y));
        }
        Ok(x)
    }

    fn atom(&mut self) -> Result<Expr> {
        if self.peek() == &Token::LParen {
            self.next();
            let e = self.or()?;
            return match self.next() {
                Token::RParen => Ok(e),
                tok => bail!("expected ), got {tok:?}"),
            };
        }
        let left = self.value()?;
        let op = self.op()?;
        let right = self.value()?;
        Ok(Expr::Cmp { left, op, right })
    }

    fn value(&mut self) -> Result<Operand> {
        match self.next() {
            Token::Str(s) => Ok(Operand::Str(s)),
            Token::Ident(v) => match VARIABLES.iter().find(|var| **var == v) {
                Some(var) => Ok(Operand::Variable(var)),
                None => bail!("unknown marker variable {v:?}"),
            },
            tok => bail!("expected a variable or a string, got {tok:?}"),
        }
    }

    fn op(&mut self) -> Result<String> {
        match self.next() {
            Token::Op(op) => Ok(op),
            Token::Ident(w) if w == "in" => Ok("in".to_string()),
            Token::Ident(w) if w == "not" => match self.next() {
                Token::Ident(w) if w == "in" => Ok("not in".to_string()),
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

    #[derive(Deserialize)]
    struct Vectors {
        environments: BTreeMap<String, Env>,
        markers: Vec<MarkerCase>,
        requirements: Vec<RequirementCase>,
        invalid: Vec<String>,
    }

    #[derive(Deserialize)]
    struct MarkerCase {
        marker: String,
        holds: Vec<String>,
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
    fn shared_markers() {
        let v = vectors();
        for case in &v.markers {
            let m = parse_marker(&case.marker).unwrap();
            let holds: Vec<&String> = v
                .environments
                .iter()
                .filter(|(_, env)| m.evaluate(env))
                .map(|(name, _)| name)
                .collect();
            let mut want: Vec<&String> = case.holds.iter().collect();
            want.sort();
            assert_eq!(holds, want, "{}", case.marker);
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

    #[test]
    fn marker_variables() {
        let m = parse_marker(
            r#"sys_platform == "linux" and (python_version < "3.10" or extra == "x")"#,
        )
        .unwrap();
        assert_eq!(m.variables(), ["extra", "python_version", "sys_platform"]);
    }

    fn config(os: &str, cpu: &str) -> Configuration {
        Configuration::new([(OS, os), (CPU, cpu)])
    }

    /// A configuration's platform, in Buck2's names, sets the platform
    /// variables, and the Python version sets the version ones.
    #[test]
    fn env_for_platforms() {
        for (os, cpu, sys_platform, system, machine) in [
            ("linux", "x86_64", "linux", "Linux", "x86_64"),
            ("linux", "arm64", "linux", "Linux", "aarch64"),
            ("macos", "x86_64", "darwin", "Darwin", "x86_64"),
            ("macos", "arm64", "darwin", "Darwin", "arm64"),
        ] {
            let want = Env::from([
                ("os_name", "posix"),
                ("sys_platform", sys_platform),
                ("platform_system", system),
                ("platform_machine", machine),
                ("implementation_name", "cpython"),
                ("platform_python_implementation", "CPython"),
                ("python_full_version", "3.13.12"),
                ("implementation_version", "3.13.12"),
                ("python_version", "3.13"),
                ("extra", "Test_Extra"),
            ]);
            assert_eq!(
                env_for(&config(os, cpu), "3.13.12", "Test_Extra"),
                Some(want),
                "{os}-{cpu}"
            );
        }
    }

    /// Without a platform or a Python version, the environment holds what
    /// it still knows, and says which markers it can't decide.
    #[test]
    fn env_for_fallbacks() {
        let env = env_for(&Configuration::default(), "3.12.1", "");
        let want = Env::from([
            ("implementation_name", "cpython"),
            ("platform_python_implementation", "CPython"),
            ("python_full_version", "3.12.1"),
            ("implementation_version", "3.12.1"),
            ("python_version", "3.12"),
            ("extra", ""),
        ]);
        assert_eq!(env, Some(want));

        let env = env_for(&config("linux", "x86_64"), "", "x").unwrap();
        assert_eq!((env.get("sys_platform"), env.get("extra")), ("linux", "x"));
        for v in [
            "python_version",
            "python_full_version",
            "implementation_version",
        ] {
            assert!(!env.0.contains_key(v), "holds {v}");
        }

        assert_eq!(env_for(&config("windows", "x86_64"), "3.13.12", ""), None);
        // A version without a minor is its own python_version
        assert_eq!(
            env_for(&Configuration::default(), "3", "")
                .unwrap()
                .get("python_version"),
            "3"
        );
    }

    /// An environment decides a marker if it holds every variable the
    /// marker reads; the extra is always held, platform_release and
    /// platform_version never.
    #[test]
    fn env_decides() {
        let no_platform = env_for(&Configuration::default(), "3.13.12", "").unwrap();
        let no_version = env_for(&config("macos", "arm64"), "", "").unwrap();
        let full = env_for(&config("macos", "arm64"), "3.13.12", "").unwrap();
        for (marker, want) in [
            (r#"extra == "x""#, [true, true, true]),
            (r#"implementation_name == "cpython""#, [true, true, true]),
            (r#"python_version < "3.11""#, [true, false, true]),
            (r#"sys_platform == "linux""#, [false, true, true]),
            (
                r#"sys_platform == "linux" and python_version < "3.11""#,
                [false, false, true],
            ),
            (r#"platform_release >= "5""#, [false, false, false]),
            (r#"platform_version == "x""#, [false, false, false]),
        ] {
            let m = parse_marker(marker).unwrap();
            let got = [&no_platform, &no_version, &full].map(|env| env.decides(Some(&m)));
            assert_eq!(got, want, "{marker}");
        }
        assert!(full.decides(None), "an env doesn't decide no marker");
    }

    /// The extra is compared normalized, as the extra env_for is given.
    #[test]
    fn env_for_extra() {
        let m = parse_marker(r#"extra == "test-extra""#).unwrap();
        let with = env_for(&Configuration::default(), "3.13.12", "Test_Extra").unwrap();
        let without = env_for(&Configuration::default(), "3.13.12", "").unwrap();
        assert!(m.evaluate(&with) && !m.evaluate(&without));
        assert!(holds(None, &without));
    }

    /// Versions compare as Go's compareVersions compares them, strings
    /// otherwise
    #[test]
    fn version_comparisons() {
        let env = Env::from([
            ("python_version", "3.13"),
            ("python_full_version", "3.13.0"),
        ]);
        for (marker, want) in [
            (r#"python_version == "3.13.0""#, true),
            (r#"python_version == "+3.13""#, true),
            (r#"python_version == "03.13""#, true),
            (r#"python_version > "-1""#, false),
            (r#"python_version != "3.x""#, true),
            (r#"python_version < "3.x""#, false),
            (r#"python_version == "3.*""#, true),
            (r#"python_version != "3.*""#, false),
            (r#"python_version >= "3.*""#, false),
            (r#"python_version == "3.x.*""#, false),
            (r#"python_full_version ~= "3""#, false),
            (r#"python_full_version ~= "3.12""#, true),
            (r#"python_full_version ~= "3.14""#, false),
            (r#"python_version === "3.13""#, true),
            (r#"python_version === "3.13.0""#, false),
            (r#"python_version == "99999999999999999999""#, false),
            (r#"platform_release == """#, true),
            (r#"platform_release < "1""#, false),
            (r#""" in platform_release"#, true),
        ] {
            assert_eq!(
                parse_marker(marker).unwrap().evaluate(&env),
                want,
                "{marker}"
            );
        }
    }

    /// Names lower as Go lowers them
    #[test]
    fn normalize_names() {
        for (name, want) in [
            ("Foo_Bar.baz", "foo-bar-baz"),
            ("__a--b..", "a-b"),
            ("İnfo", "info"),
            ("", ""),
        ] {
            assert_eq!(normalize_name(name), want, "{name:?}");
        }
    }
}
