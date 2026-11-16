# Capture a repository

Clone shallow and blobless, or read hosted files directly:

```sh
git clone --depth 50 --filter=blob:none REPOSITORY_URL target-mine-ID
```

Capture each layer:

- README and docs: what the project claims;
- code: what it does;
- CHANGELOG or releases: what changed and why;
- design docs, RFCs, and ADRs: what was decided and rejected.

Capture the audience layer only when requested or when the named outcome needs
user evidence. Sample top-reaction issues, recently active issues,
closed-as-wontfix decisions, and long-thread debates through `gh`, `issue://`,
`pr://`, or the project's forum.

Record stars, contributor count, commit cadence, and open/closed issue ratio as
context, never as truth. Record clone depth, missing blobs, API warnings, and
skipped discussions.
