package staleness

import (
	"os"
	"path/filepath"
	"testing"
)

func TestGlobRustSrcs(t *testing.T) {
	dir := t.TempDir()

	// Create Rust source files
	files := []string{
		"lib.rs",
		"main.rs",
		"helper.rs",
		"helper_test.rs", // Should be excluded
	}

	for _, name := range files {
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, []byte("// Rust code"), 0644); err != nil {
			t.Fatal(err)
		}
	}

	srcs, err := globRustSrcs(dir)
	if err != nil {
		t.Fatal(err)
	}

	// Should have 3 files (excluding test file)
	if len(srcs) != 3 {
		t.Errorf("expected 3 source files, got %d: %v", len(srcs), srcs)
	}
}

func TestParseRustUses(t *testing.T) {
	dir := t.TempDir()

	// Create a Rust source file
	rsFile := filepath.Join(dir, "lib.rs")
	content := `use std::io;
use serde::{Serialize, Deserialize};
use tokio::runtime::Runtime;
use crate::internal;
use self::module;

extern crate log;

fn main() {}
`
	if err := os.WriteFile(rsFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	uses, err := parseRustUses(dir)
	if err != nil {
		t.Fatal(err)
	}

	expected := map[string]bool{
		"std":    true,
		"serde":  true,
		"tokio":  true,
		"crate":  true,
		"self":   true,
		"log":    true,
	}

	if len(uses) != len(expected) {
		t.Errorf("expected %d uses, got %d: %v", len(expected), len(uses), uses)
	}

	for _, use := range uses {
		if !expected[use] {
			t.Errorf("unexpected use: %s", use)
		}
	}
}

func TestFilterExternalRustUses(t *testing.T) {
	uses := []string{
		"std",
		"core",
		"alloc",
		"serde",
		"tokio",
		"self",
		"super",
		"crate",
	}

	external := filterExternalRustUses(uses)

	// Should only have serde and tokio
	if len(external) != 2 {
		t.Errorf("expected 2 external uses, got %d: %v", len(external), external)
	}

	expectedExternal := map[string]bool{"serde": true, "tokio": true}
	for _, use := range external {
		if !expectedExternal[use] {
			t.Errorf("unexpected external use: %s", use)
		}
	}
}

func TestExtractRustDepCrate(t *testing.T) {
	tests := []struct {
		dep   string
		crate string
	}{
		{"rustdeps//vendor/serde:serde", "serde"},
		{"rustdeps//vendor/tokio:tokio", "tokio"},
		{"rustdeps//vendor/serde_json:serde_json", "serde_json"},
		{"//crate/mylib:mylib", ""},
		{":local", ""},
	}

	for _, tc := range tests {
		got := extractRustDepCrate(tc.dep)
		if got != tc.crate {
			t.Errorf("extractRustDepCrate(%q) = %q, want %q", tc.dep, got, tc.crate)
		}
	}
}

func TestCheckRustSrcList_InSync(t *testing.T) {
	dir := t.TempDir()

	// Create BUCK file
	buckFile := filepath.Join(dir, "BUCK")
	buckContent := `rust_library(
    name = "mylib",
    srcs = ["lib.rs", "helper.rs"],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create matching Rust files
	for _, name := range []string{"lib.rs", "helper.rs"} {
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, []byte("// Rust"), 0644); err != nil {
			t.Fatal(err)
		}
	}

	result, err := CheckRustSrcList(buckFile)
	if err != nil {
		t.Fatal(err)
	}

	if result.Stale {
		t.Errorf("expected not stale, got stale with missing=%v extra=%v",
			result.Missing, result.Extra)
	}
}

func TestCheckRustSrcList_ExtraFile(t *testing.T) {
	dir := t.TempDir()

	// Create BUCK file
	buckFile := filepath.Join(dir, "BUCK")
	buckContent := `rust_library(
    name = "mylib",
    srcs = ["lib.rs"],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create lib.rs and an extra file
	for _, name := range []string{"lib.rs", "extra.rs"} {
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, []byte("// Rust"), 0644); err != nil {
			t.Fatal(err)
		}
	}

	result, err := CheckRustSrcList(buckFile)
	if err != nil {
		t.Fatal(err)
	}

	if !result.Stale {
		t.Error("expected stale due to extra file")
	}

	if len(result.Extra) != 1 || result.Extra[0] != "extra.rs" {
		t.Errorf("expected extra=[extra.rs], got %v", result.Extra)
	}
}

func TestCheckRustUses_InSync(t *testing.T) {
	dir := t.TempDir()

	// Create BUCK file
	buckFile := filepath.Join(dir, "BUCK")
	buckContent := `rust_library(
    name = "mylib",
    srcs = ["lib.rs"],
    deps = [
        "rustdeps//vendor/serde:serde",
    ],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create Rust file with matching use
	rsFile := filepath.Join(dir, "lib.rs")
	rsContent := `use std::io;
use serde::Serialize;

fn main() {}
`
	if err := os.WriteFile(rsFile, []byte(rsContent), 0644); err != nil {
		t.Fatal(err)
	}

	result, err := CheckRustUses(buckFile)
	if err != nil {
		t.Fatal(err)
	}

	if result.Stale {
		t.Errorf("expected not stale, got stale with missing=%v extra=%v",
			result.Missing, result.Extra)
	}
}

func TestCheckRustUses_MissingDep(t *testing.T) {
	dir := t.TempDir()

	// Create BUCK file without deps
	buckFile := filepath.Join(dir, "BUCK")
	buckContent := `rust_library(
    name = "mylib",
    srcs = ["lib.rs"],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create Rust file with external use
	rsFile := filepath.Join(dir, "lib.rs")
	rsContent := `use serde::Serialize;

fn main() {}
`
	if err := os.WriteFile(rsFile, []byte(rsContent), 0644); err != nil {
		t.Fatal(err)
	}

	result, err := CheckRustUses(buckFile)
	if err != nil {
		t.Fatal(err)
	}

	if !result.Stale {
		t.Error("expected stale due to missing dep")
	}

	if len(result.Missing) != 1 || result.Missing[0] != "serde" {
		t.Errorf("expected missing=[serde], got %v", result.Missing)
	}
}

func TestCachedCheckRustPackage(t *testing.T) {
	dir := t.TempDir()

	// Create BUCK file
	buckFile := filepath.Join(dir, "BUCK")
	buckContent := `rust_library(
    name = "lib",
    srcs = ["lib.rs"],
    deps = [
        "rustdeps//vendor/serde:serde",
    ],
)
`
	if err := os.WriteFile(buckFile, []byte(buckContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Create Rust file
	rsFile := filepath.Join(dir, "lib.rs")
	rsContent := `use serde::Serialize;

fn main() {}
`
	if err := os.WriteFile(rsFile, []byte(rsContent), 0644); err != nil {
		t.Fatal(err)
	}

	cache := NewCache()
	cc := &CachedCheck{Cache: cache, BuckFile: buckFile}

	// First check should not be from cache
	result1, err := cc.CheckRustPackage()
	if err != nil {
		t.Fatal(err)
	}

	if result1.FromCache {
		t.Error("expected first check not from cache")
	}

	// Second check should be from cache
	result2, err := cc.CheckRustPackage()
	if err != nil {
		t.Fatal(err)
	}

	if !result2.FromCache {
		t.Error("expected second check from cache")
	}
}
