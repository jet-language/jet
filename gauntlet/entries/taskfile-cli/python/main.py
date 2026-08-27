import json
import os
import sys


def load():
    if not os.path.exists("tasks.json"):
        return {"tasks": []}
    with open("tasks.json", encoding="utf-8") as f:
        return json.load(f)


def save(store):
    with open("tasks.json", "w", encoding="utf-8") as f:
        json.dump(store, f, separators=(",", ":"), ensure_ascii=False)


def main():
    command = sys.argv[1] if len(sys.argv) > 1 else ""
    store = load()
    tasks = store["tasks"]
    if command == "add":
        text = sys.argv[2] if len(sys.argv) > 2 else ""
        task = {"id": len(tasks) + 1, "text": text, "done": False}
        tasks.append(task)
        save(store)
        print(f"added {task['id']} {text}")
    elif command == "done":
        task_id = int(sys.argv[2]) if len(sys.argv) > 2 else -1
        for task in tasks:
            if task["id"] == task_id:
                task["done"] = True
                save(store)
                print(f"done {task_id}")
                return
        print(f"no task {task_id}")
    elif command == "list":
        open_count = sum(not task["done"] for task in tasks)
        done_count = len(tasks) - open_count
        for task in tasks:
            mark = "x" if task["done"] else " "
            print(f"[{mark}] {task['id']} {task['text']}")
        print(f"open {open_count} done {done_count}")


if __name__ == "__main__":
    main()
