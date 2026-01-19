package main

import (
	"reflect"
	"testing"
)

func TestTransformIsolationDirValue(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		expected string
	}{
		{
			name:     "plain name gets prefixed",
			input:    "foo",
			expected: ".turnkey-foo",
		},
		{
			name:     "v2 gets prefixed",
			input:    "v2",
			expected: ".turnkey-v2",
		},
		{
			name:     "dotted name passes through",
			input:    ".custom",
			expected: ".custom",
		},
		{
			name:     "turnkey prefix passes through",
			input:    ".turnkey",
			expected: ".turnkey",
		},
		{
			name:     "turnkey-foo passes through",
			input:    ".turnkey-foo",
			expected: ".turnkey-foo",
		},
		{
			name:     "empty string gets prefixed",
			input:    "",
			expected: ".turnkey-",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := transformIsolationDirValue(tt.input)
			if result != tt.expected {
				t.Errorf("transformIsolationDirValue(%q) = %q, want %q", tt.input, result, tt.expected)
			}
		})
	}
}

func TestTransformIsolationDir(t *testing.T) {
	tests := []struct {
		name     string
		input    []string
		expected []string
	}{
		{
			name:     "no isolation-dir flag",
			input:    []string{"build", "//foo:bar"},
			expected: []string{"build", "//foo:bar"},
		},
		{
			name:     "equals format plain value",
			input:    []string{"--isolation-dir=test", "build", "//foo:bar"},
			expected: []string{"--isolation-dir=.turnkey-test", "build", "//foo:bar"},
		},
		{
			name:     "equals format dotted value",
			input:    []string{"--isolation-dir=.custom", "build", "//foo:bar"},
			expected: []string{"--isolation-dir=.custom", "build", "//foo:bar"},
		},
		{
			name:     "space format plain value",
			input:    []string{"--isolation-dir", "test", "build", "//foo:bar"},
			expected: []string{"--isolation-dir", ".turnkey-test", "build", "//foo:bar"},
		},
		{
			name:     "space format dotted value",
			input:    []string{"--isolation-dir", ".custom", "build", "//foo:bar"},
			expected: []string{"--isolation-dir", ".custom", "build", "//foo:bar"},
		},
		{
			name:     "isolation-dir in middle of args",
			input:    []string{"build", "--isolation-dir=foo", "//target"},
			expected: []string{"build", "--isolation-dir=.turnkey-foo", "//target"},
		},
		{
			name:     "multiple flags preserved",
			input:    []string{"-v", "--isolation-dir=test", "build", "--show-output"},
			expected: []string{"-v", "--isolation-dir=.turnkey-test", "build", "--show-output"},
		},
		{
			name:     "isolation-dir at end with equals",
			input:    []string{"build", "--isolation-dir=foo"},
			expected: []string{"build", "--isolation-dir=.turnkey-foo"},
		},
		{
			name:     "isolation-dir at end without value (edge case)",
			input:    []string{"build", "--isolation-dir"},
			expected: []string{"build", "--isolation-dir"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := transformIsolationDir(tt.input)
			if !reflect.DeepEqual(result, tt.expected) {
				t.Errorf("transformIsolationDir(%v) = %v, want %v", tt.input, result, tt.expected)
			}
		})
	}
}
