// jet-lang.dev — the facet lens. No dependencies.
document.querySelectorAll('.lens').forEach((lens) => {
  const buttons = lens.querySelectorAll('.lens-toggle button');
  const notes = lens.querySelectorAll('.lens-notes');
  buttons.forEach((btn) => {
    btn.addEventListener('click', () => {
      const expert = btn.dataset.facet === 'expert';
      lens.classList.toggle('lens--expert', expert);
      buttons.forEach((b) => b.setAttribute('aria-pressed', String(b === btn)));
      notes.forEach((list) => {
        list.hidden = (list.dataset.facet === 'expert') !== expert;
      });
    });
  });
});
