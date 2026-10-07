#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

// Syntax::generated_path encodes UTF-8 bytes, not Unicode scalar values.
export function decodeJet(identifier) {
  let body = identifier.slice('__jet_'.length);
  if (body.startsWith('ct_')) return '$' + body.slice(3);
  if (body.startsWith('__')) return '[generated] ' + body.slice(2);
  const bytes = [];
  const escapes = { u: 95, d: 46, c: 58, s: 47, b: 92, h: 45 };
  for (let i = 0; i < body.length;) {
    if (body[i] === '_' && escapes[body[i + 1]] !== undefined) {
      bytes.push(escapes[body[i + 1]]); i += 2;
    } else if (body[i] === '_' && body[i + 1] === 'x' && /^[0-9a-f]{2}$/i.test(body.slice(i + 2, i + 4))) {
      bytes.push(parseInt(body.slice(i + 2, i + 4), 16)); i += 4;
    } else {
      bytes.push(...Buffer.from(body[i])); i++;
    }
  }
  return Buffer.from(bytes).toString('utf8');
}

export function jetName(symbol, declarations = new Map()) {
  return symbol.replace(/::h[0-9a-f]{16}\b/g, '')
    .replace(/\bjetc_(jetfoundation|jetsema|jetcomptime|jetcodegen|jetmir|jetparser|jetlexer|jetdriver|jeteval|jetoptimizer|jetbackend|jetcli)\b/g,
      (_, module) => ({ jetfoundation: 'JetFoundation', jetsema: 'JetSema', jetcomptime: 'JetComptime',
        jetcodegen: 'JetCodegen', jetmir: 'JetMIR', jetparser: 'JetParser', jetlexer: 'JetLexer', jetdriver: 'JetDriver',
        jeteval: 'JetEval', jetoptimizer: 'JetOptimizer', jetbackend: 'JetBackend', jetcli: 'JetCli' })[module])
    .replace(/__jet_[A-Za-z0-9_]+/g, value => {
      // Names::mangle leaves a flat name unchanged; canonical paths use the
      // byte-escape lane. Known source declarations disambiguate flat names.
      const plain = value.slice('__jet_'.length);
      return declarations.has(plain) ? plain : decodeJet(value).replace(/^src::[^:]+::/, '');
    });
}

function sourceIndex(mapFile, repo) {
  const index = new Map();
  if (!fs.existsSync(mapFile)) {
    console.error(`Source map unavailable: ${mapFile}; names still decoded, no declaration locations.`);
    return index;
  }
  const map = JSON.parse(fs.readFileSync(mapFile, 'utf8'));
  if (map.schema !== 'jet-bootstrap-source-map/v2') throw new Error(`Unsupported source map schema: ${map.schema}`);
  // This map maps assembled Jet byte spans, NOT emitted Rust line numbers.
  // Use its original-file inventory to locate Jet declarations.
  for (const file of map.files) {
    const filename = path.resolve(repo, file.path);
    if (!fs.existsSync(filename)) continue;
    const bytes = fs.readFileSync(filename);
    const mapped = createHash('sha256').update(bytes).digest('hex') === file.sha256;
    const lines = bytes.toString('utf8').split('\n');
    for (let line = 0; line < lines.length; line++) {
      const declaration = /^\s*(?:pub\s+)?(?:fn|struct|enum|trait|type|const)\s+([$\p{L}_][\p{L}\p{N}_$]*)/u.exec(lines[line]);
      if (!declaration) continue;
      const location = `${file.path}:${line + 1}`;
      const entries = index.get(declaration[1]) ?? [];
      // Entries are declaration locations, never a fabricated sampled line.
      entries.push({ location, module: file.path.split('/')[1], mapped });
      index.set(declaration[1], entries);
    }
  }
  return index;
}

function declarationLocation(symbol, index) {
  const prefix = /\b(Jet[A-Za-z]+)::([$\p{L}\p{N}_:.]+)/u.exec(symbol);
  if (!prefix) return '';
  const names = prefix[2].split('::').reverse();
  for (const name of names) {
    const candidates = (index.get(name) ?? []).filter(entry => entry.module === prefix[1]);
    if (candidates.length === 1) return ` [decl ${candidates[0].location}${candidates[0].mapped ? '' : ' (current source)'}]`;
  }
  return '';
}

// Function/owner before generic arguments only: clone<RegistrationGraph> is
// not registration when its actual caller is sema_function_check.
function framePhase(symbol, weak = false) {
  const head = symbol.split('<')[0].toLowerCase();
  if (/\b(?:jetcomptime|jeteval|jet_eval_|comptime_|ct_|sema_comptime|sema_expr_comptime|sema_check_comptime)/.test(head)) return 'comptime';
  if (/\b(?:sema_registration_|sema_register_|registration_|register_)/.test(head)) return 'registration';
  if (/\b(?:jet_rust_emit_|rust_emit_|emit_)/.test(head)) return 'emission';
  if (/\b(?:mir_opt|mir_optimization|mir_pass_|mir_lint_|optimize_|optimization_|jetoptimizer)/.test(head)) return 'optimization';
  if (/\b(?:jet_codegen_|tir_to_mir|lower_|lowering_|mir_lower|jetlower)/.test(head)) return 'lowering';
  if (/\b(?:sema_check_|sema_function_check|sema_expr_|sema_statement_|check_expression|check_program)/.test(head)) return 'sema check';
  if (weak && /\b(?:jetsema::|sema_)/.test(head)) return 'sema check';
  if (weak && /\bjetcodegen::/.test(head)) return 'emission';
  return null;
}

export function classify(stack) {
  for (const frame of stack) { const phase = framePhase(frame); if (phase) return phase; }
  for (const frame of stack) { const phase = framePhase(frame, true); if (phase) return phase; }
  return 'unclassified';
}

const xml = value => value.replace(/[&<>"']/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&apos;' })[char]);
const pct = (count, total) => (100 * count / total).toFixed(2) + '%';

export function renderFolded(folded, title = 'jetc CPU samples') {
  const root = { name: 'all samples', count: 0, children: new Map() };
  let depth = 0;
  for (const line of folded.trim().split('\n')) {
    const match = /^(.*) ([0-9]+)$/.exec(line);
    if (!match) continue;
    const frames = match[1].split(';'), count = Number(match[2]);
    if (!count) continue;
    depth = Math.max(depth, frames.length);
    root.count += count;
    let node = root;
    for (const name of frames) {
      if (!node.children.has(name)) node.children.set(name, { name, count: 0, children: new Map() });
      node = node.children.get(name); node.count += count;
    }
  }
  if (!root.count) throw new Error('No folded samples to render');
  const width = 1400, height = (depth + 1) * 20 + 65;
  const svg = [`<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">`,
    '<style>text{font:12px monospace;pointer-events:none}rect:hover{stroke:black;stroke-width:1.5}</style>',
    `<rect width="100%" height="100%" fill="#fafafa"/><text x="10" y="22">${xml(title)} — ${root.count} samples; root at bottom; hover for full Jet names/locations</text>`];
  function draw(node, x, level) {
    const w = node.count / root.count * (width - 20), y = height - 30 - level * 20;
    let hash = 0;
    for (const char of node.name) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
    if (w >= 0.1) {
      const label = node.name.slice(0, Math.max(0, Math.floor(w / 7) - 1));
      svg.push(`<g><title>${xml(node.name)} — ${node.count} samples (${pct(node.count, root.count)})</title><rect x="${x.toFixed(3)}" y="${y}" width="${w.toFixed(3)}" height="19" fill="hsl(${hash % 55},85%,${60 + hash % 18}%)"/>${w > 14 ? `<text x="${(x + 3).toFixed(3)}" y="${y + 14}">${xml(label)}</text>` : ''}</g>`);
    }
    let nextX = x;
    for (const child of [...node.children.values()].sort((a, b) => a.name.localeCompare(b.name))) {
      draw(child, nextX, level + 1); nextX += child.count / root.count * (width - 20);
    }
  }
  draw(root, 10, 0); svg.push('</svg>');
  return svg.join('\n') + '\n';
}

async function analyze(script, mapFile, repo, out, graph) {
  const index = sourceIndex(mapFile, repo), cache = new Map();
  const totals = new Map(), folded = new Map(), phases = new Map();
  let stack = [], samples = 0, shortStacks = 0, unresolvedRoots = 0, jetStacks = 0;
  const normalize = raw => {
    if (!cache.has(raw)) {
      let name = jetName(raw, index);
      // Optimized Rust DWARF/perf may expose only the bare Jet function name.
      const bare = /^([$\p{L}_][\p{L}\p{N}_$]*)(?:\.llvm\.[A-Za-z0-9]+)?$/u.exec(name);
      const entries = bare ? index.get(bare[1]) : null;
      if (entries?.length === 1) name = `${entries[0].module}::${bare[1]}`;
      else if (bare) {
        const module = /^sema_/.test(bare[1]) ? 'JetSema'
          : /^jet_(?:rust_emit|codegen)_/.test(bare[1]) ? 'JetCodegen'
          : /^jet_eval_/.test(bare[1]) ? 'JetEval'
          : /^mir_(?:pass|optimizer|lint)_/.test(bare[1]) ? 'JetOptimizer' : null;
        if (module) name = `${module}::${bare[1]}`;
      }
      cache.set(raw, name + declarationLocation(name, index));
    }
    return cache.get(raw);
  };
  function flush() {
    if (!stack.length) return;
    samples++;
    if (stack.length <= 2) shortStacks++;
    if (/^(?:\[unknown\]|0x[0-9a-f]+|unknown)(?: |$)/i.test(stack.at(-1))) unresolvedRoots++;
    if (stack.some(frame => /\bJet[A-Za-z]+::/.test(frame))) jetStacks++;
    const phase = classify(stack); phases.set(phase, (phases.get(phase) ?? 0) + 1);
    for (const frame of new Set(stack)) {
      const row = totals.get(frame) ?? { name: frame, inclusive: 0, self: 0 };
      row.inclusive++; totals.set(frame, row);
    }
    totals.get(stack[0]).self++;
    const key = stack.toReversed().map(frame => frame.replaceAll(';', ':')).join(';');
    folded.set(key, (folded.get(key) ?? 0) + 1); stack = [];
  }
  for await (const line of readline.createInterface({ input: fs.createReadStream(script), crlfDelay: Infinity })) {
    if (!line.trim()) { flush(); continue; }
    // perf script callchain rows: address symbol (DSO), including (inlined).
    const frame = /^\s+([0-9a-f]+)\s+(.+?)\s+\(([^()]*)\)(?:\s+\(inlined\))?\s*$/i.exec(line);
    if (frame) { stack.push(normalize(frame[2].replace(/\+0x[0-9a-f]+$/, ''))); continue; }
    // An event header starts a new sample. The callchain includes its leaf;
    // don't count the header's copy of that leaf a second time.
    if (/^\S.*\d+\.\d+:/.test(line)) flush();
  }
  flush();
  if (!samples) throw new Error('No CPU samples captured; check PID activity and perf permissions.');
  const rows = [...totals.values()].sort((a, b) => b.inclusive - a.inclusive || b.self - a.self).slice(0, 30);
  const top = [`Top 30 by inclusive samples (${samples} samples, ${graph}; recursion counted once per sample)`,
    'INCL %   SELF %   INCL  SELF  Jet module/function [declaration location]',
    ...rows.map(row => `${pct(row.inclusive, samples).padStart(7)}  ${pct(row.self, samples).padStart(7)}  ${String(row.inclusive).padStart(5)} ${String(row.self).padStart(5)}  ${row.name}`)].join('\n') + '\n';
  const ranked = [...phases].sort((a, b) => b[1] - a[1]);
  const known = ranked.filter(([phase]) => phase !== 'unclassified');
  const winner = known[0];
  const verdict = winner ? `${winner[0]} dominates classified samples: ${pct(winner[1], samples)} of all samples, ${pct(winner[1], samples - (phases.get('unclassified') ?? 0))} of classified.` : 'No compiler phase could be classified.';
  const phaseText = ['\nCompiler phases (nearest leaf-side recognizable Jet function; generic type names ignored):',
    ...['registration', 'sema check', 'comptime', 'lowering', 'optimization', 'emission', 'unclassified'].map(phase => `${pct(phases.get(phase) ?? 0, samples).padStart(7)}  ${String(phases.get(phase) ?? 0).padStart(5)}  ${phase}`),
    `Verdict: ${verdict}`, `Stack quality: ${pct(shortStacks, samples)} <=2 frames; ${pct(unresolvedRoots, samples)} unresolved roots; ${pct(jetStacks, samples)} contain Jet symbols.`].join('\n') + '\n';
  const foldedText = [...folded].sort(([a], [b]) => a.localeCompare(b)).map(([key, count]) => `${key} ${count}`).join('\n') + '\n';
  fs.mkdirSync(out, { recursive: true });
  fs.writeFileSync(path.join(out, 'top.txt'), top);
  fs.writeFileSync(path.join(out, 'phases.txt'), phaseText);
  fs.writeFileSync(path.join(out, 'stacks.folded'), foldedText);
  fs.writeFileSync(path.join(out, 'flamegraph.svg'), renderFolded(foldedText));
  fs.writeFileSync(path.join(out, 'summary.json'), JSON.stringify({ samples, graph, shortStacks, unresolvedRoots, jetStacks,
    needsDwarf: shortStacks / samples > 0.5 || unresolvedRoots / samples > 0.5, verdict, phases: Object.fromEntries(phases), top: rows }, null, 2) + '\n');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, ...args] = process.argv.slice(2);
  if (command === 'analyze' && args.length === 5) await analyze(...args);
  else if (command === 'needs-dwarf' && args.length === 1) process.exit(JSON.parse(fs.readFileSync(args[0], 'utf8')).needsDwarf ? 0 : 1);
  else if (command === 'render' && args.length === 2) fs.writeFileSync(args[1], renderFolded(fs.readFileSync(args[0], 'utf8')));
  else { console.error('usage: profile.mjs analyze <perf.script> <map.json> <repo> <out> <graph> | needs-dwarf <summary.json> | render <folded> <svg>'); process.exitCode = 2; }
}
