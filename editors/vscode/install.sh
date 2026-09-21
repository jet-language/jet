#!/usr/bin/env bash
# Install the Jet editor extension (id: jet-lang.jet) from this checkout.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

npm install --silent
VERSION="$(node -p "require('./package.json').version")"
VSIX="$ROOT/jet-lang.jet-$VERSION.vsix"

# No --no-dependencies: the client needs vscode-languageclient bundled in
# the vsix (there is no build/bundle step).
npx --yes @vscode/vsce package --allow-missing-repository -o "$VSIX"

# Sanity check: a vsix without the runtime dependency crashes on activation.
if ! npx --yes @vscode/vsce ls | grep -q "node_modules/vscode-languageclient"; then
  echo "error: packaged vsix is missing node_modules/vscode-languageclient" >&2
  exit 1
fi

# Cursor refuses --install-extension while Jet is loaded. Nix-bundled Jet is
# also a read-only store path when mutableExtensionsDir = false. Unpack a
# versioned user copy into any writable extensions dir.
UNPACKED="$(python3 - "$VSIX" "$VERSION" <<'PY'
import json, os, shutil, sys, zipfile
from pathlib import Path

vsix = Path(sys.argv[1])
version = sys.argv[2]
ext_id = "jet-lang.jet"
rel = f"{ext_id}-{version}"
homes = [
    Path.home() / ".cursor" / "extensions",
    Path.home() / ".vscode-oss" / "extensions",
]

unpacked = 0
with zipfile.ZipFile(vsix) as z:
    names = [n for n in z.namelist() if n.startswith("extension/")]
    for dest_root in homes:
        try:
            dest_root.mkdir(parents=True, exist_ok=True)
        except OSError as err:
            print(f"skip {dest_root}: {err}", file=sys.stderr)
            continue
        dest = dest_root / rel
        if not os.access(dest_root, os.W_OK):
            print(f"skip {dest_root}: not writable (Nix-managed)", file=sys.stderr)
            continue
        if dest.exists():
            shutil.rmtree(dest)
        dest.mkdir(parents=True)
        for name in names:
            inner = name[len("extension/") :]
            if not inner:
                continue
            out = dest / inner
            if name.endswith("/"):
                out.mkdir(parents=True, exist_ok=True)
                continue
            out.parent.mkdir(parents=True, exist_ok=True)
            with z.open(name) as src, open(out, "wb") as dst:
                dst.write(src.read())
        print(f"unpacked {rel} -> {dest}", file=sys.stderr)
        unpacked += 1
        ext_json = dest_root / "extensions.json"
        if not ext_json.exists() or not os.access(ext_json, os.W_OK):
            continue
        try:
            data = json.loads(ext_json.read_text())
        except json.JSONDecodeError:
            continue
        if not isinstance(data, list):
            continue
        dest_s = str(dest)
        for entry in data:
            if not isinstance(entry, dict):
                continue
            ident = entry.get("identifier") or {}
            if ident.get("id") != ext_id:
                continue
            loc = entry.setdefault("location", {})
            loc["fsPath"] = dest_s
            loc["path"] = dest_s
            loc.setdefault("$mid", 1)
            loc.setdefault("scheme", "file")
            entry["version"] = version
            entry["relativeLocation"] = rel
        ext_json.write_text(json.dumps(data, separators=(",", ":")))
print(unpacked)
PY
)"

EDITOR=""
for cmd in cursor codium code; do
  if command -v "$cmd" >/dev/null 2>&1; then
    EDITOR="$cmd"
    break
  fi
done
if [ -z "$EDITOR" ]; then
  echo "error: need cursor, codium, or code on PATH" >&2
  exit 1
fi

if "$EDITOR" --install-extension "$VSIX" --force; then
  echo "Installed jet-lang.jet $VERSION — reload the editor window (Developer: Reload Window)."
  exit 0
fi

if [ "${UNPACKED:-0}" != "0" ]; then
  echo "CLI install blocked (editor running). User copy unpacked."
  echo "Reload Window to pick up grammar + LSP client."
  exit 0
fi

echo "error: $EDITOR refused the install and the extensions dir is not writable." >&2
echo "This happens when Cursor is running, or when Jet is a Nix-bundled store" >&2
echo "extension (programs.cursor.mutableExtensionsDir = false)." >&2
echo "Quit the editor fully, then rerun $0. Nix users: set mutableExtensionsDir" >&2
echo "to true, rebuild, then rerun $0 — or rebuild after the jetlang input" >&2
echo "contains this checkout's editors/vscode grammar." >&2
exit 1
