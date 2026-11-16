package main

import (
	"bufio"
	"fmt"
	"os"
	"strconv"
)

// An odd-only byte sieve: slot i stands for 2*i + 1, and slot 0 (the number 1)
// is never counted.
func main() {
	n, err := strconv.Atoi(os.Args[1])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	slots := n / 2
	composite := make([]bool, slots)
	for p := 3; p*p < n; p += 2 {
		if !composite[p/2] {
			for m := p * p / 2; m < slots; m += p {
				composite[m] = true
			}
		}
	}
	count, largest := 0, 0
	if n > 2 {
		count, largest = 1, 2
		for i := 1; i < slots; i++ {
			if !composite[i] {
				count++
				largest = 2*i + 1
			}
		}
	}
	out := bufio.NewWriter(os.Stdout)
	fmt.Fprintf(out, "count %d\nlargest %d\n", count, largest)
	out.Flush()
}
