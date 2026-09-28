# How to make money on Jet without becoming the villain

Research report, 2026-08-06. Three web-research passes: company-scale language businesses, solo/small-team open-source incomes, and emerging 2024–2026 models. All money figures come from primary or press sources; source URLs sit at the end of each section that needs them.

## Executive summary

Nobody gets rich selling a language. Every dollar in this research comes from an artifact **next to** a free language: an editor, an ops layer, a certificate, a retainer, a hosted runtime. The language is the demand engine. This is good news for Jet: the language stays free and open forever, and that pledge is itself an adoption asset.

The evidence gives a clear ranking:

1. **The proven solo path to $200k–$1M/yr needs 3–10 paying companies, not 10,000 users.** Expert retainers (José Valim's Dashbit model), anchor sponsors (Zig gets $154k–160k/yr salary from ~3 concentrated funders), and open-core ops tooling (Sidekiq: one person, ~$7M/yr) all charge companies, never developers.
2. **The one channel that pays before adoption is public memory-safety money.** NLnet gives €5k–50k grants to early-stage infrastructure on rolling deadlines. Sovereign Tech Agency contracted €377k to Scala and €99k to Rust coreutils. Alpha-Omega paid the Rust Foundation $460k + $695k for package-registry security — and Jet ships its own package manager.
3. **Jet's best long-term paid product is the Blueprint visual editor.** n8n took a visual editor on open code from $40M to $100M+ ARR in 2025. Construct 3 sustains $470/seat/yr against free rivals. Beginners — Jet's core audience — are the demographic that pays for polish, and schools buy seats.
4. **The villain line is precisely mapped and easy to stay behind.** Every disaster in the research (Unity, Akka, Redis, HashiCorp) broke one of four rules: no retroactive terms, no relicensing, no metering running code, no moving free features behind pay.
5. **The modal exit for great language tooling right now is an AI-lab acquisition** (Bun→Anthropic Dec 2025, Astral→OpenAI Mar 2026). Do not plan on it, but position for it: Jet as "the language agents get right" costs nothing and compounds.

The honest constraint: **adoption is the bottleneck, not monetization.** The models above are known and safe. Only four money moves are worth doing before Jet has real users, and three of them are free.

---

## Part 1 — What the evidence says

### The base rate for language companies

Of the companies that started with "the language/runtime is free, we'll find revenue later": **Bun, Astral, Deno, and npm — four for four never found it.** Bun and Astral exited to AI labs. npm collapsed into a rescue sale to GitHub. Deno shrank from 35 edge regions to 6, killed Deploy Classic in July 2026, and had layoffs in March 2026.

The companies that made real money had a different shape:

| Model | Proof | What they actually sell |
|---|---|---|
| Tool business funds the language | JetBrains ~$650M rev 2024, bootstrapped; Kotlin free | The best place to *write* the language |
| Hosting for something operationally painful | Vercel $340M ARR; MongoDB Atlas ~$2B; Docker Desktop $207M | Removal of ops pain |
| Open core, buyer-based boundary | GitLab $1B ARR; Sentry $128M+; Sidekiq ~$7M solo | Compliance, SSO, audit, scale features managers want |
| Regulated-vertical toolchain | AdaCore €15.4M; Ferrocene €240/user/yr | Qualification paper and liability transfer |
| Consortium / insurance | SQLite ~$0.6–1.2M/yr from ~5–10 members at $120k | Guaranteed stewardship and access to the author |

A compiler alone has almost no operational pain — you download it and it runs. The money is always in a surface that hurts to run or that a company must certify.

### What individual language creators actually earn

| Person / language | Mechanism | Income | Works? |
|---|---|---|---|
| Andrew Kelley / Zig | 501(c)(3) + ~3 anchor funders (Hashimoto $150k, Bun $60k, TigerBeetle $60k) | $154k (2024), $160k (2025) salary, published in 990s | Yes — the only public language-creator salary paid by the language itself |
| José Valim / Elixir | Dashbit "Elixir Development Subscription" — expert retainer to companies that run Elixir | Undisclosed; small profitable firm, well past $200k | Yes — the most replicable model |
| D. R. Hipp / SQLite | Consortium membership at $120k/yr | Funds ~3–5 people | Yes, at extreme adoption |
| Evan Czaplicki / Elm | Salaried by NoRedInk to work on Elm | ~$150–200k, then laid off 2021; Elm stalled | Worked, then failed — one employer is one point of failure |
| Louis Pilfold / Gleam | GitHub Sponsors, half from Fly.io | ~$55–75k/yr — below a lead-dev salary | No |
| Nim, V, Odin creators | Donations | Low four to five figures | No |

Grassroots donations are the worst mechanism in the data. Reaching $200k/yr on GitHub Sponsors puts you in roughly the top 20–50 accounts worldwide. Caleb Porzio's famous "$1M on GitHub Sponsors" was 97% paid screencasts and logo placement; pure goodwill was $5k of the million. Zig's donation income is the ceiling (~$670–920k/yr for the whole foundation) and it decays without active corporate courtship.

### The 2026 warning: the docs funnel is dead

Tailwind grew usage faster than ever in 2025 and lost ~80% of revenue; it laid off 75% of engineering in January 2026. AI assistants consumed the docs traffic that fed its paid component library. Any plan shaped "free thing → docs eyeballs → sell adjacent artifact" is now structurally impaired. Revenue attached to **running systems** (Sidekiq, Forge) or **skill acquisition** (courses) survived. Revenue attached to **page views** did not.

### The villain line (empirical, ranked by damage)

1. **Retroactive terms on shipped work.** Unity's per-install Runtime Fee: CEO fired, fee cancelled inside 12 months, revenue −17% the next year, market cap $57B → $6B. Unrecoverable.
2. **Relicensing anything compiled into other people's builds.** Akka → BSL produced the Apache Pekko fork within weeks and destroyed Lightbend's standing; Redis → SSPL produced Valkey (funded by AWS, Google, Oracle) and Redis lost most external contributors in 12 months; Terraform → BSL produced OpenTofu under the Linux Foundation.
3. **Unauditable metering** (Unity's self-reported installs).
4. **Moving an existing free feature behind payment** (Puppet under Perforce, 2025 → OpenVox fork).

Relicensing worked exactly once (MongoDB), and only because MongoDB held sole copyright, already had a dominant hosted product, and had a codebase nobody wanted to fork. A compiler and stdlib are the most forkable, most library-shaped artifacts that exist. For a language, relicensing is off the table forever.

Things that are empirically **not** villain triggers: charging companies above a size threshold while individuals stay free (Docker Desktop, >250 employees); charging for hosting (Vercel, Ghost); charging for enterprise governance features behind a stable, published boundary (GitLab, Sentry); charging $120k/yr for insurance and access (SQLite — Hipp is the least-hated person in the dataset and has the highest price).

The one-line law that separates beloved from villain across all ~30 cases: **nothing free may ever become unfree.**

---

## Part 2 — The Jet plan

Jet's assets map onto the evidence unusually well:

- **Memory-safe compiled language** → public memory-safety funding now; certified toolchain later (Ferrocene model).
- **Beginner-magic surface** → education and schools; the demographic that pays for polish.
- **Blueprint visual editor (planned)** → the single best-fit paid product (n8n / Construct 3 / GDevelop evidence).
- **jetpack (own package manager)** → fundable security scope today (Alpha-Omega paid Rust $1.1M+ for exactly this); enterprise supply-chain product later. The registry itself stays free — registries have no pricing power (npm; Astral's pyx died even with the best team in Python).
- **jetos (planned)** → the long-fuse platform where hosted execution can live (Replit went ~$150M → ~$525M ARR on "beginners + agents write code, we run it").

### Phase 0 — now, pre-adoption (costs ~nothing, do all four)

1. **Publish the Jet Pledge.** Permanent, public, in the repo: the language, compiler, stdlib, and public registry are free forever; no relicensing, no royalties, no per-install or per-run fees; no free feature ever moves behind payment. Godot's donations doubled off the back of Unity's blunder — the pledge converts other people's rug-pulls into Jet adoption, and it costs zero because Part 1 shows you must never do those things anyway.
2. **Write the language specification as a durable artifact.** One document unlocks three revenue paths: safety-critical qualification requires it (Ferrocene needed a written Rust spec), grant applications need scoped deliverables, and a precise spec measurably improves AI codegen. Jet's spec docs already exist; the move is to treat them as a product-grade artifact, not internal notes.
3. **Bake provenance into jetpack v0.** Signing (sigstore-style), reproducible builds, and capability/permission metadata in the manifest. This is what Chainguard (0 → $40M ARR in a year) and Socket ($1B valuation) sell as a bolt-on to ecosystems that lack it. Jet can have it natively, which is both a grant-fundable scope and the foundation of the later enterprise tier.
4. **Apply for grants.** NLnet NGI0 Commons has €21.6M to place through mid-2027, gives €5k–50k per grant, funds early-stage infrastructure, and has rolling deadlines — this is the realistic first application. Sovereign Tech Agency and Alpha-Omega become realistic once Jet has visible users (STA funds infrastructure "in use"). Stack grants the way Zig stacks sponsors. Realistic take: €25k–100k over the first 18 months; not a living alone, but real money at a stage where nothing else pays.

### Phase 1 — first production adopters (years 1–3): $200k–500k/yr

This is the conservative path, and it needs **3–10 companies**, not a big community.

1. **Expert retainer — the Dashbit transplant.** "Jet Development Subscription": companies that run Jet in production pay a monthly retainer for architecture review, direct access, and priority attention. Valim built a small profitable firm on this; Hipp's support tiers and Caddy's $999–$11,900/mo sponsor tiers are the same product. Three retainers at $5–8k/mo clears $200k/yr. Zero villain risk — you sell your scarcity, not permission.
2. **Anchor sponsors — the Zig transplant.** Convert the first companies that bet production revenue on Jet into named $30k–150k/yr sponsors. Sell it as insurance, a support SLA, and roadmap access, not charity. Zig funds a $160k salary this way with ~3 funders; Crystal ran a core team for years on one (84codes, €22k/mo since 2018).
3. **Education — the Wathan transplant, with the AI discount.** Wathan's courses made $1M+ *before* Tailwind existed and funded its creation; selling education has produced zero backlash in the entire dataset. Jet is beginner-first, so the definitive Jet book/course is on-mission. Discount the classic free-funnel math (Tailwind's collapse), but courses sell skill, not page views. Realistic: $50k–200k lumpy, launch-driven.

Structure note: a US LLC is enough for all of Phase 1. A foundation (Zig-style 501(c)(3)) becomes worth its ~$18k/yr admin cost only when corporate sponsors need tax deductibility.

### Phase 2 — ecosystem forming (years 3–5): $500k–3M/yr

1. **Blueprint editor, paid where teams are.** Free and fully capable for individuals and learners, forever. Charge for team collaboration, cloud sync, shared libraries, and classroom seats. GDevelop's gate (free until $50k company revenue) is the least-resented shape found; Construct 3 proves $470/seat/yr survives against free; n8n proves the ceiling is nine figures. Never gate export, compile, or run — that is the Unity mistake in a different coat.
2. **Open-core ops tooling — the Sidekiq transplant.** Paid Pro tools that only companies at scale want: build/CI accelerator, fleet profiler, enterprise LSP features. The Sidekiq rules: the free core stays production-complete, paid features are ops-scale features, prices are public, and nothing free ever moves behind the wall.
3. **jetpack enterprise tier.** Private namespaces, org policy, audit, signed mirrors — sold to companies with compliance budgets. Public registry and client stay free and forkable (the Anaconda counter-example: its ToS grab pushed the world to conda-forge).

### Phase 3 — scale (years 5+): the $10M+ options

1. **Certified toolchain — the Ferrocene/AdaCore transplant.** A qualified Jet compiler for regulated industries (ISO 26262, IEC 61508, IEC 62304). Ferrocene sells at €240/user/yr with the toolchain itself open source: the product is the qualification paper, frozen branches, and the SLA. Memory safety is what regulators are pushing buyers toward. Needs the spec (Phase 0), an LTS release train, and one anchor customer to co-fund qualification.
2. **Hosted execution — the Replit/Laravel Cloud shape.** Where beginners and AI agents write Jet, someone must run it; jetos is the natural substrate. The evidence says do not build this before ~100k active developers (Deno died on this hill; Laravel waited 13 years and then raised $57M for it). Until then, just keep the runtime host-friendly: deterministic builds, isolation, capability sandbox — all already Jet design goals.
3. **Jet Consortium — the SQLite transplant.** When large companies depend on Jet, sell them the guarantee: $50k–120k/yr for guaranteed stewardship, direct access, and the promise that Jet stays free forever. Beloved and lucrative are the same product here.

### The aggressive path (parallel option, not a phase)

If the goal is wealth rather than income, the evidence supports one venture shape: **a company that owns the Blueprint editor + hosted execution + certified toolchain, with the language free forever.** That is the Laravel/Vercel/n8n pattern, and investors currently fund it well (Laravel $57M, VoidZero $4.6M seed, n8n $2.5B→$5.2B). VC becomes rational only after organic adoption proves pull — Otwell took the call 13 years in; Evan You after a decade.

And name the quiet second door: strategic acquisition. Anthropic bought Bun; OpenAI bought Astral; both teams never found revenue and exited well anyway, to buyers whose agents need their artifact. "The memory-safe language that agents write correctly, with a spec, deterministic builds, and a safe sandbox" is exactly the profile those buyers paid for. Do not plan on the bid persisting — but every Phase 0 move above also builds that profile for free.

---

## Part 3 — Money math and odds

| Path | Year 1–2 | Year 3–5 | Year 5+ | Odds of $200k+/yr | Odds of $1M+/yr |
|---|---|---|---|---|---|
| Conservative (retainers + sponsors + course + grants) | $30–120k | $200–500k | $300k–1M | Moderate — needs ~3–5 production adopters, nothing else | Low-moderate |
| Aggressive (editor + tooling company, VC later) | $0–50k | $200k–2M | $3M–much more | Moderate (same adoption gate) | Moderate if adoption compounds |
| Acquisition (positioning only) | $0 | — | life-changing once | Not plannable | Not plannable |

The paths share ~90% of their work through year 3, so you do not have to choose now. Both start with Phase 0 + Phase 1, and the decision point (bootstrap vs raise vs sell) arrives only when adoption is real.

What actually moves the odds is the bottleneck the money models all sit behind: **production adopters.** One company with revenue riding on Jet is worth more than 10,000 GitHub stars — it is simultaneously your first retainer, your first anchor sponsor, your first certified-toolchain co-funder, and your proof for grants and investors.

## The rules (never break these)

1. Nothing free ever becomes unfree.
2. Never relicense the language, compiler, stdlib, or registry. Say so publicly, once, permanently.
3. Never meter running code: no per-install, per-run, or royalty fees, ever.
4. Never price on a number the user cannot audit.
5. Charge companies, not developers. Individuals and learners ride free forever.
6. Sell insurance, access, expertise, and polish — never permission.
7. Announce any paid boundary before anyone builds on the free side of it, and never move it inward.

## Sources

Full source URLs live in the three research dossiers gathered for this report (company-scale, indie-scale, and emerging models, all 2026-08-06). Highest-value primary sources: Zig Software Foundation financials (ziglang.org/news/2025-financials), Sidekiq revenue history (mikeperham.com), Laravel/Accel announcement (laravel.com/blog/accel-invests-57m-into-laravel), Ferrocene releases and pricing (ferrous-systems.com, ferrocene.dev), NLnet Commons Fund (nlnet.nl/commonsfund), Sovereign Tech Agency investments (sovereign.tech/tech), Alpha-Omega grants (alpha-omega.dev/grants/grantrecipients), Unity Runtime Fee cancellation (unity.com/blog/unity-is-canceling-the-runtime-fee), Tailwind layoffs coverage (devclass.com, 2026-01-08), Bun/Anthropic (bun.com/blog/bun-joins-anthropic), Astral/OpenAI (openai.com/index/openai-to-acquire-astral), n8n Series C (blog.n8n.io/series-c), SQLite Consortium (sqlite.org/consortium.html).
