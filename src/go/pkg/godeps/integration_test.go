package godeps

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// TestIntegration_Simple tests godeps-gen with a single dependency.
func TestIntegration_Simple(t *testing.T) {
	runIntegrationTest(t, "simple")
}

// TestIntegration_Medium tests godeps-gen with multiple dependencies including indirect.
func TestIntegration_Medium(t *testing.T) {
	runIntegrationTest(t, "medium")
}

// TestIntegration_EdgeCases tests godeps-gen with various import path formats.
func TestIntegration_EdgeCases(t *testing.T) {
	runIntegrationTest(t, "edge-cases")
}

// runIntegrationTest runs an integration test for the given test case.
func runIntegrationTest(t *testing.T, testCase string) {
	t.Helper()

	// Find testdata directory
	testdataDir := findTestdataDir(t)
	caseDir := filepath.Join(testdataDir, "godeps", testCase)

	// Read input files
	goModPath := filepath.Join(caseDir, "go.mod")
	goSumPath := filepath.Join(caseDir, "go.sum")
	expectedPath := filepath.Join(caseDir, "expected.toml")

	goMod, err := os.ReadFile(goModPath)
	if err != nil {
		t.Fatalf("failed to read go.mod: %v", err)
	}

	goSum, err := os.ReadFile(goSumPath)
	if err != nil {
		t.Fatalf("failed to read go.sum: %v", err)
	}

	expected, err := os.ReadFile(expectedPath)
	if err != nil {
		t.Fatalf("failed to read expected.toml: %v", err)
	}

	// Parse go.mod
	opts := DefaultParseOptions()
	deps, err := ParseGoMod(goMod, opts)
	if err != nil {
		t.Fatalf("failed to parse go.mod: %v", err)
	}

	// Merge hashes from go.sum
	hashes, err := ParseGoSum(goSum)
	if err != nil {
		t.Fatalf("failed to parse go.sum: %v", err)
	}
	for i := range deps {
		deps[i].GoSumHash = hashes[deps[i].ImportPath+"@"+deps[i].Version]
	}

	// Generate output (without headers for cleaner comparison)
	var buf bytes.Buffer
	outputOpts := OutputOptions{
		IncludeHeader:         false,
		IncludeHashWarning:    false,
		IncludeRegenerateHint: false,
	}
	if err := WriteTOML(&buf, deps, outputOpts); err != nil {
		t.Fatalf("failed to write TOML: %v", err)
	}

	// Compare output
	got := strings.TrimSpace(buf.String())
	want := strings.TrimSpace(string(expected))

	if got != want {
		t.Errorf("output mismatch for %s:\n--- want ---\n%s\n--- got ---\n%s", testCase, want, got)
	}
}

// findTestdataDir finds the testdata directory.
func findTestdataDir(t *testing.T) string {
	t.Helper()

	// In Buck2, resources are placed in __<test_name>__/godeps_fixtures/godeps/
	// relative to the test binary's directory

	// Get the executable path
	exe, err := os.Executable()
	if err == nil {
		exeDir := filepath.Dir(exe)

		// Buck2 places resources in a sibling directory named __<test_name>__
		// e.g., __godeps_test__/godeps_fixtures/godeps/
		buckResources := filepath.Join(exeDir, "godeps_fixtures", "godeps")
		if _, err := os.Stat(buckResources); err == nil {
			return filepath.Dir(buckResources)
		}
	}

	// Try common locations for go test from repo root
	candidates := []string{
		"testdata",
		"../testdata",
		"../../testdata",
		"../../../testdata",
		"../../../../testdata",
	}

	for _, candidate := range candidates {
		if _, err := os.Stat(filepath.Join(candidate, "godeps")); err == nil {
			abs, _ := filepath.Abs(candidate)
			return abs
		}
	}

	// Try from BUCK_PROJECT_ROOT environment variable
	if root := os.Getenv("BUCK_PROJECT_ROOT"); root != "" {
		testdata := filepath.Join(root, "testdata")
		if _, err := os.Stat(testdata); err == nil {
			return testdata
		}
	}

	t.Skip("testdata directory not found - skipping integration test")
	return ""
}
