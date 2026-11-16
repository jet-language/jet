package main

import (
	"bufio"
	"encoding/csv"
	"fmt"
	"math"
	"os"
	"sort"
	"strconv"
)

func mean(values []float64) float64 {
	sum := 0.0
	for _, value := range values {
		sum += value
	}
	return sum / float64(len(values))
}

func summary(values []float64) string {
	ordered := append([]float64(nil), values...)
	sort.Float64s(ordered)
	n := len(ordered)
	average := mean(values)
	median := ordered[(n-1)/2]
	p95 := ordered[(19*n+19)/20-1]
	squares := 0.0
	for _, value := range values {
		delta := value - average
		squares += delta * delta
	}
	return fmt.Sprintf("n=%d mean=%.2f median=%.2f p95=%.2f sd=%.2f", n, average, median, p95, math.Sqrt(squares/float64(n)))
}

func main() {
	path := "measurements.csv"
	if len(os.Args) > 1 {
		path = os.Args[1]
	}
	file, err := os.Open(path)
	if err != nil {
		panic(err)
	}
	defer file.Close()
	rows, err := csv.NewReader(file).ReadAll()
	if err != nil {
		panic(err)
	}
	groupIndex, valueIndex := -1, -1
	for index, name := range rows[0] {
		switch name {
		case "group":
			groupIndex = index
		case "value":
			valueIndex = index
		}
	}
	groups := map[string][]float64{}
	for _, row := range rows[1:] {
		value, err := strconv.ParseFloat(row[valueIndex], 64)
		if err != nil {
			panic(err)
		}
		groups[row[groupIndex]] = append(groups[row[groupIndex]], value)
	}
	names := make([]string, 0, len(groups))
	for name := range groups {
		names = append(names, name)
	}
	sort.Strings(names)
	out := bufio.NewWriter(os.Stdout)
	defer out.Flush()
	var all []float64
	for _, name := range names {
		all = append(all, groups[name]...)
		fmt.Fprintf(out, "%s %s\n", name, summary(groups[name]))
	}
	fmt.Fprintf(out, "overall n=%d mean=%.2f\n", len(all), mean(all))
}
