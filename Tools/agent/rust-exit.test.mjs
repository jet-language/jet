import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseCoreSource } from "./check-core-surface-ledger.mjs";
import { CORE, HEADER, RATCHET, REGISTRY, dispatchHash, parseRegistry, physicalLines, ratchetErrors, registryErrors, rowIdentity, rustArea, rustCounts } from "./rust-exit.mjs";

const tool = fileURLToPath(new URL("./rust-exit.mjs", import.meta.url));
const bootstraps = "bootstrap Syntax depends_on []\nbootstrap Effects depends_on [Syntax]\nbootstrap Core depends_on [Effects]\nbootstrap Derives depends_on [Core]\n";
const dispatch = 'dispatcher_row plain core.demo raw | CoreCallRecord::new("core.demo", "raw", "jet_raw", false, &[], )';
const core = bootstraps + "module core.demo exports {raw} types {} depends_on []\n" + dispatch + "\n";
const row = parseCoreSource(core).dispatcherRows[0];
const entry = (r) => [rowIdentity(r), "jet_raw", "crates/host.rs:1", "Replace with Jet syscall; remove Rust provider", "test", "leaf", dispatchHash(r)].join("\t");

function fixture(t) {
  const parent = join(homedir(), ".cache/jet-dev/scratch/RustLedger1");
  mkdirSync(parent, { recursive: true });
  const root = mkdtempSync(join(parent, "fixture-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const put = (file, text) => { mkdirSync(dirname(join(root, file)), { recursive: true }); writeFileSync(join(root, file), text); };
  execFileSync("git", ["init", "--quiet", root]);
  put(CORE, core);
  put(REGISTRY, HEADER + "\n" + entry(row) + "\n");
  put("Source/main.rs", "fn main() {}\n");
  execFileSync("git", ["add", "--", "Source/main.rs"], { cwd: root });
  const ceilings = Object.fromEntries([..."ABCDEFGH"].map((area) => [area, area === "E" ? 1 : 0]));
  put(RATCHET, JSON.stringify({ ceilings }) + "\n");
  return { root, put, run: (...args) => spawnSync(process.execPath, [tool, "--root", root, ...args], { encoding: "utf8" }) };
}

test("LOC semantics include comments, blanks, missing final newline, and empty files", () => {
  assert.equal(physicalLines(""), 0);
  assert.equal(physicalLines("\n"), 1);
  assert.equal(physicalLines("// comment\n\nfn main() {}\n"), 3);
  assert.equal(physicalLines("fn main() {}"), 1);
});

test("A-H partition files; build scripts override crate classification", () => {
  const paths = { A: "crates/jet-codegen/src/Codegen/MIRRust.rs", B: "Compiler/Bootstrap/Tests.rs", C: "crates/jet-rt/src/lib.rs", D: "Core/raw.rs", E: "crates/jet-devserver/src/lib.rs", F: "crates/jetpack/src/lib.rs", G: "tests/golden.rs", H: "crates/jetpack/build.rs" };
  for (const [area, file] of Object.entries(paths)) assert.equal(rustArea(file), area);
  assert.equal(rustArea("crates/jet-foundation/build/MirImageCodec.rs"), "H");
  assert.equal(rustArea("crates/jet-codegen/src/Prelude/CoreLib/Top/Math.rs"), "C");
});

test("receiver identities survive inserted lines and retain type/method distinctions", () => {
  const receiver = { kind: "receiver", member: "METHOD_RECEIPT_ATTACH", expression: 'CoreCallRecord::receiver_with_symbol( &[INTERNAL_RECEIPT_HANDLE], METHOD_RECEIPT_ATTACH, "jet_receipt_attach", true, &[], )' };
  assert.equal(rowIdentity(receiver), "receiver[INTERNAL_RECEIPT_HANDLE].METHOD_RECEIPT_ATTACH");
  assert.equal(dispatchHash({ ...row, sourceLine: 900 }), dispatchHash(row));
  assert.equal(dispatchHash({ ...row, expression: row.expression.replaceAll(",", ", ") }), dispatchHash(row));
  assert.notEqual(dispatchHash({ ...row, expression: row.expression.replace("jet_raw", "jet_new_raw") }), dispatchHash(row));
  assert.notEqual(dispatchHash({ ...row, expression: row.expression.replace("jet_raw", "jet_ raw") }), dispatchHash(row));
});

test("registry rejects blanks/duplicate rows; unlanded journal entries grant no live exemption", () => {
  assert.throws(() => parseRegistry(HEADER + "\n" + entry(row) + "\n" + entry(row)), /duplicate/);
  assert.throws(() => parseRegistry(HEADER + "\n" + entry(row).replace("Replace with Jet syscall; remove Rust provider", "")), /malformed/);
  const entries = parseRegistry(HEADER + "\n" + entry(row));
  assert.deepEqual(registryErrors([row], entries), []);
  entries.get(rowIdentity(row)).hash = "-";
  assert.match(registryErrors([row], entries)[0], /changed without registry review/);
});

test("CLI passes and refuses a planted unregistered leaf even behind public-by-default Jet source", (t) => {
  const f = fixture(t);
  assert.equal(f.run("--check").status, 0);
  f.put(REGISTRY, HEADER + "\n");
  f.put("Core/demo/demo.jet", "fn raw() { core.demo.raw() }\n");
  f.put(CORE, core + "source_module core.demo path Core/demo/demo.jet alias Demo owns {raw}\n");
  const result = f.run("--check");
  assert.equal(result.status, 1);
  assert.match(result.stderr, /unregistered Rust dispatcher core.demo.raw/);
});

test("CLI refuses planted +1 tracked line and +1 new nonignored Rust source; update cannot raise", (t) => {
  const f = fixture(t);
  f.put("Source/main.rs", "fn main() {}\n// planted line\n");
  const before = readFileSync(join(f.root, RATCHET), "utf8");
  const growth = f.run("--update");
  assert.equal(growth.status, 1);
  assert.match(growth.stderr, /Rust area E grew: 2 > 1 \(\+1\)/);
  assert.equal(readFileSync(join(f.root, RATCHET), "utf8"), before);
  f.put("Source/main.rs", "fn main() {}\n");
  f.put("Source/new.rs", "// new source\n");
  assert.equal(f.run("--check").status, 1);
});

test("CLI update lowers to zero and a zero ceiling stays closed", (t) => {
  const f = fixture(t);
  f.put("Source/main.rs", "");
  assert.equal(f.run("--update").status, 0);
  assert.equal(JSON.parse(readFileSync(join(f.root, RATCHET), "utf8")).ceilings.E, 0);
  f.put("Source/main.rs", "// reintroduced\n");
  assert.equal(f.run("--check").status, 1);
});

test("symlinked Rust source is counted once; ignored emitted Rust is not inventory", (t) => {
  const f = fixture(t);
  f.put("crates/jet-foundation/src/Devtools.rs", "// one physical source\n");
  mkdirSync(join(f.root, "crates/jet-codegen/src/Prelude"), { recursive: true });
  symlinkSync("../../../jet-foundation/src/Devtools.rs", join(f.root, "crates/jet-codegen/src/Prelude/Devtools.rs"));
  f.put(".gitignore", "emitted/\n");
  f.put("emitted/program.rs", "// not source\n");
  const counts = rustCounts(f.root);
  assert.equal(counts.B, 1);
  assert.equal(counts.C, 0);
  assert.equal(counts.H, 0);
});

test("every area is checked, including formerly-zero Core and build/tool areas", () => {
  const counts = Object.fromEntries([..."ABCDEFGH"].map((area) => [area, 1]));
  const zeros = Object.fromEntries([..."ABCDEFGH"].map((area) => [area, 0]));
  assert.equal(ratchetErrors(counts, zeros).length, 8);
});

test("committed snapshot reconciles exactly with the RUST-EXIT inventory total", () => {
  const root = resolve(dirname(tool), "../..");
  const table = JSON.parse(readFileSync(join(root, RATCHET), "utf8"));
  assert.equal(Object.values(table.snapshot_counts).reduce((a, b) => a + b, 0), 1882413);
  assert.equal(table.inventory_section_1.C1_CoreLib, 134064);
  assert.equal(table.inventory_section_1.C2_Core, 80533);
  assert.equal(table.inventory_section_1.C3_Prelude_top, 32283);
  assert.equal(table.inventory_section_1.F1_Jetpack, 289594);
});
