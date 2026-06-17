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
#
# Usage:
#   editors/zed/install.sh            # skip wasm rebuilds if files exist
#   FORCE=1 editors/zed/install.sh    # rebuild all wasm files from scratch
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

FORCE="${FORCE:-0}"
GRAMMAR_REPO="$ROOT/grammar-repo"
GRAMMAR_URI="file://${GRAMMAR_REPO}"
WASM_SRC="$ROOT/wasm-src"
TREE_SITTER_SRC="$ROOT/../tree-sitter"

# ── Sync grammar sources into grammar-repo/ ───────────────────────────────────
# Authoritative sources live in editors/tree-sitter/; grammar-repo/ is a
# standalone git repo that Zed clones via a file:// URI.
echo "Syncing grammar sources from editors/tree-sitter/ ..."
mkdir -p "$GRAMMAR_REPO/src"
cp "$TREE_SITTER_SRC/grammar.js" "$GRAMMAR_REPO/"
cp "$TREE_SITTER_SRC/tree-sitter.json" "$GRAMMAR_REPO/"
if [ -d "$TREE_SITTER_SRC/src" ]; then
  cp -r "$TREE_SITTER_SRC/src/." "$GRAMMAR_REPO/src/"
fi

# ── Tree-sitter grammar wasm ──────────────────────────────────────────────────
if [ ! -s "$ROOT/grammars/jet.wasm" ] || [ "$FORCE" = "1" ]; then
  echo "Building grammars/jet.wasm ..."
  if command -v tree-sitter >/dev/null 2>&1; then
    (cd "$GRAMMAR_REPO" && tree-sitter generate)
    tree-sitter build --wasm -o "$ROOT/grammars/jet.wasm" "$GRAMMAR_REPO"
  else
    nix-shell -p tree-sitter emscripten --run "
      cd '$GRAMMAR_REPO' && tree-sitter generate
      tree-sitter build --wasm -o '$ROOT/grammars/jet.wasm' '$GRAMMAR_REPO'
    "
  fi
else
  echo "  grammars/jet.wasm exists — skipping (FORCE=1 to rebuild)"
fi

# ── Grammar git repo (Zed clones from file:// URI) ───────────────────────────
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

# ── extension.wasm ────────────────────────────────────────────────────────────
if [ ! -s "$ROOT/extension.wasm" ] || [ "$FORCE" = "1" ]; then
  echo "Building extension.wasm ..."
  build_extension_wasm() {
    if command -v rustup >/dev/null 2>&1; then
      rustup target add wasm32-wasip2 >/dev/null 2>&1 || true
      cargo build --release --target wasm32-wasip2 --manifest-path "$WASM_SRC/Cargo.toml"
    else
      nix-shell -p rustup --run "
        rustup default stable >/dev/null 2>&1 || true
        rustup target add wasm32-wasip2 >/dev/null 2>&1 || true
        cargo build --release --target wasm32-wasip2 --manifest-path '$WASM_SRC/Cargo.toml'
      "
    fi
    cp "$WASM_SRC/target/wasm32-wasip2/release/jet_zed_extension.wasm" "$ROOT/extension.wasm"
  }

  if (set +e; build_extension_wasm); then
    echo "  extension.wasm built"
  elif [ -s "$ROOT/extension.wasm" ]; then
    echo "  warning: extension.wasm build failed; keeping pre-built $(du -h "$ROOT/extension.wasm" | awk '{print $1}')"
  else
    echo "  error: extension.wasm build failed and no pre-built file exists" >&2
    exit 1
  fi
else
  echo "  extension.wasm exists — skipping (FORCE=1 to rebuild)"
fi

# ── Manifest ──────────────────────────────────────────────────────────────────
sed -e "s|@GRAMMAR_URI@|${GRAMMAR_URI}|g" \
    -e "s|@GRAMMAR_REV@|${GRAMMAR_REV}|g" \
    extension.toml.in > extension.toml

echo ""
echo "Jet Zed extension ready at: $ROOT"
echo "  extension.wasm    ($(du -h extension.wasm | awk '{print $1}'))"
echo "  grammars/jet.wasm ($(du -h grammars/jet.wasm | awk '{print $1}'))"
echo "  grammar rev       ${GRAMMAR_REV}"
echo ""
echo "In Zed:"
echo "  1. Remove any old 'jet' dev extension (zed: extensions)"
echo "  2. nix develop -c cargo build     # jet → target/debug/jet"
echo "  3. Cmd+Shift+P → zed: extensions → Add Dev Extension"
echo "  4. Choose: $ROOT"
echo "  5. zed: reload window"
echo "  6. Open a .jet file — language picker shows 'Jet' (capital J)"
echo ""
echo "If you edit wasm-src/ or grammar-repo/, rerun: FORCE=1 editors/zed/install.sh"
echo "Verify LSP: nix develop -c jet lsp doctor"
