// #1414 peer adapter. Upstream identity: TechEmpower FrameworkBenchmarks
// 57d92fbec6f8fd7431bc77326dd0484e60c96e20.
package main

import (
	"bufio"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"strconv"
	"strings"
)

func handler(w http.ResponseWriter, r *http.Request) {
	switch r.URL.Path {
	case "/health":
		fmt.Fprint(w, "health")
	case "/ready":
		fmt.Fprint(w, "ready")
	case "/sum":
		raw, _ := io.ReadAll(r.Body)
		sum := 0
		for _, part := range strings.Split(string(raw), ",") {
			n, err := strconv.Atoi(part)
			if err != nil {
				http.Error(w, "reject=bad-number", http.StatusBadRequest)
				return
			}
			sum += n
		}
		fmt.Fprintf(w, "sum=%d", sum)
	default:
		http.NotFound(w, r)
	}
}

func main() {
	raw, err := os.ReadFile(os.Args[1])
	if err != nil {
		panic(err)
	}
	server := httptest.NewServer(http.HandlerFunc(handler))
	defer server.Close()
	client := server.Client()
	requests := 0
	scanner := bufio.NewScanner(strings.NewReader(string(raw)))
	for scanner.Scan() {
		line := scanner.Text()
		if line == "" {
			continue
		}
		fields := strings.SplitN(line, "|", 3)
		if len(fields) != 3 {
			continue
		}
		req, err := http.NewRequest(fields[0], server.URL+fields[1], strings.NewReader(fields[2]))
		if err != nil {
			panic(err)
		}
		response, err := client.Do(req)
		if err != nil {
			panic(err)
		}
		body, _ := io.ReadAll(response.Body)
		response.Body.Close()
		switch fields[1] {
		case "/health":
			fmt.Printf("health=%d\n", response.StatusCode)
		case "/ready":
			fmt.Printf("ready=%d\n", response.StatusCode)
		case "/sum":
			if response.StatusCode == http.StatusBadRequest {
				fmt.Println("reject=bad-number")
			} else {
				fmt.Println(string(body))
			}
		case "/missing":
			fmt.Printf("missing=%d\n", response.StatusCode)
		}
		requests++
	}
	fmt.Printf("requests=%d\n", requests)
}
