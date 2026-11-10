#!/usr/bin/env node
// The memory half of the net_read_into_vs_read pair (#5976 c112). Wall time
// is the shared harness's (`performance-surface.mjs --pair
// net_read_into_vs_read`, into the row's evidence_file). This measures the
// row's metrics the harness does not: allocations per read and peak live heap
// bytes, from the runtime counting policy's own facts in a counted build of
// each arm, plus peak RSS (getrusage through the gauntlet timer) of the
// uncounted build. Both arms build from the row's surface_source and
// plain_source at --optimize=full. Each metric gets its own verdict under the
// harness's rule that only strictly below plain wins. --write replaces the
// row's memory_evidence_file; without it the result is printed.
//
//   node Tools/perf/read-into/memory.mjs --artifact <jet> --gdb <gdb> [--write]
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { TRIALS, classifyVerdict, median, pairedBootstrap, sha256 } from "../../gauntlet/harness/performance-surface.mjs";

const repoDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const timer = path.join(repoDir, "Tools/gauntlet/harness/timer.py");
const manifestPath = path.join(repoDir, "Tools/gauntlet/measurement-manifest.json");
export const PAIR = "net_read_into_vs_read";
const COUNTED_RUNS = 3;
const ARMS = ["surface", "plain"];
const COUNTING = "    allocator: mem.Counting.over(mem.Heap, cap: 2.gb)";

function argument(argv, name, fallback = null) {
  const at = argv.indexOf(name);
  return at >= 0 && at + 1 < argv.length ? argv[at + 1] : fallback;
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", maxBuffer: 64 << 20, timeout: 300000, ...options });
  if (result.error) throw new Error(`${command} ${args.join(" ")}: ${result.error.message}`);
  return result;
}

function pairRow() {
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  const row = manifest.performance_surface_pairs.pairs.find((candidate) => candidate.id === PAIR);
  if (!row) throw new Error(`${PAIR} has no row in Tools/gauntlet/measurement-manifest.json`);
  return row;
}

// One arm's source as main.jet in a fresh directory. The counted build adds
// the counting allocator to the source's own package block.
function build(jet, source, dir, counted) {
  mkdirSync(dir, { recursive: true });
  const text = counted ? source.replace(/^package: \{$/m, `package: {\n${COUNTING}`) : source;
  if (counted && text === source) throw new Error("the pair source has no `package: {` line to take the counting allocator");
  writeFileSync(path.join(dir, "main.jet"), text);
  const out = path.join(dir, "out");
  const built = run(jet, ["build", "--optimize=full", "--quiet", `--output-dir=${out}`, "main.jet"], { cwd: dir });
  const binary = path.join(out, "main");
  if (built.status !== 0 || !existsSync(binary)) throw new Error(`build ${dir} failed:\n${built.stdout}${built.stderr}`);
  return binary;
}

function readsOf(stderr) {
  const match = /reads=(\d+)/.exec(stderr);
  if (!match) throw new Error(`no reads= line in stderr: ${stderr}`);
  return Number(match[1]);
}

// The counting policy's own facts, read by gdb at exit_group: the program's
// image keeps no fact export, so the script finds the allocator root through
// jet_rt_alloc_init's first instruction (`lea <root>(%rip)`) and reads the
// words Alloc.jet lays out there: mode144, allocations160, high-water184.
const FACTS_SCRIPT = `import gdb, re
line = gdb.execute("x/1i jet_rt_alloc_init", to_string=True)
found = re.search(r"lea\\s+0x[0-9a-f]+\\(%rip\\),%\\w+\\s+# (0x[0-9a-f]+)", line)
if not found:
    raise gdb.GdbError("jet_rt_alloc_init does not start with the root lea: " + line)
memory = gdb.selected_inferior()
word = lambda at: int.from_bytes(bytes(memory.read_memory(at, 8)), "little", signed=True)
root = word(int(found.group(1), 16))
print("ALLOCATOR-FACTS mode=%d allocations=%d high_water=%d" % (word(root + 144), word(root + 160), word(root + 184)))
`;

function counted(gdb, binary, scratch, label) {
  const errFile = path.join(scratch, `${label}.err`);
  const script = path.join(scratch, "facts.py");
  writeFileSync(script, FACTS_SCRIPT);
  const result = run(gdb, [
    "-q", "-batch",
    "-ex", "set pagination off",
    "-ex", "catch syscall exit_group",
    "-ex", `run 2>${errFile}`,
    "-x", script,
    binary,
  ]);
  const facts = /ALLOCATOR-FACTS mode=(\d+) allocations=(\d+) high_water=(\d+)/.exec(result.stdout);
  if (!facts || facts[1] !== "1") throw new Error(`no counting-policy facts from gdb:\n${result.stdout}${result.stderr}`);
  const reads = readsOf(readFileSync(errFile, "utf8"));
  const allocations = Number(facts[2]);
  return { allocations, reads, allocations_per_read: allocations / reads, peak_live_heap_bytes: Number(facts[3]) };
}

function peakRss(binary, expected) {
  const result = run("python3", [timer, "--", binary]);
  const sample = JSON.parse(result.stdout.trim().split("\n").at(-1));
  if (sample.exit_code !== 0) throw new Error(`${binary} exited ${sample.exit_code}`);
  const direct = run(binary, []);
  if (direct.stdout !== expected) throw new Error(`${binary} printed ${JSON.stringify(direct.stdout)}`);
  return sample.peak_rss_kb;
}

function measure(argv) {
  const jet = path.resolve(argument(argv, "--artifact", process.env.JET ?? "jet"));
  const gdb = argument(argv, "--gdb", "gdb");
  const row = pairRow();
  const scratch = mkdtempSync(path.join(argument(argv, "--scratch", os.tmpdir()), "read-into-memory-"));
  try {
    const sources = { surface: row.surface_source, plain: row.plain_source };
    const plainBuilds = Object.fromEntries(ARMS.map((arm) => [arm, build(jet, sources[arm], path.join(scratch, arm), false)]));
    const countedBuilds = Object.fromEntries(ARMS.map((arm) => [arm, build(jet, sources[arm], path.join(scratch, `${arm}-counted`), true)]));
    const facts = Object.fromEntries(ARMS.map((arm) => [arm,
      Array.from({ length: COUNTED_RUNS }, (_value, index) => counted(gdb, countedBuilds[arm], scratch, `${arm}-${index}`))]));
    const rss = { surface: [], plain: [] };
    for (let index = 0; index < TRIALS; index += 1) {
      for (const arm of index % 2 === 0 ? ARMS : [...ARMS].reverse()) rss[arm].push(peakRss(plainBuilds[arm], row.expected_output));
    }
    const perRead = (arm) => median(facts[arm].map((sample) => sample.allocations_per_read));
    const heap = (arm) => median(facts[arm].map((sample) => sample.peak_live_heap_bytes));
    const peak = (arm) => median(rss[arm]);
    // The harness's rule: only strictly below plain is a win. The counts are
    // exact, so they compare directly; RSS is sampled at page resolution, so
    // it is judged like wall time, on its paired runs' bootstrap interval.
    const strict = (surface, plain) => (surface < plain ? "win" : "loss");
    const rssRatio = peak("surface") / peak("plain");
    const rssInterval = pairedBootstrap(rss.surface, rss.plain);
    const round = (value) => Number(value.toFixed(3));
    return {
      schema: "jet.gauntlet.performance-surface-memory.v1",
      id: PAIR,
      surface_source_sha256: sha256(row.surface_source),
      plain_source_sha256: sha256(row.plain_source),
      expected_output: row.expected_output,
      target: "jet build --optimize=full",
      artifact_sha256: sha256(readFileSync(jet)),
      host: { platform: os.platform(), release: os.release(), arch: os.arch(), cpu_model: os.cpus()[0]?.model ?? null, cpu_count: os.cpus().length },
      generated_command: "node Tools/perf/read-into/memory.mjs --artifact <jet> --gdb <gdb> --write",
      counting_policy: "mem.Counting.over(mem.Heap, cap: 2.gb) in each arm's package block; facts read at exit_group",
      counted_runs: facts,
      rss_runs: TRIALS,
      surface_allocations_per_read: round(perRead("surface")),
      plain_allocations_per_read: round(perRead("plain")),
      surface_peak_live_heap_bytes: heap("surface"),
      plain_peak_live_heap_bytes: heap("plain"),
      surface_peak_rss_kb: peak("surface"),
      plain_peak_rss_kb: peak("plain"),
      peak_rss_ratio: rssRatio,
      peak_rss_ratio_lower95: rssInterval.ratio_lower95,
      peak_rss_ratio_upper95: rssInterval.ratio_upper95,
      surface_rss_kb: rss.surface,
      plain_rss_kb: rss.plain,
      metric_verdicts: {
        allocations_per_read: strict(perRead("surface"), perRead("plain")),
        peak_live_heap_bytes: strict(heap("surface"), heap("plain")),
        peak_rss_kb: classifyVerdict(rssRatio, rssInterval),
      },
    };
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

const argv = process.argv.slice(2);
const result = measure(argv);
if (argv.includes("--write")) writeFileSync(path.join(repoDir, pairRow().memory_evidence_file), `${JSON.stringify(result, null, 2)}\n`);
const { counted_runs, surface_rss_kb, plain_rss_kb, ...summary } = result;
console.log(JSON.stringify(summary, null, 2));
