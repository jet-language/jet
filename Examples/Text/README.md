# Text

Strings, formatting, Unicode, regular expressions and checked text.

Run a lesson from the repository root, for example `jet run Examples/Text/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Lessons

- [ASCIICase](ASCIICase.jet) — ASCII case conversion preserves non-ASCII characters.
- [ByteLiterals](ByteLiterals.jet) — Byte text uses the existing `[U8]{"…"}` typed literal.
- [CharacterClasses](CharacterClasses.jet) — Blank, graphic, punctuation and hex-digit tests: ASCII in `core.text.parse`, Unicode in `core.text`; a class is a function value.
- [CharsValue](CharsValue.jet) — `text.chars()` used as a value is a `[Char]` of the string's characters, one per Unicode scalar value, not per byte.
- [ComputedSelectors](ComputedSelectors.jet) — A format selector's number may be any Int expression computed at run time, not only a literal.
- [CutLast](CutLast.jet) — Terminal-boundary string splitting, paired with the existing first-boundary `split_once` operation.
- [FormatSelectors](FormatSelectors.jet)
- [HasControl](HasControl.jet)
- [HexBase64](HexBase64.jet) — Hex and base64 encode/decode
- [HexDecodePolicy](HexDecodePolicy.jet) — Hex decoding is strict and does not trim or repair input.
- [HTMLUnescapeStrip](HTMLUnescapeStrip.jet) — Entity decoding preserves literal UTF-8 and leaves unknown entities unchanged.
- [HumanFormat](HumanFormat.jet)
- [ParseInterpolation](ParseInterpolation.jet) — The same interpolation literal that formats a string can sit in pattern position and match — the fixed text anchors the match, and each `{hole}` binds a name.
- [PrettyInterpolation](PrettyInterpolation.jet) — One expanded formatter for selector and API.
- [RawFences](RawFences.jet) — Backtick fences are ordinary Strings: escapes and holes stay literal, line endings stay in the value, and a longer fence carries backticks.
- [Regex](Regex.jet) — One Unicode VM and Regex/Match surface for free calls and methods, numbered expansion, explicit flags, and character-index spans.
- [RegexSyntax](RegexSyntax.jet)
- [SliceRange](SliceRange.jet)
- [StreamLines](StreamLines.jet)
- [StringAffixPad](StringAffixPad.jet) — `remove_prefix`/`remove_suffix` drop one exact edge (else the text is unchanged); `pad_start`/`pad_end` pad to a width with whole fill units.
- [StringHash](StringHash.jet) — `s.hash()` is a non-cryptographic U64 of the string's UTF-8 bytes.
- [StringMethodSurface](StringMethodSurface.jet) — The built-in String, Bytes and number method surface: one call per method that the other text examples do not already exercise.
- [StringParse](StringParse.jet) — String parsing and line splitting for *small in-memory* text only.
- [Strings](Strings.jet)
- [StringTryPush](StringTryPush.jet) — `s.try_push(text)` appends to a String place and reports a refused allocation as an `AllocError` value, which `??` handles at the call.
- [TextCursor](TextCursor.jet) — `text.Cursor` — `take_pattern` reuses the interpolation-literal-as-pattern matcher (typed holes, non-greedy, anchored by literal text) in CONSUME mode: it…
- [TextTabsTitle](TextTabsTitle.jet)
- [TypedLiteralRaw](TypedLiteralRaw.jet) — Typed literal bodies own backslashes.
- [UUID](UUID.jet)
- [VersionOrder](VersionOrder.jet) — `a.compare_version(b)` is the order of GNU `sort -V`: digit runs compare as numbers, `~` sorts first, and a file suffix only breaks a tie.

## CheckedText

- [CheckedText](CheckedText/CheckedText/) — Checked text is ordinary nominal data plus one ordinary CheckedText implementation.
- [CheckedTextRuntimeHole](CheckedText/CheckedTextRuntimeHole.jet) — A checked text literal whose hole is a run-time value is built when it runs: each hole goes through encode_hole, and check sees the complete text.
- [CheckedTextRuntimeHoleStop](CheckedText/CheckedTextRuntimeHoleStop.jet) — A run-time hole whose complete text fails the type check stops the program at the literal, so no unchecked Token value can exist.
- [CheckedTextSQL](CheckedText/CheckedTextSQL.jet) — SQL and HTML are library-defined, nominal String-backed types with ordinary `CheckedText` implementations.
- [CheckedTextType](CheckedText/CheckedTextType.jet)

## Unicode

- [Collation](Unicode/Collation.jet) — Dictionary order: `text.collate` is the Unicode default order (UCA with the CLDR root table), which is also English order.
- [StringLenScalars](Unicode/StringLenScalars.jet)
- [TextBoundaries](Unicode/TextBoundaries.jet)
- [TextByteBoundaries](Unicode/TextByteBoundaries.jet)
- [UnicodeText](Unicode/UnicodeText.jet) — core.text — std-only Unicode scalar helpers.
- [UnicodeTextAudit](Unicode/UnicodeTextAudit.jet) — core.text owns Unicode algorithms; String stays small.
