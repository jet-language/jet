// Peer standings (#6834): every comparison entry against every standings
// peer (matrix.json rails.standings) and the hand-written asm ceiling, on
// startup, run time and size. Jet/peer below 1.00 passes the peer floor;
// Jet/asm at most 1.00 meets the ceiling.
//
// This module is pure: run.mjs gathers programs, toolchains, interpreter
// footprints and measured rows, and this module plans the cells, judges them
// and renders the rows. The results file's `standings` section is its output;
// Tower's Gauntlet page and `tower gauntlet standings` read that section.
// Every cell is a row: a peer with no program is `uncovered`, a peer that
// cannot express the entry is `not_applicable` with the manifest's reason, a
// peer whose toolchain is missing is `unavailable`, and a failed or wrong run
// is `failed` or `mismatched`. Nothing is dropped, picked or averaged.

export const STANDINGS_SCHEMA = "jet.gauntlet.standings.v1";

export const STANDINGS_METRICS = Object.freeze([
  Object.freeze({ id: "startup", family: "startup", label: "startup", what: "exec to first output, wall time (service: exec to ready)" }),
  Object.freeze({ id: "wall", family: "run", label: "run wall", what: "wall time of the whole run (service: p50 request latency)" }),
  Object.freeze({ id: "cycles", family: "run", label: "run cycles", what: "user-mode CPU cycles of the process tree (perf stat -e cycles:u)" }),
  Object.freeze({ id: "peak_rss", family: "run", label: "peak RSS", what: "peak resident memory of the process tree" }),
  Object.freeze({ id: "size", family: "size", label: "size", what: "deployed bytes on the stated size basis: a static stripped binary, else the stripped binary plus every shared library it loads (libc included); interpreted peers count script plus runtime" }),
]);

const METRIC_IDS = STANDINGS_METRICS.map((metric) => metric.id);

export const STANDINGS_JUDGEMENT = Object.freeze({
  ratio: "jet / peer",
  floor: Object.freeze({ pass: "ratio < 1.00", fail: "ratio >= 1.00" }),
  ceiling: Object.freeze({ meet: "ratio <= 1.00", miss: "ratio > 1.00", gap: "ratio - 1.00" }),
});

// How a peer's size is counted. Compiled peers ship a binary; interpreted
// peers ship their script and need their runtime; bundled peers ship the
// bundle a web build emits. Web entries measure every artifact peer by its
// web artifact bytes.
export const PEER_KINDS = Object.freeze({
  jet: "compiled",
  rust: "compiled",
  c: "compiled",
  zig: "compiled",
  go: "compiled",
  asm: "compiled",
  python: "interpreted",
  node: "interpreted",
  bun: "interpreted",
  vite: "bundled",
});

// The stated size basis (#6834 c6): every size cell names one of these, and
// measurement-manifest.json standings.size_basis defines each, in this order.
export const SIZE_BASES = Object.freeze(["static stripped", "stripped + shared libraries", "web artifact", "bundle", "script+runtime"]);

const WEB_MODES = new Set(["web", "web-app"]);
const TIMED_SOURCES = Object.freeze({
  startup: { metric: "runtime_first_stdout_seconds", unit: "s" },
  wall: { metric: "runtime_wall_seconds", unit: "s" },
  cycles: { metric: "runtime_cycles", unit: "cycles" },
  peak_rss: { metric: "runtime_peak_rss_kb", unit: "kB" },
});
const METRIC_SOURCES = Object.freeze({
  batch: TIMED_SOURCES,
  "batch-steps": TIMED_SOURCES,
  web: TIMED_SOURCES,
  "web-app": TIMED_SOURCES,
  service: {
    startup: { metric: "service_startup_seconds", unit: "s" },
    wall: { metric: "service_latency_ms_p50", unit: "ms" },
    cycles: { metric: null, reason: "a service runs until it is stopped, so its cycles have no run to count; startup, latency and RSS judge it" },
    peak_rss: { metric: "runtime_peak_rss_kb", unit: "kB" },
  },
});

export function isWebEntry(entry) {
  return WEB_MODES.has(entry?.mode);
}

// The standings policy: matrix.json rails.standings names the peers and the
// ceiling; measurement-manifest.json standings records what no peer can run.
export function standingsPolicy(matrix, manifest) {
  const rail = matrix?.rails?.standings;
  const contract = manifest?.standings;
  const issues = [];
  if (!rail || !Array.isArray(rail.peers) || rail.peers.length === 0) issues.push("matrix.json rails.standings.peers is missing");
  if (typeof rail?.ceiling !== "string") issues.push("matrix.json rails.standings.ceiling is missing");
  for (const peer of rail?.peers ?? []) {
    if (!Object.hasOwn(PEER_KINDS, peer) || peer === "jet") issues.push(`rails.standings peer ${peer} has no size kind`);
  }
  if (!contract) issues.push("measurement-manifest.json standings is missing");
  else {
    if (contract.schema !== STANDINGS_SCHEMA) issues.push(`measurement-manifest.json standings.schema must be ${STANDINGS_SCHEMA}`);
    if (JSON.stringify(Object.keys(contract.metrics ?? {})) !== JSON.stringify(METRIC_IDS)) issues.push(`measurement-manifest.json standings.metrics must define ${METRIC_IDS.join(", ")}`);
    for (const peer of [...(rail?.peers ?? []), rail?.ceiling]) {
      if (typeof contract.programs?.[peer] !== "string") issues.push(`measurement-manifest.json standings.programs has no ${peer} program`);
    }
    for (const [index, rule] of (contract.not_applicable ?? []).entries()) {
      if (!(rail?.peers ?? []).includes(rule?.peer)) issues.push(`standings.not_applicable[${index}] names unknown peer ${rule?.peer}`);
      if (!Array.isArray(rule?.modes) || rule.modes.length === 0) issues.push(`standings.not_applicable[${index}] names no entry modes`);
      if (typeof rule?.reason !== "string" || rule.reason.length === 0) issues.push(`standings.not_applicable[${index}] has no reason`);
    }
    if (JSON.stringify(Object.keys(contract.size_basis ?? {})) !== JSON.stringify(SIZE_BASES)) issues.push(`measurement-manifest.json standings.size_basis must define ${SIZE_BASES.join(", ")}`);
  }
  return {
    peers: [...(rail?.peers ?? [])],
    ceiling: rail?.ceiling ?? "asm",
    not_applicable: [...(contract?.not_applicable ?? [])],
    size_basis: { ...(contract?.size_basis ?? {}) },
    issues,
  };
}

function notApplicable(policy, peer, entry) {
  return policy.not_applicable.find((rule) => rule.peer === peer && rule.modes.includes(entry.mode)) ?? null;
}

function staticMetricPlan(entry, metric) {
  const source = METRIC_SOURCES[entry.mode]?.[metric];
  if (metric !== "size" && source && source.metric === null) return { status: "unmeasured", reason: source.reason };
  return null;
}

// One entry's planned cells. `programs[peer]` is { path, exists } for every
// standings peer; `references` are the asm-parity cells that name the entry's
// matrix cells; `toolchains[peer]` is the harness fingerprint row
// ({ status, version, exit_code }) of the tool the peer needs, and
// `toolchains.perf` the cycle counter's.
export function planEntry(entry, policy, { programs = {}, references = [], toolchains = {} } = {}) {
  const peers = {};
  for (const peer of policy.peers) {
    const program = programs[peer] ?? null;
    const rule = notApplicable(policy, peer, entry);
    if (rule) peers[peer] = { role: "peer", plan: "not_applicable", reason: rule.reason, program: null };
    else if (!program?.exists) peers[peer] = { role: "peer", plan: "uncovered", reason: `no ${peer} program: ${program?.path ?? "none"} is missing`, program: program?.path ?? null };
    else if (toolchains[peer] && toolchains[peer].status !== "ok") peers[peer] = { role: "peer", plan: "unavailable", reason: unavailableReason(peer, toolchains[peer]), program: program.path };
    else peers[peer] = { role: "peer", plan: "measure", reason: null, program: program.path };
  }
  const ceiling = policy.ceiling;
  const names = references.map((reference) => reference.name);
  if (names.length === 0) {
    peers[ceiling] = { role: "ceiling", plan: "uncovered", reason: `no hand-written reference in Tools/perf/asm-parity/cells.json names ${(entry.cells ?? []).join(", ") || "this entry"}`, program: null, reference: null };
  } else if (names.length > 1) {
    peers[ceiling] = { role: "ceiling", plan: "failed", reason: `references ${names.join(", ")} all name this entry; one entry has one ceiling`, program: null, reference: null };
  } else {
    const program = programs[ceiling] ?? { path: `Tools/perf/asm-parity/${names[0]}/ref.S`, exists: true };
    peers[ceiling] = !program.exists
      ? { role: "ceiling", plan: "uncovered", reason: `reference ${names[0]} has no ${program.path}`, program: program.path, reference: names[0] }
      : toolchains[ceiling] && toolchains[ceiling].status !== "ok"
        ? { role: "ceiling", plan: "unavailable", reason: unavailableReason(ceiling, toolchains[ceiling]), program: program.path, reference: names[0] }
        : { role: "ceiling", plan: "measure", reason: null, program: program.path, reference: names[0] };
  }
  for (const [peer, item] of Object.entries(peers)) {
    if (item.plan === "measure") item.size_basis = plannedSizeBasis(entry, peer, item.role);
  }
  const cycles = toolchains.perf && toolchains.perf.status !== "ok"
    ? { status: "unavailable", reason: unavailableReason("perf", toolchains.perf) }
    : null;
  // Jet links statically and has no other build to try; a web entry ships its artifacts.
  const jet_size_basis = isWebEntry(entry) ? "web artifact" : "static stripped";
  return { entry: entry.name, mode: entry.mode, cells: [...(entry.cells ?? [])], peers, cycles, jet_size_basis };
}

// The size basis a measured cell is planned on (#6834 c6). A compiled peer's
// static build is tried first; whether it links statically and reproduces the
// output is only known once built, so the plan names the fallback too. An
// entry that overrides a peer's build command keeps that build. The asm
// ceiling links statically.
export function plannedSizeBasis(entry, peer, role = "peer") {
  const kind = PEER_KINDS[peer] ?? "compiled";
  if (kind === "interpreted") return "script+runtime";
  if (isWebEntry(entry)) return kind === "bundled" ? "bundle" : "web artifact";
  if (role === "ceiling") return "static stripped";
  const build = entry.spec?.build;
  const overridden = build && typeof build === "object" && !Array.isArray(build) ? (build[peer] ?? build.default) : build;
  return overridden
    ? "stripped + shared libraries if dynamic (the entry overrides the build command)"
    : "static stripped, else stripped + shared libraries";
}

function unavailableReason(peer, toolchain) {
  const command = Array.isArray(toolchain.command) ? toolchain.command.join(" ") : peer;
  return `${peer} toolchain unavailable: \`${command}\` exit ${toolchain.exit_code ?? "?"}${toolchain.version ? `: ${toolchain.version}` : ""}`;
}

// Every entry x peer x metric cell of a plan, as --dry-run prints them.
export function planCells(plans) {
  const cells = [];
  for (const plan of plans) {
    for (const [peer, item] of Object.entries(plan.peers)) {
      for (const metric of METRIC_IDS) {
        const fixed = item.plan === "measure" ? (staticMetricPlan(plan, metric) ?? (metric === "cycles" ? plan.cycles : null)) : null;
        cells.push({
          entry: plan.entry,
          mode: plan.mode,
          peer,
          role: item.role,
          metric,
          status: fixed?.status ?? item.plan,
          reason: fixed?.reason ?? item.reason,
          program: item.program,
          size_basis: metric === "size" && (fixed?.status ?? item.plan) === "measure" ? item.size_basis : null,
          jet_size_basis: metric === "size" && (fixed?.status ?? item.plan) === "measure" ? plan.jet_size_basis : null,
        });
      }
    }
  }
  return cells;
}

const positive = (value) => typeof value === "number" && Number.isFinite(value) && value > 0;

// A row's value for a metric, with its unit, source and (for size) basis.
function metricValue(entry, peer, row, metric, runtimes) {
  if (metric === "size") return sizeValue(entry, peer, row, runtimes);
  const source = METRIC_SOURCES[entry.mode]?.[metric];
  if (!source) return { value: null, reason: `mode ${entry.mode} has no ${metric} source` };
  if (source.metric === null) return { value: null, reason: source.reason, status: "unmeasured" };
  const value = row?.metrics?.[source.metric];
  if (positive(value)) return { value, unit: source.unit, source: source.metric };
  if (metric === "cycles" && row?.cycles?.reason) return { value: null, reason: row.cycles.reason, status: row.cycles.status ?? "unmeasured" };
  return { value: null, reason: `${source.metric} was not recorded` };
}

function sizeValue(entry, peer, row, runtimes) {
  const kind = PEER_KINDS[peer] ?? "compiled";
  if (kind === "interpreted") {
    const script = row?.script?.bytes;
    const runtime = runtimes?.[peer];
    if (!positive(script)) return { value: null, reason: "script bytes were not recorded" };
    if (!runtime || runtime.status !== "ok" || !positive(runtime.bytes)) {
      return { value: null, reason: `${peer} runtime footprint unmeasured: ${runtime?.reason ?? "not recorded"}` };
    }
    return {
      value: script + runtime.bytes,
      unit: "bytes",
      source: "script+runtime",
      basis: "script+runtime",
      detail: { script_bytes: script, script_files: row.script.files ?? [], runtime_bytes: runtime.bytes, runtime: runtime.path },
    };
  }
  if (isWebEntry(entry)) {
    const value = row?.metrics?.artifact_bytes;
    return positive(value)
      ? { value, unit: "bytes", source: "artifact_bytes", basis: kind === "bundled" ? "bundle" : "web artifact" }
      : { value: null, reason: "artifact_bytes was not recorded" };
  }
  // #6834 c6: run.mjs records the deployed size (row.size): the static
  // stripped build where the peer can build that way, otherwise the stripped
  // binary plus every shared library it loads, libc included.
  const size = row?.size;
  if (!size) return { value: null, reason: "deployed size was not recorded" };
  if (size.status !== "ok" || !positive(size.bytes)) {
    return { value: null, reason: `deployed size unmeasured: ${size.reason ?? "no bytes recorded"}`, ...(size.status === "unavailable" ? { status: "unavailable" } : {}) };
  }
  return {
    value: size.bytes,
    unit: "bytes",
    source: "deployed_bytes",
    basis: size.basis,
    detail: {
      build: size.build,
      linkage: size.linkage,
      measured: size.measured,
      file_bytes: size.file_bytes,
      libraries: size.libraries,
      ...(size.static_build ? { static_build: size.static_build } : {}),
    },
  };
}

// A measured row's standing: ok, or why its numbers do not count.
function rowStatus(row) {
  if (!row) return { status: "failed", reason: "the harness recorded no row" };
  if (row.status === "ok") return { status: "ok", reason: null };
  if (row.status === "not_applicable") return { status: "not_applicable", reason: row.reason ?? "declared not applicable" };
  if (row.missing_toolchain) return { status: "unavailable", reason: row.reason ?? "toolchain missing" };
  const verification = row.verification;
  if (verification?.kind === "byte_exact_stdout" && !/^exit /.test(verification.reason ?? "")) {
    return { status: "mismatched", reason: verification.reason ?? "stdout differs from expected.out" };
  }
  return { status: "failed", reason: row.reason ?? "row failed" };
}

function judge(role, jet, peer) {
  if (jet.value === null) return { jet: null, peer: peer.value ?? null, ratio: null, verdict: jet.status ?? "unmeasured", reason: `Jet: ${jet.reason}` };
  if (peer.value === null) return { jet: jet.value, peer: null, ratio: null, verdict: peer.status ?? "unmeasured", reason: peer.reason };
  const ratio = jet.value / peer.value;
  if (role === "ceiling") return { jet: jet.value, peer: peer.value, ratio, gap: ratio - 1, verdict: ratio <= 1 ? "meet" : "miss" };
  return { jet: jet.value, peer: peer.value, ratio, verdict: ratio < 1 ? "pass" : "fail" };
}

// One entry's standings from its plan and its harness result (rows for the
// declared languages, standings_rows for the standings-only peers and the
// ceiling). `runtimes` holds the interpreter footprints.
export function entryStandings(plan, result, { runtimes = {} } = {}) {
  const entry = { name: plan.entry, mode: plan.mode };
  const jetRow = result?.rows?.jet ?? null;
  const jetStatus = rowStatus(jetRow);
  const peers = {};
  for (const [peer, item] of Object.entries(plan.peers)) {
    const metrics = {};
    if (item.plan !== "measure") {
      for (const metric of METRIC_IDS) metrics[metric] = { jet: null, peer: null, ratio: null, verdict: item.plan, reason: item.reason };
      peers[peer] = { role: item.role, status: item.plan, reason: item.reason, program: item.program, ...(item.reference ? { reference: item.reference } : {}), metrics };
      continue;
    }
    const row = result?.standings_rows?.[peer] ?? result?.rows?.[peer] ?? null;
    const status = row ? rowStatus(row) : { status: "failed", reason: result?.reason ?? "the harness recorded no row" };
    for (const metric of METRIC_IDS) {
      if (status.status !== "ok") {
        metrics[metric] = { jet: null, peer: null, ratio: null, verdict: status.status, reason: status.reason };
        continue;
      }
      const fixed = metric === "cycles" ? plan.cycles : null;
      if (fixed) {
        metrics[metric] = { jet: null, peer: null, ratio: null, verdict: fixed.status, reason: fixed.reason };
        continue;
      }
      const jet = jetStatus.status === "ok"
        ? metricValue(entry, "jet", jetRow, metric, runtimes)
        : { value: null, reason: `${jetStatus.status}: ${jetStatus.reason}` };
      const value = metricValue(entry, peer, row, metric, runtimes);
      const judged = judge(item.role, jet, value);
      metrics[metric] = {
        ...judged,
        ...(value.unit ? { unit: value.unit } : jet.unit ? { unit: jet.unit } : {}),
        ...(value.source ? { source: value.source } : {}),
        ...(metric === "size" ? { jet_basis: jet.basis ?? null, peer_basis: value.basis ?? null } : {}),
        ...(metric === "size" && jet.detail ? { jet_detail: jet.detail } : {}),
        ...(value.detail ? { detail: value.detail } : {}),
      };
    }
    peers[peer] = { role: item.role, status: status.status, reason: status.reason, program: item.program, ...(item.reference ? { reference: item.reference } : {}), metrics };
  }
  return {
    entry: plan.entry,
    mode: plan.mode,
    cells: plan.cells,
    jet: { status: jetStatus.status, reason: jetStatus.reason },
    peers,
    summary: summarize([{ peers }]),
  };
}

function summarize(entries) {
  const floor = { pass: 0, fail: 0, other: 0 };
  const ceiling = { meet: 0, miss: 0, other: 0 };
  const statuses = {};
  for (const entry of entries) {
    for (const item of Object.values(entry.peers)) {
      for (const cell of Object.values(item.metrics)) {
        statuses[cell.verdict] = (statuses[cell.verdict] ?? 0) + 1;
        const bucket = item.role === "ceiling" ? ceiling : floor;
        if (Object.hasOwn(bucket, cell.verdict)) bucket[cell.verdict] += 1;
        else bucket.other += 1;
      }
    }
  }
  return { floor, ceiling, verdicts: statuses };
}

// The results file's standings section.
export function buildStandings(entries, policy, { generated = new Date().toISOString(), runtimes = {}, toolchains = {} } = {}) {
  const losses = { floor: [], ceiling: [] };
  const open = [];
  for (const entry of entries) {
    for (const [peer, item] of Object.entries(entry.peers)) {
      for (const [metric, cell] of Object.entries(item.metrics)) {
        if (cell.verdict === "fail") losses.floor.push({ entry: entry.entry, peer, metric, ratio: cell.ratio });
        else if (cell.verdict === "miss") losses.ceiling.push({ entry: entry.entry, peer, metric, ratio: cell.ratio, gap: cell.gap });
        else if (!["pass", "meet", "not_applicable"].includes(cell.verdict)) open.push({ entry: entry.entry, peer, metric, verdict: cell.verdict, reason: cell.reason });
      }
    }
  }
  const byRatio = (left, right) => right.ratio - left.ratio;
  losses.floor.sort(byRatio);
  losses.ceiling.sort(byRatio);
  return {
    schema: STANDINGS_SCHEMA,
    generated,
    peers: [...policy.peers],
    ceiling: policy.ceiling,
    metrics: STANDINGS_METRICS,
    judgement: STANDINGS_JUDGEMENT,
    not_applicable: policy.not_applicable,
    size_basis: policy.size_basis,
    runtimes,
    toolchains,
    entries,
    summary: summarize(entries),
    losses,
    open,
  };
}

// The scoreboard's machine-ceiling rows from this run's asm and Jet numbers,
// in asm-parity results.json shape (variant asm / jet-default), so
// buildScoreboard judges the ceiling without a separate asm-parity run.
export function ceilingMeasurements(entries) {
  const rows = [];
  for (const entry of entries) {
    for (const item of Object.values(entry.peers)) {
      if (item.role !== "ceiling" || !item.reference) continue;
      if (!["ok", "mismatched"].includes(item.status)) continue;
      const output = item.status === "ok" ? "same" : "DIFF";
      const cycles = item.metrics.cycles;
      const size = item.metrics.size;
      rows.push({ cell: item.reference, variant: "asm", bytes: size.peer, cycles: cycles.peer, output });
      if (entry.jet.status === "ok" || entry.jet.status === "mismatched") {
        rows.push({
          cell: item.reference,
          variant: "jet-default",
          bytes: size.jet,
          cycles: cycles.jet,
          size_x: size.ratio,
          cycles_x: cycles.ratio,
          output: entry.jet.status === "ok" ? "same" : "DIFF",
        });
      }
    }
  }
  return { source: "gauntlet standings (in-run asm rows)", rows };
}

function formatNumber(value) {
  if (value === null || value === undefined) return "-";
  if (Number.isInteger(value)) return String(value);
  return Math.abs(value) >= 100 ? value.toFixed(1) : value.toPrecision(4);
}

export const STANDINGS_COLUMNS = Object.freeze(["entry", "peer", "role", "metric", "jet", "peer_value", "unit", "jet/peer", "verdict", "size basis (jet / peer)", "reason"]);

// Tab-separated rows, one per entry x peer x metric, header first.
export function standingsLines(section) {
  const lines = [STANDINGS_COLUMNS.join("\t")];
  for (const entry of section.entries) {
    for (const [peer, item] of Object.entries(entry.peers)) {
      for (const [metric, cell] of Object.entries(item.metrics)) {
        lines.push([
          entry.entry,
          peer,
          item.role,
          metric,
          formatNumber(cell.jet),
          formatNumber(cell.peer),
          cell.unit ?? "-",
          cell.ratio === null || cell.ratio === undefined ? "-" : cell.ratio.toFixed(2),
          cell.verdict,
          metric === "size" && (cell.jet_basis || cell.peer_basis) ? `${cell.jet_basis ?? "-"} / ${cell.peer_basis ?? "-"}` : "-",
          (cell.reason ?? "").replace(/\s+/g, " ").slice(0, 200) || "-",
        ].join("\t"));
      }
    }
  }
  return lines;
}
