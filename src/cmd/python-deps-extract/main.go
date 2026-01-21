// python-deps-extract extracts Python module dependencies and outputs the extraction protocol JSON.
//
// Usage:
//
//	python-deps-extract [flags] [dir]
//
// By default, it analyzes the current directory. If a directory is provided,
// it analyzes Python packages in that directory.
//
// Flags:
//
//	-o string
//	    Output file path (default: stdout)
//	-exclude string
//	    Comma-separated list of directory patterns to exclude (e.g., "venv,__pycache__,test")
package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/extraction"
)

// Python stdlib modules (common subset)
var pythonStdlib = map[string]bool{
	"abc": true, "aifc": true, "argparse": true, "array": true, "ast": true,
	"asyncio": true, "atexit": true, "base64": true, "bdb": true, "binascii": true,
	"bisect": true, "builtins": true, "bz2": true, "calendar": true, "cgi": true,
	"cgitb": true, "chunk": true, "cmath": true, "cmd": true, "code": true,
	"codecs": true, "codeop": true, "collections": true, "colorsys": true,
	"compileall": true, "concurrent": true, "configparser": true, "contextlib": true,
	"contextvars": true, "copy": true, "copyreg": true, "cProfile": true, "crypt": true,
	"csv": true, "ctypes": true, "curses": true, "dataclasses": true, "datetime": true,
	"dbm": true, "decimal": true, "difflib": true, "dis": true, "distutils": true,
	"doctest": true, "email": true, "encodings": true, "enum": true, "errno": true,
	"faulthandler": true, "fcntl": true, "filecmp": true, "fileinput": true,
	"fnmatch": true, "fractions": true, "ftplib": true, "functools": true, "gc": true,
	"getopt": true, "getpass": true, "gettext": true, "glob": true, "graphlib": true,
	"grp": true, "gzip": true, "hashlib": true, "heapq": true, "hmac": true,
	"html": true, "http": true, "idlelib": true, "imaplib": true, "imghdr": true,
	"imp": true, "importlib": true, "inspect": true, "io": true, "ipaddress": true,
	"itertools": true, "json": true, "keyword": true, "lib2to3": true, "linecache": true,
	"locale": true, "logging": true, "lzma": true, "mailbox": true, "mailcap": true,
	"marshal": true, "math": true, "mimetypes": true, "mmap": true, "modulefinder": true,
	"multiprocessing": true, "netrc": true, "nis": true, "nntplib": true, "numbers": true,
	"operator": true, "optparse": true, "os": true, "ossaudiodev": true, "pathlib": true,
	"pdb": true, "pickle": true, "pickletools": true, "pipes": true, "pkgutil": true,
	"platform": true, "plistlib": true, "poplib": true, "posix": true, "posixpath": true,
	"pprint": true, "profile": true, "pstats": true, "pty": true, "pwd": true,
	"py_compile": true, "pyclbr": true, "pydoc": true, "queue": true, "quopri": true,
	"random": true, "re": true, "readline": true, "reprlib": true, "resource": true,
	"rlcompleter": true, "runpy": true, "sched": true, "secrets": true, "select": true,
	"selectors": true, "shelve": true, "shlex": true, "shutil": true, "signal": true,
	"site": true, "smtpd": true, "smtplib": true, "sndhdr": true, "socket": true,
	"socketserver": true, "spwd": true, "sqlite3": true, "ssl": true, "stat": true,
	"statistics": true, "string": true, "stringprep": true, "struct": true,
	"subprocess": true, "sunau": true, "symtable": true, "sys": true, "sysconfig": true,
	"syslog": true, "tabnanny": true, "tarfile": true, "telnetlib": true, "tempfile": true,
	"termios": true, "test": true, "textwrap": true, "threading": true, "time": true,
	"timeit": true, "tkinter": true, "token": true, "tokenize": true, "tomllib": true,
	"trace": true, "traceback": true, "tracemalloc": true, "tty": true, "turtle": true,
	"turtledemo": true, "types": true, "typing": true, "unicodedata": true, "unittest": true,
	"urllib": true, "uu": true, "uuid": true, "venv": true, "warnings": true, "wave": true,
	"weakref": true, "webbrowser": true, "winreg": true, "winsound": true, "wsgiref": true,
	"xdrlib": true, "xml": true, "xmlrpc": true, "zipapp": true, "zipfile": true,
	"zipimport": true, "zlib": true, "zoneinfo": true, "_thread": true, "__future__": true,
}

func main() {
	var (
		output  = flag.String("o", "", "Output file path (default: stdout)")
		exclude = flag.String("exclude", "venv,__pycache__,.venv,build,dist,*.egg-info", "Comma-separated list of directory patterns to exclude")
	)
	flag.Parse()

	dir := "."
	if flag.NArg() > 0 {
		dir = flag.Arg(0)
	}

	excludePatterns := strings.Split(*exclude, ",")
	for i := range excludePatterns {
		excludePatterns[i] = strings.TrimSpace(excludePatterns[i])
	}

	result, err := extract(dir, excludePatterns)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error: %v\n", err)
		os.Exit(1)
	}

	// Output
	var w = os.Stdout
	if *output != "" {
		f, err := os.Create(*output)
		if err != nil {
			fmt.Fprintf(os.Stderr, "Error creating output file: %v\n", err)
			os.Exit(1)
		}
		defer f.Close()
		w = f
	}

	if err := result.Write(w); err != nil {
		fmt.Fprintf(os.Stderr, "Error writing output: %v\n", err)
		os.Exit(1)
	}
}

// Regex patterns for Python imports
var (
	importRe     = regexp.MustCompile(`^\s*import\s+([a-zA-Z_][a-zA-Z0-9_]*(?:\s*,\s*[a-zA-Z_][a-zA-Z0-9_]*)*)`)
	importAsRe   = regexp.MustCompile(`^\s*import\s+([a-zA-Z_][a-zA-Z0-9_.]*)\s+as\s+`)
	fromImportRe = regexp.MustCompile(`^\s*from\s+([a-zA-Z_][a-zA-Z0-9_.]*)\s+import`)
)

// extract analyzes Python files and extracts import dependencies.
func extract(dir string, excludePatterns []string) (*extraction.Result, error) {
	result := extraction.NewResult("python")

	absDir, err := filepath.Abs(dir)
	if err != nil {
		return nil, fmt.Errorf("getting absolute path: %w", err)
	}

	// Find Python packages (directories with __init__.py)
	packages := make(map[string]*extraction.Package)

	err = filepath.Walk(absDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}

		// Skip excluded directories
		if info.IsDir() {
			name := info.Name()
			if shouldExclude(name, excludePatterns) {
				return filepath.SkipDir
			}
			return nil
		}

		// Only process .py files
		if !strings.HasSuffix(path, ".py") {
			return nil
		}

		// Get relative path
		relPath, err := filepath.Rel(absDir, path)
		if err != nil {
			relPath = path
		}

		// Determine package path
		pkgDir := filepath.Dir(relPath)
		if pkgDir == "." {
			pkgDir = ""
		}

		// Create or get package
		pkg, exists := packages[pkgDir]
		if !exists {
			pkg = &extraction.Package{
				Path: pkgDir,
			}
			packages[pkgDir] = pkg
		}

		// Add file to package
		pkg.Files = append(pkg.Files, filepath.Base(relPath))

		// Parse imports from file
		imports, testImports, err := parseImports(path, filepath.Base(relPath))
		if err != nil {
			result.AddError(fmt.Sprintf("parsing %s: %v", relPath, err))
			return nil
		}

		// Merge imports
		pkg.Imports = mergeImports(pkg.Imports, imports)
		pkg.TestImports = mergeImports(pkg.TestImports, testImports)

		return nil
	})

	if err != nil {
		return nil, fmt.Errorf("walking directory: %w", err)
	}

	// Convert map to slice and sort
	for _, pkg := range packages {
		// Sort imports
		sortImports(pkg.Imports)
		sortImports(pkg.TestImports)
		result.AddPackage(*pkg)
	}

	// Sort packages by path
	sort.Slice(result.Packages, func(i, j int) bool {
		return result.Packages[i].Path < result.Packages[j].Path
	})

	return result, nil
}

// parseImports extracts import statements from a Python file.
func parseImports(path, filename string) (imports, testImports []extraction.Import, err error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, nil, err
	}
	defer file.Close()

	isTestFile := strings.HasPrefix(filename, "test_") || strings.HasSuffix(filename, "_test.py")

	seen := make(map[string]bool)
	scanner := bufio.NewScanner(file)

	for scanner.Scan() {
		line := scanner.Text()

		// Skip comments and empty lines
		trimmed := strings.TrimSpace(line)
		if trimmed == "" || strings.HasPrefix(trimmed, "#") {
			continue
		}

		// Try to match import patterns
		var moduleName string

		if match := fromImportRe.FindStringSubmatch(line); match != nil {
			moduleName = match[1]
		} else if match := importAsRe.FindStringSubmatch(line); match != nil {
			moduleName = match[1]
		} else if match := importRe.FindStringSubmatch(line); match != nil {
			// Handle multiple imports: import foo, bar, baz
			modules := strings.Split(match[1], ",")
			for _, m := range modules {
				m = strings.TrimSpace(m)
				if m != "" && !seen[m] {
					seen[m] = true
					imp := classifyImport(m)
					if isTestFile {
						testImports = append(testImports, imp)
					} else {
						imports = append(imports, imp)
					}
				}
			}
			continue
		}

		if moduleName == "" || seen[moduleName] {
			continue
		}
		seen[moduleName] = true

		imp := classifyImport(moduleName)

		if isTestFile {
			testImports = append(testImports, imp)
		} else {
			imports = append(imports, imp)
		}
	}

	if err := scanner.Err(); err != nil {
		return nil, nil, err
	}

	return imports, testImports, nil
}

// classifyImport determines the kind of a Python import.
func classifyImport(moduleName string) extraction.Import {
	// Get the top-level module name
	topLevel := moduleName
	if idx := strings.Index(moduleName, "."); idx > 0 {
		topLevel = moduleName[:idx]
	}

	// Check if it's stdlib
	if pythonStdlib[topLevel] {
		return extraction.Import{
			Path: moduleName,
			Kind: extraction.ImportKindStdlib,
		}
	}

	// Check for relative imports (start with .)
	if strings.HasPrefix(moduleName, ".") {
		return extraction.Import{
			Path: moduleName,
			Kind: extraction.ImportKindInternal,
		}
	}

	// Default to external (can be refined with python-deps.toml info)
	return extraction.Import{
		Path: moduleName,
		Kind: extraction.ImportKindExternal,
	}
}

// shouldExclude checks if a directory should be excluded.
func shouldExclude(name string, patterns []string) bool {
	for _, pattern := range patterns {
		if pattern == "" {
			continue
		}
		// Handle wildcard patterns
		if strings.HasPrefix(pattern, "*") {
			suffix := pattern[1:]
			if strings.HasSuffix(name, suffix) {
				return true
			}
		} else if name == pattern || strings.Contains(name, pattern) {
			return true
		}
	}
	return false
}

// mergeImports merges two import slices, avoiding duplicates.
func mergeImports(a, b []extraction.Import) []extraction.Import {
	seen := make(map[string]bool)
	result := make([]extraction.Import, 0, len(a)+len(b))

	for _, imp := range a {
		if !seen[imp.Path] {
			seen[imp.Path] = true
			result = append(result, imp)
		}
	}

	for _, imp := range b {
		if !seen[imp.Path] {
			seen[imp.Path] = true
			result = append(result, imp)
		}
	}

	return result
}

// sortImports sorts imports by path.
func sortImports(imports []extraction.Import) {
	sort.Slice(imports, func(i, j int) bool {
		return imports[i].Path < imports[j].Path
	})
}
