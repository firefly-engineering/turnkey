package main

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

// A patch from tk compose applies with no fuzz, at the line it was made for:
// appending to a file's last line must not leave a context line the file
// doesn't have
func TestGeneratedPatchAppliesExactly(t *testing.T) {
	dir := t.TempDir()
	original := filepath.Join(dir, "a", "lib.rs")
	edited := filepath.Join(dir, "b", "lib.rs")
	// The same closing lines twice, so a misplaced hunk could match earlier
	body := "fn a() {\n    }\n}\nfn b() {\n    }\n}\n"
	for path, content := range map[string]string{original: body, edited: body + "// appended\n"} {
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	diff, err := generateUnifiedDiff(original, edited, "a/lib.rs", "b/lib.rs")
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(diff, "\n \n") {
		t.Errorf("the patch has an empty context line the file doesn't have:\n%s", diff)
	}

	if _, err := exec.LookPath("patch"); err != nil {
		t.Skip("no patch command to apply it with")
	}
	target := filepath.Join(dir, "target")
	if err := os.MkdirAll(target, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(target, "lib.rs"), []byte(body), 0o644); err != nil {
		t.Fatal(err)
	}
	cmd := exec.Command("patch", "-p1", "--fuzz=0", "--forward")
	cmd.Dir = target
	cmd.Stdin = strings.NewReader(diff)
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("patch: %v\n%s\n%s", err, out, diff)
	}
	got, _ := os.ReadFile(filepath.Join(target, "lib.rs"))
	if string(got) != body+"// appended\n" {
		t.Errorf("patched file:\n%s", got)
	}
}
