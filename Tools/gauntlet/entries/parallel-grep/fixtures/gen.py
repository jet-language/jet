from pathlib import Path
import sys


GROUPS = 200
FILES_PER_GROUP = 1000
MATCHING_TEXT = "needle-7fneedle-7f\n"
NON_MATCHING_TEXT = "no match\n"


def main() -> None:
    root = Path(sys.argv[1])
    root.mkdir(parents=True, exist_ok=True)
    for group in range(GROUPS):
        directory = root / f"d{group:03}"
        directory.mkdir()
        for index in range(FILES_PER_GROUP):
            text = MATCHING_TEXT if group == 0 and index == 0 else NON_MATCHING_TEXT
            (directory / f"f{index:04}.txt").write_text(text, encoding="ascii")
        (directory / "ignored.md").write_text("not scanned\n", encoding="ascii")


if __name__ == "__main__":
    main()
