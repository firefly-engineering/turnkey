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
        (import_from_statement
          module_name: (dotted_name) @from_import)
        (import_from_statement
          module_name: (relative_import) @relative_import)
        "#,
    )?;

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
                let text = node.utf8_text(source.as_bytes())?;

                // Handle relative imports
                let (module_name, kind) = if capture.index == 3 {
                    // relative_import
                    (text.to_string(), ImportKind::Internal)
                } else {
                    classify_import(text)
                };

                // Deduplicate by package path (e.g., "python.cargo" from "python.cargo.toml")
                // This allows both "python.cfg" and "python.cargo" to be captured
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
/// "python.cargo.toml" -> "python.cargo"
/// "python.cfg" -> "python.cfg"
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
