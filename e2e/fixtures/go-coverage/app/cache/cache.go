// Package cache keeps recent greetings, with github.com/hashicorp/golang-lru/v2,
// a /v2 module whose packages import each other: lru wraps simplelru, which
// imports the module's internal/ package.
//
// This package imports simplelru directly as well as through lru, so its
// deps must list both: rules sync adds simplelru as a direct dep even though
// the build would find it through lru's own deps.
package cache

import (
	lru "github.com/hashicorp/golang-lru/v2"
	"github.com/hashicorp/golang-lru/v2/simplelru"
)

// Shared returns a cache safe for concurrent use.
func Shared(size int) (*lru.Cache[string, string], error) {
	return lru.New[string, string](size)
}

// Local returns a cache for a single goroutine.
func Local(size int) (*simplelru.LRU[string, string], error) {
	return simplelru.NewLRU[string, string](size, nil)
}
