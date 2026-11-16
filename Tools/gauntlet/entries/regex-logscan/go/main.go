package main

import (
	"bufio"
	"cmp"
	"fmt"
	"os"
	"regexp"
	"slices"
)

var request = regexp.MustCompile(`^([0-9.]+) - - \[[^\]]+\] "GET (/api/[^ ]+) HTTP/1[.]1" 5[0-9][0-9] `)

func main() {
	if len(os.Args) < 2 {
		os.Exit(2)
	}
	input, err := os.Open(os.Args[1])
	if err != nil {
		os.Exit(2)
	}
	defer input.Close()

	counts := map[string]int{}
	matches := 0
	scanner := bufio.NewScanner(input)
	for scanner.Scan() {
		line := scanner.Bytes()
		groups := request.FindSubmatchIndex(line)
		if groups == nil {
			continue
		}
		counts[string(line[groups[2]:groups[3]])]++
		matches++
	}
	if err := scanner.Err(); err != nil {
		os.Exit(2)
	}

	type client struct {
		ip    string
		count int
	}
	ranked := make([]client, 0, len(counts))
	for ip, count := range counts {
		ranked = append(ranked, client{ip, count})
	}
	slices.SortFunc(ranked, func(a, b client) int {
		return cmp.Or(cmp.Compare(b.count, a.count), cmp.Compare(a.ip, b.ip))
	})
	out := bufio.NewWriter(os.Stdout)
	fmt.Fprintf(out, "matches %d\n", matches)
	for _, c := range ranked[:min(5, len(ranked))] {
		fmt.Fprintf(out, "%d %s\n", c.count, c.ip)
	}
	out.Flush()
}
