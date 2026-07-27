// Now-tab catch-up: collapse every changed card to its current status.

/**
 * @param {object} opts
 * @param {string|null} opts.cursor  ISO digestCursor
 * @param {Array} opts.events        newest-first event log from store
 * @param {Array} opts.cards         live projected cards
 * @param {Array} [opts.decisions]   live decisions
 * @param {Array} [opts.questions]   live questions
 * @returns {{ since: string|null, items: Array<{ at, kind, text, ref? }> }}
 */
export function buildDigestTimeline({ cursor, events = [], cards = [], decisions = [], questions = [] } = {}) {
  if (!cursor) return { since: null, items: [] };

  const byId = new Map(cards.map(c => [c.id, c]));
  const decById = new Map((decisions || []).map(d => [d.id, d]));
  const questionById = new Map((questions || []).map(q => [q.id, q]));

  const recent = (events || [])
    .filter(e => e && e.at > cursor && e.by !== 'owner')
    .slice()
    .sort((a, b) => a.at.localeCompare(b.at));

  const latest = new Map();

  for (const e of recent) {
    let card = byId.get(e.ref);
    if (!card && (e.action?.startsWith('decision.') || e.action === 'clearance'))
      card = byId.get(decById.get(e.ref)?.cardId);
    if (!card && e.action === 'archive.restore')
      card = byId.get(decById.get(e.ref)?.cardId);
    if (!card && e.action?.startsWith('question.'))
      card = byId.get(questionById.get(e.ref)?.cardId);
    if (card) latest.set(card.id, { at: e.at, card });
  }

  // Full timestamps on current cards cover changes aged out of the event window.
  for (const c of cards) {
    if (latest.has(c.id)) continue;
    if (c.updatedBy && c.updatedBy !== 'owner' && isFullTimestamp(c.updated) && c.updated > cursor)
      latest.set(c.id, { at: c.updated, card: c });
  }

  const items = [...latest.values()].map(({ at, card }) => ({
    at,
    kind: card.phase === 'done' ? 'done' : 'changed',
    text: card.phase === 'done'
      ? `#${card.num} ${card.title}`
      : `#${card.num} ${card.title} — ${cardStatus(card)}`,
    ref: `#${card.num}`,
  }));
  items.sort((a, b) => a.at.localeCompare(b.at));
  return { since: cursor, items };
}

const isFullTimestamp = value => String(value || '').length > 10;

function cardStatus(card) {
  if (card.lane?.label) return card.lane.label;
  return {
    deciding: 'Deciding', planning: 'Planning', ready: 'Ready',
    building: 'Building', verify: 'Verify', frozen: 'Frozen',
  }[card.phase] || 'Updated';
}
