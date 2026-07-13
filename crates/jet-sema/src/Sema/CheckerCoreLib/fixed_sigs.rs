use crate::AST::{AccessConvention, Type};
use crate::Syntax;
use super::alloc_ptrs::{db_error_ty, db_row_ty, io_error_ty, result_ty};
use super::core_types::{encoding_error_ty, json_error_ty, json_ty, u8_ty, unit_ty};

/// c109 Phase 20: the polymorphic core specials whose return type is resolved by
/// `infer_core_call`'s bespoke arg-type logic (NOT the fixed `core_fixed_sig`
/// table). Sema writes the resolved return back onto the `Expr::MethodCall`
/// `resolved_ret` field for exactly these, so the TIR reads it totally (I3).
/// `io.input` is excluded — it IS in `core_fixed_sig` (`String?`), covered by
/// Phase 10. `core.mem` ptr ops have their own Phase-18 lowering.
pub fn is_polymorphic_core_special(module: &str, name: &str) -> bool {
    matches!(
        (module, name),
        ("core.math", "abs")
            | ("core.math", "min")
            | ("core.math", "max")
            | ("core.math", "clamp")
            // D-FLOATW1: sqrt/floor/ceil/pow are width-generic (Float→Float, F32→F32);
            // their return type is arg-type-dependent, so they use resolved_ret.
            | ("core.math", "sqrt")
            | ("core.math", "floor")
            | ("core.math", "ceil")
            | ("core.math", "pow")
            | ("core.math", "sin")
            | ("core.math", "cos")
            | ("core.math", "tan")
            | ("core.math", "asin")
            | ("core.math", "acos")
            | ("core.math", "atan")
            | ("core.math", "atan2")
            | ("core.math", "sinh")
            | ("core.math", "cosh")
            | ("core.math", "tanh")
            | ("core.math", "exp")
            | ("core.math", "ln")
            | ("core.math", "log2")
            | ("core.math", "log10")
            | ("core.math", "hypot")
            | ("core.math", "trunc")
            | ("core.math", "fract")
            | ("core.math", "sign")
            | ("core.math", "is_nan")
            | ("core.math", "is_inf")
            | ("core.math", "is_finite")
            | ("core.math", "to_bits")
            | ("core.math", "from_bits")
            | ("core.math", "degrees")
            | ("core.math", "radians")
            | ("core.math", "lerp")
            | ("core.math", "checked_add")
            | ("core.math", "checked_sub")
            | ("core.math", "checked_mul")
            | ("core.math", "checked_pow")
            | ("core.math", "saturating_add")
            | ("core.math", "saturating_sub")
            | ("core.math", "saturating_mul")
            | ("core.math", "wrapping_add")
            | ("core.math", "wrapping_sub")
            | ("core.math", "wrapping_mul")
            | ("core.math", "int_pow")
            | ("core.math", "gcd")
            | ("core.math", "lcm")
            | ("core.random", "pick")
            | ("core.random", "weighted_pick")
            | ("core.random", "sample")
            | ("core.random", "shuffle")
            | ("core.io", "eprint")
            | ("core.io", "print")
            // D-ENC1 / D-SERDE6: typed encode/decode return types depend on the value
            // type / call-site `<T>`, so codegen reads them from resolved_ret (I3).
            // D-MIGRATE3=A: `decode_traced` is the same call-site-typed shape, one
            // layer deeper (`DecodeResult<T>`).
            | (
                "core.encoding.json" | "core.encoding.csv" | "core.encoding.toml"
                | "core.encoding.yaml",
                "to_string" | "to_string_pretty" | "decode" | "decode_traced",
            )
            // D-REACT1=B: the reactive producers return `Signal<T>`/`Derived<T>` whose
            // element type is inferred from the initial value / closure return — not in
            // `core_fixed_sig`, so codegen reads it from resolved_ret (I3).
            | ("jet.reactive", "signal" | "derived")
            // D-TUPLE-DESTRUCT1: `tasks.channel<T>()` returns `(Sender<T>, Receiver<T>)`,
            // `T` read off the call-site turbofish — not in `core_fixed_sig`, so codegen
            // reads the whole tuple type from resolved_ret (I3).
            | ("core.tasks", "channel" | "after")
            | (
                "core.data",
                "csv" | "count" | "table" | "rows" | "series" | "values" | "missing_count"
                    | "lazy" | "lazy_filter" | "lazy_sort_by" | "collect" | "plan"
                    | "filter" | "sort_by" | "group_count" | "group_sum" | "group_mean",
            )
    )
}

pub fn core_fixed_sig(
    module: &str,
    name: &str,
) -> Option<(Vec<(AccessConvention, Type)>, Option<Type>)> {
    let normalized_module =
        Syntax::normalize_core_module(module).unwrap_or_else(|| module.to_string());
    let module = normalized_module.as_str();
    let read = AccessConvention::Read;
    let moved = AccessConvention::Move;
    let string = Type::String;
    let int = Type::Int;
    let float = Type::Float;
    let bool_ = Type::Bool;
    let unit = unit_ty();
    let io = io_error_ty();
    let json = json_ty();
    let list_u8 = Type::List(Box::new(u8_ty()));
    let io_unit = result_ty(unit.clone(), io.clone());
    match (module, name) {
        ("core.files", "read") => Some((vec![(read, string.clone())], Some(result_ty(string, io)))),
        ("core.files", "read_bytes") => Some((
            vec![(read, Type::String)],
            Some(result_ty(list_u8, io_error_ty())),
        )),
        // D-FILES-WRITE1 (merge) + D-FILES-APPEND1=A: `write`/`append_all` are the
        // whole-file convenience twins of the streaming `open`/`create`/`append`
        // handle constructors below. `append_all` (not `append`) so it doesn't
        // collide with the streaming handle's `.append(text)` method in the same
        // `core.files` namespace.
        ("core.files", "write" | "append_all") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(io_unit),
        )),
        ("core.files", "exists" | "is_dir") => Some((vec![(read, Type::String)], Some(bool_))),
        (
            "core.files",
            "remove" | "create_dir" | "create_dir_all" | "remove_dir" | "remove_all",
        ) => Some((
            vec![(read, Type::String)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "stat") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("Stat".to_string()), io_error_ty())),
        )),
        ("core.files", "canonicalize" | "absolute") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::String, io_error_ty())),
        )),
        // D-LSDIR1=A: returns [DirEntry] ({name, path, is_dir}) — full path + type in one step.
        ("core.files", "list_dir") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::List(Box::new(Type::Named("DirEntry".to_string()))),
                io_error_ty(),
            )),
        )),
        ("core.files", "copy" | "rename") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "copy_dir") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "symlink" | "hard_link") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "read_link") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::String, io_error_ty())),
        )),
        ("core.files", "walk") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::List(Box::new(Type::Named("WalkEntry".to_string()))),
                io_error_ty(),
            )),
        )),
        ("core.files", "glob") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::List(Box::new(Type::String)), io_error_ty())),
        )),
        ("core.files", "read_at") => Some((
            vec![(read, Type::String), (read, Type::Int), (read, Type::Int)],
            Some(result_ty(Type::List(Box::new(u8_ty())), io_error_ty())),
        )),
        ("core.files", "write_at") => Some((
            vec![
                (read, Type::String),
                (read, Type::Int),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "fsync") => Some((
            vec![(read, Type::String)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "write_atomic") => Some((
            vec![(read, Type::String), (read, Type::List(Box::new(u8_ty())))],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.files", "temp_dir") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("TempDir".to_string()), io_error_ty())),
        )),
        ("core.files", "temp_file") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("TempFile".to_string()),
                io_error_ty(),
            )),
        )),
        ("core.files", "lock") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("FileLock".to_string()),
                io_error_ty(),
            )),
        )),
        ("core.watcher", "files") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("WatchHandle".to_string()),
                io_error_ty(),
            )),
        )),
        ("core.watcher", "process_pid") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named("WatchHandle".to_string())),
        )),
        ("core.watcher", "port") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(Type::Named("WatchHandle".to_string())),
        )),
        ("core.watcher", "set") => Some((vec![], Some(Type::Named("WatchSet".to_string())))),
        ("core.io", "args") => Some((vec![], Some(Type::List(Box::new(Type::String))))),
        ("core.io", "read_all_input") => {
            Some((vec![], Some(result_ty(Type::String, io_error_ty()))))
        }
        // D-STDIN1=A: streaming line-by-line stdin.
        ("core.io", "stdin") => Some((vec![], Some(Type::Named("StdinHandle".to_string())))),
        ("core.io", "stdout") => Some((vec![], Some(Type::Named("Stdout".to_string())))),
        ("core.io", "stderr") => Some((vec![], Some(Type::Named("Stderr".to_string())))),
        ("core.io", "terminal_width" | "terminal_height") => Some((vec![], Some(Type::Int))),
        ("core.io", "style" | "style_force") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(Type::String),
        )),
        ("core.io", "progress") => Some((
            vec![(read, Type::String)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("core.env", "get") => Some((
            vec![(read, Type::String)],
            Some(Type::Option(Box::new(Type::String))),
        )),
        ("core.env", "set") => Some((vec![(read, Type::String), (read, Type::String)], None)),
        ("core.env", "unset") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Bool, Type::Named("EnvError".to_string()))),
        )),
        ("core.env", "vars") => Some((
            vec![],
            Some(result_ty(
                Type::List(Box::new(Type::String)),
                Type::Named("EnvError".to_string()),
            )),
        )),
        ("core.env", "current_dir") => Some((vec![], Some(result_ty(Type::String, io_error_ty())))),
        ("core.env", "home_dir") => Some((vec![], Some(Type::Option(Box::new(Type::String))))),
        ("core.os", "name" | "family" | "arch" | "temp_dir" | "executable" | "hostname" | "username") => {
            Some((vec![], Some(Type::String)))
        }
        ("core.os", "pid" | "cpu_count") => Some((vec![], Some(Type::Int))),
        ("core.os", "set_current_dir") => {
            Some((vec![(read, Type::String)], Some(result_ty(unit_ty(), io_error_ty()))))
        }
        ("core.os", "on_interrupt") => Some((
            vec![(
                read,
                Type::Fn {
                    params: vec![],
                    ret: None,
                    effect_bound: None,
                },
            )],
            None,
        )),
        // U13 (D-JPK-SECRETCRYPTO1): `core.vault.get(name)` — a decrypted repo
        // secret, `None` if `name` isn't in the store. Same "may be missing"
        // shape as `core.env.get`.
        ("core.vault", "get") => Some((
            vec![(read, Type::String)],
            Some(Type::Option(Box::new(Type::String))),
        )),
        ("core.process", "exit") => Some((vec![(read, int)], None)),
        ("core.process", "run") => Some((
            vec![(read, Type::List(Box::new(Type::String)))],
            Some(result_ty(
                Type::Named("ProcessResult".to_string()),
                io_error_ty(),
            )),
        )),
        ("core.process", "cmd") => Some((
            vec![(read, Type::List(Box::new(Type::String)))],
            Some(Type::Named("ProcessSpec".to_string())),
        )),
        // D-PROCESS1=A: pipeline takes a list of `ProcessSpec` (built via
        // `process.cmd(argv)...`), not raw argv lists — one canonical builder (I8).
        ("core.process", "pipeline") => Some((
            vec![(
                read,
                Type::List(Box::new(Type::Named("ProcessSpec".to_string()))),
            )],
            Some(result_ty(
                Type::Named("ProcessResult".to_string()),
                io_error_ty(),
            )),
        )),
        ("core.testing", "snap" | "golden") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(Type::Bool),
        )),
        ("core.testing", "fixture" | "temp_dir") => {
            Some((vec![(read, Type::String)], Some(Type::String)))
        }
        ("core.testing", "corpus") => Some((
            vec![(read, Type::String)],
            Some(Type::List(Box::new(Type::String))),
        )),
        ("core.testing", "fake_clock") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named("Clock".to_string())),
        )),
        ("core.testing", "fake_rng") => {
            Some((vec![(read, Type::Int)], Some(Type::Named("Rng".to_string()))))
        }
        // D-TESTKIT1=A: `bench_budget` actually runs `body` (warmup + timed
        // trials) and compares the measured mean against `max_ns` — it is not a
        // no-op assertion. The closure takes no args and returns nothing.
        ("core.testing", "bench_budget") => Some((
            vec![
                (read, Type::String),
                (read, Type::Int),
                (
                    read,
                    Type::Fn {
                        params: vec![],
                        ret: None,
                        effect_bound: None,
                    },
                ),
            ],
            Some(Type::Bool),
        )),
        ("core.math", "sqrt" | "floor" | "ceil") => {
            Some((vec![(read, float.clone())], Some(float)))
        }
        ("core.math", "pow") => Some((
            vec![(read, Type::Float), (read, Type::Float)],
            Some(Type::Float),
        )),
        ("core.math", "round") => Some((vec![(read, Type::Float)], Some(Type::Int))),
        ("core.random", "int") => {
            Some((vec![(read, Type::Int), (read, Type::Int)], Some(Type::Int)))
        }
        ("core.random", "float") => Some((vec![], Some(Type::Float))),
        ("core.random", "float_range") => Some((
            vec![(read, Type::Float), (read, Type::Float)],
            Some(Type::Float),
        )),
        ("core.random", "bool") => Some((vec![(read, Type::Float)], Some(Type::Bool))),
        ("core.random", "normal") => Some((
            vec![(read, Type::Float), (read, Type::Float)],
            Some(Type::Float),
        )),
        ("core.random", "exponential") => Some((vec![(read, Type::Float)], Some(Type::Float))),
        ("core.random", "seed") => Some((vec![(read, Type::Int)], None)),
        // D-RANDSPLIT1=A: seedable PRNG bytes — fast, NOT cryptographically secure.
        // Returns raw `[Int8N]`; for crypto contexts use `core.crypto.random.bytes`.
        ("core.random", "bytes") => {
            Some((vec![(read, Type::Int)], Some(Type::List(Box::new(u8_ty())))))
        }
        // D-CRYPTO-RNG1=A: fail-closed bytes from the target's tier-1 OS CSPRNG.
        // Edition 2026 keeps the infallible source shape and takes E3001/exit 70
        // on invalid length or provider failure; no weak fallback exists.
        ("core.crypto.random", "bytes") => {
            Some((vec![(read, Type::Int)], Some(Type::List(Box::new(u8_ty())))))
        }
        // D-DET1: deterministic injected RNG capability. `random.rng(seed)` builds a
        // reproducible `Rng` from a caller-supplied seed (a pure value); a `@Pure fn`
        // may draw randomness through it (`rng.int(lo, hi)` / `rng.float()`) while the
        // ambient `random.int(…)` stays E3403.
        ("core.random", "rng") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named(crate::Syntax::RNG_TYPE.to_string())),
        )),
        ("core.random", "split") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named(crate::Syntax::RNG_TYPE.to_string())),
        )),
        ("core.time", "now") => Some((vec![], Some(Type::Int))),
        ("core.time", "sleep") => Some((vec![(read, Type::Int)], None)),
        ("core.tasks", "interval") => Some((
            vec![(read, Type::Int)],
            Some(Type::Apply {
                name: "Receiver".to_string(),
                args: vec![Type::Int],
            }),
        )),
        ("core.time", "start") => Some((vec![], Some(Type::Named("Stopwatch".to_string())))),
        ("core.time", "instant") => Some((vec![], Some(Type::Named("Instant".to_string())))),
        ("core.time", "now_utc") => Some((vec![], Some(Type::Named("DateTime".to_string())))),
        ("core.time", "from_unix_ms") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named("DateTime".to_string())),
        )),
        ("core.time", "today") => Some((vec![], Some(Type::Named("LocalDate".to_string())))),
        ("core.time", "parse_rfc3339") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("DateTime".to_string()), Type::String)),
        )),
        ("core.time", "local_time") => Some((
            vec![(read, Type::Int), (read, Type::Int), (read, Type::Int)],
            Some(Type::Named("LocalTime".to_string())),
        )),
        ("core.time", "parse_time") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("LocalTime".to_string()),
                Type::String,
            )),
        )),
        ("core.time", "period") => Some((
            vec![(read, Type::Int), (read, Type::Int), (read, Type::Int)],
            Some(Type::Named("Period".to_string())),
        )),
        ("core.time", "period_days" | "period_months" | "period_years") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named("Period".to_string())),
        )),
        ("core.time", "zone") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("Zone".to_string()), Type::String)),
        )),
        ("core.time", "utc") => Some((vec![], Some(Type::Named("Zone".to_string())))),
        ("core.time", "zoned") => Some((
            vec![
                (read, Type::Named("DateTime".to_string())),
                (read, Type::Named("Zone".to_string())),
            ],
            Some(Type::Named("ZonedDateTime".to_string())),
        )),
        ("core.time", "zoned_local") => Some((
            vec![
                (read, Type::Named("LocalDate".to_string())),
                (read, Type::Named("LocalTime".to_string())),
                (read, Type::Named("Zone".to_string())),
            ],
            Some(Type::Named("ZonedDateTime".to_string())),
        )),
        // D-DET1: deterministic injected Clock capability. `time.clock(seed)` builds a
        // reproducible `Clock` from a caller-supplied start instant (a pure Int, ms);
        // a `@Pure fn` may read time through it (`clock.now()` / `clock.tick(ms)`)
        // while the ambient `time.now()` stays E3403.
        ("core.time", "clock") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named(crate::Syntax::CLOCK_TYPE.to_string())),
        )),
        // D-DET-CAPAPI: `time.ms(n)` / `time.secs(n)` mint a deterministic `Duration`
        // value (pure — no ambient effect, like `time.clock`). The clock advances by
        // one with `clock.wait(d)`; read it back with `duration.millis()`.
        ("core.time", "ms" | "secs" | "seconds" | "minutes" | "hours") => Some((
            vec![(read, Type::Int)],
            Some(Type::Named(crate::Syntax::DURATION_TYPE.to_string())),
        )),
        ("core.game", "run") => Some((
            vec![(read, Type::Named("GameScene".to_string()))],
            Some(Type::String),
        )),
        // D-ENC1 + D-JSONVERB1: unified encoding. `parse` → dynamic JSON value; `decode`
        // → lenient typed decode (D-JSON3); `to_string`/`to_string_pretty` → serialize.
        ("core.encoding.json", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), json_error_ty())),
        )),
        ("core.encoding.json", "decode") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), json_error_ty())),
        )),
        ("core.encoding.json", "to_string" | "to_string_pretty") => {
            Some((vec![(read, json)], Some(Type::String)))
        }
        ("core.encoding.json", "canonical" | "events") => {
            Some((vec![(read, json.clone())], Some(Type::String)))
        }
        ("core.encoding.json", "reader") => Some((
            vec![
                (moved, Type::Named("FileReader".to_string())),
                (read, Type::Named("EncodingLimits".to_string())),
            ],
            Some(result_ty(Type::Named("JSONReader".to_string()), encoding_error_ty())),
        )),
        ("core.encoding.json", "writer") => Some((
            vec![
                (moved, Type::Named("FileWriter".to_string())),
                (read, Type::Named("EncodingLimits".to_string())),
                (read, Type::Bool),
            ],
            Some(result_ty(Type::Named("JSONWriter".to_string()), encoding_error_ty())),
        )),
        ("core.encoding.jsonl", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::List(Box::new(json.clone())), json_error_ty())),
        )),
        ("core.encoding.jsonl", "to_string") => Some((
            vec![(read, Type::List(Box::new(json.clone())))],
            Some(Type::String),
        )),
        ("core.encoding.jsonl", "reader") => Some((
            vec![
                (moved, Type::Named("FileReader".to_string())),
                (read, Type::Named("EncodingLimits".to_string())),
            ],
            Some(result_ty(Type::Named("JSONLReader".to_string()), encoding_error_ty())),
        )),
        ("core.encoding.jsonl", "writer") => Some((
            vec![
                (moved, Type::Named("FileWriter".to_string())),
                (read, Type::Named("EncodingLimits".to_string())),
            ],
            Some(result_ty(Type::Named("JSONLWriter".to_string()), encoding_error_ty())),
        )),
        // jet.csv → core.encoding.csv: parse text into a list of rows (list of fields).
        ("core.encoding.csv", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::List(Box::new(Type::List(Box::new(Type::String)))),
                Type::String,
            )),
        )),
        ("core.encoding.csv", "to_string") => Some((
            vec![(
                read,
                Type::List(Box::new(Type::List(Box::new(Type::String)))),
            )],
            Some(Type::String),
        )),
        // D-DATA-SURFACE1=A / D-DATA-PLOT1=A / D-DATA-STATUS1=A: core.data
        // facade fixed-shape calls. Generic typed table calls are handled in
        // infer_core_call so selectors stay typed by sema.
        ("core.data", "sum" | "mean" | "min" | "max" | "median" | "variance" | "stddev") => Some((
            vec![(read, Type::List(Box::new(Type::Float)))],
            Some(Type::Float),
        )),
        ("core.data", "quantile") => Some((
            vec![(read, Type::List(Box::new(Type::Float))), (read, Type::Float)],
            Some(Type::Float),
        )),
        ("core.data", "rolling_mean") => Some((
            vec![(read, Type::List(Box::new(Type::Float))), (read, Type::Int)],
            Some(Type::List(Box::new(Type::Float))),
        )),
        ("core.data", "describe") => Some((
            vec![(read, Type::List(Box::new(Type::Float)))],
            Some(Type::Named("DataSummary".to_string())),
        )),
        ("core.data", "status") => Some((
            vec![],
            Some(Type::List(Box::new(Type::Named("DataStatus".to_string())))),
        )),
        ("core.data", "bar_text" | "bar_svg") => Some((
            vec![(
                read,
                Type::List(Box::new(Type::Named("DataGroup".to_string()))),
            )],
            Some(Type::String),
        )),
        ("core.fmt", "number" | "bytes" | "duration" | "ordinal") => {
            Some((vec![(read, Type::Int)], Some(Type::String)))
        }
        ("core.fmt", "decimal" | "percent") => Some((
            vec![(read, Type::Float), (read, Type::Int)],
            Some(Type::String),
        )),
        ("core.fmt", "plural") => Some((
            vec![(read, Type::Int), (read, Type::String), (read, Type::String)],
            Some(Type::String),
        )),
        ("core.fmt", "pad_left" | "pad_right" | "pad_center") => Some((
            vec![(read, Type::String), (read, Type::Int), (read, Type::String)],
            Some(Type::String),
        )),
        // D-ENC-DYN1=A+ (c152): TOML is a full adapter over the rich `Data` value —
        // `parse` returns `Toml` (= `Data`); `to_string` takes any encodable value.
        ("core.encoding.toml", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), json_error_ty())),
        )),
        ("core.encoding.toml", "to_string") => {
            Some((vec![(read, json.clone())], Some(Type::String)))
        }
        // D-ENC-YAML1 = A (c152): YAML is a full adapter over the rich `Data` value.
        ("core.encoding.yaml", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), json_error_ty())),
        )),
        ("core.encoding.yaml", "to_string") => {
            Some((vec![(read, json.clone())], Some(Type::String)))
        }
        ("core.encoding.xml", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), Type::String)),
        )),
        ("core.encoding.xml", "to_string") => Some((vec![(read, json.clone())], Some(Type::String))),
        ("core.encoding.cbor", "encode") => Some((
            vec![(read, json.clone())],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        ("core.encoding.cbor", "decode") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(result_ty(json.clone(), Type::String)),
        )),
        // E2-M7: streaming file handles (D-IO2, files.open / files.create).
        ("core.files", "open" | "append") => Some((
            vec![(read, string.clone())],
            Some(result_ty(Type::Named("FileReader".to_string()), io.clone())),
        )),
        ("core.files", "create") => Some((
            vec![(read, string.clone())],
            Some(result_ty(Type::Named("FileWriter".to_string()), io.clone())),
        )),
        // E2-M7: std.path helpers (D-IO1).
        ("core.path", "join") => Some((
            vec![(read, string.clone()), (read, string.clone())],
            Some(string),
        )),
        ("core.path", "parent" | "extension" | "normalize") => {
            Some((vec![(read, Type::String)], Some(Type::String)))
        }
        // D-URL1=A: typed URLs, query strings, component escaping, and MIME values.
        ("core.url", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("Url".to_string()), Type::String)),
        )),
        ("core.url", "from_parts") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::String),
                (
                    read,
                    Type::List(Box::new(Type::List(Box::new(Type::String)))),
                ),
                (read, Type::String),
            ],
            Some(result_ty(Type::Named("Url".to_string()), Type::String)),
        )),
        ("core.url", "file") => Some((
            vec![(read, Type::String)],
            Some(Type::Named("Url".to_string())),
        )),
        ("core.url", "data") => Some((
            vec![
                (read, Type::Named("Mime".to_string())),
                (read, Type::String),
            ],
            Some(Type::Named("Url".to_string())),
        )),
        ("core.url", "query") => Some((
            vec![(
                read,
                Type::List(Box::new(Type::List(Box::new(Type::String)))),
            )],
            Some(Type::String),
        )),
        ("core.url", "percent_encode") => Some((vec![(read, Type::String)], Some(Type::String))),
        ("core.url", "percent_decode") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::String, Type::String)),
        )),
        ("core.mime", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("Mime".to_string()), Type::String)),
        )),
        ("core.mime", "from_extension" | "extension") => Some((
            vec![(read, Type::String)],
            Some(Type::Option(Box::new(Type::String))),
        )),
        // D-TEXTUNICODE1: std-only Unicode scalar helpers.
        ("core.text.unicode", "scalar_count" | "byte_count") => {
            Some((vec![(read, Type::String)], Some(Type::Int)))
        }
        ("core.text.unicode", "is_ascii") => Some((vec![(read, Type::String)], Some(Type::Bool))),
        ("core.text.unicode", "lower" | "upper") => {
            Some((vec![(read, Type::String)], Some(Type::String)))
        }
        ("core.text.unicode", "scalars") => Some((
            vec![(read, Type::String)],
            Some(Type::List(Box::new(Type::String))),
        )),
        ("core.text", "nfc" | "nfd" | "nfkc" | "nfkd" | "casefold" | "lower" | "upper") => {
            Some((vec![(read, Type::String)], Some(Type::String)))
        }
        ("core.text", "caseless_eq") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(Type::Bool),
        )),
        ("core.text", "graphemes" | "words" | "sentences" | "scalars") => Some((
            vec![(read, Type::String)],
            Some(Type::List(Box::new(Type::String))),
        )),
        ("core.text", "scalar_count" | "byte_count") => {
            Some((vec![(read, Type::String)], Some(Type::Int)))
        }
        ("core.text", "is_alphabetic" | "is_numeric" | "is_whitespace" | "is_ascii") => {
            Some((vec![(read, Type::String)], Some(Type::Bool)))
        }
        ("core.text", "splitn" | "rsplitn") => Some((
            vec![(read, Type::String), (read, Type::String), (read, Type::Int)],
            Some(Type::List(Box::new(Type::String))),
        )),
        ("core.text", "trim" | "trim_start" | "trim_end") => {
            Some((vec![(read, Type::String)], Some(Type::String)))
        }
        ("core.text", "pad_start" | "pad_end" | "center") => Some((
            vec![(read, Type::String), (read, Type::Int), (read, Type::String)],
            Some(Type::String),
        )),
        ("core.text", "starts_any" | "ends_any") => Some((
            vec![(read, Type::String), (read, Type::List(Box::new(Type::String)))],
            Some(Type::Bool),
        )),
        ("core.text", "char_indices") => Some((
            vec![(read, Type::String)],
            Some(Type::List(Box::new(Type::String))),
        )),
        // jet.log/core.log: structured logging, typed fields, spans, sinks.
        ("jet.log", "info" | "warn" | "error" | "debug") => {
            Some((vec![(read, string.clone())], None))
        }
        ("jet.log", "field") => Some((
            vec![(read, string.clone()), (read, string.clone())],
            Some(Type::Named("LogField".to_string())),
        )),
        ("jet.log", "int") => Some((
            vec![(read, string.clone()), (read, Type::Int)],
            Some(Type::Named("LogField".to_string())),
        )),
        ("jet.log", "float") => Some((
            vec![(read, string.clone()), (read, Type::Float)],
            Some(Type::Named("LogField".to_string())),
        )),
        ("jet.log", "bool") => Some((
            vec![(read, string.clone()), (read, Type::Bool)],
            Some(Type::Named("LogField".to_string())),
        )),
        ("jet.log", "redact") => Some((
            vec![(read, string.clone())],
            Some(Type::Named("LogField".to_string())),
        )),
        ("jet.log", "info_fields" | "warn_fields" | "error_fields" | "debug_fields") => Some((
            vec![
                (read, string.clone()),
                (read, Type::List(Box::new(Type::Named("LogField".to_string())))),
            ],
            None,
        )),
        ("jet.log", "span") => Some((
            vec![(read, string.clone())],
            Some(Type::Named("LogSpan".to_string())),
        )),
        ("jet.log", "enter" | "close") => {
            Some((vec![(read, Type::Named("LogSpan".to_string()))], None))
        }
        ("jet.log", "set_sink") => {
            Some((vec![(read, string.clone()), (read, string.clone())], None))
        }
        ("jet.log", "sample_every") => Some((vec![(read, Type::Int)], None)),
        ("jet.log", "counter") => Some((
            vec![(read, string.clone()), (read, Type::Int)],
            Some(Type::Named("LogField".to_string())),
        )),
        ("jet.log", "otlp_file") => Some((vec![(read, string.clone())], None)),
        ("jet.log", "set_level") => Some((vec![(read, Type::String)], None)),
        // D-OBS3: set OTel trace_id for all subsequent log entries on this thread.
        ("jet.log", "set_trace_id") => Some((vec![(read, Type::String)], None)),
        // D-LOGFMT1=A: override log output format ("json" | "text").
        ("jet.log", "setup") => Some((vec![(read, Type::String)], None)),
        // jet.time: extended time utilities.
        ("jet.time", "now") => Some((vec![], Some(Type::Int))),
        ("jet.time", "format") => Some((
            vec![(read, Type::Int), (read, Type::String)],
            Some(Type::String),
        )),
        // jet.crypto: vetted hash functions (D-LR3).
        ("jet.crypto", "sha256") => Some((vec![(read, Type::List(Box::new(u8_ty())))], Some(Type::Named("Digest256".into())))),
        ("jet.crypto", "sha256_bytes") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::String),
        )),
        ("jet.crypto", "sha512_bytes" | "blake3_bytes") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::String),
        )),
        ("jet.crypto", "constant_time_eq") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(Type::Bool),
        )),
        ("jet.crypto", "hkdf_sha256") => Some((
            vec![
                (read, Type::Named("Secret".into())),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::Int),
            ],
            Some(result_ty(Type::Named("Secret".into()), Type::Named("CryptoError".into()))),
        )),
        ("jet.crypto", "x25519_public") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        ("jet.crypto", "x25519_shared") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        ("jet.crypto", "password_hash") => Some((
            vec![(read, Type::Named("Secret".into()))],
            Some(result_ty(Type::Named("PasswordHash".into()), Type::Named("CryptoError".into()))),
        )),
        ("jet.crypto", "password_hash_with_salt") => Some((
            vec![(read, Type::String), (read, Type::List(Box::new(u8_ty())))],
            Some(result_ty(Type::String, Type::String)),
        )),
        ("jet.crypto", "password_verify") => Some((
            vec![(read, Type::Named("Secret".into())), (read, Type::Named("PasswordHash".into()))],
            Some(result_ty(Type::Bool, Type::Named("CryptoError".into()))),
        )),
        // D-CRYPTOENV1=A: misuse-resistant envelope (RustCrypto via FFI bridge).
        ("jet.crypto", "seal") => Some((
            vec![
                (read, Type::List(Box::new(Type::Named("X25519PublicKey".into())))),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(Type::Named("Sealed".into()), Type::Named("CryptoError".into()))),
        )),
        ("jet.crypto", "open") => Some((
            vec![
                (read, Type::Named("X25519SecretKey".into())),
                (read, Type::Named("Sealed".into())),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::Named("CryptoError".into()))),
        )),
        ("jet.crypto", "file_seal" | "file_open") => Some((
            vec![(read, Type::List(Box::new(u8_ty()))), (read, Type::List(Box::new(u8_ty())))],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        ("jet.crypto", "sign") => Some((
            vec![
                (read, Type::Named("SigningKey".into())),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(Type::Named("Signature".into()), Type::Named("CryptoError".into()))),
        )),
        ("jet.crypto", "verify") => Some((
            vec![
                (read, Type::Named("VerifyKey".into())),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::Named("Signature".into())),
            ],
            Some(result_ty(Type::Bool, Type::Named("CryptoError".into()))),
        )),
        // D-CRYPTO-API1=A typed safe surface. Existing edition-2026 raw calls
        // are diagnosed/migrated separately; these signatures are nominal.
        ("jet.crypto", "wrap") => Some((
            vec![(read, Type::Named("Secret".into())), (read, Type::Named("X25519PublicKey".into()))],
            Some(result_ty(Type::Named("WrappedKey".into()), Type::Named("CryptoError".into()))),
        )),
        ("jet.crypto", "unwrap") => Some((
            vec![(read, Type::Named("X25519SecretKey".into())), (read, Type::Named("WrappedKey".into()))],
            Some(result_ty(Type::Named("Secret".into()), Type::Named("CryptoError".into()))),
        )),
        ("jet.crypto", "x25519") => Some((
            vec![(read, Type::Named("X25519SecretKey".into())), (read, Type::Named("X25519PublicKey".into()))],
            Some(result_ty(Type::Named("SharedSecret".into()), Type::Named("CryptoError".into()))),
        )),
        ("jet.crypto", "constant_time_equal") => Some((
            vec![(read, Type::Named("Secret".into())), (read, Type::Named("Secret".into()))], Some(Type::Bool),
        )),
        ("jet.crypto", "blake3") => Some((vec![(read, Type::List(Box::new(u8_ty())))], Some(Type::Named("Digest256".into())))),
        ("jet.crypto", "sha512") => Some((vec![(read, Type::List(Box::new(u8_ty())))], Some(Type::Named("Digest512".into())))),
        // D-CRYPTOENV1=A: expert-only raw AEAD (requires #Unsafe + expert import).
        ("core.crypto.expert", "aes256_gcm_seal" | "chacha20_seal") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        ("core.crypto.expert", "aes256_gcm_open" | "chacha20_open") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        // E2-M10: core.net — blocking TCP/UDP sockets (std::net, zero external deps).
        ("core.net", "tcp_listen") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("TcpListener".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "ip_addr") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("IpAddr".to_string()), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "ip_to_string") => Some((
            vec![(read, Type::Named("IpAddr".to_string()))],
            Some(Type::String),
        )),
        ("core.net", "ip_is_ipv4") => Some((
            vec![(read, Type::Named("IpAddr".to_string()))],
            Some(Type::Bool),
        )),
        ("core.net", "socket_addr") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(result_ty(
                Type::Named("SocketAddr".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "socket_addr_parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("SocketAddr".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "socket_host" | "socket_to_string") => Some((
            vec![(read, Type::Named("SocketAddr".to_string()))],
            Some(Type::String),
        )),
        ("core.net", "socket_port") => Some((
            vec![(read, Type::Named("SocketAddr".to_string()))],
            Some(Type::Int),
        )),
        ("core.net", "tcp_listen_addr") => Some((
            vec![(read, Type::Named("SocketAddr".to_string()))],
            Some(result_ty(
                Type::Named("TcpListener".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "tcp_accept") => Some((
            vec![(
                AccessConvention::Read,
                Type::Named("TcpListener".to_string()),
            )],
            Some(result_ty(
                Type::Named("TcpStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "tcp_connect") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("TcpStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "tcp_connect_addr") => Some((
            vec![(read, Type::Named("SocketAddr".to_string()))],
            Some(result_ty(
                Type::Named("TcpStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "tcp_connect_timeout") => Some((
            vec![
                (read, Type::Named("SocketAddr".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::Named("TcpStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "tcp_connect_happy") => Some((
            vec![(read, Type::String), (read, Type::Int), (read, Type::Int)],
            Some(result_ty(
                Type::Named("TcpStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "tcp_read") => Some((
            vec![(
                AccessConvention::Write,
                Type::Named("TcpStream".to_string()),
            )],
            Some(result_ty(Type::String, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_write") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::String),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_read_bytes") => Some((
            vec![
                (AccessConvention::Write, Type::Named("TcpStream".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::List(Box::new(u8_ty())),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "tcp_read_text") => Some((
            vec![
                (AccessConvention::Write, Type::Named("TcpStream".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(Type::String, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_write_bytes") => Some((
            vec![
                (AccessConvention::Write, Type::Named("TcpStream".to_string())),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(Type::Int, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_write_all_bytes") => Some((
            vec![
                (AccessConvention::Write, Type::Named("TcpStream".to_string())),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_write_text") => Some((
            vec![
                (AccessConvention::Write, Type::Named("TcpStream".to_string())),
                (read, Type::String),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_shutdown") => Some((
            vec![
                (AccessConvention::Write, Type::Named("TcpStream".to_string())),
                (read, Type::Named("NetShutdown".to_string())),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_close") => Some((
            vec![(AccessConvention::Write, Type::Named("TcpStream".to_string()))],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_ready") => Some((
            vec![
                (AccessConvention::Write, Type::Named("TcpStream".to_string())),
                (read, Type::Named("NetReadyInterest".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::Named("NetReady".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "ready_readable" | "ready_writable") => Some((
            vec![(read, Type::Named("NetReady".to_string()))],
            Some(Type::Bool),
        )),
        ("core.net", "error_operation" | "error_message") => Some((
            vec![(read, Type::Named("NetError".to_string()))],
            Some(Type::String),
        )),
        ("core.net", "error_address" | "error_name") => Some((
            vec![(read, Type::Named("NetError".to_string()))],
            Some(Type::Option(Box::new(Type::String))),
        )),
        ("core.net", "error_os_code") => Some((
            vec![(read, Type::Named("NetError".to_string()))],
            Some(Type::Option(Box::new(Type::Int))),
        )),
        ("core.net", "tcp_local_addr" | "tcp_peer_addr") => Some((
            vec![(read, Type::Named("TcpStream".to_string()))],
            Some(result_ty(Type::String, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tcp_local_socket_addr" | "tcp_peer_socket_addr") => Some((
            vec![(read, Type::Named("TcpStream".to_string()))],
            Some(result_ty(
                Type::Named("SocketAddr".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "listener_local_socket_addr") => Some((
            vec![(read, Type::Named("TcpListener".to_string()))],
            Some(result_ty(
                Type::Named("SocketAddr".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "set_timeout") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::Int),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "set_read_timeout" | "set_write_timeout") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TcpStream".to_string()),
                ),
                (read, Type::Int),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        // Convenience: send a complete HTTP/1.1 response and close the stream.
        ("core.net", "tcp_reply") => Some((
            vec![
                (AccessConvention::Move, Type::Named("TcpStream".to_string())),
                (read, Type::String),
                (read, Type::String),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "udp_bind") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("UdpSocket".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "udp_bind_addr") => Some((
            vec![(read, Type::Named("SocketAddr".to_string()))],
            Some(result_ty(
                Type::Named("UdpSocket".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "udp_local_addr") => Some((
            vec![(read, Type::Named("UdpSocket".to_string()))],
            Some(result_ty(
                Type::Named("SocketAddr".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "udp_set_timeout") => Some((
            vec![
                (read, Type::Named("UdpSocket".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "udp_send_to") => Some((
            vec![
                (read, Type::Named("UdpSocket".to_string())),
                (read, Type::String),
                (read, Type::Named("SocketAddr".to_string())),
            ],
            Some(result_ty(Type::Int, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "udp_recv_from") => Some((
            vec![
                (read, Type::Named("UdpSocket".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::Named("UdpPacket".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "udp_send_bytes_to") => Some((
            vec![
                (read, Type::Named("UdpSocket".to_string())),
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::Named("SocketAddr".to_string())),
            ],
            Some(result_ty(Type::Int, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "udp_receive") => Some((
            vec![
                (read, Type::Named("UdpSocket".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::Named("UdpPacket".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "udp_packet_data") => Some((
            vec![(read, Type::Named("UdpPacket".to_string()))],
            Some(Type::String),
        )),
        ("core.net", "udp_packet_addr") => Some((
            vec![(read, Type::Named("UdpPacket".to_string()))],
            Some(Type::Named("SocketAddr".to_string())),
        )),
        ("core.net", "udp_packet_bytes") => Some((
            vec![(read, Type::Named("UdpPacket".to_string()))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        ("core.net", "udp_packet_original_len") => Some((
            vec![(read, Type::Named("UdpPacket".to_string()))],
            Some(Type::Int),
        )),
        ("core.net", "udp_packet_truncated") => Some((
            vec![(read, Type::Named("UdpPacket".to_string()))],
            Some(Type::Bool),
        )),
        ("core.net", "unix_listen") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("UnixListener".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "unix_accept") => Some((
            vec![(read, Type::Named("UnixListener".to_string()))],
            Some(result_ty(
                Type::Named("UnixStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "unix_connect") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("UnixStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "unix_read") => Some((
            vec![(
                AccessConvention::Write,
                Type::Named("UnixStream".to_string()),
            )],
            Some(result_ty(Type::String, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "unix_write") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("UnixStream".to_string()),
                ),
                (read, Type::String),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "unix_read_bytes") => Some((
            vec![
                (AccessConvention::Write, Type::Named("UnixStream".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::List(Box::new(u8_ty())),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "unix_write_all_bytes") => Some((
            vec![
                (AccessConvention::Write, Type::Named("UnixStream".to_string())),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "unix_shutdown") => Some((
            vec![
                (AccessConvention::Write, Type::Named("UnixStream".to_string())),
                (read, Type::Named("NetShutdown".to_string())),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "unix_close") => Some((
            vec![(AccessConvention::Write, Type::Named("UnixStream".to_string()))],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "dns_a" | "dns_aaaa") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(result_ty(
                Type::List(Box::new(Type::Named("IpAddr".to_string()))),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "dns_a_at" | "dns_aaaa_at") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::List(Box::new(Type::Named("IpAddr".to_string()))),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "dns_txt") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(result_ty(Type::List(Box::new(Type::String)), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "dns_txt_at") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::Int),
            ],
            Some(result_ty(Type::List(Box::new(Type::String)), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "dns_srv") => Some((
            vec![(read, Type::String), (read, Type::Int)],
            Some(result_ty(
                Type::List(Box::new(Type::Named("DnsSrv".to_string()))),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "dns_srv_at") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::List(Box::new(Type::Named("DnsSrv".to_string()))),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "dns_srv_target") => Some((
            vec![(read, Type::Named("DnsSrv".to_string()))],
            Some(Type::String),
        )),
        ("core.net", "dns_srv_port" | "dns_srv_priority" | "dns_srv_weight") => Some((
            vec![(read, Type::Named("DnsSrv".to_string()))],
            Some(Type::Int),
        )),
        ("core.net", "tls_connect") => Some((
            vec![
                (AccessConvention::Move, Type::Named("TcpStream".to_string())),
                (read, Type::String),
            ],
            Some(result_ty(
                Type::Named("TlsStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.net", "tls_read") => Some((
            vec![(
                AccessConvention::Write,
                Type::Named("TlsStream".to_string()),
            )],
            Some(result_ty(Type::String, Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tls_write") => Some((
            vec![
                (
                    AccessConvention::Write,
                    Type::Named("TlsStream".to_string()),
                ),
                (read, Type::String),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.net", "tls_close") => Some((
            vec![(AccessConvention::Move, Type::Named("TlsStream".to_string()))],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.tls", "client") => Some((
            vec![
                (AccessConvention::Move, Type::Named("TcpStream".to_string())),
                (read, Type::String),
            ],
            Some(result_ty(
                Type::Named("TlsStream".to_string()),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.tls", "read") => Some((
            vec![
                (AccessConvention::Write, Type::Named("TlsStream".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(
                Type::List(Box::new(u8_ty())),
                Type::Named("NetError".to_string()),
            )),
        )),
        ("core.tls", "read_text") => Some((
            vec![
                (AccessConvention::Write, Type::Named("TlsStream".to_string())),
                (read, Type::Int),
            ],
            Some(result_ty(Type::String, Type::Named("NetError".to_string()))),
        )),
        ("core.tls", "write") => Some((
            vec![
                (AccessConvention::Write, Type::Named("TlsStream".to_string())),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(Type::Int, Type::Named("NetError".to_string()))),
        )),
        ("core.tls", "write_all") => Some((
            vec![
                (AccessConvention::Write, Type::Named("TlsStream".to_string())),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.tls", "write_text") => Some((
            vec![
                (AccessConvention::Write, Type::Named("TlsStream".to_string())),
                (read, Type::String),
            ],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        ("core.tls", "close") => Some((
            vec![(AccessConvention::Write, Type::Named("TlsStream".to_string()))],
            Some(result_ty(unit_ty(), Type::Named("NetError".to_string()))),
        )),
        // E2-M10: jet.http — HTTP client/server over blocking I/O.
        // GET / HEAD / DELETE requests (no body sent).
        ("jet.http", "get") => Some((
            vec![(read, Type::String)],
            Some(result_ty(
                Type::Named("HttpResponse".to_string()),
                Type::String,
            )),
        )),
        // POST / PUT / PATCH requests (body sent).
        ("jet.http", "post") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(
                Type::Named("HttpResponse".to_string()),
                Type::String,
            )),
        )),
        // serve blocks until the listener is closed; handler is called per request.
        // The handler type is resolved at the call site (lambda / fn pointer).
        ("jet.http", "serve") => None, // special-cased in check_core_call
        // D-REGEXENGINE1=A: core.regex — std-only linear regex. Every parsing
        // call returns a Result; the `Err` is a bad-pattern message at the boundary.
        ("jet.regex", "flags") => Some((
            vec![(read, Type::Bool), (read, Type::Bool), (read, Type::Bool)],
            Some(Type::Named("RegexFlags".to_string())),
        )),
        ("jet.regex", "compile") => Some((
            vec![(read, Type::String)],
            Some(result_ty(Type::Named("Regex".to_string()), Type::String)),
        )),
        ("jet.regex", "compile_with") => Some((
            vec![
                (read, Type::String),
                (read, Type::Named("RegexFlags".to_string())),
            ],
            Some(result_ty(Type::Named("Regex".to_string()), Type::String)),
        )),
        ("jet.regex", "is_match") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(Type::Bool, Type::String)),
        )),
        // First match anywhere: `Match?` (none when nothing matches).
        ("jet.regex", "match") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(
                Type::Option(Box::new(Type::Named("Match".to_string()))),
                Type::String,
            )),
        )),
        // First matched substring, or none.
        ("jet.regex", "find") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(
                Type::Option(Box::new(Type::String)),
                Type::String,
            )),
        )),
        ("jet.regex", "find_all" | "split") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(Type::List(Box::new(Type::String)), Type::String)),
        )),
        ("jet.regex", "matches") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(
                Type::List(Box::new(Type::Named("Match".to_string()))),
                Type::String,
            )),
        )),
        ("jet.regex", "split_limit") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::Int),
            ],
            Some(result_ty(Type::List(Box::new(Type::String)), Type::String)),
        )),
        ("jet.regex", "replace" | "replace_all") => Some((
            vec![
                (read, Type::String),
                (read, Type::String),
                (read, Type::String),
            ],
            Some(result_ty(Type::String, Type::String)),
        )),
        // D-CORE-COMPRESS1=A / D-DEP-ARCHIVE1=A: core.archive owns only
        // container formats. Stream gzip lives in core.compress.gzip.
        // zip_compress creates a single-entry zip archive.
        // Takes (name: String, data: [U8]) → [U8].
        ("core.archive", "zip_compress") => Some((
            vec![(read, Type::String), (read, Type::List(Box::new(u8_ty())))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: zip_decompress — extract first entry from a zip archive.
        // Takes [U8] → [U8]. Returns empty list on invalid input.
        ("core.archive", "zip_decompress") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: tar_add — append/replace a named entry in a tar archive.
        // Takes (archive: [U8], name: String, data: [U8]) → [U8].
        ("core.archive", "tar_add") => Some((
            vec![
                (read, Type::List(Box::new(u8_ty()))),
                (read, Type::String),
                (read, Type::List(Box::new(u8_ty()))),
            ],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: tar_get — extract a named entry from a tar archive.
        // Takes (archive: [U8], name: String) → [U8]. Empty on not-found or bad input.
        ("core.archive", "tar_get") => Some((
            vec![(read, Type::List(Box::new(u8_ty()))), (read, Type::String)],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        // D-DEP-ARCHIVE1=A: tar_names_json — list entry names as a JSON array string.
        // Takes [U8] → String. Returns "[]" on empty or invalid archive.
        ("core.archive", "tar_names_json") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::String),
        )),
        // D-RAYLIB1=A / D-FLAGSHIP-RAYLIB1=A: first bounded `core.raylib`
        // bridge. The surface is intentionally tiny and display-gated.
        ("core.raylib", "window_open") => Some((
            vec![(read, Type::Int), (read, Type::Int), (read, Type::String)],
            Some(Type::Named("RaylibWindow".to_string())),
        )),
        ("core.raylib", "window_should_close") => Some((
            vec![(read, Type::Named("RaylibWindow".to_string()))],
            Some(Type::Bool),
        )),
        ("core.raylib", "window_ready") => Some((
            vec![(read, Type::Named("RaylibWindow".to_string()))],
            Some(Type::Bool),
        )),
        ("core.raylib", "begin_drawing") => {
            Some((vec![(read, Type::Named("RaylibWindow".to_string()))], None))
        }
        ("core.raylib", "clear_background") => {
            Some((vec![(read, Type::Named("RaylibColor".to_string()))], None))
        }
        ("core.raylib", "draw_rectangle") => Some((
            vec![
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Named("RaylibColor".to_string())),
            ],
            None,
        )),
        ("core.raylib", "draw_text") => Some((
            vec![
                (read, Type::String),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Named("RaylibColor".to_string())),
            ],
            None,
        )),
        ("core.raylib", "end_drawing") => Some((vec![], None)),
        ("core.raylib", "close_window") => {
            Some((vec![(read, Type::Named("RaylibWindow".to_string()))], None))
        }
        ("core.raylib", "key_down") => {
            Some((vec![(read, Type::String)], Some(Type::Bool)))
        }
        ("core.raylib", "set_target_fps") => Some((vec![(read, Type::Int)], None)),
        ("core.raylib", "color") => Some((
            vec![
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
                (read, Type::Int),
            ],
            Some(Type::Named("RaylibColor".to_string())),
        )),
        // D-CORE-COMPRESS1=A / D-CODECS1: core.compress.gzip / zstd are the
        // only public stream-codec APIs. `compress` takes `[U8]` and is infallible;
        // `decompress` is fallible (malformed compressed stream → `Err(String)`),
        // following the same house style as core.encoding.hex/base64 `decode`.
        ("core.compress.gzip", "compress") | ("core.compress.zstd", "compress") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(Type::List(Box::new(u8_ty()))),
        )),
        ("core.compress.gzip", "decompress") | ("core.compress.zstd", "decompress") => Some((
            vec![(read, Type::List(Box::new(u8_ty())))],
            Some(result_ty(Type::List(Box::new(u8_ty())), Type::String)),
        )),
        // D-DBDRIVER1: jet.db — SQLite via rusqlite (bundled). `open`/`open_memory`
        // are the only module-level entry points; they PRODUCE a `DbConnection`
        // handle (mirrors `core.files`'s `open`/`create` producing a `FileReader`/
        // `FileWriter`). Every other operation — `query`/`query_one`/`execute`/
        // `begin`/`commit`/`rollback`/`close` — is an INSTANCE method dispatched
        // by the receiver's `DbConnection` type (see `check_db_connection_method`
        // below), not a second module-call surface. There is no raw-string
        // `execute(sql)` escape (D-DBDRIVER1's build plan: "must not expose a
        // generic `execute_raw(sql)` escape").
        ("jet.db", "open") => Some((
            vec![(read, Type::String)],
            Some(Type::Named("DbConnection".to_string())),
        )),
        ("jet.db", "open_memory") => Some((vec![], Some(Type::Named("DbConnection".to_string())))),
        ("jet.db", "params") => Some((
            vec![(read, Type::Named("Sql".to_string()))],
            Some(Type::List(Box::new(Type::Named(Syntax::TYPE_DB_VALUE.to_string())))),
        )),
        ("jet.db", "row_value") => Some((
            vec![(read, db_row_ty()), (read, Type::String)],
            Some(Type::Result {
                ok: Box::new(Type::Named(Syntax::TYPE_DB_VALUE.to_string())),
                err: Box::new(Type::String),
            }),
        )),
        ("jet.db", "row_int") => Some((
            vec![(read, db_row_ty()), (read, Type::String)],
            Some(Type::Result {
                ok: Box::new(Type::Int),
                err: Box::new(Type::String),
            }),
        )),
        ("jet.db", "row_float") => Some((
            vec![(read, db_row_ty()), (read, Type::String)],
            Some(Type::Result {
                ok: Box::new(Type::Float),
                err: Box::new(Type::String),
            }),
        )),
        ("jet.db", "row_text") => Some((
            vec![(read, db_row_ty()), (read, Type::String)],
            Some(Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::String),
            }),
        )),
        ("jet.db", "row_bool") => Some((
            vec![(read, db_row_ty()), (read, Type::String)],
            Some(Type::Result {
                ok: Box::new(Type::Bool),
                err: Box::new(Type::String),
            }),
        )),
        ("jet.db", "transaction") | ("jet.db", "migrate") => Some((
            vec![
                (read, Type::Named("DbConnection".to_string())),
                (read, Type::String),
                (read, Type::List(Box::new(Type::String))),
            ],
            Some(result_ty(Type::Int, db_error_ty())),
        )),
        // D-DEP-WASM1=A / D-PLUGIN1=B (c81): `core.plugin` — sandboxed WASM
        // Component Model plugin loader (wasmtime, runtime-side only, I6).
        // `load` is the only module-level entry point; it PRODUCES a `Plugin`
        // handle (mirrors `jet.db`'s `open` producing a `DbConnection`). The
        // actual calls (`.call`/`.call_int`) are instance methods dispatched by
        // the receiver's `Plugin` type (see `check_plugin_method` below).
        ("jet.plugin", "load") => Some((
            vec![(read, Type::String)],
            Some(Type::Named("Plugin".to_string())),
        )),
        // D-UUIDENC1=A: hex and base64 codecs. `encode` is infallible; `decode`
        // returns `[Byte] ? String` (invalid input → Err).
        ("core.encoding.hex", "encode") => {
            Some((vec![(read, list_u8.clone())], Some(Type::String)))
        }
        ("core.encoding.hex", "decode") => Some((
            vec![(read, Type::String)],
            Some(result_ty(list_u8.clone(), Type::String)),
        )),
        ("core.encoding.base64", "encode") => {
            Some((vec![(read, list_u8.clone())], Some(Type::String)))
        }
        ("core.encoding.base64", "decode") => Some((
            vec![(read, Type::String)],
            Some(result_ty(list_u8.clone(), Type::String)),
        )),
        ("core.encoding.base64", "encode_url") => {
            Some((vec![(read, list_u8.clone())], Some(Type::String)))
        }
        ("core.encoding.base64", "decode_url") => Some((
            vec![(read, Type::String)],
            Some(result_ty(list_u8.clone(), Type::String)),
        )),
        ("core.encoding.base32", "encode") => {
            Some((vec![(read, list_u8.clone())], Some(Type::String)))
        }
        ("core.encoding.base32", "decode") => Some((
            vec![(read, Type::String)],
            Some(result_ty(list_u8.clone(), Type::String)),
        )),
        // D-UUIDENC1=A: UUID v4 (system CSPRNG) and v7 (injectable Clock).
        // `v4()` reads /dev/urandom; `v7(clock)` extracts the timestamp from the
        // injected Clock so tests can produce a deterministic UUID.
        ("core.uuid", "v4") => Some((vec![], Some(Type::String))),
        ("core.uuid", "v7") => Some((
            vec![(read, Type::Named(crate::Syntax::CLOCK_TYPE.to_string()))],
            Some(Type::String),
        )),
        // D-OPTGC1: run a mark-sweep collection over traced `Gc<T>` roots.
        ("core.gc", "collect") => Some((vec![], Some(unit))),
        ("core.args", "spec") => Some((vec![], Some(Type::Named("ArgsSpec".to_string())))),
        // D-TERM1 (ratified 2026-06-22): terminal direct-input.
        // `term.read_key()` → `Key` (the key-event enum). No arguments.
        ("core.term", "read_key") => Some((
            vec![],
            Some(Type::Named(crate::Syntax::TYPE_KEY.to_string())),
        )),
        // D-FIDELITY-API1=A: runtime-global fidelity signal.
        ("core.perf", "fidelity") => Some((vec![], Some(float))),
        ("core.perf", "default_fidelity") => Some((vec![], Some(float.clone()))),
        ("core.perf", "override_fidelity") => Some((
            vec![(read, float)],
            Some(result_ty(unit.clone(), Type::String)),
        )),
        ("core.perf", "reset_fidelity") => Some((vec![], Some(unit))),
        // D-DECIMAL1: exact decimal parse from string.
        ("core.numeric", "decimal") => Some((
            vec![(read, string.clone())],
            Some(Type::Named(crate::Syntax::TYPE_DECIMAL.to_string())),
        )),
        // D-RENDERTGT2=A (c133 M1): UI geometry constructors.
        ("core.ui", "null_backend") => Some((vec![], Some(Type::Named("NullBackend".to_string())))),
        ("core.ui", "tui_backend") => Some((vec![], Some(Type::Named("TuiBackend".to_string())))),
        // D-UIDEVSHELL1=A (c134 Phase 8): native Linux GTK4 backend constructor.
        ("core.ui", "gtk_backend") => Some((vec![], Some(Type::Named("GtkBackend".to_string())))),
        ("core.ui", "point") => Some((
            vec![(read, float.clone()), (read, float)],
            Some(Type::Named("Point".to_string())),
        )),
        ("core.ui", "size") => Some((
            vec![(read, float.clone()), (read, float)],
            Some(Type::Named("Size".to_string())),
        )),
        ("core.ui", "rect") => Some((
            vec![
                (read, float.clone()),
                (read, float.clone()),
                (read, float.clone()),
                (read, float),
            ],
            Some(Type::Named("Rect".to_string())),
        )),
        ("core.ui", "constraint") => Some((
            vec![
                (read, float.clone()),
                (read, float.clone()),
                (read, float.clone()),
                (read, float),
            ],
            Some(Type::Named("SizeConstraint".to_string())),
        )),
        ("core.ui", "node") => Some((
            vec![(read, string.clone()), (read, float.clone()), (read, float)],
            Some(Type::Named("UiNode".to_string())),
        )),
        ("core.ui", "key_event") => Some((
            vec![(read, string)],
            Some(Type::Named("InputEvent".to_string())),
        )),
        ("core.ui", "resize_event") => Some((
            vec![(read, float.clone()), (read, float)],
            Some(Type::Named("InputEvent".to_string())),
        )),
        // D-A11YGATE1=B (c134 Phase 6): accessible-role node constructor + role
        // constants. `node_role` is the a11y-checked sibling of `node` — it's the
        // only UiNode constructor that carries a role, so it's the only one E2930
        // (unlabeled interactive control) needs to watch.
        ("core.ui", "node_role") => Some((
            vec![
                (read, string),
                (read, float.clone()),
                (read, float),
                (read, Type::Named("UiAriaRole".to_string())),
            ],
            Some(Type::Named("UiNode".to_string())),
        )),
        // D-STYLESHAPE1=A wiring: a node with an explicit fill color (a `#RRGGBB`
        // string, matching `JetPaintCmd::FillRect`'s existing color representation —
        // no new opaque type needed, this just makes the field settable from Jet).
        ("core.ui", "node_color") => Some((
            vec![
                (read, string.clone()),
                (read, float.clone()),
                (read, float),
                (read, string),
            ],
            Some(Type::Named("UiNode".to_string())),
        )),
        (
            "core.ui",
            "aria_role_button" | "aria_role_text_input" | "aria_role_label" | "aria_role_container",
        ) => Some((vec![], Some(Type::Named("UiAriaRole".to_string())))),
        // D-FLAGSHIP-WEBAPI1=A: first-party browser API for web flagship slices.
        ("core.web", "on") => Some((
            vec![
                (read, string.clone()),
                (read, string.clone()),
                (
                    read,
                    Type::Fn {
                        params: vec![Type::Named("WebEvent".to_string())],
                        ret: None,
                        effect_bound: None,
                    },
                ),
            ],
            None,
        )),
        ("core.web", "value") => Some((vec![(read, string.clone())], Some(Type::String))),
        ("core.web.storage.local" | "core.web.storage.session", "get") => Some((
            vec![(read, string.clone())],
            Some(Type::Option(Box::new(Type::String))),
        )),
        ("core.web.storage.local" | "core.web.storage.session", "set") => {
            Some((vec![(read, string.clone()), (read, string.clone())], None))
        }
        ("core.web.storage.local" | "core.web.storage.session", "remove") => {
            Some((vec![(read, string)], None))
        }
        ("core.web.storage.local" | "core.web.storage.session", "clear") => Some((vec![], None)),
        // c-devserver (owner-directed 2026-07-01): `devserver.for_app(file)` —
        // the constructor for a configurable `jet dev` server value. The
        // builder methods (`.html`/`.port`/`.serve`) are instance methods on
        // `DevServer`, dispatched through `devserver_method_return` (mirrors
        // `ui_backend_method_return`), not module-level names here.
        ("core.web.devserver", "for_app") => Some((
            vec![(read, string)],
            Some(Type::Named("DevServer".to_string())),
        )),
        // `devserver.app()` — zero-arg: watch the file `jet dev` launched
        // (passed to the running program via JET_DEV_FILE). The common case:
        // the file defining `fn dev()` is the file to watch, so no path is
        // spelled out at all.
        ("core.web.devserver", "app") => {
            Some((vec![], Some(Type::Named("DevServer".to_string()))))
        }
        _ => None,
    }
}
