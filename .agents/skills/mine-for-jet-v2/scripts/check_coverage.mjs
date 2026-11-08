#!/usr/bin/env node
// Depth gate for a mine-for-jet-v2 run directory: bounded segments, a mapped concept
// ledger per segment, a full segment x lane matrix, densities, and the recall audit.
// Exit 0 only when nothing is missing; 1 on any gap; 2 on usage errors.
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

export const SCHEMAS = {
  coverage: 'mine-for-jet-v2/coverage@1',
  ledger: 'mine-for-jet-v2/ledger@1',
  audit: 'mine-for-jet-v2/recall-audit@1',
};
export const LANES = ['mechanism-and-architecture', 'default-common-job', 'semantic-obligations', 'expert-descent',
  'cost-and-measurement', 'proven-experience', 'interop-and-portability', 'audience-corrections', 'reuse-and-anti-lessons'];
// Only these lanes may be switched off for a whole source, with a reason.
export const CONDITIONAL_LANES = new Set(['audience-corrections']);
const FAMILY = { video: 'av', audio: 'av', text: 'text', discussion: 'text', repo: 'repo' };
export const LIMITS = { avSeconds: 300, textWords: 1500 };
// Below these floors a segment needs a written density_justification.
export const FLOORS = { avItemsPerMinute: 1, textWordsPerItem: 250, repoItems: 3, claims: 1 };
export const AUDIT = { minSegments: 2, fraction: 0.1 };
const ITEM_KINDS = ['concept', 'technique', 'mechanism', 'claim', 'demo', 'measurement', 'caveat'];
export const ENUMS = {
  source_kind: ['primary', 'audience', 'linked-source', 'local-evidence', 'inference'],
  confidence: ['low', 'medium', 'high'],
  stance: ['supports', 'disputes', 'neutral'],
  lane: LANES,
  scale: ['micro', 'macro'],
  contribution: ['adds-new', 'enhances-existing', 'fills-gap', 'fixes-issue-or-misalignment', 'confirms-choice', 'refutes-choice'],
  observed: ['present', 'exercised', 'partial', 'contradictory', 'unknown'],
  intended: ['ratified', 'undecided', 'rejected-conflicting', 'retired-superseded', 'unknown'],
  work: ['none', 'planning', 'ready', 'building-verify', 'frozen', 'done', 'proposal-only', 'open-ballot', 'draft-ballot', 'unknown'],
  review_disposition: ['keep', 'attach-existing', 'redesign', 'reject-conflict', 'evidence-gap', 'undecided'],
};
const CLAIM_TEXT = ['id', 'topic', 'claim', 'source_id', 'locator', 'workload', 'incremental_contribution'];
const SEGMENT_SOURCED = new Set(['primary', 'audience', 'linked-source']);

const STOP = new Set(('a an and are as at be been being but by can could did do does for from had has have how if in into is it '
  + 'its may might more most must of on or our over so such than that the their them then there these they this those to '
  + 'under up was we were what when where which while who why will with within would you your also very just any all each '
  + 'not no nor none one two only own same both either').split(' '));
// Words that make up boilerplate dispositions; they never count as naming something specific.
const GENERIC = new Set(('applicable inapplicable relevant relevance irrelevant finding findings result results delta deficit '
  + 'jet lane lanes segment segments section part portion piece bit content context covered cover covers already nothing '
  + 'something anything everything thing things stuff action issue issues teaching general generic misc miscellaneous other '
  + 'others above below see ditto skip skipped note notes topic topics offtopic off-topic scope out-of-scope interesting useful '
  + 'important mention mentions mentioned discussed discusses discussion talk video speaker source overall particular really '
  + 'specific specifically concrete apply applies related relates regarding about here new value valuable worth enough n/a na').split(' '));

const isObj = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const text = value => typeof value === 'string' && value.trim() !== '';
const tokens = value => (String(value).toLowerCase().match(/[\p{L}\p{N}][\p{L}\p{N}_'+#/.-]*/gu) ?? [])
  .map(token => token.replace(/[.'/-]+$/, ''));
const significant = value => tokens(value).filter(token => token.length >= 3 && !STOP.has(token) && !GENERIC.has(token));
const stem = token => token.slice(0, 5);
const normal = value => tokens(value).join(' ');

// Returns why a written reason is not specific, or null when it is acceptable.
export function vague(reason, context) {
  if (!text(reason)) return 'missing reason';
  if (tokens(reason).length < 6) return 'reason too short to be specific (at least 6 words)';
  const words = significant(reason);
  if (words.length < 3) return 'generic reason: it names nothing specific';
  if (context !== undefined) {
    const known = new Set(significant(context).filter(token => token.length >= 4).map(stem));
    if (!words.some(token => token.length >= 4 && known.has(stem(token)))) {
      return 'reason names nothing from this segment (title or ledger)';
    }
  }
  return null;
}

export function seconds(value) {
  if (typeof value === 'number') return Number.isFinite(value) && value >= 0 ? value : NaN;
  if (typeof value !== 'string' || !/^\d+(?::\d{1,2}){0,2}(?:\.\d+)?$/.test(value.trim())) return NaN;
  return value.trim().split(':').reduce((total, part) => total * 60 + Number(part), 0);
}
const clock = value => {
  const whole = Math.round(value); const h = Math.floor(whole / 3600); const m = Math.floor(whole / 60) % 60;
  const s = String(whole % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${String(m).padStart(2, '0')}:${s}`;
};

// Deterministic reviewer sample: per source, max(2, 10%) segments ranked by sha256(seed, id).
export function sample(coverage, seed) {
  return (coverage?.sources ?? []).map(source => {
    const ids = (source?.segments ?? []).map(segment => segment?.id).filter(text);
    const size = Math.min(ids.length, Math.max(AUDIT.minSegments, Math.ceil(ids.length * AUDIT.fraction)));
    const ranked = ids.map(id => [createHash('sha256').update(`${seed}\0${id}`).digest('hex'), id])
      .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
    return { source_id: source?.source_id, segments: ranked.slice(0, size).map(([, id]) => id) };
  });
}

export function check({ coverage, ledger, claims, audit }, { requireAudit = false } = {}) {
  const errors = [];
  const err = (rule, where, message) => errors.push({ rule, where, message });
  const report = { sources: [] };

  // ledger.json
  let ledgerSegments = {};
  if (ledger === undefined) err('schema', 'ledger.json', 'missing');
  else if (!isObj(ledger) || ledger.schema !== SCHEMAS.ledger) err('schema', 'ledger.json', `schema must be "${SCHEMAS.ledger}"`);
  if (isObj(ledger?.segments)) ledgerSegments = ledger.segments;
  else if (ledger !== undefined) err('schema', 'ledger.json', 'segments must map segment id -> item array');

  // coverage.json
  if (coverage === undefined) err('schema', 'coverage.json', 'missing');
  else if (!isObj(coverage) || coverage.schema !== SCHEMAS.coverage) err('schema', 'coverage.json', `schema must be "${SCHEMAS.coverage}"`);
  const sources = Array.isArray(coverage?.sources) ? coverage.sources : [];
  if (coverage !== undefined && !sources.length) err('schema', 'coverage.json', 'sources must be a non-empty array');

  // claims.json
  const claimList = Array.isArray(claims) ? claims : claims?.claims;
  const byId = new Map();
  if (claims === undefined) err('schema', 'claims.json', 'missing');
  else if (!Array.isArray(claimList)) err('schema', 'claims.json', 'expected an array or {"claims": [...]}');
  for (const [index, claim] of (Array.isArray(claimList) ? claimList : []).entries()) {
    const where = `claims.json ${claim?.id ?? `#${index}`}`;
    if (!isObj(claim)) { err('schema', where, 'claim must be an object'); continue; }
    for (const field of CLAIM_TEXT) if (!text(claim[field])) err('schema', where, `missing ${field}`);
    for (const [field, values] of Object.entries(ENUMS)) {
      if (!values.includes(claim[field])) err('schema', where, `${field} must be one of ${values.join('|')}, got ${JSON.stringify(claim[field])}`);
    }
    if (SEGMENT_SOURCED.has(claim.source_kind) && !text(claim.segment)) err('claim-segment', where, `${claim.source_kind} claim has no segment id`);
    if (!text(claim.id)) continue;
    if (byId.has(claim.id)) err('schema', where, 'duplicate claim id'); else byId.set(claim.id, claim);
  }

  const segmentById = new Map();
  const sourceOf = new Map();
  const sourceIds = new Set();
  for (const [index, source] of sources.entries()) {
    const sourceWhere = `source ${source?.source_id ?? `#${index}`}`;
    if (!isObj(source)) { err('schema', sourceWhere, 'source must be an object'); continue; }
    if (!text(source.source_id)) err('schema', sourceWhere, 'missing source_id');
    else if (sourceIds.has(source.source_id)) err('schema', sourceWhere, 'duplicate source_id');
    else sourceIds.add(source.source_id);
    for (const field of ['title', 'ref']) if (!text(source[field])) err('schema', sourceWhere, `missing ${field}`);
    const family = FAMILY[source.kind];
    if (!family) err('schema', sourceWhere, `kind must be one of ${Object.keys(FAMILY).join('|')}`);
    const off = isObj(source.lanes_not_run) ? source.lanes_not_run : {};
    if (source.lanes_not_run !== undefined && !isObj(source.lanes_not_run)) err('schema', sourceWhere, 'lanes_not_run must be an object');
    for (const [lane, reason] of Object.entries(off)) {
      if (!CONDITIONAL_LANES.has(lane)) err('lane', sourceWhere, `lane ${lane} is not conditional; it must be mined per segment`);
      const problem = vague(reason);
      if (problem) err('reason', `${sourceWhere} lanes_not_run.${lane}`, problem);
    }
    const segments = Array.isArray(source.segments) ? source.segments : [];
    if (!segments.length) err('segment', sourceWhere, 'no segments');
    const sourceReport = { source_id: source.source_id, kind: source.kind, family, title: source.title, size: 0,
      items: 0, claims: 0, micro: 0, macro: 0, segments: [], span: null, ranged: source.range !== undefined };
    report.sources.push(sourceReport);
    sourceOf.set(sourceReport, source);

    // AV segments must tile [0, duration_s], or the shard's absolute range when one is declared.
    if (family === 'av') {
      const duration = seconds(source.duration_s);
      if (Number.isNaN(duration) || duration <= 0) err('schema', sourceWhere, 'video/audio source needs duration_s');
      if (source.range !== undefined) {
        const start = isObj(source.range) ? seconds(source.range.start_s) : NaN;
        const end = isObj(source.range) ? seconds(source.range.end_s) : NaN;
        if (Number.isNaN(start) || Number.isNaN(end) || end <= start) err('schema', sourceWhere, 'range needs absolute start_s < end_s (seconds)');
        else if (duration > 0 && end > duration + 2) err('schema', sourceWhere, `range ends at ${clock(end)}, past the source duration ${clock(duration)}`);
        else sourceReport.span = [start, end];
      } else if (duration > 0) sourceReport.span = [0, duration];
    } else if (source.range !== undefined) err('schema', sourceWhere, 'range applies only to video/audio sources');

    let cursor = sourceReport.span?.[0] ?? 0; let words = 0; const paths = new Set();
    for (const [position, segment] of segments.entries()) {
      const where = `segment ${segment?.id ?? `${source.source_id}#${position}`}`;
      if (!isObj(segment)) { err('schema', where, 'segment must be an object'); continue; }
      if (!text(segment.id)) { err('schema', where, 'missing id'); continue; }
      if (segmentById.has(segment.id)) { err('schema', where, 'duplicate segment id'); continue; }
      const entry = { segment, source, sourceReport, family, where, claims: [], items: [], size: 0, label: '' };
      segmentById.set(segment.id, entry);
      if (!text(segment.title)) err('schema', where, 'missing title (what the segment covers)');
      if (segment.retrieval !== 'captured') err('body', where, `retrieval is ${JSON.stringify(segment.retrieval)}, not captured: coverage gap`);
      if (segment.body_review !== 'full') err('body', where, `body_review is ${JSON.stringify(segment.body_review)}, not full: coverage gap`);
      if (family === 'av') {
        const start = seconds(segment.start); const end = seconds(segment.end);
        if (Number.isNaN(start) || Number.isNaN(end) || end <= start) { err('segment', where, 'needs start < end (seconds or [h:]mm:ss)'); continue; }
        entry.start = start; entry.end = end; entry.size = end - start; entry.label = `${clock(start)}-${clock(end)}`;
        if (end - start > LIMITS.avSeconds) err('segment-bound', where, `${clock(end - start)} long; video/audio segments are at most ${LIMITS.avSeconds / 60} min`);
        if (Math.abs(start - cursor) > 1) err('segment-gap', where, `starts at ${clock(start)} but the previous segment ended at ${clock(cursor)}`);
        cursor = end;
      } else if (family === 'text') {
        if (!text(segment.section)) err('schema', where, 'text segment needs its section locator');
        if (!Number.isInteger(segment.words) || segment.words <= 0) { err('schema', where, 'text segment needs a positive integer words count'); continue; }
        if (segment.words > LIMITS.textWords) err('segment-bound', where, `${segment.words} words; text segments are at most ${LIMITS.textWords} words (split long sections)`);
        entry.size = segment.words; words += segment.words; entry.label = `${segment.section} (${segment.words} words)`;
      } else if (family === 'repo') {
        if (!text(segment.path) || /[*?{},\s]/.test(segment.path)) err('segment-bound', where, 'repository segment is exactly one module or directory path');
        else if (paths.has(segment.path)) err('schema', where, `path ${segment.path} is already a segment`);
        else paths.add(segment.path);
        entry.size = 1; entry.label = segment.path;
      }
    }
    if (family === 'av' && sourceReport.span && Math.abs(sourceReport.span[1] - cursor) > 2) {
      err('segment-gap', sourceWhere, `segments end at ${clock(cursor)} but the ${sourceReport.ranged ? 'range' : 'source'} `
        + `${sourceReport.ranged ? 'ends at' : 'runs'} ${clock(sourceReport.span[1])}`);
    } else if (family === 'text') {
      if (!Number.isInteger(source.words) || source.words <= 0) err('schema', sourceWhere, 'text source needs its total words count');
      else if (Math.abs(source.words - words) > Math.max(10, source.words * 0.01)) err('segment-gap', sourceWhere, `segments hold ${words} of ${source.words} words`);
    } else if (family === 'repo') {
      const modules = Array.isArray(source.modules) ? source.modules : [];
      if (!modules.length) err('schema', sourceWhere, 'repository source needs its modules inventory');
      const excluded = new Map();
      for (const item of Array.isArray(source.excluded) ? source.excluded : []) {
        if (!text(item?.path)) { err('schema', sourceWhere, 'excluded entries need path and reason'); continue; }
        excluded.set(item.path, item.reason);
        const problem = vague(item.reason, item.path);
        if (problem) err('reason', `${sourceWhere} excluded ${item.path}`, problem);
      }
      for (const module of modules) if (!paths.has(module) && !excluded.has(module)) err('segment-gap', sourceWhere, `module ${module} is neither a segment nor excluded`);
      for (const path of [...paths, ...excluded.keys()]) if (!modules.includes(path)) err('schema', sourceWhere, `path ${path} is not in the modules inventory`);
    }
  }

  // Claims belong to an existing segment of their own source.
  for (const claim of byId.values()) {
    if (!text(claim.segment)) continue;
    const entry = segmentById.get(claim.segment);
    if (!entry) { err('claim-segment', `claims.json ${claim.id}`, `segment ${claim.segment} is not in coverage.json`); continue; }
    if (entry.source.source_id !== claim.source_id) err('claim-segment', `claims.json ${claim.id}`, `source_id ${claim.source_id} differs from segment source ${entry.source.source_id}`);
    entry.claims.push(claim);
    // AV claim locators use absolute source times inside the covered span (shards included).
    const { span, ranged } = entry.sourceReport;
    if (entry.family !== 'av' || !span) continue;
    for (const [stamp] of String(claim.locator ?? '').matchAll(/\b\d+:\d{2}(?::\d{2})?\b/g)) {
      const at = seconds(stamp);
      if (at < span[0] - 1 || at > span[1] + 1) {
        err('claim-segment', `claims.json ${claim.id}`, `locator time ${stamp} is outside the ${ranged ? 'range' : 'source'} `
          + `${clock(span[0])}-${clock(span[1])}; use absolute source times`);
      }
    }
  }
  const ownClaim = (entry, id, where, rule) => {
    const claim = byId.get(id);
    if (!claim) { err(rule, where, `claim ${id} is not in claims.json`); return null; }
    if (claim.segment !== entry.segment.id) { err(rule, where, `claim ${id} belongs to segment ${claim.segment ?? '(none)'}`); return null; }
    return claim;
  };

  // Ledger: every segment enumerates its concepts, each mapped to a claim or a specific disposition.
  const itemIds = new Set();
  for (const id of Object.keys(ledgerSegments)) if (!segmentById.has(id)) err('ledger', `ledger ${id}`, 'segment is not in coverage.json');
  for (const entry of segmentById.values()) {
    const { segment } = entry; const where = `ledger ${segment.id}`;
    const items = ledgerSegments[segment.id];
    if (!Array.isArray(items) || !items.length) { err('ledger', where, 'no concept ledger for this segment'); continue; }
    const mentioned = new Set();
    for (const [index, item] of items.entries()) {
      const itemWhere = `${where} ${item?.id ?? `#${index}`}`;
      if (!isObj(item)) { err('schema', itemWhere, 'ledger item must be an object'); continue; }
      if (!text(item.id)) err('schema', itemWhere, 'missing id');
      else if (itemIds.has(item.id)) err('schema', itemWhere, 'duplicate ledger item id');
      else itemIds.add(item.id);
      if (!text(item.concept)) err('schema', itemWhere, 'missing concept');
      if (!ITEM_KINDS.includes(item.kind)) err('schema', itemWhere, `kind must be one of ${ITEM_KINDS.join('|')}`);
      if (entry.family === 'av') {
        const at = seconds(item.at);
        if (Number.isNaN(at)) err('schema', itemWhere, 'at must be a timestamp');
        else if (entry.start !== undefined && (at < entry.start || at > entry.end)) err('ledger', itemWhere, `at ${clock(at)} is outside ${entry.label}`);
      } else if (!text(item.at)) err('schema', itemWhere, 'at must locate the item (paragraph, file, post)');
      const maps = ['claims', 'covered', 'out_of_scope'].filter(key => item[key] !== undefined && item[key] !== null);
      if (maps.length !== 1) { err('ledger-unmapped', itemWhere, maps.length ? `maps to ${maps.join(' and ')}; choose one` : 'unmapped: needs claims, covered or out_of_scope'); continue; }
      const context = `${item.concept ?? ''} ${segment.title ?? ''}`;
      if (maps[0] === 'claims') {
        if (!Array.isArray(item.claims) || !item.claims.length) err('ledger-unmapped', itemWhere, 'claims must be a non-empty array');
        else for (const id of item.claims) if (ownClaim(entry, id, itemWhere, 'ledger')) mentioned.add(id);
      } else if (maps[0] === 'covered') {
        if (!isObj(item.covered) || !text(item.covered.ref)) err('ledger-unmapped', itemWhere, 'covered needs the Jet ref (decision, card, spec locator or claim id)');
        const problem = vague(item.covered?.reason, context);
        if (problem) err('reason', `${itemWhere} covered`, problem);
      } else {
        const problem = vague(item.out_of_scope, context);
        if (problem) err('reason', `${itemWhere} out_of_scope`, problem);
      }
      entry.items.push(item);
    }
    for (const claim of entry.claims) if (!mentioned.has(claim.id)) err('ledger', where, `claim ${claim.id} is not traced to any ledger item`);
  }

  // Lane matrix: every segment x lane has claim ids or a segment-specific reason.
  const reasonsSeen = new Map();
  const remember = (reason, segmentId, where) => {
    const key = normal(reason);
    if (!key) return;
    const prior = reasonsSeen.get(key);
    if (prior && prior.segment !== segmentId) err('reason', where, `same reason as ${prior.where}; reasons must be segment-specific`);
    else if (!prior) reasonsSeen.set(key, { segment: segmentId, where });
  };
  for (const entry of segmentById.values()) {
    const { segment, source } = entry;
    const lanes = isObj(segment.lanes) ? segment.lanes : {};
    if (!isObj(segment.lanes)) err('lane', `segment ${segment.id}`, 'missing lanes matrix');
    const context = [segment.title, segment.section, segment.path, ...entry.items.map(item => item.concept)].filter(text).join(' ');
    for (const key of Object.keys(lanes)) if (!LANES.includes(key)) err('lane', `segment ${segment.id}`, `unknown lane ${key}`);
    for (const lane of LANES) {
      const where = `segment ${segment.id} lane ${lane}`;
      const row = lanes[lane];
      if (row === undefined) {
        if (!(isObj(source.lanes_not_run) && lane in source.lanes_not_run)) err('lane', where, 'missing row');
        continue;
      }
      const hasClaims = isObj(row) && Array.isArray(row.claims) && row.claims.length > 0;
      const hasReason = isObj(row) && row.reason !== undefined;
      if (hasClaims === hasReason) { err('lane', where, 'row needs non-empty claims or a reason, not both'); continue; }
      if (hasClaims) for (const id of row.claims) ownClaim(entry, id, where, 'lane');
      else {
        const problem = vague(row.reason, context);
        if (problem) err('reason', where, problem); else remember(row.reason, segment.id, where);
      }
    }
    for (const claim of entry.claims) {
      const row = lanes[claim.lane];
      if (!(isObj(row) && Array.isArray(row.claims) && row.claims.includes(claim.id))) err('lane', `segment ${segment.id} lane ${claim.lane}`, `claim ${claim.id} is missing from its own lane row`);
    }

    // Density: flag thin segments; each needs a written, specific justification.
    const floor = entry.family === 'av' ? Math.max(1, Math.ceil((entry.size / 60) * FLOORS.avItemsPerMinute))
      : entry.family === 'text' ? Math.max(1, Math.ceil(entry.size / FLOORS.textWordsPerItem))
        : FLOORS.repoItems;
    const low = entry.items.length < floor || entry.claims.length < FLOORS.claims;
    let status = 'ok';
    if (low) {
      const problem = segment.density_justification === undefined ? 'low density needs density_justification'
        : vague(segment.density_justification, context);
      if (problem) { status = 'LOW, unjustified'; err('density', `segment ${segment.id}`, `${entry.items.length} ledger items / ${entry.claims.length} claims (floor ${floor} / ${FLOORS.claims}): ${problem}`); }
      else { status = 'LOW, justified'; remember(segment.density_justification, segment.id, `segment ${segment.id} density_justification`); }
    }
    const { sourceReport } = entry;
    sourceReport.size += entry.size; sourceReport.items += entry.items.length; sourceReport.claims += entry.claims.length;
    for (const claim of entry.claims) if (claim.scale === 'micro' || claim.scale === 'macro') sourceReport[claim.scale]++;
    sourceReport.segments.push({ id: segment.id, label: entry.label, size: entry.size, items: entry.items.length,
      claims: entry.claims.length, floor, status });
  }

  // Scale: every source yields micro and macro claims, or says why not.
  for (const sourceReport of report.sources) {
    const source = sourceOf.get(sourceReport);
    if (!sourceReport.segments.length || (sourceReport.micro && sourceReport.macro)) continue;
    const missing = [!sourceReport.micro && 'micro', !sourceReport.macro && 'macro'].filter(Boolean).join(' and ');
    const context = [source.title, ...(source.segments ?? []).map(segment => segment?.title)].filter(text).join(' ');
    const problem = source.scale_justification === undefined ? 'needs scale_justification' : vague(source.scale_justification, context);
    if (problem) err('scale', `source ${source.source_id}`, `no ${missing} claims: ${problem}`);
  }

  // Recall audit by the fresh-context reviewer.
  if (requireAudit) {
    if (audit === undefined) err('recall', 'recall-audit.json', 'missing');
    else if (!isObj(audit) || audit.schema !== SCHEMAS.audit) err('recall', 'recall-audit.json', `schema must be "${SCHEMAS.audit}"`);
    else {
      if (!text(audit.reviewer)) err('recall', 'recall-audit.json', 'missing reviewer identity');
      if (!text(audit.seed)) err('recall', 'recall-audit.json', 'missing seed');
      const audited = new Map((Array.isArray(audit.segments) ? audit.segments : []).map(row => [row?.segment, row]));
      report.audit = { seed: audit.seed, segments: [] };
      for (const { segments } of sample(coverage, audit.seed ?? '')) {
        for (const id of segments) {
          const where = `recall-audit ${id}`;
          const row = audited.get(id); const entry = segmentById.get(id);
          if (!row) { err('recall', where, 'sampled segment was not re-mined'); continue; }
          const concepts = Array.isArray(row.concepts) ? row.concepts : [];
          if (!concepts.length) err('recall', where, 'reviewer listed no independently mined concepts');
          const ledgerIds = new Set((entry?.items ?? []).map(item => item.id));
          let missed = 0;
          for (const concept of concepts) {
            if (!text(concept?.concept)) { err('recall', where, 'concept entries need concept text'); continue; }
            if (concept.ledger === null || concept.ledger === undefined) { missed++; err('recall', where, `missed concept at ${concept.at ?? '?'}: ${concept.concept}; batch returns for re-mining`); }
            else if (!ledgerIds.has(concept.ledger)) err('recall', where, `concept matched to unknown ledger item ${concept.ledger}`);
          }
          for (const dispute of Array.isArray(row.disputes) ? row.disputes : []) {
            err('recall', where, `disputed mapping of ${dispute?.ledger ?? '?'}: ${dispute?.reason ?? 'no reason'}; batch returns for re-mining`);
          }
          report.audit.segments.push({ id, concepts: concepts.length, missed, ledger: ledgerIds.size });
        }
      }
    }
  }

  return { errors, report };
}

const per = (count, size, family) => {
  if (!size) return '-';
  if (family === 'av') return `${(count / (size / 60)).toFixed(2)}/min`;
  if (family === 'text') return `${((count * 1000) / size).toFixed(1)}/1k words`;
  return `${(count / size).toFixed(1)}/module`;
};
const sizeLabel = (size, family) => (family === 'av' ? `${(size / 60).toFixed(1)} min` : family === 'text' ? `${size} words` : `${size} modules`);

// perRule limits how many errors of each rule are listed (the counts stay complete).
export function render({ errors, report }, { perRule = Infinity } = {}) {
  const lines = [];
  for (const source of report.sources) {
    const range = source.ranged && source.span ? `range ${clock(source.span[0])}-${clock(source.span[1])}, ` : '';
    lines.push(`source ${source.source_id} (${source.kind}, ${range}${sizeLabel(source.size, source.family)}, ${source.segments.length} segments): `
      + `ledger ${source.items} (${per(source.items, source.size, source.family)}), claims ${source.claims} `
      + `(${per(source.claims, source.size, source.family)}), micro ${source.micro} / macro ${source.macro}`);
    for (const segment of source.segments) {
      lines.push(`  ${segment.id} ${segment.label}: ledger ${segment.items} (${per(segment.items, segment.size, source.family)}), `
        + `claims ${segment.claims}, floor ${segment.floor} - ${segment.status}`);
    }
  }
  if (report.audit) {
    lines.push(`recall audit (seed ${JSON.stringify(report.audit.seed)}):`);
    for (const row of report.audit.segments) lines.push(`  ${row.id}: reviewer ${row.concepts} concepts, missed ${row.missed}, ledger ${row.ledger}`);
  }
  if (errors.length) {
    const byRule = new Map();
    for (const error of errors) byRule.set(error.rule, [...(byRule.get(error.rule) ?? []), error]);
    lines.push(`errors (${errors.length}):`);
    for (const [rule, list] of byRule) {
      for (const error of list.slice(0, perRule)) lines.push(`  [${rule}] ${error.where}: ${error.message}`);
      if (list.length > perRule) lines.push(`  [${rule}] ... ${list.length - perRule} more`);
    }
    lines.push(`by rule: ${[...byRule].map(([rule, list]) => `${rule} ${list.length}`).join(', ')}`);
    lines.push('FAIL');
  } else lines.push('PASS');
  return `${lines.join('\n')}\n`;
}

// Joins shard run directories: same-source shards become one source (segments sorted, range dropped
// when the shards tile the whole duration); ledgers and claims are concatenated. Conflicts are errors.
export function merge(shards) {
  const errors = [];
  const sources = new Map(); const ledger = {}; const claims = []; const meta = {};
  for (const { dir, coverage, ledger: shardLedger, claims: shardClaims } of shards) {
    if (!isObj(coverage) || !Array.isArray(coverage.sources)) { errors.push(`${dir}: coverage.json missing or has no sources`); continue; }
    for (const source of coverage.sources) {
      const prior = sources.get(source?.source_id);
      if (!prior) { sources.set(source?.source_id, { ...source, segments: [...(source.segments ?? [])], shards: [{ dir, range: source.range }] }); continue; }
      for (const field of ['kind', 'title', 'ref', 'duration_s', 'words']) {
        if (JSON.stringify(prior[field]) !== JSON.stringify(source[field])) errors.push(`${dir}: source ${source.source_id} ${field} differs from ${prior.shards[0].dir}`);
      }
      if (prior.shards.some(shard => shard.range === undefined) || source.range === undefined) errors.push(`${dir}: source ${source.source_id} appears in several shards, so every shard needs a range`);
      prior.segments.push(...(source.segments ?? []));
      prior.lanes_not_run = { ...(source.lanes_not_run ?? {}), ...(prior.lanes_not_run ?? {}) };
      const justifications = [prior.scale_justification, source.scale_justification].filter(text);
      if (justifications.length) prior.scale_justification = justifications.join(' ');
      prior.shards.push({ dir, range: source.range });
    }
    if (shardLedger !== undefined) {
      if (!isObj(shardLedger?.segments)) errors.push(`${dir}: ledger.json has no segments map`);
      else for (const [id, items] of Object.entries(shardLedger.segments)) {
        if (id in ledger) errors.push(`${dir}: ledger segment ${id} is already in another shard`); else ledger[id] = items;
      }
    }
    if (shardClaims !== undefined) {
      const list = Array.isArray(shardClaims) ? shardClaims : shardClaims?.claims;
      if (!Array.isArray(list)) errors.push(`${dir}: claims.json is not an array or {"claims": [...]}`); else claims.push(...list);
      if (isObj(shardClaims)) for (const [key, value] of Object.entries(shardClaims)) if (key !== 'claims' && !(key in meta)) meta[key] = value;
    }
  }
  const merged = [];
  for (const { shards: parts, ...source } of sources.values()) {
    if (parts.length > 1 && FAMILY[source.kind] === 'av') {
      source.segments.sort((a, b) => seconds(a?.start) - seconds(b?.start));
      const range = { start_s: Math.min(...parts.map(part => seconds(part.range?.start_s))),
        end_s: Math.max(...parts.map(part => seconds(part.range?.end_s))) };
      if (range.start_s === 0 && Math.abs(range.end_s - seconds(source.duration_s)) <= 2) delete source.range; else source.range = range;
    } else if (parts.length > 1) errors.push(`source ${source.source_id}: only video/audio sources can be sharded by range`);
    merged.push(source);
  }
  const first = shards.find(shard => isObj(shard.coverage))?.coverage ?? {};
  return { errors, files: {
    coverage: { ...first, schema: SCHEMAS.coverage, merged_from: shards.map(shard => shard.dir), sources: merged },
    ledger: { schema: SCHEMAS.ledger, segments: ledger },
    claims: { ...meta, claims },
  } };
}

const USAGE = 'usage: check_coverage.mjs RUN_DIR [--audit] [--summary] | check_coverage.mjs RUN_DIR --sample SEED\n'
  + '       check_coverage.mjs --merge SHARD_DIR SHARD_DIR... --out RUN_DIR [--summary]';
const readRun = (dir, loadErrors) => name => {
  let raw;
  try { raw = readFileSync(join(dir, name), 'utf8'); } catch (error) { if (error.code !== 'ENOENT') loadErrors.push(`${join(dir, name)}: ${error.message}`); return undefined; }
  try { return JSON.parse(raw); } catch (error) { loadErrors.push(`${join(dir, name)}: invalid JSON: ${error.message}`); return null; }
};
export function main(argv = process.argv.slice(2), out = process.stdout, errOut = process.stderr) {
  let dir = null; let requireAudit = false; let seed = null; let perRule = Infinity; let shardDirs = null; let outDir = null;
  const usage = () => { errOut.write(`${USAGE}\n`); return 2; };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--audit') requireAudit = true;
    else if (argv[i] === '--summary') perRule = 3;
    else if (argv[i] === '--sample') { seed = argv[++i]; if (!text(seed)) return usage(); }
    else if (argv[i] === '--merge') shardDirs = [];
    else if (argv[i] === '--out') { outDir = argv[++i]; if (!text(outDir) || outDir.startsWith('--')) return usage(); }
    else if (argv[i].startsWith('--')) return usage();
    else if (shardDirs) shardDirs.push(argv[i]);
    else if (dir) return usage();
    else dir = argv[i];
  }
  const loadErrors = [];
  if (shardDirs) {
    if (shardDirs.length < 2 || !outDir || dir || seed !== null || requireAudit) return usage();
    const shards = shardDirs.map(shardDir => {
      const load = readRun(shardDir, loadErrors);
      return { dir: shardDir, coverage: load('coverage.json'), ledger: load('ledger.json'), claims: load('claims.json') };
    });
    const { errors, files } = merge(shards);
    errors.unshift(...loadErrors);
    const names = { coverage: 'coverage.json', ledger: 'ledger.json', claims: 'claims.json' };
    for (const name of Object.values(names)) if (existsSync(join(outDir, name))) errors.push(`${join(outDir, name)} already exists; merge never overwrites`);
    if (errors.length) { errOut.write(`check_coverage: merge failed:\n${errors.map(line => `  ${line}`).join('\n')}\n`); return 1; }
    mkdirSync(outDir, { recursive: true });
    for (const [key, name] of Object.entries(names)) writeFileSync(join(outDir, name), `${JSON.stringify(files[key], null, 2)}\n`, { flag: 'wx' });
    out.write(`merged ${shardDirs.length} shards into ${outDir}\n`);
    const result = check(files);
    out.write(render(result, { perRule }));
    return result.errors.length ? 1 : 0;
  }
  if (!dir || outDir) return usage();
  const load = readRun(dir, loadErrors);
  const coverage = load('coverage.json');
  if (seed !== null) {
    if (!isObj(coverage)) { errOut.write('check_coverage: --sample needs a readable coverage.json\n'); return 1; }
    for (const row of sample(coverage, seed)) out.write(`${row.source_id}: ${row.segments.join(' ')}\n`);
    return 0;
  }
  const result = check({ coverage, ledger: load('ledger.json'), claims: load('claims.json'),
    audit: requireAudit ? load('recall-audit.json') : undefined }, { requireAudit });
  for (const message of loadErrors) result.errors.unshift({ rule: 'schema', where: 'load', message });
  out.write(render(result, { perRule }));
  return result.errors.length ? 1 : 0;
}
if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) process.exitCode = main();
