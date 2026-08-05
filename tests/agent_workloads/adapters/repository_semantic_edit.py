import shutil
import sys
from pathlib import Path


if len(sys.argv) != 2:
    raise SystemExit("usage: repository_semantic_edit INPUT_ROOT")

project = Path("project")
shutil.copytree(sys.argv[1], project)
try:
    source = project / "project" / "examples" / "main.jet"
    lines = source.read_text().splitlines(keepends=True)
    edited = []
    for line in lines:
        if line.startswith("fn prepare()"):
            line = line.replace("fn prepare", "fn configure", 1)
        elif line.rstrip("\r\n") == "    prepare()":
            line = line.replace("prepare()", "configure()", 1)
        edited.append(line)
    source.write_text("".join(edited))
    sys.stdout.write(source.read_text())
finally:
    shutil.rmtree(project)
