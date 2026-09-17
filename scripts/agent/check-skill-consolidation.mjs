#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { fileURLToPath } from "node:url";
import { dirname, join, relative, resolve } from "node:path";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const HOME = homedir();
const ROUTER = join(ROOT, ".agents", "skills", "JetSkillsRouter.md");
const INVENTORY = join(ROOT, ".agents", "skills", "_shared", "skill-sources.md");
const OWNER_GUIDANCE = join(ROOT, "AGENTS.md");
const ORCHESTRATION = join(ROOT, ".agents", "skills", "orchestration", "SKILL.md");
const SKILLS_LOCK = join(ROOT, "skills-lock.json");
const LANE_DISPATCH = join(ROOT, "scripts", "agent", "lane-dispatch.mjs");
const LANE_KEEPER = join(ROOT, "scripts", "agent", "lane-keeper.sh");
const LANE_CHECK = join(ROOT, "scripts", "agent", "lane-check.sh");
const MANAGED = process.env.JET_MANAGED_SKILLS || join(HOME, ".omp", "agent", "managed-skills");
const RETIRED = new Set(["burndown", "ask-matt", "grill-me", "setup-matt-pocock-skills"]);

function fail(message) {
  throw new Error(message);
}

function assert(condition, message) {
  if (!condition) fail(message);
}

function text(path) {
  return readFileSync(path, "utf8");
}

function directSkillFiles(root) {
  if (!existsSync(root)) return [];
  return readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && existsSync(join(root, entry.name, "SKILL.md")))
    .map((entry) => ({ name: entry.name, path: join(root, entry.name, "SKILL.md") }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

function recursiveSkillFiles(root) {
  if (!existsSync(root)) return [];
  const files = [];
  const visit = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) visit(path);
      else if (entry.isFile() && entry.name === "SKILL.md") files.push(path);
    }
  };
  visit(root);
  return files.sort();
}

function versionedSkillRoots(root) {
  if (!existsSync(root)) return [];
  return readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && existsSync(join(root, entry.name, "skills")))
    .map((entry) => join(root, entry.name, "skills"))
    .sort();
}

function inventorySources() {
  const cache = join(HOME, ".codex", "plugins", "cache");
  return [
    { key: "R", label: "project", root: join(ROOT, ".agents", "skills") },
    { key: "T", label: "plugin", root: join(ROOT, "plugins", "tower", "skills") },
    { key: "M", label: "managed", root: MANAGED },
    { key: "G", label: "global", root: join(HOME, ".agents", "skills") },
    { key: "H", label: "host", root: join(HOME, ".codex", "skills") },
    ...versionedSkillRoots(join(cache, "ponytail", "ponytail")).map((root) => ({ key: "H", label: "cache", root })),
    ...versionedSkillRoots(join(cache, "personal", "tower")).map((root) => ({ key: "H", label: "cache", root })),
  ];
}

function routerRows() {
  const rows = new Map();
  for (const line of text(INVENTORY).split("\n")) {
    if (!line.startsWith("|")) continue;
    const columns = line.split("|").slice(1, -1).map((column) => column.trim());
    const name = columns[2]?.match(/^`([^`]+)`$/)?.[1];
    const status = columns[3];
    if (!name || !["keep", "narrow", "disable", "retire"].includes(status)) continue;
    assert(!rows.has(name), `router maps ${name} more than once`);
    assert(columns[4] && columns[5], `router row ${name} must preserve trigger and unique rule`);
    rows.set(name, { reach: columns[0], source: columns[1], status });
  }
  return rows;
}

function checkMap() {
  const rows = routerRows();
  assert(existsSync(INVENTORY), "skill source inventory is missing");
  assert(!/via a Luna max subagent|via a Sol .*subagent/i.test(text(ROUTER)), "router selects a model outside owner guidance");

  const discovered = [];
  for (const source of inventorySources()) {
    for (const file of directSkillFiles(source.root)) {
      const row = rows.get(file.name);
      if (!row) {
        assert(source.key !== "R" && source.key !== "T", `${source.label} skill ${file.name} is not in the disposition map`);
        // Unmatched host installations have no Jet route; do not turn this
        // routing contract into a census of the user's external catalogs.
        continue;
      }
      assert(row.source.includes(source.key), `${file.name} is missing source key ${source.key}`);
      discovered.push(file.name);
    }
  }
  for (const name of RETIRED) {
    assert(!existsSync(join(ROOT, ".agents", "skills", name, "SKILL.md")), `retired Jet skill still exists: ${name}`);
  }
  const locked = JSON.parse(text(SKILLS_LOCK)).skills ?? {};
  for (const name of RETIRED) assert(!Object.hasOwn(locked, name), `retired skill remains in skills-lock.json: ${name}`);
  for (const name of [...new Set(discovered)]) {
    assert(!RETIRED.has(name), `retired route is still discovered as a filesystem skill: ${name}`);
  }
  return {
    rows,
    discovered: new Set(discovered),
    filesystemCount: [...rows.values()].filter((row) => row.reach.includes("F") && row.status !== "retire").length,
  };
}


function checkAdapters() {
  const guide = text(OWNER_GUIDANCE);
  const configPath = process.env.JET_OMP_CONFIG || join(HOME, ".omp", "agent", "config.yml");
  assert(existsSync(configPath), `OMP adapter config is missing: ${configPath}`);
  const config = text(configPath);
  for (const marker of [
    "implementation: openai-codex/gpt-5.6-luna:max",
    "full_review: openai-codex/gpt-5.6-sol:high",
    "cavecrew: anthropic/claude-sonnet-5",
    "task: \"@implementation\"",
    "reviewer: \"@full_review\"",
    "security-reviewer: \"@full_review\"",
    "cavecrew-builder: \"@cavecrew\"",
    "cavecrew-reviewer: \"@cavecrew\"",
    "cavecrew-investigator: \"@cavecrew\"",
  ]) assert(config.includes(marker), `OMP adapter mapping missing: ${marker}`);
  for (const marker of ["@implementation", "@full_review", "@cavecrew", "GPT-5.6 Luna", "GPT-5.6 Sol", "Sonnet"]) {
    assert(guide.includes(marker), `owner adapter profile missing: ${marker}`);
  }
}

function checkConflicts() {
  const projectFiles = recursiveSkillFiles(join(ROOT, ".agents", "skills"));
  const projectText = projectFiles.map((path) => text(path)).join("\n");
  const forbiddenProject = [
    ["retired generic installer", /setup-matt-pocock-skills/],
    ["legacy Agent subagent type", /subagent_type\s*=/],
    ["legacy two-Agent dispatch", /Send a single message with two `Agent`/],
  ];
  for (const [label, pattern] of forbiddenProject) assert(!pattern.test(projectText), `${label} remains in an active Jet skill`);

  const perFile = [
    ["implement/SKILL.md", [/Run typechecking regularly/, /full test suite once at the end/, /Commit your work to the current branch/, /Once done, use \/code-review/]],
    ["improve-codebase-architecture/SKILL.md", [/Then use the Agent tool with/, /subagent_type=Explore/, /falling back to \/tmp/]],
    ["wayfinder/SKILL.md", [/run \/setup-matt-pocock-skills/, /spin up a \/research subagent/, /throwaway `research\//]],
    ["triage/SKILL.md", [/run \/setup-matt-pocock-skills/, /run the `?\/?grilling` and the `?\/?domain-modeling` skills together/]],
    ["resolving-merge-conflicts/SKILL.md", [/typically typecheck, then tests, then format/, /Stage everything and commit/]],
    ["codebase-design/DESIGN-IT-TWICE.md", [/using the Agent tool/, /Spawn 3\+ sub-agents/]],
    ["code-review/SKILL.md", [/selects both models/, /general-purpose subagent/]],
    ["batch-grill-me/SKILL.md", [/dispatch a sub-agent/]],
    ["first-principles-audit/SKILL.md", [/Launch parallel read-only research passes/]],
  ];
  for (const [relativePath, patterns] of perFile) {
    const path = join(ROOT, ".agents", "skills", relativePath);
    const content = text(path);
    for (const pattern of patterns) assert(!pattern.test(content), `${relativePath} retains conflicting rule ${pattern}`);
  }

  const orchestration = text(ORCHESTRATION);
  for (const pattern of [/Run 25-30 lanes/, /Parallelism is the whole game/, /Close on implementation; batch the proof/, /gpt-5\.6-(?:luna|sol)/i]) {
    assert(!pattern.test(orchestration), `orchestration retains conflicting rule ${pattern}`);
  }
  const dispatch = text(LANE_DISPATCH);
  for (const pattern of [/PARALLELISM IS THE WHOLE GAME/, /Run 25-30 lanes/, /codex exec/, /LANE_CAP \?\? 30/]) {
    assert(!pattern.test(dispatch), `lane-dispatch retains conflicting rule ${pattern}`);
  }
  assert(dispatch.includes("OMP task/hub") && dispatch.includes("JET_OMP_FALLBACK_REASON"), "lane-dispatch lost its fail-closed fallback guard");
  const keeper = text(LANE_KEEPER);
  assert(keeper.includes("JET_OMP_FALLBACK_REASON") && keeper.includes("lane-keeper disabled"), "lane-keeper can bypass OMP without a recorded failure");
  const laneCheck = text(LANE_CHECK);
  for (const pattern of [/up to 30 lanes/i, /thirty lanes/i, /30 lanes/i]) {
    assert(!pattern.test(laneCheck), `lane-check retains fixed concurrency rule ${pattern}`);
  }
  const review = text(join(ROOT, ".agents", "skills", "code-review", "SKILL.md"));
  assert(review.includes("parallel sub-agents") && review.includes("OMP `task`"), "code-review lost its two-axis workflow or OMP route");
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function driftLockSources() {
  return [
    { name: "repo", root: join(ROOT, ".agents", "skills"), readOnly: false },
    { name: "managed", root: MANAGED, readOnly: true },
    { name: "plugin", root: join(ROOT, "plugins", "tower", "skills"), readOnly: false },
    { name: "vendor", root: process.env.JET_VENDOR_SKILLS || join(ROOT, "vendor", "skills"), readOnly: true },
    { name: "cache", root: process.env.JET_SKILL_CACHE_ROOT || join(HOME, ".codex", "plugins", "cache"), readOnly: true },
  ];
}

function makeDriftLock() {
  return {
    schema: "jet.agent-skill-drift.v1",
    sources: driftLockSources().map((source) => ({
      name: source.name,
      root: source.root,
      readOnly: source.readOnly,
      files: recursiveSkillFiles(source.root).map((path) => ({
        path: relative(source.root, path),
        sha256: sha256(readFileSync(path)),
      })),
    })),
  };
}

function withScratch(prefix, action) {
  const base = resolve(process.env.JET_SKILL_TEST_SCRATCH || join(HOME, ".cache", "jet-test-scratch"));
  assert(base !== "/tmp" && !base.startsWith("/tmp/"), "skill exercise cannot use RAM-backed /tmp");
  mkdirSync(base, { recursive: true });
  const directory = mkdtempSync(join(base, prefix));
  try {
    return action(directory);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

function checkDriftLock() {
  return withScratch("skill-drift-", (scratch) => {
    const lockPath = join(scratch, "skill-sources.lock.json");
    const before = makeDriftLock();
    writeFileSync(lockPath, `${JSON.stringify(before, null, 2)}\n`);
    const loaded = JSON.parse(text(lockPath));
    assert(JSON.stringify(loaded) === JSON.stringify(before), "generated drift lock could not be read back");
    assert(JSON.stringify(makeDriftLock()) === JSON.stringify(before), "active skill source drifted during the lock check");
    return before.sources.reduce((count, source) => count + source.files.length, 0);
  });
}


function main() {
  if (process.argv.includes("--help")) {
    console.log("usage: node scripts/agent/check-skill-consolidation.mjs");
    return;
  }
  assert(process.argv.length === 2, "unsupported argument; this command checks sources, not host behavior");
  const map = checkMap();
  checkAdapters();
  checkConflicts();
  const lockedFiles = checkDriftLock();
  console.log(`skill index: ${map.rows.size} disposition rows (${map.filesystemCount} filesystem-backed) cover ${map.discovered.size} checked Jet source names; triggers and unique rules are present`);
  console.log(`skill source constraints: passed; generated drift lock covered ${lockedFiles} SKILL.md inputs`);
  console.log("Policy meaning and active-host behavior are not verified by this source check.");
  console.log("SKILL CHECK OK");
}

try {
  main();
} catch (error) {
  console.error(`SKILL CHECK FAIL: ${error.message}`);
  process.exitCode = 1;
}
