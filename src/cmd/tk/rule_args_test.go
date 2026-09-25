package main

import (
	"slices"
	"testing"
)

func TestRuleArgsAppliesFlagsAmongRuleNames(t *testing.T) {
	defer func() { verbose, dryRun = false, false }()

	rules := ruleArgs([]string{"--verbose", "go", "-n", "python"})

	if want := []string{"go", "python"}; !slices.Equal(rules, want) {
		t.Errorf("ruleArgs = %q, want %q", rules, want)
	}
	if !verbose || !dryRun {
		t.Errorf("verbose = %v, dryRun = %v; want both set", verbose, dryRun)
	}
}

func TestRuleArgsWithoutRuleNames(t *testing.T) {
	if rules := ruleArgs(nil); len(rules) != 0 {
		t.Errorf("ruleArgs(nil) = %q, want none", rules)
	}
}
