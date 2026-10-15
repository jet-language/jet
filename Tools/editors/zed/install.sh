#!/usr/bin/env bash
# Build and install the Jet Zed dev extension.
#
# The extension directory is this one (Tools/editors/zed). Zed loads a dev
# extension through a symlink at <zed data>/extensions/installed/jet-lang:
#   extension.toml        manifest (rendered here from extension.toml.in)
#   extension.wasm        prebuilt language-server glue (wasm-src/, wasm32-wasip2)
#   grammars/jet.wasm     the tree-sitter parser
#   languages/jet/*.scm   highlights, brackets, indents, outline, overrides
#
# grammars/jet.wasm is compiled from the committed, generated parser in
# Tools/editors/tree-sitter/src with the same command Zed's extension builder
# uses (wasi-sdk clang, -fPIC -shared -Os), so "Add Dev Extension"/"Rebuild"
# in Zed produce the same parser. Zed's builder clones the grammar from
# grammar-repo/ (a local git mirror of Tools/editors/tree-sitter) at the
# commit written into extension.toml; grammars/jet/ is removed so it can.
#
# Zed compiles Rust only when Cargo.toml sits in the extension root, which
# fails for nix-only toolchains; extension.wasm is prebuilt here instead and
# Zed loads it because the file exists.
#
# Finally the dev-extension symlink is (re)pointed at this directory, which
# Zed's extension watcher picks up live. A non-symlink install of jet-lang
# (a published extension) is never replaced.
#
# Usage:
#   Tools/editors/zed/install.sh            # rebuild only stale wasm files
#   FORCE=1 Tools/editors/zed/install.sh    # rebuild both wasm files
#   NO_LINK=1 Tools/editors/zed/install.sh  # build only; leave Zed untouched
#   ZED_EXTENSIONS_DIR=... overrides <zed data>/extensions
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

FORCE="${FORCE:-0}"
GRAMMAR_REPO="$ROOT/grammar-repo"
GRAMMAR_URI="file://${GRAMMAR_REPO}"
WASM_SRC="$ROOT/wasm-src"
TREE_SITTER_SRC="$(cd "$ROOT/../tree-sitter" && pwd)"
ZED_EXTENSIONS_DIR="${ZED_EXTENSIONS_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/zed/extensions}"

# stale TARGET SOURCE... — true when TARGET is missing, empty or older than a source.
stale() {
  local target=$1; shift
  [ "$FORCE" = "1" ] && return 0
  [ -s "$target" ] || return 0
  local source
  for source in "$@"; do
    [ "$source" -nt "$target" ] && return 0
  done
  return 1
}

# ── Grammar mirror (Zed clones it via a file:// URI) ─────────────────────────
echo "Syncing grammar sources from Tools/editors/tree-sitter/ ..."
mkdir -p "$GRAMMAR_REPO"
rm -rf "$GRAMMAR_REPO/src"
cp "$TREE_SITTER_SRC/grammar.js" "$TREE_SITTER_SRC/tree-sitter.json" "$GRAMMAR_REPO/"
cp -r "$TREE_SITTER_SRC/src" "$GRAMMAR_REPO/src"
if [ ! -d "$GRAMMAR_REPO/.git" ]; then
  git -C "$GRAMMAR_REPO" init -q
fi
git -C "$GRAMMAR_REPO" add -A
if ! git -C "$GRAMMAR_REPO" diff --cached --quiet; then
  git -C "$GRAMMAR_REPO" -c user.email=jet@example.com -c user.name=Jet commit -q -m "zed grammar"
fi
GRAMMAR_REV="$(git -C "$GRAMMAR_REPO" rev-parse HEAD)"

# Remove Zed's clone target so its checkout does not hit a stale directory.
rm -rf "$ROOT/grammars/jet"

# ── Tree-sitter grammar wasm ─────────────────────────────────────────────────
mkdir -p "$ROOT/grammars"
if stale "$ROOT/grammars/jet.wasm" "$TREE_SITTER_SRC/src/parser.c"; then
  echo "Building grammars/jet.wasm ..."
  WASI_CLANG="$ZED_EXTENSIONS_DIR/build/wasi-sdk/bin/clang"
  if [ -x "$WASI_CLANG" ]; then
    "$WASI_CLANG" -fPIC -shared -Os -Wl,--export=tree_sitter_jet \
      -o "$ROOT/grammars/jet.wasm" -I "$TREE_SITTER_SRC/src" "$TREE_SITTER_SRC/src/parser.c" 2> >(grep -v 'creating shared libraries' >&2)
  elif command -v tree-sitter >/dev/null 2>&1; then
    tree-sitter build --wasm -o "$ROOT/grammars/jet.wasm" "$TREE_SITTER_SRC"
  else
    nix-shell -p tree-sitter emscripten --run "tree-sitter build --wasm -o '$ROOT/grammars/jet.wasm' '$TREE_SITTER_SRC'"
  fi
else
  echo "  grammars/jet.wasm is current (FORCE=1 to rebuild)"
fi

# ── extension.wasm ───────────────────────────────────────────────────────────
if stale "$ROOT/extension.wasm" "$WASM_SRC/src/lib.rs" "$WASM_SRC/Cargo.toml"; then
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
  echo "  extension.wasm is current (FORCE=1 to rebuild)"
fi

# ── Manifest ─────────────────────────────────────────────────────────────────
sed -e "s|@GRAMMAR_URI@|${GRAMMAR_URI}|g" \
    -e "s|@GRAMMAR_REV@|${GRAMMAR_REV}|g" \
    extension.toml.in > extension.toml

# ── Dev-extension link ───────────────────────────────────────────────────────
LINK="$ZED_EXTENSIONS_DIR/installed/jet-lang"
link_state="not linked (NO_LINK=1)"
if [ "${NO_LINK:-0}" != "1" ]; then
  if [ -e "$LINK" ] && [ ! -L "$LINK" ]; then
    link_state="not linked: $LINK is a published (non-dev) install; uninstall it in Zed first"
  elif [ ! -d "$ZED_EXTENSIONS_DIR/installed" ]; then
    link_state="not linked: no Zed extensions directory at $ZED_EXTENSIONS_DIR"
  else
    ln -sfn "$ROOT" "$LINK"
    link_state="$LINK -> $ROOT"
  fi
fi

echo ""
echo "Jet Zed extension ready at: $ROOT"
echo "  extension.wasm    ($(du -h extension.wasm | awk '{print $1}'))"
echo "  grammars/jet.wasm ($(du -h grammars/jet.wasm | awk '{print $1}'))"
echo "  grammar rev       ${GRAMMAR_REV}"
echo "  dev extension     ${link_state}"
echo ""
echo "Zed reloads the linked extension on its own; if a .jet buffer stays plain,"
echo "run 'zed: reload extensions'. Open a .jet file — the language picker shows 'Jet'."
echo "The language server is \`jet self lsp\` from PATH (Tools/agent/jet-env cargo build)."
