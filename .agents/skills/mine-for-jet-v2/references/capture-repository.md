# Repository: mechanisms, ordinary jobs, and history

Read hosted text first when sufficient; otherwise shallow/blobless clone to an
explicit cache location, not the target Jet checkout:

```sh
Tools/agent/jet-env git clone --depth 50 --filter=blob:none 'REPOSITORY_URL' "$CAPTURE_ROOT/source"
```

Pin commit/release and distinguish tagged behavior from current main. Record
reviewed directory/file groups, tests, examples, design/RFC/ADR records, releases
and failure history as one-module/directory segments with a `modules` inventory. README
claims are not code proof. Extract actual authority/default/design consequences,
complete common jobs, failure/recovery, portability and architectural obligations,
not a README API inventory or star count.

Audience review is conditional: sample top-reaction/recent issues, closed-as-
wontfix, substantive replies and long debates through read-only gh/issue/PR tools.
Record strata, clone depth, missing blobs, skipped discussions and API warnings.
Stars/contributors/cadence are context, not independent technical corroboration.
