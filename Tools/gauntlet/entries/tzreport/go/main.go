package main

import (
	"bufio"
	"fmt"
	"os"
	"sort"
	"strings"
	"time"
)

type meeting struct {
	name string
	utc  time.Time
}

func main() {
	file, err := os.Open(os.Args[1])
	if err != nil {
		panic(err)
	}
	defer file.Close()
	var meetings []meeting
	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		name, raw, ok := strings.Cut(scanner.Text(), "|")
		if !ok {
			continue
		}
		local, err := time.Parse(time.RFC3339, raw)
		if err != nil {
			panic(err)
		}
		meetings = append(meetings, meeting{name, local.UTC()})
	}
	if err := scanner.Err(); err != nil {
		panic(err)
	}
	sort.Slice(meetings, func(i, j int) bool {
		if !meetings[i].utc.Equal(meetings[j].utc) {
			return meetings[i].utc.Before(meetings[j].utc)
		}
		return meetings[i].name < meetings[j].name
	})
	out := bufio.NewWriter(os.Stdout)
	defer out.Flush()
	for position, current := range meetings {
		gap := "-"
		if position > 0 {
			gap = fmt.Sprint(int64(current.utc.Sub(meetings[position-1].utc) / time.Minute))
		}
		fmt.Fprintf(out, "%s utc=%s day=%s gap=%s\n", current.name, current.utc.Format("2006-01-02T15:04:05Z"), current.utc.Format("Mon"), gap)
	}
	span := meetings[len(meetings)-1].utc.Sub(meetings[0].utc) / time.Minute
	fmt.Fprintf(out, "span %d minutes %d\n", int64(span), len(meetings))
}
