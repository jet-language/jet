import test from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { scratchRoot } from "./roots.mjs";
import { callSites, census, embeddedPrograms, main, moduleName, publicFunctions, ratchetDecision } from "./core-coverage-census.mjs";

const rows = [
  ...publicFunctions("Core/Text/Text.jet", `trim: fn(x: String) -> String Never! { x }
    split: fn<T>(x: T) -> String Never! { "" }
    unused: fn() {}`),
  ...publicFunctions("Core/Math/Math.jet", "trim: fn() -> Int Never! { 1 }"),
  ...publicFunctions("Core/Crypto/Crypto.jet", `Hasher: struct {
    new: fn() -> Hasher Never! { {} }
    digest: fn(self) -> String Never! { "" }
  }
  new: fn() -> Hasher Never! { Hasher.new() }`),
];
const ids = (source, context) => callSites(source, rows, context).map((call) => call.id);

test("module layout matches Core module keys, including app and nested roots", () => {
  assert.equal(moduleName("Core/Text/Text.jet"), "core.text");
  assert.equal(moduleName("Core/Data/Sketch/Cms.jet"), "core.data.sketch.cms");
  assert.equal(moduleName("Core/Web/Storage/Local.jet"), "core.web.storage.local");
  assert.equal(moduleName("Core/App/App.jet"), "app");
  assert.equal(moduleName("Core/Mod/Mod.jet"), "core.mod");
  assert.equal(moduleName("Core/Net/HttpServer.jet"), "core.net.http_server");
  assert.equal(moduleName("Core/Time/Expiring.jet"), "core.time.expiring");
});

test("every public function, including same-named methods, is inventoried", () => {
  const found = publicFunctions("Core/demo/demo.jet", `// fake: fn() {}
    /* nested /* fake2: fn() {} */ comment */
    priv internal: module { hidden: fn() {} }
    A: struct { new: fn() -> A Never! { {} } }
    B: struct<T> {}
    impl B<T> { new: fn<T>(f: fn(Int) -> Int) -[FS]> B Never! { {} } }
    new: fn() -> A Never! { A.new() }
    generic: fn<T: [Encode, Decode]>(f: fn(T) -> Int) -> B<T> Never! { {} }
    label: "fake3: fn() {}"
  `);
  assert.deepEqual(found.map((row) => row.id), ["core.demo.A.new", "core.demo.B.new", "core.demo.new", "core.demo.generic"]);
  assert.deepEqual(found.map((row) => row.returnType), ["core.demo.A", "core.demo.B", "core.demo.A", "B<T>"]);
});

test("a bare impl inside a type belongs to it, and operator overloads share one row", () => {
  const found = publicFunctions("Core/Demo/Demo.jet", `Kind: enum {
      One
      impl Display { display: fn(self) -> String Never! { "" } }
    }
    Vec: struct { impl Debug { debug: fn(^self) -> String Never! { "" } } }
    impl Vec.Mul { mul: fn(self, rhs: Vec) -> Vec Never! { rhs } }
    impl Vec.Mul(Float) { mul: fn(self, rhs: Float) -> Vec Never! { self } }
  `);
  assert.deepEqual(found.map((row) => row.id), ["core.demo.Kind.display", "core.demo.Vec.debug", "core.demo.Vec.mul"]);
});

test("return types preserve builtin, prelude and compound spellings", () => {
  const found = publicFunctions("Core/demo/demo.jet", `flag: fn() -> Bool Never! { true }
    count: fn() -> Int Never! { 1 }
    text: fn() -> String Never! { "" }
    number: fn() -> Float Never! { Float{1.0} }
    items: fn() -> [String] Never! { [] }
    lookup: fn() -> [String: Int] Never! { {} }
    maybe: fn() -> Int? Never! { None }
    limits: fn() -> Duration Never! { 1ms }
    pair: fn() -> (name: String, n: Int) Never! { ("", 1) }
  `);
  assert.deepEqual(found.map((row) => row.returnType),
    ["Bool", "Int", "String", "Float", "[String]", "[String: Int]", "Int?", "Duration", "(name: String, n: Int)"]);
});

test("calls resolve fully qualified, alias rows, member imports, use blocks and generics", () => {
  assert.deepEqual(ids(`use {
        words: core.text
        pieces: core.text.split
        core.math.[trim]
    }
    run: fn() { words.trim("a"); core.text.trim("b"); pieces<Int>(1); trim() }`),
  ["core.text.trim", "core.text.trim", "core.text.split", "core.math.trim"]);
  assert.deepEqual(ids("use core.text\ntrim: fn() {}\nrun: fn() { text.trim(\"a\") }"), ["core.text.trim"]);
});

test("comments, strings, references and declarations do not count as calls", () => {
  assert.deepEqual(ids(`use t: core.text
    // t.unused()
    /* t.unused() */
    text: "t.unused()"
    reference: t.unused
    unused: fn() {}
    run: fn() { print("{t.trim(\"actual call\")}") }
  `), ["core.text.trim"]);
});

test("scoped aliases do not escape a block or credit a same-named module", () => {
  assert.deepEqual(ids(`use t: core.text
    run: fn() {
      { use t: core.math; t.trim() }
      t.trim("a")
    }`), ["core.math.trim", "core.text.trim"]);
});

test("qualified types, typed receivers, inferred constructors and chained calls", () => {
  assert.deepEqual(ids(`use crypto: core.crypto
    use core.crypto.[Hasher]
    run: fn(h: Hasher) {
      h.digest()
      value: crypto.new()
      value.digest()
      Hasher.new().digest()
      unknown.digest()
    }`), ["core.crypto.Hasher.digest", "core.crypto.new", "core.crypto.Hasher.digest", "core.crypto.Hasher.new", "core.crypto.Hasher.digest"]);
});

test("embedded host programs isolate alias environments and ignore host comments", () => {
  const programs = embeddedPrograms(`// r#"use t: core.text; t.unused()"#
    let first = r#"use t: core.text
      run: fn() { t.trim(\"a\") }"#;
    let second = r##"use t: core.math
      run: fn() { t.trim() }"##;
    let third = "use t: core.text\\nrun: fn() { t.split<Int>(1) }";
    let message = "core.text.unused()";
    let fourth = r"run: fn() { core.text.trim() }";
  `);
  assert.equal(programs.length, 4);
  assert.deepEqual(programs.flatMap((program) => ids(program)), ["core.text.trim", "core.math.trim", "core.text.split", "core.text.trim"]);
});

function fixture(t) {
  const parent = join(scratchRoot(), "CardW-CB-CENSUS");
  mkdirSync(parent, { recursive: true });
  const root = mkdtempSync(join(parent, "census-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const put = (path, source) => {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), source);
  };
  put("Core/demo/demo.jet", `/// Text mentioning unused() is not a doctest.
    /// \`\`\`jet
    /// run: fn() { used() }
    /// \`\`\`
    used: fn() {}
    unused: fn() {}
    example_only: fn() {}
    test_only: fn() {}
  `);
  put("Examples/features/basics/demo.jet", "use d: core.demo\nrun: fn() { d.example_only(); d.example_only() }");
  put("Examples/features/expected/fake.jet", "run: fn() { core.demo.unused() }");
  put("tests/demo.rs", 'let program = r#"run: fn() { core.demo.test_only() }"#;');
  return { root, put };
}

test("census joins doctests, examples and embedded tests once per declaration", (t) => {
  const { root } = fixture(t);
  const report = census(root);
  assert.deepEqual(report.modules, [{ module: "core.demo", public: 4, doctests: 1, examples: 1, tests: 1, uncovered: 1 }]);
  assert.deepEqual(report.total, { public: 4, uncovered: 1 });
  assert.equal(report.functions.find((row) => row.name === "used").doctests.length, 1);
  assert.equal(report.functions.find((row) => row.name === "example_only").examples.length, 1);
  assert.ok(report.functions.find((row) => row.name === "test_only").tests[0].startsWith("tests/demo.rs#program-1:"));
});

test("signatures keep generics, parameters and the result type, without comments or layout", () => {
  const found = publicFunctions("Core/Demo/Demo.jet", `plain: fn() {}
    pick: fn<T: [Encode, Decode]>(
        items: [T], // the source
        at: Int,
    ) -> T? Never! { None }
    Box: struct { put: fn(&self, value: fn(Int) -> Int) -[FS]> Bool Never! { true } }
  `);
  assert.deepEqual(found.map((row) => row.signature),
    ["()", "<T: [Encode, Decode]>(items: [T], at: Int) -> T?", "(&self, value: fn(Int) -> Int) -> Bool"]);
});

const row = (module, name, signature = "()") => ({ module, name, signature, file: "Core/x.jet", line: 1 });
const listed = (modules) => ({ version: 2, modules });

test("ratchet lists names by module and signature, re-files moves, and only removes", () => {
  const first = ratchetDecision([row("core.a", "f"), row("core.a", "g", "(x: Int)")], null);
  assert.equal(first.passes, false);
  assert.deepEqual(first.next, listed({ "core.a": [{ name: "f", signature: "()" }, { name: "g", signature: "(x: Int)" }] }));
  assert.deepEqual(ratchetDecision([row("core.a", "f"), row("core.a", "g", "(x: Int)")], first.next), { passes: true, unlisted: [], removable: 0, next: first.next });
  // g gained a test: --write drops it; a listed name is never re-added.
  const tested = ratchetDecision([row("core.a", "f")], first.next);
  assert.equal(tested.passes, true);
  assert.equal(tested.removable, 1);
  assert.deepEqual(tested.next, listed({ "core.a": [{ name: "f", signature: "()" }] }));
  // A new untested function, or a changed signature, is unlisted.
  const added = ratchetDecision([row("core.a", "f"), row("core.a", "h")], tested.next);
  assert.equal(added.passes, false);
  assert.deepEqual(added.unlisted.map((entry) => entry.name), ["h"]);
  assert.equal(ratchetDecision([row("core.a", "f", "(x: Int)")], tested.next).passes, false);
  // A move keeps its entry, re-filed under the new module.
  const moved = ratchetDecision([row("core.b", "f")], tested.next);
  assert.equal(moved.passes, true);
  assert.deepEqual(moved.next, listed({ "core.b": [{ name: "f", signature: "()" }] }));
  // One entry lists one function: a copy in a second module is new code.
  assert.equal(ratchetDecision([row("core.a", "f"), row("core.b", "f")], tested.next).passes, false);
  for (const previous of [{ version: 1, uncovered: 1 }, { version: 2 }, listed({ "core.a": [{ name: "f" }] }), listed({ "core.a": {} })]) {
    assert.throws(() => ratchetDecision([], previous));
  }
});

test("CLI initializes, checks without rewriting, only removes, and refuses unlisted functions", (t) => {
  const { root, put } = fixture(t);
  const log = console.log;
  const error = console.error;
  console.log = () => {};
  console.error = () => {};
  try {
    const path = join(root, "Tools/agent/core-coverage-ratchet.json");
    mkdirSync(dirname(path), { recursive: true });
    assert.equal(main(["--check"], root), 1);
    assert.equal(main(["--write"], root), 0);
    const baseline = readFileSync(path, "utf8");
    assert.deepEqual(JSON.parse(baseline), listed({ "core.demo": [{ name: "unused", signature: "()" }] }));
    assert.equal(baseline.split("\n")[4].trim(), '{"name":"unused","signature":"()"}');
    assert.equal(main(["--check"], root), 0);
    assert.equal(readFileSync(path, "utf8"), baseline);
    put("tests/extra.jet", "run: fn() { core.demo.unused() }");
    assert.equal(main(["--check"], root), 0);
    assert.equal(readFileSync(path, "utf8"), baseline);
    assert.equal(main(["--write"], root), 0);
    assert.deepEqual(JSON.parse(readFileSync(path, "utf8")), listed({}));
    put("tests/extra.jet", "run: fn() {}");
    assert.equal(main(["--check"], root), 1);
    assert.equal(main(["--write"], root), 1);
    assert.deepEqual(JSON.parse(readFileSync(path, "utf8")), listed({}));
    assert.equal(main(["--check", "--write"], root), 1);
    assert.equal(main(["--typo"], root), 1);
  } finally {
    console.log = log;
    console.error = error;
  }
});
