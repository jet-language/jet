package main

import (
	"fmt"
	"io"
	"net/http"
	"os"
	"strconv"
	"strings"
)

var values = map[string]string{}
var service *http.Server

func reply(w http.ResponseWriter, status int, body string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_, _ = w.Write([]byte(body))
}

func key(path string) (string, bool) {
	if strings.HasPrefix(path, "/kv/") {
		return strings.TrimPrefix(path, "/kv/"), true
	}
	return "", false
}

func handler(w http.ResponseWriter, r *http.Request) {
	switch {
	case r.Method == http.MethodGet && r.URL.Path == "/health":
		reply(w, http.StatusOK, `{"status":"ok"}`)
	case r.Method == http.MethodGet && r.URL.Path == "/shutdown":
		reply(w, http.StatusOK, `{"bye":true}`)
		go func() { _ = service.Close() }()
	case r.Method == http.MethodPut:
		if k, ok := key(r.URL.Path); ok {
			body, _ := io.ReadAll(r.Body)
			values[k] = string(body)
			reply(w, http.StatusOK, fmt.Sprintf(`{"stored":"%s"}`, k))
			return
		}
		fallthrough
	default:
		if k, ok := key(r.URL.Path); ok && r.Method == http.MethodGet {
			if value, found := values[k]; found {
				reply(w, http.StatusOK, fmt.Sprintf(`{"key":"%s","value":"%s"}`, k, value))
			} else {
				reply(w, http.StatusNotFound, `{"error":"not found"}`)
			}
		} else {
			reply(w, http.StatusNotFound, `{"error":"not found"}`)
		}
	}
}

func main() {
	port, _ := strconv.Atoi(os.Args[1])
	service = &http.Server{Addr: fmt.Sprintf("127.0.0.1:%d", port), Handler: http.HandlerFunc(handler)}
	_ = service.ListenAndServe()
}
