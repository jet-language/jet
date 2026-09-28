package main

import (
	"bufio"
	"fmt"
	"net"
	"os"
	"strconv"
	"strings"
	"sync"
	"time"
)

type job struct {
	value  int
	result chan<- int
}

func worker(jobs <-chan job, wait *sync.WaitGroup) {
	defer wait.Done()
	for item := range jobs {
		time.Sleep(time.Millisecond)
		item.result <- item.value * item.value
	}
}

func runBatch(count int, jobs chan<- job) int {
	results := make(chan int, count)
	for value := 1; value <= count; value++ {
		jobs <- job{value: value, result: results}
	}
	total := 0
	for value := 0; value < count; value++ {
		total += <-results
	}
	return total
}

func serve(conn net.Conn, jobs chan<- job) bool {
	defer conn.Close()
	line, err := bufio.NewReader(conn).ReadString('\n')
	if err != nil {
		return false
	}
	command := strings.TrimSpace(line)
	response := "error"
	stop := false
	switch {
	case command == "ready":
		response = "ready"
	case command == "shutdown":
		response = "bye"
		stop = true
	case strings.HasPrefix(command, "batch "):
		count, parseErr := strconv.Atoi(strings.TrimPrefix(command, "batch "))
		if parseErr == nil && count > 0 && count <= 32 {
			response = fmt.Sprintf("batch %d total %d", count, runBatch(count, jobs))
		}
	}
	_, _ = fmt.Fprintln(conn, response)
	return stop
}

func main() {
	port := os.Args[1]
	listener, err := net.Listen("tcp", "127.0.0.1:"+port)
	if err != nil {
		return
	}
	jobs := make(chan job, 32)
	var workers sync.WaitGroup
	workers.Add(4)
	for i := 0; i < 4; i++ {
		go worker(jobs, &workers)
	}
	stopping := false
	for !stopping {
		conn, acceptErr := listener.Accept()
		if acceptErr != nil {
			break
		}
		stopping = serve(conn, jobs)
	}
	_ = listener.Close()
	close(jobs)
	workers.Wait()
}
