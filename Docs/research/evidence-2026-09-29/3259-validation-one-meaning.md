# #3259 — One validation result across data and UI

Date: 2026-09-29. Card #3259 (CORE-F035). Binary: `jet-debug-snapshot14`.

## Question

Does one struct-declared rule set (D-VALIDATE1 `validate { }` plus decode `FieldError`) give the same field paths and reasons for JSON decode, `T.validate` and form submission? Do the errors clear on a valid resubmit?

## Runtime evidence: decode and `T.validate` agree

Probe: `~/.cache/jet-test-scratch/Closer06/web/validate_decode.jet`. `#Codable struct Signup { email, password, validate { check(email.len() > 0, at: email, "email required"); check(password.len() >= 12, at: password, "needs at least 12 characters") } }`. It prints every `FieldError` path and reason from `Signup.validate` and from `json.decode<Signup>`. `jet run` and `jet run --interpret` gave identical output, rc=0:

```
bad validate: email: email required
bad validate: password: needs at least 12 characters
bad validate: 2 error(s)
good validate: ok (a@b.com)
missing decode: email: expected Text, found null
missing decode: 1 error(s)
rules decode: email: email required
rules decode: password: needs at least 12 characters
rules decode: 2 error(s)
good decode: ok (a@b.com)
```

- JSON decode runs the struct's `validate { }` rules and reports the same `(path, reason)` pairs as `T.validate`, on both tiers.
- A missing required member is reported by decode as `email: expected Text, found null`.
- A valid value produces no errors: `good … ok` on both paths, so the errors clear on the next valid input.

## Form path: an independent schema (source) and currently non-compiling

`Core/web/forms.jet` validates through its own schema:

- `WebFormFieldSpec{name, value_type, required, …}` (:50).
- `validation_error` (:522-529) returns `"required"`, `"int"`, `"float"` or `"bool"`.
- `apply_validation` (:531-545) and `validate_field_form` (:547-558) push those strings into per-field `errors` and reset them on every pass, so the errors do clear deterministically.
- `typed_set_async_validator` (:297) and `typed_validate_field` (:353) take a `validator: fn(String)` that returns nothing and cannot contribute an error.

| constraint | decode / `T.validate` (observed) | form submission (source) |
|---|---|---|
| required / missing | `email: expected Text, found null` | `email: required` (:523) |
| custom rule (`check(...)`) | `email: email required`; `password: needs at least 12 characters` | **not expressible**: the form path never consults the struct `validate { }` rules. `fn(String)` validators return Unit |
| type mismatch | decode type error | `"int"`/`"float"`/`"bool"` (:525-527) |
| range | via `check(...)` | not expressible |

The form path cannot be run today. Any `use core.web` fails to compile (the same 14 errors as #3077 WEB-1): `Core/web/forms.jet:225,754` (`len` on `String?`), `:294,308,345,346,363,377,439,450` (`&form.form` E0202), and `Core/web/web.jet:81` (`WebFormError` not visible).

## Adapter question (criterion 4)

`action_field_error(error, field, message)` (:322) already takes a field path and a reason. Projecting a `[FieldError]` onto a form is a user loop, `loop e in errs { err = forms.action_field_error(err, e.path, e.reason) }`, so **no new adapter is missing**. The real change is the one the plan names:

- make the form submit path decode through the struct and reuse `FieldError` (path, reason);
- delete the `fn(String)` validator parameters and the separate `required`/type reason strings.

That removes a public API (`typed_set_async_validator`, `typed_validate_field`) and touches every caller. It belongs to a fixer once `core.web` compiles; it is not a tiny closer edit.

## Defects

| id | repro | tiers | observed | expected |
|---|---|---|---|---|
| WEB-1 | any `use core.web` | all | 14 compile errors in Core/web (see #3077) | compiles |
| VAL-1 | `Examples/features/serde/validate.jet` (golden) | JIT, interpreter | `internal compiler error: checked TIR cannot lower to MIR at 984..1403: report_over: checked method receiver owner arguments disagree with its target` (the `Validate.over(...).check(...).finish()` chain) | the golden `validate.out` |
| FORM-1 | `Core/web/forms.jet:522-529` vs decode | source | form reasons `required`/`int` differ from decode (`expected Text, found null`), and struct rules never run on submit | one meaning (card goal) |

## Verdict

**BLOCKED** (WEB-1).

- Criterion 1: **not met**. The decode ↔ `T.validate` half is proven identical on JIT and interpreter; the form half diverges (FORM-1) and cannot run.
- Criterion 2: met for decode and validate (field paths; clears on valid input). The form clear-on-pass is source-evident only.
- Criterion 3: **not met**. `WebFormFieldSpec` plus `fn(String)` validators is an independent UI schema.
- Criterion 4: met. No missing adapter is found (`action_field_error` suffices), so no ballot is needed.
- Criterion 5 (golden `web/form_validation_one_meaning`): unmet until WEB-1 and FORM-1 are fixed.
