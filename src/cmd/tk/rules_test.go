package main

import (
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/rulesreport"
)

// fakeRulesSync is a rulesSyncer that records its requests and returns
// report.
func fakeRulesSync(report rulesreport.Report, requests *[]rulesSyncRequest) rulesSyncer {
	return func(req rulesSyncRequest) (rulesreport.Report, error) {
		*requests = append(*requests, req)
		return report, nil
	}
}

// A target whose deps sync can't read fails tk rules check; one a
// "# turnkey:no-sync" comment opts out doesn't.
func TestRulesCheckUnreadableDeps(t *testing.T) {
	// --quiet sets the package-level flag; restore it
	t.Cleanup(func() { quiet = false })

	var requests []rulesSyncRequest
	unreadable := rulesreport.Report{Results: []rulesreport.Result{{
		Path:       "/project/lib/rules.star",
		Unreadable: []rulesreport.UnreadableTarget{{Target: "lib", Attribute: "deps"}},
	}}}
	if code := checkRules(fakeRulesSync(unreadable, &requests), "/project", []string{"--all", "--quiet"}); code != 1 {
		t.Errorf("unreadable deps: exit code %d, want 1", code)
	}

	optedOut := rulesreport.Report{Results: []rulesreport.Result{{
		Path:     "/project/lib/rules.star",
		OptedOut: []string{"lib"},
	}}}
	if code := checkRules(fakeRulesSync(optedOut, &requests), "/project", []string{"--all", "--quiet"}); code != 0 {
		t.Errorf("opted out: exit code %d, want 0", code)
	}
}

// tk rules check checks every rules.star under the directory it is given,
// whatever their sources' file times and git status say: it asks rules
// sync for a forced dry run, with or without --all.
func TestRulesCheckIgnoresFileTimes(t *testing.T) {
	t.Cleanup(func() { quiet = false })

	for _, args := range [][]string{{"--quiet"}, {"--all", "--quiet", "lib"}} {
		var requests []rulesSyncRequest
		checkRules(fakeRulesSync(rulesreport.Report{}, &requests), "/project", args)
		dir := "/project"
		if slices.Contains(args, "lib") {
			dir = "/project/lib"
		}
		want := []rulesSyncRequest{{ProjectRoot: "/project", Dir: dir, DryRun: true, Force: true}}
		if !slices.Equal(requests, want) {
			t.Errorf("%q: requests %+v, want %+v", args, requests, want)
		}
	}
}

// An error that stopped rules sync fails tk rules check.
func TestRulesCheckSyncError(t *testing.T) {
	t.Cleanup(func() { quiet = false })

	for _, stage := range []rulesreport.Stage{rulesreport.StageSetup, rulesreport.StageSync} {
		var requests []rulesSyncRequest
		report := rulesreport.Report{Error: &rulesreport.Error{Stage: stage, Message: "boom"}}
		if code := checkRules(fakeRulesSync(report, &requests), "/project", []string{"--quiet"}); code != 1 {
			t.Errorf("%s error: exit code %d, want 1", stage, code)
		}
	}
}

// execRulesSync passes the request to rules-sync as flags, and reads the
// report it prints: an error stopping sync is the report's, not a failure
// to run it.
func TestExecRulesSync(t *testing.T) {
	dir := t.TempDir()
	argsFile := filepath.Join(dir, "args")
	script := filepath.Join(dir, "rules-sync")
	writeScript := func(report string, exitCode string) {
		t.Helper()
		body := "#!/bin/sh\nprintf '%s\\n' \"$@\" > '" + argsFile + "'\n" +
			"printf '%s\\n' '" + report + "'\nexit " + exitCode + "\n"
		if err := os.WriteFile(script, []byte(body), 0755); err != nil {
			t.Fatal(err)
		}
	}

	writeScript(`{"results":[{"path":"/p/rules.star","updated":true,"changes":[{"target":"t","added":["//a:a"]}]}]}`, "0")
	report, err := execRulesSync(script)(rulesSyncRequest{
		ProjectRoot: "/p", Dir: "/p/sub", DryRun: true, Verbose: true, Force: true,
	})
	if err != nil {
		t.Fatal(err)
	}
	args, err := os.ReadFile(argsFile)
	if err != nil {
		t.Fatal(err)
	}
	wantArgs := "--project-root\n/p\n--dry-run\n--verbose\n--force\n--\n/p/sub\n"
	if string(args) != wantArgs {
		t.Errorf("args %q, want %q", args, wantArgs)
	}
	if len(report.Results) != 1 || !report.Results[0].Updated ||
		!slices.Equal(report.Results[0].Changes[0].Added, []string{"//a:a"}) {
		t.Errorf("report %+v", report)
	}

	execRulesSync(script)(rulesSyncRequest{ProjectRoot: "/p", Dir: "/p"})
	args, _ = os.ReadFile(argsFile)
	if want := "--project-root\n/p\n--\n/p\n"; string(args) != want {
		t.Errorf("args %q, want %q", args, want)
	}

	writeScript(`{"results":[],"error":{"stage":"sync","message":"boom"}}`, "1")
	report, err = execRulesSync(script)(rulesSyncRequest{ProjectRoot: "/p", Dir: "/p"})
	if err != nil || report.Error == nil || report.Error.Message != "boom" {
		t.Errorf("sync error: report %+v, err %v", report, err)
	}

	writeScript(`not json`, "3")
	if _, err := execRulesSync(script)(rulesSyncRequest{ProjectRoot: "/p", Dir: "/p"}); err == nil ||
		!strings.Contains(err.Error(), "exit status 3") {
		t.Errorf("crash: err %v, want the exit status", err)
	}

	if _, err := execRulesSync(filepath.Join(dir, "missing"))(rulesSyncRequest{ProjectRoot: "/p", Dir: "/p"}); err == nil {
		t.Error("missing binary: no error")
	}
}
