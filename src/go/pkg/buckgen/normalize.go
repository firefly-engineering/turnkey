package buckgen

import (
	"sort"

	"github.com/firefly-engineering/turnkey/src/go/pkg/goparse"
)

// NormalizedDeps represents deps split into common and platform-specific
type NormalizedDeps struct {
	Common   []string                      // deps present on ALL platforms
	Platform map[goparse.Platform][]string // platform-specific deps
}

// NormalizeDeps takes per-platform imports and extracts common ones
func NormalizeDeps(imports map[goparse.Platform][]string) *NormalizedDeps {
	if len(imports) == 0 {
		return &NormalizedDeps{
			Platform: make(map[goparse.Platform][]string),
		}
	}

	// Count occurrences of each import
	counts := make(map[string]int)
	for _, deps := range imports {
		// Unique deps per platform
		seen := make(map[string]bool)
		for _, d := range deps {
			if !seen[d] {
				counts[d]++
				seen[d] = true
			}
		}
	}

	numPlatforms := len(imports)
	common := make([]string, 0)
	for dep, count := range counts {
		if count == numPlatforms {
			common = append(common, dep)
		}
	}
	sort.Strings(common)

	// Extract platform-specific deps
	platformSpecific := make(map[goparse.Platform][]string)
	isCommon := make(map[string]bool)
	for _, d := range common {
		isCommon[d] = true
	}

	for platform, deps := range imports {
		specific := make([]string, 0)
		seen := make(map[string]bool)
		for _, d := range deps {
			if !isCommon[d] && !seen[d] {
				specific = append(specific, d)
				seen[d] = true
			}
		}
		if len(specific) > 0 {
			sort.Strings(specific)
			platformSpecific[platform] = specific
		}
	}

	return &NormalizedDeps{
		Common:   common,
		Platform: platformSpecific,
	}
}
