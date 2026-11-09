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

`SHA256SUMS` pins every vendored file. `Core/Ui/FontData.jet` embeds the
files with `embed_bytes` and `embed_file` when the compiler builds, so the
installed toolchain carries them and Jet programs never read them at runtime.
The `ui_font` cases check every metric, outline and kerning pair against
FreeType and HarfBuzz, so a changed file fails them.

To move to a new release, replace the files, update `SHA256SUMS`
(`sha256sum -c SHA256SUMS` in this directory verifies it) and the release
numbers in this file. Then take the new expected outputs of the
`tests/cases/ui_font` cases from FreeType and HarfBuzz, not from Jet.
