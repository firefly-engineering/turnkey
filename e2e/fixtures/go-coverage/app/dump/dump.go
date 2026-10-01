// Package dump describes values with github.com/davecgh/go-spew, required at
// a pseudo-version (a commit after its last tag).
package dump

import "github.com/davecgh/go-spew/spew"

var state = spew.ConfigState{
	Indent:                  "  ",
	DisablePointerAddresses: true,
	DisableCapacities:       true,
	SortKeys:                true,
}

// Describe returns a stable, multi-line description of v.
func Describe(v any) string {
	return state.Sdump(v)
}
