package main

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
)

type result struct {
	path  string
	count int
}

func countFile(path, needle string, results chan<- result, wg *sync.WaitGroup) {
	defer wg.Done()
	data, err := os.ReadFile(path)
	if err != nil {
		panic(err)
	}
	count := 0
	for _, line := range strings.Split(string(data), "\n") {
		count += strings.Count(line, needle)
	}
	results <- result{path, count}
}

func main() {
	root := "files"
	needle := "needle-7f"
	if len(os.Args) > 1 {
		root = os.Args[1]
	}
	if len(os.Args) > 2 {
		needle = os.Args[2]
	}
	entries, err := os.ReadDir(root)
	if err != nil {
		panic(err)
	}
	paths := make([]string, 0, len(entries))
	for _, entry := range entries {
		if !entry.IsDir() && strings.HasSuffix(entry.Name(), ".txt") {
			paths = append(paths, filepath.Join(root, entry.Name()))
		}
	}
	sort.Strings(paths)
	results := make(chan result, len(paths))
	var wg sync.WaitGroup
	for _, path := range paths {
		wg.Add(1)
		go countFile(path, needle, results, &wg)
	}
	wg.Wait()
	close(results)
	matches := make([]result, 0)
	total := 0
	for item := range results {
		if item.count > 0 {
			matches = append(matches, item)
			total += item.count
		}
	}
	sort.Slice(matches, func(i, j int) bool { return matches[i].path < matches[j].path })
	for _, item := range matches {
		fmt.Printf("%s:%d\n", item.path, item.count)
	}
	fmt.Printf("files %d/%d total %d\n", len(matches), len(paths), total)
}
