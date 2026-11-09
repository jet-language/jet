# Default ui font

`core.ui.font` draws with Atkinson Hyperlegible Next 2.001, the Braille
Institute's typeface. It ships in the Regular (400) and Bold (700) weights,
as static TrueType (glyf) builds.

- The fonts are `AtkinsonHyperlegibleNext-Regular.ttf` and `AtkinsonHyperlegibleNext-Bold.ttf`.
  They come from
  `https://raw.githubusercontent.com/googlefonts/atkinson-hyperlegible-next/7925f50f649b3813257faf2f4c0b381011f434f1/fonts/ttf/`,
  the commit Google Fonts publishes.
- `OFL.txt` is the SIL Open Font License 1.1 from the same commit. It is the
  license of both fonts. The license declares no Reserved Font Name.

`SHA256SUMS` pins every vendored file. `Tools/agent/FontAssets.jet` checks
the fonts and the license against it before it generates `Core/Ui/FontData.jet`.
That file holds the font bytes and the license text behind `core.ui.font`:

```sh
jet Tools/agent/FontAssets.jet          # rewrite the data
jet Tools/agent/FontAssets.jet --check  # fail when the data is stale
```

To move to a new release, replace the files, update `SHA256SUMS` and the
release numbers in this file and in `Tools/agent/FontAssets.jet`, then
regenerate. Generated Jet is checked in. Jet programs never read these files
or use the network at build time or runtime.
