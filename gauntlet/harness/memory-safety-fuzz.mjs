import { createHash } from "node:crypto";
import { promises as fs } from "node:fs";
import path from "node:path";

const CASE_KINDS = ["valid", "boundary", "oob", "use-after-free", "wrong-output"];
const CASE_KIND_IDS = Object.freeze(Object.fromEntries(CASE_KINDS.map((name, id) => [name, id])));
const DEFAULT_RECORD_METADATA_BYTES = 4;
const DEFAULT_TARGET_COMMIT = "working-tree";
const DEFAULT_OUTPUT_LIMIT = 8 * 1024;

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function xorshift32(value) {
  let state = value >>> 0;
  state ^= state << 13;
  state ^= state >>> 17;
  state ^= state << 5;
  return state >>> 0;
}

function corpusShape(spec = {}) {
  const seed = Number(spec.seed);
  const caseCount = Number(spec.case_count ?? spec.caseCount);
  const bytesPerCase = Number(spec.bytes_per_case ?? spec.bytesPerCase);
  const metadataBytes = Number(spec.metadata_bytes ?? spec.metadataBytes ?? DEFAULT_RECORD_METADATA_BYTES);
  const payloadBytes = Number(spec.payload_bytes ?? spec.payloadBytes ?? bytesPerCase - metadataBytes);
  const generator = String(spec.generator ?? "xorshift32-v1");
  if (!Number.isInteger(seed) || seed < 0 || seed > 0xffff_ffff ||
    !Number.isInteger(caseCount) || caseCount < 1 ||
    !Number.isInteger(bytesPerCase) || bytesPerCase < 8 || bytesPerCase > 255 ||
    !Number.isInteger(metadataBytes) || metadataBytes < DEFAULT_RECORD_METADATA_BYTES || metadataBytes >= bytesPerCase ||
    !Number.isInteger(payloadBytes) || payloadBytes < 1 || payloadBytes !== bytesPerCase - metadataBytes ||
    generator !== "xorshift32-v1") {
    throw new Error("memory-safety corpus must declare a bounded xorshift32 record shape");
  }
  return { seed: seed >>> 0, caseCount, bytesPerCase, metadataBytes, payloadBytes, generator };
}

function generateCorpus(spec) {
  const shape = corpusShape(spec);
  const data = Buffer.alloc(shape.caseCount * shape.bytesPerCase);
  let state = shape.seed;
  for (let offset = 0; offset < data.length; offset += 1) {
    state = xorshift32(state);
    data[offset] = state & 0xff;
  }

  for (let index = 0; index < shape.caseCount; index += 1) {
    const offset = index * shape.bytesPerCase;
    const kind = index % CASE_KINDS.length;
    const randomByte = (relative) => data[offset + shape.metadataBytes + (relative % shape.payloadBytes)];
    const outOfBoundsRange = Math.max(1, 255 - shape.payloadBytes);
    let declaredLength;
    let requestedIndex;
    switch (kind) {
      case CASE_KIND_IDS.valid:
        declaredLength = 1 + (randomByte(0) % shape.payloadBytes);
        requestedIndex = randomByte(1) % declaredLength;
        break;
      case CASE_KIND_IDS.boundary:
        declaredLength = index % 2 === 0 ? 0 : shape.payloadBytes;
        requestedIndex = index % 2 === 0 ? 0 : shape.payloadBytes - 1;
        break;
      case CASE_KIND_IDS.oob:
        declaredLength = shape.payloadBytes + 1 + (randomByte(2) % outOfBoundsRange);
        requestedIndex = shape.payloadBytes + (randomByte(3) % outOfBoundsRange);
        break;
      case CASE_KIND_IDS["use-after-free"]:
        declaredLength = 1 + (randomByte(4) % shape.payloadBytes);
        requestedIndex = randomByte(5) % declaredLength;
        break;
      case CASE_KIND_IDS["wrong-output"]:
        declaredLength = 1 + (randomByte(6) % shape.payloadBytes);
        requestedIndex = randomByte(7) % declaredLength;
        break;
      default:
        throw new Error(`unknown generated corpus case ${kind}`);
    }
    data[offset] = kind;
    data[offset + 1] = declaredLength;
    data[offset + 2] = requestedIndex;
    data[offset + 3] = 0xa0 + kind;
  }
  return { ...shape, bytes: data.length, sha256: sha256(data), data };
}

function recordSummary(data, shape) {
  if (data.length !== shape.caseCount * shape.bytesPerCase) {
    throw new Error(`memory-safety corpus has ${data.length} bytes; expected ${shape.caseCount * shape.bytesPerCase}`);
  }
  const counts = Object.fromEntries(CASE_KINDS.map((kind) => [kind, 0]));
  const records = [];
  let checksum = 0;
  let semantic = 0;
  for (let offset = 0; offset < data.length; offset += 1) {
    checksum = (checksum + data[offset]) >>> 0;
  }
  for (let index = 0; index < shape.caseCount; index += 1) {
    const offset = index * shape.bytesPerCase;
    const kindId = data[offset];
    const kind = CASE_KINDS[kindId];
    if (!kind) throw new Error(`memory-safety corpus record ${index} has unknown kind ${kindId}`);
    counts[kind] += 1;
    const declaredLength = data[offset + 1];
    const requestedIndex = data[offset + 2];
    const boundedLength = Math.min(declaredLength, shape.payloadBytes);
    const safeIndex = Math.min(requestedIndex, shape.payloadBytes);
    let value = 0;
    if (kindId === CASE_KIND_IDS.valid || kindId === CASE_KIND_IDS.oob) {
      for (let payloadIndex = 0; payloadIndex < boundedLength; payloadIndex += 1) {
        value = (value + data[offset + shape.metadataBytes + payloadIndex]) >>> 0;
      }
    } else if (requestedIndex < shape.payloadBytes) {
      value = data[offset + shape.metadataBytes + requestedIndex];
      if (kindId === CASE_KIND_IDS["wrong-output"]) value ^= 0xa5;
    }
    semantic = (semantic + value + ((kindId + 1) * 257) + boundedLength + safeIndex) >>> 0;
    records.push({
      index,
      kind,
      kind_id: kindId,
      declared_length: declaredLength,
      requested_index: requestedIndex,
      bounded_length: boundedLength,
      safe_index: safeIndex,
      value,
      offset,
    });
  }
  return { counts, checksum, semantic, records };
}

function expectedOutput(summary, shape) {
  const counts = summary.counts;
  return [
    `cases ${shape.caseCount}`,
    `valid ${counts.valid}`,
    `boundary ${counts.boundary}`,
    `oob ${counts.oob}`,
    `use_after_free ${counts["use-after-free"]}`,
    `wrong_output ${counts["wrong-output"]}`,
    `bytes ${shape.bytes}`,
    `checksum ${summary.checksum}`,
    `semantic ${summary.semantic}`,
  ].join(" ") + "\n";
}

function asBuffer(value) {
  if (Buffer.isBuffer(value)) return value;
  if (value instanceof Uint8Array) return Buffer.from(value);
  if (value == null) return Buffer.alloc(0);
  return Buffer.from(String(value), "utf8");
}

function limitedText(value, limit) {
  return asBuffer(value).toString("utf8").slice(0, limit);
}

function processEvidence(result, limit) {
  const raw = result?.raw || result;
  const process = result?.process || result || {};
  const stdoutBytes = asBuffer(raw?.stdout ?? process.stdout);
  const stderrBytes = asBuffer(raw?.stderr ?? process.stderr);
  return {
    code: Number.isInteger(process.code) ? process.code : null,
    signal: process.signal ?? null,
    timed_out: Boolean(process.timed_out ?? process.timedOut),
    resource_exceeded: process.resource_exceeded ?? process.resourceExceeded ?? null,
    stdout: limitedText(stdoutBytes, limit),
    stderr: limitedText(stderrBytes, limit),
    stdout_bytes: stdoutBytes.toString("base64"),
    stderr_bytes: stderrBytes.toString("base64"),
  };
}

function normalizeFindingText(text, context, limit) {
  let normalized = String(text ?? "");
  for (const root of [context.runDir, context.repoDir].filter(Boolean)) normalized = normalized.replaceAll(String(root), "<path>");
  return normalized
    .replace(/0x[0-9a-f]+/gi, "0xADDRESS")
    .replace(/:\d+(?::\d+)?/g, ":LINE")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, limit);
}

function evidenceHits(evidence, output) {
  const lines = String(output).replace(/\u001b\[[0-?]*[ -/]*[@-~]/g, "").split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  const hits = [];
  for (const pattern of evidence?.patterns ?? []) {
    for (const line of lines) if (line.includes(pattern)) hits.push({ pattern, line });
  }
  return hits;
}

function kindForEvidence(hit) {
  return /addresssanitizer|undefinedbehavior|sanitizer|use-after-free|out.of.bounds|heap-buffer-overflow|stack-buffer-overflow/i.test(`${hit.pattern} ${hit.line}`)
    ? "sanitizer" : "crash";
}

function classifyProcess({ phase, process, expected, evidence, context, outputLimit }) {
  const output = `${process.stdout}\n${process.stderr}`;
  const hits = evidenceHits(evidence, output);
  const findings = [];
  if (process.timed_out || process.resource_exceeded) {
    const detail = process.resource_exceeded || `wall timeout after ${context.wallTimeoutMs ?? "declared"}ms`;
    findings.push({ kind: "timeout", phase, detail, pattern: "timeout", excerpts: [detail] });
  }
  for (const hit of hits) {
    findings.push({ kind: kindForEvidence(hit), phase, detail: hit.line, pattern: hit.pattern, excerpts: [hit.line] });
  }
  if (!process.timed_out && !process.resource_exceeded && !hits.length && (process.signal || (process.code !== null && process.code !== 0))) {
    findings.push({
      kind: "crash",
      phase,
      detail: process.signal ? `signal ${process.signal}` : `exit ${process.code}`,
      pattern: "non-zero exit",
      excerpts: [process.stderr || process.stdout || `exit ${process.code}`],
    });
  }
  if (phase === "run" && !process.timed_out && !process.resource_exceeded && process.code === 0 && process.stdout !== expected) {
    findings.push({
      kind: "wrong-output",
      phase,
      detail: `expected ${sha256(expected)} but received ${sha256(process.stdout)}`,
      pattern: "oracle mismatch",
      excerpts: [process.stdout.slice(0, outputLimit)],
    });
  }
  return findings;
}

function minimizedCase(corpus, summary, kind) {
  const preferred = kind === "wrong-output"
    ? [CASE_KIND_IDS["wrong-output"]]
    : kind === "sanitizer" || kind === "crash"
      ? [CASE_KIND_IDS.oob, CASE_KIND_IDS["use-after-free"], CASE_KIND_IDS.boundary]
      : [CASE_KIND_IDS.boundary, CASE_KIND_IDS.oob, CASE_KIND_IDS["use-after-free"]];
  const record = preferred.map((id) => summary.records.find((candidate) => candidate.kind_id === id)).find(Boolean) || summary.records[0];
  const bytes = corpus.data.subarray(record.offset, record.offset + corpus.bytesPerCase);
  return {
    index: record.index,
    kind: record.kind,
    byte_count: bytes.length,
    sha256: sha256(bytes),
    encoding: "base64",
    bytes: bytes.toString("base64"),
    declared_length: record.declared_length,
    requested_index: record.requested_index,
  };
}

function hardeningKey(kind) {
  return [
    "hardening:v1",
    `seam=${encodeURIComponent("unclassified.semantic-primitive")}`,
    `relation=${encodeURIComponent(kind)}`,
    `tiers=${encodeURIComponent("all-rails")}`,
    `partition=${encodeURIComponent("memory-safety-fuzz")}`,
  ].join("|");
}

function hardeningPayload({ finding, runner, process, corpus, summary, expected, context, command }) {
  const minimized = minimizedCase(corpus, summary, finding.kind);
  const dedupKey = hardeningKey(finding.kind);
  const findingId = `HF-${sha256(dedupKey).slice(0, 16)}`;
  const actual = finding.kind === "wrong-output" ? finding.detail : `${finding.kind}: ${finding.detail}`;
  const evidence = {
    source: `fuzz-input.bin#case-${minimized.index};base64:${minimized.bytes}`,
    commands: [command],
    expectedRelation: expected.trimEnd(),
    actualRelation: actual,
    seed: String(corpus.seed),
    targetCommit: String(context.targetCommit ?? context.target_commit ?? DEFAULT_TARGET_COMMIT),
    classification: finding.kind,
    stdoutBytes: process.stdout_bytes,
    stderrBytes: process.stderr_bytes,
    exit: process.code,
    signal: process.signal,
    timeout: process.timed_out || Boolean(process.resource_exceeded),
    normalization: ["ANSI control sequences", "absolute paths", "addresses", "line numbers"],
  };
  return {
    hardeningDedupKey: dedupKey,
    hardeningSeam: "unclassified.semantic-primitive",
    hardeningRelation: finding.kind,
    hardeningWrongTierMask: ["all-rails"],
    hardeningInputPartition: "memory-safety-fuzz",
    hardeningFindingId: findingId,
    hardeningEvidence: evidence,
    finding_id: findingId,
    dedup_key: dedupKey,
    minimized_case: minimized,
    expected_relation: evidence.expectedRelation,
    actual_relation: evidence.actualRelation,
    seed: corpus.seed,
    bytes: corpus.bytes,
    toolchain: runner.tools,
    classification: finding.kind,
    correctness_finding: true,
    performance_exclusion: false,
  };
}

function enrichFindings(findings, kindContext) {
  return findings.map((finding) => ({
    ...finding,
    seed: kindContext.corpus.seed,
    bytes: kindContext.corpus.bytes,
    toolchain: kindContext.tools,
    minimized_case: minimizedCase(kindContext.corpus, kindContext.summary, finding.kind),
    classification: finding.kind,
    correctness_finding: true,
    performance_exclusion: false,
  }));
}

function recordFinding(receipts, finding, runner, process, corpus, summary, expected, context, command) {
  const signature = finding.kind === "wrong-output"
    ? `${finding.kind}:${sha256(expected)}:${sha256(process.stdout)}`
    : `${finding.kind}:${normalizeFindingText(finding.detail, context, 500)}`;
  const dedupeKey = sha256(`memory_safety_fuzz\0${finding.kind}\0${signature}`);
  let receipt = receipts.get(dedupeKey);
  if (!receipt) {
    const towerPayload = hardeningPayload({ finding, runner, process, corpus, summary, expected, context, command });
    receipt = {
      id: dedupeKey.slice(0, 16),
      dedupe_key: dedupeKey,
      axis: "memory_safety_fuzz",
      kind: finding.kind,
      classification: "correctness",
      severity: finding.kind === "wrong-output" ? "P0" : "P1",
      signature,
      rails: [],
      occurrences: [],
      tower_tracking: {
        status: "pending",
        transport: "tower-cli",
        command: ["node", "plugins/tower/tower.mjs", "card", "add", "--file", "-"],
        dedup_key: towerPayload.hardeningDedupKey,
        payload: towerPayload,
      },
      correctness_finding: true,
      performance_exclusion: false,
    };
    receipts.set(dedupeKey, receipt);
  }
  if (!receipt.rails.includes(runner.id)) receipt.rails.push(runner.id);
  receipt.rails.sort((left, right) => left.localeCompare(right));
  receipt.occurrences.push({
    runner: runner.id,
    language: runner.language,
    phase: finding.phase,
    pattern: finding.pattern,
    excerpts: finding.excerpts,
    seed: corpus.seed,
    bytes: corpus.bytes,
    toolchain: runner.tools,
    process,
    minimized_case: receipt.tower_tracking.payload.minimized_case,
    classification: finding.kind,
    correctness_finding: true,
    performance_exclusion: false,
  });
  return receipt;
}

function requireContext(context) {
  const names = ["stageAxisFiles", "axisStagePath", "probeAxisTools", "axisCommand", "runMemoryCommand"];
  for (const name of names) if (typeof context[name] !== "function") throw new Error(`memory-safety axis adapter needs ${name} callback`);
}

async function runRunner(axis, runner, axisDir, corpus, summary, expected, context) {
  const runnerDir = path.join(axisDir, runner.id.replaceAll(/[^A-Za-z0-9_.-]/g, "_"));
  await fs.mkdir(runnerDir, { recursive: true });
  const files = await context.stageAxisFiles(runnerDir, runner.files);
  const inputPath = context.axisStagePath(runnerDir, axis.corpus.path);
  await fs.mkdir(path.dirname(inputPath), { recursive: true });
  await fs.writeFile(inputPath, corpus.data);
  const copiedSha = await (context.fileSha256 ? context.fileSha256(inputPath) : sha256(await fs.readFile(inputPath)));
  const probes = await context.probeAxisTools(runnerDir, runner.tools, context.jetBin);
  const result = {
    id: runner.id,
    language: runner.language,
    tools: probes,
    source_files: files,
    corpus: {
      path: axis.corpus.path,
      seed: corpus.seed,
      case_count: corpus.caseCount,
      bytes_per_case: corpus.bytesPerCase,
      bytes: corpus.bytes,
      generated_sha256: corpus.sha256,
      copied_sha256: copiedSha,
      matches_generator: copiedSha === corpus.sha256,
      case_kinds: summary.counts,
      valid_case_count: summary.counts.valid,
      adversarial_case_count: corpus.caseCount - summary.counts.valid,
    },
    resource_budget: axis.budget,
    resource_enforcement: {
      cpu: "RLIMIT_CPU via ulimit -t",
      memory: "process-tree VmRSS monitor",
      wall: "harness process-group timeout",
    },
    evidence: runner.evidence,
    declared_compile: runner.compile ?? null,
    declared_run: runner.run,
    status: "unmeasured",
    findings: [],
  };
  const unavailable = probes.find((probe) => probe.status === "unavailable");
  const probeFailure = probes.find((probe) => probe.status === "probe_failed");
  if (unavailable) {
    result.status = "unavailable";
    result.reason = unavailable.reason;
    return result;
  }
  if (probeFailure) {
    result.status = "failed";
    result.reason = probeFailure.reason;
    return result;
  }

  const localFindings = [];
  let compile = null;
  let run = null;
  try {
    if (runner.compile) {
      const compileCommand = context.axisCommand(runner.compile, { jet_bin: context.jetBin });
      compile = await context.runMemoryCommand(runnerDir, compileCommand, axis.budget);
      result.compile = compile;
      delete result.compile.raw;
      const compileEvidence = processEvidence(compile, context.outputLimit ?? DEFAULT_OUTPUT_LIMIT);
      result.compile.process = compileEvidence;
      const compileFindings = classifyProcess({
        phase: "compile",
        process: compileEvidence,
        expected,
        evidence: runner.evidence,
        context: { ...context, wallTimeoutMs: axis.budget.wall_timeout_ms },
        outputLimit: context.outputLimit ?? DEFAULT_OUTPUT_LIMIT,
      });
      localFindings.push(...compileFindings);
      if (compileEvidence.code !== 0 || compileEvidence.timed_out || compileEvidence.resource_exceeded) {
        result.findings = enrichFindings(localFindings, { corpus, summary, tools: probes });
        result.status = localFindings.length ? "finding" : "failed";
        result.reason = localFindings.length ? undefined : `compile exited ${compileEvidence.code}`;
        return result;
      }
    }

    const runCommand = context.axisCommand(runner.run, { jet_bin: context.jetBin });
    run = await context.runMemoryCommand(runnerDir, runCommand, axis.budget);
    result.run = run;
    delete result.run.raw;
    const runEvidence = processEvidence(run, context.outputLimit ?? DEFAULT_OUTPUT_LIMIT);
    result.run.process = runEvidence;
    result.toolchain = probes;
    result.output = {
      expected,
      actual_stdout: runEvidence.stdout,
      exact_stdout: runEvidence.stdout === expected,
      expected_sha256: sha256(expected),
      actual_sha256: sha256(runEvidence.stdout),
    };
    localFindings.push(...classifyProcess({
      phase: "run",
      process: runEvidence,
      expected,
      evidence: runner.evidence,
      context: { ...context, wallTimeoutMs: axis.budget.wall_timeout_ms },
      outputLimit: context.outputLimit ?? DEFAULT_OUTPUT_LIMIT,
    }));
    result.input_unchanged = await (context.fileSha256 ? context.fileSha256(inputPath) : sha256(await fs.readFile(inputPath))) === copiedSha;
    if (!result.input_unchanged) {
      localFindings.push({
        kind: "wrong-output",
        phase: "run",
        detail: "runner modified the shared fuzz input",
        pattern: "input integrity",
        excerpts: ["runner modified the shared fuzz input"],
      });
    }
    result.findings = enrichFindings(localFindings, { corpus, summary, tools: probes });
    if (result.findings.length) {
      result.status = "finding";
    } else if (runEvidence.resource_exceeded) {
      result.status = "failed";
      result.reason = runEvidence.resource_exceeded;
    } else if (runEvidence.timed_out) {
      result.status = "failed";
      result.reason = `run exceeded ${axis.budget.wall_timeout_ms}ms wall budget`;
    } else if (runEvidence.code !== 0) {
      result.status = "failed";
      result.reason = `run exited ${runEvidence.code}`;
    } else if (!result.output.exact_stdout) {
      result.status = "failed";
      result.reason = "run stdout did not match the declared oracle";
    } else if (!result.input_unchanged) {
      result.status = "failed";
      result.reason = "runner modified the shared fuzz input";
    } else {
      result.status = "complete";
    }
  } catch (error) {
    result.status = "failed";
    result.reason = `${run ? "run" : "compile"} execution failed: ${error.message}`;
    result.findings = enrichFindings(localFindings, { corpus, summary, tools: probes });
  }
  return result;
}

export async function runMemorySafetyFuzzAxis(axis, context = {}) {
  requireContext(context);
  if (!axis?.corpus || !Array.isArray(axis.runners) || axis.runners.length === 0) throw new Error("memory-safety axis has no corpus or runners");
  const axisDir = path.join(context.runDir, "axes", "memory-safety-fuzz");
  await fs.mkdir(axisDir, { recursive: true });
  const corpus = generateCorpus(axis.corpus);
  const summary = recordSummary(corpus.data, corpus);
  const expected = expectedOutput(summary, corpus);
  const corpusPath = context.axisStagePath(axisDir, axis.corpus.path);
  await fs.mkdir(path.dirname(corpusPath), { recursive: true });
  await fs.writeFile(corpusPath, corpus.data);
  const receipts = new Map();
  const runners = [];
  for (const runner of axis.runners) {
    try {
      const result = await runRunner(axis, runner, axisDir, corpus, summary, expected, context);
      runners.push(result);
      const process = result.run?.process || result.compile?.process || {
        code: null,
        signal: null,
        timed_out: false,
        resource_exceeded: null,
        stdout: "",
        stderr: "",
        stdout_bytes: "",
        stderr_bytes: "",
      };
      const command = result.run?.command || result.compile?.command || result.declared_run || result.declared_compile || [];
      for (const finding of result.findings ?? []) recordFinding(receipts, finding, runner, process, corpus, summary, expected, context, command);
    } catch (error) {
      runners.push({ id: runner.id, language: runner.language, status: "failed", reason: `runner setup failed: ${error.message}`, findings: [] });
    }
  }
  const measured = runners.length === axis.runners.length && runners.every((runner) => ["complete", "finding"].includes(runner.status));
  const findings = [...receipts.values()].sort((left, right) => left.id.localeCompare(right.id));
  const findingOccurrences = findings.reduce((total, receipt) => total + receipt.occurrences.length, 0);
  const blockers = runners.filter((runner) => !["complete", "finding"].includes(runner.status))
    .map((runner) => `${runner.id}: ${runner.reason ?? `status ${runner.status}`}`);
  if (findings.length) blockers.push(`${findings.length} deduplicated finding receipt(s) require Tower CLI tracking`);
  return {
    id: "memory_safety_fuzz",
    required: axis.status === "required",
    status: measured ? "complete" : "incomplete",
    contract: axis,
    schema: axis.schema,
    metric: axis.metric,
    corpus: {
      path: axis.corpus.path,
      generator: corpus.generator,
      seed: corpus.seed,
      case_count: corpus.caseCount,
      bytes_per_case: corpus.bytesPerCase,
      record_metadata_bytes: corpus.metadataBytes,
      payload_bytes: corpus.payloadBytes,
      bytes: corpus.bytes,
      sha256: corpus.sha256,
      case_kinds: summary.counts,
      valid_case_count: summary.counts.valid,
      adversarial_case_count: corpus.caseCount - summary.counts.valid,
      format: "jet-memory-safety-case-v1",
    },
    oracle: {
      ...(axis.oracle ?? {}),
      algorithm: "memory-safety-case-summary-v1",
      output: expected,
    },
    budget: axis.budget,
    fairness: axis.fairness,
    metrics: {
      memory_safety_findings: measured ? findings.length : null,
      finding_occurrences: measured ? findingOccurrences : null,
      valid_cases: measured ? summary.counts.valid : null,
      adversarial_cases: measured ? corpus.caseCount - summary.counts.valid : null,
    },
    expected_stdout: expected,
    runners,
    findings,
    finding_receipts: findings,
    publication: {
      status: blockers.length === 0 ? "ready" : "blocked",
      blockers,
      finding_receipts_deduplicated: true,
      tower_transport: "cli-only",
      performance_exclusions: 0,
    },
  };
}
