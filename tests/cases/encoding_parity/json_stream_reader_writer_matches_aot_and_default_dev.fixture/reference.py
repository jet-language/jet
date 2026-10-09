"""Matched Python API fixture for encoding_json_stream.jet."""

import json
import sys
from pathlib import Path


path = Path(sys.argv[1])
with path.open("w", encoding="utf-8") as output:
    json.dump({"b": 2, "a": 1}, output, sort_keys=True, separators=(",", ":"))

with path.open(encoding="utf-8") as input_file:
    assert json.load(input_file) == {"a": 1, "b": 2}

print(path.read_text(encoding="utf-8"), end="")
