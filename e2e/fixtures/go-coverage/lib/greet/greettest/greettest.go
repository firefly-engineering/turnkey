// Package greettest holds helpers for greet's tests. It imports greet, the
// package under test of the external test package that uses it: go_test
// recompiles it against greet as built with its internal tests, as go test
// does, so the test binary holds one greet.
package greettest

import (
	"testing"

	"example.com/lib/greet"
)

// AssertHello fails t unless greet.Hello(name) is want.
func AssertHello(t testing.TB, name, want string) {
	t.Helper()
	if got := greet.Hello(name); got != want {
		t.Errorf("Hello(%q) = %q, want %q", name, got, want)
	}
}
