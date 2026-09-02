// #1414 peer adapter: pinned Go standard-library implementation of the
// frozen loopback service workload. The task shape is informed by the
// TechEmpower HTTP definitions, but this file is a self-contained adapter.
package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

const defaultBodyLimit = int64(64)

type config struct {
	mode        string
	bind        string
	workers     int
	timeout     time.Duration
	shutdown    time.Duration
	bodyLimit   int64
	readyAfter  time.Duration
}

func safeConfig() config {
	return config{
		mode:       "beginner",
		bind:       "127.0.0.1:0",
		workers:    4,
		timeout:    200 * time.Millisecond,
		shutdown:   100 * time.Millisecond,
		bodyLimit:  defaultBodyLimit,
		readyAfter: 200 * time.Millisecond,
	}
}

func parseNumber(text, field string) (int64, error) {
	value, err := strconv.ParseInt(text, 10, 64)
	if err != nil {
		return 0, fmt.Errorf("invalid %s: %w", field, err)
	}
	return value, nil
}

func parseConfig(fields []string) (config, error) {
	cfg := safeConfig()
	if len(fields) < 2 {
		return cfg, nil
	}
	cfg.mode = fields[1]
	if cfg.mode != "beginner" && cfg.mode != "expert" {
		return cfg, fmt.Errorf("unknown service mode %q", cfg.mode)
	}
	if len(fields) == 2 {
		return cfg, nil
	}
	if len(fields) != 8 {
		return cfg, fmt.Errorf("CONFIG requires mode, bind, workers, timeout, shutdown, body-limit, ready-after")
	}
	cfg.bind = fields[2]
	workers, err := parseNumber(fields[3], "workers")
	if err != nil {
		return cfg, err
	}
	timeout, err := parseNumber(fields[4], "timeout")
	if err != nil {
		return cfg, err
	}
	shutdown, err := parseNumber(fields[5], "shutdown")
	if err != nil {
		return cfg, err
	}
	limit, err := parseNumber(fields[6], "body-limit")
	if err != nil {
		return cfg, err
	}
	readyAfter, err := parseNumber(fields[7], "ready-after")
	if err != nil {
		return cfg, err
	}
	if workers < 1 || timeout < 1 || shutdown < 1 || limit < 1 || readyAfter < 0 {
		return cfg, fmt.Errorf("service controls must be positive except ready-after")
	}
	cfg.workers = int(workers)
	cfg.timeout = time.Duration(timeout) * time.Millisecond
	cfg.shutdown = time.Duration(shutdown) * time.Millisecond
	cfg.bodyLimit = limit
	cfg.readyAfter = time.Duration(readyAfter) * time.Millisecond
	return cfg, nil
}

type sumRequest struct {
	Numbers []int `json:"numbers"`
}

type sumResponse struct {
	Sum int `json:"sum"`
}

type statusResponse struct {
	Status string `json:"status"`
}

type errorResponse struct {
	Error string `json:"error"`
}

type service struct {
	cfg       config
	ready     atomic.Bool
	workers   chan struct{}
	readyDone chan struct{}
	readyOnce sync.Once
}

func newService(cfg config) *service {
	return &service{
		cfg:       cfg,
		workers:   make(chan struct{}, cfg.workers),
		readyDone: make(chan struct{}),
	}
}

func (s *service) startReadiness() {
	if s.cfg.readyAfter == 0 {
		s.ready.Store(true)
		close(s.readyDone)
		return
	}
	go func() {
		timer := time.NewTimer(s.cfg.readyAfter)
		defer timer.Stop()
		<-timer.C
		s.ready.Store(true)
		s.readyOnce.Do(func() { close(s.readyDone) })
	}()
}

func writeJSON(w http.ResponseWriter, status int, value any) {
	body, err := json.Marshal(value)
	if err != nil {
		status = http.StatusInternalServerError
		body = []byte(`{"error":"internal"}`)
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_, _ = w.Write(body)
}

func writeError(w http.ResponseWriter, status int, message string) {
	writeJSON(w, status, errorResponse{Error: message})
}

func (s *service) withWorker(w http.ResponseWriter, work func()) {
	s.workers <- struct{}{}
	defer func() { <-s.workers }()
	work()
}

func (s *service) handleSum(w http.ResponseWriter, r *http.Request) {
	if r.ContentLength > s.cfg.bodyLimit {
		writeError(w, http.StatusRequestEntityTooLarge, "body-too-large")
		return
	}
	r.Body = http.MaxBytesReader(w, r.Body, s.cfg.bodyLimit)
	decoder := json.NewDecoder(r.Body)
	var request sumRequest
	if err := decoder.Decode(&request); err != nil {
		var tooLarge *http.MaxBytesError
		if errors.As(err, &tooLarge) {
			writeError(w, http.StatusRequestEntityTooLarge, "body-too-large")
		} else {
			writeError(w, http.StatusBadRequest, "malformed-json")
		}
		return
	}
	var trailing any
	if err := decoder.Decode(&trailing); !errors.Is(err, io.EOF) {
		writeError(w, http.StatusBadRequest, "malformed-json")
		return
	}
	if request.Numbers == nil {
		writeError(w, http.StatusBadRequest, "malformed-json")
		return
	}
	sum := 0
	for _, value := range request.Numbers {
		sum += value
	}
	writeJSON(w, http.StatusOK, sumResponse{Sum: sum})
}

func (s *service) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	switch r.URL.Path {
	case "/health":
		if r.Method != http.MethodGet {
			writeError(w, http.StatusMethodNotAllowed, "method-not-allowed")
			return
		}
		writeJSON(w, http.StatusOK, statusResponse{Status: "ok"})
	case "/ready":
		if r.Method != http.MethodGet {
			writeError(w, http.StatusMethodNotAllowed, "method-not-allowed")
			return
		}
		if !s.ready.Load() {
			writeError(w, http.StatusServiceUnavailable, "not-ready")
			return
		}
		writeJSON(w, http.StatusOK, statusResponse{Status: "ready"})
	case "/sum":
		if r.Method != http.MethodPost {
			writeError(w, http.StatusMethodNotAllowed, "method-not-allowed")
			return
		}
		s.withWorker(w, func() { s.handleSum(w, r) })
	default:
		writeError(w, http.StatusNotFound, "not-found")
	}
}

func loopbackListener(bind string) (net.Listener, error) {
	listener, err := net.Listen("tcp", bind)
	if err != nil {
		return nil, err
	}
	address, ok := listener.Addr().(*net.TCPAddr)
	if !ok || address.IP == nil || !address.IP.IsLoopback() {
		_ = listener.Close()
		return nil, fmt.Errorf("service bind must be loopback: %s", bind)
	}
	return listener, nil
}

func doRequest(client *http.Client, base, method, path, body string) (int, []byte, error) {
	request, err := http.NewRequest(method, base+path, strings.NewReader(body))
	if err != nil {
		return 0, nil, err
	}
	if body != "" {
		request.Header.Set("Content-Type", "application/json")
	}
	response, err := client.Do(request)
	if err != nil {
		return 0, nil, err
	}
	defer response.Body.Close()
	payload, err := io.ReadAll(response.Body)
	if err != nil {
		return response.StatusCode, nil, err
	}
	return response.StatusCode, payload, nil
}

func sumBody(body []byte) (int, error) {
	var response sumResponse
	if err := json.Unmarshal(body, &response); err != nil {
		return 0, err
	}
	return response.Sum, nil
}

func printRequestResult(client *http.Client, base, method, path, body string) error {
	status, payload, err := doRequest(client, base, method, path, body)
	if err != nil {
		return err
	}
	switch path {
	case "/health":
		fmt.Printf("health=%d\n", status)
	case "/ready":
		fmt.Printf("ready=%d\n", status)
	case "/sum":
		if status == http.StatusOK {
			sum, err := sumBody(payload)
			if err != nil {
				return err
			}
			fmt.Printf("sum=%d\n", sum)
		} else {
			var failure errorResponse
			if json.Unmarshal(payload, &failure) != nil || failure.Error == "" {
				return fmt.Errorf("invalid JSON error response")
			}
			fmt.Printf("error=%s\n", failure.Error)
		}
	default:
		fmt.Printf("missing=%d\n", status)
	}
	return nil
}

func runConcurrent(client *http.Client, base, path, body string, count int) error {
	if count < 1 {
		return fmt.Errorf("concurrent count must be positive")
	}
	statuses := make([]int, count)
	bodies := make([][]byte, count)
	errorsFound := make([]error, count)
	var group sync.WaitGroup
	for index := range count {
		group.Add(1)
		go func(index int) {
			defer group.Done()
			statuses[index], bodies[index], errorsFound[index] = doRequest(client, base, http.MethodPost, path, body)
		}(index)
	}
	group.Wait()
	for index := range statuses {
		if errorsFound[index] != nil || statuses[index] != http.StatusOK {
			return fmt.Errorf("concurrent request %d failed", index)
		}
		if _, err := sumBody(bodies[index]); err != nil {
			return err
		}
	}
	fmt.Printf("concurrent=%d\n", count)
	return nil
}

func runSlowClient(address, path, body string, timeout time.Duration) string {
	conn, err := net.DialTimeout("tcp", address, timeout)
	if err != nil {
		return "timeout"
	}
	defer conn.Close()
	_ = conn.SetDeadline(time.Now().Add(timeout * 3))
	headers := fmt.Sprintf("POST %s HTTP/1.1\r\nHost: loopback\r\nContent-Type: application/json\r\nContent-Length: %d\r\nConnection: close\r\n\r\n", path, len(body)+1)
	if _, err := io.WriteString(conn, headers); err != nil {
		return "timeout"
	}
	_, _ = io.ReadAll(conn)
	return "timeout"
}

func shutdownServer(server *http.Server, listener net.Listener, serving <-chan error, grace time.Duration) error {
	ctx, cancel := context.WithTimeout(context.Background(), grace)
	defer cancel()
	err := server.Shutdown(ctx)
	if err != nil {
		_ = server.Close()
		return err
	}
	serveErr := <-serving
	if serveErr != nil && !errors.Is(serveErr, http.ErrServerClosed) {
		return serveErr
	}
	_ = listener.Close()
	return nil
}

func run(path string) error {
	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()

	cfg := safeConfig()
	type command struct {
		kind   string
		fields []string
	}
	commands := []command{}
	scanner := bufio.NewScanner(file)
	scanner.Buffer(make([]byte, 1024), 2*1024*1024)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		fields := strings.Split(line, "|")
		commands = append(commands, command{kind: fields[0], fields: fields})
		if fields[0] == "CONFIG" {
			cfg, err = parseConfig(fields)
			if err != nil {
				return err
			}
		}
	}
	if err := scanner.Err(); err != nil {
		return err
	}

	listener, err := loopbackListener(cfg.bind)
	if err != nil {
		return err
	}
	service := newService(cfg)
	service.startReadiness()
	server := &http.Server{
		Handler:           service,
		ReadHeaderTimeout: cfg.timeout,
		ReadTimeout:       cfg.timeout,
		WriteTimeout:      cfg.timeout,
		IdleTimeout:       cfg.timeout * 2,
	}
	serving := make(chan error, 1)
	go func() { serving <- server.Serve(listener) }()
	base := "http://" + listener.Addr().String()
	client := &http.Client{Timeout: cfg.timeout}
	requests := 0
	stopped := false
	defer func() {
		if !stopped {
			_ = shutdownServer(server, listener, serving, cfg.shutdown)
		}
	}()

	for _, command := range commands {
		fields := command.fields
		switch command.kind {
		case "CONFIG":
			continue
		case "WAIT":
			if len(fields) < 2 {
				return fmt.Errorf("WAIT requires milliseconds")
			}
			milliseconds, err := parseNumber(fields[1], "wait")
			if err != nil || milliseconds < 0 {
				return fmt.Errorf("invalid wait duration")
			}
			time.Sleep(time.Duration(milliseconds) * time.Millisecond)
		case "CONCURRENT":
			if len(fields) != 4 {
				return fmt.Errorf("CONCURRENT requires path, JSON body, count")
			}
			count, err := parseNumber(fields[3], "concurrent count")
			if err != nil {
				return err
			}
			if err := runConcurrent(client, base, fields[1], fields[2], int(count)); err != nil {
				return err
			}
			requests += int(count)
		case "MALFORMED":
			if len(fields) != 3 {
				return fmt.Errorf("MALFORMED requires path and body")
			}
			status, _, err := doRequest(client, base, http.MethodPost, fields[1], fields[2])
			if err != nil {
				return err
			}
			fmt.Printf("malformed=%d\n", status)
			requests++
		case "OVERSIZE":
			if len(fields) < 2 || fields[1] != "/sum" {
				return fmt.Errorf("OVERSIZE only supports /sum")
			}
			body := strings.Repeat("x", int(cfg.bodyLimit)+1)
			status, _, err := doRequest(client, base, http.MethodPost, fields[1], body)
			if err != nil {
				return err
			}
			fmt.Printf("oversize=%d\n", status)
			requests++
		case "SLOW":
			if len(fields) != 3 {
				return fmt.Errorf("SLOW requires path and body")
			}
			fmt.Printf("slow=%s\n", runSlowClient(listener.Addr().String(), fields[1], fields[2], cfg.timeout))
			requests++
		case "SHUTDOWN":
			if len(fields) < 2 {
				return fmt.Errorf("SHUTDOWN requires a grace override")
			}
			grace := cfg.shutdown
			if fields[1] != "" {
				milliseconds, err := parseNumber(fields[1], "shutdown")
				if err != nil || milliseconds < 1 {
					return fmt.Errorf("invalid shutdown duration")
				}
				grace = time.Duration(milliseconds) * time.Millisecond
			}
			if err := shutdownServer(server, listener, serving, grace); err != nil {
				return err
			}
			stopped = true
			fmt.Println("shutdown=clean")
		case "GET", "POST", "PUT", "PATCH", "DELETE":
			if len(fields) != 3 {
				return fmt.Errorf("request requires method, path, body")
			}
			if err := printRequestResult(client, base, fields[0], fields[1], fields[2]); err != nil {
				return err
			}
			requests++
		default:
			return fmt.Errorf("unknown service command %q", command.kind)
		}
	}
	if !stopped {
		if err := shutdownServer(server, listener, serving, cfg.shutdown); err != nil {
			return err
		}
		stopped = true
		fmt.Println("shutdown=clean")
	}
	fmt.Printf("requests=%d\n", requests)
	return nil
}

func main() {
	if len(os.Args) != 2 {
		fmt.Fprintln(os.Stderr, "usage: service-json-http INPUT")
		os.Exit(64)
	}
	if err := run(os.Args[1]); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
