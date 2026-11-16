import sys
from pathlib import Path


if len(sys.argv) != 2:
    raise SystemExit("usage: desktop_interaction_focus INPUT_FILE")

focus = ["Save", "Cancel"]
index = 1
for line_number, key in enumerate(Path(sys.argv[1]).read_text().splitlines(), 1):
    if line_number == 1 or not key:
        continue
    if key == "Tab":
        print(f"focus|{focus[index]}")
        index = (index + 1) % len(focus)
    elif key == "Empty":
        print("event|Empty|observed")
    else:
        print(f"event|{key}|observed")
