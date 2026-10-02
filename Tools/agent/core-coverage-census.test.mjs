import test from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { callSites, census, embeddedPrograms, main, moduleName, publicFunctions, ratchetDecision } from "./core-coverage-census.mjs";

const rows = [
  ...publicFunctions("Core/text/text.jet", `pub fn trim(x: String) -> String Never! { x }
    pub fn split<T>(x: T) -> String Never! { "" }
    pub fn unused() {}`),
  ...publicFunctions("Core/math/math.jet", "pub fn trim() -> Int Never! { 1 }"),
  ...publicFunctions("Core/crypto/crypto.jet", `pub struct Hasher {
    pub fn new() -> Hasher Never! { {} }
    pub fn digest(self) -> String Never! { "" }
  }
  pub fn new() -> Hasher Never! { Hasher.new() }`),
];
const ids = (source, context) => callSites(source, rows, context).map((call) => call.id);

test("module layout matches Core module keys, including app and nested roots", () => {
  assert.equal(moduleName("Core/text/text.jet"), "core.text");
  assert.equal(moduleName("Core/data/sketch/cms.jet"), "core.data.sketch.cms");
  assert.equal(moduleName("Core/web/storage/local.jet"), "core.web.storage.local");
  assert.equal(moduleName("Core/app/app.jet"), "app");
  assert.equal(moduleName("Core/mod/mod.jet"), "core.mod");
});

test("every public function, including same-named methods, is inventoried", () => {
  const found = publicFunctions("Core/demo/demo.jet", `// pub fn fake() {}
    /* nested /* pub fn fake2() {} */ comment */
    fn hidden() {}
    pub struct A { pub fn new() -> A Never! { {} } }
    pub struct B<T> {}
    impl B<T> { pub fn new<T>(f: fn(Int) -> Int) -[FS]> B Never! { {} } }
    pub fn new() -> A Never! { A.new() }
    pub fn generic<T: [Encode, Decode]>(f: fn(T) -> Int) -> B<T> Never! { {} }
    label :: "pub fn fake3() {}"
  `);
  assert.deepEqual(found.map((row) => row.id), ["core.demo.A.new", "core.demo.B.new", "core.demo.new", "core.demo.generic"]);
  assert.deepEqual(found.map((row) => row.returnType), ["core.demo.A", "core.demo.B", "core.demo.A", "B<T>"]);
});

test("return types preserve builtin, prelude and compound spellings", () => {
  const found = publicFunctions("Core/demo/demo.jet", `pub fn flag() -> Bool Never! { true }
    pub fn count() -> Int Never! { 1 }
    pub fn text() -> String Never! { "" }
    pub fn number() -> Float Never! { Float{1.0} }
    pub fn items() -> [String] Never! { [] }
    pub fn lookup() -> [String: Int] Never! { {} }
    pub fn maybe() -> Int? Never! { None }
    pub fn limits() -> Duration Never! { 1ms }
    pub fn pair() -> (name: String, n: Int) Never! { ("", 1) }
  `);
  assert.deepEqual(found.map((row) => row.returnType),
    ["Bool", "Int", "String", "Float", "[String]", "[String: Int]", "Int?", "Duration", "(name: String, n: Int)"]);
});

test("calls resolve fully qualified, module aliases, member imports and generics", () => {
  assert.deepEqual(ids(`use core.text as words
    use core.text.[split as pieces]
    fn run() { words.trim("a"); core.text.trim("b"); pieces<Int>(1) }`),
  ["core.text.trim", "core.text.trim", "core.text.split"]);
  assert.deepEqual(ids("use core.text\nfn trim() {}\nfn run() { text.trim(\"a\") }"), ["core.text.trim"]);
});

test("comments, strings, references and declarations do not count as calls", () => {
  assert.deepEqual(ids(`use core.text as t
    // t.unused()
    /* t.unused() */
    text :: "t.unused()"
    reference :: t.unused
    fn unused() {}
    fn run() { print("{t.trim(\"actual call\")}") }
  `), ["core.text.trim"]);
});

test("scoped aliases do not escape a block or credit a same-named module", () => {
  assert.deepEqual(ids(`use core.text as t
    fn run() {
      { use core.math as t; t.trim() }
      t.trim("a")
    }`), ["core.math.trim", "core.text.trim"]);
});

test("qualified types, typed receivers, inferred constructors and chained calls", () => {
  assert.deepEqual(ids(`use core.crypto as crypto
    use core.crypto.[Hasher]
    fn run(h: Hasher) {
      h.digest()
      value :: crypto.new()
      value.digest()
      Hasher.new().digest()
      unknown.digest()
    }`), ["core.crypto.Hasher.digest", "core.crypto.new", "core.crypto.Hasher.digest", "core.crypto.Hasher.new", "core.crypto.Hasher.digest"]);
});

test("embedded host programs isolate alias environments and ignore host comments", () => {
  const programs = embeddedPrograms(`// r#"use core.text as t; t.unused()"#
    let first = r#"use core.text as t
      fn run() { t.trim(\"a\") }"#;
    let second = r##"use core.math as t
      fn run() { t.trim() }"##;
    let third = "use core.text as t\\nfn run() { t.split<Int>(1) }";
    let message = "core.text.unused()";
    let fourth = r"fn run() { core.text.trim() }";
  `);
  assert.equal(programs.length, 4);
  assert.deepEqual(programs.flatMap((program) => ids(program)), ["core.text.trim", "core.math.trim", "core.text.split", "core.text.trim"]);
});

function fixture(t) {
  const parent = join(homedir(), ".cache/jet-test-scratch/CardW-CB-CENSUS");
  mkdirSync(parent, { recursive: true });
  const root = mkdtempSync(join(parent, "census-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const put = (path, source) => {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), source);
  };
  put("Core/demo/demo.jet", `/// Text mentioning unused() is not a doctest.
    /// \`\`\`jet
    /// fn run() { used() }
    /// \`\`\`
    pub fn used() {}
    pub fn unused() {}
    pub fn example_only() {}
    pub fn test_only() {}
  `);
  put("Examples/features/basics/demo.jet", "use core.demo as d\nfn run() { d.example_only(); d.example_only() }");
  put("Examples/features/expected/fake.jet", "fn run() { core.demo.unused() }");
  put("tests/demo.rs", 'let program = r#"fn run() { core.demo.test_only() }"#;');
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

test("ratchet passes equal and lower counts, rejects increases, and cannot rise", () => {
  assert.deepEqual(ratchetDecision(5, null), { passes: false, next: { version: 1, uncovered: 5 } });
  assert.equal(ratchetDecision(5, { version: 1, uncovered: 5 }).passes, true);
  assert.deepEqual(ratchetDecision(3, { version: 1, uncovered: 5 }), { passes: true, next: { version: 1, uncovered: 3 } });
  assert.deepEqual(ratchetDecision(6, { version: 1, uncovered: 5 }), { passes: false, next: { version: 1, uncovered: 5 } });
  for (const previous of [{ version: 2, uncovered: 1 }, { version: 1, uncovered: -1 }, { version: 1, uncovered: "1" }]) assert.throws(() => ratchetDecision(1, previous));
});

test("CLI initializes, checks without rewriting, lowers, and refuses regressions", (t) => {
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
    assert.equal(JSON.parse(baseline).uncovered, 1);
    assert.equal(main(["--check"], root), 0);
    assert.equal(readFileSync(path, "utf8"), baseline);
    put("tests/extra.jet", "fn run() { core.demo.unused() }");
    assert.equal(main(["--check"], root), 0);
    assert.equal(readFileSync(path, "utf8"), baseline);
    assert.equal(main(["--write"], root), 0);
    assert.equal(JSON.parse(readFileSync(path, "utf8")).uncovered, 0);
    put("tests/extra.jet", "fn run() {}");
    assert.equal(main(["--check"], root), 1);
    assert.equal(main(["--write"], root), 1);
    assert.equal(JSON.parse(readFileSync(path, "utf8")).uncovered, 0);
    assert.equal(main(["--check", "--write"], root), 1);
    assert.equal(main(["--typo"], root), 1);
  } finally {
    console.log = log;
    console.error = error;
  }
});
