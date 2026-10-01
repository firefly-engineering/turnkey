package greet

import (
	_ "embed"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"example.com/lib/text"
)

// cases holds one "name=greeting" pair per line. It is compiled into the
// test binary: the go_test target lists it in embed_srcs.
//
//go:embed testdata/cases.txt
var cases string

func TestHelloEmbeddedCases(t *testing.T) {
	n := 0
	for _, line := range strings.Split(strings.TrimSpace(cases), "\n") {
		name, want, ok := strings.Cut(line, "=")
		if !ok {
			t.Fatalf("malformed case %q", line)
		}
		if got := Hello(name); got != want {
			t.Errorf("Hello(%q) = %q, want %q", name, got, want)
		}
		n++
	}
	if n == 0 {
		t.Fatal("no embedded cases")
	}
}

// golden reads testdata/golden.txt. Under buck2 it is a resource of the
// go_test target, copied next to the test binary; under go test it is in
// the package directory, the working directory.
func golden(t *testing.T) string {
	t.Helper()
	name := filepath.Join("testdata", "golden.txt")
	if exe, err := os.Executable(); err == nil {
		if data, err := os.ReadFile(filepath.Join(filepath.Dir(exe), name)); err == nil {
			return string(data)
		}
	}
	data, err := os.ReadFile(name)
	if err != nil {
		t.Fatalf("reading %s: %v", name, err)
	}
	return string(data)
}

func TestHelloMatchesGolden(t *testing.T) {
	want := strings.TrimSpace(golden(t))
	if got := Hello("world"); got != want {
		t.Errorf("Hello(%q) = %q, want %q", "world", got, want)
	}
	if got, want := Hello("go"), "Hello, "+text.Shout("go"); got != want {
		t.Errorf("Hello(%q) = %q, want %q", "go", got, want)
	}
}
