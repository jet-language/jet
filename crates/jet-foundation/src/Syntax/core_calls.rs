//! Canonical projection rows for plain Core calls.
//!
//! D-ONCE-LAW1=A: each plain Core call states its module, member, erased
//! calling convention, fallibility authority, and one Prelude/Rust symbol
//! once here. AOT and the TIR coverage gate look the row up; they do not keep
//! second key lists. The typed Jet signature remains a sema fact because the
//! foundation crate cannot depend back on sema; the row records that
//! authority explicitly instead of copying sema's `Type` construction.

use crate::Effects::Effect;
use crate::Syntax::sinks::SinkClass;
use crate::Syntax::KNOWN_CORE_MODULES;
use crate::Syntax::{INTERNAL_RECEIPT_HANDLE, METHOD_RECEIPT_ATTACH, VAULT_KEY_REF_TYPE};

/// Package authority names for the typed UI host boundary. These are policy
/// facts, not a second effect vocabulary; host rows refer to this one list.
pub const UI_CAPABILITY_NAMES: &[&str] = &[
    "UI.FileDialog",
    "UI.Clipboard",
    "UI.IME",
    "UI.DragDrop",
    "UI.Shortcuts",
    "UI.Accessibility",
    "UI.FontShaping",
];

pub fn is_ui_capability(name: &str) -> bool {
    UI_CAPABILITY_NAMES.contains(&name)
}

const fn same_text(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

const fn one_of(method: &str, names: &[&str]) -> bool {
    let mut index = 0;
    while index < names.len() {
        if same_text(method, names[index]) {
            return true;
        }
        index += 1;
    }
    false
}

/// Effect authority for rows in this table. Calls that are not rows still use
/// `Effects::core_effect`'s legacy resolver; a row never consults that second
/// key table.
const fn effect_for(module: &str, method: &str) -> Option<Effect> {
    if (same_text(module, "core.time")
        && one_of(
            method,
            &[
                "clock",
                "time",
                "local_time",
                "parse_time",
                "new",
                "parse",
                "from_timestamp",
                "period",
                "period_days",
                "period_months",
                "period_years",
                "parse_rfc3339",
                "from_unix_ms",
                "from_unix_seconds",
                "from_unix_microseconds",
                "from_unix_nanoseconds",
                "parse_iso_week_date",
                "from_iso_week",
                "parse_zoned",
                "datetime",
                "zone",
                "utc",
                "zoned",
                "zoned_local",
                "days_in_month",
                "is_leap_year",
            ],
        ))
        || (same_text(module, "core.math.random") && same_text(method, "rng"))
    {
        return None;
    }
    if same_text(module, "core.time")
        && one_of(
            method,
            &[
                "now",
                "now_utc",
                "today",
                "instant",
                "sleep",
                "sleep_until",
                "start",
            ],
        )
    {
        return Some(Effect::Time);
    }
    if same_text(module, "core.rt") && same_text(method, "callback") {
        return Some(Effect::Time);
    }

    if (same_text(module, "core.math.random")
        && one_of(
            method,
            &[
                "int",
                "float",
                "float_range",
                "bool",
                "normal",
                "exponential",
                "pick",
                "weighted_pick",
                "sample",
                "shuffle",
                "seed",
                "split",
                "bytes",
            ],
        ))
        || (same_text(module, "core.crypto.random") && same_text(method, "bytes"))
    {
        return Some(Effect::Rand);
    }
    if (same_text(module, "core.term") && same_text(method, "style_force"))
        || (same_text(module, "core.net")
            && one_of(
                method,
                &[
                    "ip_addr",
                    "ip_to_string",
                    "ip_is_ipv4",
                    "ip",
                    "ipv4",
                    "ipv6",
                    "parse_ip",
                    "is_ipv4",
                    "is_ipv6",
                    "socket_addr_parse",
                    "socket_host",
                    "socket_port",
                    "socket_to_string",
                    "ready_readable",
                    "ready_writable",
                    "error_operation",
                    "error_address",
                    "error_name",
                    "error_message",
                    "error_os_code",
                    "dns_srv_target",
                    "dns_srv_port",
                    "dns_srv_priority",
                    "dns_srv_weight",
                    "udp_packet_data",
                    "udp_packet_addr",
                    "udp_packet_bytes",
                    "udp_packet_original_len",
                    "udp_packet_truncated",
                ],
            ))
    {
        return None;
    }
    if same_text(module, "core.watcher") {
        return if same_text(method, "files") {
            Some(Effect::FS)
        } else if same_text(method, "process_pid") {
            Some(Effect::Exec)
        } else if same_text(method, "port") {
            Some(Effect::Net)
        } else {
            None
        };
    }
    if same_text(module, "core.web.browser") {
        return if one_of(method, &["profile", "timeout"]) {
            None
        } else if same_text(method, "locked") {
            Some(Effect::FS)
        } else if same_text(method, "config_from_env") {
            Some(Effect::Env)
        } else {
            Some(Effect::Net)
        };
    }
    if ((same_text(module, "core.encoding.json")
        || same_text(module, "core.encoding.jsonl")
        || same_text(module, "core.encoding.csv")
        || same_text(module, "core.encoding.xml")
        || same_text(module, "core.encoding.cbor"))
        && one_of(method, &["reader", "writer"]))
        || (same_text(module, "core.encoding.csv") && same_text(method, "query"))
    {
        return Some(Effect::FS);
    }
    if same_text(module, "core.compute") && !same_text(method, "device_cpu") {
        return Some(Effect::GPU);
    }
    if same_text(module, "core.files") {
        return Some(Effect::FS);
    }
    if same_text(module, "core.net")
        || same_text(module, "core.net.tls")
        || same_text(module, "core.http.client")
        || same_text(module, "core.http.server")
        || same_text(module, "core.http.middleware")
        || (same_text(module, "core.http") && one_of(method, &["serve", "get"]))
    {
        return Some(Effect::Net);
    }
    if same_text(module, "core.game.raylib") {
        return Some(Effect::GPU);
    }
    if same_text(module, "core.time") {
        return Some(Effect::Time);
    }
    if same_text(module, "core.math.random") || same_text(module, "core.crypto.random") {
        return Some(Effect::Rand);
    }
    // Key generation draws from the OS generator, like crypto.random.bytes.
    if same_text(module, "core.crypto")
        && one_of(method, &["__x25519_generate", "__signing_generate"])
    {
        return Some(Effect::Rand);
    }
    if same_text(module, "core.sys") && same_text(method, "success") {
        return None;
    }
    if same_text(module, "core.sys") {
        return Some(Effect::Env);
    }
    if same_text(module, "core.process") && same_text(method, "status_ok") {
        return None;
    }
    if same_text(module, "core.process") {
        return Some(Effect::Exec);
    }
    if same_text(module, "core.term") {
        return Some(Effect::IO);
    }
    if same_text(module, "core.db") || same_text(module, "core.jobs") {
        return Some(Effect::DB);
    }
    if same_text(module, "core.auth")
        && one_of(
            method,
            &[
                "register_user",
                "password_login",
                "session_validate",
                "magic_link_issue",
                "magic_link_consume",
                "oauth_begin",
                "oauth_finish",
            ],
        )
    {
        return Some(Effect::DB);
    }
    if same_text(module, "core.plugin") || same_text(module, "core.mod") {
        return Some(Effect::Exec);
    }
    if same_text(module, "core.log") {
        return Some(Effect::Log);
    }
    if same_text(module, "core.ui")
        || same_text(module, "core.web")
        || same_text(module, "core.web.storage.local")
        || same_text(module, "core.web.storage.session")
    {
        return Some(Effect::Browser);
    }
    if same_text(module, "core.crypto.vault")
        && one_of(
            method,
            &[
                "get",
                "current",
                "versions",
                "load",
                "status",
                "prepare_generate",
                "prepare_store",
                "prepare_rotate",
                "prepare_retire",
                "prepare_revoke",
                "authorize_write",
                "commit_generate",
                "commit_store",
                "commit_rotate",
                "commit_retire",
                "commit_revoke",
                "export_to_recipients",
                "export_to_passphrase",
                "prepare_import_wrapped",
                "authorize_wrapped_import",
                "commit_import_wrapped",
            ],
        )
    {
        return Some(Effect::Secret);
    }
    if same_text(module, "core.crypto.vault")
        && one_of(
            method,
            &[
                "prepare_import_signing",
                "prepare_import_x25519",
                "commit_import_signing",
                "commit_import_x25519",
            ],
        )
    {
        return Some(Effect::Secret);
    }
    None
}

/// Semantic provider contract for one Core operation.
///
/// This is a capability classification, not a second effect vocabulary. The
/// existing `Effect` and `effect_leaf` fields remain the authority for effect
/// checking; this contract says whether the operation is pure or crosses a
/// provider boundary, and names the only approved fallback/simulation seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCapabilityKind {
    Pure,
    NativeRequired,
    PortableFallback,
    ExplicitSimulation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreCapabilityContract {
    pub kind: CoreCapabilityKind,
    /// Canonical provider family. `None` is valid only for `Pure`.
    pub provider: Option<&'static str>,
    /// Canonical portable implementation, when the operation permits one.
    pub fallback: Option<&'static str>,
    /// Shared typed failure identity for provider selection/availability.
    pub failure: Option<&'static str>,
}

impl CoreCapabilityContract {
    pub const fn pure() -> Self {
        Self {
            kind: CoreCapabilityKind::Pure,
            provider: None,
            fallback: None,
            failure: None,
        }
    }

    pub const fn native_required(
        provider: &'static str,
        failure: &'static str,
    ) -> Self {
        Self {
            kind: CoreCapabilityKind::NativeRequired,
            provider: Some(provider),
            fallback: None,
            failure: Some(failure),
        }
    }

    pub const fn portable_fallback(
        provider: &'static str,
        fallback: &'static str,
        failure: &'static str,
    ) -> Self {
        Self {
            kind: CoreCapabilityKind::PortableFallback,
            provider: Some(provider),
            fallback: Some(fallback),
            failure: Some(failure),
        }
    }

    pub const fn explicit_simulation(
        provider: &'static str,
        failure: &'static str,
    ) -> Self {
        Self {
            kind: CoreCapabilityKind::ExplicitSimulation,
            provider: Some(provider),
            fallback: None,
            failure: Some(failure),
        }
    }

    pub const fn is_well_formed(self) -> bool {
        let provider = match self.provider {
            Some(value) => !value.is_empty(),
            None => false,
        };
        let fallback = match self.fallback {
            Some(value) => !value.is_empty(),
            None => false,
        };
        let failure = match self.failure {
            Some(value) => !value.is_empty(),
            None => false,
        };
        match self.kind {
            CoreCapabilityKind::Pure => self.provider.is_none()
                && self.fallback.is_none()
                && self.failure.is_none(),
            CoreCapabilityKind::NativeRequired => provider && self.fallback.is_none() && failure,
            CoreCapabilityKind::PortableFallback => provider && fallback && failure,
            CoreCapabilityKind::ExplicitSimulation => provider && self.fallback.is_none() && failure,
        }
    }
}

const fn default_capability(
    module: &'static str,
    effect: Option<Effect>,
) -> CoreCapabilityContract {
    if effect.is_some() {
        CoreCapabilityContract::native_required(module, "core.capability.unavailable")
    } else {
        CoreCapabilityContract::pure()
    }
}


/// The canonical leaf for a plain Core call that performs a precise effect.
///
/// Keep this beside `effect_for`: every engine already consumes the row returned
/// by this module, so a call cannot acquire a second sema-only key.
const fn effect_leaf_for(module: &str, method: &str) -> Option<&'static str> {
    if same_text(module, "core.time") && one_of(method, &["sleep", "sleep_until"]) {
        return Some("Time.Wait");
    }
    if same_text(module, "core.tasks") && same_text(method, "yield_now") {
        return Some("Time.Wait");
    }
    if same_text(module, "core.files") && same_text(method, "scope") {
        return Some("FS.Read");
    }
    // A file operation names the one leaf it needs, so `--allow=FS.Read`
    // covers a read and never a write. `copy` and `copy_dir` both read and
    // write, so they keep the bare `FS` root.
    if same_text(module, "core.files")
        && one_of(
            method,
            &[
                "read",
                "read_bytes",
                "read_at",
                "read_link",
                "map",
                "open",
                "exists",
                "is_dir",
                "is_fifo",
                "is_socket",
                "stat",
                "lstat",
                "list_dir",
                "walk",
                "walk_parallel",
                "walk_files",
                "glob",
                "canonicalize",
                "absolute",
            ],
        )
    {
        return Some("FS.Read");
    }
    if same_text(module, "core.files")
        && one_of(
            method,
            &[
                "write",
                "write_bytes",
                "write_at",
                "write_atomic",
                "append_all",
                "create",
                "append",
                "fsync",
                "remove",
                "remove_dir",
                "remove_all",
                "create_dir",
                "create_dir_all",
                "rename",
                "symlink",
                "hard_link",
                "chown",
                "set_mode",
                "mkdtemp",
                "mktemp",
                "temp_dir",
                "temp_file",
                "lock",
            ],
        )
    {
        return Some("FS.Write");
    }
    // D-AUTH-AMBIENT1=A: reading the program's own arguments is a leaf of
    // `Exec`; spawning a process keeps the ungranted `Exec` root.
    if same_text(module, "core.process") && one_of(method, &["argv", "args"]) {
        return Some(crate::Syntax::EFFECT_LEAF_EXEC_ARGS);
    }
    if same_text(module, "core.term")
        && one_of(
            method,
            &[
                "confirm",
                "choose",
                "input_secret",
                "read_all_input",
                "readline",
                "read_until",
                "take",
                "binread",
                "binwrite",
                "progress",
                "read_key",
            ],
        )
    {
        return Some("Time.Wait");
    }
    if same_text(module, "core.sys") && one_of(method, &["wait", "waitpid"]) {
        return Some("Time.Wait");
    }
    if same_text(module, "core.process") && one_of(method, &["run", "pipeline"]) {
        return Some("Time.Wait");
    }
    if same_text(module, "core.net")
        && one_of(
            method,
            &[
                "tcp_accept",
                "tcp_connect",
                "tcp_connect_addr",
                "tcp_connect_timeout",
                "tcp_connect_happy",
                "tcp_read",
                "tcp_write",
                "tcp_read_bytes",
                "tcp_read_text",
                "tcp_write_bytes",
                "tcp_write_all_bytes",
                "tcp_write_text",
                "tcp_ready",
                "sendfile",
                "tcp_reply",
                "udp_send_to",
                "udp_recv_from",
                "udp_send_bytes_to",
                "udp_receive",
                "unix_accept",
                "unix_connect",
                "unix_read",
                "unix_write",
                "unix_read_bytes",
                "unix_write_all_bytes",
                "dns_a",
                "dns_aaaa",
                "dns_a_at",
                "dns_aaaa_at",
                "dns_txt",
                "dns_txt_at",
                "dns_ptr",
                "dns_srv",
                "dns_srv_at",
                "getservbyname",
                "getservbyport",
                "tls_connect",
                "tls_read",
                "tls_write",
                "tls_close",
            ],
        )
    {
        return Some("Time.Wait");
    }
    if (same_text(module, "core.net.tls")
        && one_of(
            method,
            &[
                "client",
                "read",
                "read_text",
                "write",
                "write_all",
                "write_text",
                "close",
            ],
        ))
        || (same_text(module, "core.http.client")
            && one_of(method, &["get", "post", "request"]))
        || (same_text(module, "core.http")
            && same_text(method, "get"))
        || (same_text(module, "core.http.server")
            && one_of(method, &["serve", "serve_once", "serve_once_listener"]))
    {
        return Some("Time.Wait");
    }
    if ((same_text(module, "core.encoding.json")
        || same_text(module, "core.encoding.jsonl")
        || same_text(module, "core.encoding.csv")
        || same_text(module, "core.encoding.xml")
        || same_text(module, "core.encoding.cbor"))
        && one_of(method, &["reader", "writer"]))
        || (same_text(module, "core.encoding.csv") && same_text(method, "query"))
    {
        return Some("Time.Wait");
    }
    if same_text(module, "core.db") && one_of(method, &["transaction", "migrate"]) {
        return Some("Time.Wait");
    }
    if same_text(module, "core.jobs") && one_of(method, &["wait", "poll"]) {
        return Some("Time.Wait");
    }
    if same_text(module, "core.service")
        && one_of(
            method,
            &[
                "send",
                "send_durable",
                "receive",
                "endpoint_send",
                "endpoint_receive",
                "workflow_sleep",
                "workflow_activity_wait",
                "workflow_all",
            ],
        )
    {
        return Some("Time.Wait");
    }
    None
}

/// The canonical leaf for blocking methods on typed Core handles.
pub const fn receiver_effect_leaf(type_name: &str, method: &str) -> Option<&'static str> {
    if (same_text(type_name, "Clock") && same_text(method, "wait"))
        || (same_text(type_name, "Task") && same_text(method, "join"))
        || (same_text(type_name, "Receiver") && same_text(method, "receive"))
        || (same_text(type_name, "Sender") && same_text(method, "send"))
        || (same_text(type_name, "Shared")
            && one_of(method, &["read", "edit", "guard_read", "guard_edit"]))
        || (same_text(type_name, "RealtimeStream") && same_text(method, "cancel"))
        || (same_text(type_name, "FileReader") && one_of(method, &["lines", "read_line"]))
        || (same_text(type_name, "FileScope") && same_text(method, "read"))
        || (same_text(type_name, "FileWriter") && one_of(method, &["write_line", "flush"]))
        || ((same_text(type_name, "Stdout") || same_text(type_name, "Stderr"))
            && one_of(method, &["write", "write_line", "write_bytes", "flush"]))
        || (same_text(type_name, "ProcessChild") && same_text(method, "wait"))
        || (same_text(type_name, "ProcessStdin") && same_text(method, "write"))
        || ((same_text(type_name, "ProcessStdoutStream")
            || same_text(type_name, "ProcessStderrStream"))
            && same_text(method, "lines"))
        || (same_text(type_name, "HTTPRequest") && same_text(method, "send"))
        || (same_text(type_name, "HTTPClient") && same_text(method, "send"))
        || (same_text(type_name, "WsConn")
            && one_of(method, &["send_text", "send_bytes", "recv", "close"]))
        || (same_text(type_name, "Browser") && same_text(method, "next_event"))
        || (same_text(type_name, "BrowserContext") && one_of(method, &["page", "tab", "close"]))
        || (same_text(type_name, "BrowserPage")
            && one_of(
                method,
                &[
                    "goto",
                    "close",
                    "screenshot",
                    "pdf",
                    "set_cookie",
                    "clear_cookies",
                    "storage_set",
                    "storage_clear",
                ],
            ))
        || (same_text(type_name, "BrowserLocator")
            && one_of(
                method,
                &["wait", "wait_gone", "click", "hover", "fill", "press", "set_files"],
            ))
        || (same_text(type_name, "BrowserProtocol") && same_text(method, "send"))
        || (same_text(type_name, "TcpListener") && same_text(method, "accept"))
        || (same_text(type_name, "TcpStream")
            && one_of(
                method,
                &[
                    "read",
                    "read_text",
                    "write",
                    "write_all",
                    "write_text",
                    "ready",
                ],
            ))
        || (same_text(type_name, "UdpSocket")
            && one_of(method, &["receive", "send_to", "ready"]))
        || (same_text(type_name, "UnixListener") && same_text(method, "accept"))
        || (same_text(type_name, "UnixStream")
            && one_of(method, &["read", "write_all", "ready"]))
        || (same_text(type_name, "TLSStream")
            && one_of(method, &["read", "write_all", "close_write", "ready"]))
        || (same_text(type_name, "DBConnection")
            && one_of(method, &["with_policy", "begin", "commit", "rollback", "close"]))
        || (same_text(type_name, "DBScope")
            && one_of(method, &["begin", "commit", "rollback", "close"]))
        || (same_text(type_name, "DBPool")
            && one_of(method, &["acquire", "ready", "drain"]))
        || (same_text(type_name, "ServiceTree")
            && one_of(method, &["send", "send_durable", "receive"]))
        || (same_text(type_name, "ServiceEndpoint")
            && one_of(method, &["send", "send_durable", "receive"]))
        || (same_text(type_name, "ServiceWorkflow")
            && one_of(method, &["sleep", "activity_wait", "all"]))
        || (same_text(type_name, "JobQueue") && same_text(method, "wait"))
        || (same_text(type_name, "JobQueueWorker") && same_text(method, "poll"))
    {
        Some("Time.Wait")
    } else {
        None
    }
}

const fn sink_for(module: &str, method: &str) -> Option<SinkClass> {
    if same_text(module, "core.log") {
        return Some(SinkClass::Credential);
    }
    if same_text(module, "core.term") && one_of(method, &["print", "eprint"]) {
        return Some(SinkClass::Credential);
    }
    let encoding = (same_text(module, "core.encoding.json")
        || same_text(module, "core.encoding.csv")
        || same_text(module, "core.encoding.toml")
        || same_text(module, "core.encoding.yaml")
        || same_text(module, "core.encoding.cbor")
        || same_text(module, "core.encoding.xml"))
        && one_of(
            method,
            &[
                "to_string",
                "to_string_pretty",
                "to_bytes",
                "to_bytes_canonical",
            ],
        );
    if encoding || (same_text(module, "core.encoding.jsonl") && same_text(method, "to_string")) {
        return Some(SinkClass::Credential);
    }
    None
}

/// The erased calling convention needed by all plain projections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreCallSignature {
    /// Number of positional values passed to the Prelude symbol.
    pub arity: usize,
    /// Inclusive upper bound for optional-argument rows. Plain rows have the
    /// same lower and upper bound.
    pub max_arity: usize,
    /// Whether each argument is rendered as a shared borrow by AOT.
    pub borrow_mask: &'static [bool],
}

/// Exact Jet return/parameter types stay in sema. This marker prevents a
/// consumer from silently inventing a second fallibility table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCallFallibility {
    /// Resolve from the canonical sema fixed/resolved signature.
    Sema,
}

/// The compiler projections declared by one Core-call row.
///
/// The bits describe consumers of the row, not alternate semantic
/// implementations. A projection may still use a typed adapter; it must not
/// invent a second key, signature, effect, or symbol record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreCallCoverage(u8);

impl CoreCallCoverage {
    pub const SEMA: u8 = 1 << 0;
    pub const TIR_SUBSET: u8 = 1 << 1;
    pub const TIR_EVAL: u8 = 1 << 2;
    pub const AOT: u8 = 1 << 3;
    pub const INTERPRETER: u8 = 1 << 4;
    pub const COMPTIME: u8 = 1 << 5;
    pub const JIT: u8 = 1 << 6;
    pub const KNOWN: u8 = Self::SEMA
        | Self::TIR_SUBSET
        | Self::TIR_EVAL
        | Self::AOT
        | Self::INTERPRETER
        | Self::COMPTIME
        | Self::JIT;

    /// The table-driven compiler projections. Interpreter coverage is added
    /// only when the row has a real route below, or when a pure/ambient route
    /// builder explicitly supplies one.
    const fn compiler_projections() -> Self {
        Self::from_bits(Self::KNOWN & !Self::INTERPRETER)
    }

    const fn with_projection(self, projection: u8) -> Self {
        Self::from_bits(self.bits() | projection)
    }

    pub const fn contains(self, projection: u8) -> bool {
        self.0 & projection == projection
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    pub const fn is_complete(self) -> bool {
        self.bits() == Self::KNOWN
    }
}

/// Why one engine could not project a row. The engine owns user-facing
/// diagnostics; this type only reports the shared table fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCallProjectionError {
    Unknown,
    Uncovered { projection: u8 },
    Arity { expected: usize, actual: usize },
}

/// Pure comptime/REPL projection family. The row owns this routing fact so
/// the evaluator does not keep a second `(module, member)` membership list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCallPureRoute {
    None,
    Mime,
    Email,
    EncodingXml,
    Time,
    Math,
    Measurement,
    Date,
    DateTime,
    SketchHll,
    SketchTDigest,
    SketchCms,
    SketchReservoir,
    Ui,
    Raylib,
    Io,
    Net,
    Crypto,
}

/// The executable route for a row's interpreter projection.  `Pure` names
/// the shared CorePureParity family; the other two variants select the
/// existing ambient or typed evaluator adapters.  `None` is a deliberate
/// incomplete-row marker and must not coexist with interpreter coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCallInterpreterRoute {
    None,
    Pure(CoreCallPureRoute),
    Ambient,
    TypedIntrinsic,
}

impl CoreCallInterpreterRoute {
    pub const fn is_executable(self) -> bool {
        !matches!(self, Self::None | Self::Pure(CoreCallPureRoute::None))
    }
}

/// Generated registry-facing arms for the interpreter ambient dispatcher.
///
/// Core.jet owns the route declarations and constructor rows. This projection
/// is checked in for compiler consumers; hand-written marshalling remains in
/// the row adapter expressions, not in a second registry.


pub fn core_call_ambient_routes() -> &'static [(&'static str, &'static str)] {
    CORE_CALL_AMBIENT_ROUTES
}


const fn default_interpreter_route(module: &str, member: &str) -> CoreCallInterpreterRoute {
    if effect_for(module, member).is_none() {
        CoreCallInterpreterRoute::TypedIntrinsic
    } else {
        CoreCallInterpreterRoute::None
    }
}

/// Where AOT resolves a Core call's Rust symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCallSymbol {
    /// Prefix the symbol with the generated program's Prelude root.
    Prelude(&'static str),
    /// Emit the Rust symbol as written.
    Rust(&'static str),
}

impl CoreCallSymbol {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Prelude(name) | Self::Rust(name) => name,
        }
    }
}

/// One marker application carried by an ordinary Core declaration row.
///
/// D-STRUCT-LIFE1=A: Core declarations have no Jet source file in which to
/// write `#Deprecated`, so the canonical declaration row carries the same
/// marker metadata that user items carry in `AST::Deprecation`. The `member`
/// is the retiring alias; the row's own `member` remains the replacement
/// declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreMarkerApplication {
    pub member: &'static str,
    pub since: &'static str,
    pub replacement: &'static str,
    pub removed_in: Option<&'static str>,
}

impl CoreMarkerApplication {
    pub const fn deprecated(
        member: &'static str,
        since: &'static str,
        replacement: &'static str,
        removed_in: Option<&'static str>,
    ) -> Self {
        Self {
            member,
            since,
            replacement,
            removed_in,
        }
    }
}
/// One checked resource argument on a Core primitive.
///
/// This metadata is compiler-internal. It describes the already-checked
/// argument boundary; it does not add a user-facing resource declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCallResourceAccessMode {
    Read,
    Write,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreCallResourceAccess {
    pub argument: usize,
    pub mode: CoreCallResourceAccessMode,
}

impl CoreCallResourceAccess {
    pub const fn read(argument: usize) -> Self {
        Self {
            argument,
            mode: CoreCallResourceAccessMode::Read,
        }
    }

    pub const fn write(argument: usize) -> Self {
        Self {
            argument,
            mode: CoreCallResourceAccessMode::Write,
        }
    }

    pub const fn move_(argument: usize) -> Self {
        Self {
            argument,
            mode: CoreCallResourceAccessMode::Move,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCallCompletionKind {
    /// The provider returns only after all accesses are complete.
    Synchronous,
    /// The provider submits work and exposes the named token to its event
    /// adapter; the frame must retain accesses until that token is observed.
    Pending,
    /// The provider is the checked event that completes the named token.
    Event,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreCallCompletion {
    pub kind: CoreCallCompletionKind,
    pub token: Option<&'static str>,
    /// Canonical provider identity used by runtime adapters to signal the
    /// token. It is not a symbol alias or a second lowering route.
    pub provider: &'static str,
}

impl CoreCallCompletion {
    pub const fn synchronous(provider: &'static str) -> Self {
        Self {
            kind: CoreCallCompletionKind::Synchronous,
            token: None,
            provider,
        }
    }

    pub const fn pending(token: &'static str, provider: &'static str) -> Self {
        Self {
            kind: CoreCallCompletionKind::Pending,
            token: Some(token),
            provider,
        }
    }

    pub const fn event(token: &'static str, provider: &'static str) -> Self {
        Self {
            kind: CoreCallCompletionKind::Event,
            token: Some(token),
            provider,
        }
    }
}

/// One plain Core-call record shared by every engine projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreCallRecord {
    pub module: &'static str,
    pub member: &'static str,
    /// Non-empty only for receiver/static-method rows in the same registry.
    /// Function rows use `core_call`; receiver rows use `core_receiver_method`.
    pub receiver_types: &'static [&'static str],
    pub signature: CoreCallSignature,
    /// Checked resource arguments consumed by this primitive. Empty means the
    /// row does not participate in frame-resource scheduling.
    pub frame_accesses: &'static [CoreCallResourceAccess],
    /// Provider completion contract for frame-resource accesses.
    pub frame_completion: Option<CoreCallCompletion>,
    pub fallibility: CoreCallFallibility,
    pub effect: Option<Effect>,
    /// Provider/fallback contract shared by AOT, JIT, interpreter, and web.
    pub capability: CoreCapabilityContract,
    /// Optional precise leaf for a call that can wait on external work.
    pub effect_leaf: Option<&'static str>,
    pub sink_class: Option<SinkClass>,
    pub pure_route: CoreCallPureRoute,
    pub interpreter_route: CoreCallInterpreterRoute,
    pub symbol: CoreCallSymbol,
    pub coverage: CoreCallCoverage,
    /// Whether AOT can emit this row as one plain symbol call. Typed rows can
    /// still be canonical records while remaining on their typed emitter.
    pub aot_direct: bool,
    /// Whether the resident JIT can use the direct symbol ABI. `false` keeps
    /// a typed/closure adapter on its existing lowering path.
    pub jit_direct: bool,
    /// Optional resident-JIT symbol when the AOT spelling is not its host
    /// spelling. This keeps the alias in the one record instead of lowering.
    pub jit_symbol: Option<&'static str>,
    /// Whether the kernel returns its `Int` results as raw machine words
    /// (`i64`) rather than owned exact-Int carriers. Every engine widens a
    /// flagged row's `Int` results into the carrier and passes an unflagged
    /// row's results through unchanged; no engine infers the shape from a
    /// returned word, which could be read as an owner pointer.
    pub native_int_result: bool,
    /// Optional package authority names required by a UI host service row.
    /// Constructors remain capability-free; only rows that cross an operating
    /// system service boundary attach these exact, ratified names.
    pub ui_capabilities: &'static [&'static str],
    /// Marker metadata attached to this ordinary declaration row.
    pub marker: Option<CoreMarkerApplication>,
}

impl CoreCallRecord {
    pub const fn new(
        module: &'static str,
        member: &'static str,
        symbol: &'static str,
        prelude: bool,
        borrow_mask: &'static [bool],
    ) -> Self {
        let interpreter_route = default_interpreter_route(module, member);
        let coverage = if interpreter_route.is_executable() {
            CoreCallCoverage::compiler_projections().with_projection(CoreCallCoverage::INTERPRETER)
        } else {
            CoreCallCoverage::compiler_projections()
        };
        let effect = effect_for(module, member);
        Self {
            module,
            member,
            receiver_types: &[],
            signature: CoreCallSignature {
                arity: borrow_mask.len(),
                max_arity: borrow_mask.len(),
                borrow_mask,
            },
            frame_accesses: &[],
            frame_completion: None,
            fallibility: CoreCallFallibility::Sema,
            effect,
            capability: default_capability(module, effect),
            effect_leaf: effect_leaf_for(module, member),
            sink_class: sink_for(module, member),
            pure_route: CoreCallPureRoute::None,
            interpreter_route,
            symbol: if prelude {
                CoreCallSymbol::Prelude(symbol)
            } else {
                CoreCallSymbol::Rust(symbol)
            },
            coverage,
            aot_direct: true,
            jit_direct: true,
            jit_symbol: None,
            native_int_result: false,
            ui_capabilities: &[],
            marker: None,
        }
    }

    /// Construct a row with an explicit consumer projection set. Use this for
    /// fixtures and deliberate exceptions so a tier omission is reviewable at
    /// the table edit instead of being hidden by a universal marker.
    pub const fn new_with_coverage(
        module: &'static str,
        member: &'static str,
        symbol: &'static str,
        prelude: bool,
        borrow_mask: &'static [bool],
        coverage: CoreCallCoverage,
    ) -> Self {
        let mut row = Self::new(module, member, symbol, prelude, borrow_mask);
        row.coverage = coverage;
        row
    }

    /// Add a receiver/static-method row without creating a second registry.
    pub const fn receiver(
        receiver_types: &'static [&'static str],
        member: &'static str,
        borrow_mask: &'static [bool],
    ) -> Self {
        Self {
            module: "",
            member,
            receiver_types,
            signature: CoreCallSignature {
                arity: borrow_mask.len(),
                max_arity: borrow_mask.len(),
                borrow_mask,
            },
            frame_accesses: &[],
            frame_completion: None,
            fallibility: CoreCallFallibility::Sema,
            effect: None,
            capability: CoreCapabilityContract::pure(),
            effect_leaf: None,
            sink_class: None,
            pure_route: CoreCallPureRoute::None,
            interpreter_route: CoreCallInterpreterRoute::TypedIntrinsic,
            symbol: CoreCallSymbol::Rust(""),
            coverage: CoreCallCoverage::compiler_projections()
                .with_projection(CoreCallCoverage::INTERPRETER),
            aot_direct: false,
            jit_direct: false,
            jit_symbol: None,
            native_int_result: false,
            ui_capabilities: &[],
            marker: None,
        }
    }
    /// Receiver row with a shared Prelude symbol for an ambient typed handle.
    pub const fn receiver_with_symbol(
        receiver_types: &'static [&'static str],
        member: &'static str,
        symbol: &'static str,
        prelude: bool,
        borrow_mask: &'static [bool],
    ) -> Self {
        let mut row = Self::receiver(receiver_types, member, borrow_mask);
        row.symbol = if prelude {
            CoreCallSymbol::Prelude(symbol)
        } else {
            CoreCallSymbol::Rust(symbol)
        };
        row
    }

    /// Receiver row with explicit projection coverage. Web receiver methods
    /// are sema-only until their backend adapters land.
    pub const fn receiver_with_coverage(
        receiver_types: &'static [&'static str],
        member: &'static str,
        borrow_mask: &'static [bool],
        coverage: CoreCallCoverage,
    ) -> Self {
        let mut row = Self::receiver(receiver_types, member, borrow_mask);
        row.coverage = coverage;
        row.interpreter_route = CoreCallInterpreterRoute::None;
        row
    }

    pub const fn with_frame_accesses(
        mut self,
        accesses: &'static [CoreCallResourceAccess],
    ) -> Self {
        self.frame_accesses = accesses;
        self
    }

    pub const fn with_frame_completion(mut self, completion: CoreCallCompletion) -> Self {
        self.frame_completion = Some(completion);
        self
    }

    pub const fn frame_accesses(self) -> &'static [CoreCallResourceAccess] {
        self.frame_accesses
    }

    pub const fn frame_completion(self) -> Option<CoreCallCompletion> {
        self.frame_completion
    }

    /// The canonical number of positional arguments for this plain call.
    pub const fn arity(self) -> usize {
        self.signature.arity
    }

    pub const fn accepts_arity(self, count: usize) -> bool {
        count >= self.signature.arity && count <= self.signature.max_arity
    }

    pub const fn with_max_arity(mut self, max_arity: usize) -> Self {
        self.signature.max_arity = max_arity;
        self
    }

    pub const fn with_pure_route(mut self, route: CoreCallPureRoute) -> Self {
        self.pure_route = route;
        self.interpreter_route = CoreCallInterpreterRoute::Pure(route);
        if !matches!(route, CoreCallPureRoute::None) {
            self.coverage = self.coverage.with_projection(CoreCallCoverage::INTERPRETER);
        }
        self
    }

    pub const fn with_interpreter_route(mut self, route: CoreCallInterpreterRoute) -> Self {
        self.interpreter_route = route;
        if route.is_executable() {
            self.coverage = self.coverage.with_projection(CoreCallCoverage::INTERPRETER);
        }
        self
    }

    pub const fn with_capability(mut self, capability: CoreCapabilityContract) -> Self {
        self.capability = capability;
        self
    }

    pub const fn capability(self) -> CoreCapabilityContract {
        self.capability
    }

    /// Read the one effect fact. The record owns the key; `Effects` owns the
    /// value, so this projection cannot drift from sema or comptime.
    pub fn effect(self) -> Option<Effect> {
        self.effect
    }
    /// Read the optional precise wait leaf for this call.
    pub const fn effect_leaf(self) -> Option<&'static str> {
        self.effect_leaf
    }


    /// Read the one sink fact. `None` means this call is not a registered sink.
    pub fn sink_class(self) -> Option<SinkClass> {
        self.sink_class
    }

    /// A plain record has a direct symbol projection in every codegen path;
    /// argument/value eligibility is still checked by each engine's adapter.
    pub const fn has_direct_symbol(self) -> bool {
        self.aot_direct && !self.is_receiver() && !self.symbol.name().is_empty()
    }

    pub const fn is_receiver(self) -> bool {
        !self.receiver_types.is_empty()
    }

    pub const fn without_direct_aot(mut self) -> Self {
        self.aot_direct = false;
        self
    }

    /// Keep a row visible to all metadata consumers while selecting a typed
    /// or closure-shaped JIT adapter instead of the direct host projection.
    pub const fn without_direct_jit(mut self) -> Self {
        self.jit_direct = false;
        self
    }

    pub const fn with_jit_symbol(mut self, symbol: &'static str) -> Self {
        self.jit_direct = true;
        self.jit_symbol = Some(symbol);
        self
    }
    /// Mark a row whose kernel returns `Int` results as raw machine words.
    pub const fn with_native_int_result(mut self) -> Self {
        self.native_int_result = true;
        self
    }
    /// Attach the exact package authority names required by this UI host row.
    pub const fn with_ui_capabilities(
        mut self,
        capabilities: &'static [&'static str],
    ) -> Self {
        self.ui_capabilities = capabilities;
        self
    }

    pub const fn ui_capabilities(self) -> &'static [&'static str] {
        self.ui_capabilities
    }

    pub const fn with_marker(mut self, marker: CoreMarkerApplication) -> Self {
        self.marker = Some(marker);
        self
    }

    pub const fn key(self) -> (&'static str, &'static str) {
        (self.module, self.member)
    }

    pub const fn prelude_symbol(self) -> &'static str {
        self.symbol.name()
    }

    /// Positions whose erased Core ABI takes a filesystem path. Sema accepts
    /// `String | Path`; AOT, JIT, and the interpreter marshal a `Path` through
    /// its canonical string representation at this boundary.
    pub fn path_mask(self) -> &'static [bool] {
        match (self.module, self.member) {
            (
                "core.files",
                "read"
                | "read_bytes"
                | "map"
                | "exists"
                | "is_dir"
                | "remove"
                | "remove_dir"
                | "remove_all"
                | "list_dir"
                | "create_dir"
                | "create_dir_all"
                | "stat"
                | "set_mode"
                | "canonicalize"
                | "absolute"
                | "walk"
                | "walk_parallel"
                | "walk_files"
                | "glob"
                | "fsync"
                | "lock"
                | "open"
                | "create"
                | "append",
            ) => &[true],
            ("core.files", "write" | "write_bytes" | "append_all" | "write_atomic") => &[true],
            ("core.files", "copy" | "copy_dir" | "rename" | "symlink" | "hard_link") => {
                &[true, true]
            }
            ("core.files", "read_link") => &[true],
            ("core.files", "read_at") => &[true],
            ("core.files", "write_at") => &[true],
            ("core.watcher", "files") => &[true],
            ("core.encoding.csv", "query") => &[true],
            ("core.term", "binread") => &[true],
            ("core.term", "binwrite") => &[true],
            ("core.sys", "set_current_dir" | "mkfifo") => &[true],
            ("core.sys", "utime") => &[true, false, false],
            _ => &[],
        }
    }

    pub fn path_arg(self, index: usize) -> bool {
        let mask = self.path_mask();
        index < mask.len() && mask[index]
    }

    /// Candidate resident-JIT symbols for this Prelude projection.
    ///
    /// The JIT host is an ABI adapter, so its exported name has the same
    /// suffix as the Prelude symbol with the tier prefix changed. Keeping
    /// that projection here lets lowering ask the host registry for a
    /// function without maintaining a second `(module, member)` map.
    pub fn jit_symbol_candidates(self) -> Vec<String> {
        let name = self.symbol.name();
        let mut candidates = self
            .jit_symbol
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        if !name.is_empty() && !candidates.iter().any(|known| known == name) {
            candidates.push(name.to_string());
        }
        for prefix in ["jet_std_", "jet_ring_", "jet_"] {
            let Some(suffix) = name.strip_prefix(prefix) else {
                continue;
            };
            let candidate = format!("jet_jit_{suffix}");
            if !candidates.iter().any(|known| known == &candidate) {
                candidates.push(candidate);
            }
        }
        if !self.is_receiver() {
            // Most resident hosts use the stable `jet_jit_<module>_<member>`
            // spelling, while a few old hosts use only the leaf module or
            // member (`jet_jit_xml_parse`, `jet_jit_interval`). These are
            // deterministic projections of the row key, not a second
            // `(module, member)` registry.
            let module_tail = self.module.strip_prefix("core.").unwrap_or(self.module);
            let module_tail = module_tail.replace('.', "_");
            let leaf = self.module.rsplit('.').next().unwrap_or(self.module);
            for candidate in [
                format!("jet_jit_{module_tail}_{}", self.member),
                format!("jet_jit_{leaf}_{}", self.member),
                format!("jet_jit_{}", self.member),
            ] {
                if !candidates.iter().any(|known| known == &candidate) {
                    candidates.push(candidate);
                }
            }
        }
        candidates
    }
}

/// Every Core call whose form is a plain symbol call.
///
/// The generated ambient route manifest is projected onto matching rows below.
/// Keeping that choice in the generated expression avoids a const-time scan of
/// the complete route manifest for every row.
const fn sema_web_call(
    module: &'static str,
    member: &'static str,
    symbol: &'static str,
    borrow_mask: &'static [bool],
) -> CoreCallRecord {
    CoreCallRecord::new(module, member, symbol, true, borrow_mask)
        .with_interpreter_route(CoreCallInterpreterRoute::TypedIntrinsic)
}


// BEGIN GENERATED CORE CALLS
// Source: crates/jet-codegen/src/Prelude/Core.jet
// Source SHA-256: e11c2418ca73472d5f8ac2e7ca1cf90ad112f899e4754074d86a629f117aa7c5
// Dispatcher rows and ambient routes are generated from Core.jet.
pub const CORE_CALL_AMBIENT_ROUTES: &[(&str, &str)] = &[
    ("core.data", "left_join"),
    ("core.encoding.json", "decode"),
    ("core.files", "read_at"),
    ("core.files", "read_bytes"),
    ("core.files", "map"),
    ("core.game", "run"),
    ("core.jobs", "queue"),
    ("core.db", "transaction"),
    ("core.db", "migrate"),
    ("core.game.raylib", "window_open"),
    ("core.game.raylib", "window_should_close"),
    ("core.game.raylib", "window_ready"),
    ("core.game.raylib", "begin_drawing"),
    ("core.game.raylib", "clear_background"),
    ("core.game.raylib", "draw_text"),
    ("core.game.raylib", "draw_rectangle"),
    ("core.game.raylib", "end_drawing"),
    ("core.game.raylib", "close_window"),
    ("core.game.raylib", "key_down"),
    ("core.game.raylib", "set_target_fps"),
    ("core.game.raylib", "load_sound"),
    ("core.game.raylib", "play_sound"),
    ("core.game.raylib", "color"),
    ("core.game.raylib", "gamepad_down"),
    ("core.game.raylib", "gamepad_axis"),
    ("core.game.raylib", "load_texture_atlas"),
    ("core.game.raylib", "draw_sprite"),
    ("core.reactive", "signal"),
    ("core.event", "scope"),
    ("core.event", "decision_hook"),
    ("core.event", "hook"),
    ("core.event", "policy_sync"),
    ("core.event", "new"),
    ("core.event", "with_policy"),
    ("core.event", "async_result"),
    ("core.testing", "snap"),
    ("core.testing", "golden"),
    ("core.testing", "corpus"),
    ("core.testing", "test_suite"),
    ("core.testing", "compare"),
    ("core.testing", "histories"),
    ("core.testing", "assert_equal"),
    ("core.testing", "status"),
    ("core.testing", "world"),
    ("core.watcher", "files"),
    ("core.watcher", "process_pid"),
    ("core.watcher", "port"),
    ("core.watcher", "set"),
    ("core.service", "state_store"),
    ("core.service", "tree"),
    ("core.service", "tree_show"),
    ("core.service", "delivery_at_most_once"),
    ("core.service", "delivery_durable"),
    ("core.service", "set_delivery"),
    ("core.service", "worker"),
    ("core.service", "start"),
    ("core.service", "stop"),
    ("core.service", "runtime"),
    ("core.service", "endpoint_send"),
    ("core.service", "endpoint_receive"),
    ("core.service", "endpoint_show"),
    ("core.service", "delivery_state_show"),
    ("core.service", "delivery_status"),
    ("core.devtools", "publish"),
    ("core.ui", "gtk_backend"),
    ("core.ui", "point"),
    ("core.ui", "size"),
    ("core.ui", "rect"),
    ("core.ui", "constraint"),
    ("core.ui", "node"),
    ("core.ui", "text"),
    ("core.ui", "resize_event"),
    ("core.ui", "node_role"),
    ("core.ui", "node_color"),
    ("core.ui", "aria_role_button"),
    ("core.ui", "aria_role_text_input"),
    ("core.ui", "aria_role_label"),
    ("core.ui", "aria_role_container"),
    ("core.ui", "box"),
    ("core.log", "span"),
    ("core.log", "critical"),
    ("core.log", "enter"),
    ("core.log", "enabled"),
    ("core.log", "fatal"),
    ("core.log", "float"),
    ("core.log", "redact"),
    ("core.log", "set_level"),
    ("core.log", "set_trace_id"),
    ("core.log", "close"),
    ("core.log", "setup"),
    ("core.net", "getservbyname"),
    ("core.net", "getservbyport"),
    ("core.sys", "temp_dir"),
    ("core.sys", "name"),
    ("core.sys", "executable"),
    ("core.sys", "version"),
    ("core.sys", "getuid"),
    ("core.sys", "geteuid"),
    ("core.sys", "getgid"),
    ("core.sys", "getsid"),
    ("core.sys", "times"),
    ("core.sys", "wait"),
    ("core.testing", "fixture"),
    ("core.files", "rename"),
    ("core.files", "fsync"),
    ("core.files", "symlink"),
    ("core.files", "walk"),
    ("core.files", "walk_parallel"),
    ("core.files", "walk_files"),
    ("core.files", "glob"),
    ("core.files", "canonicalize"),
    ("core.files", "absolute"),
    ("core.files", "stat"),
    ("core.files", "set_mode"),
    ("core.files", "open"),
    ("core.files", "create"),
    ("core.files", "append"),
    ("core.files", "hard_link"),
    ("core.files", "lock"),
    ("core.files", "read_link"),
    ("core.files", "temp_dir"),
    ("core.files", "temp_file"),
    ("core.term", "confirm"),
    ("core.term", "choose"),
    ("core.term", "input_secret"),
    ("core.term", "read_all_input"),
    ("core.term", "readline"),
    ("core.term", "stdin"),
    ("core.term", "stdout"),
    ("core.term", "stderr"),
    ("core.term", "terminal_width"),
    ("core.term", "terminal_height"),
    ("core.term", "style"),
    ("core.term", "progress"),
    ("core.term", "progress_iter"),
    ("core.sys", "get"),
    ("core.sys", "set"),
    ("core.sys", "home_dir"),
    ("core.sys", "family"),
    ("core.sys", "unset"),
    ("core.sys", "vars"),
    ("core.sys", "current_dir"),
    ("core.sys", "arch"),
    ("core.sys", "cpu_count"),
    ("core.sys", "pid"),
    ("core.sys", "getpid"),
    ("core.sys", "hostname"),
    ("core.sys", "username"),
    ("core.sys", "release"),
    ("core.sys", "expand"),
    ("core.sys", "getppid"),
    ("core.sys", "getegid"),
    ("core.sys", "getgroups"),
    ("core.sys", "getpgrp"),
    ("core.sys", "uptime"),
    ("core.sys", "loadavg"),
    ("core.sys", "sync"),
    ("core.sys", "getpgid"),
    ("core.sys", "exitcode"),
    ("core.sys", "umask"),
    ("core.sys", "getpriority"),
    ("core.sys", "setpriority"),
    ("core.sys", "utime"),
    ("core.sys", "stop"),
    ("core.sys", "set_current_dir"),
    ("core.sys", "fork"),
    ("core.sys", "setuid"),
    ("core.sys", "setgid"),
    ("core.sys", "setpgid"),
    ("core.sys", "setpgrp"),
    ("core.sys", "setsid"),
    ("core.sys", "initgroups"),
    ("core.sys", "kill"),
    ("core.sys", "waitpid"),
    ("core.sys", "pipe"),
    ("core.sys", "close_fd"),
    ("core.sys", "mkfifo"),
    ("core.process", "cmd"),
    ("core.process", "pipeline"),
    ("core.process", "argv"),
    ("core.process", "args"),
    ("core.plugin", "load"),
    ("core.mod", "load"),
    ("core.testing", "temp_dir"),
    ("core.math", "from_bits"),
    ("core.math.random", "seed"),
    ("core.math.random", "split"),
    ("core.crypto.random", "bytes"),
    ("core.time", "now"),
    ("core.time", "sleep"),
    ("core.time", "start"),
    ("core.time", "now_utc"),
    ("core.time", "today"),
    ("core.time", "parse_rfc3339"),
    ("core.time", "new"),
    ("core.time", "days_in_month"),
    ("core.time", "is_leap_year"),
    ("core.log", "info"),
    ("core.log", "warn"),
    ("core.log", "error"),
    ("core.log", "debug"),
    ("core.log", "disable"),
    ("core.log", "flush"),
    ("core.log", "field"),
    ("core.log", "int"),
    ("core.log", "bool"),
    ("core.log", "counter"),
    ("core.log", "info_fields"),
    ("core.log", "warn_fields"),
    ("core.log", "error_fields"),
    ("core.log", "debug_fields"),
    ("core.log", "set_sink"),
    ("core.log", "sample_every"),
    ("core.log", "otlp_file"),
    ("core.auth", "session_validate"),
    ("core.auth", "session_show"),
    ("core.auth", "session_user"),
    ("core.auth", "session_cookie"),
    ("core.auth", "session_id"),
    ("core.auth", "register_user"),
    ("core.auth", "password_login"),
    ("core.auth", "magic_link_issue"),
    ("core.auth", "magic_link_consume"),
    ("core.auth", "oauth_begin"),
    ("core.auth", "oauth_finish"),
    ("core.net", "socket_addr"),
    ("core.net", "tcp_listen"),
    ("core.net", "tcp_listen_addr"),
    ("core.net", "tcp_accept"),
    ("core.net", "tcp_connect"),
    ("core.net", "tcp_connect_addr"),
    ("core.net", "tcp_connect_timeout"),
    ("core.net", "tcp_connect_happy"),
    ("core.net", "listener_local_socket_addr"),
    ("core.net", "nodelay"),
    ("core.net", "set_nodelay"),
    ("core.net", "ttl"),
    ("core.net", "set_ttl"),
    ("core.net", "socket_type"),
    ("core.net", "udp_bind"),
    ("core.net", "udp_bind_addr"),
    ("core.net", "udp_local_addr"),
    ("core.net", "udp_set_timeout"),
    ("core.net", "udp_send_to"),
    ("core.net", "udp_recv_from"),
    ("core.net", "udp_send_bytes_to"),
    ("core.net", "udp_receive"),
    ("core.net.ws", "connect"),
    ("core.net.ws", "upgrade"),
    ("core.db", "row_value"),
    ("core.db", "row_int"),
    ("core.db", "row_float"),
    ("core.db", "row_text"),
    ("core.db", "row_bool"),
    ("core.db", "policy_audit"),
    ("core.db", "pool"),
    ("core.db", "open"),
    ("core.db", "open_memory"),
    ("core.db", "policy"),
    ("core.ui", "tui_backend"),
    ("core.ui", "button"),
    ("core.ui", "key_event"),
    ("core.ui", "reactive_render"),
    ("app", "live_get"),
    ("core.web", "live_get"),
    ("app", "live_show"),
    ("core.web", "live_show"),
    ("app", "live_stats"),
    ("core.web", "live_stats"),
    ("app", "auth_routes"),
    ("core.web", "auth_routes"),
    ("app", "auth_show"),
    ("core.web", "auth_show"),
    ("core.http", "serve"),
    ("core.http.server", "serve_once"),
    ("core.http.server", "serve_once_listener"),
    ("core.http.server", "bind"),
    ("core.http.server", "static_file"),
    ("core.http.server", "static_file_range"),
    ("core.http.server", "mux"),
    ("core.http.server", "response"),
    ("core.http.server", "tls"),
    ("core.http.server", "sse"),
    ("core.http.server", "json"),
    ("core.http.server", "cors"),
    ("core.http.server", "cors_policy"),
    ("core.http.server", "access_log"),
    ("core.http.server", "request_id"),
    ("core.files", "write_at"),
    ("core.files", "write_bytes"),
    ("core.files", "write_atomic"),
    ("core.term", "buffered"),
    ("core.term", "binwrite"),
    ("core.encoding.csv", "query"),
    ("core.crypto.vault", "prepare_rotate"),
    ("core.crypto.vault", "authorize_write"),
    ("core.crypto.vault", "commit_rotate"),
    ("core.crypto.vault", "prepare_generate"),
    ("core.crypto.vault", "prepare_store"),
    ("core.crypto.vault", "versions"),
    ("core.crypto.vault", "commit_generate"),
    ("core.crypto.vault", "commit_store"),
    ("core.crypto.vault", "prepare_import_signing"),
    ("core.crypto.vault", "commit_import_signing"),
    ("core.crypto.vault", "prepare_import_x25519"),
    ("core.crypto.vault", "commit_import_x25519"),
    ("core.crypto.vault", "load"),
    ("core.crypto.vault", "status"),
    ("core.crypto.vault", "prepare_retire"),
    ("core.crypto.vault", "prepare_revoke"),
    ("core.crypto.vault", "commit_retire"),
    ("core.crypto.vault", "commit_revoke"),
    ("core.crypto.vault", "export_to_recipients"),
    ("core.crypto.vault", "export_to_passphrase"),
    ("core.crypto.vault", "prepare_import_wrapped"),
    ("core.crypto.vault", "authorize_wrapped_import"),
    ("core.crypto.vault", "commit_import_wrapped"),
    ("core.web.storage.session", "get"),
    ("core.web.storage.session", "remove"),
    ("core.web.storage.session", "set"),
    ("core.web.storage.session", "clear"),
    ("core.web.storage.local", "get"),
    ("core.web.storage.local", "set"),
    ("core.web.storage.local", "remove"),
    ("core.web.storage.local", "clear"),
    ("core.net", "tcp_local_addr"),
    ("core.net", "tcp_peer_addr"),
    ("core.net", "tcp_local_socket_addr"),
    ("core.net", "tcp_peer_socket_addr"),
    ("core.net", "udp_packet_data"),
    ("core.net", "udp_packet_addr"),
    ("core.net", "udp_packet_bytes"),
    ("core.net", "udp_packet_original_len"),
    ("core.net", "udp_packet_truncated"),
    ("core.data", "inner_join"),
    ("core.net", "unix_listen"),
    ("core.net", "tcp_reply"),
    ("core.net", "unix_accept"),
];

pub const CORE_CALLS: &[CoreCallRecord] = &[
    CoreCallRecord::new("core.prelude", "keep", "jet_keep", true, &[false]).without_direct_jit(),
    CoreCallRecord::new( "core.devtools", "publish", "jet_devtools_publish", true, &[false, false, false, false, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.mem", "volatile_read", "std::ptr::read_volatile", false, &[false], ) .without_direct_aot(),
    CoreCallRecord::new( "core.mem", "volatile_write", "std::ptr::write_volatile", false, &[false, false], ) .without_direct_aot(),
    CoreCallRecord::new( "core.tasks", "interval", "jet_std::interval", true, &[false], ),
    CoreCallRecord::new("core.tasks", "after", "jet_std::after_value", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_jit_symbol("jet_jit_after_value"),
    CoreCallRecord::new( "core.tasks", "yield_now", "jet_std::jet_task_yield", true, &[], ),
    CoreCallRecord::new( "core.tasks", "current_task", "jet_std::jet_task_current_trace", true, &[], ),
    CoreCallRecord::new("core.tasks", super::INTERNAL_CHANNEL_NEW_METHOD, "jet_std::channel", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_jit_symbol("jet_jit_channel_new"),
    CoreCallRecord::new("core.tasks", super::INTERNAL_CHANNEL_BOUNDED_METHOD, "jet_std::channel_bounded", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_jit_symbol("jet_jit_channel_bounded"),
    CoreCallRecord::new( "core.reactive", "signal", "jet_std::JetSignal::new", true, &[false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.reactive.loadable", "idle", "jet_loadable_idle", false, &[]).with_jit_symbol("jet_jit_loadable_idle"),
    CoreCallRecord::new("core.reactive.loadable", "loading", "jet_loadable_loading", false, &[]).with_jit_symbol("jet_jit_loadable_loading"),
    CoreCallRecord::new("core.reactive.loadable", "loaded", "jet_loadable_loaded", false, &[false]).with_jit_symbol("jet_jit_loadable_loaded"),
    CoreCallRecord::new("core.reactive.loadable", "failed", "jet_loadable_failed", false, &[false]).with_jit_symbol("jet_jit_loadable_failed"),
    CoreCallRecord::new( "core.event", "scope", "jet_std::JetEventScope::new", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.event", "decision_hook", "jet_std::JetDecisionHook::new", true, &[false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .without_direct_jit(),
    CoreCallRecord::new("core.event", "hook", "jet_std::JetHook::new", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_jit_symbol("jet_jit_hook_new"),
    CoreCallRecord::new( "core.event", "policy_sync", "jet_std::JetEventPolicy::sync", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.event", "new", "jet_std::JetEvent::new", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.event", "with_policy", "jet_std::JetEvent::with_policy", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.event", "async_result", "jet_std::JetAsyncEvent::new", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_jit_symbol("jet_jit_async_event_new"),
    CoreCallRecord::new( "core.math", "fraction", "jet_fraction_new", true, &[false, false], ) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "decimal", "jet_decimal_from_str", true, &[true], ) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.files", "read", "jet_std_fs_read", true, &[true]),
    CoreCallRecord::new("core.files", "scope", "jet_std_fs_scope", true, &[true]),
    CoreCallRecord::new( "core.files", "read_bytes", "jet_std_fs_read_bytes", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "map", "jet_std_fs_map", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "write", "jet_std_fs_write", true, &[true, true], ),
    CoreCallRecord::new( "core.files", "write_bytes", "jet_std_fs_write_bytes", true, &[true, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "append_all", "jet_std_fs_append", true, &[true, true], ) .with_jit_symbol("jet_jit_fs_append_all"),
    CoreCallRecord::new("core.files", "exists", "jet_std_fs_exists", true, &[true]),
    CoreCallRecord::new("core.files", "remove", "jet_std_fs_remove", true, &[true]),
    CoreCallRecord::new( "core.files", "remove_dir", "jet_std_fs_remove_dir", true, &[true], ) .with_jit_symbol("jet_jit_fs_remove_dir"),
    CoreCallRecord::new( "core.files", "remove_all", "jet_std_fs_remove_all", true, &[true], ),
    CoreCallRecord::new( "core.files", "list_dir", "jet_std_fs_list_dir", true, &[true], ),
    CoreCallRecord::new( "core.files", "create_dir", "jet_std_fs_create_dir", true, &[true], ) .with_jit_symbol("jet_jit_fs_create_dir"),
    CoreCallRecord::new( "core.files", "create_dir_all", "jet_std_fs_create_dir_all", true, &[true], ) .with_jit_symbol("jet_jit_fs_create_dir_all"),
    CoreCallRecord::new("core.files", "is_dir", "jet_std_fs_is_dir", true, &[true]) .with_jit_symbol("jet_jit_fs_is_dir"),
    CoreCallRecord::new("core.files", "copy", "jet_std_fs_copy", true, &[true, true]) .with_jit_symbol("jet_jit_fs_copy"),
    CoreCallRecord::new( "core.files", "copy_dir", "jet_std_fs_copy_dir", true, &[true, true], ) .with_jit_symbol("jet_jit_fs_copy_dir"),
    CoreCallRecord::new( "core.files", "rename", "jet_std_fs_rename", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "symlink", "jet_std_fs_symlink", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "read_link", "jet_std_fs_read_link", true, &[true], ) .with_jit_symbol("jet_jit_fs_read_link") .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "hard_link", "jet_std_fs_hard_link", true, &[true, true], ) .with_jit_symbol("jet_jit_fs_hard_link") .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "stat", "jet_std_fs_stat", true, &[true]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "lstat", "jet_std_fs_stat", true, &[true]) .with_jit_symbol("jet_jit_fs_stat") .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "is_fifo", "jet_std_fs_is_fifo", true, &[true]) .with_jit_symbol("jet_jit_fs_is_fifo") .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "is_socket", "jet_std_fs_is_socket", true, &[true]) .with_jit_symbol("jet_jit_fs_is_socket") .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "chown", "jet_std_fs_chown", true, &[true, false, false]) .with_jit_symbol("jet_jit_fs_chown") .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "mkdtemp", "jet_std_fs_temp_dir_path", true, &[true]) .with_jit_symbol("jet_jit_fs_mkdtemp") .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "mktemp", "jet_std_fs_mktemp_path", true, &[true]) .with_jit_symbol("jet_jit_fs_mktemp_path") .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "set_mode", "jet_std_fs_set_mode", true, &[true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "canonicalize", "jet_std_fs_canonicalize", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "absolute", "jet_std_fs_absolute", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "walk", "jet_std_fs_walk", true, &[true, false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "walk_parallel", "jet_std_fs_walk_parallel", true, &[true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "walk_files", "jet_std_fs_walk_files", true, &[true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "glob", "jet_std_fs_glob", true, &[true]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "read_at", "jet_std_fs_read_at", true, &[true, false, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "write_at", "jet_std_fs_write_at", true, &[true, false, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "fsync", "jet_std_fs_fsync", true, &[true]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "write_atomic", "jet_std_fs_write_atomic", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "temp_dir", "jet_std_fs_temp_dir", true, &[true], ) .with_jit_symbol("jet_jit_fs_temp_dir").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "temp_file", "jet_std_fs_temp_file", true, &[true], ) .with_jit_symbol("jet_jit_fs_temp_file").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "lock", "jet_std_fs_lock", true, &[true]) .with_jit_symbol("jet_jit_fs_lock").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.watcher", "files", "jet_watcher_files", true, &[true]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.watcher", "process_pid", "jet_watcher_process_pid", true, &[false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.watcher", "port", "jet_watcher_port", true, &[true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.watcher", "set", "jet_watcher_set", true, &[]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.args", "decode", "jet_args_decode", true, &[]) .without_direct_jit(),
    CoreCallRecord::new("core.args", "merge", "jet_config_merge", true, &[true, true]) .without_direct_jit(),
    CoreCallRecord::new("core.args", "spec", "jet_args_spec", true, &[]),
    CoreCallRecord::new("core.term", "confirm", "jet_std_io_confirm", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.term", "choose", "jet_std_io_choose", true, &[true, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "eprint", "jet_std_io_eprint", true, &[true]) .with_jit_symbol("jet_jit_eprint_str"),
    CoreCallRecord::new("core.term", "input", "jet_std_io_input", true, &[]) .with_max_arity(1),
    CoreCallRecord::new( "core.term", "input_secret", "jet_std_io_input_secret", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.term", "read_all_input", "jet_std_io_read_all_input", true, &[], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "readline", "jet_std_io_readline", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.term", "read_until", "jet_std_io_read_until", true, &[true], ),
    CoreCallRecord::new("core.term", "take", "jet_std_io_take", true, &[false]),
    CoreCallRecord::new("core.term", "buffered", "jet_std_io_buffered", true, &[]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "binread", "jet_std_io_binread", true, &[true]),
    CoreCallRecord::new( "core.term", "binwrite", "jet_std_io_binwrite", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .without_direct_jit(),
    CoreCallRecord::new("core.term", "stdin", "jet_std_io_stdin", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "stdout", "jet_std_io_stdout", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "stderr", "jet_std_io_stderr", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.term", "terminal_width", "jet_std_io_terminal_width", true, &[], ) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.term", "terminal_height", "jet_std_io_terminal_height", true, &[], ) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.term", "style", "jet_std_io_style", true, &[true, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.term", "style_force", "jet_std_io_style_force", true, &[true, true], ) .with_pure_route(CoreCallPureRoute::Io),
    CoreCallRecord::new("core.term", "progress", "jet_std_io_progress", true, &[true]).with_max_arity(3).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "progress_iter", "jet_std_io_progress_iter", true, &[false, true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "get", "jet_std_env_get", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "set", "jet_std_env_set", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "unset", "jet_std_env_unset", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "vars", "jet_std_env_vars", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.sys", "current_dir", "jet_std_env_current_dir", true, &[], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "home_dir", "jet_std_env_home_dir", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "name", "jet_std_os_name", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "family", "jet_std_os_family", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "arch", "jet_std_os_arch", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "cpu_count", "jet_std_os_cpu_count", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "temp_dir", "jet_std_os_temp_dir", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "executable", "jet_std_os_executable", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "pid", "jet_std_os_pid", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "getpid", "jet_std_os_pid", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "hostname", "jet_std_os_hostname", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "username", "jet_std_os_username", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "release", "jet_std_os_release", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "version", "jet_std_os_version", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "expand", "jet_std_os_expand", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "getppid", "jet_std_os_getppid", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "getuid", "jet_std_os_getuid", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "geteuid", "jet_std_os_geteuid", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "getgid", "jet_std_os_getgid", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "getegid", "jet_std_os_getegid", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "getgroups", "jet_std_os_getgroups", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "getpgrp", "jet_std_os_getpgrp", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "uptime", "jet_std_os_uptime", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "loadavg", "jet_std_os_loadavg", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "times", "jet_std_os_times", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "sync", "jet_std_os_sync", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "getpgid", "jet_std_os_getpgid", true, &[false]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "getsid", "jet_std_os_getsid", true, &[false]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.sys", "exitcode", "jet_std_os_exitcode", true, &[false], ) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "umask", "jet_std_os_umask", true, &[false]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.sys", "getpriority", "jet_std_os_getpriority", true, &[false], ) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.sys", "setpriority", "jet_std_os_setpriority", true, &[false, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.sys", "utime", "jet_std_os_utime", true, &[true, false, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "stop", "jet_std_os_stop", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "atexit", "jet_std_os_atexit", true, &[false]),
    CoreCallRecord::new("core.sys", "decode", "jet_std_env_decode", true, &[true, true, true]) .without_direct_jit(),
    CoreCallRecord::new( "core.sys", "set_current_dir", "jet_std_os_set_current_dir", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "fork", "jet_std_os_fork", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "setuid", "jet_std_os_setuid", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "setgid", "jet_std_os_setgid", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.sys", "setpgid", "jet_std_os_setpgid", true, &[false, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "setpgrp", "jet_std_os_setpgrp", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "setsid", "jet_std_os_setsid", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.sys", "initgroups", "jet_std_os_initgroups", true, &[true, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "kill", "jet_std_os_kill", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "wait", "jet_std_os_wait", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.sys", "waitpid", "jet_std_os_waitpid", true, &[false, false], ) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "pipe", "jet_std_os_pipe", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.sys", "close_fd", "jet_std_os_close_fd", true, &[false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.sys", "mkfifo", "jet_std_os_mkfifo", true, &[true, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.process", "exit", "jet_std_process_exit", true, &[false], ) .with_jit_symbol("jet_jit_process_exit"),
    CoreCallRecord::new("core.process", "cmd", "jet_std_process_cmd", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.process", "pipeline", "jet_std_process_pipeline", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.process", "argv", "jet_std_io_args", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.process", "args", "jet_std_io_process_args", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.process", "on_signal", "jet_process_on_signal", true, &[true], ) .with_jit_symbol("jet_jit_process_on_signal"),
    CoreCallRecord::new("core.plugin", "load", "jet_plugin_load", true, &[true, true]) .without_direct_aot() .without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.mod", "load", "jet_mod_load", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.testing", "snap", "jet_testing_snap", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.testing", "golden", "jet_testing_golden", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.testing", "fixture", "jet_testing_fixture", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.testing", "temp_dir", "jet_testing_temp_dir", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.testing", "corpus", "jet_testing_corpus", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.testing", "fake_clock", "jet_std_clock_new", true, &[false], ),
    CoreCallRecord::new( "core.testing", "fake_rng", "jet_std_rng_new", true, &[false], ),
    CoreCallRecord::new( "core.testing", "fake_data", "jet_testing_fake_new", true, &[false], ) .with_jit_symbol("jet_jit_fake_new"),
    CoreCallRecord::new( "core.testing", "test_suite", "jet_test_suite_new", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.testing", "compare", "jet_testing_compare", true, &[true, true, true, true]) .with_jit_symbol("jet_jit_testing_compare").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.testing", "histories", "jet_testing_histories", true, &[true, true, false, true, true, true]) .with_max_arity(6) .with_jit_symbol("jet_jit_testing_histories").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.testing", "assert_equal", "jet_testing_assert_equal", true, &[true]) .with_jit_symbol("jet_jit_testing_assert_equal").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.testing", "status", "jet_testing_status", true, &[true]) .with_jit_symbol("jet_jit_testing_status").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.testing", "world", "jet_testing_world", true, &[false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.math", "pi", "jet_std_math_pi", true, &[]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "round", "jet_std_math_round", true, &[false]) .with_native_int_result(),
    CoreCallRecord::new("core.math", "sin", "jet_std_math_sin", true, &[false]) .with_jit_symbol("jet_jit_math_sin") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "cos", "jet_std_math_cos", true, &[false]) .with_jit_symbol("jet_jit_math_cos") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "tan", "jet_std_math_tan", true, &[false]) .with_jit_symbol("jet_jit_math_tan") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "asin", "jet_std_math_asin", true, &[false]) .with_jit_symbol("jet_jit_math_asin") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "acos", "jet_std_math_acos", true, &[false]) .with_jit_symbol("jet_jit_math_acos") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "atan", "jet_std_math_atan", true, &[false]) .with_jit_symbol("jet_jit_math_atan") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "sinh", "jet_std_math_sinh", true, &[false]) .with_jit_symbol("jet_jit_math_sinh") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "cosh", "jet_std_math_cosh", true, &[false]) .with_jit_symbol("jet_jit_math_cosh") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "tanh", "jet_std_math_tanh", true, &[false]) .with_jit_symbol("jet_jit_math_tanh") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "ln", "jet_std_math_ln", true, &[false]) .with_jit_symbol("jet_jit_math_ln") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "log10", "jet_std_math_log10", true, &[false]) .with_jit_symbol("jet_jit_math_log10") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "log2", "jet_std_math_log2", true, &[false]) .with_jit_symbol("jet_jit_math_log2") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "trunc", "jet_std_math_trunc", true, &[false]) .with_jit_symbol("jet_jit_math_trunc") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "fract", "jet_std_math_fract", true, &[false]) .with_jit_symbol("jet_jit_math_fract") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "floor", "jet_std_math_floor", true, &[false]) .with_jit_symbol("jet_jit_math_floor") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "atan2", "jet_std_math_atan2", true, &[false, false]) .with_jit_symbol("jet_jit_math_atan2") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "ceil", "jet_std_math_ceil", true, &[false]) .with_jit_symbol("jet_jit_math_ceil") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "hypot", "jet_std_math_hypot", true, &[false, false]) .with_jit_symbol("jet_jit_math_hypot") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "pow", "jet_std_math_pow", true, &[false, false]) .with_jit_symbol("jet_jit_math_pow") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "sqrt", "jet_std_math_sqrt", true, &[false]) .with_pure_route(CoreCallPureRoute::Math) .without_direct_aot(),
    CoreCallRecord::new( "core.math", "is_finite", "jet_std_math_is_finite", true, &[false], ) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "fabs", "jet_std_math_abs_f64", true, &[false], ) .with_jit_symbol("jet_jit_math_abs_f64") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "isnan", "jet_std_math_is_nan", true, &[false], ) .with_jit_symbol("jet_jit_math_is_nan") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "isinf", "jet_std_math_is_infinite", true, &[false], ) .with_jit_symbol("jet_jit_math_is_infinite") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "isfinite", "jet_std_math_is_finite", true, &[false], ) .with_jit_symbol("jet_jit_math_is_finite") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "to_bits", "jet_std_math_to_bits", true, &[false], ) .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new( "core.math", "from_bits", "jet_std_math_from_bits", true, &[false], ) .with_pure_route(CoreCallPureRoute::Math) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.math", "factorial", "jet_std_math_factorial", true, &[false], ) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "factorial", "jet_std_math_factorial", true, &[false]).with_jit_symbol("jet_jit_math_factorial") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "binomial", "jet_std_math_binomial", true, &[false, false]).with_jit_symbol("jet_jit_math_binomial") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "permutations_count", "jet_std_math_perm", true, &[false, false]).with_jit_symbol("jet_jit_math_perm") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "combinations_count", "jet_std_math_binomial", true, &[false, false]).with_jit_symbol("jet_jit_math_binomial") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "npr", "jet_std_math_perm", true, &[false, false]).with_jit_symbol("jet_jit_math_perm") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "ncr", "jet_std_math_binomial", true, &[false, false]).with_jit_symbol("jet_jit_math_binomial") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "falling_factorial", "jet_std_math_perm", true, &[false, false]).with_jit_symbol("jet_jit_math_perm") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "rising_factorial", "jet_std_math_rising_factorial", true, &[false, false]).with_jit_symbol("jet_jit_math_rising_factorial") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "multinomial", "jet_std_math_multinomial", true, &[true]).with_jit_symbol("jet_jit_math_multinomial") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "chain", "jet_comb_chain", true, &[true, true]).with_jit_symbol("jet_jit_comb_chain") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "compress", "jet_comb_compress", true, &[true, true]).with_jit_symbol("jet_jit_comb_compress") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "drop", "jet_comb_drop", true, &[true, false]).with_jit_symbol("jet_jit_comb_drop") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "takewhile", "jet_comb_takewhile", true, &[true, false]).with_jit_symbol("jet_jit_comb_takewhile") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "dropwhile", "jet_comb_dropwhile", true, &[true, false]).with_jit_symbol("jet_jit_comb_dropwhile") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "filterfalse", "jet_comb_filterfalse", true, &[true, false]).with_jit_symbol("jet_jit_comb_filterfalse") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "islice", "jet_comb_islice", true, &[true, false, false, false]).with_jit_symbol("jet_jit_comb_islice") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "unique", "jet_comb_unique", true, &[true]).with_jit_symbol("jet_jit_comb_unique") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "repeat", "jet_comb_repeat", true, &[false, false]).with_jit_symbol("jet_jit_comb_repeat") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "count_from", "jet_comb_count_from", true, &[false, false, false]).with_jit_symbol("jet_jit_comb_count_from") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "count", "jet_comb_count_from", true, &[false, false, false]).with_jit_symbol("jet_jit_comb_count_from") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "cycle", "jet_comb_cycle", true, &[true, false]).with_jit_symbol("jet_jit_comb_cycle") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "accumulate", "jet_comb_accumulate", true, &[true]).with_jit_symbol("jet_jit_comb_accumulate") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "reverse", "jet_comb_reverse", true, &[true]).with_jit_symbol("jet_jit_comb_reverse") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "permutations", "jet_comb_permutations", true, &[true, false]).with_jit_symbol("jet_jit_comb_permutations") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "combinations", "jet_comb_combinations", true, &[true, false]).with_jit_symbol("jet_jit_comb_combinations") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "combinations_with_replacement", "jet_comb_combinations_with_replacement", true, &[true, false]).with_jit_symbol("jet_jit_comb_combinations_with_replacement") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "product", "jet_comb_product", true, &[true, false]).with_jit_symbol("jet_jit_comb_product") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "cartesian", "jet_comb_cartesian", true, &[true, true]).with_jit_symbol("jet_jit_comb_cartesian") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "pairwise", "jet_comb_pairwise", true, &[true]).with_jit_symbol("jet_jit_comb_pairwise") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "batched", "jet_comb_batched", true, &[true, false]).with_jit_symbol("jet_jit_comb_batched") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "groupby", "jet_comb_groupby", true, &[true]).with_jit_symbol("jet_jit_comb_groupby") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "tee", "jet_comb_tee", true, &[true, false]).with_jit_symbol("jet_jit_comb_tee") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "zip_longest", "jet_comb_zip_longest", true, &[true, true, false]).with_jit_symbol("jet_jit_comb_zip_longest") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "powerset", "jet_comb_powerset", true, &[true]).with_jit_symbol("jet_jit_comb_powerset") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "windows", "jet_comb_windows", true, &[true, false]).with_jit_symbol("jet_jit_comb_windows") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "flatten", "jet_comb_flatten", true, &[true]).with_jit_symbol("jet_jit_comb_flatten") .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "starmap", "jet_comb_starmap", true, &[true]).with_jit_symbol("jet_jit_comb_starmap") .with_native_int_result(),
    CoreCallRecord::new("core.collections", "heapify", "jet_coll_heapify", true, &[true]).with_jit_symbol("jet_jit_coll_heapify") .with_native_int_result(),
    CoreCallRecord::new("core.collections", "heappush", "jet_coll_heappush", true, &[true, false]).with_jit_symbol("jet_jit_coll_heappush") .with_native_int_result(),
    CoreCallRecord::new("core.collections", "heappop", "jet_coll_heappop", true, &[true]).with_jit_symbol("jet_jit_coll_heappop"),
    CoreCallRecord::new("core.collections", "heappushpop", "jet_coll_heappushpop", true, &[true, false]).with_jit_symbol("jet_jit_coll_heappushpop"),
    CoreCallRecord::new("core.collections", "heapreplace", "jet_coll_heapreplace", true, &[true, false]).with_jit_symbol("jet_jit_coll_heapreplace"),
    CoreCallRecord::new("core.collections", "bisect_left", "jet_coll_bisect_left", true, &[true, false]).with_jit_symbol("jet_jit_coll_bisect_left") .with_native_int_result(),
    CoreCallRecord::new("core.collections", "bisect_right", "jet_coll_bisect_right", true, &[true, false]).with_jit_symbol("jet_jit_coll_bisect_right") .with_native_int_result(),
    CoreCallRecord::new("core.collections", "insort_left", "jet_coll_insort_left", true, &[true, false]).with_jit_symbol("jet_jit_coll_insort_left") .with_native_int_result(),
    CoreCallRecord::new("core.collections", "insort_right", "jet_coll_insort_right", true, &[true, false]).with_jit_symbol("jet_jit_coll_insort_right") .with_native_int_result(),
    CoreCallRecord::new("core.collections", "merge_sorted", "jet_coll_merge_sorted", true, &[true, true]).with_jit_symbol("jet_jit_coll_merge_sorted") .with_native_int_result(),
    CoreCallRecord::new("core.collections", "nsmallest", "jet_coll_nsmallest", true, &[false, true]).with_jit_symbol("jet_jit_coll_nsmallest") .with_native_int_result(),
    CoreCallRecord::new("core.collections", "nlargest", "jet_coll_nlargest", true, &[false, true]).with_jit_symbol("jet_jit_coll_nlargest") .with_native_int_result(),
    CoreCallRecord::new("core.collections", "counter", "jet_coll_counter", true, &[]).with_jit_symbol("jet_jit_coll_counter").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "counter_from", "jet_coll_counter_from", true, &[true]).with_jit_symbol("jet_jit_coll_counter_from").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "add", "jet_coll_counter_add", true, &[true, true, false]).with_jit_symbol("jet_jit_coll_counter_add").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "inc", "jet_coll_counter_inc", true, &[true, true]).with_jit_symbol("jet_jit_coll_counter_inc").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "dec", "jet_coll_counter_dec", true, &[true, true]).with_jit_symbol("jet_jit_coll_counter_dec").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "get", "jet_coll_counter_get", true, &[true, true]).with_jit_symbol("jet_jit_coll_counter_get").with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new("core.collections", "set_count", "jet_coll_counter_set_count", true, &[true, true, false]).with_jit_symbol("jet_jit_coll_counter_set_count").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "total", "jet_coll_counter_total", true, &[true]).with_jit_symbol("jet_jit_coll_counter_total").with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new("core.collections", "names", "jet_coll_counter_names", true, &[true]).with_jit_symbol("jet_jit_coll_counter_names").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "elements", "jet_coll_counter_elements", true, &[true]).with_jit_symbol("jet_jit_coll_counter_elements").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "most_common", "jet_coll_counter_most_common", true, &[true, false]).with_jit_symbol("jet_jit_coll_counter_most_common").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "subtract", "jet_coll_counter_subtract", true, &[true, true]).with_jit_symbol("jet_jit_coll_counter_subtract").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "merge_add", "jet_coll_counter_merge_add", true, &[true, true]).with_jit_symbol("jet_jit_coll_counter_merge_add").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "clear_counter", "jet_coll_counter_clear", true, &[true]).with_jit_symbol("jet_jit_coll_counter_clear").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "ordered_map", "jet_coll_ordered_map", true, &[]).with_jit_symbol("jet_jit_coll_ordered_map").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "map_get", "jet_coll_map_get", true, &[true, true]).with_jit_symbol("jet_jit_coll_map_get").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "map_set", "jet_coll_map_set", true, &[true, true, true]).with_jit_symbol("jet_jit_coll_map_set").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "map_remove", "jet_coll_map_remove", true, &[true, true]).with_jit_symbol("jet_jit_coll_map_remove").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "map_keys", "jet_coll_map_keys", true, &[true]).with_jit_symbol("jet_jit_coll_map_keys").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "map_values", "jet_coll_map_values", true, &[true]).with_jit_symbol("jet_jit_coll_map_values").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "map_contains", "jet_coll_map_contains", true, &[true, true]).with_jit_symbol("jet_jit_coll_map_contains").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "map_len", "jet_coll_map_len", true, &[true]).with_jit_symbol("jet_jit_coll_map_len").with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new("core.collections", "chain", "jet_coll_chain", true, &[]).with_jit_symbol("jet_jit_coll_chain").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "chain_push", "jet_coll_chain_push", true, &[true, true, true]).with_jit_symbol("jet_jit_coll_chain_push").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "chain_get", "jet_coll_chain_get", true, &[true, true]).with_jit_symbol("jet_jit_coll_chain_get").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "chain_contains", "jet_coll_chain_contains", true, &[true, true]).with_jit_symbol("jet_jit_coll_chain_contains").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.math", "erf", "jet_std_math_erf", true, &[false], ),
    CoreCallRecord::new("core.math", "erfc", "jet_std_math_erfc", true, &[false]),
    CoreCallRecord::new("core.math", "gamma", "jet_std_math_gamma", true, &[false]),
    CoreCallRecord::new("core.math", "lgamma", "jet_std_math_lgamma", true, &[false]),
    CoreCallRecord::new("core.math", "logb", "jet_std_math_logb", true, &[false]),
    CoreCallRecord::new( "core.math", "significand", "jet_std_math_significand", true, &[false], ),
    CoreCallRecord::new("core.math", "ulp", "jet_std_math_ulp", true, &[false]),
    CoreCallRecord::new( "core.math", "cmp", "jet_std_math_cmp", true, &[false, false], ) .with_native_int_result(),
    CoreCallRecord::new( "core.math", "next_after", "jet_std_math_next_after", true, &[false, false], ),
    CoreCallRecord::new( "core.math", "nextafter", "jet_std_math_next_after", true, &[false, false], ) .with_jit_symbol("jet_jit_math_next_after") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "sumprod", "jet_std_math_sumprod", true, &[false, false], ) .with_jit_symbol("jet_jit_list_sumprod_f64") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "ldexp", "jet_std_math_ldexp", true, &[false, false], ),
    CoreCallRecord::new( "core.math", "scaleb", "jet_std_math_ldexp", true, &[false, false], ),
    CoreCallRecord::new("core.math", "ilogb", "jet_std_math_ilogb", true, &[false]) .with_native_int_result(),
    CoreCallRecord::new( "core.math", "leading_ones", "jet_std_math_leading_ones", true, &[false], ) .with_native_int_result(),
    CoreCallRecord::new( "core.math", "trailing_ones", "jet_std_math_trailing_ones", true, &[false], ) .with_native_int_result(),
    CoreCallRecord::new("core.math", "digits", "jet_std_math_digits", true, &[false]) .with_native_int_result(),
    CoreCallRecord::new( "core.math", "binomial", "jet_std_math_binomial", true, &[false, false], ) .with_native_int_result(),
    CoreCallRecord::new( "core.math", "comb", "jet_std_math_binomial", true, &[false, false], ) .with_jit_symbol("jet_jit_math_binomial") .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new( "core.math", "perm", "jet_std_math_perm", true, &[false, false], ) .with_jit_symbol("jet_jit_math_perm") .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new( "core.math", "fmod", "jet_std_math_fmod", true, &[false, false], ) .with_jit_symbol("jet_jit_math_fmod") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "remainder", "jet_std_math_remainder", true, &[false, false], ) .with_jit_symbol("jet_jit_math_remainder") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "isclose", "jet_std_math_isclose", true, &[false, false, false, false], ) .with_jit_symbol("jet_jit_math_isclose") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "dist", "jet_std_math_dist", true, &[false, false], ) .with_jit_symbol("jet_jit_math_dist") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "checked_pow", "jet_std_math_checked_pow", true, &[false, false], ) .with_native_int_result(),
    CoreCallRecord::new( "core.math", "int_pow", "jet_std_math_int_pow", true, &[false, false], ) .with_native_int_result(),
    CoreCallRecord::new("core.math", "asinh", "jet_std_math_asinh", true, &[false]) .with_jit_symbol("jet_jit_math_asinh") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "acosh", "jet_std_math_acosh", true, &[false]) .with_jit_symbol("jet_jit_math_acosh") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "atanh", "jet_std_math_atanh", true, &[false]) .with_jit_symbol("jet_jit_math_atanh") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "cbrt", "jet_std_math_cbrt", true, &[false]) .with_jit_symbol("jet_jit_math_cbrt") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "exp", "jet_std_math_exp", true, &[false]) .with_jit_symbol("jet_jit_math_exp") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "exp2", "jet_std_math_exp2", true, &[false]) .with_jit_symbol("jet_jit_math_exp2") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "exp_m1", "jet_std_math_exp_m1", true, &[false]) .with_jit_symbol("jet_jit_math_exp_m1") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "expm1", "jet_std_math_exp_m1", true, &[false], ) .with_jit_symbol("jet_jit_math_exp_m1") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "fsum", "jet_std_math_fsum", true, &[false], ) .with_jit_symbol("jet_jit_list_sum_fixed_f64") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "prod", "jet_std_math_prod_int", true, &[false], ) .with_jit_symbol("jet_jit_math_prod_int") .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "ln_1p", "jet_std_math_ln_1p", true, &[false]) .with_jit_symbol("jet_jit_math_ln_1p") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "log", "jet_std_math_log", true, &[false, false]) .with_jit_symbol("jet_jit_math_log") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "copysign", "jet_std_math_copysign", true, &[false, false]) .with_jit_symbol("jet_jit_math_copysign") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "copy", "jet_std_math_copy", true, &[false]) .with_jit_symbol("jet_jit_math_copy") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "signum", "jet_std_math_signum", true, &[false]) .with_jit_symbol("jet_jit_math_signum") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "checked_add", "jet_std_math_checked_add", true, &[false, false]) .with_jit_symbol("jet_jit_math_checked_add") .with_native_int_result(),
    CoreCallRecord::new("core.math", "checked_sub", "jet_std_math_checked_sub", true, &[false, false]) .with_jit_symbol("jet_jit_math_checked_sub") .with_native_int_result(),
    CoreCallRecord::new("core.math", "checked_mul", "jet_std_math_checked_mul", true, &[false, false]) .with_jit_symbol("jet_jit_math_checked_mul") .with_native_int_result(),
    CoreCallRecord::new("core.math", "saturating_sub", "jet_std_math_saturating_sub", true, &[false, false]) .with_jit_symbol("jet_jit_math_saturating_sub") .with_native_int_result(),
    CoreCallRecord::new("core.math", "saturating_mul", "jet_std_math_saturating_mul", true, &[false, false]) .with_jit_symbol("jet_jit_math_saturating_mul") .with_native_int_result(),
    CoreCallRecord::new("core.math", "is_nan", "jet_std_math_is_nan", true, &[false]) .with_jit_symbol("jet_jit_math_is_nan") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "is_inf", "jet_std_math_is_infinite", true, &[false]) .with_jit_symbol("jet_jit_math_is_infinite") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "tau_const", "jet_std_math_tau", true, &[]) .with_jit_symbol("jet_jit_math_tau") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "hypot3", "jet_std_math_hypot3", true, &[false, false, false]) .with_jit_symbol("jet_jit_math_hypot3") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "clamp_float", "jet_std_math_clamp_f64", true, &[false, false, false]) .with_jit_symbol("jet_jit_math_clamp_f64") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "min_float", "jet_std_math_min_f64", true, &[false, false]) .with_jit_symbol("jet_jit_math_min_f64") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "max_float", "jet_std_math_max_f64", true, &[false, false]) .with_jit_symbol("jet_jit_math_max_f64") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "midpoint", "jet_std_math_midpoint", true, &[false, false]) .with_jit_symbol("jet_jit_math_midpoint") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "gcd_many", "jet_std_math_gcd_many", true, &[true]) .with_jit_symbol("jet_jit_math_gcd_many") .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "lcm_many", "jet_std_math_lcm_many", true, &[true]) .with_jit_symbol("jet_jit_math_lcm_many") .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "powmod", "jet_std_math_powmod", true, &[false, false, false]) .with_jit_symbol("jet_jit_math_powmod") .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "sum_int", "jet_std_math_sum_int", true, &[true]) .with_jit_symbol("jet_jit_math_sum_int") .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "prod_int", "jet_std_math_prod_int_ref", true, &[true]) .with_jit_symbol("jet_jit_math_prod_int") .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "abs_diff", "jet_std_math_abs_diff", true, &[false, false]) .with_jit_symbol("jet_jit_math_abs_diff") .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "even", "jet_std_math_is_even", true, &[false]) .with_jit_symbol("jet_jit_math_is_even") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "odd", "jet_std_math_is_odd", true, &[false]) .with_jit_symbol("jet_jit_math_is_odd") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "in_range", "jet_std_math_in_range", true, &[false, false, false]) .with_jit_symbol("jet_jit_math_in_range") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "log1p", "jet_std_math_ln_1p", true, &[false]) .with_jit_symbol("jet_jit_math_ln_1p") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "xor", "jet_std_math_xor", true, &[false, false]) .with_jit_symbol("jet_jit_math_xor") .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "random", "jet_std_random_float", true, &[]) .with_jit_symbol("jet_jit_random_float"),
    CoreCallRecord::new("core.math", "saturating_add", "jet_std_math_saturating_add", true, &[false, false]) .with_jit_symbol("jet_jit_math_saturating_add") .with_native_int_result(),
    CoreCallRecord::new("core.math", "fma", "jet_std_math_fma", true, &[false, false, false]) .with_jit_symbol("jet_jit_math_fma") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "muladd", "jet_std_math_fma", true, &[false, false, false]) .with_jit_symbol("jet_jit_math_fma") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "real", "jet_std_math_identity_f64", true, &[false]) .with_jit_symbol("jet_jit_math_identity_f64") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "imag", "jet_std_math_zero_f64", true, &[false]) .with_jit_symbol("jet_jit_math_zero_f64") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "conj", "jet_std_math_identity_f64", true, &[false]) .with_jit_symbol("jet_jit_math_identity_f64") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "float32", "jet_std_math_identity_f64", true, &[false]) .with_jit_symbol("jet_jit_math_identity_f64") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "float64", "jet_std_math_identity_f64", true, &[false]) .with_jit_symbol("jet_jit_math_identity_f64") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "truncate", "jet_std_math_trunc", true, &[false]) .with_jit_symbol("jet_jit_math_trunc") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "degrees", "jet_std_math_degrees", true, &[false]) .with_jit_symbol("jet_jit_math_degrees") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "radians", "jet_std_math_radians", true, &[false]) .with_jit_symbol("jet_jit_math_radians") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "lerp", "jet_std_math_lerp", true, &[false, false, false]) .with_jit_symbol("jet_jit_math_lerp") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "checked_abs", "jet_std_math_checked_abs", true, &[false]) .with_jit_symbol("jet_jit_math_checked_abs") .with_native_int_result(),
    CoreCallRecord::new("core.math", "checked_neg", "jet_std_math_checked_neg", true, &[false]) .with_jit_symbol("jet_jit_math_checked_neg") .with_native_int_result(),
    CoreCallRecord::new("core.math", "checked_div", "jet_std_math_checked_div", true, &[false, false]) .with_jit_symbol("jet_jit_math_checked_div") .with_native_int_result(),
    CoreCallRecord::new("core.math", "checked_rem", "jet_std_math_checked_rem", true, &[false, false]) .with_jit_symbol("jet_jit_math_checked_rem") .with_native_int_result(),
    CoreCallRecord::new("core.math", "is_normal", "jet_std_math_is_normal", true, &[false]) .with_jit_symbol("jet_jit_math_is_normal") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "is_subnormal", "jet_std_math_is_subnormal", true, &[false]) .with_jit_symbol("jet_jit_math_is_subnormal") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "is_canonical", "jet_std_math_is_canonical", true, &[false]) .with_jit_symbol("jet_jit_math_is_canonical") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "is_signed", "jet_std_math_is_signed", true, &[false]) .with_jit_symbol("jet_jit_math_is_signed") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "is_zero", "jet_std_math_is_zero", true, &[false]) .with_jit_symbol("jet_jit_math_is_zero_f") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "is_integer", "jet_std_math_is_integer", true, &[false]) .with_jit_symbol("jet_jit_math_is_integer") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "sign_bit", "jet_std_math_sign_bit", true, &[false]) .with_jit_symbol("jet_jit_math_is_signed") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "next_up", "jet_std_math_next_up", true, &[false]) .with_jit_symbol("jet_jit_math_next_up") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "next_down", "jet_std_math_next_down", true, &[false]) .with_jit_symbol("jet_jit_math_next_down") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "radix", "jet_std_math_radix", true, &[false]) .with_jit_symbol("jet_jit_math_radix") .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "zero", "jet_std_math_zero", true, &[]) .with_jit_symbol("jet_jit_math_zero") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "cot", "jet_std_math_cot", true, &[false]) .with_jit_symbol("jet_jit_math_cot") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "inv", "jet_std_math_inv", true, &[false]) .with_jit_symbol("jet_jit_math_inv") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "sin_cos", "jet_std_math_sin_cos", true, &[false]) .with_jit_symbol("jet_jit_math_sin_cos") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "modf", "jet_std_math_modf", true, &[false]) .with_jit_symbol("jet_jit_math_modf") .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "frexp", "jet_std_math_frexp", true, &[false]) .with_jit_symbol("jet_jit_math_frexp") .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "div_mod", "jet_std_math_div_mod", true, &[false, false]) .with_jit_symbol("jet_jit_math_div_mod") .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "div_rem", "jet_std_math_div_rem", true, &[false, false]) .with_jit_symbol("jet_jit_math_div_rem") .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new( "core.math.stats", "sum", "jet_stats_sum", true, &[true], ) .with_jit_symbol("jet_jit_stats_sum"),
    CoreCallRecord::new( "core.math.stats", "prod", "jet_stats_prod", true, &[true], ) .with_jit_symbol("jet_jit_stats_prod"),
    CoreCallRecord::new( "core.math.stats", "count", "jet_stats_count", true, &[true], ) .with_jit_symbol("jet_jit_stats_count") .with_native_int_result(),
    CoreCallRecord::new( "core.math.stats", "cumsum", "jet_stats_cumsum", true, &[true], ) .with_jit_symbol("jet_jit_stats_cumsum"),
    CoreCallRecord::new( "core.math.stats", "cumprod", "jet_stats_cumprod", true, &[true], ) .with_jit_symbol("jet_jit_stats_cumprod"),
    CoreCallRecord::new( "core.math.stats", "diff", "jet_stats_diff", true, &[true], ) .with_jit_symbol("jet_jit_stats_diff"),
    CoreCallRecord::new( "core.math.stats", "moving_average", "jet_stats_moving_average", true, &[true, false], ) .with_jit_symbol("jet_jit_stats_moving_average"),
    CoreCallRecord::new( "core.math.stats", "ewma", "jet_stats_ewma", true, &[true, false], ) .with_jit_symbol("jet_jit_stats_ewma"),
    CoreCallRecord::new( "core.math.stats", "rank", "jet_stats_rank", true, &[true], ) .with_jit_symbol("jet_jit_stats_rank"),
    CoreCallRecord::new( "core.math.stats", "zscore", "jet_stats_zscore", true, &[true], ) .with_jit_symbol("jet_jit_stats_zscore"),
    CoreCallRecord::new( "core.math.stats", "clip", "jet_stats_clip", true, &[true, false, false], ) .with_jit_symbol("jet_jit_stats_clip"),
    CoreCallRecord::new("core.math.stats", "mean", "jet_stats_mean", true, &[true]).with_jit_symbol("jet_jit_stats_mean"),
    CoreCallRecord::new("core.math.stats", "fmean", "jet_stats_mean", true, &[true]).with_jit_symbol("jet_jit_stats_mean"),
    CoreCallRecord::new("core.math.stats", "geometric_mean", "jet_stats_geometric_mean", true, &[true]).with_jit_symbol("jet_jit_stats_geometric_mean"),
    CoreCallRecord::new("core.math.stats", "harmonic_mean", "jet_stats_harmonic_mean", true, &[true]).with_jit_symbol("jet_jit_stats_harmonic_mean"),
    CoreCallRecord::new("core.math.stats", "median", "jet_stats_median", true, &[true]).with_jit_symbol("jet_jit_stats_median"),
    CoreCallRecord::new("core.math.stats", "median_grouped", "jet_stats_median_grouped", true, &[true, false]).with_jit_symbol("jet_jit_stats_median_grouped"),
    CoreCallRecord::new("core.math.stats", "median_low", "jet_stats_median_low", true, &[true]).with_jit_symbol("jet_jit_stats_median_low"),
    CoreCallRecord::new("core.math.stats", "median_high", "jet_stats_median_high", true, &[true]).with_jit_symbol("jet_jit_stats_median_high"),
    CoreCallRecord::new("core.math.stats", "quantile", "jet_stats_quantile", true, &[true, false]).with_jit_symbol("jet_jit_stats_quantile"),
    CoreCallRecord::new("core.math.stats", "percentile", "jet_stats_percentile", true, &[true, false]).with_jit_symbol("jet_jit_stats_percentile"),
    CoreCallRecord::new("core.math.stats", "mode", "jet_stats_mode", true, &[true]).with_jit_symbol("jet_jit_stats_mode"),
    CoreCallRecord::new("core.math.stats", "multimode", "jet_stats_multimode", true, &[true]).with_jit_symbol("jet_jit_stats_multimode"),
    CoreCallRecord::new("core.math.stats", "pvariance", "jet_stats_pvariance", true, &[true]).with_jit_symbol("jet_jit_stats_pvariance"),
    CoreCallRecord::new("core.math.stats", "pstdev", "jet_stats_pstdev", true, &[true]).with_jit_symbol("jet_jit_stats_pstdev"),
    CoreCallRecord::new("core.math.stats", "variance", "jet_stats_variance", true, &[true]).with_jit_symbol("jet_jit_stats_variance"),
    CoreCallRecord::new("core.math.stats", "stdev", "jet_stats_stdev", true, &[true]).with_jit_symbol("jet_jit_stats_stdev"),
    CoreCallRecord::new("core.math.stats", "covariance", "jet_stats_covariance", true, &[true, true]).with_jit_symbol("jet_jit_stats_covariance"),
    CoreCallRecord::new("core.math.stats", "correlation", "jet_stats_correlation", true, &[true, true]).with_jit_symbol("jet_jit_stats_correlation"),
    CoreCallRecord::new("core.math.stats", "kde", "jet_stats_kde", true, &[true, false]).with_jit_symbol("jet_jit_stats_kde"),
    CoreCallRecord::new("core.math.stats", "kde_random", "jet_stats_kde_random", true, &[true, false, false]).with_jit_symbol("jet_jit_stats_kde_random"),
    CoreCallRecord::new("core.math.stats", "min", "jet_stats_min", true, &[true]).with_jit_symbol("jet_jit_stats_min"),
    CoreCallRecord::new("core.math.stats", "max", "jet_stats_max", true, &[true]).with_jit_symbol("jet_jit_stats_max"),
    CoreCallRecord::new("core.math.stats", "range", "jet_stats_range", true, &[true]).with_jit_symbol("jet_jit_stats_range"),
    CoreCallRecord::new("core.math.stats", "sumprod", "jet_stats_sumprod", true, &[true, true]).with_jit_symbol("jet_jit_stats_sumprod"),
    CoreCallRecord::new("core.math.stats", "weighted_mean", "jet_stats_weighted_mean", true, &[true, true]).with_jit_symbol("jet_jit_stats_weighted_mean"),
    CoreCallRecord::new("core.math.stats", "quantiles", "jet_stats_quantiles", true, &[true, false]).with_jit_symbol("jet_jit_stats_quantiles"),
    CoreCallRecord::new("core.math.stats", "iqr", "jet_stats_iqr", true, &[true]).with_jit_symbol("jet_jit_stats_iqr"),
    CoreCallRecord::new("core.math.stats", "mad", "jet_stats_mad", true, &[true]).with_jit_symbol("jet_jit_stats_mad"),
    CoreCallRecord::new("core.math.stats", "mean_abs_deviation", "jet_stats_mean_abs_deviation", true, &[true]).with_jit_symbol("jet_jit_stats_mean_abs_deviation"),
    CoreCallRecord::new("core.math.stats", "skew", "jet_stats_skew", true, &[true]).with_jit_symbol("jet_jit_stats_skew"),
    CoreCallRecord::new("core.math.stats", "kurtosis", "jet_stats_kurtosis", true, &[true]).with_jit_symbol("jet_jit_stats_kurtosis"),
    CoreCallRecord::new("core.math.stats", "spearman", "jet_stats_spearman", true, &[true, true]).with_jit_symbol("jet_jit_stats_spearman"),
    CoreCallRecord::new("core.math.stats", "residuals", "jet_stats_residuals", true, &[true, true]).with_jit_symbol("jet_jit_stats_residuals"),
    CoreCallRecord::new("core.math.stats", "histogram", "jet_stats_histogram", true, &[true, false]).with_jit_symbol("jet_jit_stats_histogram") .with_native_int_result(),
    CoreCallRecord::new("core.math.stats", "winsorize", "jet_stats_winsorize", true, &[true, false]).with_jit_symbol("jet_jit_stats_winsorize"),
    CoreCallRecord::new("core.math.stats", "pearson", "jet_stats_correlation", true, &[true, true]).with_jit_symbol("jet_jit_stats_correlation"),
    CoreCallRecord::new("core.math.stats", "covariance_population", "jet_stats_covariance_population", true, &[true, true]).with_jit_symbol("jet_jit_stats_covariance_population"),
    CoreCallRecord::new( "core.math.random", "seed", "jet_std_random_seed", true, &[false], ) .with_jit_symbol("jet_jit_random_seed").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.math.random", "getrandbits", "jet_std_random_getrandbits", true, &[false], ) .with_jit_symbol("jet_jit_random_getrandbits") .with_native_int_result(),
    CoreCallRecord::new( "core.crypto.random", "bytes", "jet_std_crypto_random_bytes_controlled", true, &[false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.math.random", "rng", "jet_std_rng_new", true, &[false]) .with_jit_symbol("jet_jit_rng_new"),
    CoreCallRecord::new( "core.math.random", "split", "jet_std_random_split", true, &[false], ) .with_jit_symbol("jet_jit_random_split").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.time", "now", "jet_std_time_now", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.time", "sleep", "jet_std_time_sleep_duration", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.time", "sleep_until", "jet_time_sleep_until", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.rt", "callback", "jet_rt_callback", true, &[false, false, false], ),
    CoreCallRecord::new("core.time", "start", "jet_std_time_start", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.time", "instant", "jet_time_instant_now", true, &[]) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "now_utc", "jet_time_now_utc", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.time", "from_unix_ms", "jet_time_from_unix_ms", false, &[false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "from_unix_seconds", "jet_time_from_unix_seconds", false, &[false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "from_unix_microseconds", "jet_time_from_unix_microseconds", false, &[false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "from_unix_nanoseconds", "jet_time_from_unix_nanoseconds", false, &[false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "today", "jet_time_today", true, &[]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.time", "parse_rfc3339", "jet_time_parse_rfc3339", true, &[true], ) .with_pure_route(CoreCallPureRoute::Time) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.time", "parse_iso_week_date", "JetDate::parse_iso_week_date", true, &[true], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "from_iso_week", "jet_time_from_iso_week", true, &[false, false, false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "parse_zoned", "jet_time_parse_zoned", true, &[true], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "datetime", "jet_time_datetime", true, &[false, false, false, false, false, false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "time", "JetLocalTime::new", false, &[false, false, false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "local_time", "JetLocalTime::new", false, &[false, false, false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "days_in_month", "jet_time_days_in_month", true, &[false, false], ) .with_pure_route(CoreCallPureRoute::Time) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new( "core.time", "is_leap_year", "jet_time_is_leap_year", true, &[false], ) .with_pure_route(CoreCallPureRoute::Time) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.time", "period", "jet_time_period", true, &[false, false, false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "period_days", "jet_time_period_days", true, &[false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "period_months", "jet_time_period_months", true, &[false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "period_years", "jet_time_period_years", true, &[false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "zone", "jet_time_zone_named", true, &[true]) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "utc", "jet_time_zone_utc", true, &[]) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "zoned", "jet_time_zoned", true, &[true, true]) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "zoned_local", "jet_time_zoned_local", true, &[true, true, true, true], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "clock", "jet_std_clock_new", true, &[false]),
    CoreCallRecord::new( "core.encoding.json", "parse", "jet_std_json_parse", true, &[true], ),
    CoreCallRecord::new( "core.encoding.json", "loads", "jet_std_json_parse", true, &[true], ),
    CoreCallRecord::new( "core.encoding.json", "decode", "jet_std_json_decode_lenient", true, &[true], ) .without_direct_aot() .without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.encoding.json", "to_string", "jet_std_json_render", true, &[true], ) .with_jit_symbol("jet_jit_json_to_string"),
    CoreCallRecord::new( "core.encoding.json", "dumps", "jet_std_json_render", true, &[true], ) .with_jit_symbol("jet_jit_json_to_string"),
    CoreCallRecord::new( "core.encoding.json", "to_string_pretty", "jet_std_json_render_pretty", true, &[true], ) .with_jit_symbol("jet_jit_json_to_string_pretty"),
    CoreCallRecord::new( "core.encoding.csv", "decode", "jet_enc_csv_decode", true, &[true], ) .without_direct_jit(),
    CoreCallRecord::new( "core.encoding.csv", "to_string", "jet_enc_csv_to_string", true, &[true], ) .with_jit_symbol("jet_jit_csv_to_string"),
    CoreCallRecord::new( "core.game", "run", "jet_game_run", true, &[true, true, true, true], ) .without_direct_aot() .without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.encoding.json", "events", "jet_std_json_events", true, &[true], ),
    CoreCallRecord::new( "core.encoding.jsonl", "parse", "jet_std_jsonl_parse", true, &[true], ),
    CoreCallRecord::new( "core.encoding.jsonl", "to_string", "jet_std_jsonl_render", true, &[true], ),
    CoreCallRecord::new( "core.encoding.jsonl", "loads", "jet_std_jsonl_parse", true, &[true], ) .with_jit_symbol("jet_jit_jsonl_parse"),
    CoreCallRecord::new( "core.encoding.jsonl", "dumps", "jet_std_jsonl_render", true, &[true], ) .with_jit_symbol("jet_jit_jsonl_to_string"),
    CoreCallRecord::new( "core.encoding.jsonl", "count_rows", "jet_std_jsonl_count_rows", true, &[true], ),
    CoreCallRecord::new( "core.encoding.jsonl", "append_line", "jet_std_jsonl_append_line", true, &[true, true], ) .with_jit_symbol("jet_jit_jsonl_append_line"),
    CoreCallRecord::new( "core.encoding.jsonl", "first", "jet_std_jsonl_first", true, &[true], ) .with_jit_symbol("jet_jit_jsonl_first"),
    CoreCallRecord::new( "core.encoding.csv", "parse", "jet_ring_csv_parse", true, &[true, true, false, false], ),
    CoreCallRecord::new( "core.encoding.csv", "rows", "jet_ring_csv_rows", true, &[true, true, false, false], ) .without_direct_aot() .without_direct_jit(),
    CoreCallRecord::new( "core.encoding.csv", "query", "jet_enc_csv_query", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_enc_csv_query"),
    CoreCallRecord::new("core.encoding.json", "reader", "jet_enc_json_reader", true, &[false, true]).with_jit_symbol("jet_jit_json_reader"),
    CoreCallRecord::new("core.encoding.json", "writer", "jet_enc_json_writer", true, &[false, true, true]).with_jit_symbol("jet_jit_json_writer"),
    CoreCallRecord::new("core.encoding.jsonl", "reader", "jet_enc_jsonl_reader", true, &[false, true]).with_jit_symbol("jet_jit_jsonl_reader"),
    CoreCallRecord::new("core.encoding.jsonl", "writer", "jet_enc_jsonl_writer", true, &[false, true]).with_jit_symbol("jet_jit_jsonl_writer"),
    CoreCallRecord::new("core.encoding.csv", "reader", "jet_enc_csv_reader", true, &[false, true, true, true, true]).with_jit_symbol("jet_jit_csv_reader"),
    CoreCallRecord::new("core.encoding.csv", "writer", "jet_enc_csv_writer", true, &[false, true]).with_jit_symbol("jet_jit_csv_writer"),
    CoreCallRecord::new("core.encoding.cbor", "reader", "jet_enc_cbor_reader", true, &[false, true]).with_jit_symbol("jet_jit_cbor_reader"),
    CoreCallRecord::new("core.encoding.cbor", "writer", "jet_enc_cbor_writer", true, &[false, true]).with_jit_symbol("jet_jit_cbor_writer"),
    CoreCallRecord::new("core.encoding.xml", "reader", "jet_enc_xml_reader", true, &[false, true, true]).with_jit_symbol("jet_jit_xml_reader"),
    CoreCallRecord::new("core.crypto.vault", "prepare_rotate", "jet_vault_prepare_rotate_impl", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_prepare_rotate"),
    CoreCallRecord::new("core.crypto.vault", "authorize_write", "jet_vault_authorize_write_impl", false, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_authorize_write"),
    CoreCallRecord::new("core.crypto.vault", "commit_rotate", "jet_vault_commit_rotate_impl", false, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_commit_rotate"),
    CoreCallRecord::new("core.crypto.vault", "current", "jet_vault_current_impl", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_current"),
    CoreCallRecord::new("core.crypto.vault", "versions", "jet_vault_versions_impl", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_versions"),
    CoreCallRecord::new("core.crypto.vault", "load", "jet_vault_load_impl", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_load"),
    CoreCallRecord::new("core.crypto.vault", "status", "jet_vault_status_impl", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_status"),
    CoreCallRecord::new("core.crypto.vault", "prepare_generate", "jet_vault_prepare_generate_impl", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_prepare_generate"),
    CoreCallRecord::new("core.crypto.vault", "prepare_store", "jet_vault_prepare_store_impl", false, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_prepare_store"),
    CoreCallRecord::new("core.crypto.vault", "prepare_retire", "jet_vault_prepare_retire_impl", false, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_prepare_retire"),
    CoreCallRecord::new("core.crypto.vault", "prepare_revoke", "jet_vault_prepare_revoke_impl", false, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_prepare_revoke"),
    CoreCallRecord::new("core.crypto.vault", "commit_generate", "jet_vault_commit_generate_impl", false, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_commit_generate"),
    CoreCallRecord::new("core.crypto.vault", "commit_store", "jet_vault_commit_store_impl", false, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_commit_store"),
    CoreCallRecord::new("core.crypto.vault", "commit_retire", "jet_vault_commit_retire_impl", false, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_commit_retire"),
    CoreCallRecord::new("core.crypto.vault", "commit_revoke", "jet_vault_commit_revoke_impl", false, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_commit_revoke"),
    CoreCallRecord::new("core.crypto.vault", "export_to_recipients", "jet_vault_export_to_recipients_impl", false, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_export_to_recipients"),
    CoreCallRecord::new("core.crypto.vault", "export_to_passphrase", "jet_vault_export_to_passphrase_impl", false, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_export_to_passphrase"),
    CoreCallRecord::new("core.crypto.vault", "prepare_import_wrapped", "jet_vault_prepare_import_wrapped_impl", false, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_prepare_import_wrapped"),
    CoreCallRecord::new("core.crypto.vault", "authorize_wrapped_import", "jet_vault_authorize_wrapped_import_impl", false, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_authorize_wrapped_import"),
    CoreCallRecord::new("core.crypto.vault", "commit_import_wrapped", "jet_vault_commit_import_wrapped_impl", false, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_commit_import_wrapped"),
    CoreCallRecord::new("core.crypto.vault", "prepare_import_signing", "jet_vault_expert_prepare_import_signing_impl", false, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_expert_prepare_import_signing"),
    CoreCallRecord::new("core.crypto.vault", "commit_import_signing", "jet_vault_expert_commit_import_signing_impl", false, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_expert_commit_import_signing"),
    CoreCallRecord::new("core.crypto.vault", "prepare_import_x25519", "jet_vault_expert_prepare_import_x25519_impl", false, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_expert_prepare_import_x25519"),
    CoreCallRecord::new("core.crypto.vault", "commit_import_x25519", "jet_vault_expert_commit_import_x25519_impl", false, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_expert_commit_import_x25519"),
    CoreCallRecord::new("core.crypto", "__vault_wrapped_from_bytes", "jet_vault_wrapped_from_bytes_impl", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_wrapped_from_bytes"),
    CoreCallRecord::new("core.crypto", "__vault_wrapped_bytes", "jet_vault_wrapped_bytes_impl", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_wrapped_bytes"),
    CoreCallRecord::new("core.crypto", "__vault_unlock_recipient", "jet_vault_unlock_recipient_impl", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_unlock_recipient"),
    CoreCallRecord::new("core.crypto", "__vault_unlock_passphrase", "jet_vault_unlock_passphrase_impl", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot().with_jit_symbol("jet_jit_vault_unlock_passphrase"),
    CoreCallRecord::new("core.crypto", "__zeroize", "jet_crypto_zeroize", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_jit_symbol("jet_jit_crypto_zeroize"),
    CoreCallRecord::new( "core.web.storage.session", "get", "jet_web_storage_session_get", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .with_jit_symbol("jet_jit_web_storage_session_get"),
    CoreCallRecord::new( "core.web.storage.session", "remove", "jet_web_storage_session_remove", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .with_jit_symbol("jet_jit_web_storage_session_remove"),
    CoreCallRecord::new( "core.web.storage.session", "set", "jet_web_storage_session_set", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .with_jit_symbol("jet_jit_web_storage_session_set"),
    CoreCallRecord::new( "core.web.storage.session", "clear", "jet_web_storage_session_clear", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .with_jit_symbol("jet_jit_web_storage_session_clear"),
    CoreCallRecord::new( "core.web.storage.local", "get", "jet_web_storage_local_get", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .with_jit_symbol("jet_jit_web_storage_local_get"),
    CoreCallRecord::new( "core.web.storage.local", "set", "jet_web_storage_local_set", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .with_jit_symbol("jet_jit_web_storage_local_set"),
    CoreCallRecord::new( "core.web.storage.local", "remove", "jet_web_storage_local_remove", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .with_jit_symbol("jet_jit_web_storage_local_remove"),
    CoreCallRecord::new( "core.web.storage.local", "clear", "jet_web_storage_local_clear", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .with_jit_symbol("jet_jit_web_storage_local_clear"),
    CoreCallRecord::new("core.data", "count", "jet_data_count", true, &[true]) .with_jit_symbol("jet_jit_data_count") .with_native_int_result(),
    CoreCallRecord::new("core.data", "query", "jet_data_query", true, &[true]) .with_jit_symbol("jet_jit_data_query"),
    CoreCallRecord::new("core.data", "track", "jet_data_track", true, &[true, false]) .without_direct_jit(),
    CoreCallRecord::new("core.compute", "gradient", "jet_compute_curried_new", true, &[true]) .with_max_arity(128) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .without_direct_jit(),
    CoreCallRecord::new("core.compute", "value_and_gradient", "jet_compute_curried_new", true, &[true]) .with_max_arity(128) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .without_direct_jit(),
    CoreCallRecord::new("core.compute", "vjp", "jet_compute_curried_new", true, &[true]) .with_max_arity(128) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .without_direct_jit(),
    CoreCallRecord::new("core.compute", "jvp", "jet_compute_curried_new", true, &[true]) .with_max_arity(128) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .without_direct_jit(),
    CoreCallRecord::new("core.compute", "zeros", "jet_compute_zeros", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.compute", "ones", "jet_compute_ones", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.compute", "full", "jet_compute_full", true, &[true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.compute", "from_list", "jet_compute_from_list", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_from_list")),
    CoreCallRecord::new( "core.compute", "set", "jet_compute_set", true, &[true, true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .with_frame_accesses(&[CoreCallResourceAccess::write(0), CoreCallResourceAccess::read(1)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_set")),
    CoreCallRecord::new( "core.compute", "matrix", "jet_compute_matrix", true, &[false, false, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.compute", "vec", "jet_compute_vec", true, &[false, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.compute", "add", "jet_compute_add", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0), CoreCallResourceAccess::read(1)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_add")),
    CoreCallRecord::new( "core.compute", "mul", "jet_compute_mul", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0), CoreCallResourceAccess::read(1)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_mul")),
    CoreCallRecord::new( "core.compute", "sub", "jet_compute_sub", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0), CoreCallResourceAccess::read(1)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_sub")),
    CoreCallRecord::new( "core.compute", "div", "jet_compute_div", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0), CoreCallResourceAccess::read(1)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_div")),
    CoreCallRecord::new( "core.compute", "maximum", "jet_compute_maximum", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0), CoreCallResourceAccess::read(1)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_maximum")),
    CoreCallRecord::new( "core.compute", "minimum", "jet_compute_minimum", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0), CoreCallResourceAccess::read(1)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_minimum")),
    CoreCallRecord::new( "core.compute", "negate", "jet_compute_negate", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_negate")),
    CoreCallRecord::new( "core.compute", "abs", "jet_compute_abs", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_abs")),
    CoreCallRecord::new( "core.compute", "exp", "jet_compute_exp", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_exp")),
    CoreCallRecord::new( "core.compute", "log", "jet_compute_log", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_log")),
    CoreCallRecord::new( "core.compute", "sqrt", "jet_compute_sqrt", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_sqrt")),
    CoreCallRecord::new( "core.compute", "matmul", "jet_compute_matmul", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0), CoreCallResourceAccess::read(1)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_matmul")),
    CoreCallRecord::new( "core.compute", "reshape", "jet_compute_reshape", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0), CoreCallResourceAccess::read(1)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_reshape")),
    CoreCallRecord::new( "core.compute", "get", "jet_compute_get", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0), CoreCallResourceAccess::read(1)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_get")),
    CoreCallRecord::new( "core.compute", "shape", "jet_compute_tensor_shape", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_tensor_shape")) .with_native_int_result(),
    CoreCallRecord::new( "core.compute", "rank", "jet_compute_tensor_rank", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_tensor_rank")) .with_native_int_result(),
    CoreCallRecord::new( "core.compute", "numel", "jet_compute_tensor_numel", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_tensor_numel")) .with_native_int_result(),
    CoreCallRecord::new( "core.compute", "to_list", "jet_compute_tensor_to_list", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_tensor_to_list")),
    CoreCallRecord::new( "core.compute", "device", "jet_compute_tensor_device", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_tensor_device")),
    CoreCallRecord::new( "core.compute", "placement", "jet_compute_tensor_placement", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_tensor_placement")),
    CoreCallRecord::new( "core.compute", "device_cpu", "jet_compute_device_cpu", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.compute", "device_auto", "jet_compute_device_auto", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.compute", "device_metal", "jet_compute_device_metal", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.compute", "device_cuda", "jet_compute_device_cuda", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.compute", "device_vulkan", "jet_compute_device_vulkan", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.compute", "device_webgpu", "jet_compute_device_webgpu", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.compute", "on_device", "jet_compute_on_device", true, &[true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_on_device")),
    CoreCallRecord::new( "core.compute", "broadcast_to", "jet_compute_broadcast_to", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0), CoreCallResourceAccess::read(1)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_broadcast_to")),
    CoreCallRecord::new( "core.compute", "transpose", "jet_compute_transpose", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_transpose")),
    CoreCallRecord::new( "core.compute", "sum_axis", "jet_compute_sum_axis", true, &[true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_sum_axis")),
    CoreCallRecord::new("core.compute", "eye", "jet_compute_eye", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.compute", "det", "jet_compute_det", true, &[true]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_det")),
    CoreCallRecord::new("core.compute", "inv", "jet_compute_inv", true, &[true]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_inv")),
    CoreCallRecord::new("core.compute", "fft", "jet_compute_fft", true, &[true]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_fft")),
    CoreCallRecord::new( "core.compute", "solve", "jet_compute_solve", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0), CoreCallResourceAccess::read(1)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_solve")),
    CoreCallRecord::new( "core.compute", "stream_new", "jet_compute_stream_new", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.compute", "stream_new_on", "jet_compute_stream_new_on_device", true, &[false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.compute", "stream_sync", "jet_compute_stream_sync", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_stream_sync")),
    CoreCallRecord::new( "core.compute", "stream_show", "jet_compute_stream_show", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.compute", "transfer", "jet_compute_transfer", true, &[true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_transfer")),
    CoreCallRecord::new( "core.compute", "transfer_show", "jet_compute_transfer_show", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.compute", "kernel_bounds_ok", "jet_compute_kernel_bounds_ok", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.compute", "mse_loss", "jet_compute_mse_loss", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0), CoreCallResourceAccess::read(1)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_mse_loss")),
    CoreCallRecord::new( "core.compute", "sgd_step", "jet_compute_sgd_step", true, &[true, true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0), CoreCallResourceAccess::read(1)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_sgd_step")),
    CoreCallRecord::new( "core.compute", "serialize", "jet_compute_serialize", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_serialize")),
    CoreCallRecord::new( "core.compute", "deserialize", "jet_compute_deserialize", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.compute", "to_sparse", "jet_compute_to_sparse", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_to_sparse")),
    CoreCallRecord::new( "core.compute", "sparse_nnz", "jet_compute_sparse_nnz", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_sparse_nnz")) .with_native_int_result(),
    CoreCallRecord::new( "core.compute", "sparse_mv", "jet_compute_sparse_mv", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0), CoreCallResourceAccess::read(1)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_sparse_mv")),
    CoreCallRecord::new( "core.compute", "sparse_show", "jet_compute_sparse_show", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_sparse_show")),
    CoreCallRecord::new( "core.compute", "matmul_f32_tile", "jet_compute_matmul_f32_tile", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_frame_accesses(&[CoreCallResourceAccess::read(0), CoreCallResourceAccess::read(1)]) .with_frame_completion(CoreCallCompletion::synchronous("jet_compute_matmul_f32_tile")),
    CoreCallRecord::new( "core.compute", "profile_f32_strict", "jet_compute_profile_f32_strict", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.compute", "profile_show", "jet_compute_profile_show", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.service", "state_store", "jet_services_state_store", true, &[false], ) .without_direct_aot() .without_direct_jit() .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "tree", "jet_services_tree", true, &[false]) .without_direct_aot() .without_direct_jit() .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.service", "tree_show", "jet_services_tree_show", true, &[true], ) .without_direct_aot() .without_direct_jit() .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.service", "delivery_at_most_once", "jet_services_delivery_at_most_once", true, &[], ) .without_direct_aot() .without_direct_jit() .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.service", "delivery_durable", "jet_services_delivery_durable", true, &[], ) .without_direct_aot() .without_direct_jit() .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.service", "set_delivery", "jet_services_set_delivery", true, &[true, false], ) .without_direct_aot() .without_direct_jit() .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.service", "worker", "jet_services_worker", true, &[true, false, false, false, false], ) .without_direct_aot() .without_direct_jit() .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "start", "jet_services_start", true, &[true]) .without_direct_aot() .without_direct_jit() .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "stop", "jet_services_stop", true, &[true]) .without_direct_aot() .without_direct_jit() .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.service", "runtime", "jet_services_runtime", true, &[false, false], ) .without_direct_aot() .without_direct_jit() .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "endpoint_send", "jet_services_endpoint_send", true, &[true, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "endpoint_receive", "jet_services_endpoint_receive", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "endpoint_show", "jet_services_endpoint_show", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "delivery_state_show", "jet_services_delivery_state_show", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "delivery_status", "jet_services_delivery_status", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "append_event", "jet_services_append_event", true, &[true, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "chaos_fail", "jet_services_chaos_fail", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new("core.service", "commit_snapshot", "jet_services_commit_snapshot", true, &[true, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "dead_letter_count", "jet_services_dead_letter_count", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new("core.service", "delivery_cancel", "jet_services_delivery_cancel", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "delivery_events", "jet_services_delivery_events", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "delivery_receipt", "jet_services_delivery_receipt", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "delivery_retry", "jet_services_delivery_retry", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "delivery_wait", "jet_services_delivery_wait", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "directory_generation", "jet_services_directory_generation", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new("core.service", "directory_register", "jet_services_directory_register", true, &[true, false, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "directory_resolve", "jet_services_directory_resolve", true, &[true, true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "drain_dead_letters", "jet_services_drain_dead_letters", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new("core.service", "drain_worker", "jet_services_drain_worker", true, &[true, true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "event_count", "jet_services_event_count", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new("core.service", "fail_worker", "jet_services_fail_worker", true, &[true, true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "group", "jet_services_group", true, &[true, false, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "handoff_generation", "jet_services_handoff_generation", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new("core.service", "mailbox_depth", "jet_services_mailbox_depth", true, &[true, true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new("core.service", "observe", "jet_services_observe", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "partition_worker", "jet_services_partition_worker", true, &[true, true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "receive", "jet_services_receive", true, &[true, true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "reconcile_worker", "jet_services_reconcile_worker", true, &[true, true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "replay_events", "jet_services_replay_events", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "restarts", "jet_services_restarts", true, &[true, true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new("core.service", "restore_snapshot", "jet_services_restore_snapshot", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "rollback_generation", "jet_services_rollback_generation", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new("core.service", "send", "jet_services_send", true, &[true, true, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "send_durable", "jet_services_send_durable", true, &[true, true, false, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "set_restart", "jet_services_set_restart", true, &[true, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "set_state_empty", "jet_services_set_state_empty", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "set_state_event_log", "jet_services_set_state_event_log", true, &[true, false, false, false, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "set_state_snapshot", "jet_services_set_state_snapshot", true, &[true, false, false, false, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "upgrade_receipt", "jet_services_upgrade_receipt", true, &[true]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "workflow_activity", "jet_services_workflow_activity", true, &[true, false, false, false, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "workflow_activity_complete", "jet_services_workflow_activity_complete", true, &[true, false, false, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "workflow_activity_retry", "jet_services_workflow_activity_retry", true, &[true, false, false, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "workflow_activity_wait", "jet_services_workflow_activity_wait", true, &[true, false, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "workflow_all", "jet_services_workflow_all", true, &[true, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "workflow_sleep", "jet_services_workflow_sleep", true, &[true, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "workflow_step", "jet_services_workflow_step", true, &[true, false, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.data", "load", "jet_data_loader_load", true, &[true, true]) .without_direct_jit(),
    CoreCallRecord::new("core.data", "load_default", "jet_data_loader_load_default", true, &[true]) .without_direct_jit(),
    CoreCallRecord::new("core.data", "file", "jet_data_loader_file", true, &[true, true, true]) .without_direct_jit(),
    CoreCallRecord::new( "core.data", "file_member", "jet_data_loader_file_member", true, &[true, true, true, true], ) .without_direct_jit(),
    CoreCallRecord::new("core.data", "url", "jet_data_loader_url", true, &[true, true, true, true]) .without_direct_jit(),
    CoreCallRecord::new( "core.data", "database", "jet_data_loader_database", true, &[true, true, true, true], ) .without_direct_jit(),
    CoreCallRecord::new("core.data", "value", "jet_data_loader_value", true, &[true, true]) .without_direct_jit(),
    CoreCallRecord::new("core.data", "snapshot", "jet_data_loader_snapshot", true, &[true]) .without_direct_jit(),
    CoreCallRecord::new( "core.data.loader", "bind", "jet_data_loader_bind", true, &[true, true], ) .without_direct_jit(),
    CoreCallRecord::new( "core.data.loader", "bind_text", "jet_data_loader_bind_text", true, &[true, true], ) .without_direct_jit(),
    CoreCallRecord::new( "core.data.loader", "cancel", "jet_data_loader_cancel", true, &[true], ) .without_direct_jit(),
    CoreCallRecord::new( "core.data.loader", "offline", "jet_data_loader_offline", true, &[true, false], ) .without_direct_jit(),
    CoreCallRecord::new( "core.data.loader", "invalidate", "jet_data_loader_invalidate", true, &[true, false], ) .without_direct_jit(),
    CoreCallRecord::new( "core.data.loader", "needs_refresh", "jet_data_loader_needs_refresh", true, &[true], ) .without_direct_jit(),
    CoreCallRecord::new( "core.data.loader", "ready", "jet_data_loader_ready", true, &[true], ) .without_direct_jit(),
    CoreCallRecord::new( "core.data.loader", "status", "jet_data_loader_status", true, &[true], ) .without_direct_jit(),
    CoreCallRecord::new( "core.data.loader", "source_identity", "jet_data_loader_source_identity", true, &[true], ) .without_direct_jit(),
    CoreCallRecord::new( "core.data.loader", "authority", "jet_data_loader_authority", true, &[true], ) .without_direct_jit(),
    CoreCallRecord::new( "core.data.loader", "snapshot_reusable", "jet_data_snapshot_reusable", true, &[true, true], ) .without_direct_jit(),
    CoreCallRecord::new( "core.data.loader", "stream", "jet_data_loader_stream", true, &[true], ) .without_direct_jit(),
    CoreCallRecord::new( "core.data.stream", "next", "jet_data_stream_next", true, &[true], ) .without_direct_jit(),
    CoreCallRecord::new( "core.data.stream", "collect", "jet_data_stream_collect", true, &[true], ) .without_direct_jit(),
    CoreCallRecord::new( "core.data.stream", "cancel", "jet_data_stream_cancel", true, &[true], ) .without_direct_jit(),
    CoreCallRecord::new("core.data", "inspect", "jet_data_plot_inspect", true, &[true]) .with_jit_symbol("jet_jit_data_plot_inspect"),
    CoreCallRecord::new( "core.data", "inspect_json", "jet_data_plot_inspect_json", true, &[true], ) .with_jit_symbol("jet_jit_data_plot_inspect_json"),
    CoreCallRecord::new("core.data", "text", "jet_data_plot_text", true, &[true]) .with_jit_symbol("jet_jit_data_plot_text"),
    CoreCallRecord::new("core.data", "svg", "jet_data_plot_svg", true, &[true]) .with_jit_symbol("jet_jit_data_plot_svg"),
    CoreCallRecord::new( "core.data", "line_text", "jet_data_line_text_plot_checked", true, &[true, true], ) .with_jit_symbol("jet_jit_data_line_text"),
    CoreCallRecord::new( "core.data", "line_svg", "jet_data_line_svg_plot_checked", true, &[true, true], ) .with_jit_symbol("jet_jit_data_line_svg"),
    CoreCallRecord::new("core.data", "show", "jet_data_plot_show", true, &[true]) .with_jit_symbol("jet_jit_data_plot_show"),
    CoreCallRecord::new( "core.data", "render", "jet_data_plot_render", true, &[true, false], ) .with_jit_symbol("jet_jit_data_plot_render"),
    CoreCallRecord::new( "core.data.plot", "plot", "jet_data_plot", true, &[true], ) .with_jit_symbol("jet_jit_data_plot"),
    CoreCallRecord::new( "core.data.plot", "inspect", "jet_data_plot_inspect", true, &[true], ) .with_jit_symbol("jet_jit_data_plot_inspect"),
    CoreCallRecord::new( "core.data.plot", "inspect_json", "jet_data_plot_inspect_json", true, &[true], ) .with_jit_symbol("jet_jit_data_plot_inspect_json"),
    CoreCallRecord::new("core.data.plot", "text", "jet_data_plot_text", true, &[true]) .with_jit_symbol("jet_jit_data_plot_text"),
    CoreCallRecord::new("core.data.plot", "svg", "jet_data_plot_svg", true, &[true]) .with_jit_symbol("jet_jit_data_plot_svg"),
    CoreCallRecord::new("core.data.plot", "show", "jet_data_plot_show", true, &[true]) .with_jit_symbol("jet_jit_data_plot_show"),
    CoreCallRecord::new( "core.data.plot", "render", "jet_data_plot_render", true, &[true, false], ) .with_jit_symbol("jet_jit_data_plot_render"),
    CoreCallRecord::new("core.data", "plot", "jet_data_plot", true, &[true]) .with_jit_symbol("jet_jit_data_plot"),
    CoreCallRecord::new( "core.data", "left_join", "jet_data_left_join_checked_default", true, &[true, true, false, false], ) .without_direct_aot() .with_jit_symbol("jet_jit_data_left_join").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.data", "pivot_sum", "jet_data_pivot_sum_checked_default", true, &[true, false, false, false]).without_direct_aot().with_jit_symbol("jet_jit_data_pivot_sum"),
    CoreCallRecord::new("core.data", "describe", "jet_data_describe_checked", true, &[true]).with_jit_symbol("jet_jit_data_describe"),
    CoreCallRecord::new("core.data", "rolling_mean", "jet_data_rolling_mean_checked", true, &[true, false]).with_jit_symbol("jet_jit_data_rolling_mean"),
    CoreCallRecord::new("core.data", "quantile", "jet_data_quantile_checked", true, &[true, false]).with_jit_symbol("jet_jit_data_quantile"),
    CoreCallRecord::new("core.data", "variance", "jet_data_variance_checked", true, &[true]).with_jit_symbol("jet_jit_data_variance"),
    CoreCallRecord::new("core.data", "sum", "jet_data_sum_checked", true, &[true]).with_jit_symbol("jet_jit_data_sum"),
    CoreCallRecord::new("core.data", "bar_text", "jet_data_bar_text_checked", true, &[true]).with_jit_symbol("jet_jit_data_bar_text"),
    CoreCallRecord::new("core.data", "status", "jet_data_status", true, &[]),
    CoreCallRecord::new( "core.data", "require_bridge", "jet_data_require_bridge", true, &[true], ),
    CoreCallRecord::new("core.data", "csv", "jet_enc_csv_decode", true, &[true]).without_direct_jit(),
    CoreCallRecord::new("core.data", "json", "jet_data_json_decode", true, &[true]).without_direct_jit(),
    CoreCallRecord::new( "core.data", "csv_reader", "jet_data_csv_reader", true, &[false, false], ),
    CoreCallRecord::new( "core.data", "json_reader", "jet_data_json_reader", true, &[false, false], ),
    CoreCallRecord::new("core.text.fmt", "number", "jet_fmt_number", true, &[false]),
    CoreCallRecord::new("core.text.fmt", "pretty", "jet_fmt_pretty", true, &[true]),
    CoreCallRecord::new( "core.text.fmt", "decimal", "jet_fmt_decimal", true, &[false, false], ),
    CoreCallRecord::new( "core.text.fmt", "grouped", "jet_fmt_grouped", true, &[false, false], ),
    CoreCallRecord::new("core.text.fmt", "hex", "jet_fmt_hex", true, &[false, false]),
    CoreCallRecord::new("core.text.fmt", "sci", "jet_fmt_sci", true, &[false, false]),
    CoreCallRecord::new( "core.text.fmt", "percent", "jet_fmt_percent", true, &[false, false], ),
    CoreCallRecord::new("core.text.fmt", "bin", "jet_fmt_bin", true, &[false]),
    CoreCallRecord::new("core.text.fmt", "oct", "jet_fmt_oct", true, &[false]),
    CoreCallRecord::new("core.text.fmt", "bytes", "jet_fmt_bytes", true, &[false]),
    CoreCallRecord::new( "core.text.fmt", "duration", "jet_fmt_duration", true, &[false], ),
    CoreCallRecord::new( "core.text.fmt", "ordinal", "jet_fmt_ordinal", true, &[false], ),
    CoreCallRecord::new( "core.text.fmt", "plural", "jet_fmt_plural", true, &[false, true, true], ),
    CoreCallRecord::new( "core.text.fmt", "pad", "jet_fmt_pad", true, &[true, false, true], ),
    CoreCallRecord::new( "core.text.fmt", "pad_left", "jet_fmt_pad_left", true, &[true, false, true], ),
    CoreCallRecord::new( "core.text.fmt", "pad_right", "jet_fmt_pad_right", true, &[true, false, true], ),
    CoreCallRecord::new( "core.text.fmt", "pad_center", "jet_fmt_pad_center", true, &[true, false, true], ),
    CoreCallRecord::new( "core.encoding.toml", "parse", "jet_std_toml_parse", true, &[true], ),
    CoreCallRecord::new( "core.encoding.toml", "load", "jet_std_toml_parse", true, &[true], ) .with_jit_symbol("jet_jit_toml_load"),
    CoreCallRecord::new( "core.encoding.toml", "loads", "jet_std_toml_parse", true, &[true], ) .with_jit_symbol("jet_jit_toml_loads"),
    CoreCallRecord::new( "core.encoding.yaml", "parse", "jet_std_yaml_parse", true, &[true], ),
    CoreCallRecord::new( "core.encoding.xml", "parse", "jet_std_xml_parse", true, &[true], ),
    CoreCallRecord::new( "core.encoding.xml", "parse_with", "jet_std_xml_parse_with", true, &[true, true], ),
    CoreCallRecord::new( "core.encoding.xml", "to_string", "jet_std_xml_render", true, &[true], ),
    CoreCallRecord::new( "core.encoding.xml", "canonical", "jet_std_xml_canonical", true, &[true, true], ) .with_jit_symbol("jet_jit_xml_canonical") .with_pure_route(CoreCallPureRoute::EncodingXml),
    CoreCallRecord::new( "core.encoding.xml", "root", "jet_std_xml_root", true, &[true], ),
    CoreCallRecord::new( "core.encoding.xml", "attribute", "jet_std_xml_attribute", true, &[true, true], ),
    CoreCallRecord::new( "core.encoding.xml", "content", "jet_std_xml_content", true, &[true], ),
    CoreCallRecord::new( "core.encoding.cbor", "to_bytes", "jet_enc_cbor_to_bytes", true, &[true], ) .with_marker(CoreMarkerApplication::deprecated( "encode", "2027", "cbor.to_bytes", Some("2028"), )),
    CoreCallRecord::new( "core.encoding.cbor", "parse", "jet_enc_cbor_parse", true, &[true], ) .with_max_arity(2) .without_direct_aot() .without_direct_jit() .with_marker(CoreMarkerApplication::deprecated( "decode", "2027", "cbor.parse", Some("2028"), )),
    CoreCallRecord::new( "core.encoding.cbor", "to_bytes_canonical", "jet_enc_cbor_to_bytes_canonical", true, &[true], ),
    CoreCallRecord::new("core.encoding.binary", "pack_u8", "jet_std_binary_pack_u8", true, &[false]) .with_jit_symbol("jet_jit_binary_pack_u8"),
    CoreCallRecord::new("core.encoding.binary", "pack_i8", "jet_std_binary_pack_i8", true, &[false]) .with_jit_symbol("jet_jit_binary_pack_i8"),
    CoreCallRecord::new("core.encoding.binary", "pack_u16le", "jet_std_binary_pack_u16le", true, &[false]) .with_jit_symbol("jet_jit_binary_pack_u16le"),
    CoreCallRecord::new("core.encoding.binary", "pack_u16be", "jet_std_binary_pack_u16be", true, &[false]) .with_jit_symbol("jet_jit_binary_pack_u16be"),
    CoreCallRecord::new("core.encoding.binary", "pack_u32le", "jet_std_binary_pack_u32le", true, &[false]) .with_jit_symbol("jet_jit_binary_pack_u32le"),
    CoreCallRecord::new("core.encoding.binary", "pack_u32be", "jet_std_binary_pack_u32be", true, &[false]) .with_jit_symbol("jet_jit_binary_pack_u32be"),
    CoreCallRecord::new("core.encoding.binary", "pack_u64le", "jet_std_binary_pack_u64le", true, &[false]) .with_jit_symbol("jet_jit_binary_pack_u64le"),
    CoreCallRecord::new("core.encoding.binary", "pack_u64be", "jet_std_binary_pack_u64be", true, &[false]) .with_jit_symbol("jet_jit_binary_pack_u64be"),
    CoreCallRecord::new("core.encoding.binary", "unpack_u8", "jet_std_binary_unpack_u8", true, &[true, false]) .with_jit_symbol("jet_jit_binary_unpack_u8") .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "unpack_u16le", "jet_std_binary_unpack_u16le", true, &[true, false]) .with_jit_symbol("jet_jit_binary_unpack_u16le") .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "unpack_u16be", "jet_std_binary_unpack_u16be", true, &[true, false]) .with_jit_symbol("jet_jit_binary_unpack_u16be") .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "unpack_u32le", "jet_std_binary_unpack_u32le", true, &[true, false]) .with_jit_symbol("jet_jit_binary_unpack_u32le") .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "unpack_u32be", "jet_std_binary_unpack_u32be", true, &[true, false]) .with_jit_symbol("jet_jit_binary_unpack_u32be") .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "unpack_u64le", "jet_std_binary_unpack_u64le", true, &[true, false]) .with_jit_symbol("jet_jit_binary_unpack_u64le") .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "unpack_u64be", "jet_std_binary_unpack_u64be", true, &[true, false]) .with_jit_symbol("jet_jit_binary_unpack_u64be") .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "sign_extend", "jet_std_binary_sign_extend", true, &[false, false]) .with_jit_symbol("jet_jit_binary_sign_extend") .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "pack_f64le", "jet_std_binary_pack_f64le", true, &[false]) .with_jit_symbol("jet_jit_binary_pack_f64le"),
    CoreCallRecord::new("core.encoding.binary", "pack_f64be", "jet_std_binary_pack_f64be", true, &[false]) .with_jit_symbol("jet_jit_binary_pack_f64be"),
    CoreCallRecord::new("core.encoding.binary", "unpack_f64le", "jet_std_binary_unpack_f64le", true, &[true, false]) .with_jit_symbol("jet_jit_binary_unpack_f64le"),
    CoreCallRecord::new("core.encoding.binary", "unpack_f64be", "jet_std_binary_unpack_f64be", true, &[true, false]) .with_jit_symbol("jet_jit_binary_unpack_f64be"),
    CoreCallRecord::new("core.encoding.binary", "calcsize", "jet_std_binary_calcsize", true, &[true]) .with_jit_symbol("jet_jit_binary_calcsize") .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "pack", "jet_std_binary_pack", true, &[true, true]) .with_jit_symbol("jet_jit_binary_pack"),
    CoreCallRecord::new("core.encoding.binary", "unpack", "jet_std_binary_unpack", true, &[true, true]) .with_jit_symbol("jet_jit_binary_unpack") .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "iter_unpack", "jet_std_binary_iter_unpack", true, &[true, true]) .with_jit_symbol("jet_jit_binary_iter_unpack") .with_native_int_result(),
    CoreCallRecord::new( "core.encoding.hex", "encode", "jet_std_hex_encode", true, &[true], ),
    CoreCallRecord::new( "core.encoding.hex", "encode_upper", "jet_std_hex_encode_upper", true, &[true], ) .with_jit_symbol("jet_jit_hex_encode_upper"),
    CoreCallRecord::new( "core.encoding.hex", "is_hex", "jet_std_hex_is_hex", true, &[true], ) .with_jit_symbol("jet_jit_hex_is_hex"),
    CoreCallRecord::new( "core.encoding.hex", "decode", "jet_std_hex_decode", true, &[true], ),
    CoreCallRecord::new( "core.encoding.hex", "crc_hqx", "jet_std_crc_hqx", true, &[true, false], ) .with_jit_symbol("jet_jit_crc_hqx") .with_native_int_result(),
    CoreCallRecord::new( "core.encoding.hex", "crc32", "jet_std_crc32", true, &[true], ) .with_jit_symbol("jet_jit_crc32") .with_native_int_result(),
    CoreCallRecord::new( "core.encoding.hex", "b2a_base64", "jet_std_b64_encode", true, &[true], ),
    CoreCallRecord::new( "core.encoding.hex", "a2b_base64", "jet_std_b64_decodebytes", true, &[true], ) .with_jit_symbol("jet_jit_b64_decodebytes"),
    CoreCallRecord::new( "core.encoding.hex", "encode_sep", "jet_std_hex_encode_sep", true, &[true, true], ) .with_jit_symbol("jet_jit_hex_encode_sep"),
    CoreCallRecord::new( "core.encoding.hex", "dump", "jet_std_hex_dump", true, &[true], ) .with_jit_symbol("jet_jit_hex_dump"),
    CoreCallRecord::new( "core.encoding.base64", "encode", "jet_std_b64_encode", true, &[true], ),
    CoreCallRecord::new( "core.encoding.base64", "decode", "jet_std_b64_decode", true, &[true], ),
    CoreCallRecord::new( "core.encoding.base64", "decode_url", "jet_std_b64url_decode", true, &[true], ) .with_jit_symbol("jet_jit_b64_decode_url"),
    CoreCallRecord::new( "core.encoding.base64", "encode_url", "jet_std_b64url_encode", true, &[true], ) .with_jit_symbol("jet_jit_b64_encode_url"),
    CoreCallRecord::new( "core.encoding.base64", "encode_url_padded", "jet_std_b64url_encode_padded", true, &[true], ) .with_jit_symbol("jet_jit_b64_encode_url_padded"),
    CoreCallRecord::new( "core.encoding.base64", "pad", "jet_std_b64_pad", true, &[true], ) .with_jit_symbol("jet_jit_b64_pad"),
    CoreCallRecord::new( "core.encoding.base64", "unpad", "jet_std_b64_unpad", true, &[true], ) .with_jit_symbol("jet_jit_b64_unpad"),
    CoreCallRecord::new( "core.encoding.base64", "encodebytes", "jet_std_b64_encodebytes", true, &[true], ) .with_jit_symbol("jet_jit_b64_encodebytes"),
    CoreCallRecord::new( "core.encoding.base64", "decodebytes", "jet_std_b64_decodebytes", true, &[true], ) .with_jit_symbol("jet_jit_b64_decodebytes"),
    CoreCallRecord::new( "core.encoding.base32", "b32encode", "jet_std_base32_encode", true, &[true], ),
    CoreCallRecord::new( "core.encoding.base32", "b32decode", "jet_std_base32_decode", true, &[true], ),
    CoreCallRecord::new( "core.encoding.base32", "b32hexencode", "jet_std_base32hex_encode", true, &[true], ) .with_jit_symbol("jet_jit_base32hex_encode"),
    CoreCallRecord::new( "core.encoding.base32", "b32hexdecode", "jet_std_base32hex_decode", true, &[true], ) .with_jit_symbol("jet_jit_base32hex_decode"),
    CoreCallRecord::new("core.files", "open", "jet_std_files_open", true, &[true]) .with_jit_symbol("jet_jit_fs_open").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "create", "jet_std_files_create", true, &[true], ) .with_jit_symbol("jet_jit_fs_create").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "append", "jet_std_files_append", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_fs_append"),
    CoreCallRecord::new("core.net.url", "parse", "jet_url_parse", true, &[true]),
    CoreCallRecord::new( "core.net.url", "from_parts", "jet_url_from_parts", true, &[true, true, true, true, true], ),
    CoreCallRecord::new("core.net.url", "file", "jet_url_file", true, &[true]),
    CoreCallRecord::new("core.net.url", "data", "jet_url_data", true, &[true, true]),
    CoreCallRecord::new("core.net.url", "query", "jet_url_query", true, &[true]),
    CoreCallRecord::new( "core.net.url", "percent_encode", "jet_url_percent_encode_component", true, &[true], ),
    CoreCallRecord::new( "core.net.url", "percent_decode", "jet_url_percent_decode_component", true, &[true], ),
    CoreCallRecord::new("core.net.url", "geturl", "jet_url_to_string", true, &[true],),
    CoreCallRecord::new("core.net.url", "unparse", "jet_url_to_string", true, &[true],),
    CoreCallRecord::new("core.net.url", "urljoin", "jet_url_join", true, &[true, true],) .with_jit_symbol("jet_jit_url_join_text"),
    CoreCallRecord::new("core.net.url", "parse_qsl", "jet_url_parse_qsl", true, &[true],) .with_jit_symbol("jet_jit_url_parse_qsl"),
    CoreCallRecord::new("core.net.url", "parse_qs", "jet_url_parse_qsl", true, &[true],) .with_jit_symbol("jet_jit_url_parse_qsl"),
    CoreCallRecord::new("core.net.url", "urlencode", "jet_url_urlencode", true, &[true],) .with_jit_symbol("jet_jit_url_urlencode"),
    CoreCallRecord::new("core.net.url", "split_fragment", "jet_url_split_fragment", true, &[true],) .with_jit_symbol("jet_jit_url_split_fragment"),
    CoreCallRecord::new("core.net.url", "quote", "jet_url_quote", true, &[true],) .with_jit_symbol("jet_jit_url_quote"),
    CoreCallRecord::new("core.net.url", "quote_from_bytes", "jet_url_quote_from_bytes", true, &[true],) .with_jit_symbol("jet_jit_url_quote_from_bytes"),
    CoreCallRecord::new("core.net.url", "quote_plus", "jet_url_quote_plus", true, &[true],) .with_jit_symbol("jet_jit_url_quote_plus"),
    CoreCallRecord::new("core.net.url", "unquote", "jet_url_unquote", true, &[true],) .with_jit_symbol("jet_jit_url_unquote"),
    CoreCallRecord::new("core.net.url", "unquote_to_bytes", "jet_url_unquote_to_bytes", true, &[true],) .with_jit_symbol("jet_jit_url_unquote_to_bytes"),
    CoreCallRecord::new("core.net.url", "unquote_plus", "jet_url_unquote_plus", true, &[true],) .with_jit_symbol("jet_jit_url_unquote_plus"),
    CoreCallRecord::new("core.net.url", "urlparse", "jet_url_parse", true, &[true],),
    CoreCallRecord::new("core.net.url", "urlsplit", "jet_url_parse", true, &[true],),
    CoreCallRecord::new("core.net.url", "urlunparse", "jet_url_to_string", true, &[true],),
    CoreCallRecord::new("core.net.url", "urlunsplit", "jet_url_to_string", true, &[true],),
    CoreCallRecord::new("core.net.url", "urldefrag", "jet_url_urldefrag", true, &[true],) .with_jit_symbol("jet_jit_url_urldefrag"),
    CoreCallRecord::new("core.net.mime", "parse", "jet_mime_parse", true, &[true]) .with_pure_route(CoreCallPureRoute::Mime),
    CoreCallRecord::new( "core.net.mime", "from_extension", "jet_mime_from_extension", true, &[true], ) .with_pure_route(CoreCallPureRoute::Mime),
    CoreCallRecord::new( "core.net.mime", "extension", "jet_mime_extension", true, &[true], ) .with_pure_route(CoreCallPureRoute::Mime),
    CoreCallRecord::new("core.email", "address", "jet_email::address", true, &[true]) .with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::new("core.email", "limits_safe", "jet_email::Limits::safe", true, &[]) .with_jit_symbol("jet_jit_email_limits_safe"),
    CoreCallRecord::new( "core.email", "attachment", "jet_email::attachment", true, &[true, true, true], ) .with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::new( "core.email", "message", "jet_email::message", true, &[true, true, true, true, true, true, true], ) .with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::new( "core.email", "envelope", "jet_email::envelope", true, &[true, true], ) .with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::new( "core.email", "serialize", "jet_email::serialize", true, &[true], ) .with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::new("core.text", "nfc", "jet_text_nfc", true, &[true]),
    CoreCallRecord::new("core.text", "nfd", "jet_text_nfd", true, &[true]),
    CoreCallRecord::new("core.text", "nfkc", "jet_text_nfkc", true, &[true]),
    CoreCallRecord::new("core.text", "nfkd", "jet_text_nfkd", true, &[true]),
    CoreCallRecord::new("core.text", "casefold", "jet_text_casefold", true, &[true]),
    CoreCallRecord::new( "core.text", "caseless_eq", "jet_text_caseless_eq", true, &[true, true], ),
    CoreCallRecord::new("core.text", "lower", "jet_text_lower", true, &[true]),
    CoreCallRecord::new("core.text", "upper", "jet_text_upper", true, &[true]),
    CoreCallRecord::new( "core.text", "graphemes", "jet_text_graphemes", true, &[true], ),
    CoreCallRecord::new("core.text", "grapheme_views", "jet_text_grapheme_views", true, &[true]),
    CoreCallRecord::new("core.text", "words", "jet_text_words", true, &[true]),
    CoreCallRecord::new("core.text", "word_views", "jet_text_word_views", true, &[true]),
    CoreCallRecord::new( "core.text", "sentences", "jet_text_sentences", true, &[true], ),
    CoreCallRecord::new("core.text", "line_views", "jet_text_line_views", true, &[true]),
    CoreCallRecord::new("core.text", "byte_views", "jet_text_byte_views", true, &[true]),
    CoreCallRecord::new( "core.text", "scalar_count", "jet_text_unicode_scalar_count", true, &[true], ) .with_native_int_result(),
    CoreCallRecord::new( "core.text", "byte_count", "jet_text_unicode_byte_count", true, &[true], ) .with_native_int_result(),
    CoreCallRecord::new( "core.text", "is_alphabetic", "jet_text_is_alphabetic", true, &[true], ),
    CoreCallRecord::new( "core.text", "is_numeric", "jet_text_is_numeric", true, &[true], ),
    CoreCallRecord::new( "core.text", "is_whitespace", "jet_text_is_whitespace", true, &[true], ),
    CoreCallRecord::new( "core.text", "is_ascii", "jet_text_unicode_is_ascii", true, &[true], ),
    CoreCallRecord::new( "core.text", "scalars", "jet_text_unicode_scalars", true, &[true], ),
    CoreCallRecord::new( "core.text", "splitn", "jet_text_splitn", true, &[true, true, false], ),
    CoreCallRecord::new( "core.text", "rsplitn", "jet_text_rsplitn", true, &[true, true, false], ),
    CoreCallRecord::new("core.text", "trim", "jet_text_trim", true, &[true]),
    CoreCallRecord::new( "core.text", "trim_start", "jet_text_trim_start", true, &[true], ),
    CoreCallRecord::new("core.text", "trim_end", "jet_text_trim_end", true, &[true]),
    CoreCallRecord::new( "core.text", "pad_start", "jet_text_pad_start_i64", true, &[true, false, true], ),
    CoreCallRecord::new( "core.text", "pad_end", "jet_text_pad_end_i64", true, &[true, false, true], ),
    CoreCallRecord::new( "core.text", "center", "jet_text_center_i64", true, &[true, false, true], ),
    CoreCallRecord::new( "core.text", "starts_any", "jet_text_starts_any", true, &[true, true], ),
    CoreCallRecord::new( "core.text", "ends_any", "jet_text_ends_any", true, &[true, true], ),
    CoreCallRecord::new("core.text", "inspect", "jet_text_inspect", true, &[true]),
    CoreCallRecord::new( "core.text", "char_indices", "jet_text_char_indices", true, &[true], ),
    CoreCallRecord::new("core.text.parse", "startswith", "jet_string_starts_with", true, &[true, true]),
    CoreCallRecord::new("core.text.parse", "endswith", "jet_string_ends_with", true, &[true, true]),
    CoreCallRecord::new("core.text.parse", "removeprefix", "jet_text_remove_prefix", true, &[true, true]),
    CoreCallRecord::new("core.text.parse", "removesuffix", "jet_text_remove_suffix", true, &[true, true]),
    CoreCallRecord::new("core.text.parse", "rindex", "jet_text_rindex", true, &[true, true]) .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "encode", "jet_text_encode", true, &[true]),
    CoreCallRecord::new("core.text.parse", "isalpha", "jet_text_is_alphabetic", true, &[true]),
    CoreCallRecord::new("core.text.parse", "isdecimal", "jet_text_isdecimal", true, &[true]),
    CoreCallRecord::new("core.text.parse", "isnumeric", "jet_text_isnumeric", true, &[true]),
    CoreCallRecord::new("core.text.parse", "isalnum", "jet_text_isalnum", true, &[true]),
    CoreCallRecord::new("core.text.parse", "isascii", "jet_text_unicode_is_ascii", true, &[true]),
    CoreCallRecord::new("core.text.parse", "isdigit", "jet_text_isdigit", true, &[true]),
    CoreCallRecord::new("core.text.parse", "isidentifier", "jet_text_isidentifier", true, &[true]),
    CoreCallRecord::new("core.text.parse", "islower", "jet_text_is_lower", true, &[true]),
    CoreCallRecord::new("core.text.parse", "isspace", "jet_text_is_whitespace", true, &[true]),
    CoreCallRecord::new("core.text.parse", "istitle", "jet_text_istitle", true, &[true]),
    CoreCallRecord::new("core.text.parse", "isprintable", "jet_text_isprintable", true, &[true]),
    CoreCallRecord::new("core.text.parse", "isupper", "jet_text_is_upper", true, &[true]),
    CoreCallRecord::new("core.text.parse", "starts_with", "jet_string_starts_with", true, &[true, true]).with_jit_symbol("jet_string_starts_with"),
    CoreCallRecord::new("core.text.parse", "ends_with", "jet_string_ends_with", true, &[true, true]).with_jit_symbol("jet_string_ends_with"),
    CoreCallRecord::new("core.text.parse", "strip_prefix", "jet_text_remove_prefix", true, &[true, true]).with_jit_symbol("jet_text_remove_prefix"),
    CoreCallRecord::new("core.text.parse", "strip_suffix", "jet_text_remove_suffix", true, &[true, true]).with_jit_symbol("jet_text_remove_suffix"),
    CoreCallRecord::new("core.text.parse", "is_digit", "jet_text_isdigit", true, &[true]).with_jit_symbol("jet_text_isdigit"),
    CoreCallRecord::new("core.text.parse", "is_alnum", "jet_text_isalnum", true, &[true]).with_jit_symbol("jet_text_isalnum"),
    CoreCallRecord::new("core.text.parse", "is_space", "jet_text_is_whitespace", true, &[true]).with_jit_symbol("jet_text_is_whitespace"),
    CoreCallRecord::new("core.text.parse", "is_lower", "jet_text_is_lower", true, &[true]).with_jit_symbol("jet_text_is_lower"),
    CoreCallRecord::new("core.text.parse", "is_upper", "jet_text_is_upper", true, &[true]).with_jit_symbol("jet_text_is_upper"),
    CoreCallRecord::new("core.text.parse", "is_title", "jet_text_istitle", true, &[true]).with_jit_symbol("jet_text_istitle"),
    CoreCallRecord::new("core.text.parse", "is_ascii", "jet_text_unicode_is_ascii", true, &[true]).with_jit_symbol("jet_text_unicode_is_ascii"),
    CoreCallRecord::new("core.text.parse", "is_identifier", "jet_text_isidentifier", true, &[true]).with_jit_symbol("jet_text_isidentifier"),
    CoreCallRecord::new("core.text.parse", "expandtabs", "jet_text_expandtabs", true, &[true, false]),
    CoreCallRecord::new("core.text.parse", "capitalize", "jet_text_capitalize", true, &[true]) .with_jit_symbol("jet_jit_text_capitalize"),
    CoreCallRecord::new("core.text.parse", "lower", "jet_text_lower", true, &[true]) .with_jit_symbol("jet_jit_text_lower"),
    CoreCallRecord::new("core.text.parse", "swapcase", "jet_text_swapcase", true, &[true]) .with_jit_symbol("jet_jit_text_swapcase"),
    CoreCallRecord::new("core.text.parse", "title", "jet_text_title", true, &[true]) .with_jit_symbol("jet_jit_text_title"),
    CoreCallRecord::new("core.text.parse", "upper", "jet_text_upper", true, &[true]) .with_jit_symbol("jet_jit_text_upper"),
    CoreCallRecord::new("core.text.parse", "center", "jet_text_center_i64", true, &[true, false, true]).with_jit_symbol("jet_jit_text_center"),
    CoreCallRecord::new("core.text.parse", "ljust", "jet_text_pad_end_i64", true, &[true, false, true]).with_jit_symbol("jet_jit_text_pad_end"),
    CoreCallRecord::new("core.text.parse", "rjust", "jet_text_pad_start_i64", true, &[true, false, true]).with_jit_symbol("jet_jit_text_pad_start"),
    CoreCallRecord::new("core.text.parse", "zfill", "jet_text_zfill", true, &[true, false]).with_jit_symbol("jet_jit_text_zfill"),
    CoreCallRecord::new("core.text.parse", "split", "jet_text_parse_split", true, &[true, true]).with_jit_symbol("jet_text_parse_split"),
    CoreCallRecord::new("core.text.parse", "find", "jet_text_parse_find", true, &[true, true]).with_jit_symbol("jet_text_parse_find") .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "rfind", "jet_text_parse_rfind", true, &[true, true]).with_jit_symbol("jet_text_parse_rfind") .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "count", "jet_text_parse_count", true, &[true, true]).with_jit_symbol("jet_text_parse_count") .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "replace", "jet_text_parse_replace_n", true, &[true, true, true, false]).with_jit_symbol("jet_text_parse_replace_n"),
    CoreCallRecord::new("core.text.parse", "rsplit", "jet_text_parse_rsplit", true, &[true, true, false]).with_jit_symbol("jet_text_parse_rsplit"),
    CoreCallRecord::new("core.text.parse", "split_once", "jet_text_parse_split_once", true, &[true, true]).with_jit_symbol("jet_text_parse_split_once"),
    CoreCallRecord::new("core.text.parse", "partition", "jet_text_parse_partition", true, &[true, true]).with_jit_symbol("jet_text_parse_partition"),
    CoreCallRecord::new("core.text.parse", "rpartition", "jet_text_parse_rpartition", true, &[true, true]).with_jit_symbol("jet_text_parse_rpartition"),
    CoreCallRecord::new("core.text.parse", "split_ws", "jet_text_parse_split_ws", true, &[true]).with_jit_symbol("jet_text_parse_split_ws"),
    CoreCallRecord::new("core.text.parse", "parse_bool", "jet_text_parse_bool", true, &[true]).with_jit_symbol("jet_text_parse_bool"),
    CoreCallRecord::new("core.text.parse", "parse_int", "jet_text_parse_int", true, &[true]).with_jit_symbol("jet_text_parse_int") .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "parse", "jet_text_parse_int", true, &[true]).with_jit_symbol("jet_text_parse_int") .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "parse_int_base", "jet_text_parse_int_base", true, &[true, false]).with_jit_symbol("jet_text_parse_int_base") .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "parse_float", "jet_text_parse_float", true, &[true]).with_jit_symbol("jet_text_parse_float"),
    CoreCallRecord::new("core.text.parse", "parse_kv", "jet_text_parse_kv", true, &[true, true]).with_jit_symbol("jet_text_parse_kv"),
    CoreCallRecord::new("core.text.parse", "join", "jet_text_parse_join", true, &[true, true]).with_jit_symbol("jet_text_parse_join"),
    CoreCallRecord::new("core.text.parse", "splitlines", "jet_text_parse_splitlines", true, &[true, false]).with_jit_symbol("jet_text_parse_splitlines"),
    CoreCallRecord::new("core.text.parse", "capwords", "jet_text_parse_capwords", true, &[true]).with_jit_symbol("jet_text_parse_capwords"),
    CoreCallRecord::new("core.text.parse", "find_from", "jet_text_parse_find_from", true, &[true, true, false]).with_jit_symbol("jet_text_parse_find_from") .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "rfind_from", "jet_text_parse_rfind_from", true, &[true, true, false]).with_jit_symbol("jet_text_parse_rfind_from") .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "index", "jet_text_parse_index", true, &[true, true]).with_jit_symbol("jet_text_parse_index") .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "contains", "jet_text_parse_contains", true, &[true, true]).with_jit_symbol("jet_text_parse_contains"),
    CoreCallRecord::new("core.text.parse", "lstrip", "jet_text_parse_lstrip", true, &[true]).with_jit_symbol("jet_text_parse_lstrip"),
    CoreCallRecord::new("core.text.parse", "rstrip", "jet_text_parse_rstrip", true, &[true]).with_jit_symbol("jet_text_parse_rstrip"),
    CoreCallRecord::new("core.text.parse", "strip", "jet_text_parse_strip", true, &[true]).with_jit_symbol("jet_text_parse_strip"),
    CoreCallRecord::new("core.text.parse", "escape_c", "jet_text_parse_escape_c", true, &[true]).with_jit_symbol("jet_text_parse_escape_c"),
    CoreCallRecord::new("core.text.parse", "unescape_c", "jet_text_parse_unescape_c", true, &[true]).with_jit_symbol("jet_text_parse_unescape_c"),
    CoreCallRecord::new("core.text.wrap", "wrap", "jet_text_wrap", true, &[true, false]).with_jit_symbol("jet_jit_text_wrap"),
    CoreCallRecord::new("core.text.wrap", "fill", "jet_text_fill", true, &[true, false]).with_jit_symbol("jet_jit_text_fill"),
    CoreCallRecord::new("core.text.wrap", "shorten", "jet_text_shorten", true, &[true, false, true]).with_jit_symbol("jet_jit_text_shorten"),
    CoreCallRecord::new("core.text.wrap", "indent", "jet_text_indent", true, &[true, true]).with_jit_symbol("jet_jit_text_indent"),
    CoreCallRecord::new("core.text.wrap", "dedent", "jet_text_dedent", true, &[true]).with_jit_symbol("jet_jit_text_dedent"),
    CoreCallRecord::new("core.text.wrap", "expand_tabs", "jet_text_expand_tabs", true, &[true, false]).with_jit_symbol("jet_jit_text_expand_tabs"),
    CoreCallRecord::new("core.text.wrap", "html_escape", "jet_text_html_escape", true, &[true, false]).with_jit_symbol("jet_jit_text_html_escape"),
    CoreCallRecord::new("core.text.wrap", "html_unescape", "jet_text_html_unescape", true, &[true]).with_jit_symbol("jet_jit_text_html_unescape"),
    CoreCallRecord::new("core.text.wrap", "indent_with", "jet_text_indent_with", true, &[true, true, false]).with_jit_symbol("jet_jit_text_indent_with"),
    CoreCallRecord::new("core.text.wrap", "wrap_paragraphs", "jet_text_wrap_paragraphs", true, &[true, false]).with_jit_symbol("jet_jit_text_wrap_paragraphs"),
    CoreCallRecord::new("core.text.wrap", "hanging_indent", "jet_text_hanging_indent", true, &[true, true, true, false]).with_jit_symbol("jet_jit_text_hanging_indent"),
    CoreCallRecord::new("core.text.html", "escape", "jet_text_html_escape_quoted", true, &[true]).with_jit_symbol("jet_jit_text_html_escape_quoted"),
    CoreCallRecord::new("core.text.html", "escape_quote", "jet_text_html_escape", true, &[true, false]).with_jit_symbol("jet_jit_text_html_escape"),
    CoreCallRecord::new("core.text.html", "unescape", "jet_text_html_unescape", true, &[true]).with_jit_symbol("jet_jit_text_html_unescape"),
    CoreCallRecord::new("core.text.html", "strip_tags", "jet_text_html_strip_tags", true, &[true]).with_jit_symbol("jet_jit_text_html_strip_tags"),
    CoreCallRecord::new("core.text.html", "unescape_and_strip", "jet_text_html_unescape_and_strip", true, &[true]).with_jit_symbol("jet_jit_text_html_unescape_and_strip"),
    CoreCallRecord::new("core.text.html", "attr_escape", "jet_text_html_escape_quoted", true, &[true]).with_jit_symbol("jet_jit_text_html_escape_quoted"),
    CoreCallRecord::new("core.text.html", "text_escape", "jet_text_html_escape_text", true, &[true]).with_jit_symbol("jet_jit_text_html_escape_text"),
    CoreCallRecord::new("core.time.calendar", "isleap", "jet_calendar_isleap", true, &[false]).with_jit_symbol("jet_jit_calendar_isleap"),
    CoreCallRecord::new("core.time.calendar", "leapdays", "jet_calendar_leapdays", true, &[false, false]).with_jit_symbol("jet_jit_calendar_leapdays"),
    CoreCallRecord::new("core.time.calendar", "weekday", "jet_calendar_weekday", true, &[false, false, false]).with_jit_symbol("jet_jit_calendar_weekday"),
    CoreCallRecord::new("core.time.calendar", "monthrange", "jet_calendar_monthrange", true, &[false, false]).with_jit_symbol("jet_jit_calendar_monthrange"),
    CoreCallRecord::new("core.time.calendar", "monthcalendar", "jet_calendar_monthcalendar", true, &[false, false]).with_jit_symbol("jet_jit_calendar_monthcalendar"),
    CoreCallRecord::new("core.time.calendar", "monthcalendar_start", "jet_calendar_monthcalendar_start", true, &[false, false, false]).with_jit_symbol("jet_jit_calendar_monthcalendar_start"),
    CoreCallRecord::new("core.time.calendar", "yearcalendar", "jet_calendar_yearcalendar", true, &[false]).with_jit_symbol("jet_jit_calendar_yearcalendar"),
    CoreCallRecord::new("core.time.calendar", "weekheader", "jet_calendar_weekheader", true, &[false, false]).with_jit_symbol("jet_jit_calendar_weekheader"),
    CoreCallRecord::new("core.time.calendar", "formatmonth", "jet_calendar_formatmonth", true, &[false, false, false]).with_jit_symbol("jet_jit_calendar_formatmonth"),
    CoreCallRecord::new("core.time.calendar", "formatyear", "jet_calendar_formatyear", true, &[false]).with_jit_symbol("jet_jit_calendar_formatyear"),
    CoreCallRecord::new("core.time.calendar", "day_name", "jet_calendar_day_name", true, &[false]).with_jit_symbol("jet_jit_calendar_day_name"),
    CoreCallRecord::new("core.time.calendar", "day_abbr", "jet_calendar_day_abbr", true, &[false]).with_jit_symbol("jet_jit_calendar_day_abbr"),
    CoreCallRecord::new("core.time.calendar", "month_name", "jet_calendar_month_name", true, &[false]).with_jit_symbol("jet_jit_calendar_month_name"),
    CoreCallRecord::new("core.time.calendar", "month_abbr", "jet_calendar_month_abbr", true, &[false]).with_jit_symbol("jet_jit_calendar_month_abbr"),
    CoreCallRecord::new("core.time.calendar", "timegm", "jet_calendar_timegm", true, &[false, false, false, false, false, false]).with_jit_symbol("jet_jit_calendar_timegm"),
    CoreCallRecord::new("core.log", "info", "jet_ring_log_info", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "warn", "jet_ring_log_warn", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "error", "jet_ring_log_error", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "debug", "jet_ring_log_debug", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.log", "critical", "jet_ring_log_critical", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "fatal", "jet_ring_log_fatal", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "disable", "jet_ring_log_disable", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "flush", "jet_ring_log_flush", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "enabled", "jet_ring_log_enabled", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.log", "field", "jet_ring_log_field", true, &[true, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "int", "jet_ring_log_int", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.log", "float", "jet_ring_log_float", true, &[true, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.log", "bool", "jet_ring_log_bool", true, &[true, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "redact", "jet_ring_log_redact", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.log", "info_fields", "jet_ring_log_info_fields", true, &[true, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.log", "warn_fields", "jet_ring_log_warn_fields", true, &[true, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.log", "error_fields", "jet_ring_log_error_fields", true, &[true, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.log", "debug_fields", "jet_ring_log_debug_fields", true, &[true, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "span", "jet_ring_log_span", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "enter", "jet_ring_log_enter", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "close", "jet_ring_log_close", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.log", "set_sink", "jet_ring_log_set_sink", true, &[true, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.log", "sample_every", "jet_ring_log_sample_every", true, &[false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.log", "counter", "jet_ring_log_counter", true, &[true, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.log", "otlp_file", "jet_ring_log_otlp_file", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.log", "set_level", "jet_ring_log_set_level", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.log", "set_trace_id", "jet_ring_log_set_trace_id", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "setup", "jet_ring_log_setup", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.auth", "session_validate", "jet_auth_session_validate", true, &[true, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.auth", "session_show", "jet_auth_session_show", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.auth", "session_user", "jet_auth_session_user", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.auth", "session_cookie", "jet_auth_session_cookie", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.auth", "session_id", "jet_auth_session_id", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.auth", "register_user", "jet_auth_register_user", true, &[false, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot().with_jit_symbol("jet_jit_auth_register_user"),
    CoreCallRecord::new( "core.auth", "password_login", "jet_auth_password_login", true, &[false, false, false, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot().with_jit_symbol("jet_jit_auth_password_login"),
    CoreCallRecord::new( "core.auth", "magic_link_issue", "jet_auth_magic_link_issue", true, &[false, false, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot().with_jit_symbol("jet_jit_auth_magic_link_issue"),
    CoreCallRecord::new( "core.auth", "magic_link_consume", "jet_auth_magic_link_consume", true, &[false, false, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot().with_jit_symbol("jet_jit_auth_magic_link_consume"),
    CoreCallRecord::new( "core.auth", "oauth_begin", "jet_auth_oauth_begin", true, &[false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot().with_jit_symbol("jet_jit_auth_oauth_begin"),
    CoreCallRecord::new( "core.auth", "oauth_finish", "jet_auth_oauth_finish", true, &[false, false, false, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot().with_jit_symbol("jet_jit_auth_oauth_finish"),
    CoreCallRecord::new( "core.sync", "text_merge", "jet_sync_text_merge", true, &[true, true], ),
    CoreCallRecord::new( "core.sync", "text_show", "jet_sync_text_show", true, &[true], ),
    CoreCallRecord::new( "core.sync", "text_metadata", "jet_sync_text_metadata", true, &[true], ),
    CoreCallRecord::new( "core.sync", "counter_merge", "jet_sync_counter_merge", true, &[true, true], ),
    CoreCallRecord::new( "core.sync", "counter_value", "jet_sync_counter_value", true, &[true], ) .with_native_int_result(),
    CoreCallRecord::new("core.sync", "map_new", "jet_sync_map_new", true, &[]),
    CoreCallRecord::new( "core.sync", "map_get", "jet_sync_map_get", true, &[true, true], ),
    CoreCallRecord::new( "core.sync", "map_merge", "jet_sync_map_merge", true, &[true, true], ),
    CoreCallRecord::new("core.sync", "map_show", "jet_sync_map_show", true, &[true]),
    CoreCallRecord::new("core.sync", "list_new", "jet_sync_list_new", true, &[]),
    CoreCallRecord::new( "core.sync", "list_merge", "jet_sync_list_merge", true, &[true, true], ),
    CoreCallRecord::new( "core.sync", "list_show", "jet_sync_list_show", true, &[true], ),
    CoreCallRecord::new( "core.sync", "policy_allows", "jet_db_policy_allows", true, &[true, true, true], ),
    CoreCallRecord::new( "core.sync", "policy_show", "jet_db_policy_show", true, &[true], ),
    CoreCallRecord::new("core.net", "ip_addr", "jet_net_ip_addr", true, &[true]) .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "gethostname", "jet_std_os_hostname", true, &[]),
    CoreCallRecord::new( "core.net", "ip_to_string", "jet_net_ip_to_string", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new( "core.net", "ip_is_ipv4", "jet_net_ip_is_ipv4", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new( "core.net", "socket_addr", "jet_net_socket_addr", true, &[true, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "socket_addr_parse", "jet_net_socket_addr_parse", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new( "core.net", "socket_host", "jet_net_socket_host", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new( "core.net", "socket_port", "jet_net_socket_port", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net) .with_native_int_result(),
    CoreCallRecord::new( "core.net", "socket_to_string", "jet_net_socket_to_string", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new( "core.net", "tcp_listen", "jet_net_tcp_listen", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "tcp_listen_addr", "jet_net_tcp_listen_addr", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "tcp_accept", "jet_net_tcp_accept", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "tcp_connect", "jet_net_tcp_connect", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "tcp_connect_addr", "jet_net_tcp_connect_addr", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "tcp_connect_timeout", "jet_net_tcp_connect_timeout", true, &[true, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "tcp_connect_happy", "jet_net_tcp_connect_happy", true, &[true, false, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "set_timeout", "jet_net_set_timeout", true, &[true, false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_net_set_timeout"),
    CoreCallRecord::new("core.net", "set_read_timeout", "jet_net_set_read_timeout", true, &[true, false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_net_set_read_timeout"),
    CoreCallRecord::new("core.net", "set_write_timeout", "jet_net_set_write_timeout", true, &[true, false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_net_set_write_timeout"),
    CoreCallRecord::new( "core.net", "ready_readable", "jet_net_ready_readable", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new( "core.net", "ready_writable", "jet_net_ready_writable", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new( "core.net", "error_operation", "jet_net_error_operation", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new( "core.net", "error_address", "jet_net_error_address", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new( "core.net", "error_name", "jet_net_error_name", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new( "core.net", "error_message", "jet_net_error_message", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new( "core.net", "error_os_code", "jet_net_error_os_code", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net) .with_native_int_result(),
    CoreCallRecord::new( "core.net", "tcp_local_addr", "jet_net_tcp_local_addr", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "tcp_peer_addr", "jet_net_tcp_peer_addr", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "tcp_local_socket_addr", "jet_net_tcp_local_socket_addr", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "tcp_peer_socket_addr", "jet_net_tcp_peer_socket_addr", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "listener_local_socket_addr", "jet_net_listener_local_socket_addr", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "nodelay", "jet_net_nodelay", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "set_nodelay", "jet_net_set_nodelay", true, &[true, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "ttl", "jet_net_ttl", true, &[true]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "set_ttl", "jet_net_set_ttl", true, &[true, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "socket_type", "jet_net_socket_type", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "tcp_reply", "jet_net_tcp_reply", true, &[false, true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "udp_bind", "jet_net_udp_bind", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "udp_bind_addr", "jet_net_udp_bind_addr", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "udp_local_addr", "jet_net_udp_local_addr", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "udp_set_timeout", "jet_net_udp_set_timeout", true, &[true, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "udp_send_to", "jet_net_udp_send_to", true, &[true, true, true], ) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "udp_recv_from", "jet_net_udp_recv_from", true, &[true, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "udp_send_bytes_to", "jet_net_udp_send_bytes_to", true, &[true, true, true], ) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "udp_receive", "jet_net_udp_receive", true, &[true, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "udp_packet_data", "jet_net_udp_packet_data", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "udp_packet_addr", "jet_net_udp_packet_addr", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "udp_packet_bytes", "jet_net_udp_packet_bytes", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "udp_packet_original_len", "jet_net_udp_packet_original_len", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new( "core.net", "udp_packet_truncated", "jet_net_udp_packet_truncated", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "unix_listen", "jet_net_unix_listen", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "unix_accept", "jet_net_unix_accept", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "unix_read", "jet_net_unix_read", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_net_unix_read"),
    CoreCallRecord::new( "core.net", "unix_write_all_bytes", "jet_net_unix_write_all_bytes", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_net_unix_write_all_bytes"),
    CoreCallRecord::new( "core.net", "unix_connect", "jet_net_unix_connect", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_net_unix_connect"),
    CoreCallRecord::new( "core.net", "unix_shutdown", "jet_net_unix_shutdown", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "unix_close", "jet_net_unix_close", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_net_unix_close"),
    CoreCallRecord::new( "core.net", "dns_a", "jet_net_dns_a", true, &[true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_net_dns_a"),
    CoreCallRecord::new( "core.net", "dns_a_at", "jet_net_dns_a_at", true, &[true, true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_net_dns_a_at"),
    CoreCallRecord::new( "core.net", "dns_aaaa", "jet_net_dns_aaaa", true, &[true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_net_dns_aaaa"),
    CoreCallRecord::new( "core.net", "dns_aaaa_at", "jet_net_dns_aaaa_at", true, &[true, true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_net_dns_aaaa_at"),
    CoreCallRecord::new( "core.net", "dns_txt", "jet_net_dns_txt", true, &[true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_net_dns_txt"),
    CoreCallRecord::new( "core.net", "dns_txt_at", "jet_net_dns_txt_at", true, &[true, true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_net_dns_txt_at"),
    CoreCallRecord::new( "core.net", "dns_ptr", "jet_net_dns_ptr", true, &[true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_net_dns_ptr"),
    CoreCallRecord::new( "core.net", "dns_srv", "jet_net_dns_srv", true, &[true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_net_dns_srv"),
    CoreCallRecord::new( "core.net", "dns_srv_at", "jet_net_dns_srv_at", true, &[true, true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_net_dns_srv_at"),
    CoreCallRecord::new( "core.net.tls", "client", "jet_net_tls_client", false, &[false, true], ) .with_max_arity(2) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .with_jit_symbol("jet_jit_tls_client"),
    CoreCallRecord::new("core.net.tls", "read", "jet_net_tls_read_bytes", true, &[true, false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_tls_read_bytes"),
    CoreCallRecord::new("core.net.tls", "write", "jet_net_tls_write_bytes", true, &[true, true]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_tls_write_bytes") .with_native_int_result(),
    CoreCallRecord::new("core.net.tls", "write_all", "jet_net_tls_write_all_bytes", true, &[true, true]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_tls_write_all_bytes"),
    CoreCallRecord::new("core.net.tls", "write_text", "jet_net_tls_write_text", true, &[true, true]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_tls_write_text"),
    CoreCallRecord::new( "core.net", "getservbyname", "jet_net_getservbyname", true, &[true], ) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "getservbyport", "jet_net_getservbyport", true, &[false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "dns_srv_target", "jet_net_dns_srv_target", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new( "core.net", "dns_srv_port", "jet_net_dns_srv_port", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net) .with_native_int_result(),
    CoreCallRecord::new( "core.net", "dns_srv_priority", "jet_net_dns_srv_priority", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net) .with_native_int_result(),
    CoreCallRecord::new( "core.net", "dns_srv_weight", "jet_net_dns_srv_weight", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net) .with_native_int_result(),
    CoreCallRecord::new("core.http", "router", "jet_http_router_new", true, &[]) .with_jit_symbol("jet_jit_http_router_new"),
    CoreCallRecord::new( "core.http", "parse", "jet_http_parse_request", true, &[true], ),
    CoreCallRecord::new( "core.http", "dispatch", "jet_http_router_dispatch", true, &[true, false], ),
    CoreCallRecord::new("core.http", "reason_phrase", "jet_http_reason_phrase", true, &[false]).with_jit_symbol("jet_jit_http_reason_phrase"),
    CoreCallRecord::new("core.http", "basic_auth", "jet_http_basic_auth", true, &[true, true]).with_jit_symbol("jet_jit_http_basic_auth"),
    CoreCallRecord::new("core.http", "bearer_auth", "jet_http_bearer_auth", true, &[true]).with_jit_symbol("jet_jit_http_bearer_auth"),
    CoreCallRecord::new( "core.http", "serve", "jet_http_server_default", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.http", "get", "jet_http_client_get", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_http_client_get"),
    CoreCallRecord::new( "core.http.client", "get", "jet_http_client_get", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_http_client_get"),
    CoreCallRecord::new( "core.http.client", "post", "jet_http_client_post", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_http_client_post"),
    CoreCallRecord::new( "core.http.client", "request", "jet_http_client_request_new", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_http_client_request_new"),
    CoreCallRecord::new( "core.regex", "flags", "jet_std::jet_regex_flags", true, &[false, false, false], ),
    CoreCallRecord::new( "core.regex", "escape", "jet_std::jet_regex_escape", true, &[true], ),
    CoreCallRecord::new( "core.regex", "compile", "jet_std::jet_regex_compile", true, &[true], ),
    CoreCallRecord::new( "core.regex", "compile_with", "jet_std::jet_regex_compile_with", true, &[true, true], ),
    CoreCallRecord::new( "core.regex", "literal", "jet_std::jet_regex_literal", true, &[true], ),
    CoreCallRecord::new( "core.regex", "is_match", "jet_std::jet_regex_is_match", true, &[true, true], ),
    CoreCallRecord::new( "core.regex", "full_match", "jet_std::jet_regex_full_match", true, &[true, true], ),
    CoreCallRecord::new( "core.regex", "match", "jet_std::jet_regex_match", true, &[true, true], ),
    CoreCallRecord::new( "core.regex", "find", "jet_std::jet_regex_find", true, &[true, true], ),
    CoreCallRecord::new( "core.regex", "find_all", "jet_std::jet_regex_find_all", true, &[true, true], ),
    CoreCallRecord::new( "core.regex", "matches", "jet_std::jet_regex_matches", true, &[true, true], ),
    CoreCallRecord::new( "core.regex", "finditer", "jet_std::jet_regex_matches", true, &[true, true], ) .with_jit_symbol("jet_jit_regex_matches"),
    CoreCallRecord::new( "core.regex", "split", "jet_std::jet_regex_split", true, &[true, true], ),
    CoreCallRecord::new( "core.regex", "split_limit", "jet_std::jet_regex_split_limit", true, &[true, true, false], ),
    CoreCallRecord::new( "core.regex", "replace", "jet_std::jet_regex_replace", true, &[true, true, true], ),
    CoreCallRecord::new( "core.regex", "replace_first", "jet_std::jet_regex_replace_first", true, &[true, true, true], ),
    CoreCallRecord::new( "core.regex", "search", "jet_std::jet_regex_match", true, &[true, true], ),
    CoreCallRecord::new( "core.regex", "findall", "jet_std::jet_regex_find_all", true, &[true, true], ),
    CoreCallRecord::new( "core.regex", "fullmatch", "jet_std::jet_regex_full_match", true, &[true, true], ),
    CoreCallRecord::new( "core.regex", "sub", "jet_std::jet_regex_replace", true, &[true, true, true], ),
    CoreCallRecord::new( "core.game.raylib", "window_open", "jet_raylib_window_open", true, &[false, false, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.game.raylib", "window_should_close", "jet_raylib_window_should_close", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.game.raylib", "window_ready", "jet_raylib_window_ready", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.game.raylib", "begin_drawing", "jet_raylib_begin_drawing", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.game.raylib", "clear_background", "jet_raylib_clear_background", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.game.raylib", "draw_text", "jet_raylib_draw_text", true, &[true, false, false, false, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.game.raylib", "draw_rectangle", "jet_raylib_draw_rectangle", true, &[false, false, false, false, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.game.raylib", "end_drawing", "jet_raylib_end_drawing", true, &[], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.game.raylib", "close_window", "jet_raylib_close_window", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.game.raylib", "key_down", "jet_raylib_key_down", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.game.raylib", "set_target_fps", "jet_raylib_set_target_fps", true, &[false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.game.raylib", "load_sound", "jet_raylib_load_sound", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.game.raylib", "play_sound", "jet_raylib_play_sound", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.game.raylib", "color", "jet_raylib_color", true, &[false, false, false, false], ) .with_pure_route(CoreCallPureRoute::Raylib),
    CoreCallRecord::new( "core.game.raylib", "gamepad_down", "jet_raylib_gamepad_down", true, &[false, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.game.raylib", "gamepad_axis", "jet_raylib_gamepad_axis", true, &[false, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.game.raylib", "load_texture_atlas", "jet_raylib_load_texture_atlas", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.game.raylib", "draw_sprite", "jet_raylib_draw_sprite", true, &[true, true, false, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.db", "open", "jet_db_open", true, &[true], ) .with_jit_symbol("jet_jit_db_open").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.db", "open_memory", "jet_db_open_memory", true, &[], ) .with_jit_symbol("jet_jit_db_open_memory").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.db", "pool", "jet_db_pool_new", true, &[true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_db_pool_new"),
    CoreCallRecord::new( "core.db", "policy", "jet_db_policy_checked", true, &[true, true], ) .with_jit_symbol("jet_jit_db_policy").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.jobs", "queue", "jet_job_queue_default", true, &[], ) .with_jit_symbol("jet_jit_job_queue_default").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "enqueue", "jet_job_queue_enqueue", true, &[true, true, true], ) .with_max_arity(4) .with_jit_symbol("jet_jit_job_queue_enqueue"),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "delay", "jet_job_queue_delay", true, &[true, false, false, false], ) .with_jit_symbol("jet_jit_job_queue_delay"),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "receipt", "jet_job_queue_receipt", true, &[true, true], ) .with_jit_symbol("jet_jit_job_queue_receipt"),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "inspect", "jet_job_queue_inspect", true, &[true, false, false], ) .with_jit_symbol("jet_jit_job_queue_inspect"),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "events", "jet_job_queue_events", true, &[true, true], ) .with_jit_symbol("jet_jit_job_queue_events"),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "claim", "jet_job_queue_claim", true, &[true, true, false], ) .with_jit_symbol("jet_jit_job_queue_claim"),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "heartbeat", "jet_job_queue_heartbeat", true, &[true, true], ) .with_jit_symbol("jet_jit_job_queue_heartbeat"),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "acknowledge", "jet_job_queue_acknowledge", true, &[true, true, false], ) .with_jit_symbol("jet_jit_job_queue_acknowledge"),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "fail", "jet_job_queue_fail", true, &[true, true, false], ) .with_jit_symbol("jet_jit_job_queue_fail"),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "cancel", "jet_job_queue_cancel", true, &[true, true, true], ) .with_max_arity(4) .with_jit_symbol("jet_jit_job_queue_cancel"),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "dead_letter", "jet_job_queue_dead_letter", true, &[true, true, true], ) .with_max_arity(4) .with_jit_symbol("jet_jit_job_queue_dead_letter"),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "recover_expired", "jet_job_queue_recover_expired", true, &[true], ) .with_jit_symbol("jet_jit_job_queue_recover_expired"),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "status", "jet_job_queue_status", true, &[true], ) .with_jit_symbol("jet_jit_job_queue_status"),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "pause", "jet_job_queue_pause", true, &[true], ) .with_jit_symbol("jet_jit_job_queue_pause"),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "resume", "jet_job_queue_resume", true, &[true], ) .with_jit_symbol("jet_jit_job_queue_resume"),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "wait", "jet_job_queue_wait", true, &[true, false], ) .with_jit_symbol("jet_jit_job_queue_wait"),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "prune", "jet_job_queue_prune", true, &[true], ) .with_jit_symbol("jet_jit_job_queue_prune") .with_native_int_result(),
    CoreCallRecord::new( "core.db", "policy_audit", "jet_db_policy_audit", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_db_policy_audit"),
    CoreCallRecord::new( "core.db", "row_value", "jet_std::jet_db_row_value", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_db_row_value"),
    CoreCallRecord::new( "core.db", "decode", "jet_db_decode", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_db_decode"),
    CoreCallRecord::new( "core.db", "row_int", "jet_std::jet_db_row_int", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_db_row_int") .with_native_int_result(),
    CoreCallRecord::new( "core.db", "row_float", "jet_std::jet_db_row_float", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_db_row_float"),
    CoreCallRecord::new( "core.db", "row_text", "jet_std::jet_db_row_text", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_db_row_text"),
    CoreCallRecord::new( "core.db", "row_bool", "jet_std::jet_db_row_bool", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_db_row_bool"),
    CoreCallRecord::new( "core.db", "transaction", "jet_db_scope_transaction", false, &[true, true, true], ) .without_direct_aot() .without_direct_jit() .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_db_transaction") .with_native_int_result(),
    CoreCallRecord::new( "core.db", "migrate", "jet_db_scope_migrate", false, &[true, true, true], ) .without_direct_aot() .without_direct_jit() .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_db_migrate") .with_native_int_result(),
    CoreCallRecord::new("core.term", "read_key", "jet_term_read_key", true, &[]),
    CoreCallRecord::new("core.perf", "fidelity", "jet_perf_fidelity", false, &[]),
    CoreCallRecord::new( "core.perf", "default_fidelity", "jet_perf_default_fidelity", false, &[], ),
    CoreCallRecord::new( "core.perf", "override_fidelity", "jet_perf_override_fidelity", false, &[false], ),
    CoreCallRecord::new( "core.perf", "reset_fidelity", "jet_perf_reset_fidelity", false, &[], ),
    CoreCallRecord::new("core.ui", "null_backend", "jet_ui_null", true, &[]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "tui_backend", "jet_ui_tui", true, &[]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "gtk_backend", "jet_ui_gtk", true, &[]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "point", "jet_ui_point", true, &[false, false]) .with_pure_route(CoreCallPureRoute::Ui) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "size", "jet_ui_size", true, &[false, false]) .with_pure_route(CoreCallPureRoute::Ui) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui", "rect", "jet_ui_rect", true, &[false, false, false, false], ) .with_pure_route(CoreCallPureRoute::Ui) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui", "constraint", "jet_ui_constraint", true, &[false, false, false, false], ) .with_pure_route(CoreCallPureRoute::Ui) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui", "node", "jet_ui_node", true, &[true, false, false], ) .with_pure_route(CoreCallPureRoute::Ui) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "text", "jet_ui_text", true, &[true]) .with_pure_route(CoreCallPureRoute::Ui) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui", "preview", "jet_ui_preview_with_viewport", true, &[true, false, false], ) .with_max_arity(3) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui", "playground", "jet_ui_playground_with_viewport", true, &[true, false, false], ) .with_max_arity(3) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "previews", "jet_ui_previews", true, &[true]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "playgrounds", "jet_ui_playgrounds", true, &[true]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "phone", "jet_ui_phone", true, &[]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "tablet", "jet_ui_tablet", true, &[]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "desktop", "jet_ui_desktop", true, &[]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "button", "jet_ui_button", true, &[true]) .with_max_arity(4) .with_pure_route(CoreCallPureRoute::Ui) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "key_event", "jet_ui_key_event", true, &[true]) .with_pure_route(CoreCallPureRoute::Ui) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui", "resize_event", "jet_ui_resize_event", true, &[false, false], ) .with_pure_route(CoreCallPureRoute::Ui) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui", "node_role", "jet_ui_node_role", true, &[true, false, false, false], ) .with_pure_route(CoreCallPureRoute::Ui) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui", "node_color", "jet_ui_node_color", true, &[true, false, false, true], ) .with_pure_route(CoreCallPureRoute::Ui) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui", "aria_role_button", "jet_ui_aria_role_button", true, &[], ) .with_pure_route(CoreCallPureRoute::Ui) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui", "aria_role_text_input", "jet_ui_aria_role_text_input", true, &[], ) .with_pure_route(CoreCallPureRoute::Ui) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui", "aria_role_label", "jet_ui_aria_role_label", true, &[], ) .with_pure_route(CoreCallPureRoute::Ui) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui", "aria_role_container", "jet_ui_aria_role_container", true, &[], ) .with_pure_route(CoreCallPureRoute::Ui) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.font", "system", "jet_font_system", true, &[false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.font", "shape", "jet_font_shape", true, &[true, true]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_ui_capabilities(&["UI.FontShaping"]),
    CoreCallRecord::new( "core.ui", "node_accessibility", "jet_ui_node_accessibility", true, &[false, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "node_shortcut", "jet_ui_node_shortcut", true, &[false, false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "text_input", "jet_ui_text_input", true, &[true, false]) .with_max_arity(3) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "reactive_render", "jet_ui_reactive_render", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_jit_symbol("jet_jit_core_ui_reactive_render"),
    CoreCallRecord::new("core.tui", "ascii", "jet_tui_ascii", true, &[true]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "capabilities", "jet_tui_capabilities", true, &[]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "close_event", "jet_tui_close_event", true, &[]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "color_ansi16", "jet_tui_color_ansi16", true, &[false]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "color_ansi256", "jet_tui_color_ansi256", true, &[false]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "color_rgb", "jet_tui_color_rgb", true, &[false, false, false]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "display_width", "jet_tui_display_width_int", true, &[true]) .with_pure_route(CoreCallPureRoute::Ui) .with_native_int_result(),
    CoreCallRecord::new("core.tui", "fill", "jet_tui_fill", true, &[false]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "focus_event", "jet_tui_focus_event", true, &[false]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "horizontal", "jet_tui_horizontal", true, &[]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "interrupt_event", "jet_tui_interrupt_event", true, &[]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "io_event", "jet_tui_io_event", true, &[true, false]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "key_event", "jet_tui_key_event", true, &[true]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new( "core.tui", "key_event_modifiers", "jet_tui_key_event_modifiers", true, &[true, false], ) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "layout", "jet_tui_layout", true, &[true, false, false]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "length", "jet_tui_length", true, &[false]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "list", "jet_tui_list", true, &[false]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "list_state", "jet_tui_list_state", true, &[]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new( "core.tui", "list_state_offset", "jet_tui_list_state_offset", true, &[false, false], ) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new( "core.tui", "list_state_select", "jet_tui_list_state_select", true, &[false, false], ) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new( "core.tui", "list_state_selected", "jet_tui_list_state_selected", true, &[false], ) .with_pure_route(CoreCallPureRoute::Ui) .with_native_int_result(),
    CoreCallRecord::new("core.tui", "max", "jet_tui_max", true, &[false]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "min", "jet_tui_min", true, &[false]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "percent", "jet_tui_percent", true, &[false]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "resize_event", "jet_tui_resize_event", true, &[false, false]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "style", "jet_tui_style", true, &[]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new( "core.tui", "style_background", "jet_tui_style_background", true, &[false, false], ) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "style_bold", "jet_tui_style_bold", true, &[false, false]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new( "core.tui", "style_dim", "jet_tui_style_dim", true, &[false, false], ) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new( "core.tui", "style_foreground", "jet_tui_style_foreground", true, &[false, false], ) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "style_text", "jet_tui_style_text", true, &[true, false, false]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new( "core.tui", "style_underline", "jet_tui_style_underline", true, &[false, false], ) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "table", "jet_tui_table", true, &[false, false]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "timer_event", "jet_tui_timer_event", true, &[true, false]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.tui", "vertical", "jet_tui_vertical", true, &[]) .with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new( "core.ui.host", "capabilities", "jet_ui_host_capabilities", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui.host", "file_filter", "jet_ui_host_file_filter", true, &[true, true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui.host", "file_filter_text", "jet_ui_host_file_filter_text", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui.host", "fs_rights_read", "jet_ui_host_fs_rights_read", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui.host", "fs_rights_write", "jet_ui_host_fs_rights_write", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui.host", "fs_rights_read_write", "jet_ui_host_fs_rights_read_write", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui.host", "fs_grant", "jet_ui_host_fs_grant", true, &[true, false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui.host", "open_request", "jet_ui_host_open_request", true, &[false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui.host", "save_request", "jet_ui_host_save_request", true, &[false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui.host", "open_file", "jet_ui_host_open_file", true, &[false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_ui_capabilities(&["UI.FileDialog"]),
    CoreCallRecord::new("core.ui.host", "save_file", "jet_ui_host_save_file", true, &[false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_ui_capabilities(&["UI.FileDialog"]),
    CoreCallRecord::new("core.ui.host", "shortcut", "jet_ui_host_shortcut", true, &[true, false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_ui_capabilities(&["UI.Shortcuts"]),
    CoreCallRecord::new( "core.ui.host", "accessibility", "jet_ui_host_accessibility", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_ui_capabilities(&["UI.Accessibility"]),
    CoreCallRecord::new( "core.ui.host.clipboard", "read_text", "jet_ui_host_clipboard_read_text", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_ui_capabilities(&["UI.Clipboard"]),
    CoreCallRecord::new( "core.ui.host.clipboard", "write_text", "jet_ui_host_clipboard_write_text", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_ui_capabilities(&["UI.Clipboard"]),
    CoreCallRecord::new("core.ui.host.ime", "poll", "jet_ui_host_ime_poll", true, &[]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_ui_capabilities(&["UI.IME"]),
    CoreCallRecord::new("core.ui.host.drag_drop", "poll", "jet_ui_host_drag_poll", true, &[]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_ui_capabilities(&["UI.DragDrop"]),
    CoreCallRecord::new( "core.ui.host.shortcuts", "binding", "jet_ui_host_shortcut_binding", true, &[false, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_ui_capabilities(&["UI.Shortcuts"]),
    CoreCallRecord::new( "core.ui.host.shortcuts", "register", "jet_ui_host_shortcuts_register", true, &[false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_ui_capabilities(&["UI.Shortcuts"]),
    CoreCallRecord::new( "core.ui.host.shortcuts", "dispatch", "jet_ui_host_shortcuts_dispatch", true, &[false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_ui_capabilities(&["UI.Shortcuts"]),
    CoreCallRecord::new( "core.ui.host.accessibility", "attach", "jet_ui_host_attach_accessibility", true, &[false, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_ui_capabilities(&["UI.Accessibility"]),
    CoreCallRecord::new( "core.ui.host.accessibility", "project", "jet_ui_host_project_accessibility", true, &[false, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_ui_capabilities(&["UI.Accessibility"]),
    sema_web_call("core.web", "openapi", "jet_web_openapi", &[true]),
    sema_web_call("core.web", "form", "jet_web_form", &[true, false]),
    sema_web_call("core.web.router", "new", "jet_web_router_new", &[]),
    sema_web_call("core.web.router", "navigate", "jet_web_router_navigate", &[true, false]),
    sema_web_call( "core.web.router", "route", "jet_web_router_route", &[true, false, false, false, false], ),
    sema_web_call( "core.web.router", "route_with_search_codec", "jet_web_router_route_with_search_codec", &[true, false, false, false, false, false], ),
    sema_web_call( "core.web.router", "not_found", "jet_web_router_not_found", &[true, false], ),
    sema_web_call( "core.web.router", "preload", "jet_web_router_preload", &[true, false], ),
    sema_web_call("core.web.router", "abort", "jet_web_router_abort", &[true]),
    sema_web_call( "core.web.router", "stale", "jet_web_router_stale", &[true, false], ) .with_native_int_result(),
    sema_web_call( "core.web.router", "invalidate", "jet_web_router_invalidate", &[true, false], ) .with_native_int_result(),
    sema_web_call( "core.web.router", "collect", "jet_web_router_collect", &[true, false], ) .with_native_int_result(),
    sema_web_call( "core.web.router", "cache_state", "jet_web_router_cache_state", &[true, false], ),
    sema_web_call( "core.web.router", "cache_show", "jet_web_router_cache_show", &[true], ),
    sema_web_call("core.web.router", "current", "jet_web_router_current", &[true]),
    sema_web_call( "core.web.router", "link", "jet_web_router_link", &[true, false, false, false], ),
    sema_web_call("core.web.query", "new", "jet_web_query_new", &[false, true]),
    sema_web_call( "core.web.query", "live", "jet_web_query_live", &[false, false, false, false], ),
    sema_web_call( "core.web.query", "mutate", "jet_web_query_mutate", &[true, false, false, false], ),
    sema_web_call( "core.web.query", "mutate_with_invalidations", "jet_web_query_mutate_with_invalidations", &[true, false, false, false, false], ),
    sema_web_call( "core.web.query", "retry", "jet_web_query_retry", &[true, false], ) .with_native_int_result(),
    sema_web_call( "core.web.query", "subscribe", "jet_web_query_subscribe", &[false], ),
    sema_web_call( "core.web.query", "invalidate", "jet_web_query_invalidate", &[true], ) .with_native_int_result(),
    sema_web_call("core.web.query", "get", "jet_web_query_get", &[true]),
    sema_web_call("core.web.query", "state", "jet_web_query_state", &[true]),
    sema_web_call("core.web.query", "state_signal", "jet_web_query_state_signal", &[true]),
    sema_web_call("core.web.query", "mutation_signal", "jet_web_query_mutation_signal", &[true]),
    sema_web_call("core.web.query", "show", "jet_web_query_show", &[true]),
    sema_web_call( "core.web.query", "queue", "jet_web_query_queue", &[true, false], ) .with_native_int_result(),
    sema_web_call( "core.web.query", "refresh", "jet_web_query_refresh", &[true], ),
    sema_web_call("core.web.query", "facts", "jet_web_query_facts", &[true]),
    sema_web_call("core.web.query", "cancel", "jet_web_query_cancel", &[true]),
    sema_web_call( "core.web.query", "set_online", "jet_web_query_set_online", &[true, false], ),
    sema_web_call( "core.web.query", "set_mode", "jet_web_query_set_mode", &[true, false], ),
    sema_web_call( "core.web.browser", "config", "jet_browser_test_config", &[], ),
    sema_web_call( "core.web.browser", "config_from_env", "jet_browser_test_config_from_env", &[], ),
    sema_web_call( "core.web.browser", "begin_named", "jet_browser_test_begin_named", &[true, true, true, false, true, false, false], ),
    sema_web_call( "core.web.browser", "generate_source", "jet_browser_test_generate_source", &[true, true, true], ),
    sema_web_call( "core.web.browser", "selected", "jet_browser_test_selected", &[true, true], ),
    sema_web_call( "core.web.browser", "report_new", "jet_browser_test_report_new", &[true], ),
    sema_web_call( "core.web.browser", "report_add_case", "jet_browser_test_report_add_case", &[false, false], ),
    sema_web_call( "core.web.browser", "report_json", "jet_browser_test_report_json", &[true], ),
    sema_web_call( "core.web.browser", "report_text", "jet_browser_test_report_text", &[true], ),
    sema_web_call( "core.web.browser", "report_html", "jet_browser_test_report_html", &[true], ),
    sema_web_call( "core.web.browser", "write_report", "jet_browser_test_write_report", &[true, true], ),
    sema_web_call( "core.web.browser", "report_exit_code", "jet_browser_test_report_exit_code", &[true], ) .with_native_int_result(),
    sema_web_call( "core.web.browser", "server_start", "jet_browser_test_server_start", &[true], ),
    sema_web_call( "core.web.browser", "server_stop", "jet_browser_test_server_stop", &[true], ),
    sema_web_call( "core.web.browser", "server_url", "jet_browser_test_server_url", &[true], ),
    sema_web_call( "core.web.browser", "server_logs", "jet_browser_test_server_logs", &[true], ),
    sema_web_call( "core.web.browser", "watch_changed", "jet_browser_test_watch_changed", &[true, false], ),
    sema_web_call("core.web.forms", "new", "jet_web_forms_new", &[false]),
    sema_web_call( "core.web.forms", "field", "jet_web_forms_field", &[true, false, false, false], ),
    sema_web_call("core.web.forms", "set", "jet_web_forms_set", &[true, false, false]),
    sema_web_call("core.web.forms", "blur", "jet_web_forms_blur", &[true, false]),
    sema_web_call( "core.web.forms", "validate", "jet_web_forms_validate", &[true], ),
    sema_web_call( "core.web.forms", "validate_async", "jet_web_forms_validate_async", &[true], ),
    sema_web_call( "core.web.forms", "submit", "jet_web_forms_submit", &[true], ),
    sema_web_call( "core.web.forms", "no_script", "jet_web_forms_no_script", &[true], ),
    sema_web_call("core.web.forms", "html", "jet_web_forms_html", &[true]),
    sema_web_call("core.web.forms", "show", "jet_web_forms_show", &[true]),
    sema_web_call( "core.web.forms", "input", "jet_web_forms_input", &[false, false], ),
    sema_web_call( "core.web.forms", "input_rename", "jet_web_forms_input_rename", &[false, false, false], ),
    sema_web_call( "core.web.forms", "input_exclude", "jet_web_forms_input_exclude", &[false, false], ),
    sema_web_call( "core.web.forms", "input_group", "jet_web_forms_input_group", &[false, false, false], ),
    sema_web_call( "core.web.forms", "input_replace", "jet_web_forms_input_replace", &[false, false, false], ),
    sema_web_call("core.web.forms", "typed", "jet_web_forms_typed", &[true, false]),
    sema_web_call( "core.web.forms", "typed_set", "jet_web_forms_typed_set", &[true, false, false], ),
    sema_web_call( "core.web.forms", "typed_set_async_validator", "jet_web_forms_typed_set_async_validator", &[true, false, false, false, false], ),
    sema_web_call( "core.web.forms", "typed_set_action", "jet_web_forms_typed_set_action", &[true, false], ),
    sema_web_call("core.web.forms", "action_error", "jet_web_forms_action_error", &[]),
    sema_web_call( "core.web.forms", "action_field_error", "jet_web_forms_action_field_error", &[false, false, false], ),
    sema_web_call( "core.web.forms", "action_form_error", "jet_web_forms_action_form_error", &[false, false], ),
    sema_web_call( "core.web.forms", "typed_blur", "jet_web_forms_typed_blur", &[true, false], ),
    sema_web_call( "core.web.forms", "typed_validate", "jet_web_forms_typed_validate", &[true], ),
    sema_web_call( "core.web.forms", "typed_validate_async", "jet_web_forms_typed_validate_async", &[true], ),
    sema_web_call( "core.web.forms", "typed_validate_field", "jet_web_forms_typed_validate_field", &[true, false, false, false], ),
    sema_web_call( "core.web.forms", "typed_validation_render", "jet_web_forms_typed_validation_render", &[true], ),
    sema_web_call( "core.web.forms", "typed_submit", "jet_web_forms_typed_submit", &[true], ),
    sema_web_call( "core.web.forms", "typed_submit_async", "jet_web_forms_typed_submit_async", &[true], ),
    sema_web_call( "core.web.forms", "typed_no_script", "jet_web_forms_typed_no_script", &[true], ),
    sema_web_call( "core.web.forms", "typed_post", "jet_web_forms_typed_post", &[true, false], ),
    sema_web_call( "core.web.forms", "typed_decode_post", "jet_web_forms_typed_decode_post", &[true, false], ),
    sema_web_call( "core.web.forms", "typed_html", "jet_web_forms_typed_html", &[true], ),
    sema_web_call( "core.web.forms", "typed_show", "jet_web_forms_typed_show", &[true], ),
    sema_web_call( "core.web.forms", "typed_state", "jet_web_forms_typed_state", &[true], ),
    sema_web_call( "core.web.forms", "typed_lifecycle", "jet_web_forms_typed_lifecycle", &[true], ),
    sema_web_call( "core.web.forms", "typed_errors", "jet_web_forms_typed_errors", &[true], ),
    sema_web_call( "core.web.forms", "typed_focus", "jet_web_forms_typed_focus", &[true, false], ),
    sema_web_call( "core.web.forms", "typed_cancel", "jet_web_forms_typed_cancel", &[true], ),
    sema_web_call( "core.web.forms", "typed_select_field", "jet_web_forms_typed_select_field", &[true, false], ),
    sema_web_call( "core.web.forms", "typed_validation_wait", "jet_web_forms_typed_validation_wait", &[false], ),
    sema_web_call( "core.web.forms", "typed_validation_cancel", "jet_web_forms_typed_validation_cancel", &[true], ),
    sema_web_call( "core.web.forms", "typed_submission_wait", "jet_web_forms_typed_submission_wait", &[false], ),
    sema_web_call( "core.web.forms", "typed_submission_cancel", "jet_web_forms_typed_submission_cancel", &[true], ),
    sema_web_call("core.web.table", "new", "jet_web_table", &[false, false]),
    sema_web_call( "core.web.table", "new_keyed", "jet_web_table_new_keyed", &[false, false, false], ),
    sema_web_call( "core.web.table", "column", "jet_web_table_column", &[false, false, false], ),
    sema_web_call("core.web.table", "sort", "jet_web_table_sort", &[true, false, false]),
    sema_web_call( "core.web.table", "filter", "jet_web_table_filter", &[true, false], ),
    sema_web_call("core.web.table", "page", "jet_web_table_page", &[true, false, false]),
    sema_web_call( "core.web.table", "with_column", "jet_web_table_with_column", &[true, false], ),
    sema_web_call( "core.web.table", "with_server_page", "jet_web_table_with_server_page", &[true, false], ),
    sema_web_call("core.web.table", "state", "jet_web_table_state", &[true]),
    sema_web_call("core.web.table", "facts", "jet_web_table_facts", &[true]),
    sema_web_call("core.web.table", "keys", "jet_web_table_keys", &[true]),
    sema_web_call( "core.web.table", "page_state", "jet_web_table_page_state", &[true], ),
    sema_web_call( "core.web.table", "sort_by", "jet_web_table_sort_by", &[true, false, false], ),
    sema_web_call( "core.web.table", "filter_by", "jet_web_table_filter_by", &[true, false, false], ),
    sema_web_call( "core.web.table", "paginate", "jet_web_table_paginate", &[true, false, false], ),
    sema_web_call( "core.web.table", "set_rows", "jet_web_table_set_rows", &[true, false], ),
    sema_web_call( "core.web.table", "set_selected", "jet_web_table_set_selected", &[true, false, false], ),
    sema_web_call( "core.web.table", "toggle_selection", "jet_web_table_toggle_selection", &[true, false], ),
    sema_web_call( "core.web.table", "focus", "jet_web_table_focus", &[true, false], ),
    sema_web_call( "core.web.table", "clear_focus", "jet_web_table_clear_focus", &[true], ),
    sema_web_call( "core.web.table", "focused_key", "jet_web_table_focused_key", &[true], ),
    sema_web_call( "core.web.table", "clear_selection", "jet_web_table_clear_selection", &[true], ),
    sema_web_call( "core.web.table", "selected_keys", "jet_web_table_selected_keys", &[true], ),
    sema_web_call( "core.web.table", "selected_rows", "jet_web_table_selected_rows", &[true], ),
    sema_web_call( "core.web.table", "insert_row", "jet_web_table_insert_row", &[true, false], ),
    sema_web_call( "core.web.table", "replace_row", "jet_web_table_replace_row", &[true, false, false], ),
    sema_web_call( "core.web.table", "update_row", "jet_web_table_update_row", &[true, false, false], ),
    sema_web_call( "core.web.table", "remove_row", "jet_web_table_remove_row", &[true, false], ),
    sema_web_call( "core.web.table", "first_page", "jet_web_table_first_page", &[true], ),
    sema_web_call( "core.web.table", "next_page", "jet_web_table_next_page", &[true], ),
    sema_web_call( "core.web.table", "last_page", "jet_web_table_last_page", &[true], ),
    sema_web_call( "core.web.table", "visible_rows", "jet_web_table_visible_rows", &[true, true], ),
    sema_web_call( "core.web.virtual", "plan_measure", "jet_web_virtual_plan_measure", &[true, false, false], ),
    sema_web_call( "core.web.virtual", "plan_viewport_state", "jet_web_virtual_plan_viewport_state", &[true], ),
    sema_web_call( "core.web.virtual", "plan_scroll_to", "jet_web_virtual_plan_scroll_to", &[true, false], ),
    sema_web_call( "core.web.virtual", "plan_resize", "jet_web_virtual_plan_resize", &[true, false, false], ),
    sema_web_call( "core.web.virtual", "plan_viewport_measure", "jet_web_virtual_plan_viewport_measure", &[true, false, false], ),
    sema_web_call( "core.web.virtual", "plan_facts", "jet_web_virtual_plan_facts", &[true], ),
    sema_web_call( "core.web.virtual", "window", "jet_web_virtual_window", &[false, false, false, false, false], ),
    sema_web_call( "core.web.virtual", "window_measured", "jet_web_virtual_window_measured", &[false, false, false, false, false, true], ),
    sema_web_call( "core.web.virtual", "slice", "jet_web_virtual_slice", &[true, true], ),
    sema_web_call( "core.web.virtual", "indices", "jet_web_virtual_indices", &[true], ) .with_native_int_result(),
    sema_web_call( "core.web.virtual", "plan", "jet_web_virtual_plan", &[false, false, false, false, false, false], ),
    sema_web_call( "core.web.virtual", "plan_measured", "jet_web_virtual_plan_measured", &[false, false, false, false, false, false, true], ),
    sema_web_call( "core.web.virtual", "plan_from_sizes", "jet_web_virtual_plan_from_sizes", &[false, false, false, false, false, false, true], ),
    sema_web_call( "core.web.virtual", "plan_slice", "jet_web_virtual_plan_slice", &[true, true], ),
    sema_web_call( "core.web.virtual", "plan_indices", "jet_web_virtual_plan_indices", &[true], ) .with_native_int_result(),
    sema_web_call( "core.web.virtual", "plan_viewport", "jet_web_virtual_plan_viewport", &[false], ),
    sema_web_call("core.web.store", "new", "jet_web_store", &[false, false]),
    sema_web_call("core.web.store", "value", "jet_web_store_value", &[true]),
    sema_web_call( "core.web.store", "state_signal", "jet_web_store_state_signal", &[true], ),
    sema_web_call( "core.web.store", "signal", "jet_web_store_signal", &[true], ),
    sema_web_call( "core.web.store", "with_history", "jet_web_store_with_history", &[false, false, false], ),
    sema_web_call( "core.web.store", "transaction", "jet_web_store_transaction", &[true, false, false, false], ),
    sema_web_call( "core.web.store", "update", "jet_web_store_update", &[true, false, false, false], ),
    sema_web_call( "core.web.store", "batch", "jet_web_store_batch", &[true, false, false, false], ),
    sema_web_call("core.web.store", "set", "jet_web_store_set", &[true, false]),
    sema_web_call( "core.web.store", "set_state", "jet_web_store_set_state", &[true, false], ),
    sema_web_call( "core.web.store", "optimistic", "jet_web_store_optimistic", &[true, false, false, false], ),
    sema_web_call( "core.web.store", "patch", "jet_web_store_patch", &[true, false, false, false], ),
    sema_web_call( "core.web.store", "patch_generation", "jet_web_store_patch_generation", &[true], ) .with_native_int_result(),
    sema_web_call( "core.web.store", "patch_transaction", "jet_web_store_patch_transaction", &[true], ),
    sema_web_call( "core.web.store", "patch_active", "jet_web_store_patch_active", &[true], ),
    sema_web_call( "core.web.store", "patch_commit", "jet_web_store_patch_commit", &[false], ),
    sema_web_call( "core.web.store", "patch_rollback", "jet_web_store_patch_rollback", &[false], ),
    sema_web_call("core.web.store", "back", "jet_web_store_back", &[true]),
    sema_web_call("core.web.store", "forward", "jet_web_store_forward", &[true]),
    sema_web_call( "core.web.store", "jump", "jet_web_store_jump", &[true, false], ),
    sema_web_call( "core.web.store", "scrub", "jet_web_store_scrub", &[true, false], ),
    sema_web_call( "core.web.store", "restore", "jet_web_store_restore", &[true, false], ),
    sema_web_call("core.web.store", "history", "jet_web_store_history", &[true]),
    sema_web_call( "core.web.store", "history_at", "jet_web_store_history_at", &[true, false], ),
    sema_web_call("core.web.store", "events", "jet_web_store_events", &[true]),
    sema_web_call( "core.web.store", "events_since", "jet_web_store_events_since", &[true, false], ),
    sema_web_call( "core.web.store", "clear_history", "jet_web_store_clear_history", &[true], ),
    sema_web_call( "core.web.store", "history_enabled", "jet_web_store_history_enabled", &[true], ),
    sema_web_call( "core.web.store", "history_limit", "jet_web_store_history_limit", &[true], ) .with_native_int_result(),
    sema_web_call( "core.web.store", "set_history_limit", "jet_web_store_set_history_limit", &[true, false], ),
    sema_web_call("core.web.store", "cursor", "jet_web_store_cursor", &[true]) .with_native_int_result(),
    sema_web_call( "core.web.store", "current_generation", "jet_web_store_current_generation", &[true], ) .with_native_int_result(),
    sema_web_call( "core.web.store", "subscribe", "jet_web_store_subscribe", &[true, false], ),
    sema_web_call( "core.web.store", "subscribe_selector", "jet_web_store_subscribe_selector", &[true, false, false], ),
    sema_web_call( "core.web.store", "subscription_unsubscribe", "jet_web_store_subscription_unsubscribe", &[true], ),
    sema_web_call( "core.web.store", "subscription_active", "jet_web_store_subscription_active", &[true], ),
    sema_web_call( "core.web.store", "derived", "jet_web_store_derived", &[true, false], ),
    sema_web_call( "core.web.store", "selector", "jet_web_store_selector", &[true, false], ),
    sema_web_call( "core.web.store", "inspect", "jet_web_store_inspect", &[true], ),
    sema_web_call( "core.web.store", "facts_json", "jet_web_store_facts_json", &[true], ),
    sema_web_call( "core.web.store", "event_json", "jet_web_store_event_json", &[true], ),
    sema_web_call("core.web", "app", "jet_app", &[]),
    sema_web_call("core.web", "page", "jet_web_page", &[false, false]),
    CoreCallRecord::new("app", "live_get", "jet_app_live_get", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.web", "live_get", "jet_app_live_get", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("app", "live_show", "jet_app_live_show", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.web", "live_show", "jet_app_live_show", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("app", "live_stats", "jet_app_live_stats", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.web", "live_stats", "jet_app_live_stats", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("app", "live", "jet_app_live", true, &[false, false]),
    CoreCallRecord::new("core.web", "live", "jet_app_live", true, &[false, false]),
    CoreCallRecord::new("app", "subscribe", "jet_app_subscribe", true, &[false]),
    CoreCallRecord::new("core.web", "subscribe", "jet_app_subscribe", true, &[false]),
    CoreCallRecord::new("app", "invalidate", "jet_app_invalidate", true, &[false]) .with_native_int_result(),
    CoreCallRecord::new("core.web", "invalidate", "jet_app_invalidate", true, &[false]) .with_native_int_result(),
    CoreCallRecord::new("app", "transact_invalidate", "jet_app_transact_invalidate", true, &[false]) .with_native_int_result(),
    CoreCallRecord::new("core.web", "transact_invalidate", "jet_app_transact_invalidate", true, &[false]) .with_native_int_result(),
    CoreCallRecord::new("app", "signal_push", "jet_app_signal_push", true, &[true, false]),
    CoreCallRecord::new("core.web", "signal_push", "jet_app_signal_push", true, &[true, false]),
    CoreCallRecord::new("app", "sync", "jet_app_sync", true, &[false, false]),
    CoreCallRecord::new("core.web", "sync", "jet_app_sync", true, &[false, false]),
    CoreCallRecord::new("app", "auth_routes", "jet_app_auth_routes", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.web", "auth_routes", "jet_app_auth_routes", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("app", "auth_show", "jet_app_auth_show", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.web", "auth_show", "jet_app_auth_show", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.web.devserver", "for_app", "jet_devserver_for_app", true, &[true], ),
    CoreCallRecord::new("core.web.devserver", "app", "jet_devserver_app", true, &[]),
    CoreCallRecord::new( "core.data.sketch.hll", "new", "JetHyperLogLog::new", false, &[], ) .with_pure_route(CoreCallPureRoute::SketchHll),
    CoreCallRecord::new( "core.data.sketch.tdigest", "new", "JetTDigest::new", false, &[], ) .with_pure_route(CoreCallPureRoute::SketchTDigest),
    CoreCallRecord::new( "core.data.sketch.cms", "new", "JetCountMinSketch::new", false, &[], ) .with_pure_route(CoreCallPureRoute::SketchCms),
    CoreCallRecord::new( "core.data.sketch.reservoir", "new", "JetReservoirSampler::new", false, &[false], ) .with_pure_route(CoreCallPureRoute::SketchReservoir),
    CoreCallRecord::new( "core.web.browser", "profile", "jet_browser_profile", false, &[true], ),
    CoreCallRecord::new( "core.web.browser", "timeout", "jet_browser_timeout", false, &[false], ),
    CoreCallRecord::new( "core.web.browser", "locked", "jet_browser_locked", false, &[true], ),
    CoreCallRecord::new( "core.web.browser", "connect", "jet_browser_connect", false, &[true], ),
    CoreCallRecord::new( "core.web.browser", "connect_profile", "jet_browser_connect_profile", false, &[true, true, false], ),
    CoreCallRecord::new( "core.http.server", "serve", "jet_http_mux_serve", false, &[true, false, false, false], ) .with_max_arity(4) .without_direct_aot() .without_direct_jit(),
    CoreCallRecord::new( "core.http.server", "cors_policy", "jet_http_cors_policy", false, &[true, true, true, true, true, true, true], ) .with_max_arity(7) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_jit_symbol("jet_jit_http_cors_policy") .without_direct_aot(),
    CoreCallRecord::new( "core.http.server", "serve_once", "jet_http_mux_serve_once", false, &[true, false], ) .without_direct_aot() .without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.http.server", "serve_once_listener", "jet_http_mux_serve_once_listener", false, &[true, true], ) .without_direct_aot() .without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.http.server", "static_file", "jet_http_srv_static_file", false, &[true, true], ) .without_direct_aot() .without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.http.server", "static_file_range", "jet_http_srv_static_file_range", false, &[true, true, true], ) .without_direct_aot() .without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.http.server", "static_files", "jet_http_srv_static_files_mount_defaulted", false, &[true, true, true], ) .with_max_arity(6) .without_direct_aot() .with_jit_symbol("jet_jit_http_static_files"),
    CoreCallRecord::new("core.http.server", "mux", "jet_http_mux_new", false, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.http.server", "response", "jet_http_srv_response", false, &[false, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.http.server", "tls", "jet_http_srv_tls", false, &[true, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.http.server", "sse", "jet_http_srv_sse", false, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.http.server", "json", "jet_http_srv_json", false, &[false, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.http.server", "cors", "jet_http_srv_install_cors", false, &[true, true], ) .with_jit_symbol("jet_jit_http_cors").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.http.server", "access_log", "jet_http_srv_access_log", false, &[true, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.http.server", "request_id", "jet_http_srv_install_request_id", false, &[true], ).with_jit_symbol("jet_jit_http_request_id").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::receiver_with_symbol(&["HTTPMux"], "middleware", "jet_http_mux_middleware", false, &[true, false]).with_jit_symbol("jet_jit_http_mux_middleware"),
    CoreCallRecord::new("core.net.ws", "connect", "jet_ws_connect", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net.ws", "upgrade", "jet_ws_upgrade", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.time", "new", "JetDate::new", false, &[false, false, false], ) .with_pure_route(CoreCallPureRoute::Date) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot(),
    CoreCallRecord::new( "core.time", "from_timestamp", "JetDateTime::from_timestamp", false, &[false], ) .with_pure_route(CoreCallPureRoute::DateTime) .without_direct_aot(),
    CoreCallRecord::new( "core.time", "parse_time", "JetLocalTime::parse", false, &[true], ) .with_pure_route(CoreCallPureRoute::Time) .without_direct_aot() .with_jit_symbol("jet_jit_time_parse_time"),
    CoreCallRecord::new("core.time", "parse", "JetDate::parse", false, &[true]) .with_pure_route(CoreCallPureRoute::Date) .without_direct_aot() .without_direct_jit(),
    CoreCallRecord::new("core.ui", "box", "jet_ui_box", true, &[true]) .with_pure_route(CoreCallPureRoute::Ui) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .with_jit_symbol("jet_jit_ui_box"),
    CoreCallRecord::new( "core.encoding.xml", "decode", "jet_enc_xml_decode", true, &[true], ) .with_pure_route(CoreCallPureRoute::EncodingXml) .with_max_arity(2) .without_direct_aot() .without_direct_jit(),
    CoreCallRecord::receiver_with_coverage( &["WebTable"], "with_column", &[false], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA), ),
    CoreCallRecord::receiver_with_coverage( &["WebTable"], "paginate", &[true, true], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA), ),
    CoreCallRecord::receiver_with_coverage( &["WebTable"], "page", &[], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA), ),
    CoreCallRecord::receiver_with_coverage( &["WebStore"], "set", &[false], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA), ),
    CoreCallRecord::receiver_with_coverage( &["WebStore"], "facts_json", &[], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA), ),
    CoreCallRecord::receiver_with_coverage( &["WebVirtualWindow"], "facts_json", &[], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA), ),
    CoreCallRecord::receiver_with_symbol( &[INTERNAL_RECEIPT_HANDLE], METHOD_RECEIPT_ATTACH, "jet_receipt_attach", true, &[true, false, false, false], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "line", "jet_data_plot_line", true, &[true], ) .with_jit_symbol("jet_jit_data_plot_line"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "bar", "jet_data_plot_bar", true, &[true], ) .with_jit_symbol("jet_jit_data_plot_bar"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "point", "jet_data_plot_point", true, &[true], ) .with_jit_symbol("jet_jit_data_plot_point"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "x", "jet_data_plot_x", true, &[true, false], ) .with_jit_symbol("jet_jit_data_plot_x"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "y", "jet_data_plot_y", true, &[true, false, false], ) .with_jit_symbol("jet_jit_data_plot_y"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "color", "jet_data_plot_color", true, &[true, false], ) .with_jit_symbol("jet_jit_data_plot_color"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "size", "jet_data_plot_size", true, &[true, false], ) .with_jit_symbol("jet_jit_data_plot_size"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "text_channel", "jet_data_plot_text_channel", true, &[true, false], ) .with_jit_symbol("jet_jit_data_plot_text_channel"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "detail", "jet_data_plot_detail", true, &[true, false], ) .with_jit_symbol("jet_jit_data_plot_detail"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "with_transform", "jet_data_plot_with_transform", true, &[true, false], ) .with_jit_symbol("jet_jit_data_plot_with_transform"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "with_scale", "jet_data_plot_with_scale", true, &[true, false], ) .with_jit_symbol("jet_jit_data_plot_with_scale"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "with_axis", "jet_data_plot_with_axis", true, &[true, false], ) .with_jit_symbol("jet_jit_data_plot_with_axis"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "with_legend", "jet_data_plot_with_legend", true, &[true, false], ) .with_jit_symbol("jet_jit_data_plot_with_legend"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "facet", "jet_data_plot_facet", true, &[true, false], ) .with_jit_symbol("jet_jit_data_plot_facet"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "with_layer", "jet_data_plot_with_layer", true, &[true, false], ) .with_jit_symbol("jet_jit_data_plot_with_layer"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "with_interaction", "jet_data_plot_with_interaction", true, &[true, false], ) .with_jit_symbol("jet_jit_data_plot_with_interaction"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "accessibility", "jet_data_plot_accessibility", true, &[true, false], ) .with_jit_symbol("jet_jit_data_plot_accessibility"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "layout", "jet_data_plot_layout", true, &[true, false], ) .with_jit_symbol("jet_jit_data_plot_layout"),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "select_indices", "jet_data_plot_select_indices", true, &[true, false], ) .with_jit_symbol("jet_jit_data_plot_select_indices"),
    CoreCallRecord::receiver_with_symbol( &[crate::Syntax::INTERNAL_LIST_QUERY_HANDLE], "query", "jet_data_query_sql", true, &[true, true], ) .with_jit_symbol("jet_jit_data_query_sql"),
    CoreCallRecord::receiver_with_symbol( &["Query"], "filter", "jet_data_query_filter", true, &[true, false], ) .with_jit_symbol("jet_jit_data_query_filter"),
    CoreCallRecord::receiver_with_symbol( &["Query"], "sort_by", "jet_data_query_sort_by", true, &[true, false], ) .with_jit_symbol("jet_jit_data_query_sort_by"),
    CoreCallRecord::receiver_with_symbol( &["Query"], "map", "jet_data_query_map", true, &[true, false], ) .without_direct_jit(),
    CoreCallRecord::receiver_with_symbol( &["Query"], "min", "jet_data_query_min", true, &[true, false], ) .without_direct_jit(),
    CoreCallRecord::receiver_with_symbol( &["Query"], "max", "jet_data_query_max", true, &[true, false], ) .without_direct_jit(),
    CoreCallRecord::receiver_with_symbol( &["Query"], "inner_join", "jet_data_query_inner_join", true, &[true, true, false, false], ) .without_direct_jit(),
    CoreCallRecord::receiver_with_symbol( &["Query"], "left_join", "jet_data_query_left_join", true, &[true, true, false, false], ) .without_direct_jit(),
    CoreCallRecord::receiver_with_symbol( &["Query"], "collect", "jet_data_query_collect", true, &[true], ) .with_jit_symbol("jet_jit_data_query_collect"),
    CoreCallRecord::receiver_with_symbol( &["Query"], "plan", "jet_data_query_plan", true, &[true], ) .with_jit_symbol("jet_jit_data_query_plan"),
    CoreCallRecord::receiver_with_symbol( &["Query"], "group_by", "jet_data_query_group_by", true, &[true, false], ) .with_jit_symbol("jet_jit_data_query_group_by"),
    CoreCallRecord::receiver_with_symbol( &["DataTracked"], "query", "jet_data_query_tracked", true, &[true], ) .without_direct_jit(),
    CoreCallRecord::receiver_with_symbol( &["DataTracked"], "insert", "jet_data_track_insert", true, &[true, false], ) .without_direct_jit(),
    CoreCallRecord::receiver_with_symbol( &["DataTracked"], "replace", "jet_data_track_replace", true, &[true, false, false], ) .without_direct_jit(),
    CoreCallRecord::receiver_with_symbol( &["DataTracked"], "remove", "jet_data_track_remove", true, &[true, false], ) .without_direct_jit(),
    CoreCallRecord::receiver_with_symbol( &["Query"], "watch", "jet_data_query_watch", true, &[true], ) .without_direct_jit(),
    CoreCallRecord::receiver_with_symbol( &["DataWatch"], "get", "jet_data_watch_get", true, &[true], ) .without_direct_jit(),
    CoreCallRecord::receiver_with_symbol( &["DataWatch"], "status", "jet_data_watch_status", true, &[true], ) .without_direct_jit(),
    CoreCallRecord::receiver_with_symbol( &["DataWatch"], "cancel", "jet_data_watch_cancel", true, &[true], ) .without_direct_jit(),
    CoreCallRecord::receiver_with_symbol( &["DataGroupedQuery"], "count", "jet_data_group_count_query", true, &[true], ) .with_jit_symbol("jet_jit_data_group_count_query"),
    CoreCallRecord::receiver_with_symbol( &["DataGroupedQuery"], "sum", "jet_data_group_sum_query", true, &[true, false], ) .with_jit_symbol("jet_jit_data_group_sum_query"),
    CoreCallRecord::receiver_with_symbol( &["DataGroupedQuery"], "mean", "jet_data_group_mean_query", true, &[true, false], ) .with_jit_symbol("jet_jit_data_group_mean_query"),
    CoreCallRecord::receiver(&["ProcessStdin"], "close", &[]),
    CoreCallRecord::receiver_with_symbol( &["FileScope"], "read", "jet_std_fs_scope_read", true, &[true, true], ),
    CoreCallRecord::receiver(&["RealtimeStream"], "next_deadline", &[]),
    CoreCallRecord::receiver(&["RealtimeStream"], "receipt", &[]),
    CoreCallRecord::receiver(&["RealtimeStream"], "cancel", &[]),
    CoreCallRecord::receiver(&["RealtimeStream"], "is_cancelled", &[]),
    CoreCallRecord::receiver(&["Condition"], "notify_one", &[]),
    CoreCallRecord::receiver(&["Condition"], "notify_all", &[]),
    CoreCallRecord::receiver(&["URL"], "scheme", &[]),
    CoreCallRecord::receiver(&["URL"], "username", &[]),
    CoreCallRecord::receiver(&["URL"], "password", &[]),
    CoreCallRecord::receiver(&["URL"], "userinfo", &[]),
    CoreCallRecord::receiver(&["URL"], "authority", &[]),
    CoreCallRecord::receiver(&["URL"], "path", &[]),
    CoreCallRecord::receiver(&["URL"], "query", &[]),
    CoreCallRecord::receiver(&["URL"], "host", &[]),
    CoreCallRecord::receiver(&["URL"], "port", &[]),
    CoreCallRecord::receiver(&["URL"], "default_port", &[]),
    CoreCallRecord::receiver(&["URL"], "fragment", &[]),
    CoreCallRecord::receiver(&["URL"], "path_segments", &[]),
    CoreCallRecord::receiver(&["URL"], "query_pairs", &[]),
    CoreCallRecord::receiver(&["URL"], "normalize", &[]),
    CoreCallRecord::receiver(&["URL"], "join", &[false]),
    CoreCallRecord::receiver(&["URL"], "set_query", &[false, false]),
    CoreCallRecord::receiver(&["URL"], "add_query", &[false, false]),
    CoreCallRecord::receiver(&["URL"], "to_string", &[]),
    CoreCallRecord::receiver(&["MIME"], "media_type", &[]),
    CoreCallRecord::receiver(&["MIME"], "subtype", &[]),
    CoreCallRecord::receiver(&["MIME"], "essence", &[]),
    CoreCallRecord::receiver(&["MIME"], "to_string", &[]),
    CoreCallRecord::receiver(&["MIME"], "param", &[false]),
    CoreCallRecord::receiver(&["MIME"], "params", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "year", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "month", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "day", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "to_string", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "equal", &[false]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "compare", &[false]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "weekday", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "iso_weekday", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "day_of_year", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "iso_week", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "iso_week_year", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "quarter_of_year", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "days_in_month", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "is_leap_year", &[]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "replace", &[false, false, false]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "add_days", &[false]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "add_months", &[false]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "diff_days", &[false]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "add_period", &[false]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "subtract_period", &[false]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "truncate", &[true]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "format", &[true]),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "format_checked", &[true]),
    CoreCallRecord::receiver( &["Date", "LocalDate"], "until", &[false, true, true, true, false], ),
    CoreCallRecord::receiver( &["Date", "LocalDate"], "since", &[false, true, true, true, false], ),
    CoreCallRecord::receiver(&["Date", "LocalDate"], "with", &[false, false, false, true]),
    CoreCallRecord::receiver(&["LocalTime"], "hour", &[]),
    CoreCallRecord::receiver(&["LocalTime"], "minute", &[]),
    CoreCallRecord::receiver(&["LocalTime"], "second", &[]),
    CoreCallRecord::receiver(&["LocalTime"], "millisecond", &[]),
    CoreCallRecord::receiver(&["LocalTime"], "microsecond", &[]),
    CoreCallRecord::receiver(&["LocalTime"], "nanosecond", &[]),
    CoreCallRecord::receiver(&["LocalTime"], "add_duration", &[false]),
    CoreCallRecord::receiver(&["LocalTime"], "subtract_duration", &[false]),
    CoreCallRecord::receiver(&["LocalTime"], "round", &[true, false, true]),
    CoreCallRecord::receiver(&["LocalTime"], "truncate", &[true, false]),
    CoreCallRecord::receiver(&["LocalTime"], "floor", &[true, false]),
    CoreCallRecord::receiver(&["LocalTime"], "ceil", &[true, false]),
    CoreCallRecord::receiver(&["LocalTime"], "until", &[false, true, true, true, false]),
    CoreCallRecord::receiver(&["LocalTime"], "since", &[false, true, true, true, false]),
    CoreCallRecord::receiver(&["LocalTime"], "format", &[true]),
    CoreCallRecord::receiver(&["LocalTime"], "format_checked", &[true]),
    CoreCallRecord::receiver(&["LocalTime"], "to_string", &[]),
    CoreCallRecord::receiver(&["LocalTime"], "equal", &[false]),
    CoreCallRecord::receiver(&["LocalTime"], "compare", &[false]),
    CoreCallRecord::receiver(&["DateTime"], "to_timestamp", &[]),
    CoreCallRecord::receiver(&["DateTime"], "to_unix_ms", &[]),
    CoreCallRecord::receiver(&["DateTime"], "to_unix_s", &[]),
    CoreCallRecord::receiver(&["DateTime"], "to_unix_us", &[]),
    CoreCallRecord::receiver(&["DateTime"], "to_unix_ns", &[]),
    CoreCallRecord::receiver(&["DateTime"], "to_string", &[]),
    CoreCallRecord::receiver(&["DateTime"], "date", &[]),
    CoreCallRecord::receiver(&["DateTime"], "time", &[]),
    CoreCallRecord::receiver(&["DateTime"], "hour", &[]),
    CoreCallRecord::receiver(&["DateTime"], "minute", &[]),
    CoreCallRecord::receiver(&["DateTime"], "second", &[]),
    CoreCallRecord::receiver(&["DateTime"], "millisecond", &[]),
    CoreCallRecord::receiver(&["DateTime"], "microsecond", &[]),
    CoreCallRecord::receiver(&["DateTime"], "nanosecond", &[]),
    CoreCallRecord::receiver(&["DateTime"], "format_rfc3339", &[]),
    CoreCallRecord::receiver(&["DateTime"], "format", &[true]),
    CoreCallRecord::receiver(&["DateTime"], "format_checked", &[true]),
    CoreCallRecord::receiver(&["DateTime"], "plus_duration", &[false]),
    CoreCallRecord::receiver(&["DateTime"], "subtract_duration", &[false]),
    CoreCallRecord::receiver(&["DateTime"], "add_nanoseconds", &[false]),
    CoreCallRecord::receiver(&["DateTime"], "add_period", &[false]),
    CoreCallRecord::receiver(&["DateTime"], "subtract_period", &[false]),
    CoreCallRecord::receiver(&["DateTime"], "difference", &[false]),
    CoreCallRecord::receiver(&["DateTime"], "truncate", &[true, false]),
    CoreCallRecord::receiver(&["DateTime"], "round", &[true, false, true]),
    CoreCallRecord::receiver(&["DateTime"], "floor", &[true, false]),
    CoreCallRecord::receiver(&["DateTime"], "ceil", &[true, false]),
    CoreCallRecord::receiver(&["DateTime"], "equal", &[false]),
    CoreCallRecord::receiver(&["DateTime"], "compare", &[false]),
    CoreCallRecord::receiver(&["DateTime"], "until", &[false, true, true, true, false]),
    CoreCallRecord::receiver(&["DateTime"], "since", &[false, true, true, true, false]),
    CoreCallRecord::receiver( &["DateTime"], "replace", &[false, false, false, false, false, false], ),
    CoreCallRecord::receiver(&["DateTime"], "in_zone", &[false]),
    CoreCallRecord::receiver( &["DateTime"], "with", &[false, false, false, false, false, false, true], ),
    CoreCallRecord::receiver(&["Instant"], "elapsed_millis", &[]),
    CoreCallRecord::receiver(&["Instant"], "elapsed", &[]),
    CoreCallRecord::receiver(&["Instant"], "equal", &[false]),
    CoreCallRecord::receiver(&["Instant"], "compare", &[false]),
    CoreCallRecord::receiver(&["Zone"], "name", &[]),
    CoreCallRecord::receiver(&["Zone"], "next_transition", &[false]),
    CoreCallRecord::receiver(&["Zone"], "previous_transition", &[false]),
    CoreCallRecord::receiver(&["Zone"], "start_of_day", &[false]),
    CoreCallRecord::receiver(&["Zone"], "hours_in_day", &[false]),
    CoreCallRecord::receiver(&["Fraction"], "to_string", &[]),
    CoreCallRecord::receiver(&["Fraction"], "numerator", &[]),
    CoreCallRecord::receiver(&["Fraction"], "denominator", &[]),
    CoreCallRecord::receiver(&["Fraction"], "to_float", &[]),
    CoreCallRecord::receiver(&["Fraction"], "is_zero", &[]),
    CoreCallRecord::receiver(&["Fraction"], "equal", &[false]),
    CoreCallRecord::receiver(&["Fraction"], "add", &[false]),
    CoreCallRecord::receiver(&["Fraction"], "sub", &[false]),
    CoreCallRecord::receiver(&["Fraction"], "mul", &[false]),
    CoreCallRecord::receiver(&["Fraction"], "div", &[false]),
    CoreCallRecord::receiver(&["Decimal"], "to_string", &[]),
    CoreCallRecord::receiver(&["Decimal"], "add", &[false]),
    CoreCallRecord::receiver(&["Decimal"], "sub", &[false]),
    CoreCallRecord::receiver(&["Decimal"], "mul", &[false]),
    CoreCallRecord::receiver(&["Decimal"], "div", &[false]),
    CoreCallRecord::receiver(&["Decimal"], "round", &[]),
    CoreCallRecord::receiver(&["Decimal"], "floor", &[]),
    CoreCallRecord::receiver(&["Decimal"], "ceil", &[]),
    CoreCallRecord::receiver(&["Decimal"], "equal", &[false]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "date", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "time", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "offset_seconds", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "is_dst", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "to_datetime", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "zone", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "to_string", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "equal", &[false]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "compare", &[false]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "format", &[true]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "add_duration", &[false]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "subtract_duration", &[false]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "add_period", &[false]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "subtract_period", &[false]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "with_time", &[true, true]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "with_zone", &[true]),
    CoreCallRecord::receiver( &["ZonedDateTime"], "until", &[false, true, true, true, false], ),
    CoreCallRecord::receiver( &["ZonedDateTime"], "since", &[false, true, true, true, false], ),
    CoreCallRecord::receiver(&["ZonedDateTime"], "next_transition", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "previous_transition", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "start_of_day", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "hours_in_day", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "format_rfc9557", &[]),
    CoreCallRecord::receiver(&["ZonedDateTime"], "format_checked", &[true]),
    CoreCallRecord::receiver(&["Period"], "to_string", &[]),
    CoreCallRecord::receiver(&["Period"], "years", &[]),
    CoreCallRecord::receiver(&["Period"], "months", &[]),
    CoreCallRecord::receiver(&["Period"], "days", &[]),
    CoreCallRecord::receiver(&["Period"], "sign", &[]),
    CoreCallRecord::receiver(&["Period"], "is_zero", &[]),
    CoreCallRecord::receiver(&["Period"], "abs", &[]),
    CoreCallRecord::receiver(&["Period"], "negated", &[]),
    CoreCallRecord::receiver(&["Period"], "add", &[false]),
    CoreCallRecord::receiver(&["Period"], "sub", &[false]),
    CoreCallRecord::receiver(&["Period"], "total_in", &[true, true]),
    CoreCallRecord::receiver(&["Measurement"], "value", &[]),
    CoreCallRecord::receiver(&["Measurement"], "uncertainty", &[]),
    CoreCallRecord::receiver(&["Measurement"], "add", &[false]),
    CoreCallRecord::receiver(&["Measurement"], "sub", &[false]),
    CoreCallRecord::receiver(&["Measurement"], "mul", &[false]),
    CoreCallRecord::receiver(&["Measurement"], "div", &[false]),
    CoreCallRecord::receiver(&["Measurement"], "sqrt", &[]),
    CoreCallRecord::receiver(&["HyperLogLog"], "new", &[]),
    CoreCallRecord::receiver(&["HyperLogLog"], "add", &[true]),
    CoreCallRecord::receiver(&["HyperLogLog"], "count", &[]),
    CoreCallRecord::receiver(&["TDigest"], "new", &[]),
    CoreCallRecord::receiver(&["TDigest"], "add", &[false]),
    CoreCallRecord::receiver(&["TDigest"], "quantile", &[false]),
    CoreCallRecord::receiver(&["CountMinSketch"], "new", &[]),
    CoreCallRecord::receiver(&["CountMinSketch"], "add", &[true]),
    CoreCallRecord::receiver(&["CountMinSketch"], "count", &[true]),
    CoreCallRecord::receiver(&["ReservoirSampler"], "new", &[false]),
    CoreCallRecord::receiver(&["ReservoirSampler"], "add", &[true]),
    CoreCallRecord::receiver(&["ReservoirSampler"], "sample", &[]),
    CoreCallRecord::receiver(&["Solver"], "new", &[false]),
    CoreCallRecord::receiver(&["Solver"], "require", &[false]),
    CoreCallRecord::receiver(&["Solver"], "failure_count", &[]),
    CoreCallRecord::receiver(&["Solver"], "status", &[]),
    CoreCallRecord::receiver( &[ "HyperLogLog", "TDigest", "CountMinSketch", "ReservoirSampler", "Solver", "ServiceUpgradeReceipt", "DataError", VAULT_KEY_REF_TYPE, ], "__display", &[], ),
    CoreCallRecord::new( "core.http.server", "bind", "jet_http_server_bind", false, &[true, false, false, false], ) .with_max_arity(4) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .without_direct_jit(),
    CoreCallRecord::new( "core.data", "inner_join", "jet_data_inner_join_checked_default", true, &[true, true, false, false], ) .without_direct_aot() .with_jit_symbol("jet_jit_data_inner_join").with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.data.arrow", "import", "jet_data_arrow_import", true, &[false]).with_jit_symbol("jet_jit_data_arrow_import"),
    CoreCallRecord::new("core.data.arrow", "query", "jet_data_query_arrow", true, &[false]).with_jit_symbol("jet_jit_data_query_arrow"),
    CoreCallRecord::new("core.auth", "verify_jwt", "jet_auth_verify_jwt_defaulted", true, &[true, true, true, true, true]).with_max_arity(5).with_jit_symbol("jet_jit_auth_verify_jwt"),
    CoreCallRecord::new("core.data", "mean", "jet_data_mean_checked", true, &[true]).without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.encoding.toml", "to_string", "jet_enc_toml_to_string", true, &[true]).with_jit_symbol("jet_jit_toml_to_string"),
    CoreCallRecord::new("core.encoding.yaml", "to_string", "jet_enc_yaml_to_string", true, &[true]).with_jit_symbol("jet_jit_yaml_to_string"),
    CoreCallRecord::new("core.encoding.xml", "to_bytes", "jet_std_xml_to_bytes", true, &[true, true]).with_jit_symbol("jet_jit_xml_to_bytes"),
    CoreCallRecord::new("core.service", "workflow_start", "jet_services_workflow_start", true, &[true, false, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sync", "text_edit", "jet_sync_text_edit", true, &[true, false, false, false, false]).without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "workflow_outcome", "jet_services_workflow_outcome", true, &[true, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sync", "list_push", "jet_sync_list_push", true, &[false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "workflow_history", "jet_services_workflow_history", true, &[true, false]).without_direct_aot().without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sync", "policy_new", "jet_db_policy_new", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_jit_symbol("jet_jit_db_policy"),
    sema_web_call("core.web", "on", "jet_web_on", &[true, true, false]),
];
// END GENERATED CORE CALLS


/// Build gate for the one Core-call registry. A row may use a typed adapter,
/// but it cannot claim an interpreter tier without an executable route.
const fn assert_core_call_coverage() {
    let mut index = 0;
    while index < CORE_CALLS.len() {
        assert!(
            CORE_CALLS[index].coverage.bits() & !CoreCallCoverage::KNOWN == 0,
            "Core-call registry row declares unknown coverage bits"
        );
        if CORE_CALLS[index]
            .coverage
            .contains(CoreCallCoverage::INTERPRETER)
        {
            assert!(
                CORE_CALLS[index].interpreter_route.is_executable(),
                "Core-call registry row declares interpreter coverage without an executable route"
            );
        }
        if CORE_CALLS[index].interpreter_route.is_executable() {
            assert!(
                CORE_CALLS[index]
                    .coverage
                    .contains(CoreCallCoverage::INTERPRETER),
                "Core-call registry row has an executable route without interpreter coverage"
            );
        }
        index += 1;
    }
}

const _: () = assert_core_call_coverage();

/// The one canonical lookup used by all plain Core-call projections.
pub fn core_call(module: &str, member: &str) -> Option<&'static CoreCallRecord> {
    CORE_CALLS
        .iter()
        .find(|row| row.receiver_types.is_empty() && row.module == module && row.member == member)
}

/// Resolve the unique Core module behind a bare module alias.
///
/// Prefer the projection rows so a bare alias follows the same registry as
/// the compiler consumers. Fall back to the module registry for modules that
/// have no plain-call row yet.
pub fn core_module_for_alias(alias: &str) -> Option<&'static str> {
    let mut found = None;
    for module in CORE_CALLS
        .iter()
        .filter(|row| row.receiver_types.is_empty())
        .map(|row| row.module)
        .filter(|module| module.rsplit('.').next() == Some(alias))
    {
        if found.is_some_and(|known| known != module) {
            return None;
        }
        found = Some(module);
    }
    if found.is_some() {
        return found;
    }

    let mut found = None;
    for module in KNOWN_CORE_MODULES
        .iter()
        .copied()
        .filter(|module| module.rsplit('.').next() == Some(alias))
    {
        if found.is_some_and(|known| known != module) {
            return None;
        }
        found = Some(module);
    }
    found
}

/// Resolve a Core call's canonical submodule.
///
/// Most callers already name the concrete module. The root encoding namespace
/// keeps the historical JSON shorthand, so that alias is resolved here with
/// the rest of the shared Core-call lookup rather than in an editor table.
pub fn core_module_for_call(module: &str, member: &str) -> Option<&'static str> {
    if let Some(row) = core_call(module, member) {
        return Some(row.module);
    }
    if module == "core.encoding"
        && one_of(
            member,
            &[
                "parse",
                "decode",
                "to_string",
                "to_string_pretty",
                "canonical",
                "events",
            ],
        )
    {
        return Some("core.encoding.json");
    }
    None
}

/// Find marker metadata on an ordinary Core declaration row.
pub fn core_marker_application(module: &str, member: &str) -> Option<CoreMarkerApplication> {
    CORE_CALLS.iter().find_map(|row| {
        (row.module == module
            && row.receiver_types.is_empty()
            && row.marker.is_some_and(|marker| marker.member == member))
        .then(|| row.marker.expect("marker checked above"))
    })
}

/// Find one receiver/static-method projection from the same Core registry.
pub fn core_receiver_method(receiver_type: &str, member: &str) -> Option<&'static CoreCallRecord> {
    CORE_CALLS.iter().find(|row| {
        row.member == member && row.receiver_types.iter().any(|name| *name == receiver_type)
    })
}

/// A generic table lookup used by tests and future projections. A new record
/// is therefore data-only: no consumer match arm is needed to make it visible.
pub fn core_call_in<'a>(
    rows: &'a [CoreCallRecord],
    module: &str,
    member: &str,
) -> Option<&'a CoreCallRecord> {
    rows.iter()
        .find(|row| row.receiver_types.is_empty() && row.module == module && row.member == member)
}

/// Project one plain row for one engine and validate its fixed erased arity.
/// Engines convert these errors into their own diagnostics or internal
/// adapter errors; no engine gets a private membership check.
pub fn core_call_projection(
    module: &str,
    member: &str,
    projection: u8,
    actual_arity: usize,
) -> Result<&'static CoreCallRecord, CoreCallProjectionError> {
    core_call_projection_in(CORE_CALLS, module, member, projection, actual_arity)
}

/// Project a row from any table-shaped slice. The production helper above is
/// the fixed canonical table; this generic form lets tests prove that a new
/// row needs no consumer match arm.
pub fn core_call_projection_in<'a>(
    rows: &'a [CoreCallRecord],
    module: &str,
    member: &str,
    projection: u8,
    actual_arity: usize,
) -> Result<&'a CoreCallRecord, CoreCallProjectionError> {
    let row = core_call_in(rows, module, member).ok_or(CoreCallProjectionError::Unknown)?;
    if !row.coverage.contains(projection) {
        return Err(CoreCallProjectionError::Uncovered { projection });
    }
    // D-AUTHORITY-WORD2=E: process.run may carry one extra ordinary Authority
    // value. Plugin loading has an exact two-argument signature above.
    let authority_boundary_form =
        actual_arity == row.signature.arity + 1
            && module == "core.process"
            && member == "run";
    if !row.accepts_arity(actual_arity) && !authority_boundary_form {
        return Err(CoreCallProjectionError::Arity {
            expected: row.signature.arity,
            actual: actual_arity,
        });
    }
    Ok(row)
}

/// Validate the table's structural invariants and report every violation.
pub fn core_call_table_violations(rows: &[CoreCallRecord]) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if (!row.is_receiver() && row.module.is_empty()) || row.member.is_empty() {
            violations.push(format!("row {index} has an empty Core key"));
        }
        if !row.is_receiver() && row.prelude_symbol().is_empty() {
            violations.push(format!("{}.{} has no symbol", row.module, row.member));
        }
        if row.signature.arity != row.signature.borrow_mask.len()
            || row.signature.arity > row.signature.max_arity
        {
            violations.push(format!(
                "{}.{} signature arity/range disagrees with its borrow mask",
                row.module, row.member
            ));
        }
        for access in row.frame_accesses {
            if access.argument >= row.signature.arity {
                violations.push(format!(
                    "{}.{} frame access argument {} is outside arity {}",
                    row.module, row.member, access.argument, row.signature.arity
                ));
            }
        }
        if !row.frame_accesses.is_empty() && row.frame_completion.is_none() {
            violations.push(format!(
                "{}.{} has frame accesses but no completion contract",
                row.module, row.member
            ));
        }
        if let Some(completion) = row.frame_completion {
            if completion.provider.is_empty() {
                violations.push(format!(
                    "{}.{} has an empty frame completion provider",
                    row.module, row.member
                ));
            }
            if matches!(
                completion.kind,
                CoreCallCompletionKind::Pending | CoreCallCompletionKind::Event
            ) && completion.token.is_none()
            {
                violations.push(format!(
                    "{}.{} has asynchronous frame completion without a token",
                    row.module, row.member
                ));
            }
        }
        if !row.capability.is_well_formed() {
            violations.push(format!(
                "{}.{} has a malformed capability/provider contract",
                row.module, row.member
            ));
        }
        if row.coverage.bits() == 0 {
            violations.push(format!(
                "{}.{} declares no consumer projection",
                row.module, row.member
            ));
        }
        if row.coverage.bits() & !CoreCallCoverage::KNOWN != 0 {
            violations.push(format!(
                "{}.{} declares unknown coverage bits 0x{:02x}",
                row.module,
                row.member,
                row.coverage.bits() & !CoreCallCoverage::KNOWN
            ));
        }
        if row.coverage.contains(CoreCallCoverage::INTERPRETER)
            && !row.interpreter_route.is_executable()
        {
            violations.push(format!(
                "{}.{} declares interpreter coverage without an interpreter route",
                row.module, row.member
            ));
        }
        for other in &rows[index + 1..] {
            let duplicate = if row.is_receiver() || other.is_receiver() {
                row.is_receiver()
                    && other.is_receiver()
                    && row.member == other.member
                    && row.receiver_types == other.receiver_types
            } else {
                row.key() == other.key()
            };
            if duplicate {
                violations.push(format!("{}.{} appears twice", row.module, row.member));
            }
        }
    }
    violations
}

/// Return rows that do not declare one consumer projection.
pub fn core_call_coverage_violations(rows: &[CoreCallRecord], projection: u8) -> Vec<String> {
    rows.iter()
        .flat_map(|row| {
            let missing = (!row.coverage.contains(projection)).then(|| {
                format!(
                    "{}.{} missing projection 0x{:02x}",
                    row.module, row.member, projection
                )
            });
            let missing_route = (projection == CoreCallCoverage::INTERPRETER
                && row.coverage.contains(CoreCallCoverage::INTERPRETER)
                && !row.interpreter_route.is_executable())
            .then(|| {
                format!(
                    "{}.{} declares interpreter coverage without an interpreter route",
                    row.module, row.member
                )
            });
            missing.into_iter().chain(missing_route)
        })
        .collect()
}

/// Compare a consumer's declared plain keys with the canonical table. This
/// guard catches both a forgotten row projection and a consumer-only arm.
pub fn core_call_mismatch(
    rows: &[CoreCallRecord],
    consumer: &str,
    consumer_keys: &[(&str, &str)],
) -> Vec<String> {
    let mut violations = Vec::new();
    for row in rows.iter().filter(|row| !row.is_receiver()) {
        if !consumer_keys
            .iter()
            .any(|(module, member)| *module == row.module && *member == row.member)
        {
            violations.push(format!(
                "{consumer} does not project {}.{}",
                row.module, row.member
            ));
        }
    }
    for (module, member) in consumer_keys {
        if !rows
            .iter()
            .any(|row| !row.is_receiver() && row.module == *module && row.member == *member)
        {
            violations.push(format!("{consumer} projects unknown {}.{}", module, member));
        }
    }
    violations
}
#[cfg(test)]
mod tests {
    use super::{core_call, CoreCapabilityKind};
    use crate::Effects::Effect;

    #[test]
    fn filesystem_reads_and_walks_use_fs_read_leaf() {
        for method in ["read", "read_bytes", "walk", "walk_parallel"] {
            let row = core_call("core.files", method)
                .unwrap_or_else(|| panic!("missing core.files.{method} row"));
            let leaf = row.effect_leaf();
            assert_eq!(leaf, Some("FS.Read"), "core.files.{method}");
            assert_ne!(leaf, Some("Time.Wait"), "core.files.{method}");
        }
    }

    #[test]
    fn http_client_requests_require_network_and_wait() {
        for method in ["get", "post", "request"] {
            let row = core_call("core.http.client", method)
                .unwrap_or_else(|| panic!("missing core.http.client.{method} row"));
            assert_eq!(row.effect(), Some(Effect::Net), "core.http.client.{method}");
            assert_eq!(
                row.effect_leaf(),
                Some("Time.Wait"),
                "core.http.client.{method}"
            );
        }
    }
    #[test]
    fn capability_contract_follows_effect_authority() {
        let pure = core_call("core.math", "pi")
            .expect("core.math.pi capability row");
        assert_eq!(pure.capability().kind, CoreCapabilityKind::Pure);
        assert!(pure.capability().is_well_formed());

        let native = core_call("core.files", "read")
            .expect("core.files.read capability row");
        assert_eq!(
            native.capability().kind,
            CoreCapabilityKind::NativeRequired
        );
        assert_eq!(native.capability().provider, Some("core.files"));
        assert!(native.capability().is_well_formed());
    }

}
