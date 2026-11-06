// compilerSliceDrift on a fixture compiler: only code names a dependency, and
// an enum's variants are names the slice declares.
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { compilerSliceDrift } from "./compiler-slice.mjs";

function fixture(files) {
  const root = mkdtempSync(resolve(tmpdir(), "jetpack-compiler-slice-"));
  for (const [name, text] of Object.entries(files)) {
    mkdirSync(dirname(resolve(root, name)), { recursive: true });
    writeFileSync(resolve(root, name), text);
  }
  return root;
}

const compiler = {
  "Compiler/JetCli/package.jet": 'name: "jet-cli"\ndeps: {\n    front: ../JetFront,\n    sema: ../JetSema,\n    wasm: ../JetWasm\n}\n',
  "Compiler/JetFront/package.jet": 'name: "jet-front"\n',
  "Compiler/JetSema/package.jet": 'name: "jet-sema"\n',
  "Compiler/JetWasm/package.jet": 'name: "jet-wasm"\n',
  // Prelude source held in a raw string: its `struct Err` declares nothing.
  "Compiler/JetSema/Source/Derives.jet": "PRELUDE_SOURCE :: comptime { ```// The default error.\nstruct Err {\n    message: String\n}\n``` }\n",
  "Compiler/JetWasm/Source/Module.jet": "struct Export {\n    name: String\n}\nfn wasm_size() -> Int {\n    0\n}\n",
  "Jetpack/Bootstrap/sources.list": "# == Compiler front end ==\nCompiler/JetFront/Source/Items.jet\n",
  "Jetpack/Bootstrap/tests.list": "",
};

test("a raw string's text and an enum's own variants name no dependency", () => {
  const root = fixture({
    ...compiler,
    "Compiler/JetFront/Source/Items.jet": [
      "enum Marker {",
      "    Plain",
      "    Export, Named(String)",
      "}",
      "fn front_marker() -> Marker { Marker.Export }",
      "fn front_error(message: String) -> Err { Err{message: message} }",
      "NOTE :: `calls wasm_size() with \"quotes\" and // slashes`",
      "",
    ].join("\n"),
  });
  try {
    assert.deepEqual(compilerSliceDrift(root), []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a call past the slice is still drift", () => {
  const root = fixture({
    ...compiler,
    "Compiler/JetFront/Source/Items.jet": "fn front_size() -> Int {\n    wasm_size()\n}\n",
  });
  try {
    assert.deepEqual(compilerSliceDrift(root), [
      "Compiler/JetFront/Source/Items.jet uses wasm_size, which only Compiler/JetWasm/Source/Module.jet declares, outside the compiler slice",
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
