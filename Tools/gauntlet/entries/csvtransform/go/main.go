package main

import (
	"encoding/csv"
	"fmt"
	"os"
	"sort"
	"strconv"
)

type group struct {
	count int
	sum   float64
}

func main() {
	file, err := os.Open(os.Args[1])
	if err != nil {
		panic(err)
	}
	defer file.Close()

	reader := csv.NewReader(file)
	if _, err := reader.Read(); err != nil {
		panic(err)
	}
	groups := map[string]group{}
	totalCount := 0
	totalSum := 0.0
	for {
		row, err := reader.Read()
		if err != nil {
			break
		}
		amount, err := strconv.ParseFloat(row[3], 64)
		if err != nil || amount <= 0.0 {
			continue
		}
		current := groups[row[1]]
		current.count++
		current.sum += amount
		groups[row[1]] = current
		totalCount++
		totalSum += amount
	}
	keys := make([]string, 0, len(groups))
	for region := range groups {
		keys = append(keys, region)
	}
	sort.Strings(keys)
	for _, region := range keys {
		current := groups[region]
		fmt.Printf("%s n=%d sum=%.2f\n", region, current.count, current.sum)
	}
	fmt.Printf("total n=%d sum=%.2f\n", totalCount, totalSum)
}
