// Hardening findings are machine-produced facts. Keep their identity and
// evidence normalization here so the CLI and store cannot drift apart.
import { createHash } from 'node:crypto';
import { existsSync, lstatSync, readFileSync } from 'node:fs';
import { isAbsolute, relative, resolve } from 'node:path';

export const HARDENING_SCHEMA_VERSION = 1;
export const HARDENING_REPRO_SCHEMA = 'jet.hardening.repro.v1';
export const UNCLASSIFIED_SEAM = 'unclassified.semantic-primitive';

const SEAM_ALIASES = new Map([
  ['prelude', 'prelude-semantic-function'],
  ['prelude-semantic-function', 'prelude-semantic-function'],
  ['semantic-function', 'prelude-semantic-function'],
  ['semantic-equality', 'interpreter-equality'],
  ['interpreter-equality', 'interpreter-equality'],
  ['indexed-place', 'tir-place-lowering'],
  ['tir-place-lowering', 'tir-place-lowering'],
  ['packed-int', 'packed-int-representation'],
  ['packed-int-representation', 'packed-int-representation'],
  ['packed_int', 'packed-int-representation'],
  ['aot-emit', 'aot-emission'],
  ['aot-emission', 'aot-emission'],
  ['release-emission', 'aot-emission'],
  ['input', 'input-transport'],
  ['input-transport', 'input-transport'],
  ['stdin-transport', 'input-transport'],
  ['unclassified', UNCLASSIFIED_SEAM],
  [UNCLASSIFIED_SEAM, UNCLASSIFIED_SEAM],
]);

const pick = (value, ...keys) => {
  for (const key of keys) if (value?.[key] !== undefined && value[key] !== null) return value[key];
  return undefined;
};

const text = (value) => value == null ? '' : String(value).trim();

const token = (value, fallback) => {
  const out = text(value).toLowerCase().replace(/\s+/g, '-');
  return out || fallback;
};

export function normalizeHardeningSeam(value) {
  const key = token(value, UNCLASSIFIED_SEAM).replace(/_/g, '-');
  return SEAM_ALIASES.get(key) || UNCLASSIFIED_SEAM;
}

function list(value) {
  if (Array.isArray(value)) return value;
  if (value && typeof value === 'object') return Object.keys(value);
  return text(value).split(/[,+]/);
}

function normalizeTierMask(value) {
  const values = [...new Set(list(value)
    .map(item => token(item, ''))
    .filter(Boolean))].sort();
  return values.length ? values : ['unknown'];
}

const encode = (value) => encodeURIComponent(text(value));

export function buildHardeningDedupKey({
  schemaVersion = HARDENING_SCHEMA_VERSION,
  seam,
  relation,
  wrongTierMask,
  inputPartition,
} = {}) {
  const version = Number(schemaVersion);
  if (!Number.isInteger(version) || version < 1) throw new HardeningInputError('schema version must be a positive integer');
  return [
    `hardening:v${version}`,
    `seam=${encode(normalizeHardeningSeam(seam))}`,
    `relation=${encode(token(relation, 'unknown-relation'))}`,
    `tiers=${encode(normalizeTierMask(wrongTierMask).join(','))}`,
    `partition=${encode(token(inputPartition, 'unknown-partition'))}`,
  ].join('|');
}

function parseKey(key) {
  try {
    const fields = text(key).split('|');
    if (fields.length !== 5 || !fields[0].startsWith('hardening:v')) return null;
    const values = Object.fromEntries(fields.slice(1).map(field => {
      const at = field.indexOf('=');
      return at < 0 ? [field, ''] : [field.slice(0, at), decodeURIComponent(field.slice(at + 1))];
    }));
    const match = fields[0].match(/^hardening:v(\d+)$/);
    if (!match || !values.seam || !values.relation || !values.tiers || !values.partition) return null;
    return {
      schemaVersion: Number(match[1]),
      seam: normalizeHardeningSeam(values.seam),
      relation: values.relation,
      wrongTierMask: normalizeTierMask(values.tiers),
      inputPartition: values.partition,
    };
  } catch {
    return null;
  }
}

export class HardeningInputError extends Error {
  constructor(message) {
    super(message);
    this.code = 'E_INVALID';
  }
}

function nestedPayload(input = {}) {
  const hardening = input.hardening && typeof input.hardening === 'object' ? input.hardening : {};
  const bundle = input.hardeningBundle && typeof input.hardeningBundle === 'object'
    ? input.hardeningBundle
    : input.repro && typeof input.repro === 'object' ? input.repro
      : hardening.bundle && typeof hardening.bundle === 'object' ? hardening.bundle : {};
  return { ...bundle, ...hardening, ...input };
}

const hasAny = (value, keys) => keys.some(key => value[key] !== undefined && value[key] !== null);

export function hasHardeningPayload(input = {}) {
  const p = nestedPayload(input);
  return hasAny(p, [
    'hardeningDedupKey', 'hardening_dedup_key', 'dedupKey', 'dedup_key',
    'hardeningSeam', 'rootSeam', 'semanticPrimitive', 'semantic_primitive',
    'hardeningRelation', 'violatedRelation', 'violated_relation',
    'hardeningWrongTierMask', 'wrongTierMask', 'wrong_tier_mask',
    'hardeningInputPartition', 'inputPartition', 'input_partition',
    'hardeningEvidence', 'hardeningFindingId', 'findingId', 'finding_id',
    'hardeningBundleDigest', 'bundleDigest', 'bundle_digest', 'hardeningFixture',
  ]);
}

function commandList(value) {
  if (Array.isArray(value)) {
    return value.map(item => {
      if (typeof item === 'string') return item.trim();
      if (item && typeof item === 'object') {
        const tier = text(pick(item, 'tier', 'name'));
        const command = text(pick(item, 'command', 'cmd', 'value'));
        return tier ? `${tier}: ${command}` : command;
      }
      return text(item);
    }).filter(Boolean);
  }
  if (value && typeof value === 'object')
    return Object.entries(value).map(([tier, command]) => `${tier}: ${text(command)}`).filter(Boolean);
  return text(value) ? [text(value)] : [];
}

function stable(value) {
  if (Array.isArray(value)) return `[${value.map(stable).join(',')}]`;
  if (value && typeof value === 'object')
    return `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${stable(value[key])}`).join(',')}}`;
  return JSON.stringify(value);
}

const sha256 = (value) => `sha256:${createHash('sha256').update(stable(value)).digest('hex')}`;

function bool(value) {
  return value === true || value === 'true' || value === 1 || value === '1';
}

function classification(value) {
  return token(value, '').replace(/_/g, '-');
}

export function hardeningSeverity(input = {}) {
  const p = nestedPayload(input);
  const kind = classification(p.classification ?? p.findingClassification ?? p.finding_classification ?? p.hardeningClassification);
  const silent = bool(p.silentWrongData) || bool(p.silent_wrong_data) || bool(p.silentData) || bool(p.silent_data) || [
    'silent', 'silent-data', 'wrong-data', 'silent-wrong-data',
  ].includes(kind);
  const defaultDivergence = bool(p.defaultJetRunDivergence)
    || bool(p.default_jet_run_divergence)
    || bool(p.defaultRunDivergence)
    || bool(p.default_run_divergence)
    || (bool(p.divergence) && ['jet-run', 'jet_run', 'default'].includes(classification(p.tier)));
  if (silent || defaultDivergence) return 'P0';
  const loud = bool(p.loudFailure) || bool(p.loud_failure) || bool(p.timeout) || !!p.signal
    || (p.exit !== undefined && p.exit !== null && Number(p.exit) !== 0);
  if (p.priority === 'P0') return 'P0';
  return loud || p.priority === 'P1' ? 'P1' : 'P0';
}

function evidenceFrom(input, key, findingId) {
  const p = nestedPayload(input);
  const raw = p.hardeningEvidence && typeof p.hardeningEvidence === 'object'
    ? (Array.isArray(p.hardeningEvidence) ? (p.hardeningEvidence[0] || p) : p.hardeningEvidence)
    : p;
  const source = text(pick(raw, 'source', 'minimizedSource', 'exactSource', 'exact_minimized_source'));
  const commands = commandList(pick(raw, 'commands', 'tierCommands', 'tier_commands', 'tierCommand', 'tier_command'));
  const expected = pick(raw, 'expectedRelation', 'expected_relation', 'expected');
  const actual = pick(raw, 'actualRelation', 'actual_relation', 'actual');
  const seed = pick(raw, 'seed');
  const commit = text(pick(raw, 'targetCommit', 'target_commit', 'commit', 'jetCommit', 'jet_commit'));
  const suppliedDigest = text(pick(raw, 'bundleDigest', 'bundle_digest', 'digest'));
  const missing = [];
  if (!source) missing.push('source');
  if (!commands.length) missing.push('commands');
  if (expected === undefined || expected === null || !text(expected)) missing.push('expectedRelation');
  if (actual === undefined || actual === null || !text(actual)) missing.push('actualRelation');
  if (seed === undefined || seed === null || !text(seed)) missing.push('seed');
  if (!commit) missing.push('targetCommit');
  if (missing.length) throw new HardeningInputError(`hardening repro bundle missing: ${missing.join(', ')}`);
  const core = {
    schema_version: HARDENING_SCHEMA_VERSION,
    repro_schema: HARDENING_REPRO_SCHEMA,
    finding_id: findingId,
    stable_key: key,
    source,
    commands,
    expected_relation: text(expected),
    actual_relation: text(actual),
    seed: text(seed),
    target_commit: commit,
    classification: text(pick(raw, 'classification', 'findingClassification', 'finding_classification')),
    stdout_bytes: pick(raw, 'stdoutBytes', 'stdout_bytes', 'stdout') ?? '',
    stderr_bytes: pick(raw, 'stderrBytes', 'stderr_bytes', 'stderr') ?? '',
    exit: pick(raw, 'exit'),
    signal: pick(raw, 'signal') ?? null,
    timeout: bool(pick(raw, 'timeout')),
    normalization: pick(raw, 'normalization') ?? [],
  };
  const bundleDigest = sha256(core);
  if (suppliedDigest && suppliedDigest !== bundleDigest) {
    throw new HardeningInputError('hardening repro bundle digest does not match its canonical evidence');
  }
  return { ...core, bundleDigest, bundle_digest: bundleDigest };
}

export function normalizeHardeningFixture(value, findingId) {
  if (value == null || value === '') return null;
  if (typeof value === 'string') return { path: value.trim(), findingId };
  if (typeof value !== 'object') throw new HardeningInputError('hardening fixture must be a path or object');
  return {
    path: text(value.path ?? value.file),
    findingId: text(value.findingId ?? value.finding_id ?? findingId),
    digest: text(value.digest ?? value.bundleDigest),
  };
}

// Closure checks use the host project root attached non-enumerably to the
// store config. A fixture must be a real checked-in corpus/example file.
export function hardeningFixtureIssue(card, config = {}) {
  if (!card?.hardeningDedupKey) return null;
  const fixture = card.hardeningFixture;
  if (!fixture?.path) return `card #${card.num} needs a permanent corpus fixture`;
  if (fixture.findingId !== card.hardeningFindingId)
    return `fixture must name finding ${card.hardeningFindingId}`;
  const root = config.projectRoot || process.cwd();
  const path = isAbsolute(fixture.path) ? resolve(fixture.path) : resolve(root, fixture.path);
  const rel = relative(root, path);
  if (rel.startsWith('..') || isAbsolute(rel)) return 'fixture must stay inside the project root';
  if (!/(^|[\\/])(tests|examples)[\\/]/.test(rel)) return 'fixture must live under tests/ or examples/';
  if (!existsSync(path)) return `fixture does not exist: ${fixture.path}`;
  try {
    const stat = lstatSync(path);
    if (!stat.isFile()) return `fixture is not a regular file: ${fixture.path}`;
    const contents = readFileSync(path, 'utf8');
    if (!contents.includes(card.hardeningFindingId)) return `fixture does not name finding ${card.hardeningFindingId}`;
  } catch {
    return `fixture is unreadable: ${fixture.path}`;
  }
  return null;
}

export function prepareHardening(input = {}, previous = null) {
  const p = nestedPayload(input);
  const explicitKeyValue = pick(p, 'hardeningDedupKey', 'hardening_dedup_key', 'dedupKey', 'dedup_key');
  if (explicitKeyValue !== undefined && !text(explicitKeyValue))
    throw new HardeningInputError('hardening dedup key cannot be empty');
  const explicitKey = text(explicitKeyValue);
  const old = previous || {};
  const oldKey = text(old.hardeningDedupKey);
  const explicitComponents = hasAny(p, [
    'hardeningSchemaVersion', 'schemaVersion', 'schema_version', 'hardeningSeam', 'rootSeam', 'semanticPrimitive', 'semantic_primitive',
    'hardeningRelation', 'violatedRelation', 'violated_relation', 'relation', 'hardeningWrongTierMask', 'wrongTierMask', 'wrong_tier_mask',
    'hardeningInputPartition', 'inputPartition', 'input_partition',
  ]);
  const parsed = parseKey(explicitKey || oldKey);
  const schemaVersion = pick(p, 'hardeningSchemaVersion', 'schemaVersion', 'schema_version')
    ?? old.hardeningSchemaVersion ?? parsed?.schemaVersion ?? HARDENING_SCHEMA_VERSION;
  const seam = pick(p, 'hardeningSeam', 'rootSeam', 'semanticPrimitive', 'semantic_primitive')
    ?? old.hardeningSeam ?? parsed?.seam;
  const relation = pick(p, 'hardeningRelation', 'violatedRelation', 'violated_relation', 'relation')
    ?? old.hardeningRelation ?? parsed?.relation;
  const wrongTierMask = pick(p, 'hardeningWrongTierMask', 'wrongTierMask', 'wrong_tier_mask', 'tierMask')
    ?? old.hardeningWrongTierMask ?? parsed?.wrongTierMask;
  const inputPartition = pick(p, 'hardeningInputPartition', 'inputPartition', 'input_partition')
    ?? old.hardeningInputPartition ?? parsed?.inputPartition;
  const canonicalKey = explicitKey && !explicitComponents
    ? (previous?.hardeningDedupKey || explicitKey)
    : buildHardeningDedupKey({ schemaVersion, seam, relation, wrongTierMask, inputPartition });
  const aliases = [...new Set([
    ...(Array.isArray(old.hardeningDedupAliases) ? old.hardeningDedupAliases : []),
    ...list(pick(p, 'hardeningDedupAliases', 'hardening_dedup_aliases', 'dedupAliases', 'aliases')).map(text).filter(Boolean),
    ...(explicitKey && explicitKey !== canonicalKey ? [explicitKey] : []),
    ...(oldKey && oldKey !== canonicalKey ? [oldKey] : []),
  ])].filter(alias => alias !== canonicalKey);
  const findingId = text(pick(p, 'hardeningFindingId', 'findingId', 'finding_id'))
    || text(old.hardeningFindingId)
    || `HF-${createHash('sha256').update(canonicalKey).digest('hex').slice(0, 16)}`;
  const hasEvidence = hasAny(p, [
    'hardeningEvidence', 'source', 'minimizedSource', 'exactSource', 'exact_minimized_source', 'commands', 'tierCommands',
    'tier_commands', 'expectedRelation', 'expected_relation', 'actualRelation', 'actual_relation',
    'seed', 'targetCommit', 'target_commit', 'commit', 'jetCommit', 'jet_commit', 'bundleDigest', 'bundle_digest', 'digest',
  ]);
  const evidence = hasEvidence ? evidenceFrom(p, canonicalKey, findingId) : null;
  if (!previous && !evidence) throw new HardeningInputError('hardening card needs a repro bundle');
  const fixture = p.hardeningFixture !== undefined
    ? normalizeHardeningFixture(p.hardeningFixture, findingId)
    : old.hardeningFixture || null;
  const title = text(p.title) || text(old.title) || `Hardening finding ${findingId}`;
  const body = text(p.body);
  return {
    payload: p,
    key: canonicalKey,
    aliases,
    schemaVersion: Number(schemaVersion),
    seam: normalizeHardeningSeam(seam),
    relation: token(relation, 'unknown-relation'),
    wrongTierMask: normalizeTierMask(wrongTierMask),
    inputPartition: token(inputPartition, 'unknown-partition'),
    findingId,
    severity: hardeningSeverity(p),
    evidence,
    fixture,
    title,
    body,
    priority: hardeningSeverity(p),
  };
}

export function formatHardeningEvidence(evidence) {
  const commands = evidence.commands.map(command => `  ${command}`).join('\n');
  return [
    `Hardening finding: ${evidence.finding_id}`,
    'Minimized reproducer:',
    evidence.source,
    'Exact commands:',
    commands,
    `Expected relation: ${evidence.expected_relation}`,
    `Actual relation: ${evidence.actual_relation}`,
    `Seed: ${evidence.seed}`,
    `Target commit: ${evidence.target_commit}`,
    `Bundle digest: ${evidence.bundleDigest}`,
  ].join('\n');
}
