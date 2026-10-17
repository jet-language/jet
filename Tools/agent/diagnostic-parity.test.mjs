import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const SCRIPT = join(dirname(fileURLToPath(import.meta.url)), "diagnostic-parity.mjs");

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "diagnostic-parity-"));
  const put = (path, text) => {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), text);
  };
  const row = (code, status) => `    DiagnosticRegistryRow{\n        code: "${code}",\n        status: DiagnosticRegistryStatus.${status},\n    },\n`;
  put("Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet", `ROWS :: [Row]{\n${row("E9001", "Active")}${row("E9002", "Active")}${row("E9003", "Retired")}}\n`);
  put("crates/front/src/lib.rs", 'fn a() { e("E9001"); e("E9002"); e("E9003") }\n#[cfg(test)]\nmod tests { fn t() { e("E9004") } }\n');
  put("Compiler/JetSema/Source/Emit.jet", 'fn b() { sema_error("E9002") } // "E9001" in a comment is no emitter\n');
  put("tests/ui/e9002.stderr", "error[E9002]: example\n");
  const run = (...args) => spawnSync(process.execPath, [SCRIPT, "--root", root, ...args], { encoding: "utf8" });
  return { root, put, run, ratchet: () => JSON.parse(readFileSync(join(root, "Tools/agent/diagnostic-parity-ratchet.json"), "utf8")) };
}

test("census lists each registered code with its fronts and snapshot", () => {
  const { root, run } = fixture();
  try {
    const out = run("--census");
    assert.equal(out.status, 0, out.stderr);
    const rows = out.stdout.trim().split("\n").map((line) => line.split("\t"));
    assert.deepEqual(rows[1], ["E9001", "Active", "crates/front/src/lib.rs:1", "MISSING", "-"]);
    assert.deepEqual(rows[2], ["E9002", "Active", "crates/front/src/lib.rs:1", "Compiler/JetSema/Source/Emit.jet:1", "tests/ui/e9002.stderr"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a code comparison is no emitter, and #[cfg(test)] skips only its own item", () => {
  const { root, put, run } = fixture();
  try {
    put("Compiler/JetSema/Source/Emit.jet", 'fn b() { if d.code == "E9002" -> keep(d) }\n');
    put("crates/front/src/lib.rs", [
      "#[cfg(test)]",
      "mod early { fn t() { e(\"E9002\"); let s = \"{\"; } }",
      "#[cfg(test)] fn helper() -> bool {",
      "    e(\"E9003\")",
      "}",
      "fn emit() { if d.code != \"E9002\" { return } e(\"E9001\") }",
      "",
    ].join("\n"));
    const rows = run("--census").stdout.trim().split("\n").map((line) => line.split("\t"));
    assert.deepEqual(rows[1].slice(0, 4), ["E9001", "Active", "crates/front/src/lib.rs:6", "MISSING"], "code after a test-only item is still scanned");
    assert.deepEqual(rows[2].slice(0, 4), ["E9002", "Active", "-", "MISSING"], "comparisons and test modules are not emitters");
    assert.equal(rows[3][2], "-", "a test-only fn is skipped");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a new Rust-only code fails until the ratchet records it", () => {
  const { root, run, ratchet } = fixture();
  try {
    assert.equal(run("--check").status, 2, "a missing ratchet is an error");
    assert.equal(run("--update").status, 0);
    assert.deepEqual(ratchet().rustOnly, ["E9001"]);
    assert.equal(run("--check").status, 0);
    writeFileSync(join(root, "Tools/agent/diagnostic-parity-ratchet.json"), JSON.stringify({ schema: 1, rustOnly: [] }));
    const grown = run("--check");
    assert.equal(grown.status, 1);
    assert.match(grown.stderr, /E9001: emitted by the Rust front only/);
    assert.equal(run("--update").status, 1, "--update never records a new Rust-only code");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("losing the Rust emitter without a Jet one is a silent drop; a port lowers the ratchet", () => {
  const { root, put, run, ratchet } = fixture();
  try {
    assert.equal(run("--update").status, 0);
    put("crates/front/src/lib.rs", 'fn a() { e("E9002") }\n');
    const dropped = run("--check");
    assert.equal(dropped.status, 1);
    assert.match(dropped.stderr, /E9001: silent drop/);
    put("Compiler/JetSema/Source/Port.jet", 'fn c() { sema_error("E9001") }\n');
    const resolved = run("--check");
    assert.equal(resolved.status, 1);
    assert.match(resolved.stderr, /now resolved \(E9001\)/);
    assert.equal(run("--update").status, 0);
    assert.deepEqual(ratchet().rustOnly, []);
    assert.equal(run("--check").status, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
