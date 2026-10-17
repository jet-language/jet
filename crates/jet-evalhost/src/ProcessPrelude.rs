pub(crate) mod process_prelude {
    use std::ffi::{OsStr, OsString};

    #[cfg(unix)]
    use jet_codegen::scheduler::{
        jet_scheduler_raw_io_handle, jet_scheduler_raw_io_set_nonblocking,
        jet_scheduler_raw_io_write_wait,
    };
    use jet_codegen::interrupt_runtime::{
        jet_process_signal_error, jet_process_signal_register,
    };
    use jet_codegen::scheduler::{
        jet_scheduler_current_task_control, jet_scheduler_root_task_control,
        jet_scheduler_shielded, jet_scheduler_wait_without_unwind, jet_task_deliver_cancel,
        JetSchedulerWait,
    };
    use jet_foundation::Outcome::{jet_outcome_of, JetAbsent, JetOutcome};

    mod terminal_default {
        include!("../../jet-codegen/src/Prelude/TerminalDefault.rs");
    }

    mod jet_process_pty {
        pub use jet_codegen::process_pty::*;
    }

    include!("../../jet-codegen/src/Prelude/Core/FileHandleOwners.rs");

    pub(crate) mod jet_std {
        use super::{JetAbsent, JetOutcome};
        pub(crate) use super::JetProcessDirectory;

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

        #[derive(Clone, Copy, Debug, PartialEq)]
        pub enum ProcessResourceLimit {
            WallTime,
            CpuTime,
            Memory,
            OpenFiles,
            Output,
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
            ResourceLimit(ProcessResourceLimit),
            AlreadyExists(IOContext),
            Unsupported(IOContext),
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

        pub fn io_error_at(operation: IOOperation, path: &str, error: std::io::Error) -> IOError {
            let context = IOContext::new(
                operation,
                Some(path.to_string()),
                error.raw_os_error().map(i64::from),
                Some(error.to_string()),
            );
            let not_supported = matches!(
                context.os_code,
                Ok(code) if (cfg!(target_os = "linux") && code == 95)
                    || (cfg!(target_os = "macos") && (code == 45 || code == 102))
            );
            if not_supported {
                return IOError::Unsupported(context);
            }
            match error.kind() {
                std::io::ErrorKind::AlreadyExists => IOError::AlreadyExists(context),
                std::io::ErrorKind::Unsupported => IOError::Unsupported(context),
                std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => {
                    IOError::InvalidInput(context)
                }
                std::io::ErrorKind::NotFound => IOError::NotFound(context),
                std::io::ErrorKind::PermissionDenied => IOError::PermissionDenied(context),
                std::io::ErrorKind::TimedOut => IOError::TimedOut(context),
                std::io::ErrorKind::AlreadyExists => IOError::AlreadyExists(context),
                std::io::ErrorKind::Unsupported => IOError::Unsupported(context),
                std::io::ErrorKind::NotConnected | std::io::ErrorKind::BrokenPipe => {
                    IOError::Closed(context)
                }
                _ => IOError::Other(context),
            }
        }

        // #3593: the same held-directory and *at(2) leaves AOT embeds.
        include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/FileHandles.rs");

        #[derive(Clone, Debug, PartialEq)]
        pub struct DirEntry {
            pub name: String,
            pub path: String,
            pub is_dir: bool,
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
                    Self::NonUnicode => {
                        "environment contains a name or value that is not valid Unicode".to_string()
                    }
                }
            }
        }

        #[derive(Clone, Debug, PartialEq)]
        pub struct ProcessReceipt {
            // Keep exact Jet `Int` fields aligned with the AOT Prelude type.
            pub code: jet_foundation::Numeric::JetInt,
            pub output: String,
            pub errors: String,
            pub success: bool,
            // Mirrors the Prelude declaration (JetStd/Open.rs): the one
            // optional carrier, never a raw Rust `Option`.
            pub signal: JetOutcome<jet_foundation::Numeric::JetInt, JetAbsent>,
            pub timed_out: bool,
            pub executable_identity: String,
            pub argv: Vec<String>,
            pub input_digest: String,
            pub policy_digest: String,
            pub backend: String,
            pub authority: Vec<String>,
            pub descendants: String,
            pub limits: Vec<String>,
            pub outputs: Vec<String>,
            pub redacted: bool,
            pub pid: jet_foundation::Numeric::JetInt,
            pub limit_hit: JetOutcome<ProcessResourceLimit, JetAbsent>,
        }

        pub type ProcessResult = ProcessReceipt;

        #[derive(Clone, Debug, PartialEq)]
        pub enum ProcessStreamMode {
            Stream,
            Inherit,
            Capture,
        }
        // D-FOUND-LIFECYCLE1=A: mirror the shared ProcessSignal value and its
        // mask so the resident host only marshals the checked enum.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum ProcessSignal {
            Term,
            Hup,
            Int,
        }

        impl ProcessSignal {
            pub const fn mask(self) -> usize {
                match self {
                    Self::Int => 1 << 0,
                    Self::Hup => 1 << 1,
                    Self::Term => 1 << 2,
                }
            }
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

        #[cfg(windows)]
        #[derive(Debug)]
        pub(crate) struct ConPtyControl {
            handle: std::cell::Cell<usize>,
        }

        #[cfg(windows)]
        #[link(name = "kernel32")]
        unsafe extern "system" {
            #[link_name = "ClosePseudoConsole"]
            fn close_pseudo_console(handle: *mut std::ffi::c_void);
        }

        #[cfg(windows)]
        impl ConPtyControl {
            pub(crate) fn new(handle: usize) -> Self {
                Self {
                    handle: std::cell::Cell::new(handle),
                }
            }

            pub(crate) fn raw(&self) -> usize {
                self.handle.get()
            }

            pub(crate) fn close(&self) {
                let handle = self.handle.replace(0);
                if handle != 0 {
                    // SAFETY: this control owns the one live HPCON; replacing
                    // the handle with zero makes close idempotent across wait
                    // and drop.
                    unsafe { close_pseudo_console(handle as *mut std::ffi::c_void) };
                }
            }
        }

        #[cfg(windows)]
        impl Drop for ConPtyControl {
            fn drop(&mut self) {
                self.close();
            }
        }

        #[derive(Clone, Debug)]
        pub struct TerminalSession {
            #[cfg(unix)]
            pub master: std::rc::Rc<std::fs::File>,
            #[cfg(windows)]
            pub control: std::rc::Rc<ConPtyControl>,
        }

        impl PartialEq for TerminalSession {
            fn eq(&self, other: &Self) -> bool {
                #[cfg(unix)]
                {
                    std::rc::Rc::ptr_eq(&self.master, &other.master)
                }
                #[cfg(windows)]
                {
                    std::rc::Rc::ptr_eq(&self.control, &other.control)
                }
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
            Shared(std::sync::Arc<ProcessOutputState>),
        }

        #[derive(Debug)]
        pub(crate) struct ProcessOutputState {
            pub(crate) bytes: std::sync::Mutex<ProcessOutputBuffer>,
            pub(crate) ready: std::sync::Condvar,
        }

        #[derive(Debug)]
        pub(crate) struct ProcessOutputBuffer {
            pub(crate) bytes: Vec<u8>,
            pub(crate) cursor: usize,
            pub(crate) closed: bool,
            pub(crate) error: Option<(std::io::ErrorKind, String)>,
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
            pub(crate) inherited_directories: Vec<JetProcessDirectory>,
            pub env_clear: bool,
            pub env_set: Vec<(String, String)>,
            pub env_remove: Vec<String>,
            pub stdin: Option<ProcessStreamMode>,
            pub stdout: ProcessStreamMode,
            pub stderr: ProcessStreamMode,
            pub timeout_ms: Option<i64>,
            pub output_limit: Option<i64>,
            pub cpu_time_limit_ms: Option<i64>,
            pub memory_limit_bytes: Option<i64>,
            pub open_file_limit: Option<i64>,
            pub detached: bool,
            pub terminal: Option<TerminalPolicy>,
            pub policy_wire: Option<String>,
        }

        #[derive(Clone, Debug, PartialEq)]
        pub struct ProcessPlan {
            pub executable_identity: String,
            pub argv: Vec<String>,
            pub input_digest: String,
            pub policy_digest: String,
            pub backend: String,
            pub authority: Vec<String>,
            pub descendants: String,
            pub limits: Vec<String>,
            pub outputs: Vec<String>,
        }

        #[derive(Debug)]
        pub(crate) enum ProcessHandle {
            Std {
                child: std::process::Child,
                job: Option<std::rc::Rc<std::fs::File>>,
            },
            #[cfg(windows)]
            Native {
                process: std::fs::File,
                job: std::rc::Rc<std::fs::File>,
                pid: u32,
            },
        }

        #[derive(Clone, Debug)]
        pub struct ProcessChild {
            pub inner: std::rc::Rc<std::cell::RefCell<Option<ProcessHandle>>>,
            pub wait_result: std::rc::Rc<std::cell::RefCell<Option<ProcessResult>>>,
            // Keep cancellation/drop cleanup failures visible through the
            // shared Prelude wait path.
            pub cleanup_error: std::rc::Rc<std::cell::RefCell<Option<IOError>>>,
            pub stdin: std::rc::Rc<std::cell::RefCell<Option<ProcessStdin>>>,
            pub stdout: std::rc::Rc<std::cell::RefCell<Option<std::io::BufReader<ProcessReader>>>>,
            pub stderr: std::rc::Rc<std::cell::RefCell<Option<std::io::BufReader<ProcessReader>>>>,
            pub stdout_state: Option<std::sync::Arc<ProcessOutputState>>,
            pub stderr_state: Option<std::sync::Arc<ProcessOutputState>>,
            pub stdout_worker: std::rc::Rc<
                std::cell::RefCell<Option<std::thread::JoinHandle<std::io::Result<()>>>>,
            >,
            pub stderr_worker: std::rc::Rc<
                std::cell::RefCell<Option<std::thread::JoinHandle<std::io::Result<()>>>>,
            >,
            pub output_limit_hit: std::sync::Arc<std::sync::atomic::AtomicBool>,
            pub output_read_error: std::sync::Arc<std::sync::atomic::AtomicBool>,
            pub terminal: JetOutcome<TerminalSession, JetAbsent>,
            pub process_group: bool,
            pub detached: bool,
            pub timeout_ms: Option<i64>,
            pub output_limit: Option<i64>,
            pub audit_spec: std::rc::Rc<ProcessSpec>,
            pub audit_plan: Option<std::rc::Rc<ProcessPlan>>,
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

    /// The one logical environment table (D-ENV-MUTATE1) the resident host
    /// owns in `CoreHost`; child processes read it under the same guard the
    /// AOT `EnvInit` prelude holds through `Command::envs`.
    fn jet_env_read() -> std::sync::RwLockReadGuard<'static, JetEnvEntries> {
        crate::CoreHost::jit_env_read()
    }

    fn jet_env_key_eq(left: &OsStr, right: &OsStr) -> bool {
        crate::CoreHost::jit_env_key_eq(left, right)
    }

    fn jet_env_validate_name(name: &str) -> Result<(), jet_std::EnvError> {
        crate::CoreHost::jit_env_validate_name(name).map_err(|_| jet_std::EnvError::InvalidName)
    }

    fn jet_env_validate_value(value: &str) -> Result<(), jet_std::EnvError> {
        crate::CoreHost::jit_env_validate_value(value).map_err(|_| jet_std::EnvError::InvalidValue)
    }

    fn jet_scheduler_park_ms(wait_kind: &'static str, millis: u64) {
        jet_codegen::scheduler::jet_scheduler_park_ms(wait_kind, millis);
    }

    include!("../../jet-codegen/src/Prelude/CoreLib/Top/SHA256Raw.rs");
    mod jet_process_sandbox {
        include!("../../jet-codegen/src/Prelude/CoreLib/Top/ProcessSandbox.rs");
        include!("../../jet-codegen/src/Prelude/CoreLib/Top/ProcessWindowsSandbox.rs");
    }
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/ProcessPolicy.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/ProcessSpec.rs");
    // D-DX-DEVTOOLS1 / #2454: the process runtime publishes typed topology
    // facts; AOT emits this producer-only panel source beside Process.rs
    // (`push_runtime_devtools_panel_preludes`), so the resident tier includes
    // the same file.
    use jet_foundation::Devtools::*;
    include!("../../jet-codegen/src/Prelude/Core/DevtoolsTopologyPanel.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/Process.rs");

    pub(crate) use jet_std::{
        Duration, IOContext, IOError, IOOperation, ProcessChild, ProcessPlan, ProcessReader,
        ProcessReceipt, ProcessResourceLimit, ProcessSpec, ProcessStreamMode,
        TerminalMode, TerminalPolicy, TerminalSession, TerminalSize,
    };

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn spec_fields(
        cmd: Vec<String>, has_cwd: bool, cwd: String, env_clear: bool,
        env_names: Vec<String>, env_values: Vec<String>, env_remove: Vec<String>,
        has_stdin: bool, stdin: jet_std::ProcessStreamMode, stdout: jet_std::ProcessStreamMode,
        stderr: jet_std::ProcessStreamMode, has_timeout: bool, timeout_ms: i64,
        has_output_limit: bool, output_limit: i64,
        has_cpu_limit: bool, cpu_time_limit_ms: i64,
        has_memory_limit: bool, memory_limit_bytes: i64,
        has_open_limit: bool, open_file_limit: i64,
        detached: bool, has_terminal: bool, terminal: jet_std::TerminalPolicy,
        has_policy: bool, policy_wire: String,
    ) -> ProcessSpec {
        jet_process_spec_fields_raw(cmd, has_cwd, cwd, env_clear, env_names, env_values, env_remove,
            has_stdin, stdin, stdout, stderr, has_timeout, timeout_ms, has_output_limit, output_limit,
            has_cpu_limit, cpu_time_limit_ms, has_memory_limit, memory_limit_bytes,
            has_open_limit, open_file_limit, detached, has_terminal, terminal, has_policy, policy_wire)
    }
    pub(crate) fn spec_with_directory_raw(
        spec: ProcessSpec, source_fd: i64, child_fd: i64, as_cwd: bool,
    ) -> (ProcessSpec, bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
        jet_process_spec_with_directory_raw(spec, source_fd, child_fd, as_cwd)
    }
    pub(crate) fn spec_arg(spec: ProcessSpec, extra: &String) -> ProcessSpec {
        jet_process_spec_arg(spec, extra)
    }

    pub(crate) fn spec_args_extend(spec: ProcessSpec, extra: &Vec<String>) -> ProcessSpec {
        jet_process_spec_args_extend(spec, extra)
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
        jet_process_spec_output_limit(
            spec,
            jet_foundation::Numeric::JetInt::from_i64(output_limit),
        )
    }

    pub(crate) fn spec_cpu_time_limit(spec: ProcessSpec, timeout: &Duration) -> ProcessSpec {
        jet_process_spec_cpu_time_limit(spec, timeout)
    }

    pub(crate) fn spec_memory_limit(spec: ProcessSpec, limit: i64) -> ProcessSpec {
        jet_process_spec_memory_limit(
            spec,
            jet_foundation::Numeric::JetInt::from_i64(limit),
        )
    }

    pub(crate) fn spec_open_file_limit(spec: ProcessSpec, limit: i64) -> ProcessSpec {
        jet_process_spec_open_file_limit(
            spec,
            jet_foundation::Numeric::JetInt::from_i64(limit),
        )
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

    pub(crate) fn spec_abilities(spec: &ProcessSpec) -> std::collections::HashSet<String> {
        jet_process_spec_abilities(spec)
    }

    pub(crate) fn spec_under_wire(spec: ProcessSpec, authority_wire: &String) -> ProcessSpec {
        jet_process_spec_under_wire(spec, authority_wire)
    }

    pub(crate) fn spec_plan(spec: &ProcessSpec) -> Result<ProcessPlan, IOError> {
        jet_process_spec_plan(spec)
    }

    pub(crate) fn spec_run(spec: &ProcessSpec) -> Result<ProcessReceipt, IOError> {
        jet_process_spec_run(spec)
    }

    pub(crate) fn spec_run_checked(spec: &ProcessSpec) -> Result<ProcessReceipt, IOError> {
        jet_process_spec_run_checked(spec)
    }

    pub(crate) fn spec_pipeline(specs: &Vec<ProcessSpec>) -> Result<ProcessReceipt, IOError> {
        jet_process_spec_pipeline(specs)
    }

    pub(crate) fn spec_spawn(spec: &ProcessSpec) -> Result<ProcessChild, IOError> {
        jet_process_spec_spawn(spec)
    }

    pub(crate) fn child_id(child: &ProcessChild) -> i64 {
        jet_process_child_id(child)
    }

    pub(crate) fn child_wait(child: &ProcessChild) -> Result<ProcessReceipt, IOError> {
        jet_process_child_wait(child)
    }
    pub(crate) fn child_close(child: &ProcessChild) {
        jet_process_child_close(child)
    }

    pub(crate) fn child_stdin_write(child: &ProcessChild, text: &String) -> Result<(), IOError> {
        jet_process_stdin_write(&child.stdin, text)
    }
    pub(crate) fn child_stdin_close(child: &ProcessChild) {
        jet_process_stdin_close(&child.stdin)
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

    /// Send-safe owner for a process output reader.  The ordinary process
    /// Prelude keeps an `Rc<RefCell<...>>` because a child binding is
    /// thread-affine; Source loop cursors need the same reader lease behind a
    /// scheduler-safe mutex once the checked owner has been adopted.
    pub(crate) type SourceProcessReader =
        std::sync::Arc<std::sync::Mutex<Option<std::io::BufReader<ProcessReader>>>>;

    pub(crate) fn source_process_stream_next_line(
        reader: &SourceProcessReader,
    ) -> Result<Option<String>, IOError> {
        let mut reader = reader.lock().map_err(|_| {
            IOError::other(
                IOOperation::Read,
                None,
                "process output reader lock is poisoned",
            )
        })?;
        jet_process_child_read_line(&mut *reader)
    }

    pub(crate) fn terminal_session_resize(
        session: &TerminalSession,
        size: &TerminalSize,
    ) -> Result<(), IOError> {
        jet_terminal_session_resize(session, size)
    }

    #[cfg(test)]
    mod carrier_tests {
        #[test]
        fn process_directory_carrier_has_one_owner_across_the_process_prelude() {
            assert_eq!(
                std::any::TypeId::of::<super::JetProcessDirectory>(),
                std::any::TypeId::of::<super::jet_std::JetProcessDirectory>(),
            );
        }
    }

    #[cfg(all(test, target_os = "linux"))]
    mod held_directory_tests {
        use super::*;
        use std::os::fd::AsRawFd;

        #[test]
        fn process_directory_carrier_keeps_cwd_and_inherited_source_after_rename_and_close() {
            let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let root = std::env::temp_dir().join(format!("jet-held-process-{}-{nonce}", std::process::id()));
            std::fs::create_dir(&root).unwrap();
            struct Cleanup(std::path::PathBuf);
            impl Drop for Cleanup {
                fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
            }
            let _cleanup = Cleanup(root.clone());
            let destination = root.join("destination");
            let source = root.join("source");
            std::fs::create_dir(&destination).unwrap();
            std::fs::create_dir(&source).unwrap();
            std::fs::write(destination.join("marker.txt"), "held destination\n").unwrap();
            std::fs::write(source.join("source.txt"), "held source\n").unwrap();
            let destination_fd = std::fs::File::open(&destination).unwrap();
            let source_fd = std::fs::File::open(&source).unwrap();
            let spec = ProcessSpec {
                cmd: vec!["sh".into(), "-c".into(), "cat marker.txt; cat /proc/self/fd/199/source.txt".into()],
                cwd: Some("/missing-lexical-process-directory".into()),
                inherited_directories: Vec::new(),
                env_clear: false, env_set: Vec::new(), env_remove: Vec::new(), stdin: None,
                stdout: jet_std::ProcessStreamMode::Capture, stderr: jet_std::ProcessStreamMode::Capture,
                timeout_ms: None, output_limit: None, cpu_time_limit_ms: None,
                memory_limit_bytes: None, open_file_limit: None, detached: false,
                terminal: None, policy_wire: None,
            };
            let (spec, available, assigned_fd, errno) = spec_with_directory_raw(spec, destination_fd.as_raw_fd() as i64, 198, true);
            assert!(available);
            assert_eq!(errno.to_i64(), Some(0));
            assert_eq!(assigned_fd.to_i64(), Some(198));
            let (spec, available, assigned_fd, errno) = spec_with_directory_raw(spec, source_fd.as_raw_fd() as i64, 199, false);
            assert!(available);
            assert_eq!(errno.to_i64(), Some(0));
            assert_eq!(assigned_fd.to_i64(), Some(199));
            let (rejected, available, assigned_fd, errno) = spec_with_directory_raw(spec.clone(), destination_fd.as_raw_fd() as i64, 198, true);
            assert!(available);
            assert_eq!(errno.to_i64(), Some(0));
            assert_ne!(assigned_fd.to_i64(), Some(198), "an occupied descriptor must not be replaced");
            drop(rejected);
            drop(destination_fd);
            drop(source_fd);
            std::fs::rename(&destination, root.join("moved-destination")).unwrap();
            std::fs::rename(&source, root.join("moved-source")).unwrap();
            std::fs::create_dir(&destination).unwrap();
            std::fs::create_dir(&source).unwrap();
            std::fs::write(destination.join("marker.txt"), "wrong destination\n").unwrap();
            std::fs::write(source.join("source.txt"), "wrong source\n").unwrap();
            let receipt = spec_run(&spec).unwrap();
            assert!(receipt.success, "{receipt:?}");
            assert_eq!(receipt.output, "held destination\nheld source\n");
        }
    }
}

