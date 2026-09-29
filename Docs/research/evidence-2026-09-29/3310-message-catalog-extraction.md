# #3310 — Message-catalog extraction and PO→MO build contract

Date: 2026-09-29. Card #3310 (CORE-F087). This is an investigation and ballot draft. No product code was changed.

## Question

What exactly do source-message extraction and PO→MO compilation do, measured on the reference peer (GNU gettext), independent of runtime catalog lookup? Where would each job sit in Jet's compiler and build graph?

## Reference peer run (criterion 2)

- Tools: GNU gettext-tools **1.0** from nixpkgs (`xgettext`, `msgfmt` and `msgunfmt` all report `(GNU gettext-tools) 1.0`; runtime `gettext (GNU gettext-runtime) 1.0`). Store path: `/nix/store/rb8rna9gkhs0ybl6z2p904myslh8llg8-gettext-1.0`, source `mirror://gnu/gettext/gettext-1.0.tar.gz`.
- Fixture: `~/.cache/jet-test-scratch/Closer06/gettext/` contains `src/app.c`, `de.po` and `build.sh`. The source has two `N_` array marks, one plain literal, one `ngettext` plural, two `pgettext` contexts over the same msgid, one `gettext(key)` and one `_(user_text)` dynamic call, one unmarked literal, and a `TRANSLATORS:` comment.
- Command, run inside `nix-shell -p gettext`:
  ```
  xgettext --language=C --from-code=UTF-8 --add-comments=TRANSLATORS \
    --keyword=_ --keyword=N_ --keyword=pgettext:1c,2 --keyword=ngettext:1,2 \
    --package-name=fixture --package-version=1 --msgid-bugs-address=none --sort-by-file -o app.pot app.c
  msgfmt --check --statistics -o de.mo de.po      # "6 translated messages."
  msgunfmt de.mo > de.roundtrip.po
  ```
- Artifacts and hashes. Five clean runs went into separate output directories (run1–run5):

| artifact | sha256 | reproducible? |
|---|---|---|
| `de.mo` | `62be256409ea186439c2e28d2f195a656b7122449264c93ad4ab1d4e459391bb` | yes: identical in all 5 runs |
| `de.roundtrip.po` | `9eadc8eef87e1140a9f6ccc6796f14c45f183e99ac2c3a9643f07cc4ac89508a` | yes |
| `app.pot` (source mtime 2026-09-29 02:15 -0400) | `dbf65449dd7ae7368f61ab642d71180fae0cba255c0e7663ed021b15661aaa4b` | runs 1–4 identical, including run4 at 03:55 with `SOURCE_DATE_EPOCH=1757548800` |
| `app.pot` after `touch -d '2026-01-01 00:00:00 UTC' src/app.c` | `cb82371851dec1af8bb83c7694ceada5882a88fdb982f5634384e66e7128b3ab` | header became `POT-Creation-Date: 2025-12-31 19:00-0500` |

Observed: gettext 1.0 takes `POT-Creation-Date` from the **newest input-file mtime**, rendered in the local time zone. It ignored `SOURCE_DATE_EPOCH` in this run. The template is therefore reproducible only when mtimes and the TZ are pinned. The compiled catalog is reproducible regardless.

Warnings: `msgfmt` warned only about missing `PO-Revision-Date`, `Last-Translator` and `Language-Team` header fields. `xgettext` printed nothing.

## Contract table (criterion 1)

| job | peer behaviour observed | Jet mapping | cell |
|---|---|---|---|
| Literal IDs | `_("Save")`, `N_("Draft")`, `N_("Published")` → `msgid` entries with `#: app.c:12` / `app.c:8` references | Parser `Expr.Str(parts, span)` (`Compiler/JetAst/Source/AST/Expressions.jet:5`) is a literal ID only if every `StrPart` is `.Lit` (`:204-207`). The span supplies file:line | unsupported (no marker call exists; parser data is sufficient) |
| Plural forms | `ngettext("%d file", "%d files", n)` → `msgid`/`msgid_plural`, `#, c-format`; `de.po` `Plural-Forms: nplurals=2; plural=(n != 1)` compiled | needs a two-literal marker plus a runtime plural rule; the rule belongs to #3289/#3307 | unsupported |
| Context | `pgettext("menu","Open")` and `pgettext("verb","Open")` → two distinct entries (`msgctxt`) | same as literal IDs, with a context literal | unsupported |
| Source references | `#: app.c:N` for each site. `--sort-by-file` makes the order deterministic | spans are exact. Paths must be package-relative to stay reproducible | supported by existing spans |
| Translator comments | `/* TRANSLATORS: … */` → `#.` comment | doc comments are parsed. A comment-to-call binding rule is not defined | unknown |
| Dynamic IDs | `gettext(key)` and `_(user_text)` were **silently skipped**: no entry, no warning | `Expr.Str` with any `.Interp` part, or a non-literal argument, must produce a **diagnostic** from `jet check`, never a guessed msgid | unsupported (needs a diagnostic) |
| Unmarked text | `"not marked for translation"` not extracted | extraction only at marker calls; never scrape arbitrary literals | policy |
| Macro pitfall | xgettext lexed `#define pgettext(ctx, s) …` (app.c:6) as a keyword call and emitted a spurious empty `msgid ""` entry that collides with the header slot | an AST-driven extractor sees declarations, not text, so this class of error does not arise | argument for option A |
| No target execution | xgettext is lexical and runs nothing | comptime/AST walk only, with no `run` | policy |
| PO→MO compile | `msgfmt --check` validates format directives and plural header. The .mo is byte-stable | a build node over `.po` inputs with a content-hash cache key (existing build-graph model) | unsupported |

## Existing Jet graph

There is no i18n, gettext, catalog or msgid code under `Core/` (a `grep` for `gettext|i18n|msgid|ngettext|translat` finds nothing). Siblings: #3289 (runtime catalog composition) and #3307 (locale formatting).

## Owner ballot draft (criterion 3). Pip files this

**D-I18N-EXTRACT1 — how Jet ships message extraction and catalog compilation**

- **A. `jet i18n extract` / `jet i18n compile`.** A first-party subcommand driven by the Compiler/ front end: literal IDs only, a diagnostic for dynamic IDs, no target execution.
  - Beginner: `jet i18n extract` writes `messages.pot`; editing `de.po` and running `jet build` compiles it.
  - Expert: `jet i18n extract --check` fails CI on dynamic IDs or stale templates.
  - Pros: exact spans, no macro/lexer pitfalls, one toolchain. Cons: a new command and extraction metadata to maintain.
- **B. Core/devtools library API used from a build script.** For example `devtools.i18n.extract(package) -> Catalog`.
  - Beginner: a copied build-script snippet. Expert: full control.
  - Pros: no new CLI verb. Cons: every project wires it itself, and the extractor still needs compiler access.
- **C. Defer until #3289 ratifies the runtime catalog shape, then choose A or B.**

**Recommendation: C, then A.** Extraction keys (literal, plural, context) must match the runtime lookup API, which #3289 has not fixed yet. Any external-tool path (calling GNU `msgfmt`) is a separate dependency ballot. An in-tree .mo writer needs none. Runtime catalog approval grants none of the above.

## Verdict

- Criterion 1: met by the contract table. Literal, plural, context, source-reference and dynamic-ID cells are distinguished, and the no-scrape and no-execution rules are stated.
- Criterion 2: met. Exact tool versions, store path and reproducible artifact hashes are recorded, including the finding that POT dates come from mtime.
- Criterion 3: met by the ballot draft. Nothing new is added.
