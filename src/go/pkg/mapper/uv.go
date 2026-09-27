package mapper

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/pelletier/go-toml/v2"
)

// loadUVWorkspaceModules reads the uv workspace declared by the
// pyproject.toml in projectRoot and returns the packages its members
// provide, as dotted module names keyed to the member's directory relative
// to projectRoot: {"turnkey.cfg": "src/python/cfg", ...}.
//
// Packages come from each member's source layout (the member directory, or
// its src/ directory), so a namespace package shared by several members
// (PEP 420, a directory without __init__.py) contributes one entry per
// member subpackage. A module two members both provide is ambiguous and
// left out.
func loadUVWorkspaceModules(projectRoot string) (map[string]string, error) {
	path := filepath.Join(projectRoot, "pyproject.toml")
	content, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	var pyproject struct {
		Tool struct {
			UV struct {
				Workspace struct {
					Members []string `toml:"members"`
				} `toml:"workspace"`
			} `toml:"uv"`
		} `toml:"tool"`
	}
	if err := toml.Unmarshal(content, &pyproject); err != nil {
		return nil, fmt.Errorf("parsing %s: %w", path, err)
	}

	modules := make(map[string]string)
	ambiguous := make(map[string]bool)
	for _, pattern := range pyproject.Tool.UV.Workspace.Members {
		dirs, err := filepath.Glob(filepath.Join(projectRoot, pattern))
		if err != nil {
			return nil, fmt.Errorf("workspace member %q: %w", pattern, err)
		}
		for _, memberDir := range dirs {
			if _, err := os.Stat(filepath.Join(memberDir, "pyproject.toml")); err != nil {
				continue
			}
			rel, err := filepath.Rel(projectRoot, memberDir)
			if err != nil {
				continue
			}
			rel = filepath.ToSlash(rel)

			sourceRoot := memberDir
			if info, err := os.Stat(filepath.Join(memberDir, "src")); err == nil && info.IsDir() {
				sourceRoot = filepath.Join(memberDir, "src")
			}
			for _, module := range memberPackages(sourceRoot, "") {
				if other, ok := modules[module]; ok && other != rel {
					ambiguous[module] = true
				}
				modules[module] = rel
			}
		}
	}
	for module := range ambiguous {
		delete(modules, module)
	}
	return modules, nil
}

// memberPackages returns the dotted names of the packages under dir, with
// prefix prepended. A directory holding Python files is a package; one
// holding only directories is a namespace, whose subdirectories are
// searched in turn.
func memberPackages(dir, prefix string) []string {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil
	}
	var packages []string
	for _, entry := range entries {
		name := entry.Name()
		if !entry.IsDir() || !isPythonIdentifier(name) || name == "__pycache__" {
			continue
		}
		sub := filepath.Join(dir, name)
		if hasPythonFiles(sub) {
			packages = append(packages, prefix+name)
		} else {
			packages = append(packages, memberPackages(sub, prefix+name+".")...)
		}
	}
	return packages
}

// hasPythonFiles reports whether dir directly holds a .py file.
func hasPythonFiles(dir string) bool {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return false
	}
	for _, entry := range entries {
		if !entry.IsDir() && strings.HasSuffix(entry.Name(), ".py") {
			return true
		}
	}
	return false
}

// isPythonIdentifier reports whether name can be a Python package name:
// ASCII letters, digits and underscores, not starting with a digit.
func isPythonIdentifier(name string) bool {
	if name == "" || (name[0] >= '0' && name[0] <= '9') {
		return false
	}
	for _, r := range name {
		if r != '_' && !(r >= 'a' && r <= 'z') && !(r >= 'A' && r <= 'Z') && !(r >= '0' && r <= '9') {
			return false
		}
	}
	return true
}

// workspaceModuleDir returns the directory of the workspace member that
// provides module, the member whose package is the longest prefix of it:
// "turnkey.cargo.toml" -> "src/python/cargo".
func workspaceModuleDir(modules map[string]string, module string) (string, bool) {
	for name := module; name != ""; {
		if dir, ok := modules[name]; ok {
			return dir, true
		}
		dot := strings.LastIndex(name, ".")
		if dot < 0 {
			break
		}
		name = name[:dot]
	}
	return "", false
}
