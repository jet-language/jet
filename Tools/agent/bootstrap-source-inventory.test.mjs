import assert from "node:assert/strict";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { SUPPORT_ITEMS } from "../../Compiler/JetBackend/Tests/mir-debug.mjs";

const assembler = readFileSync(new URL("../../Compiler/Bootstrap/assemble.mjs", import.meta.url), "utf8");
const roots = [...assembler.match(/const sourceRoots = \[([\s\S]*?)\];/)[1].matchAll(/"([^"]+)"/g)].map((match) => match[1]);
const observerModules = ["mir-debug.mjs", "front-feed-codec.mjs"].map((name) => [name,
  readFileSync(new URL(`../../Compiler/JetBackend/Tests/${name}`, import.meta.url), "utf8")]);

function fixture(t, { unlisted = false, unlistedCli = false, unwiredCli = false, runtimeManifest = true, duplicate = false } = {}) {
  const root = mkdtempSync(join(tmpdir(), "jet-bootstrap-inventory-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const put = (path, text) => {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), text);
  };
  for (const path of roots) mkdirSync(join(root, path), { recursive: true });
  put("Compiler/Bootstrap/assemble.mjs", assembler);
  for (const [name, text] of observerModules) put(`Compiler/JetBackend/Tests/${name}`, text);
  const schemaPath = "Compiler/JetFoundation/Source/MIR/MIR.jet";
  const listed = ["Compiler/JetFoundation/Source/Compiler.jet", "Compiler/JetBackend/Source/OS/Linux.jet", schemaPath,
    ...SUPPORT_ITEMS.map(([path]) => path)];
  for (const [path] of SUPPORT_ITEMS) put(path, "\n");
  put(schemaPath, "struct MIRProgram {\n    values: [Int]\n}\n");
  if (duplicate) listed.push(listed[0]);
  put("Compiler/Bootstrap/sources.list", `${listed.join("\n")}\n`);
  put("Compiler/JetFoundation/Source/Compiler.jet", "COMPILER_MARKER :: prep { 1 }\n");
  put("Compiler/JetBackend/Source/OS/Linux.jet", "SHARED_MARKER :: prep { 2 }\n");
  put("Compiler/JetBackend/Source/ByteHandlesABI.jet", "RUNTIME_ONLY_MARKER :: prep { 3 }\n");
  if (runtimeManifest) put("Compiler/JetRuntime/sources.list", "# Runtime inventory\nCompiler/JetBackend/Source/OS/Linux.jet\nCompiler/JetBackend/Source/ByteHandlesABI.jet\n");
  if (unlisted) put("Compiler/JetFoundation/Source/Unlisted.jet", "UNLISTED_MARKER :: prep { 4 }\n");
  if (unlistedCli) put("Compiler/JetCli/Source/Cli/Unlisted.jet", "UNLISTED_CLI_MARKER :: prep { 5 }\n");
  if (unwiredCli) {
    for (const path of ["Dependency.jet", "Jobs.jet", "Package/Bundle.jet", "Package/FlashAudit.jet", "Package/FlashPlan.jet", "Package/NativeBundle.jet", "Package/NativeFlash.jet", "Supply.jet", "ToolchainPin.jet"]) {
      put(`Compiler/JetCli/Source/Cli/${path}`, "UNWIRED_CLI_MARKER :: prep { 6 }\n");
    }
  }
  const result = spawnSync(process.execPath, [join(root, "Compiler/Bootstrap/assemble.mjs")], {
    cwd: root, encoding: "utf8", env: { ...process.env, HOME: join(root, "home"), JET_BOOTSTRAP_SOURCE_ROOT: root },
  });
  return { root, result };
}

test("compiler inventory excludes runtime-only ABI units and retains explicitly shared sources", (t) => {
  const { root, result } = fixture(t);
  assert.equal(result.status, 0, result.stderr);
  const unit = readFileSync(join(root, "home/.cache/jet-luna/compiler-bootstrap/project/src/compiler.jet"), "utf8");
  assert.match(unit, /COMPILER_MARKER/);
  assert.match(unit, /SHARED_MARKER/);
  assert.match(unit, /mirw_list_int\(v\.values/);
  assert.doesNotMatch(unit, /RUNTIME_ONLY_MARKER/);
});

test("runtime exclusion does not mask an unlisted compiler source", (t) => {
  const { result } = fixture(t, { unlisted: true });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Jet compiler sources are not in/);
  assert.match(result.stderr, /Compiler\/JetFoundation\/Source\/Unlisted\.jet/);
  assert.doesNotMatch(result.stderr, /ByteHandlesABI\.jet/);
});

test("compiler inventory fails closed when the runtime source manifest is missing", (t) => {
  const { result } = fixture(t, { runtimeManifest: false });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /cannot read runtime source manifest Compiler\/JetRuntime\/sources\.list/);
});

test("compiler inventory rejects duplicate manifest entries", (t) => {
  const { result } = fixture(t, { duplicate: true });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /duplicate source Compiler\/JetFoundation\/Source\/Compiler\.jet/);
});

test("compiler inventory retains the explicit pre-merge unwired CLI boundary", (t) => {
  const { root, result } = fixture(t, { unwiredCli: true });
  assert.equal(result.status, 0, result.stderr);
  const unit = readFileSync(join(root, "home/.cache/jet-luna/compiler-bootstrap/project/src/compiler.jet"), "utf8");
  assert.doesNotMatch(unit, /UNWIRED_CLI_MARKER/);
});

test("unwired CLI exclusions do not mask another unlisted CLI source", (t) => {
  const { result } = fixture(t, { unwiredCli: true, unlistedCli: true });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Compiler\/JetCli\/Source\/Cli\/Unlisted\.jet/);
  assert.doesNotMatch(result.stderr, /(?:Dependency|Jobs|Supply|ToolchainPin)\.jet/);
});

test("shared CLI registries have one authored owner in the compiler inventory", () => {
  const repo = new URL("../../", import.meta.url);
  const paths = readFileSync(new URL("Compiler/Bootstrap/sources.list", repo), "utf8")
    .split("\n").map((line) => line.trim()).filter((line) => line && !line.startsWith("#"));
  const canonical = "Compiler/JetFoundation/Source/Registry/CLICommands.jet";
  assert.equal(paths.filter((path) => path === canonical).length, 1);
  assert.equal(existsSync(new URL("Compiler/JetCli/Source/Cli/Commands.jet", repo)), false);
  const owners = new Map(["JET_CLI_COMMANDS", "JET_CLI_HANDLERS", "JET_CLI_FLAGS"].map((name) => [name, []]));
  for (const path of paths) {
    const text = readFileSync(new URL(path, repo), "utf8");
    for (const declaration of text.matchAll(/^([A-Za-z_][A-Za-z_0-9]*)[ \t]*::/gm)) {
      owners.get(declaration[1])?.push(path);
    }
  }
  for (const [name, found] of owners) assert.deepEqual(found, [canonical], `${name} must have one canonical declaration`);
});
