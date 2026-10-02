// An external test package: it sees greet only through its exported API,
// and go_test builds it as a package of its own, example.com/lib/greet_test.
package greet_test

import (
	"testing"

	"example.com/lib/greet"
	"example.com/lib/greet/greettest"
	"example.com/lib/text"
)

func TestHelloShouts(t *testing.T) {
	if got, want := greet.Hello("go"), "Hello, "+text.Shout("go"); got != want {
		t.Errorf("Hello(%q) = %q, want %q", "go", got, want)
	}
}

// greettest imports greet itself: the test binary links one greet, the
// one built with its internal tests, as with go test.
func TestHelloThroughHelper(t *testing.T) {
	greettest.AssertHello(t, "helper", "Hello, HELPER!")
}
