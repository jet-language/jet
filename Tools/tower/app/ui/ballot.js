// Pure ballot text/rendering; focus-mode wiring stays in tower.js.
const esc = value => String(value ?? '').replace(/[&<>"]/g,
  char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[char]);

const QUESTION_STATE = { open: '✎ awaiting answer', answered: '✓ question answered' };

export function lossText(item) {
  if (!item || typeof item !== 'object') return item;
  return [item.loss, item.mitigation ? `How we reduce it: ${item.mitigation}` : '']
    .filter(Boolean).join(' — ');
}

// The situation is plain prose: blank lines separate paragraphs and single
// line breaks are soft wraps.
export function renderSituation(situation) {
  const text = typeof situation === 'string' ? situation.trim() : '';
  if (!text) return '';
  const paragraphs = text.split(/\n\s*\n/)
    .map(paragraph => `<p>${esc(paragraph.replace(/\s*\n\s*/g, ' '))}</p>`).join('');
  return `<section class="situation" aria-label="The situation">
    <div class="situation__label">The situation</div>${paragraphs}</section>`;
}

// Every ballot opens with one quiet line naming the ballot and its card, then
// the situation summary. The summary, not a row of chips, introduces the vote.
export function renderBallotIntro(d, card = null, questionState = '') {
  const from = card ? ` · card #${esc(card.num)} · ${esc(card.title)}` : '';
  const question = QUESTION_STATE[questionState]
    ? `<span class="qchip qchip--${questionState}">${QUESTION_STATE[questionState]}</span>` : '';
  return `<div class="fdeck__head"><span class="fdeck__for"><span class="fdeck__id">${esc(d.id)}</span>${from}</span>${question}</div>
    ${renderSituation(d.situation)}`;
}
