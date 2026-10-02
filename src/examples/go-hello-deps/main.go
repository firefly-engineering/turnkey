package main

import (
	"fmt"
	"runtime"

	"github.com/google/uuid"
	"golang.org/x/sys/cpu"
)

// greeting is the line main prints for a generated UUID.
func greeting(id uuid.UUID) string {
	return fmt.Sprintf("Hello from turnkey! Generated UUID: %s", id)
}

func main() {
	fmt.Println(greeting(uuid.New()))

	// Use golang.org/x/sys/cpu to demonstrate assembly-based dependency
	fmt.Printf("Running on %s/%s\n", runtime.GOOS, runtime.GOARCH)
	fmt.Printf("CPU has AVX: %v\n", cpu.X86.HasAVX)
	fmt.Printf("CPU has SSE4.2: %v\n", cpu.X86.HasSSE42)
}
