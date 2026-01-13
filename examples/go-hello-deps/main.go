package main

import (
	"fmt"
	"runtime"

	"github.com/google/uuid"
	"golang.org/x/sys/cpu"
)

func main() {
	id := uuid.New()
	fmt.Printf("Hello from turnkey! Generated UUID: %s\n", id)

	// Use golang.org/x/sys/cpu to demonstrate assembly-based dependency
	fmt.Printf("Running on %s/%s\n", runtime.GOOS, runtime.GOARCH)
	fmt.Printf("CPU has AVX: %v\n", cpu.X86.HasAVX)
	fmt.Printf("CPU has SSE4.2: %v\n", cpu.X86.HasSSE42)
}
