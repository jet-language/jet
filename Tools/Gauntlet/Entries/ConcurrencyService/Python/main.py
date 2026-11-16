#!/usr/bin/env python3
import queue
import socket
import sys
import threading
import time


def worker(jobs):
    while True:
        item = jobs.get()
        if item is None:
            jobs.task_done()
            return
        value, results = item
        time.sleep(0.001)
        results.put(value * value)
        jobs.task_done()


def run_batch(count, jobs):
    results = queue.Queue(maxsize=count)
    for value in range(1, count + 1):
        jobs.put((value, results))
    return sum(results.get() for _ in range(count))


def serve(conn, jobs):
    with conn:
        command = conn.makefile("rb").readline().decode("ascii").strip()
        response = "error"
        stop = False
        if command == "ready":
            response = "ready"
        elif command == "shutdown":
            response = "bye"
            stop = True
        elif command.startswith("batch "):
            try:
                count = int(command[6:])
            except ValueError:
                count = 0
            if 0 < count <= 32:
                response = f"batch {count} total {run_batch(count, jobs)}"
        conn.sendall((response + "\n").encode("ascii"))
    return stop


def main():
    port = int(sys.argv[1])
    jobs = queue.Queue(maxsize=32)
    workers = [threading.Thread(target=worker, args=(jobs,)) for _ in range(4)]
    for thread in workers:
        thread.start()
    with socket.socket() as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind(("127.0.0.1", port))
        listener.listen()
        stopping = False
        while not stopping:
            conn, _ = listener.accept()
            stopping = serve(conn, jobs)
    for _ in workers:
        jobs.put(None)
    for thread in workers:
        thread.join()


if __name__ == "__main__":
    main()
