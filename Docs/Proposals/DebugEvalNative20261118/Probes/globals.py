"""Probe N10: how often do module globals share a `<module>.<name>` when the
module is named the way function symbols name it (the file's module name,
without its package path)? Counts top-level `name := value` and
`name: value` bindings that are not functions, types or modules, in every
.jet file under the given roots. Usage: python3 globals.py <root>..."""
import collections
import os
import re
import sys

binding = re.compile(r"^([a-z_][a-z0-9_]*)\s*(:=|:)\s*(?!fn\b|struct\b|enum\b|trait\b|module\b|type\b|impl\b)\S")
mutable = collections.defaultdict(list)
every = collections.defaultdict(list)
for root in sys.argv[1:]:
    for folder, _, names in os.walk(root):
        for name in names:
            if not name.endswith(".jet"):
                continue
            path = os.path.join(folder, name)
            module = name[:-4]
            for line in open(path, encoding="utf-8", errors="replace"):
                found = binding.match(line)
                if found:
                    key = "{}.{}".format(module, found.group(1))
                    every[key].append(path)
                    if found.group(2) == ":=":
                        mutable[key].append(path)
for label, table in (("mutable", mutable), ("all", every)):
    shared = {key: paths for key, paths in table.items() if len(paths) > 1}
    print("{} globals: {} names, {} shared by more than one file ({} bindings)".format(label, len(table), len(shared), sum(len(p) for p in shared.values())))
    for key, paths in sorted(shared.items(), key=lambda item: -len(item[1]))[:3]:
        print("  {} x{}: {}".format(key, len(paths), ", ".join(sorted(paths)[:3])))
