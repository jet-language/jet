#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { basename, dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "../../..");
const DEFAULT_MANIFEST = join(HERE, "manifest.json");
const RUNNER = join(ROOT, "scripts/agent/jet-env");
const CHECKER = join(ROOT, "proof/compiler/optimization/check.mjs");
const HARNESS = join(ROOT, "gauntlet/harness/run.mjs");
const TIMEOUT_MS = 120000;
const OBSERVATION_FIELDS = Object.freeze([
  "result",
  "typed_failure",
  "mutation",
  "input_consumption",
  "ordered_effects",
  "cleanup",
  "resource_premises",
  "schedule",
  "allowed_schedules",
]);
const VARIANT_IDS = Object.freeze([
  "control",
  "candidate",
  "invalid-fast",
  "legal-slow",
  "legal-measured-improvement",
]);
const CLASSIFICATIONS = new Set([
  "control",
  "candidate",
  "invalid_fast",
  "legal_slow",
  "legal_measured_improvement",
]);

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function fileSha256(file) {
  return existsSync(file) ? sha256(readFileSync(file)) : null;
}

function textSha256(text) {
  return sha256(Buffer.from(text, "utf8"));
}

function readJson(file) {
  return JSON.parse(readFileSync(file, "utf8"));
}

function writeJson(file, value) {
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);
}

function repoPath(relativePath, label) {
  if (typeof relativePath !== "string" || relativePath.length === 0 || isAbsolute(relativePath)) {
    throw new Error(`${label} must be a non-empty relative path`);
  }
  const resolved = resolve(ROOT, relativePath);
  const remainder = relative(ROOT, resolved);
  if (remainder === "" || remainder === ".." || remainder.startsWith(`..${"/"}`) || isAbsolute(remainder)) {
    throw new Error(`${label} escapes repository: ${relativePath}`);
  }
  return resolved;
}

function fixturePath(family, relativePath, label) {
  if (typeof relativePath !== "string" || relativePath.length === 0 || isAbsolute(relativePath)) {
    throw new Error(`${label} must be a non-empty relative path`);
  }
  const familyDir = repoPath(family.fixture_dir, `${family.id}.fixture_dir`);
  const resolved = resolve(familyDir, relativePath);
  const remainder = relative(familyDir, resolved);
  if (remainder === "" || remainder === ".." || remainder.startsWith(`..${"/"}`) || isAbsolute(remainder)) {
    throw new Error(`${label} escapes family fixture: ${relativePath}`);
  }
  return resolved;
}

function parseArgs(argv) {
  const options = {
    mode: "plan",
    manifest: DEFAULT_MANIFEST,
    output: null,
    raw: null,
    scratch: null,
    jetBin: null,
    runs: null,
    timeoutMs: TIMEOUT_MS,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "plan" || arg === "execute" || arg === "report") {
      options.mode = arg;
      continue;
    }
    if (arg === "--execute") {
      options.mode = "execute";
      continue;
    }
    if (["--manifest", "--output", "--raw", "--scratch", "--jet-bin", "--runs", "--timeout-ms"].includes(arg)) {
      if (index + 1 >= argv.length) throw new Error(`${arg} needs a value`);
      const value = argv[++index];
      if (arg === "--manifest") options.manifest = resolve(process.cwd(), value);
      if (arg === "--output") options.output = resolve(process.cwd(), value);
      if (arg === "--raw") options.raw = resolve(process.cwd(), value);
      if (arg === "--scratch") options.scratch = resolve(process.cwd(), value);
      if (arg === "--jet-bin") options.jetBin = resolve(process.cwd(), value);
      if (arg === "--runs") options.runs = Number.parseInt(value, 10);
      if (arg === "--timeout-ms") options.timeoutMs = Number.parseInt(value, 10);
      continue;
    }
    if (arg === "--help" || arg === "-h") {
      console.log("usage: optimization.mjs [plan|execute|report] [--manifest path] [--output path] [--raw path] [--scratch path] [--jet-bin path] [--runs n] [--timeout-ms n]");
      process.exit(0);
    }
    throw new Error(`unknown argument: ${arg}`);
  }
  if (options.runs !== null && (!Number.isInteger(options.runs) || options.runs < 1)) {
    throw new Error("--runs must be a positive integer");
  }
  if (!Number.isInteger(options.timeoutMs) || options.timeoutMs < 1) {
    throw new Error("--timeout-ms must be a positive integer");
  }
  if (options.mode === "report" && options.raw === null) {
    throw new Error("report mode requires --raw");
  }
  return options;
}

function validateVariant(family, variant) {
  if (!variant || typeof variant !== "object") throw new Error(`${family.id}: variant must be an object`);
  if (!VARIANT_IDS.includes(variant.id)) throw new Error(`${family.id}: unknown variant ${variant.id}`);
  if (typeof variant.source !== "string" || variant.source.length === 0) throw new Error(`${family.id}/${variant.id}: source missing`);
  if (!CLASSIFICATIONS.has(variant.classification)) throw new Error(`${family.id}/${variant.id}: classification missing`);
  const source = fixturePath(family, variant.source, `${family.id}/${variant.id}.source`);
  if (!existsSync(source)) throw new Error(`${family.id}/${variant.id}: missing source ${source}`);
}

function validateManifest(manifest, manifestPath) {
  if (manifest?.schema !== "jet.optimization-experiments.v1") throw new Error("unexpected optimization manifest schema");
  if (manifest?.version !== 1) throw new Error("unsupported optimization manifest version");
  if (manifest?.card !== "#2951") throw new Error("optimization manifest must remain owned by #2951");
  if (!Array.isArray(manifest?.families) || manifest.families.length !== 5) {
    throw new Error("optimization manifest must contain exactly five experiment families");
  }
  if (manifest?.contract?.path !== "proof/compiler/optimization/contract.json") {
    throw new Error("families must consume the canonical optimization contract");
  }
  for (const field of OBSERVATION_FIELDS) {
    if (!manifest.contract.observation_fields?.includes(field)) {
      throw new Error(`canonical observation missing from manifest: ${field}`);
    }
  }
  const familyIds = new Set();
  for (const family of manifest.families) {
    if (!family || typeof family !== "object" || typeof family.id !== "string" || family.id.length === 0) {
      throw new Error("family id missing");
    }
    if (familyIds.has(family.id)) throw new Error(`duplicate family id: ${family.id}`);
    familyIds.add(family.id);
    if (!/^#\d+$/.test(family.owner_card ?? "")) throw new Error(`${family.id}: exact owner card missing`);
    if (!Array.isArray(family.premises) || family.premises.length === 0) throw new Error(`${family.id}: premises missing`);
    if (typeof family.falsifier !== "string" || family.falsifier.length === 0) throw new Error(`${family.id}: falsifier missing`);
    for (const field of [...OBSERVATION_FIELDS, "identity"]) {
      if (typeof family.observations?.[field] !== "string" || family.observations[field].length === 0) {
        throw new Error(`${family.id}: observation missing ${field}`);
      }
    }
    if (!family.rule_record || family.rule_record.owner_card !== family.owner_card) {
      throw new Error(`${family.id}: rule record owner mismatch`);
    }
    const variants = Array.isArray(family.variants) ? family.variants : [];
    if (variants.length !== VARIANT_IDS.length || new Set(variants.map((variant) => variant.id)).size !== VARIANT_IDS.length) {
      throw new Error(`${family.id}: all five variants are required`);
    }
    for (const variant of variants) validateVariant(family, variant);
    const input = fixturePath(family, family.input, `${family.id}.input`);
    const expected = fixturePath(family, family.expected, `${family.id}.expected`);
    const requiredPeers = Array.isArray(family.required_peers) ? family.required_peers : [];
    const peerSources = family.peer_sources && typeof family.peer_sources === "object" ? family.peer_sources : {};
    if (requiredPeers.length === 0 || requiredPeers.some((language) => typeof language !== "string" || !peerSources[language])) {
      throw new Error(`${family.id}: required_peers and peer_sources must name every matched peer`);
    }
    const peerPaths = Object.fromEntries(requiredPeers.map((language) => [
      language,
      fixturePath(family, peerSources[language], `${family.id}.peer_sources.${language}`),
    ]));
    if (!existsSync(input) || !existsSync(expected) || Object.values(peerPaths).some((peerPath) => !existsSync(peerPath))) {
      throw new Error(`${family.id}: input, expected, and all required peer fixtures are required`);
    }
    try {
      JSON.parse(readFileSync(input, "utf8"));
    } catch (error) {
      throw new Error(`${family.id}: input is not JSON: ${error.message}`);
    }
  }
  if (manifestPath !== DEFAULT_MANIFEST && !existsSync(manifestPath)) throw new Error(`manifest does not exist: ${manifestPath}`);
}

function loadContext(manifestPath) {
  const manifest = readJson(manifestPath);
  validateManifest(manifest, manifestPath);
  const contractPath = repoPath(manifest.contract.path, "contract.path");
  const context = {
    manifest,
    manifestPath,
    manifestSha256: fileSha256(manifestPath),
    contractSha256: fileSha256(contractPath),
    checkerSha256: fileSha256(CHECKER),
    harnessSha256: fileSha256(HARNESS),
    families: [],
  };
  for (const family of manifest.families) {
    const inputPath = fixturePath(family, family.input, `${family.id}.input`);
    const expectedPath = fixturePath(family, family.expected, `${family.id}.expected`);
    const peerPaths = Object.fromEntries(family.required_peers.map((language) => [
      language,
      fixturePath(family, family.peer_sources[language], `${family.id}.peer_sources.${language}`),
    ]));
    const inputSha256 = fileSha256(inputPath);
    const expectedSha256 = fileSha256(expectedPath);
    const peerSha256ByLanguage = Object.fromEntries(Object.entries(peerPaths).map(([language, peerPath]) => [
      language,
      fileSha256(peerPath),
    ]));
    const peerPath = peerPaths.python;
    const peerSha256 = peerSha256ByLanguage.python;
    const variants = family.variants.map((variant) => {
      const sourcePath = fixturePath(family, variant.source, `${family.id}/${variant.id}.source`);
      return {
        ...variant,
        sourcePath,
        sourceSha256: fileSha256(sourcePath),
        identity: {
          manifest_sha256: context.manifestSha256,
          family: family.id,
          variant: variant.id,
          source_sha256: fileSha256(sourcePath),
          input_sha256: inputSha256,
          expected_sha256: expectedSha256,
          peer_sha256: peerSha256,
          peer_sha256_by_language: peerSha256ByLanguage,
          checker_sha256: context.checkerSha256,
          contract_sha256: context.contractSha256,
          compiler_sha256: null,
          target_identity: null,
          toolchain_identity: null,
          harness_sha256: context.harnessSha256,
        },
      };
    });
    context.families.push({
      ...family,
      inputPath,
      expectedPath,
      peerPath,
      peerPaths,
      inputSha256,
      expectedSha256,
      peerSha256,
      peerSha256ByLanguage,
      variants,
    });
  }
  return context;
}

function defaultJetBinary() {
  const debug = join(ROOT, "target/debug/jet");
  return existsSync(debug) ? debug : null;
}

function targetIdentity(jetBin) {
  const compilerPath = jetBin ?? defaultJetBinary();
  return {
    platform: process.platform,
    arch: process.arch,
    target: process.env.JET_TARGET ?? process.env.RUST_TARGET ?? null,
    compiler_path: compilerPath,
    compiler_sha256: compilerPath ? fileSha256(compilerPath) : null,
  };
}

function toolchainIdentity() {
  return {
    node: process.version,
    platform: process.platform,
    arch: process.arch,
    captured_at_execution: true,
  };
}

function runProcess(command, args, { cwd = ROOT, timeoutMs = TIMEOUT_MS, env = process.env } = {}) {
  const result = spawnSync(command, args, {
    cwd,
    env,
    encoding: "utf8",
    timeout: timeoutMs,
    maxBuffer: 16 * 1024 * 1024,
  });
  const timedOut = result.error?.code === "ETIMEDOUT";
  return {
    command: [command, ...args],
    cwd,
    status: timedOut ? "timeout" : (result.status === 0 ? "ok" : "failed"),
    exit_code: typeof result.status === "number" ? result.status : null,
    signal: result.signal ?? null,
    timed_out: timedOut,
    error: result.error ? String(result.error.message ?? result.error) : null,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
  };
}

function parseRecord(stdout) {
  const text = String(stdout ?? "").trim();
  if (text.length === 0) return null;
  try {
    return JSON.parse(text);
  } catch {
    const lines = text.split(/\r?\n/).reverse();
    for (const line of lines) {
      try {
        return JSON.parse(line);
      } catch {
        // Keep searching for a JSON record after launcher diagnostics.
      }
    }
    return null;
  }
}

function identityWithExecution(identity, execution, target, tools) {
  return {
    ...identity,
    compiler_sha256: target?.compiler_sha256 ?? null,
    target_identity: target ?? null,
    toolchain_identity: tools ?? null,
    execution_status: execution?.status ?? "unexecuted",
  };
}

function baseVariantRecord(family, variant) {
  return {
    family: family.id,
    variant: variant.id,
    classification: variant.classification,
    role: variant.role,
    source: relative(ROOT, variant.sourcePath),
    identity: { ...variant.identity },
    status: "unexecuted",
    observations: null,
    inspect: null,
    direct_run: null,
    harness_entry: `${family.id}--${variant.id}`,
    selection: {
      selected: false,
      owner_card: family.owner_card,
      decision: "awaiting-owner-choice",
    },
  };
}

function plan(context) {
  return {
    schema: "jet.optimization-plan.v1",
    status: "authored_unexecuted",
    card: context.manifest.card,
    manifest_sha256: context.manifestSha256,
    contract_sha256: context.contractSha256,
    checker_sha256: context.checkerSha256,
    harness_sha256: context.harnessSha256,
    bounded_execution: context.manifest.execution,
    families: context.families.map((family) => ({
      id: family.id,
      title: family.title,
      owner_card: family.owner_card,
      production_owner: family.production_owner,
      required_peers: family.required_peers,
      machine_domain: family.machine_domain,
      premises: family.premises,
      falsifier: family.falsifier,
      observations: family.observations,
      rule_record: family.rule_record,
      variants: family.variants.map((variant) => baseVariantRecord(family, variant)),
    })),
    proof_boundary: "Plan metadata and fixture hashes are not compiler proof, measurements, or owner adoption.",
  };
}

function harnessEntry(family, variant, entryDir) {
  const languages = ["jet", ...family.required_peers];
  const authoring = Object.fromEntries(languages.map((language) => [
    language,
    {
      author: "optimization-experiment",
      turns: 0,
      retries: 0,
      diagnosticsHit: [],
      notes: language === "jet"
        ? "Authored optimization fixture; execution is pending."
        : "Matched required peer fixture; execution is pending.",
    },
  ]));
  const metricApplicability = {};
  for (const language of ["python", "node"]) {
    metricApplicability[language] = {
      cold_build_seconds: { status: "not_applicable", basis: "no_compile_phase", reason: `${language} has no entry-local compile phase.`, evidence: `Harness invokes ${language === "python" ? "python3 main.py" : "node main.mjs"} directly.` },
      warm_build_seconds: { status: "not_applicable", basis: "no_compile_phase", reason: `${language} has no entry-local compile phase.`, evidence: `Harness invokes ${language === "python" ? "python3 main.py" : "node main.mjs"} directly.` },
      binary_bytes: { status: "not_applicable", basis: "no_compile_phase", reason: `${language} has no entry-local executable artifact.`, evidence: `Harness invokes ${language === "python" ? "python3 main.py" : "node main.mjs"} directly.` },
    };
  }
  return {
    name: `${family.id}--${variant.id}`,
    tier: "micro",
    cells: [family.matrix_cell],
    mode: family.mode ?? "batch",
    perf: true,
    languages,
    spec: {
      behavior: `${family.title}: ${variant.role}`,
      args: [],
      fixtureGen: null,
      expected: "expected.out",
    },
    metric_applicability: metricApplicability,
    authoring,
    optimization_experiment: {
      card: "#2951",
      family: family.id,
      variant: variant.id,
      owner_card: family.owner_card,
      source_identity: variant.identity,
      required_peers: family.required_peers,
      staging_root: entryDir,
    },
  };
}

function peerFileName(language) {
  return {
    python: "main.py",
    rust: "main.rs",
    c: "main.c",
    zig: "main.zig",
    node: "main.mjs",
  }[language];
}

function prepareHarnessEntries(context, scratch) {
  const entriesDir = join(scratch, "entries");
  mkdirSync(entriesDir, { recursive: true });
  for (const family of context.families) {
    for (const variant of family.variants) {
      const entryDir = join(entriesDir, `${family.id}--${variant.id}`);
      mkdirSync(join(entryDir, "jet"), { recursive: true });
      copyFileSync(variant.sourcePath, join(entryDir, "jet", "run.jet"));
      for (const language of family.required_peers) {
        mkdirSync(join(entryDir, language), { recursive: true });
        copyFileSync(family.peerPaths[language], join(entryDir, language, peerFileName(language)));
      }
      copyFileSync(family.expectedPath, join(entryDir, "expected.out"));
      writeJson(join(entryDir, "entry.json"), harnessEntry(family, variant, entryDir));
    }
  }
  return entriesDir;
}

function execute(context, options) {
  const timestamp = new Date().toISOString().replaceAll(/[^0-9A-Za-z]/g, "").slice(0, 24);
  const defaultScratch = join(process.env.XDG_CACHE_HOME ?? join(homedir(), ".cache"), "jet-optimization", "runs", `${timestamp}-${process.pid}`);
  const scratch = options.scratch ?? defaultScratch;
  mkdirSync(scratch, { recursive: true });
  const target = targetIdentity(options.jetBin);
  const tools = toolchainIdentity();
  const compilerCommand = target.compiler_path ?? "jet";
  const checkerProcess = runProcess(RUNNER, ["full", "node", CHECKER, "--json"], { timeoutMs: options.timeoutMs });
  const checkerRecord = parseRecord(checkerProcess.stdout);
  const variants = [];
  for (const family of context.families) {
    for (const variant of family.variants) {
      const inspectProcess = runProcess(RUNNER, ["full", compilerCommand, "inspect", "compiler", "check", variant.sourcePath], { timeoutMs: options.timeoutMs });
      const runProcessRecord = runProcess(RUNNER, ["full", compilerCommand, "run", variant.sourcePath], { timeoutMs: options.timeoutMs });
      const expected = readFileSync(family.expectedPath);
      const output = Buffer.from(runProcessRecord.stdout, "utf8");
      const variantIdentity = identityWithExecution(variant.identity, runProcessRecord, target, tools);
      variants.push({
        ...baseVariantRecord(family, variant),
        identity: variantIdentity,
        status: runProcessRecord.status,
        inspect: {
          ...inspectProcess,
          record: parseRecord(inspectProcess.stdout),
        },
        direct_run: {
          ...runProcessRecord,
          stdout_sha256: sha256(output),
          expected_sha256: sha256(expected),
          output_matches_expected: runProcessRecord.status === "ok" && output.equals(expected),
        },
      });
    }
  }
  const entriesDir = prepareHarnessEntries(context, scratch);
  const harnessArgs = ["full", "node", HARNESS, "--entries-dir", entriesDir];
  if (target.compiler_path !== null) harnessArgs.push("--jet-bin", target.compiler_path);
  const harnessProcess = runProcess(RUNNER, harnessArgs, { timeoutMs: options.timeoutMs });
  const resultMatch = harnessProcess.stdout.match(/^results\t(.+)$/m);
  const resultPath = resultMatch ? resultMatch[1].trim() : null;
  const harnessReport = resultPath && existsSync(resultPath) ? readJson(resultPath) : null;
  const raw = {
    schema: "jet.optimization-raw.v1",
    status: "executed",
    generated: new Date().toISOString(),
    card: context.manifest.card,
    manifest_sha256: context.manifestSha256,
    contract_sha256: context.contractSha256,
    checker_sha256: context.checkerSha256,
    harness_sha256: context.harnessSha256,
    target_identity: target,
    toolchain_identity: tools,
    timeout_ms: options.timeoutMs,
    scratch,
    checker: { ...checkerProcess, record: checkerRecord },
    variants,
    gauntlet: {
      process: harnessProcess,
      result_path: resultPath,
      result_sha256: resultPath ? fileSha256(resultPath) : null,
      report: harnessReport,
    },
    proof_boundary: "Execution records are evidence for later review; bounded runs, checker blocked status, missing modes, and unavailable rows remain unverified.",
  };
  return raw;
}

function inspectStatus(record) {
  if (!record || typeof record !== "object") return "unverified";
  const status = record.status ?? record.compiler?.status ?? record.result?.status ?? null;
  if (["ok", "passed", "valid", "complete"].includes(status)) return "valid_record";
  if (["blocked", "unverified", "unknown", "failed", "error"].includes(status)) return status;
  return "unverified";
}

function rowFor(harnessReport, entryName, language) {
  const entry = harnessReport?.entries?.find((item) => item?.entry?.name === entryName);
  if (!entry) return { entry: null, row: null, comparison: null };
  return {
    entry,
    row: entry.rows?.[language] ?? null,
    comparison: entry.comparisons?.[language] ?? null,
  };
}

function requiredHarnessReady(harnessReport, entryName, requiredPeers) {
  const record = rowFor(harnessReport, entryName, "jet");
  if (record.entry === null || record.row?.status !== "ok") return false;
  for (const language of requiredPeers) {
    const peer = rowFor(harnessReport, entryName, language);
    if (peer.entry === null || peer.row?.status !== "ok") return false;
  }
  const tiers = record.entry.jet_tiers ?? {};
  return ["aot", "run"].every((tier) => tiers[tier]?.status === "ok");
}

function classifyVariant(family, variant, raw, controlVariant) {
  const rawVariant = raw.variants?.find((item) => item.family === family.id && item.variant === variant.id);
  const base = {
    family: family.id,
    variant: variant.id,
    owner_card: family.owner_card,
    expected_classification: variant.classification,
    observed_classification: "unverified",
    legality: "unverified",
    profitability: "unverified",
    selected: false,
    reason: "required raw evidence is unavailable",
    identity: rawVariant?.identity ?? variant.identity,
    direct_run: rawVariant?.direct_run ?? null,
    inspect_status: inspectStatus(rawVariant?.inspect?.record),
    harness: null,
  };
  if (!rawVariant) return base;
  const direct = rawVariant.direct_run;
  if (variant.classification === "invalid_fast") {
    if (direct?.status === "ok" && direct.output_matches_expected === false) {
      return { ...base, observed_classification: "invalid_fast", legality: "rejected", reason: "negative control changed byte-exact output" };
    }
    return { ...base, reason: "negative control did not produce a complete observable mismatch" };
  }
  if (variant.classification === "control") {
    return {
      ...base,
      observed_classification: direct?.status === "ok" && direct.output_matches_expected === true ? "control_observed" : "unverified",
      legality: direct?.status === "ok" && direct.output_matches_expected === true ? "control_valid" : "unverified",
      reason: direct?.status === "ok" && direct.output_matches_expected === true ? "control output matched expected bytes" : "control output is unavailable or mismatched",
    };
  }
  const entryName = `${family.id}--${variant.id}`;
  const controlName = `${family.id}--${controlVariant.id}`;
  const candidateHarness = rowFor(raw.gauntlet?.report, entryName, "jet");
  const peerRows = Object.fromEntries(family.required_peers.map((language) => [
    language,
    rowFor(raw.gauntlet?.report, entryName, language),
  ]));
  const controlHarness = rowFor(raw.gauntlet?.report, controlName, "jet");
  const ready = requiredHarnessReady(raw.gauntlet?.report, entryName, family.required_peers)
    && requiredHarnessReady(raw.gauntlet?.report, controlName, family.required_peers);
  const candidateRuntime = candidateHarness.row?.metrics?.runtime_wall_seconds;
  const controlRuntime = controlHarness.row?.metrics?.runtime_wall_seconds;
  const peerReady = family.required_peers.every((language) => peerRows[language].row?.status === "ok");
  const semanticReady = direct?.status === "ok" && direct.output_matches_expected === true;
  const legalReady = semanticReady && inspectStatus(rawVariant.inspect?.record) === "valid_record";
  const harness = {
    candidate_entry: entryName,
    control_entry: controlName,
    candidate_row: candidateHarness.row,
    peer_rows: Object.fromEntries(Object.entries(peerRows).map(([language, value]) => [language, value.row])),
    candidate_comparison: candidateHarness.comparison,
    runtime_wall_seconds: { candidate: candidateRuntime ?? null, control: controlRuntime ?? null },
    required_tiers_ready: ready,
    required_peers: family.required_peers,
    peer_ready: peerReady,
  };
  if (!legalReady || !ready || !peerReady || !Number.isFinite(candidateRuntime) || !Number.isFinite(controlRuntime)) {
    return { ...base, harness, reason: "legality, required modes, peer, or same-job cost evidence is unavailable" };
  }
  if (candidateRuntime < controlRuntime) {
    return {
      ...base,
      observed_classification: "legal_measured_improvement",
      legality: "legal",
      profitability: "measured_improvement",
      reason: "same-job candidate runtime is lower than the unchanged control",
      harness,
    };
  }
  if (candidateRuntime > controlRuntime) {
    return {
      ...base,
      observed_classification: "legal_slow",
      legality: "legal",
      profitability: "slower_than_control",
      reason: "same-job candidate runtime is higher than the unchanged control",
      harness,
    };
  }
  return {
    ...base,
    observed_classification: "legal_no_improvement",
    legality: "legal",
    profitability: "parity_no_improvement",
    reason: "same-job runtime is equal to the unchanged control",
    harness,
  };
}

function report(context, raw) {
  if (raw?.schema !== "jet.optimization-raw.v1") throw new Error("raw record has unexpected schema");
  if (raw.manifest_sha256 !== context.manifestSha256) throw new Error("raw record does not match the current manifest");
  const families = context.families.map((family) => {
    const controlVariant = family.variants.find((variant) => variant.id === "control");
    const variants = family.variants.map((variant) => classifyVariant(family, variant, raw, controlVariant));
    const selected = variants.find((variant) => variant.observed_classification === "legal_measured_improvement") ?? null;
    return {
      id: family.id,
      title: family.title,
      owner_card: family.owner_card,
      production_owner: family.production_owner,
      observations: family.observations,
      premises: family.premises,
      falsifier: family.falsifier,
      variants,
      rule_record: {
        ...family.rule_record,
        observed_selected: selected?.variant ?? null,
        selected: false,
        rejected: variants.filter((variant) => ["invalid_fast", "legal_slow"].includes(variant.observed_classification)).map((variant) => variant.variant),
        decision: selected ? "owner-choice-required" : "no-adoption",
        owner_card: family.owner_card,
      },
    };
  });
  const unverified = families.flatMap((family) => family.variants.filter((variant) => variant.observed_classification === "unverified").map((variant) => `${family.id}/${variant.variant}`));
  return {
    schema: "jet.optimization-report.v1",
    status: unverified.length === 0 ? "complete_evidence_pending_owner_choice" : "unverified",
    card: context.manifest.card,
    manifest_sha256: context.manifestSha256,
    raw_sha256: textSha256(JSON.stringify(raw)),
    contract_sha256: context.contractSha256,
    checker_sha256: context.checkerSha256,
    harness_sha256: context.harnessSha256,
    target_identity: raw.target_identity ?? null,
    toolchain_identity: raw.toolchain_identity ?? null,
    checker: raw.checker ?? null,
    gauntlet: raw.gauntlet ?? null,
    families,
    unverified,
    adoption: {
      selected_rules: [],
      owner_choice_required: true,
      exact_owner_cards: families.map((family) => ({ family: family.id, card: family.owner_card })),
      no_production_change: true,
    },
    proof_boundary: "A report classifies only observed same-job records. It is not universal optimization proof, release qualification, or automatic adoption.",
  };
}

function output(value, outputPath) {
  const serialized = `${JSON.stringify(value, null, 2)}\n`;
  if (outputPath) {
    writeFileSync(outputPath, serialized);
    console.log(`output\t${outputPath}`);
  } else {
    process.stdout.write(serialized);
  }
}


function main() {
  const options = parseArgs(process.argv.slice(2));
  const context = loadContext(options.manifest);
  if (options.mode === "plan") {
    output(plan(context), options.output);
    return;
  }
  if (options.mode === "execute") {
    output(execute(context, options), options.output);
    return;
  }
  const raw = readJson(options.raw);
  output(report(context, raw), options.output);
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  try {
    main();
  } catch (error) {
    console.error(`optimization: ${error.message}`);
    process.exitCode = 1;
  }
}
