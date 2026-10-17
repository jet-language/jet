#!/usr/bin/env python3
"""Batch source guard; registry presence is not complete technical mining."""
import argparse
import json
import re
from pathlib import Path
from urllib.parse import parse_qs, parse_qsl, urlencode, urlparse

_YT_ID = re.compile(r"[A-Za-z0-9_-]{11}")
_TRACKING = {"fbclid", "gclid", "msclkid", "mc_cid", "mc_eid"}


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
        raise ValueError(f"cannot extract YouTube video ID from {value!r}; playlist expansion requires explicit scope")
    return candidate


def stable_id(value: str) -> str:
    """Historical source ID stays unchanged; content_id carries selectors."""
    youtube = _youtube_id(value)
    if youtube is not None:
        return youtube
    parsed = urlparse(value if "://" in value else f"https://{value}")
    host = parsed.netloc.lower().removeprefix("www.")
    if not host or "." not in host:
        raise ValueError(f"cannot derive a source identity from {value!r}")
    return f"{host}{parsed.path.rstrip('/').removesuffix('.git')}"


def content_id(value: str) -> str:
    identity = stable_id(value)
    if _YT_ID.fullmatch(identity):
        return identity
    parsed = urlparse(value if "://" in value else f"https://{value}")
    query = [(key, val) for key, val in parse_qsl(parsed.query, keep_blank_values=True)
             if not key.lower().startswith("utm_") and key.lower() not in _TRACKING]
    # Keep order/repeated keys and fragments: their meaning is resource-specific.
    return identity + ("?" + urlencode(query) if query else "") + ("#" + parsed.fragment if parsed.fragment else "")


def _registry_pattern(identity: str) -> re.Pattern[str]:
    if _YT_ID.fullmatch(identity):
        return re.compile(rf"(?<![A-Za-z0-9_-]){re.escape(identity)}(?![A-Za-z0-9_-])")
    return re.compile(rf"(?<![A-Za-z0-9.-])(?:www\.)?{re.escape(identity)}(?:\.git)?(?![A-Za-z0-9/_.?&#=:%+~-])")


def _line_matches(line: str, resource: str) -> bool:
    if _registry_pattern(resource).search(line):
        return True
    for url in re.findall(r"https?://[^\s<>\"'\)\]]+", line):
        try:
            if content_id(url) == resource:
                return True
        except ValueError:
            continue
    return False


def _canonical_url(identity: str) -> str:
    return f"https://www.youtube.com/watch?v={identity}" if _YT_ID.fullmatch(identity) else f"https://{identity}"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("urls", nargs="+")
    parser.add_argument("--registry", default="Docs/spec/reference/prior-art.md")
    parser.add_argument("--evidence-index", help="optional jet-state:v1 JSON; report links are leads, not proof of review")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--allow-rerun", action="append", default=[], metavar="SOURCE", type=content_id)
    mode.add_argument("--verify-tracked", action="store_true")
    args = parser.parse_args()
    registry = Path(args.registry)
    if not registry.is_file():
        parser.error(f"source registry does not exist: {registry}")
    lines = registry.read_text().splitlines()
    evidence = []
    if args.evidence_index:
        packet = json.loads(Path(args.evidence_index).read_text())
        if packet.get("schema") != "jet-state:v1":
            parser.error("evidence index must be jet-state:v1")
        evidence = packet.get("research", [])
    results = []
    approved = set(args.allow_rerun)
    tracked_ids = set()
    unapproved_tracked = untracked = input_duplicate = False
    seen: dict[str, int] = {}
    for input_index, value in enumerate(args.urls, 1):
        identity, resource = stable_id(value), content_id(value)
        matches = [index for index, line in enumerate(lines, 1) if _line_matches(line, resource)]
        historical = [index for index, line in enumerate(lines, 1)
                      if identity != resource and _registry_pattern(identity).search(line)]
        leads = []
        for report in evidence:
            matched = []
            for url in report.get("source_urls", []):
                try:
                    if content_id(url) == resource:
                        matched.append(url)
                except ValueError:
                    continue
            if matched:
                leads.append({"report": report.get("path"), "urls": matched,
                              "coverage": report.get("coverage", "unknown"), "state": report.get("state", "unknown")})
        first_input = seen.get(resource)
        if first_input is not None:
            status = "duplicate-input"
            input_duplicate = True
        else:
            status = "tracked" if matches or leads else "new"
            if status == "tracked":
                tracked_ids.add(resource)
                unapproved_tracked |= resource not in approved
            else:
                untracked = True
            seen[resource] = input_index
        results.append({"source_id": identity, "content_id": resource,
                        "canonical_url": _canonical_url(resource), "status": status,
                        "registry": str(registry), "matched_lines": matches,
                        "historical_selector_unknown_lines": historical, "report_candidates": leads,
                        "first_input": first_input, "rerun_approved": resource in approved,
                        "coverage": "unknown; URL occurrence is not full mining",
                        "revision": "unknown; specify pinned revision and subresource in capture"})
    unused_approvals = approved - tracked_ids
    if unused_approvals:
        parser.error("rerun approval does not match a tracked input: " + ", ".join(sorted(unused_approvals)))
    print(json.dumps(results, indent=2))
    if input_duplicate:
        return 3
    if args.verify_tracked:
        return 4 if untracked else 0
    return 3 if unapproved_tracked else 0


if __name__ == "__main__":
    raise SystemExit(main())
