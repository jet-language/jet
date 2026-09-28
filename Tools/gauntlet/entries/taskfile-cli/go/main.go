package main

import (
	"encoding/json"
	"fmt"
	"os"
)

type Task struct {
	ID   int    `json:"id"`
	Text string `json:"text"`
	Done bool   `json:"done"`
}

type Store struct {
	Tasks []Task `json:"tasks"`
}

func load() Store {
	b, err := os.ReadFile("tasks.json")
	if err != nil {
		return Store{}
	}
	var store Store
	if json.Unmarshal(b, &store) != nil {
		return Store{}
	}
	return store
}

func save(store Store) {
	b, err := json.Marshal(store)
	if err != nil || os.WriteFile("tasks.json", b, 0644) != nil {
		panic("could not write tasks.json")
	}
}

func main() {
	command := ""
	if len(os.Args) > 1 {
		command = os.Args[1]
	}
	store := load()
	switch command {
	case "add":
		text := ""
		if len(os.Args) > 2 {
			text = os.Args[2]
		}
		task := Task{ID: len(store.Tasks) + 1, Text: text}
		store.Tasks = append(store.Tasks, task)
		save(store)
		fmt.Printf("added %d %s\n", task.ID, text)
	case "done":
		id := -1
		if len(os.Args) > 2 {
			fmt.Sscanf(os.Args[2], "%d", &id)
		}
		for i := range store.Tasks {
			if store.Tasks[i].ID == id {
				store.Tasks[i].Done = true
				save(store)
				fmt.Printf("done %d\n", id)
				return
			}
		}
		fmt.Printf("no task %d\n", id)
	case "list":
		open, done := 0, 0
		for _, task := range store.Tasks {
			if task.Done {
				fmt.Printf("[x] %d %s\n", task.ID, task.Text)
				done++
			} else {
				fmt.Printf("[ ] %d %s\n", task.ID, task.Text)
				open++
			}
		}
		fmt.Printf("open %d done %d\n", open, done)
	}
}
