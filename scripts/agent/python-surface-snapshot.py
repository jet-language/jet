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
import inspect
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
    "argparse", "asyncio", "base64", "binascii", "builtins", "collections",
    "copy", "csv", "datetime", "decimal", "email", "fractions", "functools",
    "glob", "gzip", "hashlib", "heapq", "hmac", "http", "http.server",
    "inspect", "io", "itertools", "json", "logging", "math", "mimetypes",
    "os", "pathlib", "queue", "random", "re", "secrets", "shutil", "socket",
    "sqlite3", "ssl", "statistics", "string", "struct", "subprocess", "sys",
    "tarfile", "tempfile", "textwrap", "threading", "time", "tomllib",
    "unicodedata", "unittest", "urllib.parse", "uuid",
    "xml.etree.ElementTree", "zipfile", "zlib",
]

# builtins holds every exception class and the primitive types, which already
# have containers of their own. Flattening its classes put property.setter and
# BaseException.args into unrelated containers, so it is recorded by module
# level name only.
NO_TYPE_FLATTEN = {"builtins"}

# These builtin types are compared as containers of their own, so counting them
# again as builtins operations would score the same gap twice. Every other
# builtins class stays an operation.
PRIMITIVE_CONTAINERS = {
    "bool", "bytes", "dict", "float", "int", "list", "range", "set", "str",
    "tuple",
}


def public(names):
    return sorted(n for n in names if not n.startswith("_"))


def main():
    snapshot = {
        "schemaVersion": 1,
        "title": "Python comparison surface",
        "generator": "scripts/agent/python-surface-snapshot.py",
        "pythonVersion": ".".join(str(p) for p in sys.version_info[:3]),
        "scopeRule": (
            "Comparison points are the operations a user calls. An exception "
            "class is a failure signal rather than an operation, and a "
            "module-level constant is configuration, so both are excluded and "
            "both stay counted. Class members such as socket.socket.recv are "
            "recorded because that is where most of a module's real surface "
            "lives; builtins is excluded from that flattening because its "
            "classes are the primitive types, which have containers already. "
            "Only a class's methods count; its data attributes are fields."
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
        exceptions = []
        type_names = []
        types_members = {}
        for member in names:
            value = getattr(module, member, None)
            is_class = inspect.isclass(value)
            if is_class and issubclass(value, BaseException):
                exceptions.append(member)
                continue
            if not callable(value):
                constants.append(member)
                continue
            # A builtins class whose own container exists is compared there, so
            # counting list, dict and str again would score one gap twice. The
            # rest are ordinary calls in real code -- enumerate, zip, map,
            # filter, range, reversed, frozenset, complex, memoryview -- and
            # excluding them deleted real gaps.
            if is_class and name in NO_TYPE_FLATTEN:
                if member in PRIMITIVE_CONTAINERS:
                    type_names.append(member)
                    continue
            operations.append(member)
            if is_class and name not in NO_TYPE_FLATTEN:
                # Only a class's methods are operations. Its data attributes are
                # fields: flattening them put os.terminal_size.columns and
                # os.times_result.children_system into core.os as missing calls.
                # Only what the class itself introduces. http.HTTPMethod is a
                # StrEnum, so its inherited str members put istitle, isupper
                # and capitalize into core.http as missing operations.
                inherited = set()
                for base in value.__mro__[1:]:
                    inherited.update(dir(base))
                types_members[member] = public(
                    m
                    for m in dir(value)
                    if callable(getattr(value, m, None)) and m not in inherited
                )
        snapshot["stdlibModules"][name] = {
            "operations": operations,
            "operationCount": len(operations),
            "types": types_members,
            "typeMemberCount": sum(len(v) for v in types_members.values()),
            "excludedConstantCount": len(constants),
            "excludedConstants": constants,
            "excludedExceptionCount": len(exceptions),
            "excludedExceptions": exceptions,
            "excludedTypeCount": len(type_names),
            "excludedTypes": type_names,
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
        "excludedExceptions": sum(
            entry["excludedExceptionCount"] for entry in snapshot["stdlibModules"].values()
        ),
        "excludedTypes": sum(
            entry["excludedTypeCount"] for entry in snapshot["stdlibModules"].values()
        ),
        "moduleTypeMembers": sum(
            entry["typeMemberCount"] for entry in snapshot["stdlibModules"].values()
        ),
    }
    totals["comparisonPoints"] = totals["builtinTypeMembers"] + totals["moduleOperations"]
    snapshot["totals"] = totals

    json.dump(snapshot, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
