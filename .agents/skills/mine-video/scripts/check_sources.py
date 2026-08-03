#!/usr/bin/env python3
"""Reject previously mined YouTube IDs before expensive capture."""

import argparse
import json
import re
from pathlib import Path
from urllib.parse import parse_qs, urlparse


def video_id(value: str) -> str:
    parsed = urlparse(value)
    host = parsed.netloc.lower().removeprefix("www.")
    if host in {"youtube.com", "m.youtube.com", "music.youtube.com", "youtube-nocookie.com"}:
        if parsed.path == "/watch":
            candidate = parse_qs(parsed.query).get("v", [""])[0]
        else:
            parts = [part for part in parsed.path.split("/") if part]
            candidate = parts[1] if len(parts) > 1 and parts[0] in {"shorts", "embed", "live"} else ""
    elif host == "youtu.be":
        candidate = parsed.path.lstrip("/").split("/", 1)[0]
    else:
        candidate = value if re.fullmatch(r"[A-Za-z0-9_-]{11}", value) else ""
    if not re.fullmatch(r"[A-Za-z0-9_-]{11}", candidate):
        raise ValueError(f"cannot extract YouTube video ID from {value!r}")
    return candidate


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("urls", nargs="+")
    parser.add_argument("--registry", default="docs/reference/prior-art.md")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--allow-rerun",
        action="append",
        default=[],
        metavar="VIDEO_ID",
        type=video_id,
    )
    mode.add_argument("--verify-tracked", action="store_true")
    args = parser.parse_args()

    registry = Path(args.registry)
    if not registry.is_file():
        parser.error(f"source registry does not exist: {registry}")
    lines = registry.read_text().splitlines()
    results = []
    approved = set(args.allow_rerun)
    tracked_ids = set()
    unapproved_tracked = False
    untracked = False
    input_duplicate = False
    seen: dict[str, int] = {}
    for input_index, value in enumerate(args.urls, 1):
        stable_id = video_id(value)
        pattern = re.compile(rf"(?<![A-Za-z0-9_-]){re.escape(stable_id)}(?![A-Za-z0-9_-])")
        matches = [index for index, line in enumerate(lines, 1) if pattern.search(line)]
        first_input = seen.get(stable_id)
        if first_input is not None:
            status = "duplicate-input"
            input_duplicate = True
        else:
            status = "tracked" if matches else "new"
            if matches:
                tracked_ids.add(stable_id)
                unapproved_tracked |= stable_id not in approved
            else:
                untracked = True
            seen[stable_id] = input_index
        results.append({
            "video_id": stable_id,
            "canonical_url": f"https://www.youtube.com/watch?v={stable_id}",
            "status": status,
            "registry": str(registry),
            "matched_lines": matches,
            "first_input": first_input,
            "rerun_approved": stable_id in approved,
        })
    unused_approvals = approved - tracked_ids
    if unused_approvals:
        parser.error(
            "rerun approval does not match a tracked input: "
            + ", ".join(sorted(unused_approvals))
        )
    print(json.dumps(results, indent=2))
    if input_duplicate:
        return 3
    if args.verify_tracked:
        return 4 if untracked else 0
    return 3 if unapproved_tracked else 0


if __name__ == "__main__":
    raise SystemExit(main())
