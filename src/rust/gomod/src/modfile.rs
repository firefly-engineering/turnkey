//! go.mod and go.work files: their directives, read from the statements
//! [`crate::syntax`] parses, as `golang.org/x/mod/modfile`'s `Parse` (in
//! strict mode, with no version fixer) and `ParseWork` read them
//!
//! A file with any invalid directive is an error, listing every one. A
//! version must already be canonical (`v1.2.3`, or `+incompatible`); the
//! `go` command writes them so.

use crate::module::{self, canonical_version, check_path_major, split_path_version};
use crate::syntax::{self, Error, ErrorList, Line, Stmt};

/// A module version
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Version {
    pub path: String,
    /// Empty for a module path alone (a replace's old module without a
    /// version, a local replacement)
    pub version: String,
}

/// A `require` directive
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Require {
    pub module: Version,
    /// Marked `// indirect`
    pub indirect: bool,
}

/// A `replace` directive
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replace {
    pub old: Version,
    /// A module version, or a directory path with no version
    pub new: Version,
}

/// A parsed go.mod
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModFile {
    /// The module path, `None` without a module directive
    pub module: Option<String>,
    pub go: Option<String>,
    pub toolchain: Option<String>,
    pub godebug: Vec<(String, String)>,
    pub require: Vec<Require>,
    pub exclude: Vec<Version>,
    pub replace: Vec<Replace>,
    /// The retracted versions, as `(low, high)`
    pub retract: Vec<(String, String)>,
    pub tool: Vec<String>,
    pub ignore: Vec<String>,
}

/// A parsed go.work
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkFile {
    pub go: Option<String>,
    pub toolchain: Option<String>,
    pub godebug: Vec<(String, String)>,
    /// The `use` directives' directories, as written
    pub use_dirs: Vec<String>,
    pub replace: Vec<Replace>,
}

/// `modfile.Parse(file, data, nil)`: the go.mod named `file` (for errors)
/// holding `data`
pub fn parse_mod(file: &str, data: &str) -> Result<ModFile, ErrorList> {
    let stmts = syntax::parse(file, data).map_err(|e| ErrorList(vec![e]))?;
    let mut f = ModFile::default();
    let mut d = Directives::new(file);
    for stmt in &stmts {
        match stmt {
            Stmt::Line(line) => d.add_mod(&mut f, line, &line.tokens[0], &line.tokens[1..]),
            Stmt::Block(block) => match block.tokens.as_slice() {
                [verb]
                    if matches!(
                        verb.as_str(),
                        "module"
                            | "godebug"
                            | "require"
                            | "exclude"
                            | "replace"
                            | "retract"
                            | "tool"
                            | "ignore"
                    ) =>
                {
                    for line in &block.lines {
                        d.add_mod(&mut f, line, verb, &line.tokens);
                    }
                }
                tokens => d.unknown_block(block.start, tokens),
            },
        }
    }
    d.finish(f)
}

/// `modfile.ParseWork(file, data, nil)`: the go.work named `file` (for
/// errors) holding `data`
pub fn parse_work(file: &str, data: &str) -> Result<WorkFile, ErrorList> {
    let stmts = syntax::parse(file, data).map_err(|e| ErrorList(vec![e]))?;
    let mut f = WorkFile::default();
    let mut d = Directives::new(file);
    for stmt in &stmts {
        match stmt {
            Stmt::Line(line) => d.add_work(&mut f, line, &line.tokens[0], &line.tokens[1..]),
            Stmt::Block(block) => match block.tokens.as_slice() {
                [verb] if matches!(verb.as_str(), "godebug" | "use" | "replace") => {
                    for line in &block.lines {
                        d.add_work(&mut f, line, verb, &line.tokens);
                    }
                }
                tokens => d.unknown_block(block.start, tokens),
            },
        }
    }
    d.finish(f)
}

/// `modfile.IsDirectoryPath`: whether a replacement (or `use`) path is a
/// directory rather than a module path: `.` or `..` and below, or rooted,
/// in Unix or Windows syntax
pub fn is_directory_path(ns: &str) -> bool {
    let b = ns.as_bytes();
    ns == "."
        || ns.starts_with("./")
        || ns.starts_with(".\\")
        || ns == ".."
        || ns.starts_with("../")
        || ns.starts_with("..\\")
        || ns.starts_with('/')
        || ns.starts_with('\\')
        || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
}

/// Reads directives into a file, collecting their errors
struct Directives<'a> {
    file: &'a str,
    errors: Vec<Error>,
    /// Whether a module directive was seen, valid or not
    module_seen: bool,
}

impl<'a> Directives<'a> {
    fn new(file: &'a str) -> Self {
        Self {
            file,
            errors: Vec::new(),
            module_seen: false,
        }
    }

    fn finish<T>(self, f: T) -> Result<T, ErrorList> {
        if self.errors.is_empty() {
            Ok(f)
        } else {
            Err(ErrorList(self.errors))
        }
    }

    fn error_at(&mut self, line: &Line, verb: &str, mod_path: &str, message: String) {
        self.errors.push(Error {
            filename: self.file.to_string(),
            pos: Some(line.start),
            // modfile.Error: "verb path", or "verb" without a path
            directive: match (verb, mod_path) {
                (verb, "") => verb.to_string(),
                (verb, path) => format!("{verb} {path}"),
            },
            message,
        });
    }

    /// An error about the line, not a module
    fn error(&mut self, line: &Line, message: String) {
        self.error_at(line, "", "", message);
    }

    fn unknown_block(&mut self, start: syntax::Position, tokens: &[String]) {
        self.errors.push(Error {
            filename: self.file.to_string(),
            pos: Some(start),
            directive: String::new(),
            message: format!("unknown block type: {}", tokens.join(" ")),
        });
    }

    /// `File.add`, strict
    fn add_mod(&mut self, f: &mut ModFile, line: &Line, verb: &str, args: &[String]) {
        match verb {
            "go" => {
                if let Some(v) = self.go_version(line, f.go.is_some(), args) {
                    f.go = Some(v);
                }
            }
            "toolchain" => {
                if let Some(t) = self.toolchain(line, f.toolchain.is_some(), args) {
                    f.toolchain = Some(t);
                }
            }
            "module" => {
                if self.module_seen {
                    return self.error(line, "repeated module statement".to_string());
                }
                self.module_seen = true;
                if args.len() != 1 {
                    return self.error(line, "usage: module module/path".to_string());
                }
                match parse_string(&args[0]) {
                    Ok(path) => f.module = Some(path),
                    Err(e) => self.error(line, format!("invalid quoted string: {e}")),
                }
            }
            "godebug" => {
                if let Some(kv) = self.godebug(line, args) {
                    f.godebug.push(kv);
                }
            }
            "require" | "exclude" => {
                if args.len() != 2 {
                    return self.error(line, format!("usage: {verb} module/path v1.2.3"));
                }
                let path = match parse_string(&args[0]) {
                    Ok(path) => path,
                    Err(e) => return self.error(line, format!("invalid quoted string: {e}")),
                };
                let Some(version) = self.version(line, verb, &path, &args[1]) else {
                    return;
                };
                let Some((_, path_major)) = split_path_version(&path) else {
                    return self.error(line, "invalid module path".to_string());
                };
                if let Err(e) = check_path_major(&version, path_major) {
                    return self.error_at(line, verb, &path, e.to_string());
                }
                let module = Version { path, version };
                if verb == "require" {
                    f.require.push(Require {
                        module,
                        indirect: is_indirect(line),
                    });
                } else {
                    f.exclude.push(module);
                }
            }
            "replace" => {
                if let Some(r) = self.replace(line, verb, args) {
                    f.replace.push(r);
                }
            }
            "retract" => {
                let mut args = args;
                let Some(interval) = self.version_interval(line, verb, &mut args) else {
                    return;
                };
                if let Some(extra) = args.first() {
                    return self.error(
                        line,
                        format!(
                            "unexpected token after version: {}",
                            gostd::strconv::quote(extra)
                        ),
                    );
                }
                f.retract.push(interval);
            }
            "tool" | "ignore" => {
                if args.len() != 1 {
                    return self.error(
                        line,
                        format!("{verb} directive expects exactly one argument"),
                    );
                }
                match parse_string(&args[0]) {
                    Ok(path) if verb == "tool" => f.tool.push(path),
                    Ok(path) => f.ignore.push(path),
                    Err(e) => self.error(line, format!("invalid quoted string: {e}")),
                }
            }
            _ => self.error(line, format!("unknown directive: {verb}")),
        }
    }

    /// `WorkFile.add`
    fn add_work(&mut self, f: &mut WorkFile, line: &Line, verb: &str, args: &[String]) {
        match verb {
            "go" => {
                if let Some(v) = self.go_version(line, f.go.is_some(), args) {
                    f.go = Some(v);
                }
            }
            "toolchain" => {
                if let Some(t) = self.toolchain(line, f.toolchain.is_some(), args) {
                    f.toolchain = Some(t);
                }
            }
            "godebug" => {
                if let Some(kv) = self.godebug(line, args) {
                    f.godebug.push(kv);
                }
            }
            "use" => {
                if args.len() != 1 {
                    return self.error(line, format!("usage: {verb} local/dir"));
                }
                match parse_string(&args[0]) {
                    Ok(dir) => f.use_dirs.push(dir),
                    Err(e) => self.error(line, format!("invalid quoted string: {e}")),
                }
            }
            "replace" => {
                if let Some(r) = self.replace(line, verb, args) {
                    f.replace.push(r);
                }
            }
            _ => self.error(line, format!("unknown directive: {verb}")),
        }
    }

    fn go_version(&mut self, line: &Line, repeated: bool, args: &[String]) -> Option<String> {
        if repeated {
            self.error(line, "repeated go statement".to_string());
            return None;
        }
        if args.len() != 1 {
            self.error(
                line,
                "go directive expects exactly one argument".to_string(),
            );
            return None;
        }
        if !is_go_version(&args[0]) {
            self.error(
                line,
                format!("invalid go version '{}': must match format 1.23.0", args[0]),
            );
            return None;
        }
        Some(args[0].clone())
    }

    fn toolchain(&mut self, line: &Line, repeated: bool, args: &[String]) -> Option<String> {
        if repeated {
            self.error(line, "repeated toolchain statement".to_string());
            return None;
        }
        if args.len() != 1 {
            self.error(
                line,
                "toolchain directive expects exactly one argument".to_string(),
            );
            return None;
        }
        let name = &args[0];
        // modfile.ToolchainRE: ^default$|^go1($|\.)
        if !(name == "default" || name == "go1" || name.starts_with("go1.")) {
            self.error(
                line,
                format!(
                    "invalid toolchain version '{name}': must match format go1.23.0 or default"
                ),
            );
            return None;
        }
        Some(name.clone())
    }

    fn godebug(&mut self, line: &Line, args: &[String]) -> Option<(String, String)> {
        let kv = match args {
            [arg] if !arg.contains(['"', '`', '\'', ',']) => arg.split_once('='),
            _ => None,
        };
        if kv.is_none() {
            self.error(line, "usage: godebug key=value".to_string());
        }
        kv.map(|(k, v)| (k.to_string(), v.to_string()))
    }

    /// `parseVersion` with no fixer: the token, unquoted, must be a
    /// canonical version
    fn version(&mut self, line: &Line, verb: &str, path: &str, token: &str) -> Option<String> {
        let invalid = |version: &str, reason: String| module::Error::InvalidVersion {
            version: version.to_string(),
            reason,
        };
        let err = match parse_string(token) {
            Err(e) => invalid(token, e),
            Ok(v) => {
                let cv = canonical_version(&v);
                if !cv.is_empty() {
                    return Some(cv);
                }
                invalid(&v, "must be of the form v1.2.3".to_string())
            }
        };
        self.error_at(line, verb, path, err.to_string());
        None
    }

    /// `parseReplace`
    fn replace(&mut self, line: &Line, verb: &str, args: &[String]) -> Option<Replace> {
        let arrow = if args.len() >= 2 && args[1] == "=>" {
            1
        } else {
            2
        };
        if args.len() < arrow + 2 || args.len() > arrow + 3 || args[arrow] != "=>" {
            self.error(
                line,
                format!(
                    "usage: {verb} module/path [v1.2.3] => other/module v1.4\n\t or {verb} module/path [v1.2.3] => ../local/directory"
                ),
            );
            return None;
        }
        let old_path = match parse_string(&args[0]) {
            Ok(p) => p,
            Err(e) => {
                self.error(line, format!("invalid quoted string: {e}"));
                return None;
            }
        };
        let Some((_, path_major)) = split_path_version(&old_path) else {
            self.error_at(line, verb, &old_path, "invalid module path".to_string());
            return None;
        };
        let mut old_version = String::new();
        if arrow == 2 {
            old_version = self.version(line, verb, &old_path, &args[1])?;
            if let Err(e) = check_path_major(&old_version, path_major) {
                self.error_at(line, verb, &old_path, e.to_string());
                return None;
            }
        }
        let new_path = match parse_string(&args[arrow + 1]) {
            Ok(p) => p,
            Err(e) => {
                self.error(line, format!("invalid quoted string: {e}"));
                return None;
            }
        };
        let mut new_version = String::new();
        if args.len() == arrow + 2 {
            if !is_directory_path(&new_path) {
                let message = if new_path.contains('@') {
                    "replacement module must match format 'path version', not 'path@version'"
                } else {
                    "replacement module without version must be directory path (rooted or starting with . or ..)"
                };
                self.error(line, message.to_string());
                return None;
            }
            if new_path.contains('\\') {
                self.error(
                    line,
                    "replacement directory appears to be Windows path (on a non-windows system)"
                        .to_string(),
                );
                return None;
            }
        }
        if args.len() == arrow + 3 {
            new_version = self.version(line, verb, &new_path, &args[arrow + 2])?;
            if is_directory_path(&new_path) {
                self.error(
                    line,
                    format!(
                        "replacement module directory path {} cannot have version",
                        gostd::strconv::quote(&new_path)
                    ),
                );
                return None;
            }
        }
        Some(Replace {
            old: Version {
                path: old_path,
                version: old_version,
            },
            new: Version {
                path: new_path,
                version: new_version,
            },
        })
    }

    /// `parseVersionInterval`, with versions taken as they are written
    /// (x/mod's `dontFixRetract`): `v` or `[low, high]`, consuming it from
    /// `args`
    fn version_interval(
        &mut self,
        line: &Line,
        verb: &str,
        args: &mut &[String],
    ) -> Option<(String, String)> {
        let toks = *args;
        let fail = |d: &mut Self, message: &str| {
            d.error(line, message.to_string());
            None
        };
        let version = |d: &mut Self, token: &str| match parse_string(token) {
            Ok(v) => Some(v),
            Err(e) => {
                d.error_at(
                    line,
                    verb,
                    "",
                    module::Error::InvalidVersion {
                        version: token.to_string(),
                        reason: e,
                    }
                    .to_string(),
                );
                None
            }
        };
        match toks.first().map(String::as_str) {
            None | Some("(") => return fail(self, "expected '[' or version"),
            Some("[") => {}
            Some(v) => {
                let v = version(self, v)?;
                *args = &toks[1..];
                return Some((v.clone(), v));
            }
        }
        let toks = &toks[1..];
        let Some(low) = toks.first() else {
            return fail(self, "expected version after '['");
        };
        let low = version(self, low)?;
        let toks = &toks[1..];
        if toks.first().map(String::as_str) != Some(",") {
            return fail(self, "expected ',' after version");
        }
        let toks = &toks[1..];
        let Some(high) = toks.first() else {
            return fail(self, "expected version after ','");
        };
        let high = version(self, high)?;
        let toks = &toks[1..];
        if toks.first().map(String::as_str) != Some("]") {
            return fail(self, "expected ']' after version");
        }
        *args = &toks[1..];
        Some((low, high))
    }
}

/// `isIndirect`: the line's suffix comment is `// indirect`, or starts
/// with `// indirect;` and says more
fn is_indirect(line: &Line) -> bool {
    let Some(comment) = &line.suffix else {
        return false;
    };
    let text = comment.token.strip_prefix("//").unwrap_or(&comment.token);
    let fields: Vec<&str> = text
        .split(gostd::unicode::is_space)
        .filter(|f| !f.is_empty())
        .collect();
    (fields.len() == 1 && fields[0] == "indirect") || (fields.len() > 1 && fields[0] == "indirect;")
}

/// `parseString`: a token's value. An interpreted string is unquoted; any
/// other quote is reserved, so a token holding one is an error.
fn parse_string(token: &str) -> Result<String, String> {
    if token.starts_with('"') {
        return gostd::strconv::unquote(token).map_err(|e| e.to_string());
    }
    if token.contains(['"', '\'', '`']) {
        return Err("unquoted string cannot contain quote".to_string());
    }
    Ok(token.to_string())
}

/// `modfile.GoVersionRE`:
/// `^([1-9][0-9]*)\.(0|[1-9][0-9]*)(\.(0|[1-9][0-9]*))?([a-z]+[0-9]+)?$`
fn is_go_version(v: &str) -> bool {
    /// A number without a leading zero (or "0" when `zero` allows it),
    /// and the rest
    fn number(s: &str, zero: bool) -> Option<&str> {
        let n = s.bytes().take_while(u8::is_ascii_digit).count();
        match s.as_bytes().first() {
            Some(b'0') if zero && n == 1 => Some(&s[1..]),
            Some(b'1'..=b'9') => Some(&s[n..]),
            _ => None,
        }
    }
    let Some(rest) = number(v, false) else {
        return false;
    };
    let Some(rest) = rest.strip_prefix('.').and_then(|r| number(r, true)) else {
        return false;
    };
    let rest = match rest.strip_prefix('.') {
        Some(r) => match number(r, true) {
            Some(r) => r,
            None => return false,
        },
        None => rest,
    };
    if rest.is_empty() {
        return true;
    }
    // A prerelease: lower-case letters, then digits
    let letters = rest.bytes().take_while(u8::is_ascii_lowercase).count();
    let digits = rest[letters..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    letters > 0 && digits > 0 && letters + digits == rest.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requires(src: &str) -> Vec<(String, String, bool)> {
        parse_mod("go.mod", src)
            .unwrap()
            .require
            .into_iter()
            .map(|r| (r.module.path, r.module.version, r.indirect))
            .collect()
    }

    fn req(path: &str, version: &str, indirect: bool) -> (String, String, bool) {
        (path.to_string(), version.to_string(), indirect)
    }

    #[test]
    fn reads_the_module_and_its_requirements() {
        let f = parse_mod(
            "go.mod",
            "module example.com/m\n\ngo 1.21\ntoolchain go1.22.1\n\nrequire (\n\tgithub.com/a/b v1.0.0\n\tgithub.com/c/d/v2 v2.3.0 // indirect\n)\n\nrequire \"github.com/e/f\" v4.0.0+incompatible\n",
        )
        .unwrap();
        assert_eq!(f.module.as_deref(), Some("example.com/m"));
        assert_eq!(f.go.as_deref(), Some("1.21"));
        assert_eq!(f.toolchain.as_deref(), Some("go1.22.1"));
        assert_eq!(
            requires(
                "module example.com/m\n\nrequire (\n\tgithub.com/a/b v1.0.0\n\tgithub.com/c/d/v2 v2.3.0 // indirect\n)\n\nrequire \"github.com/e/f\" v4.0.0+incompatible\n"
            ),
            vec![
                req("github.com/a/b", "v1.0.0", false),
                req("github.com/c/d/v2", "v2.3.0", true),
                req("github.com/e/f", "v4.0.0+incompatible", false),
            ]
        );
    }

    #[test]
    fn indirect_is_the_whole_comment_or_its_first_field_with_a_semicolon() {
        let cases = [
            ("// indirect", true),
            ("//indirect", true),
            ("//  indirect  ", true),
            ("// indirect; more", true),
            ("// indirect;", false),
            ("// indirect more", false),
            ("// not indirect", false),
            ("// Indirect", false),
        ];
        for (comment, want) in cases {
            let got = requires(&format!("module m\nrequire a.com/b v1.0.0 {comment}\n"));
            assert_eq!(got, vec![req("a.com/b", "v1.0.0", want)], "{comment:?}");
        }
        // A comment on its own line is no suffix
        let got = requires("module m\nrequire (\n\ta.com/b v1.0.0\n\t// indirect\n)\n");
        assert_eq!(got, vec![req("a.com/b", "v1.0.0", false)]);
    }

    #[test]
    fn reads_replacements() {
        let f = parse_mod(
            "go.mod",
            "module m\nreplace (\n\ta.com/x => ../x\n\ta.com/y v1.0.0 => b.com/y v1.1.0\n\ta.com/z => \"./z z\"\n)\n",
        )
        .unwrap();
        let got: Vec<_> = f
            .replace
            .iter()
            .map(|r| {
                (
                    r.old.path.as_str(),
                    r.old.version.as_str(),
                    r.new.path.as_str(),
                    r.new.version.as_str(),
                )
            })
            .collect();
        assert_eq!(
            got,
            vec![
                ("a.com/x", "", "../x", ""),
                ("a.com/y", "v1.0.0", "b.com/y", "v1.1.0"),
                ("a.com/z", "", "./z z", ""),
            ]
        );
    }

    #[test]
    fn reads_a_work_file() {
        let f = parse_work(
            "go.work",
            "go 1.22\n\nuse (\n\t./a\n\t\"./b c\"\n)\nuse ./d\n\nreplace example.com/lib => ./lib\n",
        )
        .unwrap();
        assert_eq!(f.go.as_deref(), Some("1.22"));
        assert_eq!(f.use_dirs, vec!["./a", "./b c", "./d"]);
        assert_eq!(f.replace[0].new.path, "./lib");
    }

    #[test]
    fn accepts_every_directive_and_empty_blocks() {
        let src = "module m\ngo 1.22rc1\ngodebug (\n\tpanicnil=1\n)\nrequire ()\nexclude a.com/b v1.0.0\nretract v1.0.1 // oops\nretract [v1.1.0, v1.2.0]\ntool a.com/b/cmd\nignore ./node_modules\n";
        let f = parse_mod("go.mod", src).unwrap();
        assert_eq!(f.godebug, vec![("panicnil".to_string(), "1".to_string())]);
        assert_eq!(f.exclude.len(), 1);
        assert_eq!(
            f.retract,
            vec![
                ("v1.0.1".to_string(), "v1.0.1".to_string()),
                ("v1.1.0".to_string(), "v1.2.0".to_string())
            ]
        );
        assert_eq!(f.tool, vec!["a.com/b/cmd"]);
        assert_eq!(f.ignore, vec!["./node_modules"]);
    }

    #[test]
    fn rejects_invalid_directives() {
        let cases = [
            (
                "this is not valid go.mod syntax at all",
                "go.mod:1: unknown directive: this",
            ),
            (
                "module a\nmodule b\n",
                "go.mod:2: repeated module statement",
            ),
            ("module\n", "go.mod:1: usage: module module/path"),
            ("go 1.21\ngo 1.22\n", "go.mod:2: repeated go statement"),
            (
                "go 1.021\n",
                "go.mod:1: invalid go version '1.021': must match format 1.23.0",
            ),
            (
                "toolchain go2\n",
                "go.mod:1: invalid toolchain version 'go2': must match format go1.23.0 or default",
            ),
            (
                "require a.com/b\n",
                "go.mod:1: usage: require module/path v1.2.3",
            ),
            (
                "require a.com/b 1.0.0\n",
                "go.mod:1: require a.com/b: version \"1.0.0\" invalid: must be of the form v1.2.3",
            ),
            ("require a.com/b v1.0\n", ""),
            (
                "require a.com/b v2.0.0\n",
                "go.mod:1: require a.com/b: version \"v2.0.0\" invalid: should be v0 or v1, not v2",
            ),
            (
                "require a.com/b/v1 v1.0.0\n",
                "go.mod:1: invalid module path",
            ),
            (
                "require 'a.com/b' v1.0.0\n",
                "go.mod:1: invalid quoted string: unquoted string cannot contain quote",
            ),
            (
                "require `a.com/b` v1.0.0\n",
                "go.mod:1: invalid quoted string: unquoted string cannot contain quote",
            ),
            (
                "require \"a\\q\" v1.0.0\n",
                "go.mod:1: invalid quoted string: invalid syntax",
            ),
            (
                "replace a.com/b => c.com/d\n",
                "go.mod:1: replacement module without version must be directory path (rooted or starting with . or ..)",
            ),
            (
                "replace a.com/b => c.com/d@v1.0.0\n",
                "go.mod:1: replacement module must match format 'path version', not 'path@version'",
            ),
            (
                "replace a.com/b => ./d v1.0.0\n",
                "go.mod:1: replacement module directory path \"./d\" cannot have version",
            ),
            (
                "replace a.com/b => .\\d\n",
                "go.mod:1: replacement directory appears to be Windows path (on a non-windows system)",
            ),
            (
                "replace a.com/b\n",
                "go.mod:1: usage: replace module/path [v1.2.3] => other/module v1.4\n\t or replace module/path [v1.2.3] => ../local/directory",
            ),
            ("retract\n", "go.mod:1: expected '[' or version"),
            (
                "retract [v1.0.0 v1.1.0]\n",
                "go.mod:1: expected ',' after version",
            ),
            (
                "retract v1.0.0 v1.1.0\n",
                "go.mod:1: unexpected token after version: \"v1.1.0\"",
            ),
            ("godebug a\n", "go.mod:1: usage: godebug key=value"),
            (
                "tool\n",
                "go.mod:1: tool directive expects exactly one argument",
            ),
            (
                "require a.com/b v1.0.0 x\nfoo\n",
                "go.mod:1: usage: require module/path v1.2.3\ngo.mod:2: unknown directive: foo",
            ),
            (
                "require a b (\n)\n",
                "go.mod:1: unknown block type: require a b",
            ),
            ("use (\n)\n", "go.mod:1: unknown block type: use"),
        ];
        for (src, want) in cases {
            let got = parse_mod("go.mod", src)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert_eq!(got, want, "{src:?}");
        }
        assert_eq!(
            parse_work(
                "go.work",
                "use ./a ./b\nrequire a.com/b v1.0.0\nmodule (\n)\n"
            )
            .unwrap_err()
            .to_string(),
            "go.work:1: usage: use local/dir\ngo.work:2: unknown directive: require\ngo.work:3: unknown block type: module"
        );
    }

    #[test]
    fn canonicalizes_versions() {
        assert_eq!(
            requires("module m\nrequire a.com/b v1.2\nrequire c.com/d v1.2.3+meta\n"),
            vec![
                req("a.com/b", "v1.2.0", false),
                req("c.com/d", "v1.2.3", false)
            ]
        );
    }

    #[test]
    fn go_versions() {
        for v in ["1.21", "1.21.0", "1.22rc1", "1.0", "10.20.30", "1.21beta2"] {
            assert!(is_go_version(v), "{v}");
        }
        for v in [
            "1", "01.2", "1.02", "1.2.03", "1.21rc", "1.21-rc1", "v1.21", "1.21RC1", "1.2.3.4", "",
        ] {
            assert!(!is_go_version(v), "{v}");
        }
    }

    #[test]
    fn directory_paths() {
        for p in [
            ".", "./a", ".\\a", "..", "../a", "..\\a", "/a", "\\a", "C:", "c:\\a",
        ] {
            assert!(is_directory_path(p), "{p}");
        }
        for p in ["a", ".a", "...", "a/b", "1:"] {
            assert!(!is_directory_path(p), "{p}");
        }
    }
}
