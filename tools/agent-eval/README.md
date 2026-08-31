# Cold-agent evaluator

`run-cold-context.mjs` runs the four fixed tasks (`hello`, `cli`,
`data-transform`, and `http`) twice per adapter: once with the complete capsule
and once with the capsule-sized UTF-8 prefix of `llms.text`. It writes the
scoreboard named by `--output` and returns failure when `--check-baseline`
finds a capsule score below the recorded baseline.

## Adapters

OpenAI uses the built-in OMP command transport by default:

```text
omp --model openai-codex/gpt-5.6-luna --no-session --no-tools --no-skills --no-rules --no-extensions --print PROMPT
```

Anthropic also uses the built-in OMP command transport by default:

```text
omp --model opus --no-session --no-tools --no-skills --no-rules --no-extensions --print PROMPT
```

These command transports use OMP authentication. The harness does not require
`JET_COLD_AGENT_OPENAI_API_KEY` or `JET_COLD_AGENT_ANTHROPIC_API_KEY`.

To select another built-in OMP model, override the matching command with a JSON
argv array:

```sh
export JET_COLD_AGENT_OPENAI_COMMAND='["omp","--model","openai-codex/gpt-5.6-luna","--no-session","--no-tools","--no-skills","--no-rules","--no-extensions","--print"]'
export JET_COLD_AGENT_ANTHROPIC_COMMAND='["omp","--model","sonnet","--no-session","--no-tools","--no-skills","--no-rules","--no-extensions","--print"]'
```

To use the OpenAI-compatible API instead, set
`JET_COLD_AGENT_OPENAI_TRANSPORT=api` and provide the endpoint, key, and model
variables named in `adapters.json`.

Command transport rules:

- `*_COMMAND` is a JSON argv array, not a shell command.
- `input: "prompt-argument"` appends the full request prompt as the final argv
  item. The command must write only the model response to stdout.
- The default input mode is `json-stdin`: the command receives one
  `jet.cold-agent.request.v1` JSON line on stdin.
- Command failures, non-zero exits, timeouts, and oversized output block the
  run and are recorded in the scoreboard.
- A missing required adapter blocks before the matrix starts; partial scores
  are not recorded.

Record a new capsule baseline with `--record-baseline`, or check an existing
one with `--check-baseline`; the flags cannot be combined. A lower capsule
compile, run, or total score returns failure.
