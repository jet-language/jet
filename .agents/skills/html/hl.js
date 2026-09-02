// html skill — code highlighter. Paste inside the page's single <script>; the same tokenizer and classes as Tower's board (plugins/tower/app/ui/tower.js).
// Usage: pre.innerHTML = hl(sourceText)   or   `<pre>${hl(src)}</pre>`. Classes: hl-k keyword, hl-s string, hl-n number, hl-c comment, hl-f call, hl-t type or sigil.
const HL_ESC = (s) => String(s ?? '').replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const HL_KW = new Set(('fn func function def let var const val mut return yield if elif else match switch case when default for while loop do in of as is import use mod module package from pub priv private public protected internal static struct enum trait impl interface type class extends implements where async await comptime defer go chan select new self this super sizeof typeof null nil none None true false True False and or not break continue throw try catch finally with lambda then begin end macro derive emit').split(' '));
function hl(src) {
  const re = /(\/\/[^\n]*|\/\*[\s\S]*?\*\/|#\s[^\n]*|--\s[^\n]*)|([#@][A-Za-z_]\w*)|("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`)|(0[xX][0-9a-fA-F_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)|([A-Za-z_$]\w*)|(\s+)|([\s\S])/g;
  let m, out = '';
  while ((m = re.exec(src))) {
    if (m[1]) out += `<span class="hl-c">${HL_ESC(m[1])}</span>`;
    else if (m[2]) out += `<span class="hl-t">${HL_ESC(m[2])}</span>`;
    else if (m[3]) out += `<span class="hl-s">${HL_ESC(m[3])}</span>`;
    else if (m[4]) out += `<span class="hl-n">${HL_ESC(m[4])}</span>`;
    else if (m[5]) {
      const w = m[5], next = src[re.lastIndex];
      if (HL_KW.has(w)) out += `<span class="hl-k">${HL_ESC(w)}</span>`;
      else if (next === '(') out += `<span class="hl-f">${HL_ESC(w)}</span>`;
      else if (/^[A-Z]/.test(w)) out += `<span class="hl-t">${HL_ESC(w)}</span>`;
      else out += HL_ESC(w);
    } else if (m[6]) out += m[6];
    else out += HL_ESC(m[7]);
  }
  return out;
}
