# HTML visual identity and page mechanics

Read this file before implementing any HTML page. This is the authority for the identity. Do not replace it with a generic template.

## Canonical assets

- `theme.css` is the canonical CSS block. Paste it inline at the top of the page's single `<style>` block.
- `fonts.css` follows `theme.css` in that same `<style>` block. Its fonts are embedded as data URIs, never linked. It adds about 100 KB per page and keeps the page zero-network.
- `hl.js` is the canonical highlighter. Paste it inside the page's single `<script>` block. It is the same tokenizer Tower uses.
- Use tokens from `theme.css`, never raw hex values in page rules. The tokens match `plugins/tower/app/ui/tower.css` so reports and the board read as one product.

## Identity

Jet black and signal red, monospace placards, and an instrument panel that stays quiet until something needs attention. Accents mean something or they are not used.

| Token | Value | Role |
|---|---|---|
| `--jet` | `#060608` | Page background |
| `--soot` / `--ash` | `#0E0E12` / `#16161C` | Panels, raised cards, code, inputs |
| `--seam` / `--seam-hi` | `#232330` / `#343444` | Borders and rules; hover borders; no drop shadows |
| `--bone` / `--bone-dim` | `#F4F3F5` / `#C9C7D1` | Primary and secondary text |
| `--smoke` | `#9D9CA8` | Captions, legends, and muted values |
| `--ember` | `#FF2E4D` | Signal red: emphasis, live, critical, links, ignition line, and keywords |
| `--oxblood` | `#B3122D` | Deep red surfaces: selected rows and attention fills |
| `--ok` / `--amber` | `#43C78C` / `#DFA14F` | Green for healthy, added, and strings; amber for caution and numbers |
| `--cyan` / `--blue` / `--frost` | `#45B8CA` / `#6A8EF2` / `#8D92C9` | Types and in-the-wild chips; calls; low-emphasis series |

Status chips use `.status` for a quiet outline, `attention` for an oxblood fill, `critical` for an ember fill, `ok` for a green outline, and `wild` for a cyan outline. Removed diffs use an oxblood tint; added diffs use a green tint. Chart series use ember, bone, cyan, amber, then frost. Grid lines use seam.

## Code and typography

Every code block uses `hl.js` with these classes: `hl-k` keyword, `hl-s` string, `hl-n` number, `hl-c` comment, `hl-f` call, and `hl-t` type. `pre` takes the full width of its container, wraps with `white-space: pre-wrap`, and never scrolls sideways with `overflow-x: auto`. Two code blocks may sit side by side only when neither has a line over 72 characters and the container is at least 900px wide; otherwise stack them.
The focused checker enforces highlighting and wrapping.
- **Display (`--display`):** Exo 2, italic, `800`, embedded from `fonts.css`. Titles lean forward like a jet: h1 `clamp(38px, 5.2vw, 58px)`, h2 `26px`. Use it for h1 and h2 only.
- **Body (`--body`):** Atkinson Hyperlegible with system fallbacks at `16px/1.6`; h3 is body at `17px/600`; secondary text uses `--bone-dim`.
- **Labels and code (`--mono`):** JetBrains Mono, embedded. Legends, eyebrows, table heads, and status chips use `12px` uppercase text with `0.06–0.08em` tracking; code uses `13.5px/1.6`. Nothing is smaller than 12px.
- **Big values:** Use `.value`; `.value.attention` turns ember.


## Structure and motion

Every section opens with `.section-head`; its tick lights when the section arrives. Cards hold one decision, finding, or record. The page uses this structure:

```html
<div class="page">
  <header>
    <p class="legend rise"><span class="live">Live</span><span>Source · path/or/repo</span><span>2026-09-02</span></p>
    <h1 class="rise">Placard title</h1>
    <div class="ignition"></div>
    <p class="thesis rise">The one sentence the reader must take away.</p>
  </header>
  <section class="reveal">
    <div class="section-head"><h2>Section title</h2></div>
    <!-- panels, cards, tables -->
  </section>
  <footer>Canonical source: path/to/source.md; this page is a rendering.</footer>
</div>
<script>
(() => {
  const els = document.querySelectorAll('.reveal, section');
  const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
  if (reduce || !('IntersectionObserver' in window)) { els.forEach((e) => e.classList.add('is-visible')); return; }
  document.documentElement.classList.add('js'); /* reveal is opt-in: without scripting everything stays visible */
  const io = new IntersectionObserver((entries) => {
    for (const en of entries) if (en.isIntersecting) { en.target.classList.add('is-visible'); io.unobserve(en.target); }
  }, { rootMargin: '0px 0px -8% 0px' });
  els.forEach((e) => io.observe(e));
})();
</script>
```

On load, the legend, title, and thesis rise in sequence and the ignition line draws left to right under the title with `@keyframes ignite`. Sections reveal on scroll and their ticks light through `IntersectionObserver` adding `is-visible`. `prefers-reduced-motion` removes every animation and shows everything at once.

## Quality floor

- Include semantic HTML and keyboard-operable controls.
- Use `<button>` controls with `aria-pressed` where a toggle has a pressed state.
- Use sortable table headers with `aria-sort`.
- Put tables inside `.table-wrap` for narrow screens.
- Add a `:focus-visible` outline in ember.
- Keep text contrast at least 4.5:1; bone, bone-dim, and smoke on jet pass.
- Respond to 360px and verify at 768px and 1280px.
- Keep the console free of errors.
