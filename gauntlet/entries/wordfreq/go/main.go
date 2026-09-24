package main

import (
	"fmt"
	"os"
	"sort"
	"unicode/utf8"
)

func isSpace(b byte) bool {
	return b == ' ' || (b >= '\t' && b <= '\r')
}

func main() {
	input, err := os.ReadFile(os.Args[1])
	if err != nil {
		panic(err)
	}
	if !utf8.Valid(input) {
		panic("invalid utf8")
	}

	counts := make(map[string]int)
	total := 0
	for start, end := 0, 0; end <= len(input); end++ {
		if end < len(input) && !isSpace(input[end]) {
			continue
		}
		if end > start {
			counts[string(input[start:end])]++
			total++
		}
		start = end + 1
	}

	words := make([]string, 0, len(counts))
	for word := range counts {
		words = append(words, word)
	}
	sort.Slice(words, func(i, j int) bool {
		if counts[words[i]] != counts[words[j]] {
			return counts[words[i]] > counts[words[j]]
		}
		return words[i] < words[j]
	})
	for i, word := range words {
		if i == 20 {
			break
		}
		fmt.Printf("%d %s\n", counts[word], word)
	}
	fmt.Printf("distinct %d total %d\n", len(words), total)
}
