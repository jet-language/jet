# Clone gate

Lists every deep copy of a large compiler value in a generated `stage-zero.rs`,
maps it to its Jet function and source line, ranks it, and optionally fails
on copies that run per item, per call, or per loop iteration.

```sh
node Tools/perf/clone-gate/clone-gate.mjs ~/.cache/jet-luna/loop10/stage-zero.rs \
  --out ~/.cache/jet-test-scratch/clone-gate \
  [--profile perf-script.txt] [--check] [--check-tier 2] [--top 40]
```

Node 20+, no dependencies; about 7 s and under 1 GB of heap for the full
compiler. Outputs `clone-gate.tsv` and `summary.txt` (also printed).

## What counts

A site is any `.clone()`, `.to_vec()`, `.to_owned()`, `Clone::clone(…)`, or a
cloning runtime helper (`jet_map_get_opt`, `jet_slice_vec_range`,
`jet_list_slice`, `jet_view_copy`) inside an emitted Jet function. Its static
type comes from the receiver: locals (`let mut _vN: Option<T>`), parameters,
struct and enum field types, `jet_index_vec_ref` elements, and match payload
bindings; when the receiver cannot be typed, the destination local, the callee
parameter, or the struct-literal field decides.

Size classes: `program` (SemaRegistrationGraph, Program, MIRProgram), `item`
(SemaRegistrationModule/Function/Nominal, SemaGraphModuleResult, Item, Func,
TFunc, MIRFunction), `node` (Stmt, Expr, TStmt, TExpr). Every other type takes
the largest class it owns by value; `JetMap`, `JetShared`, `Rc`, and `Arc` share
their payload, so copying through them is shallow. A directly copied list of
item-class seeds is one class up (a module's worth). `--seed Name=tier` adds
or overrides seeds.

## Ranking

The call graph is read from the emitted Rust. Recursive strongly connected
components count as one loop level. `fn_nest` is the most loop levels on any
call path into the function; `loop_depth` is the site's own loop nesting;
`log10_calls` estimates calls with 10 iterations per loop level. Rows sort by
profile samples, then size class, then repeated path, then `fn_nest +
loop_depth`.

`path` is `per-loop` (inside a loop in its own function), `per-item` (reached
through a caller's loop), `per-call` (recursive, or called from several sites),
or `once`.

`shape` names the construct behind the copy, separating emitter rules from
source requests: `pattern-test` (kind test on a place), `loop-source`
(read-only iteration over a place), `capture` (closure captures a borrowed
value), `element` (`x := list[i]`), `field`, `whole` (a borrowed parameter
copied whole), `value` (anything else, including last uses the emitter clones).

`source` is the nearest preceding emitted source line marker, mapped through
the assembled unit (`compiler-project/src/compiler.jet` next to the input by
default, or `--unit` / `--map compiler.map.json`); it is often the enclosing
statement or loop header rather than the exact expression.

## Profile join

`--profile` takes `perf script` text from a frame-pointer profile. Each sample
whose leaf-side frames copy (`clone`/`to_vec`) or drop (`drop_in_place`) a
large type is charged to the nearest emitted Jet function on its stack and to
every static site in that function whose copied value owns that type.

## Gate

`--check` exits 1 when a site of class `--check-tier` (default 2, item) or
larger sits on a repeated path and has no row in `allow.tsv`:
`jet_function<TAB>type<TAB>reason`, where `*` matches any function or type and
the reason is required.
