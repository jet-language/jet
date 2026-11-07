// The internal publisher uses the existing assembler/area graph. These are
// filesystem/assembly fixtures only: no compiler, provider or runtime executes.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const assembler = resolve(dirname(fileURLToPath(import.meta.url)), "assemble.mjs");
const entry = "Tools/packaging/native-runtime-pack-publisher.jet";
const paths = ["Jetpack/Foundation/Source/Fixture.jet", "Jetpack/Store/Source/Fixture.jet", entry, "Jetpack/CLI/Source/Fixture.jet"];
const manifest = `# == Foundation ==\n${paths[0]}\n# == Engine (folders: Store) ==\n${paths[1]}\n${entry}\n# == CLI ==\n${paths[3]}\n`;

async function fixture(run) {
  const root = await mkdtemp(resolve(tmpdir(), "jetpack-publication-assembly-"));
  const put = async (name, text) => {
    const path = resolve(root, name);
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, text);
  };
  try {
    await put("Jetpack/Bootstrap/areas.list", "Foundation:\nEngine: Foundation\nCLI: Foundation\n");
    await put("Jetpack/Bootstrap/sources.list", manifest);
    await put("Jetpack/Bootstrap/tests.list", "# == Engine (folders: Store) ==\nJetpack/Store/Tests/Fixture.jet\n");
    // The compiler-slice check walks the compiler packages from Compiler/JetCli;
    // an empty entry package keeps the slice (and its drift) empty.
    await put("Compiler/JetCli/package.jet", 'name: "jet-cli"\n');
    await put(paths[0], "foundation_fixture: fn() {}\n");
    await put(paths[1], "store_fixture: fn() {}\n");
    await put(entry, "native_runtime_pack_publisher: fn() NativeRuntimePackError! {}\n");
    await put(paths[3], "cli_fixture: fn() {}\n");
    await put("Jetpack/Store/Tests/Fixture.jet", '#Test("store fixture") {\n    assert(true)\n}\n');
    const invoke = (mode, extra = {}) => spawnSync(process.execPath, [assembler, mode], {
      cwd: root,
      encoding: "utf8",
      env: { ...process.env, JETPACK_BOOTSTRAP_SOURCE_ROOT: root, JETPACK_BOOTSTRAP_SCRATCH: resolve(root, "scratch"), JETPACK_WORKER: "publication-fixture", JETPACK_BOOTSTRAP_AREAS: "", JETPACK_BOOTSTRAP_OWN_AREA: "", JETPACK_BOOTSTRAP_DEPS_FROM_HEAD: "0", ...extra },
    });
    await run({ root, put, invoke });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

test("publication emits only the canonical Engine closure plus one fixed forwarding entry", async () => {
  await fixture(async ({ root, invoke }) => {
    const result = invoke("publication", { JETPACK_BOOTSTRAP_AREAS: "CLI", JETPACK_BOOTSTRAP_OWN_AREA: "CLI" });
    assert.equal(result.status, 0, result.stderr);
    const map = JSON.parse(await readFile(resolve(root, "scratch/publication/jetpack.map.json"), "utf8"));
    assert.equal(map.mode, "publication");
    assert.deepEqual(map.files.map(({ path }) => path), paths.slice(0, 3));
    assert.deepEqual(map.tests, []);
    const source = await readFile(resolve(root, "scratch/publication/project/src/jetpack.jet"), "utf8");
    assert.equal(source.includes("#Test"), false);
    assert.equal(source.includes("cli_fixture"), false);
    assert.equal((source.match(/\brun: fn\(\)/g) ?? []).length, 1);
    assert.equal(source.endsWith("// [jetpack-bootstrap generated: internal native runtime-pack publisher]\nrun: fn() NativeRuntimePackError! {\n    native_runtime_pack_publisher()\n}\n"), true);
  });
});

test("ordinary modes retain test semantics without emitting the fixed Tools entry", async () => {
  await fixture(async ({ root, invoke }) => {
    for (const mode of ["check", "aot"]) {
      const result = invoke(mode, { JETPACK_BOOTSTRAP_OWN_AREA: "Engine" });
      assert.equal(result.status, 0, result.stderr);
      const map = JSON.parse(await readFile(resolve(root, `scratch/${mode}/jetpack.map.json`), "utf8"));
      assert.deepEqual(map.files.map(({ path }) => path), [paths[0], paths[1], paths[3], "Jetpack/Store/Tests/Fixture.jet"]);
      assert.deepEqual(map.tests, ["store fixture"]);
      assert.deepEqual(map.test_areas, ["Engine"]);
      const source = await readFile(resolve(root, `scratch/${mode}/project/src/jetpack.jet`), "utf8");
      assert.equal(source.includes("native_runtime_pack_publisher"), false);
      assert.equal(source.includes('#Test("store fixture")'), true);
      assert.equal((source.match(/\brun: fn\(\)/g) ?? []).length, 0);
    }
    const engine = invoke("aot", { JETPACK_BOOTSTRAP_OWN_AREA: "Engine", JETPACK_BOOTSTRAP_TEST_AREAS: "Engine" });
    assert.equal(engine.status, 0, engine.stderr);
    const engineMap = JSON.parse(await readFile(resolve(root, "scratch/aot/jetpack.map.json"), "utf8"));
    assert.deepEqual(engineMap.test_areas, ["Engine"]);
    const unknown = invoke("aot", { JETPACK_BOOTSTRAP_TEST_AREAS: "CLI" });
    assert.notEqual(unknown.status, 0);
    assert.match(unknown.stderr, /JETPACK_BOOTSTRAP_TEST_AREAS names CLI/);
  });
});

test("publication refuses missing or duplicate registration, generic Tools paths and a wrong section", async () => {
  await fixture(async ({ put, invoke }) => {
    await put("Tools/packaging/other.jet", "other: fn() {}\n");
    for (const sources of [manifest.replace(`${entry}\n`, ""), manifest.replace(`${entry}\n`, `${entry}\n${entry}\n`), manifest.replace(entry, "Tools/packaging/other.jet"), manifest.replace(`${entry}\n# == CLI ==`, `# == CLI ==\n${entry}`)]) {
      await put("Jetpack/Bootstrap/sources.list", sources);
      const result = invoke("publication");
      assert.equal(result.status, 1);
      assert.equal(result.stderr.startsWith("jetpack-bootstrap: "), true);
      assert.equal(result.stdout, "");
    }
  });
});
