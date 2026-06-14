#!/usr/bin/env bash
# Prepare the Jet Zed dev extension for "Add Dev Extension".
#
# Zed tries to compile Rust when Cargo.toml sits in the extension root. That fails
# for many setups (nix-only rustc, missing wasm32-wasip2 std). We prebuild
# extension.wasm here instead.
#
# Zed clones tree-sitter grammars into grammars/<name>/ via git. If that folder
# already exists without a matching remote, install fails. Grammar sources live
# in grammar-repo/; grammars/jet/ is removed so Zed can clone cleanly.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

GRAMMAR_REPO="$ROOT/grammar-repo"
GRAMMAR_URI="file://${GRAMMAR_REPO}"
WASM_SRC="$ROOT/wasm-src"

# ── Tree-sitter grammar (sources in grammar-repo/, not grammars/jet/) ───────────
if command -v tree-sitter >/dev/null 2>&1; then
  (cd "$GRAMMAR_REPO" && tree-sitter generate)
  tree-sitter build --wasm -o "$ROOT/grammars/jet.wasm" "$GRAMMAR_REPO"
else
  nix-shell -p tree-sitter --run "
    cd '$GRAMMAR_REPO' && tree-sitter generate
    tree-sitter build --wasm -o '$ROOT/grammars/jet.wasm' '$GRAMMAR_REPO'
  "
fi

# Zed checks out into grammars/jet/ — keep sources in a separate git repo.
if [ ! -d "$GRAMMAR_REPO/.git" ]; then
  git -C "$GRAMMAR_REPO" init -q
fi
git -C "$GRAMMAR_REPO" add -A
if ! git -C "$GRAMMAR_REPO" diff --cached --quiet; then
  git -C "$GRAMMAR_REPO" -c user.email=jet@example.com -c user.name=Jet commit -q -m "zed grammar"
fi
GRAMMAR_REV="$(git -C "$GRAMMAR_REPO" rev-parse HEAD)"

# Remove Zed's clone target so checkout does not hit a stale directory.
rm -rf "$ROOT/grammars/jet"
touch "$ROOT/grammars/jet.wasm"

# ── extension.wasm ────────────────────────────────────────────────────────────
build_wasm() {
  cargo build --release --target wasm32-wasip2 --manifest-path "$WASM_SRC/Cargo.toml"
  cp "$WASM_SRC/target/wasm32-wasip2/release/jet_zed_extension.wasm" "$ROOT/extension.wasm"
}

if command -v rustup >/dev/null 2>&1; then
  rustup target add wasm32-wasip2 >/dev/null 2>&1 || true
  build_wasm
else
  nix-shell -p rustup cargo --run "
    rustup target add wasm32-wasip2 >/dev/null 2>&1 || true
    cargo build --release --target wasm32-wasip2 --manifest-path '$WASM_SRC/Cargo.toml'
    cp '$WASM_SRC/target/wasm32-wasip2/release/jet_zed_extension.wasm' '$ROOT/extension.wasm'
  "
fi

# ── Manifest ──────────────────────────────────────────────────────────────────
sed -e "s|@GRAMMAR_URI@|${GRAMMAR_URI}|g" \
    -e "s|@GRAMMAR_REV@|${GRAMMAR_REV}|g" \
    extension.toml.in > extension.toml

echo ""
echo "Jet Zed extension ready at: $ROOT"
echo "  extension.wasm   ($(du -h extension.wasm | awk '{print $1}'))"
echo "  grammars/jet.wasm ($(du -h grammars/jet.wasm | awk '{print $1}'))"
echo "  grammar rev      ${GRAMMAR_REV}"
echo ""
echo "In Zed:"
echo "  1. Remove any old 'jet' dev extension (zed: extensions)"
echo "  2. nix develop -c cargo build     # jet → target/debug/jet"
echo "  3. Cmd+Shift+P → zed: extensions → Add Dev Extension"
echo "  4. Choose: $ROOT"
echo "  5. zed: reload window"
echo "  6. Open a .jet file — language picker shows 'Jet' (capital J)"
echo ""
echo "If you edit wasm-src/ or grammar-repo/, rerun: editors/zed/install.sh"
echo "Verify LSP: nix develop -c jet lsp doctor"
