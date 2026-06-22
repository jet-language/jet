// Minimal markdown -> HTML + a small multi-language syntax highlighter.
// Std-only; deliberately tiny. Handles: headings, paragraphs, lists, tables,
// fenced code (highlighted), inline code/bold/italic.

export function escapeHtml(s) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

export function inline(s) {
  return escapeHtml(s)
    .replace(/`([^`]+)`/g, (_, c) => `<code>${c}</code>`)
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/\*([^*]+)\*/g, "<em>$1</em>");
}

export function renderMd(md) {
  const lines = (md || "").split("\n");
  const out = [];
  let i = 0;
  const para = [];
  const flushPara = () => { if (para.length) { out.push(`<p>${inline(para.join(" "))}</p>`); para.length = 0; } };
  while (i < lines.length) {
    const line = lines[i];
    const fence = line.match(/^(\s*)```(\w+)?\s*$/);
    if (fence) {
      flushPara();
      const indent = fence[1].length, lang = fence[2] || "", buf = [];
      i++;
      while (i < lines.length && !/^\s*```/.test(lines[i])) { buf.push(lines[i].slice(indent)); i++; }
      i++;
      out.push(`<pre class="code" data-lang="${escapeHtml(lang)}"><code>${highlight(buf.join("\n"), lang)}</code></pre>`);
      continue;
    }
    if (line.trim().startsWith("|") && i + 1 < lines.length && /^\s*\|?\s*:?-{2,}/.test(lines[i + 1])) {
      flushPara();
      const rows = [];
      while (i < lines.length && lines[i].trim().startsWith("|")) { rows.push(lines[i]); i++; }
      out.push(renderTable(rows));
      continue;
    }
    if (/^\s*-\s+/.test(line)) {
      flushPara();
      const items = [];
      while (i < lines.length && /^\s*-\s+/.test(lines[i])) { items.push(`<li>${inline(lines[i].replace(/^\s*-\s+/, ""))}</li>`); i++; }
      out.push(`<ul>${items.join("")}</ul>`);
      continue;
    }
    const h = line.match(/^(#{2,6})\s+(.+)$/);
    if (h) { flushPara(); out.push(`<h4>${inline(h[2])}</h4>`); i++; continue; }
    if (line.trim() === "") { flushPara(); i++; continue; }
    para.push(line.trim());
    i++;
  }
  flushPara();
  return out.join("\n");
}

export function renderTable(rows) {
  const cells = (r) => r.trim().replace(/^\|/, "").replace(/\|$/, "").split("|").map((c) => c.trim());
  const head = cells(rows[0]);
  const body = rows.slice(2).map(cells);
  const th = head.map((c) => `<th>${inline(c)}</th>`).join("");
  const trs = body.map((r) => `<tr>${r.map((c) => `<td>${inline(c)}</td>`).join("")}</tr>`).join("");
  return `<table><thead><tr>${th}</tr></thead><tbody>${trs}</tbody></table>`;
}

// ---- syntax highlighter (multi-language) -----------------------------------

const KEYWORDS = new Set(
  ("fn struct enum trait impl use module pub return self mut take const if else loop break continue " +
   "for in while match when new init derive let var val error ok value view edit share copy " +
   "func type package import range defer go chan map interface " +
   "comptime class extension protocol guard def async await yield where").split(/\s+/)
);

export function highlight(code) { return code.split("\n").map(highlightLine).join("\n"); }

function highlightLine(line) {
  let comment = "";
  const cidx = line.indexOf("//");
  let codePart = line;
  if (cidx >= 0) { codePart = line.slice(0, cidx); comment = line.slice(cidx); }
  let html = "";
  const re = /("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')/g;
  let last = 0, m;
  while ((m = re.exec(codePart)) !== null) {
    html += highlightCode(codePart.slice(last, m.index));
    html += `<span class="s">${escapeHtml(m[0])}</span>`;
    last = m.index + m[0].length;
  }
  html += highlightCode(codePart.slice(last));
  if (comment) html += `<span class="c">${escapeHtml(comment)}</span>`;
  return html;
}

function highlightCode(s) {
  return escapeHtml(s)
    .replace(/\b([A-Za-z_][A-Za-z0-9_]*)\b/g, (w) =>
      KEYWORDS.has(w) ? `<span class="k">${w}</span>`
      : /^[A-Z]/.test(w) ? `<span class="t">${w}</span>` : w)
    .replace(/\b(\d+\.?\d*)\b/g, '<span class="n">$1</span>');
}
