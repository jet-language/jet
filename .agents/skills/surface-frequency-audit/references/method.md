# Surface frequency audit method

Use this method for each run. Pin this file in the checkpoint state.

## Coverage contract

The audit is exhaustive inside its declared feature catalogs and sampled corpus. It is not exhaustive across all code ever written.

Use these source strata:

1. Professional production projects
2. Mature open-source projects
3. Small libraries and applications
4. One-off tools, scripts, helpers, and personal projects
5. Education, assignments, tutorials, and beginner projects

Keep measured prevalence neutral. Give entry-level friction a visible `1.15` adoption factor. Give general work `1.05`. Give expert-only work `1.00`. The factor changes recommendation priority only. It never changes source counts.

Classify tasks as `entry`, `general`, `expert`, or `unknown`. Use required concepts, setup, failure risk, and system knowledge. Do not classify a whole repository from popularity or size alone.

## Required baseline

### Programming languages

Python, Rust, C, C++, Go, Nix, JavaScript, TypeScript, Java, C#, Kotlin, Swift, Objective-C, Ruby, PHP, Lua, Bash, PowerShell, SQL, R, Julia, Haskell, OCaml, F#, Elixir, Erlang, Zig, and WebAssembly text.

Treat material dialects separately. Examples include SQL dialects, shell dialects, C and C++ standards, JavaScript runtimes, and WebAssembly proposals. Pin the language or dialect version when evidence permits.

### Adjacent declarative surfaces

HTML, CSS, regular expressions, build files, CI files, query languages, infrastructure configuration, package manifests, and deployment configuration.

Rank adjacent surfaces separately from programming languages.

### Domains

Web frontend, web backend, mobile, desktop, systems, embedded, games, data engineering, data science, machine learning, scientific work, finance, business software, databases, networking, security, DevOps, infrastructure, build systems, CLIs, automation, compilers, libraries, education, and personal tools.

### Sample targets

- At least 30 independent projects for each language.
- At least 30 independent projects for each domain.
- At least five projects in each populated language-domain cell.
- All five source strata where public evidence exists.

These are targets, not permission to invent coverage. Mark a cell `weak` when it misses its target. Mark it `unavailable` when no sound public sample exists.

Select sources from releases, reverse dependencies, package registries, public deployments, curated examples, public assignments, small repositories, and one-off code collections. Do not use popularity as the only selection rule.

Freeze a deterministic sampling frame for each language-domain-stratum cell:

1. Record the index, registry, query, time window, retrieval time, and full candidate count.
2. Canonicalize repository URLs and remove sources that fail the inclusion rules.
3. Set the selection seed to the run ID.
4. Sort candidates by `SHA-256(seed + canonical source ID)`.
5. Take the first candidates that meet the cell target.
6. Replace an unavailable source with the next candidate in that frozen order.
7. Cap each canonical source identity at one project in the primary prevalence view.

Put curated or purposeful sources in a separate case-study stratum. Do not use them for population prevalence. Record the seed and selection frame in the report.

## Source independence

Use a canonical `source_identity` for forks, mirrors, vendored packages, generated output, templates, benchmark suites, and copied files.

Publish both raw and deduplicated views. The primary view excludes:

- Generated files
- Vendored dependencies
- Exact forks and mirrors
- Fixtures and snapshots
- Tutorials and examples outside their named stratum
- Benchmarks outside their named stratum
- Tests outside their named stratum

Do not hide their scale. Some excluded forms still measure what users must read or maintain.

## Taxonomy

Use this hierarchy:

```text
ontology family → user task → semantic operation → exact surface or API
```

Use `.agents/skills/isomorphic-ontology-audit/ontology.md` as the category catalog. Add the official feature catalog for each language. Preserve unmatched constructs.

Do not put a parent group and one of its children in the same rank. Do not infer semantic equivalence from a shared name or glyph.

Cover these surface classes:

- Syntax forms, declarations, literals, keywords, sigils, operators, and patterns
- Semantic operations and effects
- Built-ins and standard-library APIs
- Platform and resolved third-party APIs
- Common idioms and multi-step task sequences
- Repeated helper functions, wrapper types, macros, and workarounds
- Build, run, test, debug, format, lint, dependency, package, deployment, REPL, documentation, and editor workflows

## Work-unit result

Each result JSON uses this minimum shape:

```json
{
  "schema": 1,
  "unit_id": "repo-language-id",
  "source_ids": ["canonical-source-id"],
  "tool": {"name": "parser-or-scanner", "version": "exact-version"},
  "coverage": {
    "files_seen": 100,
    "files_parsed": 96,
    "files_skipped": 4,
    "normalized_lines": 12000,
    "lexical_tokens": 85000
  },
  "measurements": [
    {
      "ontology_ids": ["C21"],
      "operation_id": "error-propagation",
      "feature_id": "rust:error-propagation-question-mark",
      "level": "surface",
      "surface": "?",
      "metric": "usage",
      "numerator": 42,
      "denominator": 60,
      "opportunity": "eligible Result-returning propagation sites",
      "scope": "non-generated production source",
      "difficulty": "general",
      "eligible": true,
      "source_sites": ["src/lib.rs:10"]
    }
  ],
  "citations": ["repository URL pinned to commit"],
  "warnings": ["four macro-generated files were not parsed"]
}
```

Use `"metric": "usage"` for normalized collector rows. The aggregator derives project prevalence, opportunity share, breadth, and density from those rows. Treat co-occurrences, idioms, workarounds, and tooling sequences as named features with an exact eligible opportunity.

Emit both layers. Add one `level: operation` row for the language-agnostic operation. Add `level: surface` rows for its exact language forms and APIs. Give each row the same `operation_id`. Give each exact form its own `feature_id`.

Use `not-recorded`, not zero, when a metric is unavailable. Each completed unit must account for every seen file as parsed or skipped.

Emit one row for each eligible project-feature-metric combination, including rows with a zero numerator. Set `eligible` to `false` when the semantic opportunity does not exist. This rule makes project prevalence reproducible.

Freeze one catalog entry for each language version or dialect before collection:

```json
{
  "schema": 1,
  "catalogs": [
    {
      "id": "rust-2024",
      "language": "Rust",
      "version": "2024",
      "official_sources": ["https://doc.rust-lang.org/reference/"],
      "official_sections_total": 120,
      "official_sections_mapped": 118,
      "unmatched_sections": ["section-id-a", "section-id-b"],
      "built_by": "catalog-builder-agent-id",
      "reviewed_by": "independent-agent-id",
      "reviewed_at": "YYYY-MM-DDTHH:MM:SSZ",
      "official_sections": [
        {
          "id": "expressions.question-mark",
          "url": "https://doc.rust-lang.org/reference/expressions/operator-expr.html#the-question-mark-operator",
          "status": "mapped",
          "measurement_keys": [
            ["error-propagation", "rust:error-propagation-question-mark", "surface", "usage", "non-generated production source", "eligible Result-returning propagation sites"]
          ],
          "reason": null
        }
      ],
      "measurements": [
      {
        "operation_id": "error-propagation",
        "feature_id": "rust:error-propagation-question-mark",
        "level": "surface",
        "metric": "usage",
        "scope": "non-generated production source",
        "opportunity": "eligible Result-returning propagation sites"
      }
      ]
    }
  ]
}
```

Each catalog measurement also includes `ontology_ids`, `surface`, and `difficulty`. Each surface row has a matching operation row. Every measurement maps from an official section. The section counts must reconcile. The checkpoint tool rejects missing, extra, changed, or duplicate result rows.

Use this minimum partial-checkpoint shape after each bounded source slice:

```json
{
  "schema": 1,
  "unit_id": "repo-language-id",
  "cursor": "exact next file, source, or range",
  "completed_inputs": ["src/a.rs", "src/b.rs"],
  "measurements": [],
  "warnings": []
}
```

Store accumulated normalized measurements in `measurements`. The checkpoint tool copies and hashes the partial file before it renews the lease.

## Metrics

Report all applicable metrics. Never publish a numerator without its denominator.

| Metric | Definition | Main use |
| --- | --- | --- |
| Project prevalence | Eligible projects with item / eligible projects | Primary commonness rank |
| Opportunity share | Uses / eligible semantic opportunities | Default-path preference |
| Density | Sites / 1,000 normalized lexical tokens; use KLOC as support | Repetition inside projects |
| Breadth | Populated language, domain, and stratum cells with item / eligible cells | Cross-context reach |
| Distribution | Project median and p90 density | Typical and heavy use |
| Co-occurrence | Projects or sites with item pair / eligible projects or sites | Idioms and clusters |
| Workaround rate | Eligible tasks using helper or manual sequence / eligible tasks | Missing-default signal |
| Coverage | Parsed files / files seen; resolved sites / candidate sites | Evidence quality |
| Trend | Metric by pinned time cohort over five years | Material adoption change |

Static source frequency is primary. Runtime telemetry stays in a separate table and names its observed population.

Balance project prevalence across language and domain cells. Also show equal-project, equal-domain, equal-language, equal-stratum, total-weighted, median, and p90 views.

## Friction measures

Compare equivalent tasks. Record these costs:

- Lexical tokens
- Statements or AST nodes
- Temporary values
- Required concepts
- Nonlocal lookups
- Mandatory annotations
- Control-flow branches
- Error-handling steps
- Repeated boilerplate
- Tool or build steps

Compare Jet with the peer median and the best proven peer surface. Show safety, control, diagnostics, and audit value separately. A shorter unsafe or unclear path is not the best peer baseline.

## Priority index

Keep component values visible. Calculate the default index only for a concrete Jet recommendation.

```text
frequency = 0.60 × balanced_project_prevalence
          + 0.20 × eligible_opportunity_share
          + 0.20 × breadth

friction = mean(positive normalized Jet cost gaps across recorded friction measures)

priority = 100 × frequency × friction × audience_factor × confidence_factor
```

Normalize each positive cost gap as:

```text
max(0, Jet cost - sound peer baseline cost) / max(1, Jet cost)
```

Use confidence factors `Strong=1.00`, `Moderate=0.75`, and `Weak=0.50`. Use audience factors `entry=1.15`, `general=1.05`, `expert=1.00`, and `unknown=1.00`.

If opportunity share is unavailable, redistribute its weight to project prevalence. If breadth is unavailable, do the same. State every redistribution.

Run sensitivity with each frequency weight changed by `±0.10`, one at a time, while the weights still sum to `1.00`. Also run without audience and confidence factors. Report rank bands when recommendations move materially.

The priority index does not override safety, accessibility, diagnostics, or expert control. Use those as explicit review gates.

## Confidence

Assign confidence from evidence, not tone.

- `Strong`: sample targets met, parser coverage at least 95%, symbol resolution at least 90% when required, multiple domains and strata, and stable sensitivity rank.
- `Moderate`: one target or coverage threshold misses, but the direction survives sensitivity checks.
- `Weak`: sparse cells, heuristic parsing, unresolved symbols, unstable ranks, or material source bias.

State the exact cause of every `Moderate` or `Weak` grade.

## Recommendation classes

Use only:

- `Keep`
- `Reduce friction`
- `Add`
- `Remove`
- `Study`

Separate the measurement from the Jet judgment. Discuss no more than 20 recommendations in full. Keep lower-priority findings in a compact watchlist.

Apply the beginner and expert passes to every recommendation:

- Beginner: fast start, safe default, few required concepts, direct errors.
- Expert: explicit control, auditability, target and effect visibility, no hidden mechanism.

## Tower cross-reference

Cross-reference Tower only after rankings stabilize. Read through the Tower CLI. Do not write.

Use these statuses:

- `Covered`
- `Partly covered`
- `Not covered`
- `Conflicts with current plan`

Cite exact card or decision IDs. A broad title does not count as coverage. Do not create proposals, cards, ballots, or acceptance criteria.
