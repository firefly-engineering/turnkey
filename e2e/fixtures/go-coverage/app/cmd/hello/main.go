// Command hello ties the workspace together: it imports packages of its own
// module (config, cache, dump) and of the other member (greet).
package main

import (
	"fmt"
	"os"

	"example.com/app/cache"
	"example.com/app/config"
	"example.com/app/dump"
	"example.com/lib/greet"
)

const defaultConfig = `
name = "turnkey"
cache_size = 4
`

func main() {
	cfg, err := config.Parse(defaultConfig)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	recent, err := cache.Shared(cfg.CacheSize)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	msg := greet.Hello(cfg.Name)
	recent.Add(cfg.Name, msg)
	fmt.Println(msg)
	fmt.Print(dump.Describe(cfg))
}
