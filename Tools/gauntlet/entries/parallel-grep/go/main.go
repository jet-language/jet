package main

import (
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"sync"
)

type result struct {
	path  string
	count int
}

func collectFiles(root string) ([]string, error) {
	files := make([]string, 0)
	err := filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if entry.Type().IsRegular() && strings.HasSuffix(entry.Name(), ".txt") {
			files = append(files, path)
		}
		return nil
	})
	return files, err
}

func countFile(path, needle string) result {
	data, err := os.ReadFile(path)
	if err != nil {
		panic(err)
	}
	count := 0
	for _, line := range strings.Split(string(data), "\n") {
		count += strings.Count(line, needle)
	}
	return result{path, count}
}

func scan(paths []string, needle string) []result {
    jobs := make(chan string)
    workers := runtime.GOMAXPROCS(0)
    batches := make(chan []result, workers)
    var wg sync.WaitGroup
    for range workers {
        wg.Add(1)
        go func() {
            defer wg.Done()
            batch := make([]result, 0)
            for path := range jobs {
                item := countFile(path, needle)
                if item.count > 0 {
                    batch = append(batch, item)
                }
            }
            batches <- batch
        }()
    }
	for _, path := range paths {
		jobs <- path
	}
	close(jobs)
	wg.Wait()
	close(batches)
	matches := make([]result, 0)
	for batch := range batches {
		matches = append(matches, batch...)
	}
	return matches
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
	paths, err := collectFiles(root)
	if err != nil {
		panic(err)
	}
	sort.Strings(paths)
	matches := scan(paths, needle)
	sort.Slice(matches, func(left, right int) bool { return matches[left].path < matches[right].path })
	total := 0
	for _, item := range matches {
		fmt.Printf("%s:%d\n", item.path, item.count)
		total += item.count
	}
	fmt.Printf("files %d/%d total %d\n", len(matches), len(paths), total)
}
