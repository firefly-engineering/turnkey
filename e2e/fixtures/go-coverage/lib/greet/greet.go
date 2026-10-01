// Package greet builds greetings. It imports a package of its own module.
package greet

import "example.com/lib/text"

// Hello greets name.
func Hello(name string) string {
	return "Hello, " + text.Shout(name)
}
