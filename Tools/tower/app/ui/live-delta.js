// Board index deltas, shared by the server (diff) and the browser (apply).
//
// The server projects one compact board index per revision. Instead of
// re-sending that whole index on every agent write, it sends the difference
// from the previous index. A patch is plain JSON:
//
//   { set:  { key: value },                  // top-level keys replaced whole
//     lists: { key: { upsert, remove, order } },   // arrays of {id} records
//     heads: { key: { prepend, length } } }   // newest-first logs (events)
//
// `applyIndexDelta(prev, patch)` returns a new index object; untouched
// records keep their identity, so the client can cheaply tell what moved.

const KEYED = new Set(['cards', 'decisions', 'questions', 'milestones', 'ideas', 'papercuts', 'briefings', 'recentlyDecided', 'radar', 'epochs']);
const HEADS = new Set(['events']);

const jsonCache = new WeakMap();
// Records are immutable once projected, so each one is stringified at most
// once across successive diffs.
const json = (value) => {
  if (value === null || typeof value !== 'object') return JSON.stringify(value);
  let text = jsonCache.get(value);
  if (text === undefined) { text = JSON.stringify(value); jsonCache.set(value, text); }
  return text;
};

// Keyed diffs need unique string ids; anything else is replaced whole.
const keyedArray = (value) => Array.isArray(value)
  && value.every(item => item && typeof item === 'object' && typeof item.id === 'string')
  && new Set(value.map(item => item.id)).size === value.length;

function diffKeyed(prev, next) {
  const before = new Map(prev.map(item => [item.id, item]));
  const nextIds = new Set();
  const upsert = [];
  for (const item of next) {
    nextIds.add(item.id);
    const old = before.get(item.id);
    if (!old || json(old) !== json(item)) upsert.push(item);
  }
  const remove = prev.filter(item => !nextIds.has(item.id)).map(item => item.id);
  // Order the client would reach on its own: survivors in old order, then
  // brand-new records appended. Send the full order only when that is wrong.
  const expected = prev.filter(item => nextIds.has(item.id)).map(item => item.id);
  for (const item of next) if (!before.has(item.id)) expected.push(item.id);
  const reordered = expected.length !== next.length || expected.some((id, i) => id !== next[i].id);
  if (!upsert.length && !remove.length && !reordered) return null;
  const patch = {};
  if (upsert.length) patch.upsert = upsert;
  if (remove.length) patch.remove = remove;
  if (reordered) patch.order = next.map(item => item.id);
  return patch;
}

// Newest-first, length-capped logs: the common change is "some entries were
// added at the head and the oldest fell off the tail".
function diffHead(prev, next) {
  if (!prev.length) return next.length ? { prepend: next, length: next.length } : null;
  const anchor = json(prev[0]);
  const at = next.findIndex(item => json(item) === anchor);
  if (at < 0) return undefined;
  const kept = next.length - at;
  for (let i = 0; i < kept; i++) if (i >= prev.length || json(prev[i]) !== json(next[at + i])) return undefined;
  if (at === 0 && kept === prev.length) return null;
  return { prepend: next.slice(0, at), length: next.length };
}

export function diffIndex(prev, next) {
  const patch = {};
  const set = {};
  const lists = {};
  const heads = {};
  for (const key of new Set([...Object.keys(prev || {}), ...Object.keys(next || {})])) {
    const a = prev?.[key];
    const b = next?.[key];
    if (a === b) continue;
    if (KEYED.has(key) && keyedArray(a) && keyedArray(b)) {
      const d = diffKeyed(a, b);
      if (d) lists[key] = d;
      continue;
    }
    if (HEADS.has(key) && Array.isArray(a) && Array.isArray(b)) {
      const d = diffHead(a, b);
      if (d === null) continue;
      if (d) { heads[key] = d; continue; }
    }
    if (!(key in (next || {}))) { set[key] = null; continue; }
    if (json(a) !== json(b)) set[key] = b;
  }
  if (Object.keys(set).length) patch.set = set;
  if (Object.keys(lists).length) patch.lists = lists;
  if (Object.keys(heads).length) patch.heads = heads;
  return patch;
}

export function applyIndexDelta(prev, patch) {
  const next = { ...prev };
  for (const [key, value] of Object.entries(patch.set || {})) {
    if (value === null) delete next[key];
    else next[key] = value;
  }
  for (const [key, d] of Object.entries(patch.lists || {})) {
    const byId = new Map((prev[key] || []).map(item => [item.id, item]));
    for (const id of d.remove || []) byId.delete(id);
    for (const item of d.upsert || []) byId.set(item.id, item);
    next[key] = d.order ? d.order.map(id => byId.get(id)).filter(Boolean) : [...byId.values()];
  }
  for (const [key, d] of Object.entries(patch.heads || {})) {
    next[key] = [...(d.prepend || []), ...(prev[key] || [])].slice(0, d.length);
  }
  return next;
}

// Which top-level sections a patch touched — lets the client skip work.
export function touchedKeys(patch) {
  return new Set([...Object.keys(patch.set || {}), ...Object.keys(patch.lists || {}), ...Object.keys(patch.heads || {})]);
}
