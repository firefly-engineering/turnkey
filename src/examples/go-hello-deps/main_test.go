package main

import (
	"testing"

	"github.com/google/uuid"
)

func TestGreeting(t *testing.T) {
	id := uuid.MustParse("6ba7b810-9dad-11d1-80b4-00c04fd430c8")
	want := "Hello from turnkey! Generated UUID: 6ba7b810-9dad-11d1-80b4-00c04fd430c8"
	if got := greeting(id); got != want {
		t.Errorf("greeting(%s) = %q, want %q", id, got, want)
	}
}
