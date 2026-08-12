# Agent communication and skill research — 2026-08-10

## Decision

Use four writing tests:

- **Clear:** Lead with the result. Use concrete words.
- **Simple:** Prefer short, familiar words. Remove needless rules and jargon.
- **Brief:** Say each fact once. Cut ceremony, not meaning.
- **Human:** Write for the reader. Use specific detail and honest limits. Do not fake experience or emotion.

Use `caveman` for short agent chatter. Use full grammar for user-facing prose, risk, nuance, and multi-step work.

Measure token savings by workload. Caveman changes output style. It does not remove input or reasoning cost. Do not claim total session savings from short examples.

Do not add a `bro` mode. The search found no stable technical definition of `bro` as an agent communication mode.

## Evidence

- [Thariq on X](https://x.com/trq212/status/2080710971228918066) and [Anthropic's rules](https://claude.com/blog/the-new-rules-of-context-engineering-for-claude-5-generation-models): remove conflicting rules, keep skills light, and use progressive disclosure.
- [Omar Sanseviero on X](https://x.com/omarsar0/status/2080761013826433138): keep prompts minimal, tools simple, context rich, and evaluation strong.
- [Caveman README](https://github.com/JuliusBrussee/caveman) and [honest numbers](https://github.com/JuliusBrussee/caveman/blob/main/docs/HONEST-NUMBERS.md): report 65% chat output reduction, 8.5% agentic coding output reduction, and possible net-negative workloads.
- [Ziwen on X](https://x.com/ziwenxu_/status/2040534172981223878) claims 30–40% response savings. [Monali on X](https://x.com/monali_dambre/status/2040351690990424372) rejects the total-cost claim. Treat both as discourse, not proof.
- [Lynn Cole on X](https://x.com/priestessofdada/status/2050493632000512264): aggressive compression can harm iterative work and nuance.
- [Anthropic context engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents): keep the smallest high-signal context, preserve critical details, and add rules after observed failures.
- [CMU LLM style study](https://eric-mingjie.github.io/llm-idiosyncrasies/): style fingerprints persist across format and length controls. Do not use word blacklists as a human-writing test.
- [LLMLingua](https://arxiv.org/abs/2310.05736) and [information-preservation study](https://arxiv.org/abs/2503.19114): real input compression needs semantic checks. Naive deletion can lose key details.

## Local changes

- Updated local `caveman@caveman` from `25d22f864ad6` to `309834233183`.
- Added the four tests to `.agents/skills/simple/SKILL.md`.
- Restart Claude Code before using the new plugin version.
