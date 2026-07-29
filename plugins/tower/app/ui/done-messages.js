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

function completionTime(card) {
  return card.completedAt || (isFullTimestamp(card.updated) ? card.updated : null);
}

const isFullTimestamp = value => String(value || '').length > 10;
