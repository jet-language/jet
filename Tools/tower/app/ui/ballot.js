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

// The reports a ballot links to: audits, research and measurements in Tower
// docs (opened in the docs view) or outside sources (opened in a new tab).
// They sit right under the situation, so the evidence is one click away.
export function renderReports(reports) {
  const list = Array.isArray(reports) ? reports.filter(r => r && r.title && (r.path || r.url)) : [];
  if (!list.length) return '';
  const items = list.map(r => {
    const link = r.path
      ? `<a class="reports__link" href="#docs" data-doc-path="${esc(r.path)}">${esc(r.title)}</a><span class="reports__where">${esc(r.path)}</span>`
      : `<a class="reports__link" href="${esc(r.url)}" target="_blank" rel="noopener noreferrer">${esc(r.title)}</a><span class="reports__where">${esc(r.url)}</span>`;
    return `<li>${link}${r.note ? `<div class="reports__note">${esc(r.note)}</div>` : ''}</li>`;
  }).join('');
  return `<section class="reports" aria-label="Reports and data">
    <div class="reports__label">Reports and data</div><ul>${items}</ul></section>`;
}

// Every ballot opens with one quiet line naming the ballot and its card, then
// the situation summary. The summary, not a row of chips, introduces the vote.
export function renderBallotIntro(d, card = null, questionState = '') {
  const from = card ? ` · card #${esc(card.num)} · ${esc(card.title)}` : '';
  const question = QUESTION_STATE[questionState]
    ? `<span class="qchip qchip--${questionState}">${QUESTION_STATE[questionState]}</span>` : '';
  return `<div class="fdeck__head"><span class="fdeck__for"><span class="fdeck__id">${esc(d.id)}</span>${from}</span>${question}</div>
    ${renderSituation(d.situation)}${renderReports(d.reports)}`;
}
