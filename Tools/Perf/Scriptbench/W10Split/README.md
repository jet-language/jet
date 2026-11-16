# w10: split

Build a 20,000-line text, split it into lines and words, and count the words, the words longer than four bytes and the word bytes.

Origin: the 2026-11-01 card-close bench (the cells logged on its cards). Cards: #6088, #6089.

The audit's w10 was a 2,000-row TSV grouped by department. The card-close bench reduced it to the splitting cost the cards are about, and that reduction is what the cards' cells measured; this bench reproduces it. The grouping idiom of #6088 is not a separate cell.

The expected output is `expected.out`; `run.jet` is the Jet program, and `peer.*` are the peers named in `../manifest.json`.
