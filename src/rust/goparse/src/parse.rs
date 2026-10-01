//! A Go file's header (package clause and imports), build constraint and
//! `//go:embed` patterns
//!
//! The file is parsed with `tree-sitter-go`, which recovers from errors
//! anywhere. `go/parser` in `ImportsOnly` mode, which the Go version used,
//! fails a file only on an error up to the token after its imports, so
//! only that part is checked here ([`Header`]): its tree must have no
//! error, the package clause and every import must be followed by a
//! semicolon (an explicit one, or a newline, as Go inserts them), and the
//! token after the imports must be one Go's scanner accepts. The comments
//! (build constraints, `//go:embed` directives) are found as Go's scanner
//! finds them ([`crate::comments`]), anywhere in the file.
//!
//! Known differences, on files no Go toolchain builds:
//!
//! - tree-sitter-go doesn't take a block comment spanning lines as a
//!   semicolon (`package a /* ...\n */ import "b"`), where Go does: such a
//!   file is an error here;
//! - a malformed number literal right after the imports (`0x`), which Go's
//!   scanner rejects, isn't checked;
//! - in an import path whose escapes aren't UTF-8 (`"a\xffb"`), the bytes
//!   that aren't UTF-8 are replaced with U+FFFD.

use crate::GoFile;
use crate::comments::{Comment, comments};
use gostd::constraint::{self, Expr};
use gostd::strconv::{quoted_prefix, unquote, unquote_bytes};
use gostd::unicode::{is_digit, is_letter};
use std::fmt;
use std::path::Path;
use tree_sitter::{Node, Parser, Tree};

/// Why a file was not read
#[derive(Debug)]
pub enum Error {
    /// The file couldn't be read
    Io(std::io::Error),
    /// Its header doesn't parse, an import path isn't a valid Go string,
    /// or a `//go:embed` directive is malformed
    Syntax(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Syntax(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for Error {}

/// Reads and parses the Go file at `path`
pub fn parse_file(path: &Path) -> Result<GoFile, Error> {
    let src = std::fs::read(path).map_err(Error::Io)?;
    parse_source(path, &src)
}

/// Parses `src`, the content of the Go file at `path`
pub fn parse_source(path: &Path, src: &[u8]) -> Result<GoFile, Error> {
    let syntax = |msg: String| Error::Syntax(format!("{}: {msg}", path.display()));
    let tree = parse_tree(src);
    let header = Header::read(&tree, src).map_err(syntax)?;

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut file = GoFile {
        path: path.to_path_buf(),
        package: header.package,
        is_test: name.ends_with("_test.go"),
        ..GoFile::default()
    };
    for lit in header.import_paths {
        // Go's import path is the literal's bytes, which escapes can make
        // invalid UTF-8 (go/parser doesn't check them): here those bytes
        // are replaced
        let import = unquote_bytes(&lit).map_err(|e| syntax(format!("import {lit}: {e}")))?;
        let import = String::from_utf8_lossy(&import).into_owned();
        file.has_cgo |= import == "C";
        file.imports.push(import);
    }

    let comments = comments(src);
    file.constraint = build_constraint(&comments, header.package_start);
    for c in &comments {
        let text = String::from_utf8_lossy(&c.text);
        let Some(args) = text.strip_prefix("//go:embed") else {
            continue;
        };
        if !args.is_empty() && !args.starts_with([' ', '\t']) {
            continue;
        }
        let patterns =
            parse_embed_patterns(args).map_err(|e| syntax(format!("invalid //go:embed: {e}")))?;
        file.embed_patterns.extend(patterns);
    }
    Ok(file)
}

/// The file's syntax tree
fn parse_tree(src: &[u8]) -> Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_go::LANGUAGE.into())
        .expect("tree-sitter-go is compatible with the tree-sitter version");
    parser
        .parse(src, None)
        .expect("tree-sitter parses without a timeout or cancellation")
}

/// What `go/parser` reads in `ImportsOnly` mode: the package clause and the
/// import declarations that follow it
struct Header {
    /// The package name
    package: String,
    /// Where the `package` keyword starts: the comments before it can be
    /// build constraints
    package_start: usize,
    /// The import paths, as written (quoted)
    import_paths: Vec<String>,
}

impl Header {
    fn read(tree: &Tree, src: &[u8]) -> Result<Header, String> {
        let root = tree.root_node();
        let mut cursor = root.walk();
        let children: Vec<Node> = root.children(&mut cursor).collect();
        let skippable = |n: &Node| n.kind() == "comment" || n.kind() == ";";

        let mut i = 0;
        while i < children.len() && children[i].kind() == "comment" {
            i += 1;
        }
        let package = children
            .get(i)
            .filter(|n| n.kind() == "package_clause")
            .copied()
            .ok_or("expected 'package'")?;
        i += 1;
        let mut decls = Vec::new();
        loop {
            let mut j = i;
            while j < children.len() && skippable(&children[j]) {
                j += 1;
            }
            if j < children.len() && children[j].kind() == "import_declaration" {
                decls.push(children[j]);
                i = j + 1;
            } else {
                break;
            }
        }
        // The token go/parser looks at after the imports, and stops at
        while i < children.len() && skippable(&children[i]) {
            i += 1;
        }
        let next = children.get(i).map_or(src.len(), |n| n.start_byte());

        check_text(src, next)?;
        if let Some(e) = first_error(root, next) {
            return Err(format!("syntax error at offset {}", e.start_byte()));
        }
        check_semicolon(src, package.end_byte(), false)?;
        let mut import_paths = Vec::new();
        for decl in &decls {
            for spec in children_of_kind(*decl, "import_spec").into_iter().chain(
                children_of_kind(*decl, "import_spec_list")
                    .into_iter()
                    .flat_map(|list| children_of_kind(list, "import_spec")),
            ) {
                let in_list = spec
                    .parent()
                    .is_some_and(|p| p.kind() == "import_spec_list");
                check_semicolon(src, spec.end_byte(), in_list)?;
                let path = spec
                    .child_by_field_name("path")
                    .ok_or("missing import path")?;
                import_paths.push(node_text(src, &path).into_owned());
            }
            check_semicolon(src, decl.end_byte(), false)?;
        }
        check_token(src, next)?;

        let name = children_of_kind(package, "package_identifier")
            .first()
            .map(|n| node_text(src, n).into_owned())
            .ok_or("expected the package name")?;
        let keyword = package
            .child(0)
            .map_or(package.start_byte(), |k| k.start_byte());
        Ok(Header {
            package: name,
            package_start: keyword,
            import_paths,
        })
    }
}

/// The node's children of the given kind
fn children_of_kind<'t>(node: Node<'t>, kind: &str) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|n| n.kind() == kind)
        .collect()
}

/// The first error or missing node of the tree that starts before `end`
/// (a missing one at `end` too: a token go/parser expected there)
fn first_error(node: Node, end: usize) -> Option<Node> {
    let start = node.start_byte();
    if start > end || !(node.has_error() || node.is_missing()) {
        return None;
    }
    if node.is_missing() {
        return Some(node);
    }
    if node.is_error() {
        // One at `end` is the token after the imports, which check_token
        // checks
        return (start < end).then_some(node);
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children.into_iter().find_map(|c| first_error(c, end))
}

/// Checks that the source up to `end` (the token after the imports) is
/// what Go's scanner accepts in any token or comment: UTF-8 without NUL,
/// and no byte order mark but at the start
fn check_text(src: &[u8], end: usize) -> Result<(), String> {
    let text = std::str::from_utf8(&src[..end]).map_err(|_| "illegal UTF-8 encoding")?;
    if text.contains('\0') {
        return Err("illegal character NUL".into());
    }
    if text.char_indices().any(|(i, c)| c == '\u{feff}' && i > 0) {
        return Err("illegal byte order mark".into());
    }
    Ok(())
}

/// Checks that a semicolon follows the header element ending at `pos`, as
/// Go reads one: an explicit `;`, a newline (in the space before the next
/// token, or in a comment there), the end of the file, or, for an import
/// in parentheses, the closing parenthesis
fn check_semicolon(src: &[u8], mut pos: usize, in_list: bool) -> Result<(), String> {
    let missing = |pos: usize| format!("expected ';' at offset {pos}");
    loop {
        match src.get(pos) {
            None | Some(b'\n') | Some(b';') => return Ok(()),
            Some(b')') if in_list => return Ok(()),
            Some(b' ' | b'\t' | b'\r') => pos += 1,
            Some(b'/') => match src.get(pos + 1) {
                // A line comment runs to the end of the line, or of the file
                Some(b'/') => return Ok(()),
                Some(b'*') => {
                    let body = &src[pos + 2..];
                    let len = body
                        .windows(2)
                        .position(|w| w == b"*/")
                        .ok_or("comment not terminated")?;
                    if body[..len].contains(&b'\n') {
                        return Ok(());
                    }
                    pos += 2 + len + 2;
                }
                _ => return Err(missing(pos)),
            },
            Some(_) => return Err(missing(pos)),
        }
    }
}

/// Checks that the token at `pos`, which go/parser scans after the
/// imports, is one Go's scanner accepts: an identifier or keyword, a
/// number, an operator, or a terminated string, raw string or character
/// literal
fn check_token(src: &[u8], pos: usize) -> Result<(), String> {
    if pos >= src.len() {
        return Ok(());
    }
    let rest = String::from_utf8_lossy(&src[pos..]);
    let c = rest.chars().next().expect("not at the end");
    let ok = match c {
        '"' | '`' => quoted_prefix(&rest).is_ok(),
        '\'' => quoted_prefix(&rest).is_ok_and(|lit| lit != "''"),
        // A comment tree-sitter-go didn't read as one is unterminated
        '/' => !rest.starts_with("/*") || rest[2..].contains("*/"),
        '0'..='9' | '_' => true,
        // go/parser parses an import declaration at an import keyword,
        // and tree-sitter-go read none there
        c if is_letter(c) => !is_keyword(&rest, "import"),
        c => "+-*/%&|^<>=!()[]{},;.:~".contains(c),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("illegal token at offset {pos}"))
    }
}

/// Whether `text` starts with the identifier or keyword `word`
fn is_keyword(text: &str, word: &str) -> bool {
    text.strip_prefix(word).is_some_and(|rest| {
        !rest
            .chars()
            .next()
            .is_some_and(|c| c == '_' || is_letter(c) || is_digit(c))
    })
}

/// The text of a node, invalid UTF-8 replaced
fn node_text<'s>(src: &'s [u8], node: &Node) -> std::borrow::Cow<'s, str> {
    String::from_utf8_lossy(&src[node.byte_range()])
}

/// The file's build constraint: the last `//go:build` line before the
/// package clause, or else every `// +build` line there, ANDed. A line
/// whose expression doesn't parse is ignored.
fn build_constraint(comments: &[Comment], package_start: usize) -> Option<Expr> {
    let mut go_build = None;
    let mut plus_build: Option<Expr> = None;
    for c in comments.iter().filter(|c| c.start < package_start) {
        // The header is UTF-8 (Header::read checks it)
        let text = String::from_utf8_lossy(&c.text);
        let Ok(expr) = constraint::parse(&text) else {
            continue;
        };
        if constraint::is_go_build(&text) {
            go_build = Some(expr);
        } else if constraint::is_plus_build(&text) {
            plus_build = Some(match plus_build {
                None => expr,
                Some(x) => constraint::and(x, expr),
            });
        }
    }
    go_build.or(plus_build)
}

/// Splits a `//go:embed` directive's arguments as the go command does:
/// separated by spaces, each either bare or a Go string literal in double
/// quotes or backquotes
fn parse_embed_patterns(args: &str) -> Result<Vec<String>, String> {
    let mut patterns = Vec::new();
    let mut args = args;
    loop {
        args = args.trim_start_matches([' ', '\t']);
        if args.is_empty() {
            return Ok(patterns);
        }
        let pattern;
        if args.starts_with(['"', '`']) {
            let quoted = quoted_prefix(args)
                .map_err(|_| format!("unterminated or malformed string {args}"))?;
            // A quoted prefix unquotes; one with an escape that isn't
            // UTF-8 reads as the bytes Go would hold, replaced
            pattern = unquote(quoted).unwrap_or_else(|_| quoted.to_string());
            args = &args[quoted.len()..];
            if !args.is_empty() && !args.starts_with([' ', '\t']) {
                return Err(format!("missing space after {quoted}"));
            }
        } else {
            let end = args.find([' ', '\t']).unwrap_or(args.len());
            pattern = args[..end].to_string();
            args = &args[end..];
        }
        patterns.push(pattern);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Result<GoFile, Error> {
        parse_source(Path::new("dir/test.go"), src.as_bytes())
    }

    #[test]
    fn parse_file_reads_the_header_constraint_and_embeds() {
        let gf = parse(
            "//go:build linux && amd64\npackage testpkg\nimport (\n\t\"fmt\"\n\t\"os\"\n\t\"C\"\n)\n//go:embed testdata/*\nvar data []byte\n",
        )
        .unwrap();
        assert_eq!(gf.package, "testpkg");
        assert_eq!(gf.imports, ["fmt", "os", "C"]);
        assert!(gf.has_cgo);
        assert_eq!(
            gf.constraint.map(|c| c.to_string()).as_deref(),
            Some("linux && amd64")
        );
        assert_eq!(gf.embed_patterns, ["testdata/*"]);
        assert!(!gf.is_test);
        assert!(
            parse_source(Path::new("a_test.go"), b"package a\n")
                .unwrap()
                .is_test
        );
    }

    #[test]
    fn embed_patterns() {
        let gf = parse(concat!(
            "package testpkg\n",
            "import \"embed\"\n",
            "//go:embed static/*.html \"my file.txt\" `raw dir`\n",
            "var files embed.FS\n",
            "//go:embed\tversion.txt\n",
            "var version string\n",
            "// see //go:embed notadirective\n",
            "var s = \"//go:embed instring\"\n",
            "/* //go:embed inblock */\n",
            "//go:embedded notadirective\n",
            "//go:embed \"crlf.txt\"\r\n",
        ))
        .unwrap();
        assert_eq!(
            gf.embed_patterns,
            [
                "static/*.html",
                "my file.txt",
                "raw dir",
                "version.txt",
                "crlf.txt"
            ]
        );
    }

    #[test]
    fn a_malformed_embed_is_an_error() {
        for src in [
            "package testpkg\n//go:embed \"unterminated\nvar b []byte\n",
            "package testpkg\n//go:embed \"a\"b\nvar b []byte\n",
            "package testpkg\n//go:embed \"\\q\"\nvar b []byte\n",
        ] {
            assert!(parse(src).is_err(), "{src:?}");
        }
    }

    #[test]
    fn raw_string_import() {
        let gf = parse("package testpkg\nimport (\n\t\"fmt\"\n\t`example.com/foo`\n)\n").unwrap();
        assert_eq!(gf.imports, ["fmt", "example.com/foo"]);
    }

    /// Named, dot and blank imports, several declarations, semicolons, and
    /// imports after the first other declaration, which go/parser in
    /// ImportsOnly mode doesn't reach
    #[test]
    fn imports_as_go_parser_reads_them() {
        let gf = parse(
            "// Package doc\npackage a; import x \"x\"; import (. \"dot\"; _ \"blank\")\nimport \"\\x61\"\n\nfunc f() {}\nimport \"late\"\n",
        )
        .unwrap();
        assert_eq!(gf.imports, ["x", "dot", "blank", "a"]);
        // Escapes need not make UTF-8
        let gf = parse("package a\nimport \"a\\xffb\"\n").unwrap();
        assert_eq!(gf.imports, ["a\u{fffd}b"]);
    }

    /// The file's body can have any error: only its header is parsed
    #[test]
    fn errors_after_the_imports_dont_count() {
        for src in [
            "package a\nimport \"fmt\"\nfunc f() { $$$ }\n",
            "package a\nimport \"fmt\"\nfunc f() {\n",
            "package a\nfunc f() { x := `unterminated\n}\n",
            "package a\n\nvar x = 1 +\n",
            "package a\n\ntype A[T any] = []T\n",
            "\u{feff}package a\n",
            "package a\nimport (\n\t\"a\" // c\n\t\"b\" /* c */\n)\n",
            "package a /* c */\n",
            "package a\nimport \"a\"\nvar s = \"unterminated\n",
            "package a",
            "package a\nimporter := 1\n",
        ] {
            assert!(parse(src).is_ok(), "{src:?}: {:?}", parse(src));
        }
    }

    /// An error up to the token after the imports fails the file
    #[test]
    fn header_errors() {
        for src in [
            "",
            "// only a comment\n",
            "package\n",
            "package a b\n",
            "packag a\n",
            "package a\nimport x\n",
            "package a\nimport\n",
            "package a\nimport \"a\" func f() {}\n",
            "package a import \"a\"\n",
            "package a\nimport (\n\t\"a\" \"b\"\n)\n",
            "package a\nimport \"\\q\"\n",
            "package a\nimport \"a\"\n@\n",
            "package a\nimport \"a\"\n/* unterminated\n",
            "package a\nimport \"a\"\n\"unterminated\n",
            "package a\nimport \"a\"\n''\n",
            "package a\x00\n",
            "package a\n// \u{feff}\n",
            "package a\nimport (\n\t\"a\"\n",
            "package a\nfunc f() { $$$ \n//go:embed \"bad\n}\n",
        ] {
            assert!(parse(src).is_err(), "{src:?}");
        }
        assert!(parse_source(Path::new("x.go"), b"package a\n// \xff\n").is_err());
        assert!(parse_source(Path::new("x.go"), b"package a\nfunc f() {}\n// \xff\n").is_ok());
    }

    #[test]
    fn build_constraints() {
        let constraint = |src: &str| parse(src).unwrap().constraint.map(|c| c.to_string());
        // A //go:build line wins over // +build lines; the last one wins
        assert_eq!(
            constraint("// +build old\n//go:build a\n//go:build b || c\n\npackage p\n").as_deref(),
            Some("b || c")
        );
        // // +build lines are ANDed
        assert_eq!(
            constraint("// +build a b\n// +build !c\n\npackage p\n").as_deref(),
            Some("(a || b) && !c")
        );
        // Lines that don't parse, after the package clause, or in a block
        // comment are ignored
        assert_eq!(
            constraint("//go:build (a\n/* //go:build x */\npackage p\n//go:build y\n"),
            None
        );
        assert_eq!(
            constraint("//go:build a\r\n\r\npackage p\r\n").as_deref(),
            Some("a")
        );
    }

    #[test]
    fn constraint_tags() {
        let gf = parse("//go:build (cgo || netgo) && !arm64 && cgo\n\npackage b\n").unwrap();
        assert_eq!(gf.constraint_tags(), ["arm64", "cgo", "netgo"]);
    }
}
