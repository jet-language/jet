#!/usr/bin/env python3
"""Reject previously mined sources (videos, repos, articles) before expensive capture."""

import argparse
import json
import re
from pathlib import Path
from urllib.parse import parse_qs, urlparse

_YT_ID = re.compile(r"[A-Za-z0-9_-]{11}")


def _youtube_id(value: str) -> str | None:
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
    elif not parsed.netloc and _YT_ID.fullmatch(value):
        candidate = value
    else:
        return None
    if not _YT_ID.fullmatch(candidate):
        raise ValueError(f"cannot extract YouTube video ID from {value!r}")
    return candidate


def stable_id(value: str) -> str:
    """One stable identity per source: bare YouTube ID, or normalized host/path."""
    youtube = _youtube_id(value)
    if youtube is not None:
        return youtube
    parsed = urlparse(value if "://" in value else f"https://{value}")
    host = parsed.netloc.lower().removeprefix("www.")
    if not host or "." not in host:
        raise ValueError(f"cannot derive a source identity from {value!r}")
    path = parsed.path.rstrip("/")
    path = path.removesuffix(".git")
    return f"{host}{path}"


def _registry_pattern(identity: str) -> re.Pattern[str]:
    if _YT_ID.fullmatch(identity):
        return re.compile(rf"(?<![A-Za-z0-9_-]){re.escape(identity)}(?![A-Za-z0-9_-])")
    # Allow URL prefixes such as "https://" and "www." before the identity, but
    # reject longer hosts/paths on either side ("notexample.com", "…/posts").
    return re.compile(rf"(?<![A-Za-z0-9-]){re.escape(identity)}(?:\.git)?(?![A-Za-z0-9/_-])")


def _canonical_url(identity: str) -> str:
    if _YT_ID.fullmatch(identity):
        return f"https://www.youtube.com/watch?v={identity}"
    return f"https://{identity}"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("urls", nargs="+")
    parser.add_argument("--registry", default="docs/reference/prior-art.md")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--allow-rerun",
        action="append",
        default=[],
        metavar="SOURCE",
        type=stable_id,
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
        identity = stable_id(value)
        pattern = _registry_pattern(identity)
        matches = [index for index, line in enumerate(lines, 1) if pattern.search(line)]
        first_input = seen.get(identity)
        if first_input is not None:
            status = "duplicate-input"
            input_duplicate = True
        else:
            status = "tracked" if matches else "new"
            if matches:
                tracked_ids.add(identity)
                unapproved_tracked |= identity not in approved
            else:
                untracked = True
            seen[identity] = input_index
        results.append({
            "source_id": identity,
            "canonical_url": _canonical_url(identity),
            "status": status,
            "registry": str(registry),
            "matched_lines": matches,
            "first_input": first_input,
            "rerun_approved": identity in approved,
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
