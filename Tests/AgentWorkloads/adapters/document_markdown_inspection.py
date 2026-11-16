import sys
from pathlib import Path


if len(sys.argv) != 2:
    raise SystemExit("usage: document_markdown_inspection INPUT_ROOT")

rows = []
for path in sorted(Path(sys.argv[1]).rglob("*")):
    if not path.is_file():
        continue
    headings = 0
    bullets = 0
    malformed = False
    for line in path.read_text().splitlines():
        stripped = line.strip()
        if line.startswith("#"):
            if stripped == "#":
                malformed = True
            else:
                headings += 1
        if line.startswith("- "):
            bullets += 1
    relative = path.relative_to(sys.argv[1]).as_posix()
    if malformed:
        rows.append(f"reject|{relative}|empty-heading")
    else:
        rows.append(f"document|{relative}|headings={headings}|bullets={bullets}")
for row in sorted(rows):
    print(row)
