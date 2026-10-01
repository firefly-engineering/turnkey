// Package rulesreport is the report rules-sync (src/cmd/rules-sync) prints
// on stdout, as JSON, and tk reads: what rules sync did to each rules.star
// file, or the error that stopped it.
//
// It is the wire format between tk and rules-sync, so it holds data only.
// tk decides what to print, and its exit code, from the report.
package rulesreport

// Report is the outcome of one rules-sync run.
type Report struct {
	// Results lists, per rules.star file sync looked at, what it did.
	Results []Result `json:"results"`

	// Error is the error that stopped sync, if any: Results is then empty.
	Error *Error `json:"error,omitempty"`
}

// Stage is where sync stopped on an error.
type Stage string

const (
	// StageSetup is reading the sync configuration and loading the
	// languages' plug-ins, before any rules.star is read.
	StageSetup Stage = "setup"

	// StageSync is walking the directory and syncing its rules.star files.
	StageSync Stage = "sync"
)

// Error is the error that stopped sync.
type Error struct {
	// Stage is where sync stopped.
	Stage Stage `json:"stage"`

	// Message is the error's text.
	Message string `json:"message"`
}

// Result is what sync did to one rules.star file.
type Result struct {
	// Path is the path to the rules.star file.
	Path string `json:"path"`

	// Updated is true if the file was modified, or would be in a dry run.
	Updated bool `json:"updated"`

	// Skipped is true if the file was skipped as up-to-date.
	Skipped bool `json:"skipped"`

	// Changes lists, per target whose deps changed, what was added and
	// removed, in the order the targets appear in the file.
	Changes []TargetChange `json:"changes,omitempty"`

	// OptedOut lists the targets a "# turnkey:no-sync" comment opts out.
	OptedOut []string `json:"opted_out,omitempty"`

	// Unreadable lists the targets sync skipped because their deps aren't
	// a list of labels, and nothing opts them out.
	Unreadable []UnreadableTarget `json:"unreadable,omitempty"`

	// Errors lists the errors sync met in the file.
	Errors []string `json:"errors,omitempty"`
}

// UnreadableTarget is a target whose deps attribute sync can't read.
type UnreadableTarget struct {
	// Target is the target's name.
	Target string `json:"target"`

	// Attribute is the deps attribute that isn't a list of labels.
	Attribute string `json:"attribute"`
}

// TargetChange records how sync changed one target's deps, or another
// attribute it owns.
type TargetChange struct {
	// Target is the target's name.
	Target string `json:"target"`

	// Attribute is the attribute that changed, when it isn't the deps.
	Attribute string `json:"attribute,omitempty"`

	// Added lists dependencies that were added.
	Added []string `json:"added,omitempty"`

	// Removed lists dependencies that were removed.
	Removed []string `json:"removed,omitempty"`

	// Kept lists dependencies sync would have removed but kept because
	// the target has unmapped imports (listed in Unmapped).
	Kept []string `json:"kept,omitempty"`

	// Unmapped lists the imports that made sync keep the deps in Kept.
	Unmapped []string `json:"unmapped,omitempty"`
}
