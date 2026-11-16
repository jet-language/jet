package main

import (
	"bufio"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
)

var pattern = regexp.MustCompile(`^IMG_([0-9]+)\.jpeg$`)

type photo struct {
	number int
	name   string
}

func main() {
	directory := "photos"
	if len(os.Args) > 1 {
		directory = os.Args[1]
	}
	entries, err := os.ReadDir(directory)
	if err != nil {
		panic(err)
	}
	var photos []photo
	files := 0
	for _, entry := range entries {
		if !entry.Type().IsRegular() {
			continue
		}
		files++
		if match := pattern.FindStringSubmatch(entry.Name()); match != nil {
			number, err := strconv.Atoi(match[1])
			if err != nil {
				panic(err)
			}
			photos = append(photos, photo{number, entry.Name()})
		}
	}
	sort.Slice(photos, func(i, j int) bool {
		if photos[i].number != photos[j].number {
			return photos[i].number < photos[j].number
		}
		return photos[i].name < photos[j].name
	})
	out := bufio.NewWriter(os.Stdout)
	defer out.Flush()
	for index, current := range photos {
		target := fmt.Sprintf("photo-%04d.jpg", index+1)
		if err := os.Rename(filepath.Join(directory, current.name), filepath.Join(directory, target)); err != nil {
			panic(err)
		}
		fmt.Fprintf(out, "renamed %s -> %s\n", current.name, target)
	}
	fmt.Fprintf(out, "renamed %d skipped %d\n", len(photos), files-len(photos))
}
