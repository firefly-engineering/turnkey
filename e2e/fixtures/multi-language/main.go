package main

import (
	"fmt"

	"github.com/google/uuid"
)

func main() {
	id := uuid.New()
	fmt.Printf("Go: Hello from multi-language project! UUID: %s\n", id)
}
