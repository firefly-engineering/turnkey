// Package config reads the app's TOML configuration with
// github.com/BurntSushi/toml, a module whose path has upper-case letters
// (stored as !burnt!sushi in the module cache) and whose root package
// imports its own internal/ package.
package config

import "github.com/BurntSushi/toml"

// Config is the app's configuration.
type Config struct {
	Name      string `toml:"name"`
	CacheSize int    `toml:"cache_size"`
}

// Parse decodes a TOML document.
func Parse(doc string) (Config, error) {
	var c Config
	_, err := toml.Decode(doc, &c)
	return c, err
}
