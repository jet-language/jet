// Now-tab activity digest: turn board events since digestCursor into a
// human timeline (no agent names). Pure helper — UI just renders.

const FIELD_NOTE_RE = /^(phase|title|body|plan|priority|blockedBy|workOrder|criteria|needsAcceptance|refs|logEntry|track|epoch|milestoneId|kind)(,(phase|title|body|plan|priority|blockedBy|workOrder|criteria|needsAcceptance|refs|logEntry|track|epoch|milestoneId|kind))*$/;

/**
 * @param {object} opts
 * @param {string|null} opts.cursor  ISO digestCursor
 * @param {Array} opts.events        newest-first event log from store
 * @param {Array} opts.cards         live projected cards (with .log, .num, .title, .id, .phase)
 * @param {Array} [opts.decisions]   live decisions
 * @returns {{ since: string|null, items: Array<{ at, kind, text, ref? }> }}
 */
export function buildDigestTimeline({ cursor, events = [], cards = [], decisions = [] } = {}) {
  if (!cursor) return { since: null, items: [] };

  const byId = new Map(cards.map(c => [c.id, c]));
  const byNum = new Map(cards.map(c => [c.num, c]));
  const decById = new Map((decisions || []).map(d => [d.id, d]));

  const recent = (events || [])
    .filter(e => e && e.at > cursor && e.by !== 'owner')
    .slice()
    .sort((a, b) => a.at.localeCompare(b.at));

  const items = [];
  const seenDone = new Set();

  for (const e of recent) {
    const at = e.at;
    if (e.action === 'card.update' || e.action === 'card.add') {
      const card = byId.get(e.ref) || null;
      const why = oneLineWhy(e, card);
      const label = cardLabel(card, e.ref);
      if (card?.phase === 'done' || /→\s*done|phase.*done|closed|Verified — closed|Accepted — closed/i.test(String(e.note || '') + ' ' + why)) {
        const key = card?.num ?? e.ref;
        if (!seenDone.has(key) && (card?.phase === 'done' || /done|closed/i.test(why))) {
          seenDone.add(key);
          items.push({ at, kind: 'done', text: `Done · ${label}`, ref: card ? `#${card.num}` : e.ref });
          continue;
        }
      }
      if (e.action === 'card.add') {
        items.push({ at, kind: 'changed', text: `Added · ${label}`, ref: card ? `#${card.num}` : e.ref });
        continue;
      }
      items.push({
        at,
        kind: 'changed',
        text: why ? `Updated · ${label} — ${why}` : `Updated · ${label}`,
        ref: card ? `#${card.num}` : e.ref,
      });
      continue;
    }
    if (e.action === 'decision.add') {
      const d = decById.get(e.ref);
      const title = d?.title || e.note || e.ref;
      items.push({ at, kind: 'ballot', text: `Ballot · ${e.ref} — ${clip(title, 80)}`, ref: e.ref });
      continue;
    }
    if (e.action === 'decision.ratify' || e.action === 'clearance') {
      const d = decById.get(e.ref);
      const title = d?.title || e.note || e.ref;
      const outcome = d?.outcome ? ` → ${d.outcome}` : '';
      items.push({ at, kind: 'ballot', text: `Ratified · ${e.ref}${outcome} — ${clip(title, 60)}`, ref: e.ref });
      continue;
    }
    if (e.action === 'question.answer') {
      items.push({ at, kind: 'changed', text: `Question answered · ${e.ref}`, ref: e.ref });
      continue;
    }
    if (e.action === 'card.criteria-meet' || e.action === 'card.criteria-verify' || e.action === 'card.criteria-add') {
      const card = byId.get(e.ref);
      const label = cardLabel(card, e.ref);
      const verb = e.action === 'card.criteria-verify' ? 'Criterion verified'
        : e.action === 'card.criteria-meet' ? 'Criterion met' : 'Criterion added';
      items.push({ at, kind: 'changed', text: `${verb} · ${label}${e.note ? ` — ${clip(e.note, 60)}` : ''}`, ref: card ? `#${card.num}` : e.ref });
    }
  }

  // Fallback: done cards touched after cursor when the event note was field-list only.
  // Compare against the full cursor (not YYYY-MM-DD) so Dismiss → now() clears the strip.
  for (const c of cards) {
    if (c.phase !== 'done') continue;
    const key = c.num;
    if (seenDone.has(key)) continue;
    if (!(c.updated && c.updated > cursor)) continue;
    const logHit = (c.log || []).find(l => l.at && logAtIso(l.at) > cursor);
    const at = logHit ? logAtIso(logHit.at) : c.updated;
    seenDone.add(key);
    items.push({
      at,
      kind: 'done',
      text: `Done · #${c.num} ${c.title}`,
      ref: `#${c.num}`,
    });
  }

  items.sort((a, b) => a.at.localeCompare(b.at));
  return { since: cursor, items };
}

/** Normalize card log timestamps: bare YYYY-MM-DD → start of that UTC day. */
function logAtIso(at) {
  const s = String(at || '');
  if (!s) return '';
  return s.length <= 10 ? `${s}T00:00:00.000Z` : s;
}

function cardLabel(card, ref) {
  if (card) return `#${card.num} ${card.title || ''}`.trim();
  return ref || 'card';
}

function oneLineWhy(event, card) {
  const note = String(event.note || '').trim();
  if (note && !FIELD_NOTE_RE.test(note)) return clip(note, 100);
  // Prefer newest log line that looks like prose.
  for (const l of (card?.log || [])) {
    const t = String(l.text || '').trim();
    if (t && !FIELD_NOTE_RE.test(t)) return clip(t, 100);
  }
  if (note) return clip(note.replace(/,/g, ', '), 80);
  return '';
}

function clip(s, n) {
  const t = String(s || '').replace(/\s+/g, ' ').trim();
  return t.length <= n ? t : t.slice(0, n - 1) + '…';
}

/** Detect phase→done from a card + events (exported for tests). */
export function isFieldListNote(note) {
  return FIELD_NOTE_RE.test(String(note || '').trim());
}
