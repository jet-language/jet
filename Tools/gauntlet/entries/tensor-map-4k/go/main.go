package main

import (
	"fmt"
	"os"
)

const (
	side     = 4096
	elements = side * side
)

func filled(value float64) []float64 {
	values := make([]float64, elements)
	for i := range values {
		values[i] = value
	}
	return values
}

func main() {
	left := filled(1)
	right := filled(2)
	mapped := make([]float64, elements)
	for i := range mapped {
		mapped[i] = left[i] + right[i]
	}

	corner := mapped[elements-1]
	checksum := 0.0
	for _, x := range mapped {
		checksum += x
	}
	if corner != 3 || checksum != 50331648 {
		os.Exit(1)
	}
	fmt.Printf("shape:[%d, %d] numel:%d corner:%.1f checksum:%.0f\n", side, side, elements, corner, checksum)
}
