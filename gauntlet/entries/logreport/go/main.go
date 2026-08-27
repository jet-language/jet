package main

import (
	"bufio"
	"fmt"
	"os"
	"sort"
	"strings"
	"time"
)

type componentCount struct {
	name  string
	count int
}

func main() {
	path := os.Args[1]
	file, err := os.Open(path)
	if err != nil {
		panic(err)
	}
	defer file.Close()
	levels := []string{"DEBUG", "INFO", "WARN", "ERROR"}
	levelCounts := make(map[string]int)
	errorCounts := make(map[string]int)
	firstText, lastText := "", ""
	var firstTime, lastTime time.Time
	total := 0
	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		fields := strings.SplitN(scanner.Text(), " ", 4)
		timestampText, level, component := fields[0], fields[1], fields[2]
		timestamp, err := time.Parse(time.RFC3339, timestampText)
		if err != nil {
			panic(err)
		}
		if total == 0 {
			firstText, firstTime = timestampText, timestamp
		}
		lastText, lastTime = timestampText, timestamp
		levelCounts[level]++
		if level == "ERROR" {
			errorCounts[component]++
		}
		total++
	}
	if err := scanner.Err(); err != nil {
		panic(err)
	}
	for _, level := range levels {
		fmt.Printf("%s %d\n", level, levelCounts[level])
	}
	fmt.Println("top-error-components:")
	ranked := make([]componentCount, 0, len(errorCounts))
	for name, count := range errorCounts {
		ranked = append(ranked, componentCount{name, count})
	}
	sort.Slice(ranked, func(i, j int) bool {
		if ranked[i].count != ranked[j].count {
			return ranked[i].count > ranked[j].count
		}
		return ranked[i].name < ranked[j].name
	})
	for _, item := range ranked[:3] {
		fmt.Printf("%d %s\n", item.count, item.name)
	}
	fmt.Printf("span %s .. %s (%ds)\n", firstText, lastText, int(lastTime.Sub(firstTime).Seconds()))
}
