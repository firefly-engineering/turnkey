package buckgen

import (
	"bytes"
	"strings"
	"testing"

	"github.com/firefly-engineering/turnkey/src/go/pkg/goparse"
)

func TestNormalizeDeps(t *testing.T) {
	linux := goparse.Platform{OS: "linux", Arch: "amd64"}
	darwin := goparse.Platform{OS: "darwin", Arch: "amd64"}

	imports := map[goparse.Platform][]string{
		linux:  {"fmt", "os", "golang.org/x/sys/unix"},
		darwin: {"fmt", "os", "syscall"},
	}

	norm := NormalizeDeps(imports)

	if len(norm.Common) != 2 {
		t.Errorf("expected 2 common deps, got %d", len(norm.Common))
	}

	if len(norm.Platform[linux]) != 1 || norm.Platform[linux][0] != "golang.org/x/sys/unix" {
		t.Errorf("expected linux specific dep golang.org/x/sys/unix, got %v", norm.Platform[linux])
	}

	if len(norm.Platform[darwin]) != 1 || norm.Platform[darwin][0] != "syscall" {
		t.Errorf("expected darwin specific dep syscall, got %v", norm.Platform[darwin])
	}
}

func TestRenderPackage(t *testing.T) {
	pkg := &goparse.GoPackage{
		Name:       "testpkg",
		ImportPath: "github.com/example/testpkg",
		Imports: map[goparse.Platform][]string{
			{OS: "linux", Arch: "amd64"}:  {"fmt", "github.com/example/common", "github.com/example/linuxonly"},
			{OS: "darwin", Arch: "amd64"}: {"fmt", "github.com/example/common", "github.com/example/maconly"},
		},
	}

	cfg := DefaultConfig()
	var buf bytes.Buffer
	err := RenderPackage(&buf, pkg, cfg)
	if err != nil {
		t.Fatalf("RenderPackage failed: %v", err)
	}

	output := buf.String()

	// Check for common dep
	if !strings.Contains(output, "\"godeps//vendor/github.com/example/common:common\"") {
		t.Errorf("output missing common dep: %s", output)
	}

	// Check for select
	if !strings.Contains(output, "select({") {
		t.Errorf("output missing select: %s", output)
	}

	// Check for platform-specific deps
	if !strings.Contains(output, "\"config//os:linux\": [") {
		t.Errorf("output missing linux constraint: %s", output)
	}
	if !strings.Contains(output, "\"godeps//vendor/github.com/example/linuxonly:linuxonly\"") {
		t.Errorf("output missing linux-only dep: %s", output)
	}
}

func TestConfig(t *testing.T) {
	cfg := DefaultConfig()
	if cfg.Buck.BuildfileName != "rules.star" {
		t.Errorf("expected default buildfile_name rules.star, got %s", cfg.Buck.BuildfileName)
	}
}
