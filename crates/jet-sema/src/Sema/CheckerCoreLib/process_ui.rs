use crate::AST::Type;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use super::alloc_ptrs::{io_error_ty, result_ty};
use super::core_types::unit_ty;
use super::serde_diags::wrong_core_arity;

/// D-ARGS1: type-check a method call on `ArgsSpec` (the builder).
/// Builder methods return `ArgsSpec` for chaining; `parse` returns `ParsedArgs ? String`.
/// Returns `Some(Some(ty))` for valid calls, `Some(None)` for void (none here),
/// `None` for unknown method (caller emits E0102).
pub(crate) fn args_spec_method_return(
    method: &str,
    n_args: usize,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    let spec_ty = Type::Named("ArgsSpec".to_string());
    match (method, n_args) {
        // .flag("name", "help text") → ArgsSpec  (boolean flag, no value)
        ("flag", 2) => Some(Some(spec_ty)),
        ("flag_short", 3) => Some(Some(spec_ty)),
        // .option("name", "help text", "METAVAR") → ArgsSpec  (value option)
        ("option", 3) => Some(Some(spec_ty)),
        ("option_short", 4)
        | ("option_default", 4)
        | ("option_env", 4)
        | ("option_choice", 4) => Some(Some(spec_ty)),
        ("option_int", 3)
        | ("option_float", 3)
        | ("repeat", 3)
        | ("required_option", 3) => Some(Some(spec_ty)),
        // .positional("name", "help text") → ArgsSpec  (positional argument)
        ("positional", 2) => Some(Some(spec_ty)),
        ("subcommand", 3) => Some(Some(spec_ty)),
        ("version", 1) => Some(Some(spec_ty)),
        ("completion", 1) => Some(Some(Type::String)),
        // .help() → String  (render --help text)
        ("help", 0) => Some(Some(Type::String)),
        // .parse([String]) → ParsedArgs ? String
        ("parse", 1) => Some(Some(result_ty(
            Type::Named("ParsedArgs".to_string()),
            Type::String,
        ))),
        ("parse_or_exit", 1) => Some(Some(Type::Named("ParsedArgs".to_string()))),
        // Arity mismatches
        ("flag", _) => {
            diags.push(Diagnostic::error(
                "E1301",
                format!("`flag` expects 2 arguments (name, help), got {}", n_args),
                "`ArgsSpec.flag(name, help)` registers a boolean flag like `--verbose`".to_string(),
                "pass exactly two strings: the flag name and a help description".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("flag_short", _) => {
            diags.push(Diagnostic::error(
                "E1301",
                format!(
                    "`flag_short` expects 3 arguments (name, short, help), got {}",
                    n_args
                ),
                "`ArgsSpec.flag_short(name, short, help)` registers a boolean flag with a `-v` alias".to_string(),
                "pass exactly three strings: long name, one-letter short name, and help text".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("option", _) => {
            diags.push(Diagnostic::error(
                "E1302",
                format!("`option` expects 3 arguments (name, help, metavar), got {}", n_args),
                "`ArgsSpec.option(name, help, metavar)` registers a value option like `--output FILE`".to_string(),
                "pass three strings: the option name, a help description, and a metavar like `FILE`".to_string(),
                Some(span),
            ));
            Some(None)
        }
        (
            "option_short"
            | "option_default"
            | "option_env"
            | "option_int"
            | "option_float"
            | "option_choice"
            | "repeat"
            | "required_option",
            _,
        ) => {
            diags.push(Diagnostic::error(
                "E1302",
                format!("`{}` was called with the wrong number of arguments", method),
                "`core.args` option builders declare long names, help text, and a value name; variants add one extra string where needed".to_string(),
                "check the `core.args` builder signature and pass the required strings".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("positional", _) => {
            diags.push(Diagnostic::error(
                "E1303",
                format!(
                    "`positional` expects 2 arguments (name, help), got {}",
                    n_args
                ),
                "`ArgsSpec.positional(name, help)` registers a required positional argument"
                    .to_string(),
                "pass exactly two strings: the positional name and a help description".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("subcommand", _) => {
            diags.push(Diagnostic::error(
                "E1303",
                format!(
                    "`subcommand` expects 3 arguments (name, help, spec), got {}",
                    n_args
                ),
                "`ArgsSpec.subcommand(name, help, spec)` gives a subcommand its own nested ArgsSpec".to_string(),
                "pass the subcommand name, help text, and an ArgsSpec built with `args.spec()`".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("version" | "completion", _) => {
            diags.push(Diagnostic::error(
                "E1303",
                format!("`{}` expects 1 argument, got {}", method, n_args),
                "`version(text)` configures `--version`; `completion(shell)` renders shell completion text".to_string(),
                "pass exactly one string".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("parse", _) => {
            diags.push(Diagnostic::error(
                "E1304",
                format!("`parse` expects 1 argument (argv), got {}", n_args),
                "`ArgsSpec.parse(argv)` parses a `[String]` (from `io.args()`) against the spec"
                    .to_string(),
                "pass exactly one argument: the argv list, e.g. `io.args()`".to_string(),
                Some(span),
            ));
            Some(None)
        }
        _ => None,
    }
}

/// D-ARGS1: type-check a method call on `ParsedArgs`.
pub(crate) fn parsed_args_method_return(
    method: &str,
    n_args: usize,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    match (method, n_args) {
        // .flag("name") → Bool
        ("flag", 1) => Some(Some(Type::Bool)),
        // .option("name") → String?
        ("option", 1) => Some(Some(Type::Option(Box::new(Type::String)))),
        ("option_int", 1) => Some(Some(Type::Option(Box::new(Type::Int)))),
        ("option_float", 1) => Some(Some(Type::Option(Box::new(Type::Float)))),
        ("options", 1) => Some(Some(Type::List(Box::new(Type::String)))),
        // .positional(n) → String?
        ("positional", 1) => Some(Some(Type::Option(Box::new(Type::String)))),
        ("subcommand", 0) => Some(Some(Type::Option(Box::new(Type::String)))),
        ("flag", _) => {
            diags.push(Diagnostic::error(
                "E1301",
                format!(
                    "`ParsedArgs.flag` expects 1 argument (flag name), got {}",
                    n_args
                ),
                "`parsed.flag(\"verbose\")` returns `true` when `--verbose` was passed".to_string(),
                "pass exactly one string: the flag name (without leading `--`)".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("option", _) => {
            diags.push(Diagnostic::error(
                "E1302",
                format!(
                    "`ParsedArgs.option` expects 1 argument (option name), got {}",
                    n_args
                ),
                format!(
                    "`parsed.option(\"output\")` returns the value of `--output VALUE`, or `{}`",
                    Syntax::LIT_NULL
                ),
                "pass exactly one string: the option name (without leading `--`)".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("option_int" | "option_float" | "options", _) => {
            diags.push(Diagnostic::error(
                "E1302",
                format!(
                    "`ParsedArgs.{}` expects 1 argument (option name), got {}",
                    method, n_args
                ),
                "`ParsedArgs` typed option queries read values already validated by `ArgsSpec.parse`".to_string(),
                "pass exactly one string: the option name (without leading `--`)".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("positional", _) => {
            diags.push(Diagnostic::error(
                "E1303",
                format!(
                    "`ParsedArgs.positional` expects 1 argument (index), got {}",
                    n_args
                ),
                format!(
                    "`parsed.positional(0)` returns the first positional argument, or `{}`",
                    Syntax::LIT_NULL
                ),
                "pass exactly one Int: the zero-based positional argument index".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("subcommand", _) => {
            diags.push(Diagnostic::error(
                "E1303",
                format!("`ParsedArgs.subcommand` expects 0 arguments, got {}", n_args),
                "`ParsedArgs.subcommand()` returns the matched subcommand name, if any".to_string(),
                "call it with no arguments".to_string(),
                Some(span),
            ));
            Some(None)
        }
        _ => None,
    }
}

/// D-PROCESS1: type-check `ProcessSpec` builder/run/spawn methods.
pub(crate) fn process_spec_method_return(
    method: &str,
    n_args: usize,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    let spec_ty = Type::Named("ProcessSpec".to_string());
    match (method, n_args) {
        ("cwd" | "env_remove" | "stdin" | "stdout" | "stderr", 1) => Some(Some(spec_ty)),
        ("env", 2) => Some(Some(spec_ty)),
        // D-PROCESS-SESSION1=A / D-PROCESS-SESSION2=D: `.terminal()` keeps
        // portable defaults; `.terminal(TerminalPolicy)` selects explicit
        // size and mode on the same ProcessSpec.
        ("terminal", 0 | 1) => Some(Some(spec_ty.clone())),
        ("env_clear" | "detached", 0) => Some(Some(spec_ty)),
        ("capabilities", 0) => Some(Some(Type::Apply {
            name: "Set".to_string(),
            args: vec![Type::String],
        })),
        ("timeout" | "output_limit", 1) => Some(Some(spec_ty)),
        ("run" | "run_checked", 0) => Some(Some(result_ty(
            Type::Named("ProcessResult".to_string()),
            io_error_ty(),
        ))),
        ("spawn", 0) => Some(Some(result_ty(
            Type::Named("ProcessChild".to_string()),
            io_error_ty(),
        ))),
        ("cwd" | "env_remove" | "stdin" | "stdout" | "stderr", _) => {
            diags.push(wrong_core_arity(method, 1, n_args, span));
            Some(None)
        }
        ("env", _) => {
            diags.push(wrong_core_arity(method, 2, n_args, span));
            Some(None)
        }
        ("env_clear" | "detached" | "run" | "run_checked" | "spawn" | "capabilities", _) => {
            diags.push(wrong_core_arity(method, 0, n_args, span));
            Some(None)
        }
        ("terminal", _) => {
            diags.push(Diagnostic::error(
                "E0104",
                format!("`terminal` expects 0 or 1 arguments, got {n_args}"),
                "`terminal()` uses portable defaults; `terminal(policy)` uses one TerminalPolicy"
                    .to_string(),
                "pass no argument or one TerminalPolicy".to_string(),
                Some(span),
            ));
            Some(None)
        }
        ("timeout" | "output_limit", _) => {
            diags.push(wrong_core_arity(method, 1, n_args, span));
            Some(None)
        }
        _ => None,
    }
}

/// D-PROCESS-SESSION2=D: type-check the terminal handle on ProcessChild.
pub(crate) fn terminal_session_method_return(
    method: &str,
    n_args: usize,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    match (method, n_args) {
        ("resize", 1) => Some(Some(result_ty(unit_ty(), io_error_ty()))),
        ("resize", _) => {
            diags.push(wrong_core_arity(method, 1, n_args, span));
            Some(None)
        }
        _ => None,
    }
}

/// D-PROCESS1: type-check `ProcessChild` control methods (`id`/`wait`/`kill`/
/// `terminate`/`interrupt`). Streaming I/O is a field access, not a method:
/// see `process_stdin_method_return`/`process_stream_method_return`.
pub(crate) fn process_child_method_return(
    method: &str,
    n_args: usize,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    let io = io_error_ty();
    match (method, n_args) {
        ("id", 0) => Some(Some(Type::Int)),
        ("wait", 0) => Some(Some(result_ty(
            Type::Named("ProcessResult".to_string()),
            io.clone(),
        ))),
        ("kill" | "terminate" | "interrupt", 0) => Some(Some(result_ty(unit_ty(), io))),
        ("id" | "wait" | "kill" | "terminate" | "interrupt", _) => {
            diags.push(wrong_core_arity(method, 0, n_args, span));
            Some(None)
        }
        _ => None,
    }
}

/// D-PROCESS1=A: type-check `.write(text)` on the `child.stdin` writer handle.
pub(crate) fn process_stdin_method_return(
    method: &str,
    n_args: usize,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    match (method, n_args) {
        ("write", 1) => Some(Some(result_ty(unit_ty(), io_error_ty()))),
        ("write", _) => {
            diags.push(wrong_core_arity(method, 1, n_args, span));
            Some(None)
        }
        _ => None,
    }
}

/// D-PROCESS1=A: type-check `.lines()` on a `child.stdout`/`child.stderr`
/// streaming reader handle. Mirrors `FileReader`/`StdinHandle` — the result is
/// a loop-source-only `ProcessLines` (E2502 in `bindings.rs`).
pub(crate) fn process_stream_method_return(
    method: &str,
    n_args: usize,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    match (method, n_args) {
        ("lines", 0) => Some(Some(Type::Named("ProcessLines".to_string()))),
        ("lines", _) => {
            diags.push(wrong_core_arity(method, 0, n_args, span));
            Some(None)
        }
        _ => None,
    }
}

/// D-RENDERTGT2=A (c133 M1/M2): type-check method calls on UI backends.
pub(crate) fn ui_backend_method_return(
    backend: &str,
    method: &str,
    n_args: usize,
    _span: Span,
    _diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    let size_ty = Type::Named("Size".to_string());
    let unit = unit_ty();
    match (backend, method, n_args) {
        ("NullBackend" | "TuiBackend" | "GtkBackend", "measure", 2) => Some(Some(size_ty)),
        ("NullBackend" | "TuiBackend" | "GtkBackend", "layout", 2) => Some(Some(unit)),
        ("NullBackend" | "TuiBackend" | "GtkBackend", "paint", 1) => Some(Some(unit)),
        ("NullBackend" | "TuiBackend" | "GtkBackend", "on_event", 1) => {
            Some(Some(Type::Named("EventResult".to_string())))
        }
        ("NullBackend", "commands", 0) => Some(Some(Type::List(Box::new(Type::String)))),
        ("TuiBackend", "frame_lines", 0) => Some(Some(Type::List(Box::new(Type::String)))),
        ("TuiBackend", "render_count", 0) => Some(Some(Type::Int)),
        // D-A11YGATE1=B (c134 Phase 6): keyboard focus routing over a flat
        // list of interactive nodes.
        ("NullBackend" | "TuiBackend" | "GtkBackend", "set_focus_group", 1) => Some(Some(unit)),
        ("NullBackend" | "TuiBackend" | "GtkBackend", "focused_label", 0) => {
            Some(Some(Type::String))
        }
        // D-UIDEVSHELL1=A (c134 Phase 8): native GTK4 retained-widget surface.
        // `label`/`button` create a widget and return its handle; `set_text`/
        // `set_size`/`set_color` mutate a live widget; `on_click(id, handler)`
        // wires a button; `present(title)` opens the window (no-op headless).
        ("GtkBackend", "label", 1) => Some(Some(Type::Int)),
        ("GtkBackend", "button", 1) => Some(Some(Type::Int)),
        ("GtkBackend", "set_text", 2) => Some(Some(unit)),
        ("GtkBackend", "set_size", 3) => Some(Some(unit)),
        ("GtkBackend", "set_color", 2) => Some(Some(unit)),
        ("GtkBackend", "on_click", 2) => Some(Some(unit)),
        ("GtkBackend", "present", 1) => Some(Some(unit)),
        _ => None,
    }
}

/// c-devserver (owner-directed 2026-07-01): type-check builder method calls
/// on a `DevServer` value (`.html`/`.port`/`.serve`). `.html`/`.port` return
/// `DevServer` for chaining, but are equally valid as bare statements (they
/// are not `#MustUse`) — the reference example calls them as plain
/// statements without reassigning. `.serve()` blocks forever and returns
/// nothing.
pub(crate) fn devserver_method_return(
    method: &str,
    n_args: usize,
    _span: Span,
    _diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    let devserver_ty = Type::Named("DevServer".to_string());
    let unit = unit_ty();
    match (method, n_args) {
        ("html", 1) => Some(Some(devserver_ty)),
        ("port", 1) => Some(Some(devserver_ty)),
        ("serve", 0) => Some(Some(unit)),
        _ => None,
    }
}

/// D-WEBAPP1=D / D-WEBAUTHOR1=D: builder methods on `WebApp`. Chainable methods
/// return `WebApp`; zero-arg render-mode setters also return `WebApp`.
pub(crate) fn webapp_method_return(
    method: &str,
    n_args: usize,
    _span: Span,
    _diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    let app = Type::Named("WebApp".to_string());
    let unit = unit_ty();
    match (method, n_args) {
        ("route" | "page" | "layout", 2) => Some(Some(app)),
        ("action" | "form" | "data", 2) => Some(Some(app)),
        ("mount", 2 | 3 | 4) => Some(Some(app)),
        ("routes", 1) => Some(Some(app)),
        (
            "csr" | "ssr" | "ssg" | "stream" | "streaming" | "island" | "hydration_dev"
                | "hydration_release",
            0,
        ) => Some(Some(app)),
        ("security" | "assets" | "split" | "code_split" | "cache" | "a11y" | "adapter", 1) => {
            Some(Some(app))
        }
        ("facts_json", 0) => Some(Some(Type::String)),
        ("serve", 0) => Some(Some(unit)),
        _ => None,
    }
}
