import sqlite3
import sys
from pathlib import Path


if len(sys.argv) != 2:
    raise SystemExit("usage: database_access INPUT_FILE")

connection = sqlite3.connect(":memory:")
try:
    connection.execute("CREATE TABLE agent_rows (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
    for line_number, line in enumerate(Path(sys.argv[1]).read_text().splitlines(), 1):
        if line_number == 1 or not line:
            continue
        fields = line.split("\t")
        if len(fields) != 2:
            print(f"invalid-row|{line_number}|field-count")
            break
        try:
            row_id = int(fields[0])
        except ValueError:
            print(f"invalid-row|{line_number}|id")
            break
        if row_id <= 0:
            print(f"invalid-row|{line_number}|id")
            break
        connection.execute(
            "INSERT INTO agent_rows (id, name) VALUES (?, ?)", (row_id, fields[1])
        )
    else:
        rows = connection.execute("SELECT COUNT(*) FROM agent_rows").fetchone()[0]
        selected = connection.execute(
            "SELECT name FROM agent_rows WHERE id = ?", (2,)
        ).fetchone()[0]
        print(f"rows={rows}")
        print(f"selected={selected}")
        print("table=present")
finally:
    connection.close()
