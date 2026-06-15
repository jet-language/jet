#!/usr/bin/env bash
# Verify the jetos capstone end to end. Run from the repo root inside the Nix
# dev shell:  nix develop -c bash examples/jetos/demo/verify.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"   # examples/jetos
root="$(cd "$here/../.." && pwd)"                          # repo root
cd "$root"

pass=0; fail=0
ok()   { echo "  ok   $1"; pass=$((pass+1)); }
bad()  { echo "  FAIL $1"; fail=$((fail+1)); }

echo "== unit tests (jet test) =="
jet test examples/jetos/lib/jetpack.jet | tail -1
jet test examples/jetos/lib/ansi.jet    | tail -1

echo "== build the config evaluator and the CLI =="
jet build examples/jetos/config.jet >/dev/null
jet build examples/jetos/jetos.jet  >/dev/null

echo "== config.jet evaluates to a deterministic, merged system tree =="
# Priority resolution: the laptop host's normal-priority cinnamon must beat the
# desktop module's default-priority gnome.
JETOS_HOST=laptop ./build/config > "$here/state/system.json"
if grep -q '"sys.desktop.environment": "cinnamon"' "$here/state/system.json"; then
  ok "host priority overrides module default"
else
  bad "priority resolution"
fi
# Default host falls back to the module's suggestion.
if JETOS_HOST=default ./build/config | grep -q '"sys.desktop.environment": "gnome"'; then
  ok "default host keeps the module suggestion"
else
  bad "default fallback"
fi

echo "== CLI golden output (--no-color) =="
cd "$here"
for cmd in help check list diff generations switch sync; do
  if ../../build/jetos "$cmd" --no-color | diff -u demo/expected/"$cmd".out - >/dev/null; then
    ok "jetos $cmd"
  else
    bad "jetos $cmd  (run: ../../build/jetos $cmd --no-color | diff demo/expected/$cmd.out -)"
  fi
done

echo
echo "$pass passed, $fail failed"
test "$fail" -eq 0
