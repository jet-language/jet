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
npx --yes @vscode/vsce package -o "$VSIX"

# Sanity check: a vsix without the runtime dependency crashes on activation.
if ! npx --yes @vscode/vsce ls | grep -q "node_modules/vscode-languageclient"; then
  echo "error: packaged vsix is missing node_modules/vscode-languageclient" >&2
  exit 1
fi

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

"$EDITOR" --install-extension "$VSIX"
echo "Installed jet-lang.jet $VERSION — reload the editor window."
