// Package buck2args reads the command line of the pinned buck2 release
// (docs/adr/0002-turnkey-owns-the-buck2-version.md) the way buck2 does.
package buck2args

import "strings"

// globalOptionsWithValue are buck2's universal options that take their value
// as the next argument, as `buck2 --help` lists them. Their `--name=value`
// forms are one argument.
var globalOptionsWithValue = map[string]bool{
	"--isolation-dir":   true,
	"-v":                true,
	"--verbose":         true,
	"--oncall":          true,
	"--client-metadata": true,
	"--setting":         true,
	"--agent-context":   true,
}

// Subcommand returns buck2's subcommand in args and its index, skipping the
// universal options before it, or "" and -1 when there is none. In
// `--isolation-dir x test //...`, it is test, at 2.
func Subcommand(args []string) (string, int) {
	for i := 0; i < len(args); i++ {
		arg := args[i]
		if !strings.HasPrefix(arg, "-") {
			return arg, i
		}
		if globalOptionsWithValue[arg] {
			i++ // its value
		}
	}
	return "", -1
}
