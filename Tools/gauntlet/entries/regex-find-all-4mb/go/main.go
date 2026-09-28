package main

import (
	"bytes"
	"fmt"
	"os"
)

func main() {
	path := "corpus.txt"
	if len(os.Args) > 1 {
		path = os.Args[1]
	}
	text, err := os.ReadFile(path)
	if err != nil {
		panic(err)
	}
	fmt.Printf("matches %d\n", bytes.Count(text, []byte("struct")))
}
