// Package text formats the words other packages print.
package text

import "strings"

// Shout returns s in upper case, with an exclamation mark.
func Shout(s string) string {
	return strings.ToUpper(s) + "!"
}
