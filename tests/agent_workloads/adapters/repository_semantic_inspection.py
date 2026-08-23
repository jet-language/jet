import sys
from pathlib import Path


if len(sys.argv) != 2:
    raise SystemExit("usage: repository_semantic_inspection INPUT_ROOT")

source = Path(sys.argv[1]) / "project" / "examples" / "main.jet"
lines = source.read_text().splitlines()
definitions = sum(line.startswith("fn ") for line in lines)
references = sum(line.lstrip().startswith(("print(", "prepare(")) for line in lines)
print(f"definitions={definitions}")
print(f"references={references}")
print(f"calls={references}")
