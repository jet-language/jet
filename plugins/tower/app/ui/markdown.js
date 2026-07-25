// Small GFM-ish markdown → HTML renderer. Zero deps. Source is always truth.
export function renderMarkdown(src) {
  const text = String(src ?? '').replace(/\r\n/g, '\n');
  const parts = text.split(/(```[\s\S]*?```)/);
  let html = '';
  for (const part of parts) {
    if (part.startsWith('```')) {
      const m = /^```(\w*)\n?([\s\S]*?)```$/.exec(part);
      const code = esc((m ? m[2] : part.slice(3)).replace(/\n$/, ''));
      html += `<pre class="md__pre"><code>${code}</code></pre>`;
      continue;
    }
    html += renderBlocks(part);
  }
  return html;
}

function renderBlocks(src) {
  const lines = src.split('\n');
  const out = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) { i++; continue; }
    if (/^---+$/.test(line.trim())) { out.push('<hr class="md__hr">'); i++; continue; }

    const hm = /^(#{1,6})\s+(.*)$/.exec(line);
    if (hm) {
      const n = hm[1].length;
      out.push(`<h${n} class="md__h md__h${n}">${inline(hm[2])}</h${n}>`);
      i++;
      continue;
    }

    if (/^>\s?/.test(line)) {
      const chunk = [];
      while (i < lines.length && /^>\s?/.test(lines[i])) {
        chunk.push(lines[i].replace(/^>\s?/, ''));
        i++;
      }
      out.push(`<blockquote class="md__bq">${inline(chunk.join(' '))}</blockquote>`);
      continue;
    }

    if (line.includes('|') && i + 1 < lines.length && isTableSep(lines[i + 1])) {
      const tableLines = [];
      while (i < lines.length && lines[i].includes('|')) {
        tableLines.push(lines[i]);
        i++;
      }
      out.push(renderTable(tableLines));
      continue;
    }

    if (/^([-*+]|\d+\.)\s/.test(line)) {
      const ordered = /^\d+\.\s/.test(line);
      const items = [];
      while (i < lines.length && /^([-*+]|\d+\.)\s/.test(lines[i])) {
        items.push(lines[i].replace(/^([-*+]|\d+\.)\s+/, ''));
        i++;
      }
      const tag = ordered ? 'ol' : 'ul';
      out.push(`<${tag} class="md__list">${items.map(t => `<li>${inline(t)}</li>`).join('')}</${tag}>`);
      continue;
    }

    const para = [];
    while (i < lines.length && lines[i].trim()
      && !/^#{1,6}\s/.test(lines[i])
      && !/^>\s?/.test(lines[i])
      && !/^([-*+]|\d+\.)\s/.test(lines[i])
      && !/^---+$/.test(lines[i].trim())
      && !(lines[i].includes('|') && i + 1 < lines.length && isTableSep(lines[i + 1]))) {
      para.push(lines[i]);
      i++;
    }
    if (para.length) out.push(`<p class="md__p">${inline(para.join('\n')).replace(/\n/g, '<br>')}</p>`);
    else i++;
  }
  return out.join('');
}

function isTableSep(line) {
  return /^\s*\|?[\s:\-|]+\|[\s:\-|]*\|?\s*$/.test(line) && /:-|-:|---/.test(line);
}

function splitRow(line) {
  let cells = line.split('|').map(c => c.trim());
  if (cells[0] === '') cells = cells.slice(1);
  if (cells.length && cells[cells.length - 1] === '') cells = cells.slice(0, -1);
  return cells;
}

function renderTable(lines) {
  if (lines.length < 2) return `<p class="md__p">${inline(lines.join(' '))}</p>`;
  const head = splitRow(lines[0]);
  const body = lines.slice(2).map(splitRow);
  return `<table class="md__table"><thead><tr>${head.map(c => `<th>${inline(c)}</th>`).join('')}</tr></thead>`
    + `<tbody>${body.map(r => `<tr>${r.map(c => `<td>${inline(c)}</td>`).join('')}</tr>`).join('')}</tbody></table>`;
}

function inline(s) {
  let t = esc(s);
  t = t.replace(/`([^`]+)`/g, '<code class="md__code">$1</code>');
  t = t.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  t = t.replace(/(?<!\*)\*([^*]+)\*(?!\*)/g, '<em>$1</em>');
  t = t.replace(/\[([^\]]+)\]\((https?:[^)\s]+)\)/g, '<a class="md__a" href="$2" target="_blank" rel="noopener">$1</a>');
  return t;
}

function esc(s) {
  return String(s ?? '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

/** Split markdown into top-level source blocks for Typora-style editing. */
export function splitBlocks(src) {
  const text = String(src ?? '').replace(/\r\n/g, '\n');
  if (!text.trim()) return [text];
  const blocks = [];
  const lines = text.split('\n');
  let i = 0;
  while (i < lines.length) {
    if (!lines[i].trim()) { i++; continue; }
    if (lines[i].startsWith('```')) {
      const start = i;
      i++;
      while (i < lines.length && !lines[i].startsWith('```')) i++;
      if (i < lines.length) i++;
      blocks.push(lines.slice(start, i).join('\n'));
      continue;
    }
    if (/^#{1,6}\s/.test(lines[i]) || /^---+$/.test(lines[i].trim())) {
      blocks.push(lines[i]);
      i++;
      continue;
    }
    if (/^>\s?/.test(lines[i])) {
      const start = i;
      i++;
      while (i < lines.length && /^>\s?/.test(lines[i])) i++;
      blocks.push(lines.slice(start, i).join('\n'));
      continue;
    }
    if (/^([-*+]|\d+\.)\s/.test(lines[i])) {
      const start = i;
      i++;
      while (i < lines.length && /^([-*+]|\d+\.)\s/.test(lines[i])) i++;
      blocks.push(lines.slice(start, i).join('\n'));
      continue;
    }
    if (lines[i].includes('|') && i + 1 < lines.length && isTableSep(lines[i + 1])) {
      const start = i;
      i += 2;
      while (i < lines.length && lines[i].includes('|')) i++;
      blocks.push(lines.slice(start, i).join('\n'));
      continue;
    }
    const start = i;
    i++;
    while (i < lines.length && lines[i].trim()
      && !/^#{1,6}\s/.test(lines[i])
      && !/^```/.test(lines[i])
      && !/^>\s?/.test(lines[i])
      && !/^([-*+]|\d+\.)\s/.test(lines[i])
      && !/^---+$/.test(lines[i].trim())
      && !(lines[i].includes('|') && i + 1 < lines.length && isTableSep(lines[i + 1]))) {
      i++;
    }
    blocks.push(lines.slice(start, i).join('\n'));
  }
  return blocks.length ? blocks : [''];
}
