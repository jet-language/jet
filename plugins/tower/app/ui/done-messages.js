// Now-tab queue: completed cards since the last clear plus durable messages.

export function buildDoneMessageQueue({ cursor, cards = [], questions = [] } = {}) {
  const cardById = new Map(cards.map(card => [card.id, card]));
  const done = cursor ? cards
    .filter(card => card.phase === 'done')
    .map(card => ({ card, at: completionTime(card) }))
    .filter(item => item.at && item.at > cursor)
    .map(({ card, at }) => ({
      at,
      cardId: card.id,
      kind: 'done',
      marker: 'DONE',
      ref: `#${card.num}`,
      text: `#${card.num} ${card.title}`,
    }))
    .sort((a, b) => a.at.localeCompare(b.at)) : [];

  const messages = questions
    .filter(note => note.kind === 'message' && note.status === 'open')
    .map(note => ({ note, card: cardById.get(note.cardId) }))
    .filter(item => item.card)
    .map(({ note, card }) => ({
      at: note.created,
      by: note.by,
      cardId: card.id,
      id: note.id,
      kind: 'message',
      marker: 'MESSAGE',
      ref: `#${card.num}`,
      text: note.text,
      title: card.title,
    }))
    .sort((a, b) => a.at.localeCompare(b.at));

  return { since: cursor || null, done, messages };
}

export function renderDoneMessageQueue({ done = [], messages = [], since = null } = {}) {
  if (!done.length && !messages.length) return '';
  const doneRows = done.map(item => `<button class="queue__row" data-card="${escapeHtml(item.cardId)}">
      <span class="queue__marker queue__marker--done">${escapeHtml(item.marker)}</span>
      <span>${escapeHtml(item.text)}</span>
    </button>`).join('');
  const messageRows = messages.map(item => `<div class="queue__row queue__row--message">
      <span class="queue__marker queue__marker--message">${escapeHtml(item.marker)}</span>
      <button class="queue__message" data-card="${escapeHtml(item.cardId)}">
        <b>${escapeHtml(item.ref)} ${escapeHtml(item.title)}</b><span>${escapeHtml(item.text)}</span>
      </button>
      <button class="btn btn--sm" data-message-done="${escapeHtml(item.id)}">Done</button>
    </div>`).join('');
  const sinceText = since
    ? ` since ${new Date(since).toLocaleString([], {
      month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
    })}`
    : '';
  const countText = [
    done.length ? `<span class="queue__done-count">${done.length} done</span>${sinceText}` : '',
    `${messages.length} message${messages.length === 1 ? '' : 's'}`,
  ].filter(Boolean).join(' · ');
  return `<div class="queue">
      <div class="queue__head">
        <div class="queue__h"><span class="queue__signal" aria-hidden="true">✦</span>Done &amp; messages</div>
        <span class="queue__count">${countText}</span>
        ${done.length ? '<button class="btn btn--sm" data-clear-done>Clear done cards</button>' : ''}
      </div>
      ${done.length ? `<div class="queue__section"><div class="queue__label">Completed</div>${doneRows}</div>` : ''}
      ${messages.length ? `<div class="queue__section"><div class="queue__label">Messages</div>${messageRows}</div>` : ''}
    </div>`;
}

function completionTime(card) {
  return card.completedAt || (isFullTimestamp(card.updated) ? card.updated : null);
}

const isFullTimestamp = value => String(value || '').length > 10;
const escapeHtml = value => String(value ?? '').replace(
  /[&<>"]/g,
  char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[char],
);
