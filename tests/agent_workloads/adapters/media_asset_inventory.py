import sys
from pathlib import Path


if len(sys.argv) != 2:
    raise SystemExit("usage: media_asset_inventory INPUT_ROOT")

types = {"ppm": "image/x-portable-pixmap", "svg": "image/svg+xml", "mp3": "audio/mpeg"}
for path in sorted(Path(sys.argv[1]).rglob("*")):
    if not path.is_file():
        continue
    relative = path.relative_to(sys.argv[1]).as_posix()
    media_type = types.get(path.suffix.removeprefix("."))
    if media_type is None:
        print(f"reject|{relative}|extension")
    else:
        print(f"asset|{relative}|{media_type}|{len(path.read_bytes())}")
