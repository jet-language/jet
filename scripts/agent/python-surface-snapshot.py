#!/usr/bin/env python3
"""Emit the closed Python comparison surface used by the Core surface ledger.

The ledger may not assert that a Python member exists. It reads this snapshot,
which is produced by introspecting a real interpreter, so every "Python has
this" and "Python lacks this" claim is checkable.

Regenerate with:

    scripts/agent/jet-env python3 scripts/agent/python-surface-snapshot.py \
        > docs/reference/python-surface.json

Scope rule (recorded in the snapshot, never applied silently): the comparison
set is callables and types. Module-level integer/string constants such as
socket.AF_INET are excluded because they are configuration values, not
operations. The excluded names stay counted so the exclusion cannot hide a gap.
"""

import importlib
import json
import sys

BUILTIN_TYPES = {
    "bool": bool,
    "bytes": bytes,
    "dict": dict,
    "float": float,
    "int": int,
    "list": list,
    "range": range,
    "set": set,
    "str": str,
    "tuple": tuple,
}

STDLIB_MODULES = [
    "asyncio", "base64", "binascii", "collections", "csv", "datetime",
    "functools", "heapq", "http", "io", "itertools", "json", "logging",
    "math", "os", "pathlib", "random", "re", "secrets", "socket", "sqlite3",
    "ssl", "statistics", "struct", "subprocess", "tarfile", "tempfile",
    "time", "tomllib", "unicodedata", "unittest", "urllib.parse", "uuid",
    "zipfile",
]


def public(names):
    return sorted(n for n in names if not n.startswith("_"))


def main():
    snapshot = {
        "schemaVersion": 1,
        "title": "Python comparison surface",
        "generator": "scripts/agent/python-surface-snapshot.py",
        "pythonVersion": ".".join(str(p) for p in sys.version_info[:3]),
        "scopeRule": (
            "Comparison points are callables and types. Module-level constant "
            "values are excluded as configuration, not operations. Excluded "
            "names remain counted so the exclusion is visible."
        ),
        "officialIndex": "https://docs.python.org/3/library/index.html",
        "builtinIndex": "https://docs.python.org/3/library/functions.html",
        "builtinTypes": {},
        "stdlibModules": {},
    }

    for name, obj in BUILTIN_TYPES.items():
        members = public(dir(obj))
        snapshot["builtinTypes"][name] = {
            "members": members,
            "memberCount": len(members),
        }

    for name in STDLIB_MODULES:
        module = importlib.import_module(name)
        exported = getattr(module, "__all__", None)
        names = public(exported if exported else dir(module))
        operations = []
        constants = []
        for member in names:
            value = getattr(module, member, None)
            (operations if callable(value) else constants).append(member)
        snapshot["stdlibModules"][name] = {
            "operations": operations,
            "operationCount": len(operations),
            "excludedConstantCount": len(constants),
            "excludedConstants": constants,
        }

    totals = {
        "builtinTypeMembers": sum(
            entry["memberCount"] for entry in snapshot["builtinTypes"].values()
        ),
        "moduleOperations": sum(
            entry["operationCount"] for entry in snapshot["stdlibModules"].values()
        ),
        "excludedConstants": sum(
            entry["excludedConstantCount"] for entry in snapshot["stdlibModules"].values()
        ),
    }
    totals["comparisonPoints"] = totals["builtinTypeMembers"] + totals["moduleOperations"]
    snapshot["totals"] = totals

    json.dump(snapshot, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
