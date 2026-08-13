//! Whole-program interpreter hosts for `core.auth` / `core.db` / `core.crypto` (#1254).
//!
//! Same bridge runtimes as Cranelift hosts; CtValue at the boundary. Installed
//! only around `run_whole_interp` so comptime/REPL stay pure / native-denied.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use jet_codegen::AST::{CtFloat, CtKey, CtValue, Type};
use jet_codegen::Diagnostics::{Diagnostic, Span};
use jet_foundation::Prelude::jet_as_bytes as as_bytes;

use crate::Crypto;
use crate::DB;
use crate::IO;
use jet_codegen::Comptime::ServicesLite as service_prelude;

trait JetShow {
    fn jet_show(&self) -> String;
}

// The shared DB wire fragment receives the host's row carrier through this
// name. The interpreter uses its native map until converting to CtValue.
type JetMap<K, V> = BTreeMap<K, V>;

mod wire {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/DBPluginWire.rs");
}

fn unsupported(what: &str, span: Span) -> Diagnostic {
    jet_foundation::Prelude::jet_e0956_unsupported(what, span)
}

// The interpreter only owns CtValue handles. Process policy and lifecycle
// semantics stay in the exact Prelude fragments used by AOT; this module
// supplies the native values and logical-environment hooks those fragments
// need at the interpreter boundary.
pub(crate) mod process_prelude {
    use std::ffi::{OsStr, OsString};

    use jet_foundation::Outcome::{jet_outcome_of, JetAbsent, JetOutcome};
    use jet_codegen::scheduler::{jet_scheduler_wait_without_unwind, JetSchedulerWait};
    #[cfg(unix)]
    use jet_codegen::scheduler::{
        jet_scheduler_raw_io_handle, jet_scheduler_raw_io_set_nonblocking,
        jet_scheduler_raw_io_write_wait,
    };

    mod terminal_default {
        include!("../../jet-codegen/src/Prelude/TerminalDefault.rs");
    }

    mod jet_process_pty {
        pub use jet_codegen::process_pty::*;
    }

    pub(crate) mod jet_std {
        use super::{JetAbsent, JetOutcome};

        #[derive(Clone, Copy, Debug, PartialEq)]
        pub enum IOOperation {
            Read,
            Write,
            Flush,
            Connect,
            Accept,
            Close,
            Resolve,
            Codec,
        }

        #[derive(Clone, Debug, PartialEq)]
        pub struct IOContext {
            pub operation: IOOperation,
            pub resource: JetOutcome<String, JetAbsent>,
            pub os_code: JetOutcome<i64, JetAbsent>,
            pub cause: JetOutcome<String, JetAbsent>,
        }

        impl IOContext {
            pub fn new(
                operation: IOOperation,
                resource: Option<String>,
                os_code: Option<i64>,
                cause: Option<String>,
            ) -> Self {
                Self {
                    operation,
                    resource: super::jet_outcome_of(resource),
                    os_code: super::jet_outcome_of(os_code),
                    cause: super::jet_outcome_of(cause),
                }
            }
        }

        #[derive(Clone, Debug, PartialEq)]
        pub enum IOError {
            InvalidInput(IOContext),
            NotFound(IOContext),
            PermissionDenied(IOContext),
            TimedOut(IOContext),
            Cancelled(IOContext),
            Closed(IOContext),
            Protocol(IOContext),
            Other(IOContext),
        }

        impl IOError {
            pub fn other(
                operation: IOOperation,
                resource: Option<String>,
                cause: impl ToString,
            ) -> Self {
                Self::Other(IOContext::new(
                    operation,
                    resource,
                    None,
                    Some(cause.to_string()),
                ))
            }
        }

        #[derive(Clone, Debug, PartialEq)]
        pub enum EnvError {
            InvalidName,
            InvalidValue,
            NonUnicode,
        }

        impl EnvError {
            pub fn jet_show(&self) -> String {
                match self {
                    Self::InvalidName => "invalid environment variable name".to_string(),
                    Self::InvalidValue => "invalid environment variable value".to_string(),
                    Self::NonUnicode => "environment contains non-unicode data".to_string(),
                }
            }
        }

        #[derive(Clone, Debug, PartialEq)]
        pub struct ProcessResult {
            pub code: i64,
            pub output: String,
            pub errors: String,
            pub success: bool,
            pub signal: Option<i64>,
            pub timed_out: bool,
        }

        #[derive(Clone, Debug, PartialEq)]
        pub enum ProcessStreamMode {
            Stream,
            Inherit,
            Capture,
        }

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct TerminalSize {
            pub cols: i64,
            pub rows: i64,
        }

        impl Default for TerminalSize {
            fn default() -> Self {
                Self {
                    cols: super::terminal_default::JET_TERMINAL_DEFAULT_COLS,
                    rows: super::terminal_default::JET_TERMINAL_DEFAULT_ROWS,
                }
            }
        }

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum TerminalMode {
            Raw,
            Cooked,
        }

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct TerminalPolicy {
            pub size: TerminalSize,
            pub mode: TerminalMode,
        }

        impl Default for TerminalPolicy {
            fn default() -> Self {
                Self {
                    size: TerminalSize::default(),
                    mode: TerminalMode::Cooked,
                }
            }
        }

        #[derive(Clone, Debug)]
        pub struct TerminalSession {
            pub master: std::rc::Rc<std::fs::File>,
        }

        impl PartialEq for TerminalSession {
            fn eq(&self, other: &Self) -> bool {
                std::rc::Rc::ptr_eq(&self.master, &other.master)
            }
        }

        impl Eq for TerminalSession {}

        #[derive(Debug)]
        pub enum ProcessStdin {
            Pipe(std::process::ChildStdin),
            Terminal(std::fs::File),
        }

        #[derive(Debug)]
        pub enum ProcessReader {
            Stdout(std::process::ChildStdout),
            Stderr(std::process::ChildStderr),
            Terminal(std::fs::File),
        }

        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct Duration {
            pub ns: i64,
        }

        impl Duration {
            pub fn as_millis(self) -> i64 {
                self.ns / 1_000_000
            }
        }

        #[derive(Clone, Debug, PartialEq)]
        pub struct ProcessSpec {
            pub cmd: Vec<String>,
            pub cwd: Option<String>,
            pub env_clear: bool,
            pub env_set: Vec<(String, String)>,
            pub env_remove: Vec<String>,
            pub stdin: Option<ProcessStreamMode>,
            pub stdout: ProcessStreamMode,
            pub stderr: ProcessStreamMode,
            pub timeout_ms: Option<i64>,
            pub output_limit: Option<i64>,
            pub detached: bool,
            pub terminal: Option<TerminalPolicy>,
        }

        #[derive(Clone, Debug)]
        pub struct ProcessChild {
            pub inner: std::rc::Rc<std::cell::RefCell<Option<std::process::Child>>>,
            pub wait_result: std::rc::Rc<std::cell::RefCell<Option<ProcessResult>>>,
            pub stdin: std::rc::Rc<std::cell::RefCell<Option<ProcessStdin>>>,
            pub stdout:
                std::rc::Rc<std::cell::RefCell<Option<std::io::BufReader<ProcessReader>>>>,
            pub stderr:
                std::rc::Rc<std::cell::RefCell<Option<std::io::BufReader<ProcessReader>>>>,
            pub terminal: JetOutcome<TerminalSession, JetAbsent>,
            pub timeout_ms: Option<i64>,
            pub started: std::time::Instant,
        }

        impl PartialEq for ProcessChild {
            fn eq(&self, other: &Self) -> bool {
                std::rc::Rc::ptr_eq(&self.inner, &other.inner)
            }
        }
    }

    type JetEnvEntries = Vec<(OsString, OsString)>;

    fn jet_std_env_snapshot_raw() -> JetEnvEntries {
        crate::CoreHost::jit_env_snapshot_raw()
    }

    fn jet_env_key_eq(left: &OsStr, right: &OsStr) -> bool {
        crate::CoreHost::jit_env_key_eq(left, right)
    }

    fn jet_env_validate_name(name: &str) -> Result<(), jet_std::EnvError> {
        crate::CoreHost::jit_env_validate_name(name)
            .map_err(|_| jet_std::EnvError::InvalidName)
    }

    fn jet_env_validate_value(value: &str) -> Result<(), jet_std::EnvError> {
        crate::CoreHost::jit_env_validate_value(value)
            .map_err(|_| jet_std::EnvError::InvalidValue)
    }

    fn jet_scheduler_park_ms(wait_kind: &'static str, millis: u64) {
        jet_codegen::scheduler::jet_scheduler_park_ms(wait_kind, millis);
    }

    include!("../../jet-codegen/src/Prelude/CoreLib/Top/ProcessPolicy.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/ProcessSpec.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/Process.rs");

    pub(crate) use jet_std::{
        Duration, IOError, IOContext, IOOperation, ProcessChild, ProcessReader, ProcessResult,
        ProcessSpec, ProcessStdin, ProcessStreamMode, TerminalMode, TerminalPolicy,
        TerminalSession, TerminalSize,
    };

    pub(crate) fn spec_new(cmd: Vec<String>) -> ProcessSpec {
        jet_std_process_cmd(&cmd)
    }

    pub(crate) fn spec_cwd(spec: ProcessSpec, cwd: &String) -> ProcessSpec {
        jet_process_spec_cwd(spec, cwd)
    }

    pub(crate) fn spec_env(spec: ProcessSpec, name: &String, value: &String) -> ProcessSpec {
        jet_process_spec_env(spec, name, value)
    }

    pub(crate) fn spec_env_remove(spec: ProcessSpec, name: &String) -> ProcessSpec {
        jet_process_spec_env_remove(spec, name)
    }

    pub(crate) fn spec_env_clear(spec: ProcessSpec) -> ProcessSpec {
        jet_process_spec_env_clear(spec)
    }

    pub(crate) fn spec_stdin(spec: ProcessSpec, mode: &ProcessStreamMode) -> ProcessSpec {
        jet_process_spec_stdin(spec, mode)
    }

    pub(crate) fn spec_stdout(spec: ProcessSpec, mode: &ProcessStreamMode) -> ProcessSpec {
        jet_process_spec_stdout(spec, mode)
    }

    pub(crate) fn spec_stderr(spec: ProcessSpec, mode: &ProcessStreamMode) -> ProcessSpec {
        jet_process_spec_stderr(spec, mode)
    }

    pub(crate) fn spec_timeout(spec: ProcessSpec, timeout: &Duration) -> ProcessSpec {
        jet_process_spec_timeout(spec, timeout)
    }

    pub(crate) fn spec_output_limit(spec: ProcessSpec, output_limit: i64) -> ProcessSpec {
        jet_process_spec_output_limit(spec, output_limit)
    }

    pub(crate) fn spec_detached(spec: ProcessSpec) -> ProcessSpec {
        jet_process_spec_detached(spec)
    }

    pub(crate) fn spec_terminal(spec: ProcessSpec) -> ProcessSpec {
        jet_process_spec_terminal(spec)
    }

    pub(crate) fn spec_terminal_with_policy(
        spec: ProcessSpec,
        policy: &TerminalPolicy,
    ) -> ProcessSpec {
        jet_process_spec_terminal_with_policy(spec, policy)
    }

    pub(crate) fn spec_capabilities(spec: &ProcessSpec) -> std::collections::HashSet<String> {
        jet_process_spec_capabilities(spec)
    }

    pub(crate) fn spec_run(spec: &ProcessSpec) -> Result<ProcessResult, IOError> {
        jet_process_spec_run(spec)
    }

    pub(crate) fn spec_run_checked(spec: &ProcessSpec) -> Result<ProcessResult, IOError> {
        jet_process_spec_run_checked(spec)
    }

    pub(crate) fn spec_pipeline(specs: &Vec<ProcessSpec>) -> Result<ProcessResult, IOError> {
        jet_process_spec_pipeline(specs)
    }

    pub(crate) fn spec_spawn(spec: &ProcessSpec) -> Result<ProcessChild, IOError> {
        jet_process_spec_spawn(spec)
    }

    pub(crate) fn child_id(child: &ProcessChild) -> i64 {
        jet_process_child_id(child)
    }

    pub(crate) fn child_wait(child: &ProcessChild) -> Result<ProcessResult, IOError> {
        jet_process_child_wait(child)
    }

    pub(crate) fn child_exited(child: &ProcessChild) -> Result<bool, IOError> {
        jet_process_child_exited(child)
    }

    pub(crate) fn child_kill(child: &ProcessChild) -> Result<(), IOError> {
        jet_process_child_kill(child)
    }

    pub(crate) fn child_terminate(child: &ProcessChild) -> Result<(), IOError> {
        jet_process_child_terminate(child)
    }

    pub(crate) fn child_interrupt(child: &ProcessChild) -> Result<(), IOError> {
        jet_process_child_interrupt(child)
    }

    pub(crate) fn stream_next_line(
        reader: &std::rc::Rc<std::cell::RefCell<Option<std::io::BufReader<ProcessReader>>>>,
    ) -> Result<Option<String>, IOError> {
        jet_process_stream_next_line(reader)
    }

    pub(crate) fn terminal_session_resize(
        session: &TerminalSession,
        size: &TerminalSize,
    ) -> Result<(), IOError> {
        jet_terminal_session_resize(session, size)
    }

}

fn interpreter_process_spec(cmd: Vec<CtValue>) -> CtValue {
    let words = cmd
        .into_iter()
        .filter_map(|value| match value {
            CtValue::Str(value) => Some(value),
            _ => None,
        })
        .collect();
    process_spec_value(&process_prelude::spec_new(words))
}

fn process_spec_field<'a>(recv: &'a CtValue, wanted: &str) -> Option<&'a CtValue> {
    let CtValue::Struct { type_name, fields } = recv else {
        return None;
    };
    (type_name == "ProcessSpec")
        .then(|| fields.iter().find_map(|(name, value)| (name == wanted).then_some(value)))
        .flatten()
}

fn process_field<'a>(value: &'a CtValue, wanted: &str) -> Option<&'a CtValue> {
    let CtValue::Struct { fields, .. } = value else {
        return None;
    };
    fields
        .iter()
        .find_map(|(name, value)| (name == wanted).then_some(value))
}

fn process_optional(
    value: Option<&CtValue>,
    what: &str,
    span: Span,
) -> Result<Option<CtValue>, Diagnostic> {
    match value {
        None => Ok(None),
        Some(value) if value.is_clean_stop() => Ok(None),
        Some(CtValue::Present(value)) => Ok(Some((**value).clone())),
        Some(_) => Err(unsupported(what, span)),
    }
}

fn process_string(value: &CtValue, what: &str, span: Span) -> Result<String, Diagnostic> {
    match value {
        CtValue::Str(value) => Ok(value.clone()),
        _ => Err(unsupported(what, span)),
    }
}

fn process_int(value: &CtValue, what: &str, span: Span) -> Result<i64, Diagnostic> {
    match value {
        CtValue::Int(value) => Ok(*value),
        _ => Err(unsupported(what, span)),
    }
}

fn process_bool(value: Option<&CtValue>, default: bool, what: &str, span: Span) -> Result<bool, Diagnostic> {
    match value {
        None => Ok(default),
        Some(CtValue::Bool(value)) => Ok(*value),
        Some(_) => Err(unsupported(what, span)),
    }
}

fn process_stream_mode(
    value: &CtValue,
    what: &str,
    span: Span,
) -> Result<process_prelude::ProcessStreamMode, Diagnostic> {
    let CtValue::Enum { variant, .. } = value else {
        return Err(unsupported(what, span));
    };
    match variant.as_str() {
        "Stream" => Ok(process_prelude::ProcessStreamMode::Stream),
        "Inherit" => Ok(process_prelude::ProcessStreamMode::Inherit),
        "Capture" => Ok(process_prelude::ProcessStreamMode::Capture),
        _ => Err(unsupported(what, span)),
    }
}

fn process_stream_mode_value(mode: &process_prelude::ProcessStreamMode) -> CtValue {
    let variant = match mode {
        process_prelude::ProcessStreamMode::Stream => "Stream",
        process_prelude::ProcessStreamMode::Inherit => "Inherit",
        process_prelude::ProcessStreamMode::Capture => "Capture",
    };
    CtValue::Enum {
        type_name: "ProcessStreamMode".to_string(),
        variant: variant.to_string(),
        args: vec![],
    }
}

fn process_duration(value: &CtValue, what: &str, span: Span) -> Result<process_prelude::Duration, Diagnostic> {
    let CtValue::Struct { type_name, .. } = value else {
        return Err(unsupported(what, span));
    };
    if type_name != "Duration" {
        return Err(unsupported(what, span));
    }
    let ns = process_int(
        process_field(value, "ns").ok_or_else(|| unsupported(what, span))?,
        what,
        span,
    )?;
    Ok(process_prelude::Duration { ns })
}

fn process_duration_value(duration: process_prelude::Duration) -> CtValue {
    CtValue::Struct {
        type_name: "Duration".to_string(),
        fields: vec![("ns".to_string(), CtValue::Int(duration.ns))],
    }
}

fn process_terminal_mode(
    value: &CtValue,
    what: &str,
    span: Span,
) -> Result<process_prelude::TerminalMode, Diagnostic> {
    let CtValue::Enum { variant, .. } = value else {
        return Err(unsupported(what, span));
    };
    match variant.as_str() {
        "Raw" => Ok(process_prelude::TerminalMode::Raw),
        "Cooked" => Ok(process_prelude::TerminalMode::Cooked),
        _ => Err(unsupported(what, span)),
    }
}

fn process_terminal_mode_value(mode: &process_prelude::TerminalMode) -> CtValue {
    let variant = match mode {
        process_prelude::TerminalMode::Raw => "Raw",
        process_prelude::TerminalMode::Cooked => "Cooked",
    };
    CtValue::Enum {
        type_name: "TerminalMode".to_string(),
        variant: variant.to_string(),
        args: vec![],
    }
}

fn process_terminal_policy(
    value: &CtValue,
    what: &str,
    span: Span,
) -> Result<process_prelude::TerminalPolicy, Diagnostic> {
    let CtValue::Struct { type_name, .. } = value else {
        return Err(unsupported(what, span));
    };
    if type_name != "TerminalPolicy" {
        return Err(unsupported(what, span));
    }
    let size = process_field(value, "size").ok_or_else(|| unsupported(what, span))?;
    let CtValue::Struct { type_name, .. } = size else {
        return Err(unsupported(what, span));
    };
    if type_name != "TerminalSize" {
        return Err(unsupported(what, span));
    }
    let cols = process_int(
        process_field(size, "cols").ok_or_else(|| unsupported(what, span))?,
        what,
        span,
    )?;
    let rows = process_int(
        process_field(size, "rows").ok_or_else(|| unsupported(what, span))?,
        what,
        span,
    )?;
    let mode = process_terminal_mode(
        process_field(value, "mode").ok_or_else(|| unsupported(what, span))?,
        what,
        span,
    )?;
    Ok(process_prelude::TerminalPolicy {
        size: process_prelude::TerminalSize { cols, rows },
        mode,
    })
}

fn process_terminal_policy_value(policy: &process_prelude::TerminalPolicy) -> CtValue {
    CtValue::Struct {
        type_name: "TerminalPolicy".to_string(),
        fields: vec![
            (
                "size".to_string(),
                CtValue::Struct {
                    type_name: "TerminalSize".to_string(),
                    fields: vec![
                        ("cols".to_string(), CtValue::Int(policy.size.cols)),
                        ("rows".to_string(), CtValue::Int(policy.size.rows)),
                    ],
                },
            ),
            ("mode".to_string(), process_terminal_mode_value(&policy.mode)),
        ],
    }
}

fn process_spec_from_value(recv: &CtValue, span: Span) -> Result<process_prelude::ProcessSpec, Diagnostic> {
    let CtValue::Struct { type_name, .. } = recv else {
        return Err(unsupported("ProcessSpec receiver", span));
    };
    if type_name != "ProcessSpec" {
        return Err(unsupported("ProcessSpec receiver", span));
    }
    let CtValue::List(command) = process_spec_field(recv, "cmd")
        .ok_or_else(|| unsupported("ProcessSpec.cmd", span))?
    else {
        return Err(unsupported("ProcessSpec.cmd", span));
    };
    let mut words = Vec::with_capacity(command.len());
    for value in command {
        words.push(process_string(value, "ProcessSpec.cmd", span)?);
    }
    let mut spec = process_prelude::spec_new(words);
    spec.cwd = process_optional(process_spec_field(recv, "cwd"), "ProcessSpec.cwd", span)?
        .map(|value| process_string(&value, "ProcessSpec.cwd", span))
        .transpose()?;
    spec.env_clear = process_bool(
        process_spec_field(recv, "env_clear"),
        false,
        "ProcessSpec.env_clear",
        span,
    )?;
    if let Some(value) = process_spec_field(recv, "env_set") {
        let CtValue::List(entries) = value else {
            return Err(unsupported("ProcessSpec.env_set", span));
        };
        for entry in entries {
            let CtValue::List(pair) = entry else {
                return Err(unsupported("ProcessSpec.env_set", span));
            };
            let [name, value] = pair.as_slice() else {
                return Err(unsupported("ProcessSpec.env_set", span));
            };
            spec.env_set.push((
                process_string(name, "ProcessSpec.env_set", span)?,
                process_string(value, "ProcessSpec.env_set", span)?,
            ));
        }
    }
    if let Some(value) = process_spec_field(recv, "env_remove") {
        let CtValue::List(names) = value else {
            return Err(unsupported("ProcessSpec.env_remove", span));
        };
        for name in names {
            spec.env_remove
                .push(process_string(name, "ProcessSpec.env_remove", span)?);
        }
    }
    spec.stdin = process_optional(process_spec_field(recv, "stdin"), "ProcessSpec.stdin", span)?
        .map(|value| process_stream_mode(&value, "ProcessSpec.stdin", span))
        .transpose()?;
    spec.stdout = match process_spec_field(recv, "stdout") {
        Some(value) => process_stream_mode(value, "ProcessSpec.stdout", span)?,
        None => process_prelude::ProcessStreamMode::Capture,
    };
    spec.stderr = match process_spec_field(recv, "stderr") {
        Some(value) => process_stream_mode(value, "ProcessSpec.stderr", span)?,
        None => process_prelude::ProcessStreamMode::Capture,
    };
    spec.timeout_ms = process_optional(process_spec_field(recv, "timeout"), "ProcessSpec.timeout", span)?
        .map(|value| process_duration(&value, "ProcessSpec.timeout", span).map(|duration| duration.as_millis()))
        .transpose()?;
    spec.output_limit = process_optional(
        process_spec_field(recv, "output_limit"),
        "ProcessSpec.output_limit",
        span,
    )?
    .map(|value| process_int(&value, "ProcessSpec.output_limit", span))
    .transpose()?;
    spec.detached = process_bool(
        process_spec_field(recv, "detached"),
        false,
        "ProcessSpec.detached",
        span,
    )?;
    spec.terminal = process_optional(
        process_spec_field(recv, "terminal"),
        "ProcessSpec.terminal",
        span,
    )?
    .map(|value| process_terminal_policy(&value, "ProcessSpec.terminal", span))
    .transpose()?;
    Ok(spec)
}

fn process_spec_value(spec: &process_prelude::ProcessSpec) -> CtValue {
    let optional = |value: Option<CtValue>, ty: Type| match value {
        Some(value) => CtValue::Present(Box::new(value)),
        None => CtValue::absent(ty),
    };
    let env_set = spec
        .env_set
        .iter()
        .map(|(name, value)| {
            CtValue::List(vec![CtValue::Str(name.clone()), CtValue::Str(value.clone())])
        })
        .collect();
    CtValue::Struct {
        type_name: "ProcessSpec".to_string(),
        fields: vec![
            (
                "cmd".to_string(),
                CtValue::List(spec.cmd.iter().cloned().map(CtValue::Str).collect()),
            ),
            (
                "cwd".to_string(),
                optional(spec.cwd.clone().map(CtValue::Str), Type::String),
            ),
            ("env_clear".to_string(), CtValue::Bool(spec.env_clear)),
            ("env_set".to_string(), CtValue::List(env_set)),
            (
                "env_remove".to_string(),
                CtValue::List(spec.env_remove.iter().cloned().map(CtValue::Str).collect()),
            ),
            (
                "stdin".to_string(),
                optional(
                    spec.stdin.as_ref().map(process_stream_mode_value),
                    Type::Named("ProcessStreamMode".to_string()),
                ),
            ),
            ("stdout".to_string(), process_stream_mode_value(&spec.stdout)),
            ("stderr".to_string(), process_stream_mode_value(&spec.stderr)),
            (
                "timeout".to_string(),
                optional(
                    spec.timeout_ms.map(|ms| {
                        process_duration_value(process_prelude::Duration {
                            ns: ms.saturating_mul(1_000_000),
                        })
                    }),
                    Type::Named("Duration".to_string()),
                ),
            ),
            (
                "output_limit".to_string(),
                optional(spec.output_limit.map(CtValue::Int), Type::Int),
            ),
            ("detached".to_string(), CtValue::Bool(spec.detached)),
            (
                "terminal".to_string(),
                optional(
                    spec.terminal.as_ref().map(process_terminal_policy_value),
                    Type::Named("TerminalPolicy".to_string()),
                ),
            ),
        ],
    }
}

fn process_set_value(mut facts: Vec<String>) -> CtValue {
    facts.sort();
    CtValue::Struct {
        type_name: "Set".to_string(),
        fields: vec![(
            "items".to_string(),
            CtValue::List(facts.into_iter().map(CtValue::Str).collect()),
        )],
    }
}

fn process_result_value(result: process_prelude::ProcessResult) -> CtValue {
    CtValue::Struct {
        type_name: "ProcessResult".to_string(),
        fields: vec![
            ("code".to_string(), CtValue::Int(result.code)),
            ("output".to_string(), CtValue::Str(result.output)),
            ("errors".to_string(), CtValue::Str(result.errors)),
            ("success".to_string(), CtValue::Bool(result.success)),
            (
                "signal".to_string(),
                result
                    .signal
                    .map(|signal| CtValue::Present(Box::new(CtValue::Int(signal))))
                    .unwrap_or_else(|| CtValue::absent(Type::Int)),
            ),
            ("timed_out".to_string(), CtValue::Bool(result.timed_out)),
        ],
    }
}

fn process_io_operation(operation: process_prelude::IOOperation) -> CtValue {
    let variant = match operation {
        process_prelude::IOOperation::Read => "Read",
        process_prelude::IOOperation::Write => "Write",
        process_prelude::IOOperation::Flush => "Flush",
        process_prelude::IOOperation::Connect => "Connect",
        process_prelude::IOOperation::Accept => "Accept",
        process_prelude::IOOperation::Close => "Close",
        process_prelude::IOOperation::Resolve => "Resolve",
        process_prelude::IOOperation::Codec => "Codec",
    };
    CtValue::Enum {
        type_name: "IOOperation".to_string(),
        variant: variant.to_string(),
        args: vec![],
    }
}

fn process_io_context(context: process_prelude::IOContext) -> CtValue {
    let outcome_string = |value: Result<String, jet_foundation::Outcome::JetAbsent>| match value {
        Ok(value) => CtValue::Present(Box::new(CtValue::Str(value))),
        Err(_) => CtValue::absent(Type::String),
    };
    let outcome_int = |value: Result<i64, jet_foundation::Outcome::JetAbsent>| match value {
        Ok(value) => CtValue::Present(Box::new(CtValue::Int(value))),
        Err(_) => CtValue::absent(Type::Int),
    };
    CtValue::Struct {
        type_name: "IOContext".to_string(),
        fields: vec![
            ("operation".to_string(), process_io_operation(context.operation)),
            ("resource".to_string(), outcome_string(context.resource)),
            ("os_code".to_string(), outcome_int(context.os_code)),
            ("cause".to_string(), outcome_string(context.cause)),
        ],
    }
}

fn process_io_error(error: process_prelude::IOError) -> CtValue {
    let (variant, context) = match error {
        process_prelude::IOError::InvalidInput(context) => ("InvalidInput", context),
        process_prelude::IOError::NotFound(context) => ("NotFound", context),
        process_prelude::IOError::PermissionDenied(context) => ("PermissionDenied", context),
        process_prelude::IOError::TimedOut(context) => ("TimedOut", context),
        process_prelude::IOError::Cancelled(context) => ("Cancelled", context),
        process_prelude::IOError::Closed(context) => ("Closed", context),
        process_prelude::IOError::Protocol(context) => ("Protocol", context),
        process_prelude::IOError::Other(context) => ("Other", context),
    };
    CtValue::Enum {
        type_name: "IOError".to_string(),
        variant: variant.to_string(),
        args: vec![(None, process_io_context(context))],
    }
}

fn process_result_outcome(
    result: Result<process_prelude::ProcessResult, process_prelude::IOError>,
) -> CtValue {
    match result {
        Ok(result) => CtValue::Present(Box::new(process_result_value(result))),
        Err(error) => CtValue::failed(Box::new(process_io_error(error))),
    }
}

fn process_unit_outcome(result: Result<(), process_prelude::IOError>) -> CtValue {
    match result {
        Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
        Err(error) => CtValue::failed(Box::new(process_io_error(error))),
    }
}

thread_local! {
    static INTERP_PROCESS_CHILDREN: RefCell<Vec<process_prelude::ProcessChild>> = RefCell::new(Vec::new());
}

fn process_child_value(child: process_prelude::ProcessChild) -> CtValue {
    let handle = INTERP_PROCESS_CHILDREN.with(|children| {
        let mut children = children.borrow_mut();
        let handle = children.len() as i64;
        children.push(child);
        handle
    });
    CtValue::Struct {
        type_name: "ProcessChild".to_string(),
        fields: vec![("handle".to_string(), CtValue::Int(handle))],
    }
}

fn with_process_child<T>(value: &CtValue, f: impl FnOnce(&process_prelude::ProcessChild) -> T) -> Option<T> {
    let handle = match process_field(value, "handle") {
        Some(CtValue::Int(handle)) if *handle >= 0 => *handle as usize,
        _ => return None,
    };
    INTERP_PROCESS_CHILDREN.with(|children| children.borrow().get(handle).map(f))
}

fn ambient_process_handle(
    op: &str,
    recv: &mut CtValue,
    args: &mut [CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let method = op.strip_prefix("ProcessSpec:")?;
    if !matches!(
        method,
        "cwd"
            | "env"
            | "env_remove"
            | "env_clear"
            | "stdin"
            | "stdout"
            | "stderr"
            | "timeout"
            | "output_limit"
            | "detached"
            | "terminal"
            | "capabilities"
            | "run"
            | "run_checked"
            | "spawn"
    ) {
        return None;
    }
    Some((|| {
        let spec = process_spec_from_value(recv, span)?;
        match method {
            "cwd" => {
                let cwd = process_string(args.first().ok_or_else(|| unsupported("ProcessSpec.cwd argument", span))?, "ProcessSpec.cwd argument", span)?;
                Ok(process_spec_value(&process_prelude::spec_cwd(spec, &cwd)))
            }
            "env" => {
                let name = process_string(args.first().ok_or_else(|| unsupported("ProcessSpec.env name", span))?, "ProcessSpec.env name", span)?;
                let value = process_string(args.get(1).ok_or_else(|| unsupported("ProcessSpec.env value", span))?, "ProcessSpec.env value", span)?;
                Ok(process_spec_value(&process_prelude::spec_env(spec, &name, &value)))
            }
            "env_remove" => {
                let name = process_string(args.first().ok_or_else(|| unsupported("ProcessSpec.env_remove argument", span))?, "ProcessSpec.env_remove argument", span)?;
                Ok(process_spec_value(&process_prelude::spec_env_remove(spec, &name)))
            }
            "env_clear" => Ok(process_spec_value(&process_prelude::spec_env_clear(spec))),
            "stdin" => {
                let mode = process_stream_mode(args.first().ok_or_else(|| unsupported("ProcessSpec.stdin argument", span))?, "ProcessSpec.stdin argument", span)?;
                Ok(process_spec_value(&process_prelude::spec_stdin(spec, &mode)))
            }
            "stdout" => {
                let mode = process_stream_mode(args.first().ok_or_else(|| unsupported("ProcessSpec.stdout argument", span))?, "ProcessSpec.stdout argument", span)?;
                Ok(process_spec_value(&process_prelude::spec_stdout(spec, &mode)))
            }
            "stderr" => {
                let mode = process_stream_mode(args.first().ok_or_else(|| unsupported("ProcessSpec.stderr argument", span))?, "ProcessSpec.stderr argument", span)?;
                Ok(process_spec_value(&process_prelude::spec_stderr(spec, &mode)))
            }
            "timeout" => {
                let timeout = process_duration(args.first().ok_or_else(|| unsupported("ProcessSpec.timeout argument", span))?, "ProcessSpec.timeout argument", span)?;
                Ok(process_spec_value(&process_prelude::spec_timeout(spec, &timeout)))
            }
            "output_limit" => {
                let output_limit = process_int(args.first().ok_or_else(|| unsupported("ProcessSpec.output_limit argument", span))?, "ProcessSpec.output_limit argument", span)?;
                Ok(process_spec_value(&process_prelude::spec_output_limit(spec, output_limit)))
            }
            "detached" => Ok(process_spec_value(&process_prelude::spec_detached(spec))),
            "terminal" => match args {
                [] => Ok(process_spec_value(&process_prelude::spec_terminal(spec))),
                [policy] => {
                    let policy = process_terminal_policy(policy, "ProcessSpec.terminal policy", span)?;
                    Ok(process_spec_value(&process_prelude::spec_terminal_with_policy(spec, &policy)))
                }
                _ => Err(unsupported("ProcessSpec.terminal arguments", span)),
            },
            "capabilities" => Ok(process_set_value(
                process_prelude::spec_capabilities(&spec).into_iter().collect(),
            )),
            "run" => Ok(process_result_outcome(process_prelude::spec_run(&spec))),
            "run_checked" => Ok(process_result_outcome(process_prelude::spec_run_checked(&spec))),
            "spawn" => match process_prelude::spec_spawn(&spec) {
                Ok(child) => Ok(CtValue::Present(Box::new(process_child_value(child)))),
                Err(error) => Ok(CtValue::failed(Box::new(process_io_error(error)))),
            },
            _ => unreachable!(),
        }
    })())
}

fn ambient_process_child_handle(
    op: &str,
    recv: &mut CtValue,
    _args: &mut [CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let method = op.strip_prefix("ProcessChild:")?;
    if !matches!(method, "id" | "wait" | "exited" | "kill" | "terminate" | "interrupt") {
        return None;
    }
    let result = match method {
        "id" => with_process_child(recv, process_prelude::child_id)
            .map(CtValue::Int)
            .ok_or_else(|| unsupported("ProcessChild receiver", span)),
        "wait" => with_process_child(recv, |child| process_result_outcome(process_prelude::child_wait(child)))
            .ok_or_else(|| unsupported("ProcessChild receiver", span)),
        "exited" => with_process_child(recv, |child| match process_prelude::child_exited(child) {
            Ok(value) => CtValue::Present(Box::new(CtValue::Bool(value))),
            Err(error) => CtValue::failed(Box::new(process_io_error(error))),
        })
        .ok_or_else(|| unsupported("ProcessChild receiver", span)),
        "kill" => with_process_child(recv, |child| process_unit_outcome(process_prelude::child_kill(child)))
            .ok_or_else(|| unsupported("ProcessChild receiver", span)),
        "terminate" => with_process_child(recv, |child| process_unit_outcome(process_prelude::child_terminate(child)))
            .ok_or_else(|| unsupported("ProcessChild receiver", span)),
        "interrupt" => with_process_child(recv, |child| process_unit_outcome(process_prelude::child_interrupt(child)))
            .ok_or_else(|| unsupported("ProcessChild receiver", span)),
        _ => unreachable!(),
    };
    Some(result)
}

fn crypto_err(msg: impl Into<String>) -> CtValue {
    CtValue::Struct {
        type_name: "CryptoError".to_string(),
        fields: vec![("message".to_string(), CtValue::Str(msg.into()))],
    }
}

fn clock_now(value: &CtValue, span: Span) -> Result<i64, Diagnostic> {
    match value {
        CtValue::Struct {
            type_name,
            fields,
        } if type_name == "__JetTirClock" || type_name == "Clock" => fields
            .iter()
            .find_map(|(name, value)| match (name.as_str(), value) {
                ("now", CtValue::Int(now)) => Some(*now),
                _ => None,
            })
            .ok_or_else(|| unsupported("core.uuid.v7 clock state", span)),
        _ => Err(unsupported("core.uuid.v7 clock", span)),
    }
}

fn db_err(msg: impl Into<String>) -> CtValue {
    CtValue::Struct {
        type_name: "DBError".to_string(),
        fields: vec![("message".to_string(), CtValue::Str(msg.into()))],
    }
}

fn io_error(kind: &str, cause: impl Into<String>) -> CtValue {
    CtValue::Enum {
        type_name: "IOError".to_string(),
        variant: kind.to_string(),
        args: vec![(
            None,
            CtValue::Struct {
                type_name: "IOContext".to_string(),
                fields: vec![
                    (
                        "operation".to_string(),
                        CtValue::Enum {
                            type_name: "IOOperation".to_string(),
                            variant: "Read".to_string(),
                            args: vec![],
                        },
                    ),
                    (
                        "resource".to_string(),
                        CtValue::Present(Box::new(CtValue::Str("stdin".to_string()))),
                    ),
                    ("os_code".to_string(), CtValue::absent(Type::Int)),
                    (
                        "cause".to_string(),
                        CtValue::Present(Box::new(CtValue::Str(cause.into()))),
                    ),
                ],
            },
        )],
    }
}

fn secret_bytes(v: &CtValue, span: Span) -> Result<Vec<u8>, Diagnostic> {
    match v {
        CtValue::Struct { type_name, fields } if type_name == "Secret" => {
            let field = fields.iter().find_map(|(n, val)| match (n.as_str(), val) {
                ("bytes", val) => Some(val),
                _ => None,
            });
            match field {
                Some(val) => as_bytes(val, span),
                None => Err(unsupported("Secret.bytes", span)),
            }
        }
        _ => as_bytes(v, span),
    }
}

fn secret_value(bytes: Vec<u8>) -> CtValue {
    CtValue::Struct {
        type_name: "Secret".to_string(),
        fields: vec![("bytes".to_string(), CtValue::Bytes(bytes))],
    }
}

fn x25519_secret_value(bytes: Vec<u8>) -> CtValue {
    CtValue::Struct {
        type_name: "X25519SecretKey".to_string(),
        fields: vec![("bytes".to_string(), CtValue::Bytes(bytes))],
    }
}

fn x25519_public_value(bytes: Vec<u8>) -> CtValue {
    CtValue::Struct {
        type_name: "X25519PublicKey".to_string(),
        fields: vec![("bytes".to_string(), CtValue::Bytes(bytes))],
    }
}

fn password_hash_value(text: String) -> CtValue {
    CtValue::Struct {
        type_name: "PasswordHash".to_string(),
        fields: vec![("text".to_string(), CtValue::Str(text))],
    }
}

fn digest256_value(bytes: Vec<u8>) -> CtValue {
    CtValue::Struct {
        type_name: "Digest256".to_string(),
        fields: vec![("bytes".to_string(), CtValue::Bytes(bytes))],
    }
}

fn hasher_value(bytes: Vec<u8>) -> CtValue {
    CtValue::Struct {
        type_name: "__JetCryptoHasher".to_string(),
        fields: vec![("bytes".to_string(), CtValue::Bytes(bytes))],
    }
}

fn hasher_bytes(value: &CtValue, span: Span) -> Result<Vec<u8>, Diagnostic> {
    struct_bytes(value, "__JetCryptoHasher", span)
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn path_string(v: &CtValue) -> Option<String> {
    match v {
        CtValue::Str(s) => Some(s.clone()),
        CtValue::Struct { type_name, fields } if type_name == "Path" => fields
            .iter()
            .find_map(|(n, val)| match (n.as_str(), val) {
                ("inner", CtValue::Str(s)) => Some(s.clone()),
                _ => None,
            }),
        _ => None,
    }
}

fn db_conn_value(handle: u64) -> CtValue {
    CtValue::Struct {
        type_name: "DBConnection".to_string(),
        fields: vec![("handle".to_string(), CtValue::Int(handle as i64))],
    }
}

fn db_policy_value(table: String, expression: String) -> CtValue {
    CtValue::Struct {
        type_name: "RowPolicy".to_string(),
        fields: vec![
            ("table".to_string(), CtValue::Str(table)),
            ("expression".to_string(), CtValue::Str(expression)),
        ],
    }
}

fn db_scope_value(handle: u64, table: String, expression: String, user: String) -> CtValue {
    CtValue::Struct {
        type_name: "DBScope".to_string(),
        fields: vec![
            ("handle".to_string(), CtValue::Int(handle as i64)),
            ("policy".to_string(), db_policy_value(table, expression)),
            ("user".to_string(), CtValue::Str(user)),
        ],
    }
}

fn db_handle(recv: &CtValue) -> Option<u64> {
    match recv {
        CtValue::Struct { type_name, fields }
            if matches!(type_name.as_str(), "DBConnection" | "DBScope") => fields
            .iter()
            .find_map(|(n, v)| match (n.as_str(), v) {
                ("handle", CtValue::Int(h)) if *h > 0 => Some(*h as u64),
                _ => None,
            }),
        _ => None,
    }
}

fn mod_grant_roots(value: &CtValue) -> Option<Vec<String>> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != "ModGrant" {
        return None;
    }
    let CtValue::List(values) = fields
        .iter()
        .find_map(|(name, value)| (name == "read").then_some(value))?
    else {
        return None;
    };
    values
        .iter()
        .map(|value| match value {
            CtValue::Str(value) => Some(value.clone()),
            _ => None,
        })
        .collect()
}

fn mod_handle(value: &CtValue) -> Option<i64> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    (type_name == "Mod").then(|| {
        fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
            ("handle", CtValue::Int(value)) if *value > 0 => Some(*value),
            _ => None,
        })
    })?
}

fn mod_value(handle: i64) -> CtValue {
    CtValue::Struct {
        type_name: "Mod".to_string(),
        fields: vec![("handle".to_string(), CtValue::Int(handle))],
    }
}

fn db_scope_parts(recv: &CtValue) -> Option<(u64, String, String, String)> {
    let handle = db_handle(recv)?;
    let CtValue::Struct { fields, .. } = recv else {
        return None;
    };
    let policy = fields.iter().find_map(|(name, value)| {
        (name == "policy").then_some(value)
    })?;
    let CtValue::Struct { fields: policy_fields, .. } = policy else {
        return None;
    };
    let table = policy_fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("table", CtValue::Str(value)) => Some(value.clone()),
        _ => None,
    })?;
    let expression = policy_fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("expression", CtValue::Str(value)) => Some(value.clone()),
        _ => None,
    })?;
    let user = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("user", CtValue::Str(value)) => Some(value.clone()),
        _ => None,
    })?;
    Some((handle, table, expression, user))
}

fn service_runtime_value(store: String, retention_ms: i64) -> CtValue {
    CtValue::Struct {
        type_name: "ServiceRuntime".to_string(),
        fields: vec![
            ("store".to_string(), CtValue::Str(store)),
            ("retention_ms".to_string(), CtValue::Int(retention_ms)),
        ],
    }
}

fn service_runtime_parts(recv: &CtValue) -> Option<service_prelude::JetServiceRuntime> {
    let CtValue::Struct { type_name, fields } = recv else {
        return None;
    };
    if type_name != "ServiceRuntime" {
        return None;
    }
    let store = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("store", CtValue::Str(value)) => Some(value.clone()),
        _ => None,
    })?;
    let retention_ms = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("retention_ms", CtValue::Int(value)) => Some(*value),
        _ => None,
    })?;
    Some(service_prelude::JetServiceRuntime { store, retention_ms })
}

fn service_endpoint_value(value: &CtValue) -> Option<service_prelude::JetServiceEndpoint> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != "ServiceEndpoint" {
        return None;
    }
    let tree = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("tree", CtValue::Str(value)) => Some(value.clone()),
        _ => None,
    })?;
    let worker = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("worker", CtValue::Str(value)) => Some(value.clone()),
        _ => None,
    })?;
    let generation = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("generation", CtValue::Int(value)) => Some(*value),
        _ => None,
    })?;
    let authority = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("authority", CtValue::Str(value)) => Some(value.clone()),
        _ => None,
    })?;
    service_prelude::jet_services_authority_endpoint(tree, worker, generation, authority).ok()
}

fn service_receipt_value(receipt: service_prelude::JetServiceReceipt) -> CtValue {
    match receipt {
        service_prelude::JetServiceReceipt::Accepted(id) => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "Accepted".to_string(),
            args: vec![(None, CtValue::Str(id))],
        },
        service_prelude::JetServiceReceipt::Duplicate(id) => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "Duplicate".to_string(),
            args: vec![(None, CtValue::Str(id))],
        },
        service_prelude::JetServiceReceipt::Retained { id, until } => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "Retained".to_string(),
            args: vec![
                (Some("id".to_string()), CtValue::Str(id)),
                (Some("until".to_string()), CtValue::Int(until)),
            ],
        },
        service_prelude::JetServiceReceipt::DeadLettered(id) => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "DeadLettered".to_string(),
            args: vec![(None, CtValue::Str(id))],
        },
        service_prelude::JetServiceReceipt::Rejected(reason) => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "Rejected".to_string(),
            args: vec![(None, CtValue::Str(reason))],
        },
        service_prelude::JetServiceReceipt::Unavailable(reason) => CtValue::Enum {
            type_name: "ServiceReceipt".to_string(),
            variant: "Unavailable".to_string(),
            args: vec![(None, CtValue::Str(reason))],
        },
    }
}

fn service_error_value(error: service_prelude::JetServiceError) -> CtValue {
    let (variant, message) = match error {
        service_prelude::JetServiceError::Full(message) => ("Full", message),
        service_prelude::JetServiceError::Ambiguous(message) => ("Ambiguous", message),
        service_prelude::JetServiceError::Unknown(message) => ("Unknown", message),
        service_prelude::JetServiceError::NotStarted(message) => ("NotStarted", message),
        service_prelude::JetServiceError::Policy(message) => ("Policy", message),
        service_prelude::JetServiceError::Unavailable(message) => ("Unavailable", message),
        service_prelude::JetServiceError::Partitioned(message) => ("Partitioned", message),
        service_prelude::JetServiceError::Revoked(message) => ("Revoked", message),
        service_prelude::JetServiceError::Stale(message) => ("Stale", message),
        service_prelude::JetServiceError::Expired(message) => ("Expired", message),
    };
    CtValue::Enum {
        type_name: "ServiceError".to_string(),
        variant: variant.to_string(),
        args: vec![(None, CtValue::Str(message))],
    }
}

fn service_duration_ns(value: &CtValue) -> Option<i64> {
    match value {
        // The `Duration` carrier's one field is `ns` (see eval/handles.rs
        // `duration_new`); adapters pass the exact signed value onward.
        CtValue::Struct { type_name, fields } if type_name == "Duration" => fields
            .iter()
            .find_map(|(name, value)| (name == "ns").then_some(value))
            .and_then(|value| match value {
                CtValue::Int(ns) => Some(*ns),
                _ => None,
            }),
        _ => None,
    }
}

fn service_duration_ms(value: &CtValue) -> Option<i64> {
    service_duration_ns(value).map(|ns| ns / 1_000_000)
}

fn ct_db_value(v: &CtValue) -> Option<wire::DBValue> {
    match v {
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if type_name == "DBValue" => match (variant.as_str(), args.as_slice()) {
            ("Null", _) => Some(wire::DBValue::Null),
            ("Int", [(_, CtValue::Int(n))]) => Some(wire::DBValue::Int(*n)),
            ("Float", [(_, CtValue::Float(f))]) => Some(wire::DBValue::Float(f.as_f64())),
            ("Text", [(_, CtValue::Str(s))]) => Some(wire::DBValue::Text(s.clone())),
            ("Bool", [(_, CtValue::Bool(b))]) => Some(wire::DBValue::Bool(*b)),
            _ => None,
        },
        _ => None,
    }
}

fn wire_db_value(v: wire::DBValue) -> CtValue {
    match v {
        wire::DBValue::Null => CtValue::Enum {
            type_name: "DBValue".into(),
            variant: "Null".into(),
            args: vec![],
        },
        wire::DBValue::Int(n) => CtValue::Enum {
            type_name: "DBValue".into(),
            variant: "Int".into(),
            args: vec![(None, CtValue::Int(n))],
        },
        wire::DBValue::Float(f) => CtValue::Enum {
            type_name: "DBValue".into(),
            variant: "Float".into(),
            args: vec![(None, CtValue::Float(CtFloat::f64(f)))],
        },
        wire::DBValue::Text(s) => CtValue::Enum {
            type_name: "DBValue".into(),
            variant: "Text".into(),
            args: vec![(None, CtValue::Str(s))],
        },
        wire::DBValue::Bool(b) => CtValue::Enum {
            type_name: "DBValue".into(),
            variant: "Bool".into(),
            args: vec![(None, CtValue::Bool(b))],
        },
    }
}

fn row_map(row: wire::JetDBRow) -> CtValue {
    let mut m = BTreeMap::new();
    for (k, v) in row {
        m.insert(CtKey::Str(k), wire_db_value(v));
    }
    CtValue::Map(m)
}

fn db_params(list: &CtValue, span: Span) -> Result<Vec<wire::DBValue>, Diagnostic> {
    let CtValue::List(items) = list else {
        return Err(unsupported("db params list", span));
    };
    let mut vals = Vec::with_capacity(items.len());
    for item in items {
        vals.push(ct_db_value(item).ok_or_else(|| unsupported("DBValue param", span))?);
    }
    Ok(vals)
}

fn ambient_db_scope_execute(
    scope: &(u64, String, String, String),
    sql: &str,
    params: &Vec<wire::DBValue>,
    allow_schema: bool,
) -> Result<i64, wire::DBError> {
    let (handle, table, expression, user) = scope;
    let (sql, values) = if allow_schema {
        wire::jet_db_apply_migration_policy(sql, params, table, expression, user)?
    } else {
        wire::jet_db_apply_policy(sql, params, table, expression, user)?
    };
    let result = DB::runtime_execute(*handle, &sql, &wire::jet_db_encode_params(&values));
    wire::jet_db_decode_execute_result(&result)
}

fn ambient_db_scope_query(
    scope: &(u64, String, String, String),
    sql: &str,
    params: &Vec<wire::DBValue>,
    allow_schema: bool,
) -> Result<Vec<wire::JetDBRow>, wire::DBError> {
    let (handle, table, expression, user) = scope;
    let (sql, values) = if allow_schema {
        wire::jet_db_apply_migration_policy(sql, params, table, expression, user)?
    } else {
        wire::jet_db_apply_policy(sql, params, table, expression, user)?
    };
    let result = DB::runtime_query(*handle, &sql, &wire::jet_db_encode_params(&values));
    wire::jet_db_decode_query_result(&result)
}

struct AmbientDbBackend {
    scope: (u64, String, String, String),
}

impl wire::JetDBBackend for AmbientDbBackend {
    fn begin(&mut self) -> bool {
        DB::runtime_begin(self.scope.0)
    }

    fn commit(&mut self) -> bool {
        DB::runtime_commit(self.scope.0)
    }

    fn rollback(&mut self) {
        let _ = DB::runtime_rollback(self.scope.0);
    }

    fn execute(
        &mut self,
        sql: &String,
        params: &Vec<wire::DBValue>,
        allow_schema: bool,
    ) -> Result<i64, wire::DBError> {
        ambient_db_scope_execute(&self.scope, sql, params, allow_schema)
    }

    fn query(
        &mut self,
        sql: &String,
        params: &Vec<wire::DBValue>,
        allow_schema: bool,
    ) -> Result<Vec<wire::JetDBRow>, wire::DBError> {
        ambient_db_scope_query(&self.scope, sql, params, allow_schema)
    }
}

fn ambient_db_steps(value: &CtValue, span: Span) -> Result<Vec<String>, Diagnostic> {
    ct_string_list(value).ok_or_else(|| unsupported("database steps list", span))
}

fn to_secret(v: &CtValue, span: Span) -> Result<Crypto::runtime::Secret, Diagnostic> {
    let bytes = secret_bytes(v, span)?;
    Ok(Crypto::runtime::jet_crypto_secret_from_bytes_impl(bytes))
}

fn struct_bytes(v: &CtValue, type_name: &str, span: Span) -> Result<Vec<u8>, Diagnostic> {
    match v {
        CtValue::Struct {
            type_name: tn,
            fields,
        } if tn == type_name => {
            let field = fields.iter().find_map(|(n, val)| match (n.as_str(), val) {
                ("bytes", val) => Some(val),
                _ => None,
            });
            match field {
                Some(val) => as_bytes(val, span),
                None => Err(unsupported(&format!("{type_name}.bytes"), span)),
            }
        }
        _ => as_bytes(v, span),
    }
}

fn ambient_int_arg(
    args: &[CtValue],
    index: usize,
    name: &str,
    span: Span,
) -> Result<i64, Diagnostic> {
    match args.get(index) {
        Some(CtValue::Int(value)) => Ok(*value),
        _ => Err(unsupported(&format!("{name} expects an Int argument"), span)),
    }
}

fn ambient_float_arg(
    args: &[CtValue],
    index: usize,
    name: &str,
    span: Span,
) -> Result<f64, Diagnostic> {
    match args.get(index) {
        Some(CtValue::Float(value)) => Ok(value.as_f64()),
        Some(CtValue::Int(value)) => Ok(*value as f64),
        _ => Err(unsupported(&format!("{name} expects a numeric argument"), span)),
    }
}

fn ambient_random_call(
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    resolved_ret: Option<&Type>,
) -> Option<Result<CtValue, Diagnostic>> {
    if method == "rng"
        || !matches!(
            method,
            "seed"
                | "int"
                | "float"
                | "float_range"
                | "bool"
                | "normal"
                | "exponential"
                | "bytes"
                | "pick"
                | "weighted_pick"
                | "sample"
                | "shuffle"
                | "split"
        )
    {
        return None;
    }
    let result = (|| match method {
        "seed" => {
            crate::Random::ambient_seed(ambient_int_arg(&args, 0, "random.seed", span)?);
            Ok(CtValue::Unit)
        }
        "int" => Ok(CtValue::Int(crate::Random::ambient_int(
            ambient_int_arg(&args, 0, "random.int", span)?,
            ambient_int_arg(&args, 1, "random.int", span)?,
        ))),
        "float" => Ok(CtValue::Float(CtFloat::f64(crate::Random::ambient_float()))),
        "float_range" => Ok(CtValue::Float(CtFloat::f64(
            crate::Random::ambient_float_range(
                ambient_float_arg(&args, 0, "random.float_range", span)?,
                ambient_float_arg(&args, 1, "random.float_range", span)?,
            ),
        ))),
        "bool" => Ok(CtValue::Bool(crate::Random::ambient_bool(
            ambient_float_arg(&args, 0, "random.bool", span)?,
        ))),
        "normal" => Ok(CtValue::Float(CtFloat::f64(crate::Random::ambient_normal(
            ambient_float_arg(&args, 0, "random.normal", span)?,
            ambient_float_arg(&args, 1, "random.normal", span)?,
        )))),
        "exponential" => Ok(CtValue::Float(CtFloat::f64(
            crate::Random::ambient_exponential(ambient_float_arg(
                &args,
                0,
                "random.exponential",
                span,
            )?),
        ))),
        "bytes" => Ok(CtValue::Bytes(crate::Random::ambient_bytes(
            ambient_int_arg(&args, 0, "random.bytes", span)?,
        ))),
        "pick" => {
            let CtValue::List(items) = args.first().ok_or_else(|| {
                unsupported("random.pick needs a list", span)
            })? else {
                return Err(unsupported("random.pick needs a list", span));
            };
            match crate::Random::ambient_pick(items) {
                Some(value) => Ok(CtValue::Present(Box::new(value))),
                None => Ok(CtValue::absent(
                    CtValue::resolved_option_element_type(resolved_ret).ok_or_else(|| {
                        unsupported("random.pick needs a resolved element type", span)
                    })?,
                )),
            }
        }
        "weighted_pick" => {
            let CtValue::List(items) = args.first().ok_or_else(|| {
                unsupported("random.weighted_pick needs a list", span)
            })? else {
                return Err(unsupported(
                    "random.weighted_pick needs a list",
                    span,
                ));
            };
            let CtValue::List(weights) = args.get(1).ok_or_else(|| {
                unsupported("random.weighted_pick needs weights", span)
            })? else {
                return Err(unsupported(
                    "random.weighted_pick needs weights",
                    span,
                ));
            };
            let weights = weights
                .iter()
                .map(|value| match value {
                    CtValue::Float(value) => Ok(value.as_f64()),
                    CtValue::Int(value) => Ok(*value as f64),
                    _ => Err(unsupported(
                        "random.weighted_pick weights must be numeric",
                        span,
                    )),
                })
                .collect::<Result<Vec<_>, _>>()?;
            match crate::Random::ambient_weighted_pick(items, &weights) {
                Some(value) => Ok(CtValue::Present(Box::new(value))),
                None => Ok(CtValue::absent(
                    CtValue::resolved_option_element_type(resolved_ret).ok_or_else(|| {
                        unsupported("random.weighted_pick needs a resolved element type", span)
                    })?,
                )),
            }
        }
        "sample" => {
            let CtValue::List(items) = args.first().ok_or_else(|| {
                unsupported("random.sample needs a list", span)
            })? else {
                return Err(unsupported("random.sample needs a list", span));
            };
            Ok(CtValue::List(crate::Random::ambient_sample(
                items,
                ambient_int_arg(&args, 1, "random.sample", span)?,
            )))
        }
        "shuffle" => {
            let CtValue::List(mut items) = args.first().cloned().ok_or_else(|| {
                unsupported("random.shuffle needs a list", span)
            })? else {
                return Err(unsupported("random.shuffle needs a list", span));
            };
            crate::Random::ambient_shuffle(&mut items);
            Ok(CtValue::List(items))
        }
        "split" => Ok(CtValue::Struct {
            type_name: jet_foundation::Syntax::RNG_TYPE.to_string(),
            fields: vec![(
                "state".to_string(),
                CtValue::Int(crate::Random::ambient_split(ambient_int_arg(
                    &args,
                    0,
                    "random.split",
                    span,
                )?)),
            )],
        }),
        _ => unreachable!("ambient random method was filtered above"),
    })();
    Some(result)
}

fn ambient_time_call(
    module: &str,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if !matches!(
        (module, method),
        ("core.time", "now" | "now_utc" | "today" | "instant" | "sleep" | "start")
            | ("core.time.date", "today")
            | ("core.time.datetime", "now")
    ) {
        return None;
    }
    let result = (|| match (module, method) {
        ("core.time", "now") => Ok(CtValue::Int(jet_codegen::scheduler::jet_std_time_now())),
        ("core.time", "now_utc") => Ok(crate::Time::ambient_datetime_now_value()),
        ("core.time", "today") => Ok(crate::Time::ambient_date_today_value()),
        ("core.time", "instant") => Ok(crate::Time::ambient_instant_value()),
        ("core.time", "sleep") => {
            let millis = ambient_int_arg(args, 0, "time.sleep", span)?;
            jet_codegen::scheduler::jet_std_time_sleep(millis);
            Ok(CtValue::Unit)
        }
        ("core.time", "start") => Ok(CtValue::Struct {
            type_name: "Stopwatch".to_string(),
            fields: vec![(
                "start_ms".to_string(),
                CtValue::Int(crate::Time::ambient_monotonic_now_ms()),
            )],
        }),
        ("core.time.date", "today") => Ok(crate::Time::ambient_date_today_value()),
        ("core.time.datetime", "now") => Ok(crate::Time::ambient_datetime_now_value()),
        _ => unreachable!("ambient time method was filtered above"),
    })();
    Some(result)
}

fn auth_claims_value(claims: Crypto::runtime::JetAuthClaims) -> CtValue {
    let Crypto::runtime::JetAuthClaims {
        subject,
        audience,
        issuer,
        expires_at,
        not_before,
        issued_at,
    } = claims;
    CtValue::Struct {
        type_name: "Claims".to_string(),
        fields: vec![
            (
                "subject".to_string(),
                subject
                    .map(|value| CtValue::Present(Box::new(CtValue::Str(value))))
                    .unwrap_or_else(|| CtValue::absent(Type::String)),
            ),
            ("audience".to_string(), CtValue::Str(audience)),
            (
                "issuer".to_string(),
                issuer
                    .map(|value| CtValue::Present(Box::new(CtValue::Str(value))))
                    .unwrap_or_else(|| CtValue::absent(Type::String)),
            ),
            ("expires_at".to_string(), CtValue::Int(expires_at)),
            (
                "not_before".to_string(),
                not_before
                    .map(CtValue::Int)
                    .map(|value| CtValue::Present(Box::new(value)))
                    .unwrap_or_else(|| CtValue::absent(Type::Int)),
            ),
            (
                "issued_at".to_string(),
                issued_at
                    .map(CtValue::Int)
                    .map(|value| CtValue::Present(Box::new(value)))
                    .unwrap_or_else(|| CtValue::absent(Type::Int)),
            ),
        ],
    }
}

fn auth_error_value(error: Crypto::runtime::JetAuthError) -> CtValue {
    let variant = |name: &str, args: Vec<(Option<String>, CtValue)>| CtValue::Enum {
        type_name: "AuthError".to_string(),
        variant: name.to_string(),
        args,
    };
    let text = |value: String| vec![(None, CtValue::Str(value))];
    match error {
        Crypto::runtime::JetAuthError::MalformedToken(value) => variant("MalformedToken", text(value)),
        Crypto::runtime::JetAuthError::UnsupportedToken(value) => variant("UnsupportedToken", text(value)),
        Crypto::runtime::JetAuthError::InvalidSignature => variant("InvalidSignature", Vec::new()),
        Crypto::runtime::JetAuthError::WeakKey => variant("WeakKey", Vec::new()),
        Crypto::runtime::JetAuthError::MissingClaim(value) => variant("MissingClaim", text(value)),
        Crypto::runtime::JetAuthError::WrongAudience { expected, actual } => variant(
            "WrongAudience",
            vec![
                (Some("expected".to_string()), CtValue::Str(expected)),
                (Some("actual".to_string()), CtValue::Str(actual)),
            ],
        ),
        Crypto::runtime::JetAuthError::WrongIssuer { expected, actual } => variant(
            "WrongIssuer",
            vec![
                (Some("expected".to_string()), CtValue::Str(expected)),
                (
                    Some("actual".to_string()),
                    actual
                        .map(|value| CtValue::Present(Box::new(CtValue::Str(value))))
                        .unwrap_or_else(|| CtValue::absent(Type::String)),
                ),
            ],
        ),
        Crypto::runtime::JetAuthError::TokenExpired => variant("TokenExpired", Vec::new()),
        Crypto::runtime::JetAuthError::DecodeError(value) => variant("DecodeError", text(value)),
        Crypto::runtime::JetAuthError::TokenNotYetValid => {
            variant("TokenNotYetValid", Vec::new())
        }
    }
}

fn auth_struct_field<'a>(value: &'a CtValue, wanted: &str) -> Option<&'a CtValue> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    (type_name == "Session" || type_name == "Auth")
        .then(|| fields.iter().find_map(|(name, value)| (name == wanted).then_some(value)))
        .flatten()
}

fn auth_text_arg(args: &[CtValue], index: usize, what: &str, span: Span) -> Result<String, Diagnostic> {
    match args.get(index) {
        Some(CtValue::Str(value)) => Ok(value.clone()),
        _ => Err(unsupported(what, span)),
    }
}

fn auth_int_arg(args: &[CtValue], index: usize, what: &str, span: Span) -> Result<i64, Diagnostic> {
    match args.get(index) {
        Some(CtValue::Int(value)) => Ok(*value),
        _ => Err(unsupported(what, span)),
    }
}

fn auth_session_value(session: Crypto::runtime::JetAuthSession) -> CtValue {
    CtValue::Struct {
        type_name: "Session".to_string(),
        fields: vec![
            ("id".to_string(), CtValue::Str(session.id)),
            ("user_id".to_string(), CtValue::Str(session.user_id)),
            ("expires_at".to_string(), CtValue::Int(session.expires_at)),
            ("cookie".to_string(), CtValue::Str(session.cookie)),
        ],
    }
}

fn auth_session_arg(
    args: &[CtValue],
    index: usize,
    span: Span,
) -> Result<Crypto::runtime::JetAuthSession, Diagnostic> {
    let value = args
        .get(index)
        .ok_or_else(|| unsupported("core.auth Session argument", span))?;
    let id = match auth_struct_field(value, "id") {
        Some(CtValue::Str(value)) => value.clone(),
        _ => return Err(unsupported("core.auth Session.id", span)),
    };
    let user_id = match auth_struct_field(value, "user_id") {
        Some(CtValue::Str(value)) => value.clone(),
        _ => return Err(unsupported("core.auth Session.user_id", span)),
    };
    let expires_at = match auth_struct_field(value, "expires_at") {
        Some(CtValue::Int(value)) => *value,
        _ => return Err(unsupported("core.auth Session.expires_at", span)),
    };
    let cookie = match auth_struct_field(value, "cookie") {
        Some(CtValue::Str(value)) => value.clone(),
        _ => return Err(unsupported("core.auth Session.cookie", span)),
    };
    Ok(Crypto::runtime::JetAuthSession {
        id,
        user_id,
        expires_at,
        cookie,
    })
}

fn auth_app_value(app: Crypto::runtime::JetAuthApp) -> CtValue {
    CtValue::Struct {
        type_name: "Auth".to_string(),
        fields: vec![
            ("users_table".to_string(), CtValue::Str(app.users_table)),
            (
                // Internal carrier field; the public Auth surface exposes only
                // users_table, but providers must survive app.auth_oauth.
                "providers".to_string(),
                CtValue::List(app.providers.into_iter().map(CtValue::Str).collect()),
            ),
        ],
    }
}

fn auth_app_arg(
    args: &[CtValue],
    index: usize,
    span: Span,
) -> Result<Crypto::runtime::JetAuthApp, Diagnostic> {
    let value = args
        .get(index)
        .ok_or_else(|| unsupported("app Auth argument", span))?;
    let users_table = match auth_struct_field(value, "users_table") {
        Some(CtValue::Str(value)) => value.clone(),
        _ => return Err(unsupported("app Auth.users_table", span)),
    };
    let providers = match auth_struct_field(value, "providers") {
        Some(CtValue::List(values)) => values
            .iter()
            .map(|value| match value {
                CtValue::Str(value) => Ok(value.clone()),
                _ => Err(unsupported("app Auth.providers", span)),
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(unsupported("app Auth.providers", span)),
    };
    Ok(Crypto::runtime::JetAuthApp {
        users_table,
        providers,
    })
}

fn ambient_auth_session_call(
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if !matches!(
        method,
        "register_user"
            | "password_login"
            | "session_validate"
            | "magic_link_issue"
            | "magic_link_consume"
            | "oauth_begin"
            | "oauth_finish"
            | "session_show"
            | "session_user"
            | "session_cookie"
            | "session_id"
    ) {
        return None;
    }
    let result = (|| {
        let value = match method {
            "register_user" => match Crypto::runtime::auth_register_user(
                auth_text_arg(args, 0, "core.auth user id", span)?,
                auth_text_arg(args, 1, "core.auth password hash", span)?,
            ) {
                Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            },
            "password_login" => match Crypto::runtime::auth_password_login(
                auth_text_arg(args, 0, "core.auth user id", span)?,
                auth_text_arg(args, 1, "core.auth password hash", span)?,
                auth_int_arg(args, 2, "core.auth now_ms", span)?,
                auth_int_arg(args, 3, "core.auth ttl_ms", span)?,
            ) {
                Ok(session) => CtValue::Present(Box::new(auth_session_value(session))),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            },
            "session_validate" => match Crypto::runtime::auth_session_validate(
                &auth_text_arg(args, 0, "core.auth session id", span)?,
                auth_int_arg(args, 1, "core.auth now_ms", span)?,
            ) {
                Ok(session) => CtValue::Present(Box::new(auth_session_value(session))),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            },
            "magic_link_issue" => match Crypto::runtime::auth_magic_link_issue(
                auth_text_arg(args, 0, "core.auth user id", span)?,
                auth_int_arg(args, 1, "core.auth now_ms", span)?,
                auth_int_arg(args, 2, "core.auth ttl_ms", span)?,
            ) {
                Ok(token) => CtValue::Present(Box::new(CtValue::Str(token))),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            },
            "magic_link_consume" => match Crypto::runtime::auth_magic_link_consume(
                auth_text_arg(args, 0, "core.auth magic token", span)?,
                auth_int_arg(args, 1, "core.auth now_ms", span)?,
                auth_int_arg(args, 2, "core.auth ttl_ms", span)?,
            ) {
                Ok(session) => CtValue::Present(Box::new(auth_session_value(session))),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            },
            "oauth_begin" => match Crypto::runtime::auth_oauth_begin(auth_text_arg(
                args,
                0,
                "core.auth provider",
                span,
            )?) {
                Ok(state) => CtValue::Present(Box::new(CtValue::Str(state))),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            },
            "oauth_finish" => match Crypto::runtime::auth_oauth_finish(
                auth_text_arg(args, 0, "core.auth OAuth state", span)?,
                auth_text_arg(args, 1, "core.auth OAuth subject", span)?,
                auth_int_arg(args, 2, "core.auth now_ms", span)?,
                auth_int_arg(args, 3, "core.auth ttl_ms", span)?,
            ) {
                Ok(session) => CtValue::Present(Box::new(auth_session_value(session))),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            },
            "session_show" => CtValue::Str(Crypto::runtime::auth_session_show(&auth_session_arg(
                args, 0, span,
            )?)),
            "session_user" => CtValue::Str(Crypto::runtime::auth_session_user(&auth_session_arg(
                args, 0, span,
            )?)),
            "session_cookie" => CtValue::Str(Crypto::runtime::auth_session_cookie(&auth_session_arg(
                args, 0, span,
            )?)),
            "session_id" => CtValue::Str(Crypto::runtime::auth_session_id(&auth_session_arg(
                args, 0, span,
            )?)),
            _ => unreachable!("auth session method was checked above"),
        };
        Ok(value)
    })();
    Some(result)
}

fn ambient_app_auth_call(
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if !matches!(method, "auth" | "auth_oauth" | "auth_routes" | "auth_show") {
        return None;
    }
    let result = (|| {
        let value = match method {
            "auth" => auth_app_value(Crypto::runtime::app_auth(auth_text_arg(
                args,
                0,
                "app auth users table",
                span,
            )?)),
            "auth_oauth" => {
                let auth = auth_app_arg(args, 0, span)?;
                auth_app_value(Crypto::runtime::app_auth_oauth(
                    auth,
                    auth_text_arg(args, 1, "app auth providers", span)?,
                ))
            }
            "auth_routes" => {
                let auth = auth_app_arg(args, 0, span)?;
                CtValue::Str(Crypto::runtime::app_auth_routes(&auth))
            }
            "auth_show" => {
                let auth = auth_app_arg(args, 0, span)?;
                CtValue::Str(Crypto::runtime::app_auth_show(&auth))
            }
            _ => unreachable!("app auth method was checked above"),
        };
        Ok(value)
    })();
    Some(result)
}

fn ambient_auth_call(
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if !matches!(method, "verify_jwt" | "verify_paseto") {
        return ambient_auth_session_call(method, args, span);
    }
    Some((|| {
        let token = match args.first() {
            Some(CtValue::Str(value)) => value.clone(),
            _ => return Err(unsupported("core.auth token", span)),
        };
        let key = as_bytes(
            args.get(1)
                .ok_or_else(|| unsupported("core.auth key", span))?,
            span,
        )?;
        let audience = match args.get(2) {
            Some(CtValue::Str(value)) => value.clone(),
            _ => return Err(unsupported("core.auth audience", span)),
        };
        let issuer = match args.get(3) {
            None => None,
            Some(CtValue::Str(value)) => Some(value.clone()),
            _ => return Err(unsupported("core.auth issuer", span)),
        };
        let clock_skew_ns = args
            .get(4)
            .map(|value| {
                service_duration_ns(value)
                    .ok_or_else(|| unsupported("core.auth clock_skew", span))
            })
            .transpose()?;
        let result = if method == "verify_jwt" {
            Crypto::runtime::auth_verify_jwt_defaulted(
                &token,
                &key,
                &audience,
                issuer.as_ref(),
                clock_skew_ns,
            )
        } else {
            let footer = args
                .get(5)
                .map(|value| as_bytes(value, span))
                .transpose()?;
            let implicit = args
                .get(6)
                .map(|value| as_bytes(value, span))
                .transpose()?;
            Crypto::runtime::auth_verify_paseto_defaulted(
                &token,
                &key,
                &audience,
                issuer.as_ref(),
                clock_skew_ns,
                footer.as_ref(),
                implicit.as_ref(),
            )
        };
        Ok(match result {
            Ok(claims) => CtValue::Present(Box::new(auth_claims_value(claims))),
            Err(error) => CtValue::failed(Box::new(auth_error_value(error))),
        })
    })())
}

pub fn ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    resolved_ret: Option<Type>,
) -> Option<Result<CtValue, Diagnostic>> {
    if let Some(row) = jet_foundation::Syntax::core_call(module, method) {
        if !row.accepts_arity(args.len()) {
            return Some(Err(unsupported(
                &format!(
                    "{}.{}(): expected {}..{} argument(s), got {}",
                    module,
                    method,
                    row.arity(),
                    row.signature.max_arity,
                    args.len()
                ),
                span,
            )));
        }
    }
    if module == "core.email" {
        return jet_codegen::Comptime::EmailAdapter::ambient_core_call(
            method,
            &args,
            span,
            crate::Net::email_runtime_fns(),
        );
    }
    if let Some(result) = crate::enc_stream::ambient_core_call(module, method, args.clone(), span) {
        return Some(result);
    }
    if module == "core.random" {
        if let Some(result) = ambient_random_call(method, args.clone(), span, resolved_ret.as_ref()) {
            return Some(result);
        }
    }
    if module == "core.crypto.random" && method == "bytes" {
        let count = match args.first() {
            Some(CtValue::Int(count)) => *count,
            _ => return Some(Err(unsupported("crypto.random.bytes expects an Int", span))),
        };
        return Some(
            crate::Crypto::runtime::jet_crypto_entropy_bytes(count)
                .map(CtValue::Bytes)
                .map_err(|error| unsupported(&error.to_string(), span)),
        );
    }
    if let Some(result) = ambient_time_call(module, method, &args, span) {
        return Some(result);
    }
    // I9: core.http.server adapters call the same Prelude helpers as AOT/JIT.
    if module == "core.http.server" {
        return Some(ambient_http_server_call(method, &args, span));
    }
    if module == "core.http.client" && method == "request" {
        let result = match args.as_slice() {
            [CtValue::Str(method), CtValue::Str(url)] => Ok(http_handle_value(
                "HTTPRequest",
                crate::net_http_rt::runtime_http_request_new(method.clone(), url.clone()),
            )),
            _ => Err(unsupported("core.http.client.request arguments", span)),
        };
        return Some(result);
    }
    // I9: token verification uses the same Auth Prelude adapters as AOT/JIT;
    // this branch only marshals their typed result into CtValue.
    if module == "core.auth" {
        if let Some(result) = ambient_auth_call(method, &args, span) {
            return Some(result);
        }
    }
    if module == "app" || module == "core.web" {
        if let Some(result) = ambient_app_auth_call(method, &args, span) {
            return Some(result);
        }
    }
    match (module, method) {
        ("core.net", "socket_addr") => {
            let (Some(CtValue::Str(host)), Some(CtValue::Int(port))) =
                (args.first(), args.get(1))
            else {
                return Some(Err(unsupported("core.net.socket_addr arguments", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_net_socket_addr(
                host.clone(),
                *port,
            )))
        }
        ("core.net", "socket_to_string" | "socket_host" | "socket_port") => {
            let Some(address) = args.first().and_then(|value| http_handle_id(value, "SocketAddr"))
            else {
                return Some(Err(unsupported("core.net socket address", span)));
            };
            Some(Ok(match method {
                "socket_to_string" => crate::net_http_rt::runtime_net_socket_to_string(address),
                "socket_host" => crate::net_http_rt::runtime_net_socket_host(address),
                "socket_port" => crate::net_http_rt::runtime_net_socket_port(address),
                _ => unreachable!(),
            }))
        }
        ("core.net", "tcp_listen") => {
            let Some(CtValue::Str(address)) = args.first() else {
                return Some(Err(unsupported("core.net.tcp_listen address", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_tcp_listen(address.clone())))
        }
        ("core.net", "tcp_listen_addr") => {
            let Some(address) = args.first().and_then(|value| http_handle_id(value, "SocketAddr"))
            else {
                return Some(Err(unsupported("core.net.tcp_listen_addr address", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_tcp_listen_addr(address)))
        }
        ("core.net", "tcp_accept") => {
            let Some(listener) = args.first().and_then(|value| http_handle_id(value, "TcpListener"))
            else {
                return Some(Err(unsupported("core.net.tcp_accept listener", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_tcp_listener_accept(
                listener, None,
            )))
        }
        ("core.net", "tcp_connect") => {
            let Some(CtValue::Str(address)) = args.first() else {
                return Some(Err(unsupported("core.net.tcp_connect address", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_tcp_connect(address.clone())))
        }
        ("core.net", "tcp_connect_addr") => {
            let Some(address) = args.first().and_then(|value| http_handle_id(value, "SocketAddr"))
            else {
                return Some(Err(unsupported("core.net.tcp_connect_addr address", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_tcp_connect_addr(address)))
        }
        ("core.net", "tcp_connect_timeout") => {
            let Some(address) = args.first().and_then(|value| http_handle_id(value, "SocketAddr"))
            else {
                return Some(Err(unsupported("core.net.tcp_connect_timeout address", span)));
            };
            let Some(CtValue::Int(timeout_ms)) = args.get(1) else {
                return Some(Err(unsupported("core.net.tcp_connect_timeout timeout", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_tcp_connect_timeout(
                address,
                *timeout_ms,
            )))
        }
        ("core.net", "tcp_connect_happy") => {
            let (Some(CtValue::Str(host)), Some(CtValue::Int(port)), Some(CtValue::Int(timeout_ms))) =
                (args.first(), args.get(1), args.get(2))
            else {
                return Some(Err(unsupported("core.net.tcp_connect_happy arguments", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_tcp_connect_happy(
                host.clone(),
                *port,
                *timeout_ms,
            )))
        }
        ("core.net", "listener_local_socket_addr") => {
            let Some(listener) = args.first().and_then(|value| http_handle_id(value, "TcpListener"))
            else {
                return Some(Err(unsupported(
                    "core.net.listener_local_socket_addr listener",
                    span,
                )));
            };
            Some(Ok(
                crate::net_http_rt::runtime_tcp_listener_local_socket_addr(listener),
            ))
        }
        ("core.net", "udp_bind") => {
            let Some(CtValue::Str(address)) = args.first() else {
                return Some(Err(unsupported("core.net.udp_bind address", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_udp_bind(address.clone())))
        }
        ("core.net", "udp_bind_addr") => {
            let Some(address) = args.first().and_then(|value| http_handle_id(value, "SocketAddr"))
            else {
                return Some(Err(unsupported("core.net.udp_bind_addr address", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_udp_bind_addr(address)))
        }
        ("core.net", "udp_local_addr") => {
            let Some(socket) = args.first().and_then(|value| http_handle_id(value, "UdpSocket"))
            else {
                return Some(Err(unsupported("core.net.udp_local_addr receiver", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_udp_local_addr(socket)))
        }
        ("core.net", "udp_set_timeout") => {
            let Some(socket) = args.first().and_then(|value| http_handle_id(value, "UdpSocket"))
            else {
                return Some(Err(unsupported("core.net.udp_set_timeout receiver", span)));
            };
            let Some(CtValue::Int(timeout_ms)) = args.get(1) else {
                return Some(Err(unsupported("core.net.udp_set_timeout timeout", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_udp_set_timeout(
                socket,
                *timeout_ms,
            )))
        }
        ("core.net", "udp_send_to") => {
            let Some(socket) = args.first().and_then(|value| http_handle_id(value, "UdpSocket"))
            else {
                return Some(Err(unsupported("core.net.udp_send_to receiver", span)));
            };
            let Some(CtValue::Str(data)) = args.get(1) else {
                return Some(Err(unsupported("core.net.udp_send_to data", span)));
            };
            let Some(address) = args.get(2).and_then(|value| http_handle_id(value, "SocketAddr"))
            else {
                return Some(Err(unsupported("core.net.udp_send_to address", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_udp_send_to(
                socket,
                data.clone(),
                address,
            )))
        }
        ("core.net", "udp_recv_from") => {
            let Some(socket) = args.first().and_then(|value| http_handle_id(value, "UdpSocket"))
            else {
                return Some(Err(unsupported("core.net.udp_recv_from receiver", span)));
            };
            let Some(CtValue::Int(limit)) = args.get(1) else {
                return Some(Err(unsupported("core.net.udp_recv_from limit", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_udp_recv_from(socket, *limit)))
        }
        ("core.net", "udp_send_bytes_to") => {
            let Some(socket) = args.first().and_then(|value| http_handle_id(value, "UdpSocket"))
            else {
                return Some(Err(unsupported("core.net.udp_send_bytes_to receiver", span)));
            };
            let Some(data) = args.get(1).and_then(net_bytes_value) else {
                return Some(Err(unsupported("core.net.udp_send_bytes_to data", span)));
            };
            let Some(address) = args.get(2).and_then(|value| http_handle_id(value, "SocketAddr"))
            else {
                return Some(Err(unsupported("core.net.udp_send_bytes_to address", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_udp_send_bytes_to(
                socket,
                data,
                address,
            )))
        }
        ("core.net", "udp_receive") => {
            let Some(socket) = args.first().and_then(|value| http_handle_id(value, "UdpSocket"))
            else {
                return Some(Err(unsupported("core.net.udp_receive receiver", span)));
            };
            let Some(CtValue::Int(limit)) = args.get(1) else {
                return Some(Err(unsupported("core.net.udp_receive limit", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_udp_receive(socket, *limit)))
        }
        ("core.net", "udp_packet_data") => {
            let Some(packet) = args.first().and_then(|value| http_handle_id(value, "UDPPacket"))
            else {
                return Some(Err(unsupported("core.net.udp_packet_data packet", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_udp_packet_data(packet)))
        }
        ("core.net", "udp_packet_addr") => {
            let Some(packet) = args.first().and_then(|value| http_handle_id(value, "UDPPacket"))
            else {
                return Some(Err(unsupported("core.net.udp_packet_addr packet", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_udp_packet_addr(packet)))
        }
        ("core.net", "udp_packet_bytes") => {
            let Some(packet) = args.first().and_then(|value| http_handle_id(value, "UDPPacket"))
            else {
                return Some(Err(unsupported("core.net.udp_packet_bytes packet", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_udp_packet_bytes(packet)))
        }
        ("core.net", "udp_packet_original_len") => {
            let Some(packet) = args.first().and_then(|value| http_handle_id(value, "UDPPacket"))
            else {
                return Some(Err(unsupported(
                    "core.net.udp_packet_original_len packet",
                    span,
                )));
            };
            Some(Ok(crate::net_http_rt::runtime_udp_packet_original_len(
                packet,
            )))
        }
        ("core.net", "udp_packet_truncated") => {
            let Some(packet) = args.first().and_then(|value| http_handle_id(value, "UDPPacket"))
            else {
                return Some(Err(unsupported(
                    "core.net.udp_packet_truncated packet",
                    span,
                )));
            };
            Some(Ok(crate::net_http_rt::runtime_udp_packet_truncated(packet)))
        }
        ("core.net", "ready_readable" | "ready_writable") => {
            let Some(ready) = args.first().and_then(|value| http_handle_id(value, "NetReady"))
            else {
                return Some(Err(unsupported("core.net.ready receiver", span)));
            };
            let value = if method == "ready_readable" {
                crate::net_http_rt::runtime_net_ready_readable(ready)
            } else {
                crate::net_http_rt::runtime_net_ready_writable(ready)
            };
            Some(Ok(value))
        }
        ("core.process", "cmd") => {
            let Some(CtValue::List(items)) = args.into_iter().next() else {
                return Some(Err(unsupported("core.process.cmd arguments", span)));
            };
            if !items.iter().all(|item| matches!(item, CtValue::Str(_))) {
                return Some(Err(unsupported(
                    "core.process.cmd expects text command words",
                    span,
                )));
            }
            Some(Ok(interpreter_process_spec(items)))
        }
        ("core.process", "pipeline") => {
            let Some(CtValue::List(items)) = args.first() else {
                return Some(Err(unsupported("core.process.pipeline arguments", span)));
            };
            let mut specs = Vec::with_capacity(items.len());
            for item in items {
                match process_spec_from_value(item, span) {
                    Ok(spec) => specs.push(spec),
                    Err(error) => return Some(Err(error)),
                }
            }
            Some(Ok(process_result_outcome(process_prelude::spec_pipeline(&specs))))
        }
        ("core.testing", "temp_dir") => {
            let Some(CtValue::Str(prefix)) = args.first() else {
                return Some(Err(unsupported("core.testing.temp_dir arguments", span)));
            };
            Some(Ok(CtValue::Str(
                crate::testing_shared::jet_testing_temp_dir_path(prefix),
            )))
        }
        ("core.services", "runtime") => {
            let (Some(CtValue::Str(store)), Some(retention)) = (args.first(), args.get(1)) else {
                return Some(Err(unsupported("core.services.runtime arguments", span)));
            };
            let Some(retention_ms) = service_duration_ms(retention) else {
                return Some(Err(unsupported("core.services.runtime duration", span)));
            };
            Some(Ok(service_runtime_value(store.clone(), retention_ms)))
        }
        ("core.db", "policy") => {
            let (Some(CtValue::Str(table)), Some(CtValue::Str(expression))) =
                (args.first(), args.get(1))
            else {
                return Some(Err(unsupported("core.db.policy arguments", span)));
            };
            Some(Ok(match wire::jet_db_policy_validate(table, expression) {
                Ok(()) => CtValue::Present(Box::new(db_policy_value(
                    table.clone(),
                    expression.clone(),
                ))),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            }))
        }
        ("core.db", "transaction" | "migrate") => {
            let Some(scope_value) = args.first() else {
                return Some(Err(unsupported("database scope", span)));
            };
            let Some(scope) = db_scope_parts(scope_value) else {
                return Some(Ok(CtValue::failed(Box::new(db_err(
                    "database transaction requires a policy scope",
                )))));
            };
            let Some(CtValue::Str(label)) = args.get(1) else {
                return Some(Err(unsupported("database transaction label", span)));
            };
            let steps = match ambient_db_steps(args.get(2)?, span) {
                Ok(steps) => steps,
                Err(error) => return Some(Err(error)),
            };
            let mut backend = AmbientDbBackend { scope };
            let result = if method == "migrate" {
                wire::jet_db_migrate(&mut backend, label, &steps)
            } else {
                wire::jet_db_transaction(&mut backend, label, &steps)
            };
            Some(Ok(match result {
                Ok(done) => CtValue::Present(Box::new(CtValue::Int(done))),
                Err(error) => CtValue::failed(Box::new(db_err(error.message))),
            }))
        }
        ("core.io", "confirm") => {
            let Some(CtValue::Str(prompt)) = args.first() else {
                return Some(Err(unsupported("core.io.confirm prompt", span)));
            };
            Some(Ok(CtValue::Bool(IO::prompt_confirm(prompt))))
        }
        ("core.io", "choose") => {
            let Some(CtValue::Str(prompt)) = args.first() else {
                return Some(Err(unsupported("core.io.choose prompt", span)));
            };
            let Some(CtValue::List(items)) = args.get(1) else {
                return Some(Err(unsupported("core.io.choose items", span)));
            };
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                let CtValue::Str(item) = item else {
                    return Some(Err(unsupported("core.io.choose item", span)));
                };
                values.push(item.clone());
            }
            Some(Ok(match IO::prompt_choose(prompt, &values) {
                Ok(item) => CtValue::Present(Box::new(CtValue::Str(item))),
                Err(error) => CtValue::failed(Box::new(io_error("InvalidInput", error))),
            }))
        }
        ("core.io", "input_secret") => {
            let Some(CtValue::Str(prompt)) = args.first() else {
                return Some(Err(unsupported("core.io.input_secret prompt", span)));
            };
            Some(Ok(match IO::prompt_input_secret(prompt) {
                Ok(secret) => CtValue::Present(Box::new(CtValue::Str(secret))),
                Err(error) => {
                    let kind = if error == "secret input needs a terminal" {
                        "InvalidInput"
                    } else {
                        "Other"
                    };
                    CtValue::failed(Box::new(io_error(kind, error)))
                }
            }))
        }
        ("core.db", "open_memory") => Some(Ok(db_conn_value(DB::runtime_open_memory()))),
        ("core.db", "open") => {
            let path = match args.first() {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Some(Err(unsupported("core.db.open path", span))),
            };
            Some(Ok(db_conn_value(DB::runtime_open(&path))))
        }
        ("core.mod", "load") => {
            let Some(CtValue::Str(path)) = args.first() else {
                return Some(Err(unsupported("core.mod.load path", span)));
            };
            let Some(read) = args.get(1).and_then(mod_grant_roots) else {
                return Some(Err(unsupported("core.mod.load grant", span)));
            };
            Some(Ok(match crate::Mod::load(path.clone(), read) {
                Ok(handle) => CtValue::Present(Box::new(mod_value(handle))),
                Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
            }))
        }
        ("core.crypto.random", "bytes") => {
            let Some(CtValue::Int(count)) = args.first() else {
                return Some(Err(unsupported("core.crypto.random.bytes count", span)));
            };
            Some(Ok(CtValue::Bytes(
                Crypto::runtime::jet_std_crypto_random_bytes(*count),
            )))
        }
        ("core.uuid", "v4") => Some(Ok(CtValue::Str(
            Crypto::runtime::jet_crypto_uuid_v4(),
        ))),
        ("core.uuid", "v7") => {
            let timestamp = match clock_now(args.first()?, span) {
                Ok(timestamp) => timestamp,
                Err(error) => return Some(Err(error)),
            };
            Some(Ok(CtValue::Str(
                Crypto::runtime::jet_crypto_uuid_v7(timestamp),
            )))
        }
        ("core.crypto", "sha256") => {
            let data = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let digest = Crypto::runtime::jet_crypto_sha256_typed_impl(&data);
            Some(Ok(digest256_value(
                Crypto::runtime::jet_crypto_digest256_bytes_impl(&digest),
            )))
        }
        ("core.crypto", "sha1") => {
            let data = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Str(Crypto::runtime::jet_crypto_sha1_hex(&data))))
        }
        ("core.crypto", "sha224") => {
            let data = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Str(Crypto::runtime::jet_crypto_sha224_hex(&data))))
        }
        ("core.crypto", "sha384") => {
            let data = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Str(Crypto::runtime::jet_crypto_sha384_hex(&data))))
        }
        ("core.crypto", "sha3_224") => {
            let data = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Str(Crypto::runtime::jet_crypto_sha3_224_hex(&data))))
        }
        ("core.crypto", "sha3_256") => {
            let data = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Str(Crypto::runtime::jet_crypto_sha3_256_hex(&data))))
        }
        ("core.crypto", "sha3_384") => {
            let data = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Str(Crypto::runtime::jet_crypto_sha3_384_hex(&data))))
        }
        ("core.crypto", "sha3_512") => {
            let data = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Str(Crypto::runtime::jet_crypto_sha3_512_hex(&data))))
        }
        ("core.crypto", "pbkdf2_hmac") => {
            let password = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let salt = match as_bytes(args.get(1)?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let iterations = match args.get(2) {
                Some(CtValue::Int(value)) => *value,
                _ => return Some(Err(unsupported("pbkdf2_hmac iterations", span))),
            };
            let key_len = match args.get(3) {
                Some(CtValue::Int(value)) => *value,
                _ => return Some(Err(unsupported("pbkdf2_hmac key length", span))),
            };
            Some(Ok(CtValue::Bytes(Crypto::runtime::jet_crypto_pbkdf2_hmac(
                &password,
                &salt,
                iterations,
                key_len,
            ))))
        }
        ("core.crypto", "__hasher_new") => Some(Ok(hasher_value(Vec::new()))),
        ("core.crypto", "__hasher_update") => {
            let mut current = match hasher_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let chunk = match as_bytes(args.get(1)?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            current.extend_from_slice(&chunk);
            Some(Ok(hasher_value(current)))
        }
        ("core.crypto", "__hasher_digest") => {
            let data = match hasher_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let digest = Crypto::runtime::jet_crypto_sha256_typed_impl(&data);
            Some(Ok(CtValue::Str(hex_bytes(
                &Crypto::runtime::jet_crypto_digest256_bytes_impl(&digest),
            ))))
        }
        ("core.crypto", "__digest256_hex") => {
            let bytes = match struct_bytes(args.first()?, "Digest256", span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Str(hex_bytes(&bytes))))
        }
        ("core.crypto", "__digest256_bytes") => {
            let bytes = match struct_bytes(args.first()?, "Digest256", span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Bytes(bytes)))
        }
        ("core.crypto", "sha512_bytes") => {
            let data = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Str(Crypto::runtime::jet_crypto_sha512_impl(
                &data,
            ))))
        }
        ("core.crypto", "blake3_bytes") => {
            let data = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Str(Crypto::runtime::jet_crypto_blake3_impl(
                &data,
            ))))
        }
        ("core.crypto", "constant_time_equal_bytes") => {
            let a = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let b = match as_bytes(args.get(1)?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Bool(
                Crypto::runtime::jet_crypto_constant_time_equal_bytes_impl(&a, &b),
            )))
        }
        ("core.crypto", "constant_time_equal") => {
            let a = match to_secret(args.first()?, span) {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };
            let b = match to_secret(args.get(1)?, span) {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(CtValue::Bool(
                Crypto::runtime::jet_crypto_constant_time_secret_impl(&a, &b),
            )))
        }
        ("core.crypto", "hkdf_sha256") => {
            let ikm = match to_secret(args.first()?, span) {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };
            let salt = match as_bytes(args.get(1)?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let info = match as_bytes(args.get(2)?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let len = match args.get(3) {
                Some(CtValue::Int(n)) => *n,
                _ => return Some(Err(unsupported("hkdf length", span))),
            };
            Some(Ok(
                match Crypto::runtime::jet_crypto_hkdf_typed_impl(&ikm, &salt, &info, len) {
                    Ok(secret) => CtValue::Present(Box::new(secret_value(
                        Crypto::runtime::jet_crypto_expert_secret_bytes_impl(&secret),
                    ))),
                    Err(e) => CtValue::failed(Box::new(crypto_err(e.to_string()))),
                },
            ))
        }
        ("core.crypto", "x25519_public") => {
            let secret = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(
                match Crypto::runtime::jet_crypto_x25519_public_impl(&secret) {
                    Ok(pub_bytes) => CtValue::Present(Box::new(CtValue::Bytes(pub_bytes))),
                    Err(e) => CtValue::failed(Box::new(CtValue::Str(e))),
                },
            ))
        }
        ("core.crypto", "x25519_shared") => {
            let secret = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let public = match as_bytes(args.get(1)?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(
                match Crypto::runtime::jet_crypto_x25519_shared_impl(&secret, &public) {
                    Ok(shared) => CtValue::Present(Box::new(CtValue::Bytes(shared))),
                    Err(e) => CtValue::failed(Box::new(CtValue::Str(e))),
                },
            ))
        }
        ("core.crypto", "password_hash") => {
            let password = match to_secret(args.first()?, span) {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };
            Some(Ok(
                match Crypto::runtime::jet_crypto_password_hash_typed_impl(&password) {
                    Ok(ph) => CtValue::Present(Box::new(password_hash_value(
                        Crypto::runtime::jet_crypto_password_text_impl(&ph),
                    ))),
                    Err(e) => CtValue::failed(Box::new(crypto_err(e.to_string()))),
                },
            ))
        }
        ("core.crypto", "password_verify") => {
            let password = match to_secret(args.first()?, span) {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };
            let stored = match args.get(1) {
                Some(CtValue::Struct { type_name, fields }) if type_name == "PasswordHash" => {
                    fields
                        .iter()
                        .find_map(|(n, v)| match (n.as_str(), v) {
                            ("text", CtValue::Str(s)) => Some(s.clone()),
                            _ => None,
                        })
                        .ok_or_else(|| unsupported("PasswordHash.text", span))
                }
                _ => Err(unsupported("password_verify stored hash", span)),
            };
            let stored = match stored {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };
            let ph = Crypto::runtime::password_hash_from_text(stored);
            Some(Ok(
                match Crypto::runtime::jet_crypto_password_verify_typed_impl(&password, &ph) {
                    Ok(b) => CtValue::Present(Box::new(CtValue::Bool(b))),
                    Err(e) => CtValue::failed(Box::new(crypto_err(e.to_string()))),
                },
            ))
        }
        ("core.crypto", "__secret_from_text") => {
            let text = match args.first() {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Some(Err(unsupported("Secret.from_text", span))),
            };
            let secret = Crypto::runtime::jet_crypto_secret_from_text_impl(text);
            Some(Ok(secret_value(
                Crypto::runtime::jet_crypto_expert_secret_bytes_impl(&secret),
            )))
        }
        ("core.crypto", "__secret_from_bytes") => {
            let bytes = match as_bytes(args.first()?, span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let secret = Crypto::runtime::jet_crypto_secret_from_bytes_impl(bytes);
            Some(Ok(secret_value(
                Crypto::runtime::jet_crypto_expert_secret_bytes_impl(&secret),
            )))
        }
        ("core.crypto", "__x25519_generate") => Some(Ok(
            match Crypto::runtime::jet_crypto_x25519_generate_impl() {
                Ok(key) => CtValue::Present(Box::new(x25519_secret_value(
                    Crypto::runtime::jet_crypto_expert_x25519_secret_bytes_impl(&key),
                ))),
                Err(e) => CtValue::failed(Box::new(crypto_err(e.to_string()))),
            },
        )),
        ("core.crypto", "__x25519_public") => {
            let bytes = match struct_bytes(args.first()?, "X25519SecretKey", span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            match Crypto::runtime::jet_crypto_x25519_public_impl(&bytes) {
                Ok(pub_bytes) => Some(Ok(x25519_public_value(pub_bytes))),
                Err(e) => Some(Err(unsupported(&e, span))),
            }
        }
        ("core.crypto", "__password_text") => {
            let text = match args.first() {
                Some(CtValue::Struct { type_name, fields }) if type_name == "PasswordHash" => {
                    fields.iter().find_map(|(n, v)| match (n.as_str(), v) {
                        ("text", CtValue::Str(s)) => Some(s.clone()),
                        _ => None,
                    })
                }
                _ => None,
            };
            match text {
                Some(s) => Some(Ok(CtValue::Str(s))),
                None => Some(Err(unsupported("PasswordHash.text", span))),
            }
        }
        ("core.crypto", "file_seal") => {
            let recipients = match args.first() {
                Some(CtValue::List(items)) => {
                    let mut out = Vec::new();
                    for item in items {
                        let bytes = match struct_bytes(item, "X25519PublicKey", span) {
                            Ok(b) => b,
                            Err(e) => return Some(Err(e)),
                        };
                        match Crypto::runtime::jet_crypto_x25519_public_from_bytes_impl(bytes) {
                            Ok(pk) => out.push(pk),
                            Err(e) => {
                                return Some(Ok(CtValue::failed(Box::new(crypto_err(
                                    e.to_string(),
                                )))))
                            }
                        }
                    }
                    out
                }
                _ => return Some(Err(unsupported("file_seal recipients", span))),
            };
            let source = match args.get(1).and_then(path_string) {
                Some(s) => s,
                None => return Some(Err(unsupported("file_seal source", span))),
            };
            let dest = match args.get(2).and_then(path_string) {
                Some(s) => s,
                None => return Some(Err(unsupported("file_seal destination", span))),
            };
            Some(Ok(
                match Crypto::runtime::jet_crypto_file_seal_impl(
                    recipients,
                    &source,
                    &dest,
                    || false,
                ) {
                    Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
                    Err(e) => CtValue::failed(Box::new(crypto_err(e.to_string()))),
                },
            ))
        }
        ("core.crypto", "file_open") => {
            let key_bytes = match struct_bytes(args.first()?, "X25519SecretKey", span) {
                Ok(b) => b,
                Err(e) => return Some(Err(e)),
            };
            let source = match args.get(1).and_then(path_string) {
                Some(s) => s,
                None => return Some(Err(unsupported("file_open source", span))),
            };
            let dest = match args.get(2).and_then(path_string) {
                Some(s) => s,
                None => return Some(Err(unsupported("file_open destination", span))),
            };
            Some(Ok(match Crypto::x25519_secret_from_vec(key_bytes) {
                Ok(recipient) => {
                    match Crypto::runtime::jet_crypto_file_open_impl(
                        &recipient,
                        &source,
                        &dest,
                        || false,
                    ) {
                        Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
                        Err(e) => CtValue::failed(Box::new(crypto_err(e.to_string()))),
                    }
                }
                Err(e) => CtValue::failed(Box::new(crypto_err(e))),
            }))
        }
        _ => None,
    }
}

struct InterpWebCallback {
    id: i64,
    callable: CtValue,
    args: Vec<CtValue>,
    reply: mpsc::SyncSender<CtValue>,
}

struct InterpWebServer {
    requests: Mutex<mpsc::Receiver<InterpWebCallback>>,
    replies: Mutex<HashMap<i64, mpsc::SyncSender<CtValue>>>,
}

static INTERP_WEB_SERVERS: OnceLock<Mutex<Vec<Arc<InterpWebServer>>>> = OnceLock::new();
static INTERP_WEB_CALLBACK_ID: AtomicI64 = AtomicI64::new(1);

fn interp_web_servers() -> &'static Mutex<Vec<Arc<InterpWebServer>>> {
    INTERP_WEB_SERVERS.get_or_init(|| Mutex::new(Vec::new()))
}

fn interp_web_server_value(index: usize) -> CtValue {
    CtValue::Struct {
        type_name: "__JetInterpWebServer".to_string(),
        fields: vec![("index".to_string(), CtValue::Int(index as i64))],
    }
}

fn interp_web_server(value: &CtValue) -> Option<Arc<InterpWebServer>> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != "__JetInterpWebServer" {
        return None;
    }
    let index = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("index", CtValue::Int(index)) => usize::try_from(*index).ok(),
        _ => None,
    })?;
    interp_web_servers().lock().ok()?.get(index).cloned()
}

fn interp_web_field<'a>(
    fields: &'a [(String, CtValue)],
    name: &str,
) -> Option<&'a CtValue> {
    fields
        .iter()
        .find_map(|(field, value)| (field == name).then_some(value))
}

fn interp_web_steps(value: &CtValue, span: Span) -> Result<Vec<(String, Vec<CtValue>)>, Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("WebApp state", span));
    };
    if type_name != "__JetTirWebAppState" {
        return Err(unsupported("WebApp state", span));
    }
    let Some(CtValue::List(steps)) = interp_web_field(fields, "steps") else {
        return Err(unsupported("WebApp steps", span));
    };
    steps
        .iter()
        .map(|step| {
            let CtValue::Struct { type_name, fields } = step else {
                return Err(unsupported("WebApp step", span));
            };
            if type_name != "__JetTirWebAppStep" {
                return Err(unsupported("WebApp step", span));
            }
            let method = match interp_web_field(fields, "method") {
                Some(CtValue::Str(method)) => method.clone(),
                _ => return Err(unsupported("WebApp step method", span)),
            };
            let args = match interp_web_field(fields, "args") {
                Some(CtValue::List(args)) => args.clone(),
                _ => return Err(unsupported("WebApp step arguments", span)),
            };
            Ok((method, args))
        })
        .collect()
}

fn interp_web_string(args: &[CtValue], index: usize, span: Span) -> Result<String, Diagnostic> {
    match args.get(index) {
        Some(CtValue::Str(value)) => Ok(value.clone()),
        _ => Err(unsupported("WebApp text argument", span)),
    }
}

fn interp_web_callback(
    sender: &mpsc::Sender<InterpWebCallback>,
    callable: CtValue,
    args: Vec<CtValue>,
) -> CtValue {
    let id = INTERP_WEB_CALLBACK_ID.fetch_add(1, Ordering::Relaxed);
    let (reply, receive) = mpsc::sync_channel(1);
    if sender
        .send(InterpWebCallback {
            id,
            callable,
            args,
            reply,
        })
        .is_err()
    {
        return CtValue::Unit;
    }
    receive.recv().unwrap_or(CtValue::Unit)
}

fn interp_web_page(value: CtValue) -> crate::Web::web_rt::JetWebPage {
    let CtValue::Struct { type_name, fields } = value else {
        return crate::Web::web_rt::jet_web_page(String::new(), String::new());
    };
    if type_name != "__JetTirWebPage" {
        return crate::Web::web_rt::jet_web_page(String::new(), String::new());
    }
    let text = |name| match interp_web_field(&fields, name) {
        Some(CtValue::Str(value)) => value.clone(),
        _ => String::new(),
    };
    crate::Web::web_rt::jet_web_page(text("title"), text("body"))
}

fn materialize_interp_web_app(
    state: &CtValue,
    sender: Option<&mpsc::Sender<InterpWebCallback>>,
    span: Span,
) -> Result<crate::Web::web_rt::JetWebApp, Diagnostic> {
    let mut app = crate::Web::web_rt::jet_web_app();
    for (method, args) in interp_web_steps(state, span)? {
        app = match method.as_str() {
            "route" | "page" | "layout" => {
                let path = interp_web_string(&args, 0, span)?;
                let callable = args
                    .get(1)
                    .cloned()
                    .ok_or_else(|| unsupported("WebApp page callback", span))?;
                let callback_sender = sender.cloned();
                let handler = move || {
                    callback_sender
                        .as_ref()
                        .map(|sender| {
                            interp_web_page(interp_web_callback(
                                sender,
                                callable.clone(),
                                Vec::new(),
                            ))
                        })
                        .unwrap_or_default()
                };
                match method.as_str() {
                    "route" => app.route(path, std::sync::Arc::new(handler)),
                    "page" => app.page(path, std::sync::Arc::new(handler)),
                    _ => app.layout(path, std::sync::Arc::new(handler)),
                }
            }
            "action" | "form" | "data" => {
                let name = interp_web_string(&args, 0, span)?;
                let callable = args
                    .get(1)
                    .cloned()
                    .ok_or_else(|| unsupported("WebApp action callback", span))?;
                let callback_sender = sender.cloned();
                let handler = move || {
                    if let Some(sender) = &callback_sender {
                        let _ = interp_web_callback(sender, callable.clone(), Vec::new());
                    }
                };
                match method.as_str() {
                    "action" => app.action(name, std::sync::Arc::new(handler)),
                    "form" => app.form(name, std::sync::Arc::new(handler)),
                    _ => app.data(name, std::sync::Arc::new(handler)),
                }
            }
            "mount" => {
                let prefix = interp_web_string(&args, 0, span)?;
                let callable = args
                    .get(1)
                    .cloned()
                    .ok_or_else(|| unsupported("WebApp mount callback", span))?;
                let callback_sender = sender.cloned();
                app.mount(prefix, std::sync::Arc::new(move |path: &String| {
                    if let Some(sender) = &callback_sender {
                        let _ = interp_web_callback(
                            sender,
                            callable.clone(),
                            vec![CtValue::Str(path.clone())],
                        );
                    }
                }))
            }
            "routes" => app.routes(interp_web_string(&args, 0, span)?),
            "security" => app.security(interp_web_string(&args, 0, span)?),
            "assets" => app.assets(interp_web_string(&args, 0, span)?),
            "split" => app.split(interp_web_string(&args, 0, span)?),
            "code_split" => app.code_split(interp_web_string(&args, 0, span)?),
            "cache" => app.cache(interp_web_string(&args, 0, span)?),
            "a11y" => app.a11y(interp_web_string(&args, 0, span)?),
            "adapter" => app.adapter(interp_web_string(&args, 0, span)?),
            "csr" => app.csr(),
            "ssr" => app.ssr(),
            "ssg" => app.ssg(),
            "stream" => app.stream(),
            "streaming" => app.streaming(),
            "island" => app.island(),
            "hydration_dev" => app.hydration_dev(),
            "hydration_release" => app.hydration_release(),
            _ => return Err(unsupported(&format!("WebApp.{method}"), span)),
        };
    }
    Ok(app)
}

fn ambient_webapp_handle(
    op: &str,
    recv: &mut CtValue,
    args: &mut [CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let result = match op {
        "WebAppFacts" => materialize_interp_web_app(recv, None, span)
            .map(|app| CtValue::Str(app.facts_json())),
        "WebAppServe" => {
            let (requests, receiver) = mpsc::channel();
            let app = match materialize_interp_web_app(recv, Some(&requests), span) {
                Ok(app) => app,
                Err(error) => return Some(Err(error)),
            };
            let port = match args.first() {
                Some(CtValue::Int(port)) => Some(*port),
                None => None,
                _ => return Some(Err(unsupported("WebApp serve port", span))),
            };
            std::thread::spawn(move || match port {
                Some(port) => app.serve_on(port),
                None => app.serve(),
            });
            let server = Arc::new(InterpWebServer {
                requests: Mutex::new(receiver),
                replies: Mutex::new(HashMap::new()),
            });
            let mut servers = interp_web_servers()
                .lock()
                .expect("interpreter WebApp registry poisoned");
            let index = servers.len();
            servers.push(server);
            Ok(interp_web_server_value(index))
        }
        "WebAppNext" => {
            let server = match interp_web_server(recv) {
                Some(server) => server,
                None => return Some(Err(unsupported("WebApp server handle", span))),
            };
            let request = match server
                .requests
                .lock()
                .expect("interpreter WebApp request queue poisoned")
                .recv()
            {
                Ok(request) => request,
                Err(_) => return Some(Err(unsupported("WebApp request queue", span))),
            };
            server
                .replies
                .lock()
                .expect("interpreter WebApp reply queue poisoned")
                .insert(request.id, request.reply);
            Ok(CtValue::Struct {
                type_name: "__JetInterpWebCallback".to_string(),
                fields: vec![
                    ("id".to_string(), CtValue::Int(request.id)),
                    ("callable".to_string(), request.callable),
                    ("args".to_string(), CtValue::List(request.args)),
                ],
            })
        }
        "WebAppReply" => {
            let server = match interp_web_server(recv) {
                Some(server) => server,
                None => return Some(Err(unsupported("WebApp server handle", span))),
            };
            let id = match args.first() {
                Some(CtValue::Int(id)) => *id,
                _ => return Some(Err(unsupported("WebApp callback id", span))),
            };
            let value = args.get(1).cloned().unwrap_or(CtValue::Unit);
            let reply = server
                .replies
                .lock()
                .expect("interpreter WebApp reply queue poisoned")
                .remove(&id);
            match reply {
                Some(reply) => reply
                    .send(value)
                    .map(|_| CtValue::Unit)
                    .map_err(|_| unsupported("WebApp callback reply", span)),
                None => Err(unsupported("WebApp callback reply id", span)),
            }
        }
        _ => return None,
    };
    Some(result)
}

pub fn ambient_handle(
    op: &str,
    recv: &mut CtValue,
    args: &mut [CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if let Some(result) = jet_codegen::Comptime::EmailAdapter::ambient_handle(
        op,
        recv,
        args,
        span,
    ) {
        return Some(result);
    }
    if let Some(result) = crate::enc_stream::ambient_handle(op, recv, args, span) {
        return Some(result);
    }
    if let Some(result) = ambient_process_handle(op, recv, args, span) {
        return Some(result);
    }
    if let Some(result) = ambient_process_child_handle(op, recv, args, span) {
        return Some(result);
    }
    if let Some(result) = ambient_net_handle(op, recv, args, span) {
        return Some(result);
    }
    if let Some(result) = ambient_webapp_handle(op, recv, args, span) {
        return Some(result);
    }
    if let Some(result) = ambient_http_handle(op, recv, args, span) {
        return Some(result);
    }
    if op == "DBWithPolicy" {
        let handle = db_handle(recv)?;
        let (CtValue::Struct { fields, .. }, CtValue::Str(user)) =
            (args.first()?, args.get(1)?)
        else {
            return Some(Err(unsupported("DBConnection.with_policy arguments", span)));
        };
        let table = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
            ("table", CtValue::Str(value)) => Some(value.clone()),
            _ => None,
        });
        let expression = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
            ("expression", CtValue::Str(value)) => Some(value.clone()),
            _ => None,
        });
        let (Some(table), Some(expression)) = (table, expression) else {
            return Some(Err(unsupported("DBConnection.with_policy policy", span)));
        };
        return Some(match wire::jet_db_policy_validate(&table, &expression) {
            Ok(()) => Ok(db_scope_value(handle, table, expression, user.clone())),
            Err(error) => Err(unsupported(&format!("row policy: {error}"), span)),
        });
    }
    if matches!(
        op,
        "ServiceRuntimeSend"
            | "ServiceRuntimeRetry"
            | "ServiceRuntimeDeadLetter"
            | "ServiceRuntimeRetain"
            | "ServiceRuntimeCommit"
    ) {
        let Some(runtime) = service_runtime_parts(recv) else {
            return Some(Err(unsupported("ServiceRuntime receiver", span)));
        };
        if op == "ServiceRuntimeCommit" {
            let Some(CtValue::Str(id)) = args.first() else {
                return Some(Err(unsupported("ServiceRuntime.commit id", span)));
            };
            return Some(Ok(match service_prelude::jet_services_runtime_commit(&runtime, id) {
                Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
                Err(error) => CtValue::failed(Box::new(service_error_value(error))),
            }));
        }
        let result = match op {
            "ServiceRuntimeSend" => {
                let Some(endpoint) = args.first().and_then(service_endpoint_value) else {
                    return Some(Err(unsupported("ServiceRuntime.send endpoint", span)));
                };
                let Some(CtValue::Str(message)) = args.get(1) else {
                    return Some(Err(unsupported("ServiceRuntime.send message", span)));
                };
                let Some(CtValue::Str(key)) = args.get(2) else {
                    return Some(Err(unsupported("ServiceRuntime.send key", span)));
                };
                service_prelude::jet_services_runtime_send(&runtime, &endpoint, message, key)
            }
            "ServiceRuntimeRetry" => {
                let Some(CtValue::Str(id)) = args.first() else {
                    return Some(Err(unsupported("ServiceRuntime.retry id", span)));
                };
                service_prelude::jet_services_runtime_retry(&runtime, id)
            }
            "ServiceRuntimeDeadLetter" => {
                let Some(CtValue::Str(id)) = args.first() else {
                    return Some(Err(unsupported("ServiceRuntime.dead_letter id", span)));
                };
                service_prelude::jet_services_runtime_dead_letter(&runtime, id)
            }
            "ServiceRuntimeRetain" => {
                let Some(CtValue::Str(id)) = args.first() else {
                    return Some(Err(unsupported("ServiceRuntime.retain id", span)));
                };
                service_prelude::jet_services_runtime_retain(&runtime, id)
            }
            _ => unreachable!(),
        };
        return Some(Ok(match result {
            Ok(receipt) => CtValue::Present(Box::new(service_receipt_value(receipt))),
            Err(error) => CtValue::failed(Box::new(service_error_value(error))),
        }));
    }
    if op == "ModOnTick" {
        let Some(handle) = mod_handle(recv) else {
            return Some(Err(unsupported("Mod receiver", span)));
        };
        let Some(CtValue::Int(dt)) = args.first() else {
            return Some(Err(unsupported("Mod.on_tick dt", span)));
        };
        return Some(Ok(match crate::Mod::on_tick(handle, *dt) {
            Ok(value) => CtValue::Present(Box::new(CtValue::Int(value))),
            Err(error) => CtValue::failed(Box::new(CtValue::Str(error))),
        }));
    }
    let handle = db_handle(recv)?;
    match op {
        "DBBegin" => Some(Ok(CtValue::Bool(DB::runtime_begin(handle)))),
        "DBCommit" => Some(Ok(CtValue::Bool(DB::runtime_commit(handle)))),
        "DBRollback" => Some(Ok(CtValue::Bool(DB::runtime_rollback(handle)))),
        "DBClose" => Some(Ok(CtValue::Bool(DB::runtime_close(handle)))),
        "DBExecute" => {
            let sql = match args.first() {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Some(Err(unsupported("DBScope.execute sql", span))),
            };
            let values = match db_params(args.get(1).unwrap_or(&CtValue::List(vec![])), span) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            let (handle, sql, values) = match db_scope_parts(recv) {
                Some((handle, table, expression, user)) => match wire::jet_db_apply_policy(
                    &sql, &values, &table, &expression, &user,
                ) {
                    Ok((sql, values)) => (handle, sql, values),
                    Err(error) => return Some(Ok(CtValue::failed(Box::new(db_err(error.message))))),
                },
                None => return Some(Ok(CtValue::failed(Box::new(db_err(
                    "database row operations require a policy scope",
                ))))),
            };
            let params = wire::jet_db_encode_params(&values);
            let out = DB::runtime_execute(handle, &sql, &params);
            Some(Ok(match wire::jet_db_decode_execute_result(&out) {
                Ok(n) => CtValue::Present(Box::new(CtValue::Int(n))),
                Err(e) => CtValue::failed(Box::new(db_err(e.message))),
            }))
        }
        "DBQuery" => {
            let sql = match args.first() {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Some(Err(unsupported("DBScope.query sql", span))),
            };
            let values = match db_params(args.get(1).unwrap_or(&CtValue::List(vec![])), span) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            let (handle, sql, values) = match db_scope_parts(recv) {
                Some((handle, table, expression, user)) => match wire::jet_db_apply_policy(
                    &sql, &values, &table, &expression, &user,
                ) {
                    Ok((sql, values)) => (handle, sql, values),
                    Err(error) => return Some(Ok(CtValue::failed(Box::new(db_err(error.message))))),
                },
                None => return Some(Ok(CtValue::failed(Box::new(db_err(
                    "database row operations require a policy scope",
                ))))),
            };
            let params = wire::jet_db_encode_params(&values);
            let out = DB::runtime_query(handle, &sql, &params);
            Some(Ok(match wire::jet_db_decode_query_result(&out) {
                Ok(rows) => CtValue::Present(Box::new(CtValue::List(
                    rows.into_iter().map(row_map).collect(),
                ))),
                Err(e) => CtValue::failed(Box::new(db_err(e.message))),
            }))
        }
        "DBQueryOne" => {
            let sql = match args.first() {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Some(Err(unsupported("DBScope.query_one sql", span))),
            };
            let values = match db_params(args.get(1).unwrap_or(&CtValue::List(vec![])), span) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            let scope = match db_scope_parts(recv) {
                Some(scope) => scope,
                None => return Some(Ok(CtValue::failed(Box::new(db_err(
                    "database row operations require a policy scope",
                ))))),
            };
            Some(Ok(match ambient_db_scope_query(&scope, &sql, &values, false) {
                Ok(rows) => {
                    let opt = match wire::jet_db_first_row(rows) {
                        Ok(row) => CtValue::Present(Box::new(row_map(row))),
                        Err(_) => CtValue::absent(Type::Map {
                            key: Box::new(Type::String),
                            key_span: None,
                            value: Box::new(Type::Named("DBValue".into())),
                        }),
                    };
                    CtValue::Present(Box::new(opt))
                }
                Err(e) => CtValue::failed(Box::new(db_err(e.message))),
            }))
        }
        "DBLive" => {
            let sql = match args.first() {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Some(Err(unsupported("DBScope.live sql", span))),
            };
            let values = match db_params(args.get(1).unwrap_or(&CtValue::List(vec![])), span) {
                Ok(values) => values,
                Err(error) => return Some(Err(error)),
            };
            let (handle, table, expression, user) = match db_scope_parts(recv) {
                Some(parts) => parts,
                None => {
                    return Some(Ok(CtValue::failed(Box::new(db_err(
                        "database live queries require a policy scope",
                    )))))
                }
            };
            let (sql, values) = match wire::jet_db_apply_policy(
                &sql, &values, &table, &expression, &user,
            ) {
                Ok(value) => value,
                Err(error) => return Some(Ok(CtValue::failed(Box::new(db_err(error.message))))),
            };
            let out = DB::runtime_query(handle, &sql, &wire::jet_db_encode_params(&values));
            Some(Ok(match wire::jet_db_decode_query_result(&out) {
                Ok(rows) => {
                    let footprint = format!("db:{table}:{sql}");
                    let initial = format!("{rows:?}");
                    match jet_codegen::Comptime::AppLite::apply(
                        "live",
                        &[CtValue::Str(footprint), CtValue::Str(initial)],
                        span,
                    ) {
                        Ok(query) => CtValue::Present(Box::new(query)),
                        Err(error) => return Some(Err(error)),
                    }
                }
                Err(error) => CtValue::failed(Box::new(db_err(error.message))),
            }))
        }
        _ => None,
    }
}

// ── I9 HTTP ambient: marshal CtValue ↔ shared runtime_* Prelude adapters ───

fn http_handle_value(type_name: &str, handle: i64) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: vec![("handle".to_string(), CtValue::Int(handle))],
    }
}

fn http_handle_id(recv: &CtValue, type_name: &str) -> Option<i64> {
    match recv {
        CtValue::Struct {
            type_name: tn,
            fields,
        } if tn == type_name => fields.iter().find_map(|(n, v)| match (n.as_str(), v) {
            ("handle", CtValue::Int(h)) if *h > 0 => Some(*h),
            _ => None,
        }),
        _ => None,
    }
}

fn net_ready_interest_value(value: &CtValue) -> Option<i64> {
    match value {
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if type_name == "NetReadyInterest" && args.is_empty() => match variant.as_str() {
            "Read" => Some(0),
            "Write" => Some(1),
            "ReadWrite" => Some(2),
            _ => None,
        },
        _ => None,
    }
}

fn net_shutdown_value(value: &CtValue) -> Option<i64> {
    match value {
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if type_name == "NetShutdown" && args.is_empty() => match variant.as_str() {
            "Read" => Some(0),
            "Write" => Some(1),
            "Both" => Some(2),
            _ => None,
        },
        _ => None,
    }
}

fn duration_ns(value: &CtValue) -> Option<i64> {
    match value {
        CtValue::Struct { type_name, fields } if type_name == "Duration" => fields
            .iter()
            .find_map(|(name, value)| (name == "ns").then_some(value))
            .and_then(|value| match value {
                CtValue::Int(ns) => Some(*ns),
                _ => None,
            }),
        _ => None,
    }
}

fn net_bytes_value(value: &CtValue) -> Option<Vec<u8>> {
    match value {
        CtValue::Bytes(bytes) => Some(bytes.clone()),
        CtValue::List(values) => values
            .iter()
            .map(|value| match value {
                CtValue::Int(byte) if (0..=255).contains(byte) => Some(*byte as u8),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn ambient_net_handle(
    op: &str,
    recv: &mut CtValue,
    args: &mut [CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if matches!(
        op,
        "TcpListenerAccept"
            | "TcpListenerLocalAddr"
            | "TcpStreamRead"
            | "TcpStreamWrite"
            | "TcpStreamPeerAddr"
            | "TcpStreamLocalAddr"
            | "TcpStreamClose"
            | "TcpStreamReadBytes"
            | "TcpStreamReadBytesIO"
            | "TcpStreamReadText"
            | "TcpStreamWriteBytes"
            | "TcpStreamWriteBytesIO"
            | "TcpStreamWriteAllBytes"
            | "TcpStreamWriteAllBytesIO"
            | "TcpStreamWriteText"
            | "TcpStreamShutdown"
            | "TcpStreamReady"
    ) {
        if op == "TcpListenerAccept" || op == "TcpListenerLocalAddr" {
            let Some(listener) = http_handle_id(recv, "TcpListener") else {
                return Some(Err(unsupported("TcpListener receiver", span)));
            };
            return Some(match op {
                "TcpListenerAccept" => {
                    let deadline = match args {
                        [] => Ok(None),
                        [deadline] => duration_ns(deadline)
                            .map(Some)
                            .ok_or_else(|| unsupported("TcpListener.accept deadline", span)),
                        _ => Err(unsupported("TcpListener.accept arguments", span)),
                    };
                    deadline.map(|deadline| {
                        crate::net_http_rt::runtime_tcp_listener_accept(listener, deadline)
                    })
                }
                "TcpListenerLocalAddr" => {
                    if !args.is_empty() {
                        Err(unsupported("TcpListener.local_addr arguments", span))
                    } else {
                        Ok(crate::net_http_rt::runtime_tcp_listener_local_addr(listener))
                    }
                }
                _ => unreachable!(),
            });
        }
        let Some(stream) = http_handle_id(recv, "TcpStream") else {
            return Some(Err(unsupported("TcpStream receiver", span)));
        };
        return Some(match op {
            "TcpStreamRead" => {
                if !args.is_empty() {
                    Err(unsupported("TcpStream.read arguments", span))
                } else {
                    Ok(crate::net_http_rt::runtime_tcp_stream_read(stream))
                }
            }
            "TcpStreamWrite" => {
                let Some(CtValue::Str(data)) = args.first() else {
                    return Some(Err(unsupported("TcpStream.write data", span)));
                };
                if args.len() != 1 {
                    Err(unsupported("TcpStream.write arguments", span))
                } else {
                    Ok(crate::net_http_rt::runtime_tcp_stream_write(stream, data.clone()))
                }
            }
            "TcpStreamPeerAddr" => {
                if !args.is_empty() {
                    Err(unsupported("TcpStream.peer_addr arguments", span))
                } else {
                    Ok(crate::net_http_rt::runtime_tcp_stream_peer_addr(stream))
                }
            }
            "TcpStreamLocalAddr" => {
                if !args.is_empty() {
                    Err(unsupported("TcpStream.local_addr arguments", span))
                } else {
                    Ok(crate::net_http_rt::runtime_tcp_stream_local_addr(stream))
                }
            }
            "TcpStreamClose" => {
                if !args.is_empty() {
                    Err(unsupported("TcpStream.close arguments", span))
                } else {
                    Ok(crate::net_http_rt::runtime_tcp_stream_close(stream))
                }
            }
            "TcpStreamReadBytes" | "TcpStreamReadBytesIO" => {
                let Some(CtValue::Int(limit)) = args.first() else {
                    return Some(Err(unsupported("TcpStream.read limit", span)));
                };
                let deadline = match args.get(1) {
                    None => Ok(None),
                    Some(value) => duration_ns(value)
                        .map(Some)
                        .ok_or_else(|| unsupported("TcpStream.read deadline", span)),
                };
                deadline.map(|deadline| {
                    if op == "TcpStreamReadBytesIO" {
                        crate::net_http_rt::runtime_tcp_stream_read_io(stream, *limit)
                    } else {
                        crate::net_http_rt::runtime_tcp_stream_read_bytes(stream, *limit, deadline)
                    }
                })
            }
            "TcpStreamReadText" => {
                let Some(CtValue::Int(limit)) = args.first() else {
                    return Some(Err(unsupported("TcpStream.read_text limit", span)));
                };
                let deadline = match args.get(1) {
                    None => Ok(None),
                    Some(value) => duration_ns(value)
                        .map(Some)
                        .ok_or_else(|| unsupported("TcpStream.read_text deadline", span)),
                };
                deadline.map(|deadline| {
                    crate::net_http_rt::runtime_tcp_stream_read_text(stream, *limit, deadline)
                })
            }
            "TcpStreamWriteBytes" | "TcpStreamWriteBytesIO" => {
                let Some(data) = args.first().and_then(net_bytes_value) else {
                    return Some(Err(unsupported("TcpStream.write bytes", span)));
                };
                let deadline = match args.get(1) {
                    None => Ok(None),
                    Some(value) => duration_ns(value)
                        .map(Some)
                        .ok_or_else(|| unsupported("TcpStream.write deadline", span)),
                };
                deadline.map(|deadline| {
                    if op == "TcpStreamWriteBytesIO" {
                        crate::net_http_rt::runtime_tcp_stream_write_io(stream, data)
                    } else {
                        crate::net_http_rt::runtime_tcp_stream_write_bytes(stream, data, deadline)
                    }
                })
            }
            "TcpStreamWriteAllBytes" | "TcpStreamWriteAllBytesIO" => {
                let Some(data) = args.first().and_then(net_bytes_value) else {
                    return Some(Err(unsupported("TcpStream.write_all bytes", span)));
                };
                let deadline = match args.get(1) {
                    None => Ok(None),
                    Some(value) => duration_ns(value)
                        .map(Some)
                        .ok_or_else(|| unsupported("TcpStream.write_all deadline", span)),
                };
                deadline.map(|deadline| {
                    if op == "TcpStreamWriteAllBytesIO" {
                        crate::net_http_rt::runtime_tcp_stream_write_all_io(stream, data)
                    } else {
                        crate::net_http_rt::runtime_tcp_stream_write_all_bytes(
                            stream, data, deadline,
                        )
                    }
                })
            }
            "TcpStreamWriteText" => {
                let Some(CtValue::Str(data)) = args.first() else {
                    return Some(Err(unsupported("TcpStream.write_text data", span)));
                };
                let deadline = match args.get(1) {
                    None => Ok(None),
                    Some(value) => duration_ns(value)
                        .map(Some)
                        .ok_or_else(|| unsupported("TcpStream.write_text deadline", span)),
                };
                deadline.map(|deadline| {
                    crate::net_http_rt::runtime_tcp_stream_write_text(stream, data.clone(), deadline)
                })
            }
            "TcpStreamShutdown" => {
                let Some(how) = args.first().and_then(net_shutdown_value) else {
                    return Some(Err(unsupported("TcpStream.shutdown mode", span)));
                };
                if args.len() != 1 {
                    Err(unsupported("TcpStream.shutdown arguments", span))
                } else {
                    Ok(crate::net_http_rt::runtime_tcp_stream_shutdown(stream, how))
                }
            }
            "TcpStreamReady" => {
                if args.len() != 2 {
                    Err(unsupported("TcpStream.ready arguments", span))
                } else {
                    let Some(interest) = net_ready_interest_value(&args[0]) else {
                        return Some(Err(unsupported("TcpStream.ready interest", span)));
                    };
                    let Some(deadline) = duration_ns(&args[1]) else {
                        return Some(Err(unsupported("TcpStream.ready deadline", span)));
                    };
                    Ok(crate::net_http_rt::runtime_tcp_stream_ready(
                        stream, interest, deadline,
                    ))
                }
            }
            _ => unreachable!(),
        });
    }
    if !matches!(
        op,
        "UdpSocketReady"
            | "UdpSocketReceiveDeadline"
            | "UdpSocketSendToDeadline"
            | "UdpSocketClose"
    ) {
        return None;
    }
    let Some(socket) = http_handle_id(recv, "UdpSocket") else {
        return Some(Err(unsupported("UdpSocket receiver", span)));
    };
    match op {
        "UdpSocketReady" => {
            if args.len() != 2 {
                return Some(Err(unsupported("UdpSocket.ready arguments", span)));
            }
            let Some(interest) = net_ready_interest_value(&args[0]) else {
                return Some(Err(unsupported("UdpSocket.ready interest", span)));
            };
            let Some(deadline) = duration_ns(&args[1]) else {
                return Some(Err(unsupported("UdpSocket.ready deadline", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_udp_ready(
                socket, interest, deadline,
            )))
        }
        "UdpSocketReceiveDeadline" => {
            if args.len() != 2 {
                return Some(Err(unsupported("UdpSocket.receive arguments", span)));
            }
            let Some(CtValue::Int(limit)) = args.first() else {
                return Some(Err(unsupported("UdpSocket.receive limit", span)));
            };
            let Some(deadline) = duration_ns(&args[1]) else {
                return Some(Err(unsupported("UdpSocket.receive deadline", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_udp_receive_deadline(
                socket, *limit, deadline,
            )))
        }
        "UdpSocketSendToDeadline" => {
            if args.len() != 3 {
                return Some(Err(unsupported("UdpSocket.send_to arguments", span)));
            }
            let Some(data) = net_bytes_value(&args[0]) else {
                return Some(Err(unsupported("UdpSocket.send_to bytes", span)));
            };
            let Some(addr) = http_handle_id(&args[1], "SocketAddr") else {
                return Some(Err(unsupported("UdpSocket.send_to address", span)));
            };
            let Some(deadline) = duration_ns(&args[2]) else {
                return Some(Err(unsupported("UdpSocket.send_to deadline", span)));
            };
            Some(Ok(crate::net_http_rt::runtime_udp_send_to_deadline(
                socket, data, addr, deadline,
            )))
        }
        "UdpSocketClose" => {
            if !args.is_empty() {
                return Some(Err(unsupported("UdpSocket.close arguments", span)));
            }
            Some(Ok(crate::net_http_rt::runtime_udp_close(socket)))
        }
        _ => None,
    }
}

fn ct_string_list(v: &CtValue) -> Option<Vec<String>> {
    match v {
        CtValue::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    CtValue::Str(s) => out.push(s.clone()),
                    _ => return None,
                }
            }
            Some(out)
        }
        _ => None,
    }
}

fn ct_cors_origins(v: &CtValue) -> Option<(bool, Vec<String>)> {
    match v {
        CtValue::List(_) => Some((false, ct_string_list(v)?)),
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if type_name == "HTTPCorsOrigins" => match (variant.as_str(), args.as_slice()) {
            ("Any", _) => Some((true, Vec::new())),
            ("List", [(_, list)]) => Some((false, ct_string_list(list)?)),
            _ => None,
        },
        _ => None,
    }
}

fn ambient_http_server_call(
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    match method {
        "mux" if args.is_empty() => Ok(http_handle_value(
            "HTTPMux",
            crate::net_http_rt::runtime_http_mux(),
        )),
        "json" => {
            let status = match args.first() {
                Some(CtValue::Int(n)) => *n,
                _ => return Err(unsupported("core.http.server.json status", span)),
            };
            let body = match args.get(1) {
                Some(CtValue::Str(s)) => s.clone(),
                _ => {
                    return Err(unsupported(
                        "core.http.server.json body (ambient expects JSON text)",
                        span,
                    ))
                }
            };
            Ok(http_handle_value(
                "HTTPResponse",
                crate::net_http_rt::runtime_json_response(status, body),
            ))
        }
        "static_files" => {
            let mux = args
                .first()
                .ok_or_else(|| unsupported("core.http.server.static_files mux", span))?;
            let prefix = match args.get(1) {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Err(unsupported("core.http.server.static_files prefix", span)),
            };
            let root = match args.get(2) {
                Some(CtValue::Str(s)) => s.clone(),
                _ => return Err(unsupported("core.http.server.static_files root", span)),
            };
            let bool_option = |index: usize| match args.get(index) {
                Some(CtValue::Bool(value)) => Ok(Some(*value)),
                None => Ok(None),
                _ => Err(unsupported("core.http.server.static_files option", span)),
            };
            let mux_h = http_handle_id(mux, "HTTPMux")
                .ok_or_else(|| unsupported("core.http.server.static_files mux handle", span))?;
            crate::net_http_rt::runtime_static_files(
                mux_h,
                prefix,
                root,
                bool_option(3)?,
                bool_option(4)?,
                bool_option(5)?,
            )
            .map_err(|e| unsupported(&e, span))?;
            Ok(CtValue::Unit)
        }
        "cors_policy" => {
            let origins = args
                .first()
                .ok_or_else(|| unsupported("core.http.server.cors_policy origins", span))?;
            let (origins_any, origin_list) = ct_cors_origins(origins)
                .ok_or_else(|| unsupported("core.http.server.cors_policy origins form", span))?;
            let list_option = |index: usize| match args.get(index) {
                Some(value) => ct_string_list(value)
                    .map(Some)
                    .ok_or_else(|| unsupported("core.http.server.cors_policy list", span)),
                None => Ok(None),
            };
            let credentials = match args.get(3) {
                Some(CtValue::Bool(value)) => Some(*value),
                None => None,
                _ => return Err(unsupported("core.http.server.cors_policy credentials", span)),
            };
            let max_age = match args.get(4) {
                Some(CtValue::Int(value)) => Some(*value),
                None => None,
                _ => return Err(unsupported("core.http.server.cors_policy max_age", span)),
            };
            match crate::net_http_rt::runtime_cors_policy(
                origins_any,
                origin_list,
                list_option(1)?,
                list_option(2)?,
                credentials,
                max_age,
            ) {
                Ok(h) => Ok(CtValue::Present(Box::new(http_handle_value(
                    "HTTPCorsPolicy",
                    h,
                )))),
                Err(error) => Ok(CtValue::failed(Box::new(error.value))),
            }
        }
        "cors" => {
            let mux = args
                .first()
                .ok_or_else(|| unsupported("core.http.server.cors mux", span))?;
            let policy = args
                .get(1)
                .ok_or_else(|| unsupported("core.http.server.cors policy", span))?;
            let mux_h = http_handle_id(mux, "HTTPMux")
                .ok_or_else(|| unsupported("core.http.server.cors mux handle", span))?;
            let policy_h = http_handle_id(policy, "HTTPCorsPolicy")
                .ok_or_else(|| unsupported("core.http.server.cors policy handle", span))?;
            crate::net_http_rt::runtime_cors(mux_h, policy_h).map_err(|e| unsupported(&e, span))?;
            Ok(CtValue::Unit)
        }
        other => Err(unsupported(
            &format!("core.http.server.{other} ambient"),
            span,
        )),
    }
}

fn ambient_http_handle(
    op: &str,
    recv: &mut CtValue,
    args: &mut [CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if let Some(static_call) = op.strip_prefix("HTTPStatic:") {
        let Some((path, method)) = static_call.rsplit_once(':') else {
            return Some(Err(unsupported("HTTP nominal static adapter", span)));
        };
        return Some(crate::net_http_rt::runtime_http_nominal_static(path, method, args)
            .map_err(|error| unsupported(&error, span)));
    }
    if op == "HTTPNominalShow" {
        let handle = match recv {
            CtValue::Struct { type_name, fields }
                if matches!(
                    type_name.as_str(),
                    "HTTPMethod"
                        | "HTTPStatus"
                        | "HTTPVersion"
                        | "HTTPHeaderName"
                        | "HTTPHeaderValue"
                ) => fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
                    ("handle", CtValue::Int(handle)) if *handle > 0 => Some(*handle),
                    _ => None,
                }),
            _ => None,
        };
        let Some(handle) = handle else {
            return Some(Err(unsupported("HTTP nominal show receiver", span)));
        };
        return Some(
            crate::net_http_rt::runtime_http_nominal_show(handle)
                .map(CtValue::Str)
                .map_err(|error| unsupported(&error, span)),
        );
    }
    if op == "HTTPJSONDecodeError" {
        return Some(Ok(CtValue::failed(Box::new(
            crate::net_http_rt::runtime_http_json_decode_error(),
        ))));
    }
    if !(op.starts_with("HTTPClient:") || op.starts_with("HTTPServer:")) {
        return None;
    }
    let result = match op {
        "HTTPServer:HTTPRequest:json" if args.is_empty() => {
            let request = http_handle_id(recv, "HTTPRequest")
                .ok_or_else(|| unsupported("HTTPRequest.json receiver", span));
            request.and_then(|request| {
                let body = crate::net_http_rt::runtime_http_req_body(request)
                    .map_err(|error| unsupported(&error, span))?;
                let result = crate::net_http_rt::runtime_http_body_json_text(body, None)
                    .map_err(|error| unsupported(&error, span))?;
                Ok(http_json_text_result(result))
            })
        }
        "HTTPClient:HTTPResponse:json" if args.len() <= 1 => {
            let response = http_handle_id(recv, "HTTPResponse")
                .ok_or_else(|| unsupported("HTTPResponse.json receiver", span));
            response.and_then(|response| {
                let body = crate::net_http_rt::runtime_http_resp_body(response)
                    .map_err(|error| unsupported(&error, span))?;
                let limit = match args.first() {
                    Some(CtValue::Int(limit)) => Some(*limit),
                    None => None,
                    _ => return Err(unsupported("HTTPResponse.json limit", span)),
                };
                let result = crate::net_http_rt::runtime_http_body_json_text(body, limit)
                    .map_err(|error| unsupported(&error, span))?;
                Ok(http_json_text_result(result))
            })
        }
        "HTTPClient:HTTPBody:json" | "HTTPServer:HTTPBody:json" if args.len() == 1 => {
            let body = http_handle_id(recv, "HTTPBody")
                .ok_or_else(|| unsupported("HTTPBody.json receiver", span));
            body.and_then(|body| {
                let Some(CtValue::Int(limit)) = args.first() else {
                    return Err(unsupported("HTTPBody.json limit", span));
                };
                let result =
                    crate::net_http_rt::runtime_http_body_json_text(body, Some(*limit))
                        .map_err(|error| unsupported(&error, span))?;
                Ok(http_json_text_result(result))
            })
        }
        "HTTPClient:HTTPBody:bytes" | "HTTPServer:HTTPBody:bytes" if args.len() == 1 => {
            let body = http_handle_id(recv, "HTTPBody")
                .ok_or_else(|| unsupported("HTTPBody.bytes receiver", span));
            body.and_then(|body| {
                let Some(CtValue::Int(limit)) = args.first() else {
                    return Err(unsupported("HTTPBody.bytes limit", span));
                };
                let result = crate::net_http_rt::runtime_http_body_bytes(body, *limit)
                    .map_err(|error| unsupported(&error, span))?;
                Ok(match result {
                    Ok(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
                    Err(error) => CtValue::failed(Box::new(error)),
                })
            })
        }
        "HTTPClient:HTTPBody:text" | "HTTPServer:HTTPBody:text" if args.len() == 1 => {
            let body = http_handle_id(recv, "HTTPBody")
                .ok_or_else(|| unsupported("HTTPBody.text receiver", span));
            body.and_then(|body| {
                let Some(CtValue::Int(limit)) = args.first() else {
                    return Err(unsupported("HTTPBody.text limit", span));
                };
                let result = crate::net_http_rt::runtime_http_body_text(body, *limit)
                    .map_err(|error| unsupported(&error, span))?;
                Ok(match result {
                    Ok(text) => CtValue::Present(Box::new(CtValue::Str(text))),
                    Err(error) => CtValue::failed(Box::new(error)),
                })
            })
        }
        "HTTPClient:HTTPBody:copy_to" | "HTTPServer:HTTPBody:copy_to" if args.len() == 2 => {
            let body = http_handle_id(recv, "HTTPBody")
                .ok_or_else(|| unsupported("HTTPBody.copy_to receiver", span));
            body.and_then(|body| {
                let Some(CtValue::Int(writer)) = args.first() else {
                    return Err(unsupported("HTTPBody.copy_to writer", span));
                };
                let Some(CtValue::Int(limit)) = args.get(1) else {
                    return Err(unsupported("HTTPBody.copy_to limit", span));
                };
                let writer = crate::enc_stream::take_file_writer_for_http(*writer)
                    .map_err(|error| unsupported(&format!("HTTPBody.copy_to: {error}"), span))?;
                let result = crate::net_http_rt::runtime_http_body_copy_to(body, writer, *limit)
                    .map_err(|error| unsupported(&error, span))?;
                Ok(match result {
                    Ok(bytes) => CtValue::Present(Box::new(CtValue::Int(bytes))),
                    Err(error) => CtValue::failed(Box::new(error)),
                })
            })
        }
        "HTTPClient:HTTPRequest:body" if args.len() == 1 => {
            let request = http_handle_id(recv, "HTTPRequest")
                .ok_or_else(|| unsupported("HTTPRequest.body receiver", span));
            request.and_then(|request| {
                let Some(CtValue::Str(body)) = args.first() else {
                    return Err(unsupported("HTTPRequest.body text", span));
                };
                let handle = crate::net_http_rt::runtime_http_request_body(request, body.clone())
                    .map_err(|error| unsupported(&error, span))?;
                Ok(http_handle_value("HTTPRequest", handle))
            })
        }
        _ => Err(unsupported(&format!("HTTP ambient handle `{op}`"), span)),
    };
    Some(result)
}

fn http_json_text_result(result: Result<String, CtValue>) -> CtValue {
    match result {
        Ok(text) => CtValue::Present(Box::new(CtValue::Str(text))),
        Err(error) => CtValue::failed(Box::new(error)),
    }
}
