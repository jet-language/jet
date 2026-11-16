import re
import sys
from pathlib import Path


def main() -> None:
    directory = Path(sys.argv[1])
    photos = []
    files = 0
    for entry in directory.iterdir():
        if entry.is_file():
            files += 1
            match = re.fullmatch(r"IMG_(\d+)\.jpeg", entry.name)
            if match:
                photos.append((int(match.group(1)), entry))
    photos.sort()
    for index, (_, source) in enumerate(photos, 1):
        target = directory / f"photo-{index:04}.jpg"
        source.rename(target)
        print(f"renamed {source.name} -> {target.name}")
    print(f"renamed {len(photos)} skipped {files - len(photos)}")


if __name__ == "__main__":
    main()
