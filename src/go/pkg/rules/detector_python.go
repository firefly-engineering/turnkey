package rules

import (
	"bufio"
	"os"
	"path/filepath"
	"regexp"
	"strings"
)

// PythonImportDetector detects imports from Python source files.
type PythonImportDetector struct {
	// ProjectRoot is the root directory of the project.
	ProjectRoot string

	// PackageName is the name of the current package (if any).
	PackageName string
}

// NewPythonImportDetector creates a new Python import detector.
func NewPythonImportDetector(projectRoot string) (*PythonImportDetector, error) {
	d := &PythonImportDetector{
		ProjectRoot: projectRoot,
	}

	return d, nil
}

// DetectImports detects all imports from Python source files in a directory.
func (d *PythonImportDetector) DetectImports(dir string) ([]Import, error) {
	var imports []Import

	// Find all Python files
	files, err := filepath.Glob(filepath.Join(dir, "*.py"))
	if err != nil {
		return nil, err
	}

	for _, file := range files {
		// Skip test files for main module deps
		baseName := filepath.Base(file)
		if strings.HasPrefix(baseName, "test_") || strings.HasSuffix(baseName, "_test.py") {
			continue
		}

		fileImports, err := d.detectFileImports(file)
		if err != nil {
			continue
		}

		imports = append(imports, fileImports...)
	}

	return deduplicateImports(imports), nil
}

// importPattern matches Python import statements.
// Examples:
//   - import os
//   - import six
//   - import numpy as np
var importPattern = regexp.MustCompile(`^\s*import\s+([a-zA-Z_][a-zA-Z0-9_]*)`)

// fromImportPattern matches Python from ... import statements.
// Examples:
//   - from os import path
//   - from six import PY3
//   - from typing import Optional
var fromImportPattern = regexp.MustCompile(`^\s*from\s+([a-zA-Z_][a-zA-Z0-9_]*)`)

// detectFileImports detects imports from a single Python file.
func (d *PythonImportDetector) detectFileImports(path string) ([]Import, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()

	var imports []Import
	scanner := bufio.NewScanner(file)
	lineNum := 0
	inMultilineString := false
	multilineDelimiter := ""

	relPath, _ := filepath.Rel(d.ProjectRoot, path)

	for scanner.Scan() {
		lineNum++
		line := scanner.Text()
		trimmed := strings.TrimSpace(line)

		// Handle multiline strings (docstrings)
		if inMultilineString {
			if strings.Contains(line, multilineDelimiter) {
				inMultilineString = false
			}
			continue
		}

		// Check for start of multiline string
		if strings.Contains(trimmed, `"""`) || strings.Contains(trimmed, `'''`) {
			delimiter := `"""`
			if strings.Contains(trimmed, `'''`) && (!strings.Contains(trimmed, `"""`) ||
				strings.Index(trimmed, `'''`) < strings.Index(trimmed, `"""`)) {
				delimiter = `'''`
			}
			// Count occurrences to see if it closes on the same line
			count := strings.Count(trimmed, delimiter)
			if count == 1 {
				inMultilineString = true
				multilineDelimiter = delimiter
				continue
			}
			// If it opens and closes on same line, we can still check for imports
			// but skip if the import appears inside the string
		}

		// Skip comments
		if strings.HasPrefix(trimmed, "#") {
			continue
		}

		// Remove inline comments for better parsing
		if idx := strings.Index(line, " #"); idx > 0 {
			line = line[:idx]
		}

		// Check for import statements - ensure it's at the start of a statement
		if matches := importPattern.FindStringSubmatch(line); len(matches) > 1 {
			// Verify it's not inside a string literal
			beforeMatch := line[:strings.Index(line, "import")]
			if !strings.Contains(beforeMatch, `"`) && !strings.Contains(beforeMatch, `'`) {
				moduleName := matches[1]
				imports = append(imports, Import{
					Path:       moduleName,
					SourceFile: relPath,
					Line:       lineNum,
					IsStdLib:   d.isStdLib(moduleName),
				})
			}
		}

		// Check for from ... import statements
		if matches := fromImportPattern.FindStringSubmatch(line); len(matches) > 1 {
			// Verify it's not inside a string literal
			beforeMatch := line[:strings.Index(line, "from")]
			if !strings.Contains(beforeMatch, `"`) && !strings.Contains(beforeMatch, `'`) {
				moduleName := matches[1]
				imports = append(imports, Import{
					Path:       moduleName,
					SourceFile: relPath,
					Line:       lineNum,
					IsStdLib:   d.isStdLib(moduleName),
				})
			}
		}
	}

	return imports, scanner.Err()
}

// isStdLib checks if a module is part of the Python standard library.
func (d *PythonImportDetector) isStdLib(moduleName string) bool {
	// Python standard library modules (Python 3.x)
	// This is not exhaustive but covers the most common ones
	stdLibModules := map[string]bool{
		// Built-in modules
		"abc": true, "aifc": true, "argparse": true, "array": true, "ast": true,
		"asynchat": true, "asyncio": true, "asyncore": true, "atexit": true,
		"audioop": true, "base64": true, "bdb": true, "binascii": true,
		"binhex": true, "bisect": true, "builtins": true, "bz2": true,
		"calendar": true, "cgi": true, "cgitb": true, "chunk": true,
		"cmath": true, "cmd": true, "code": true, "codecs": true,
		"codeop": true, "collections": true, "colorsys": true, "compileall": true,
		"concurrent": true, "configparser": true, "contextlib": true,
		"contextvars": true, "copy": true, "copyreg": true, "cProfile": true,
		"crypt": true, "csv": true, "ctypes": true, "curses": true,
		"dataclasses": true, "datetime": true, "dbm": true, "decimal": true,
		"difflib": true, "dis": true, "distutils": true, "doctest": true,
		"email": true, "encodings": true, "enum": true, "errno": true,
		"faulthandler": true, "fcntl": true, "filecmp": true, "fileinput": true,
		"fnmatch": true, "fractions": true, "ftplib": true, "functools": true,
		"gc": true, "getopt": true, "getpass": true, "gettext": true,
		"glob": true, "graphlib": true, "grp": true, "gzip": true,
		"hashlib": true, "heapq": true, "hmac": true, "html": true,
		"http": true, "idlelib": true, "imaplib": true, "imghdr": true,
		"imp": true, "importlib": true, "inspect": true, "io": true,
		"ipaddress": true, "itertools": true, "json": true, "keyword": true,
		"lib2to3": true, "linecache": true, "locale": true, "logging": true,
		"lzma": true, "mailbox": true, "mailcap": true, "marshal": true,
		"math": true, "mimetypes": true, "mmap": true, "modulefinder": true,
		"multiprocessing": true, "netrc": true, "nis": true, "nntplib": true,
		"numbers": true, "operator": true, "optparse": true, "os": true,
		"ossaudiodev": true, "pathlib": true, "pdb": true, "pickle": true,
		"pickletools": true, "pipes": true, "pkgutil": true, "platform": true,
		"plistlib": true, "poplib": true, "posix": true, "posixpath": true,
		"pprint": true, "profile": true, "pstats": true, "pty": true,
		"pwd": true, "py_compile": true, "pyclbr": true, "pydoc": true,
		"queue": true, "quopri": true, "random": true, "re": true,
		"readline": true, "reprlib": true, "resource": true, "rlcompleter": true,
		"runpy": true, "sched": true, "secrets": true, "select": true,
		"selectors": true, "shelve": true, "shlex": true, "shutil": true,
		"signal": true, "site": true, "smtpd": true, "smtplib": true,
		"sndhdr": true, "socket": true, "socketserver": true, "spwd": true,
		"sqlite3": true, "ssl": true, "stat": true, "statistics": true,
		"string": true, "stringprep": true, "struct": true, "subprocess": true,
		"sunau": true, "symtable": true, "sys": true, "sysconfig": true,
		"syslog": true, "tabnanny": true, "tarfile": true, "telnetlib": true,
		"tempfile": true, "termios": true, "test": true, "textwrap": true,
		"threading": true, "time": true, "timeit": true, "tkinter": true,
		"token": true, "tokenize": true, "tomllib": true, "trace": true,
		"traceback": true, "tracemalloc": true, "tty": true, "turtle": true,
		"turtledemo": true, "types": true, "typing": true, "unicodedata": true,
		"unittest": true, "urllib": true, "uu": true, "uuid": true,
		"venv": true, "warnings": true, "wave": true, "weakref": true,
		"webbrowser": true, "winreg": true, "winsound": true, "wsgiref": true,
		"xdrlib": true, "xml": true, "xmlrpc": true, "zipapp": true,
		"zipfile": true, "zipimport": true, "zlib": true, "zoneinfo": true,
		// Also handle relative imports
		"__future__": true, "__main__": true,
	}
	return stdLibModules[moduleName]
}

// IsInternalImport checks if a module is internal to the monorepo.
func (d *PythonImportDetector) IsInternalImport(moduleName string) bool {
	// Check if it's a local package by looking for __init__.py
	packageDir := filepath.Join(d.ProjectRoot, "src", "python", moduleName)
	if _, err := os.Stat(filepath.Join(packageDir, "__init__.py")); err == nil {
		return true
	}
	return false
}

// DetectTestImports detects imports from Python test files.
func (d *PythonImportDetector) DetectTestImports(dir string) ([]Import, error) {
	var imports []Import

	// Find test files
	patterns := []string{
		filepath.Join(dir, "test_*.py"),
		filepath.Join(dir, "*_test.py"),
		filepath.Join(dir, "tests", "*.py"),
	}

	for _, pattern := range patterns {
		files, err := filepath.Glob(pattern)
		if err != nil {
			continue
		}

		for _, file := range files {
			fileImports, err := d.detectFileImports(file)
			if err != nil {
				continue
			}
			imports = append(imports, fileImports...)
		}
	}

	return deduplicateImports(imports), nil
}
