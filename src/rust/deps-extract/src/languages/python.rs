//! Python import extraction using tree-sitter.

use crate::extraction::{Import, ImportKind, Package, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};
use walkdir::WalkDir;

/// Python standard library modules (common subset).
/// This list covers the most common stdlib modules.
const PYTHON_STDLIB: &[&str] = &[
    "abc", "aifc", "argparse", "array", "ast", "asyncio", "atexit", "base64",
    "bdb", "binascii", "bisect", "builtins", "bz2", "calendar", "cgi", "cgitb",
    "chunk", "cmath", "cmd", "code", "codecs", "codeop", "collections",
    "colorsys", "compileall", "concurrent", "configparser", "contextlib",
    "contextvars", "copy", "copyreg", "cProfile", "crypt", "csv", "ctypes",
    "curses", "dataclasses", "datetime", "dbm", "decimal", "difflib", "dis",
    "distutils", "doctest", "email", "encodings", "enum", "errno",
    "faulthandler", "fcntl", "filecmp", "fileinput", "fnmatch", "fractions",
    "ftplib", "functools", "gc", "getopt", "getpass", "gettext", "glob",
    "graphlib", "grp", "gzip", "hashlib", "heapq", "hmac", "html", "http",
    "idlelib", "imaplib", "imghdr", "imp", "importlib", "inspect", "io",
    "ipaddress", "itertools", "json", "keyword", "lib2to3", "linecache",
    "locale", "logging", "lzma", "mailbox", "mailcap", "marshal", "math",
    "mimetypes", "mmap", "modulefinder", "multiprocessing", "netrc", "nis",
    "nntplib", "numbers", "operator", "optparse", "os", "ossaudiodev",
    "pathlib", "pdb", "pickle", "pickletools", "pipes", "pkgutil", "platform",
    "plistlib", "poplib", "posix", "posixpath", "pprint", "profile", "pstats",
    "pty", "pwd", "py_compile", "pyclbr", "pydoc", "queue", "quopri", "random",
    "re", "readline", "reprlib", "resource", "rlcompleter", "runpy", "sched",
    "secrets", "select", "selectors", "shelve", "shlex", "shutil", "signal",
    "site", "smtpd", "smtplib", "sndhdr", "socket", "socketserver", "spwd",
    "sqlite3", "ssl", "stat", "statistics", "string", "stringprep", "struct",
    "subprocess", "sunau", "symtable", "sys", "sysconfig", "syslog", "tabnanny",
    "tarfile", "telnetlib", "tempfile", "termios", "test", "textwrap",
    "threading", "time", "timeit", "tkinter", "token", "tokenize", "tomllib",
    "trace", "traceback", "tracemalloc", "tty", "turtle", "turtledemo", "types",
    "typing", "unicodedata", "unittest", "urllib", "uu", "uuid", "venv",
    "warnings", "wave", "weakref", "webbrowser", "winreg", "winsound", "wsgiref",
    "xdrlib", "xml", "xmlrpc", "zipapp", "zipfile", "zipimport", "zlib",
    "zoneinfo", "_thread", "__future__",
];

/// Default directories to exclude.
const DEFAULT_EXCLUDES: &[&str] = &[
    "venv", "__pycache__", ".venv", "build", "dist", ".egg-info", "node_modules",
    ".git", ".hg", ".svn",
];

/// Extract Python imports from a directory.
pub fn extract(dir: &Path, exclude_patterns: &[&str]) -> anyhow::Result<Result> {
    let mut result = Result::new("python");

    let mut parser = Parser::new();
    let language = tree_sitter_python::LANGUAGE;
    parser.set_language(&language.into())?;

    // Query for import statements
    // import_statement: import foo, import foo.bar, import foo as f
    // import_from_statement: from foo import bar, from . import foo
    let query = Query::new(
        &language.into(),
        r#"
        (import_statement
          name: (dotted_name) @import)
        (import_statement
          name: (aliased_import
            name: (dotted_name) @import))
        (import_from_statement) @from_import
        "#,
    )?;
    let from_import = query
        .capture_index_for_name("from_import")
        .expect("query captures from_import");

    let abs_dir = dir.canonicalize()?;
    let mut packages: HashMap<String, Package> = HashMap::new();

    for entry in WalkDir::new(&abs_dir)
        .into_iter()
        .filter_entry(|e| !should_exclude(e.file_name().to_str().unwrap_or(""), exclude_patterns))
    {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let extension = path.extension().and_then(|e| e.to_str());
        if extension != Some("py") {
            continue;
        }

        let rel_path = path.strip_prefix(&abs_dir)?;
        let pkg_dir = rel_path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let filename = rel_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();

        // Determine if this is a test file
        let is_test_file = filename.starts_with("test_")
            || filename.ends_with("_test.py")
            || filename == "conftest.py"
            || pkg_dir.contains("test");

        // Parse the file
        let source = std::fs::read_to_string(path)?;
        let tree = match parser.parse(&source, None) {
            Some(t) => t,
            None => {
                result.add_error(format!("Failed to parse {}", rel_path.display()));
                continue;
            }
        };

        // Extract imports
        let mut cursor = QueryCursor::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut imports = Vec::new();

        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        while let Some(match_) = matches.next() {
            for capture in match_.captures {
                let node = capture.node;
                let modules = if capture.index == from_import {
                    from_import_modules(node, source.as_bytes())?
                } else {
                    let text = node.utf8_text(source.as_bytes())?;
                    vec![classify_import(text)]
                };

                for (module_name, kind) in modules {
                    // Deduplicate by package path (e.g., "turnkey.cargo" from
                    // "turnkey.cargo.toml"), so both "turnkey.cfg" and
                    // "turnkey.cargo" are captured
                    let pkg_path = get_python_package_path(&module_name);

                    if !seen.contains(&pkg_path) {
                        seen.insert(pkg_path);
                        imports.push(Import {
                            path: module_name,
                            kind,
                        });
                    }
                }
            }
        }

        // Add to package
        let pkg = packages.entry(pkg_dir.clone()).or_insert_with(|| Package::new(pkg_dir));
        pkg.files.push(filename);

        for imp in imports {
            if is_test_file {
                if !pkg.test_imports.contains(&imp) {
                    pkg.test_imports.push(imp);
                }
            } else if !pkg.imports.contains(&imp) {
                pkg.imports.push(imp);
            }
        }
    }

    // Sort and add packages to result
    let mut pkgs: Vec<_> = packages.into_values().collect();
    pkgs.sort_by(|a, b| a.path.cmp(&b.path));

    for mut pkg in pkgs {
        pkg.imports.sort_by(|a, b| a.path.cmp(&b.path));
        pkg.test_imports.sort_by(|a, b| a.path.cmp(&b.path));
        result.add_package(pkg);
    }

    Ok(result)
}

/// The modules a `from <module> import <names>` statement imports.
///
/// Each name may be a submodule, so `from turnkey import cfg` yields
/// `turnkey.cfg`: under a namespace package the module alone doesn't say
/// which package provides the name. A wildcard import yields the module. A
/// relative import (`from . import x`, `from .x import y`) yields the
/// relative module, classified internal.
fn from_import_modules(
    stmt: tree_sitter::Node,
    source: &[u8],
) -> anyhow::Result<Vec<(String, ImportKind)>> {
    let Some(module) = stmt.child_by_field_name("module_name") else {
        return Ok(Vec::new());
    };
    let module_text = module.utf8_text(source)?;
    if module.kind() == "relative_import" {
        return Ok(vec![(module_text.to_string(), ImportKind::Internal)]);
    }

    let mut walker = stmt.walk();
    let mut modules = Vec::new();
    for name in stmt.children_by_field_name("name", &mut walker) {
        let name = if name.kind() == "aliased_import" {
            match name.child_by_field_name("name") {
                Some(n) => n,
                None => continue,
            }
        } else {
            name
        };
        let name_text = name.utf8_text(source)?;
        modules.push(classify_import(&format!("{module_text}.{name_text}")));
    }
    if modules.is_empty() {
        modules.push(classify_import(module_text));
    }
    Ok(modules)
}

/// Check if a directory/file should be excluded.
fn should_exclude(name: &str, patterns: &[&str]) -> bool {
    if name.starts_with('.') {
        return true;
    }

    for pattern in DEFAULT_EXCLUDES.iter().chain(patterns.iter()) {
        if name == *pattern || name.contains(pattern) {
            return true;
        }
    }

    false
}

/// Get the package path for deduplication purposes.
/// "turnkey.cargo.toml" -> "turnkey.cargo"
/// "turnkey.cfg" -> "turnkey.cfg"
/// "requests" -> "requests"
fn get_python_package_path(module: &str) -> String {
    let parts: Vec<&str> = module.split('.').collect();
    if parts.len() <= 2 {
        // Single module or two-level: use as-is
        module.to_string()
    } else {
        // Three or more levels: use first two (package path)
        format!("{}.{}", parts[0], parts[1])
    }
}

/// Classify a Python import as stdlib, external, or internal.
fn classify_import(module: &str) -> (String, ImportKind) {
    // Get the top-level module
    let top_level = module.split('.').next().unwrap_or(module);

    // Check if it's stdlib
    if PYTHON_STDLIB.contains(&top_level) {
        return (module.to_string(), ImportKind::Stdlib);
    }

    // Check for relative imports (start with .)
    if module.starts_with('.') {
        return (module.to_string(), ImportKind::Internal);
    }

    // Default to external
    (module.to_string(), ImportKind::External)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Extracts the imports of one module, `source`.
    fn imports_of(source: &str) -> Vec<(String, ImportKind)> {
        let dir = std::env::temp_dir().join(format!(
            "deps-extract-python-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mod.py"), source).unwrap();
        let result = extract(&dir, &[]);
        std::fs::remove_dir_all(&dir).unwrap();

        let result = result.unwrap();
        result.packages[0]
            .imports
            .iter()
            .map(|i| (i.path.clone(), i.kind.clone()))
            .collect()
    }

    #[test]
    fn from_import_qualifies_each_name() {
        assert_eq!(
            imports_of("from turnkey import cfg, cargo as c\n"),
            vec![
                ("turnkey.cargo".to_string(), ImportKind::External),
                ("turnkey.cfg".to_string(), ImportKind::External),
            ]
        );
    }

    #[test]
    fn from_import_of_a_submodule_keeps_its_package() {
        assert_eq!(
            imports_of("from turnkey.cargo.toml import parse\nimport requests\n"),
            vec![
                ("requests".to_string(), ImportKind::External),
                ("turnkey.cargo.toml.parse".to_string(), ImportKind::External),
            ]
        );
    }

    #[test]
    fn wildcard_and_relative_imports() {
        assert_eq!(
            imports_of("from os.path import *\nfrom . import sibling\nfrom .x import y\n"),
            vec![
                (".".to_string(), ImportKind::Internal),
                (".x".to_string(), ImportKind::Internal),
                ("os.path".to_string(), ImportKind::Stdlib),
            ]
        );
    }
}
