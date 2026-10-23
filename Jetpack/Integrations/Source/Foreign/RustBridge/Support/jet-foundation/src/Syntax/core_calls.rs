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
    if same_text(module, "core.plugin") && same_text(method, "__authority_to_wire") {
        return None;
    }
    if same_text(module, "core.files") {
        if same_text(method, "__temp_path_sample") {
            return Some(Effect::Time);
        }
        if same_text(method, "__temp_directory") {
            return Some(Effect::Env);
        }
        if one_of(method, &["__host_path_is_absolute", "__host_path_join", "__fault_should_fail", "__os_family",
            "__path_windows", "__atomic_pid", "__scope_roots", "__reader_path"]) {
            return None;
        }
    }
    if same_text(module, "core.log") && same_text(method, "__metric_publish") {
        return None;
    }
    if (same_text(module, "core.devtools") && same_text(method, "__publish_checked"))
        || (same_text(module, "core.web") && one_of(method, &["__openapi_inputs", "__openapi_render", "__contract_stop", "__live_stats_snapshot"]))
        || (same_text(module, "core.web.store") && one_of(method, &["__transact", "__facts_snapshot"]))
        || (same_text(module, "core.web.table") && one_of(method, &["__columns", "__replace_columns", "__state", "__set_state", "__alias",
            "__has_loader", "__invoke_loader", "__generation", "__query_facts", "__state_equal", "__page_facts",
            "__commit_keys", "__loading_store", "__error_store", "__page_store",
            "__state_store", "__has_key", "__page_accessor_keys", "__page_keys", "__page_replace_keys",
            "__column_info", "__cell", "__rows", "__row_key", "__make_page"]))
    {
        return None;
    }
    if same_text(module, "core.process") && same_text(method, "__with_directory_raw") {
        return Some(Effect::FS);
    }
    if same_text(module, "core.data") && same_text(method, "__host_path_is_file") {
        return Some(Effect::FS);
    }
    // Civil zone lookup reads environment/files, not the clock. Keep these
    // facts aligned with the Jet registry and their precise FS.Read leaves.
    if same_text(module, "core.time") {
        if same_text(method, "__zone_env") {
            return Some(Effect::Env);
        }
        if one_of(method, &["zone", "parse_zoned", "__zone_file_bytes"]) {
            return Some(Effect::FS);
        }
    }
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
                "datetime",
                "utc",
                "zoned",
                "zoned_local",
                "days_in_month",
                "is_leap_year",
                "__embedded_zone_bytes",
                // D-DET1: the source Clock owns its manual/system state.
                // System construction requires Time; its private raw sample
                // must not grant ambient Time to an injected manual clock.
                "__clock_mono_ns",
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
        return if one_of(method, &["files", "__files"]) {
            Some(Effect::FS)
        } else if one_of(method, &["process_pid", "__process_pid"]) {
            Some(Effect::Exec)
        } else if one_of(method, &["port", "__port"]) {
            Some(Effect::Net)
        } else {
            None
        };
    }
    if same_text(module, "core.web.browser") {
        return if one_of(method, &["profile", "timeout", "__path_join", "__html_path",
            "__report_options", "__report_json", "__report_html", "__report_text", "__write_report_bytes", "__server_stop"]) {
            None
        } else if one_of(method, &["locked", "__read_text", "__is_file", "__file_size", "__modified_ms"]) {
            Some(Effect::FS)
        } else if same_text(method, "config_from_env") {
            Some(Effect::Env)
        } else {
            Some(Effect::Net)
        };
    }
    if same_text(module, "core.encoding") && same_text(method, "__query_read_bounded") {
        return Some(Effect::FS);
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
    if same_text(module, "core.perf")
        && one_of(method, &["__read_bits", "__write_bits", "fidelity", "override_fidelity", "reset_fidelity"])
    {
        return Some(Effect::Env);
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
    if same_text(module, "core.term")
        && one_of(method, &["__stdio_fault", "__stdout_terminal", "__stderr_terminal"])
    {
        return None;
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
                "__store_load",
                "__store_replace",
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
    if same_text(module, "core.process") && same_text(method, "__with_directory_raw") {
        return Some("FS.Read");
    }
    if same_text(module, "core.time")
        && one_of(method, &["zone", "parse_zoned", "__zone_file_bytes"])
    {
        return Some("FS.Read");
    }
    if same_text(module, "core.time") && one_of(method, &["sleep", "sleep_until"]) {
        return Some("Time.Wait");
    }
    if same_text(module, "core.tasks") && one_of(method, &["yield_now", "__yield_now"]) {
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
                "__read_bytes_os",
                "__read_at_os",
                "__read_link_os",
                "__scope_read_at",
                "__reader_line_bytes",
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
                "__open_reader",
                "jet_std_files_open_reader_nonblocking",
                "__open_directory",
                "__close_directory",
                "__list_directory",
                "__stat_at",
                "__metadata_directory",
                "__directory_fd",
                "__path_kind",
                "__read_link_at",
                "__xattr_names",
                "__xattr_get",
                "__reader_stat",
                "__writer_stat",
                "__map_raw",
                "__list_dir_raw",
                "__stat_mode_raw",
                "__canonicalize_raw",
                "__glob_raw",
                "__walk_parallel_raw",
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
                "__write_bytes_os",
                "__append_bytes_os",
                "__create_dirs_os",
                "__write_at_os",
                "__writer_refusal",
                "__atomic_reserve",
                "__atomic_finish",
                "__atomic_remove",
                "__atomic_rename",
                "__atomic_sync_parent",
                "__atomic_close",
                "jet_files_link_reader_at_raw",
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
                "try_lock",
                "__open_writer",
                "__create_directory",
                "__fsync_directory",
                "__rename_at",
                "__remove_at",
                "__xattr_set",
                "__writer_set_mode",
                "__flock",
                "__remove_raw",
                "__remove_dir_raw",
                "__remove_all_raw",
                "__hard_link_raw",
                "__fsync_raw",
                "__chown_raw",
                "__set_mode_raw",
                "__symlink_directory_raw",
                "__symlink_raw",
                "__mkdtemp_raw",
                "__temp_dir_raw",
                "__temp_file_raw",
                "__lock_open", "__lock_try", "__lock_wait", "__lock_close",
                "__lock_fd_identity", "__lock_path_identity", "__lock_owner",
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
                "__terminal8_line", "__terminal8_write_stdout", "__terminal8_write_stderr",
                "__terminal8_mode_enter", "__terminal8_mode_leave", "__terminal8_read_all",
                "__terminal8_read_until", "__terminal8_take", "__terminal8_key_bytes",
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
                "__resolve_ordered",
                "__tcp_accept",
                "__tcp_connect",
                "__tcp_connect_addr",
                "__tcp_connect_timeout",
                "__tcp_connect_happy",
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
                "__tcp_reply",
                "__udp_send_to",
                "__udp_recv_from",
                "__udp_send_bytes_to",
                "__udp_receive",
                "__unix_accept",
                "__unix_read",
                "__unix_write_all_bytes",
                "__unix_connect_host",
                "__unix_connect_deadline_host",
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
                "__dns_a_lookup",
                "__dns_aaaa_lookup",
                "__dns_a_at_lookup",
                "__dns_aaaa_at_lookup",
                "__dns_txt_lookup",
                "__dns_txt_at_lookup",
                "__dns_ptr_lookup",
                "__dns_srv_lookup",
                "__dns_srv_at_lookup",
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
                "__read",
                "__write",
                "__write_all",
                "__client_stream",
                "__client_deadline",
                "__client_config_deadline",
            ],
        ))
        || (same_text(module, "core.http.client")
            && one_of(method, &["get", "post", "request", "__get", "__post", "__request", "jet_http_build_resolve", "jet_http_build_get"]))
        || (same_text(module, "core.http")
            && same_text(method, "get"))
        || (same_text(module, "core.http.server")
            && one_of(method, &["serve", "serve_once", "serve_once_listener", "__serve_once", "__serve_once_listener", "__serve"]))
    {
        return Some("Time.Wait");
    }
    if same_text(module, "core.encoding") && same_text(method, "__query_read_bounded") {
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
    if same_text(module, "core.db") && one_of(method, &["transaction", "__transaction", "migrate", "__migrate"]) {
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
        || (same_text(type_name, "FileReader") && one_of(method, &["lines", "read_line", "read"]))
        || (same_text(type_name, "FileScope") && same_text(method, "read"))
        || (same_text(type_name, "FileWriter")
            && one_of(method, &["write_line", "flush", "write_all", "fsync"]))
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
    pub const KNOWN: u8 = Self::SEMA
        | Self::TIR_SUBSET
        | Self::TIR_EVAL
        | Self::AOT
        | Self::INTERPRETER
        | Self::COMPTIME;

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

/// A Core declaration's implementation owner, or its executable host symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreCallSymbol {
    /// Prefix the symbol with the generated program's Prelude root.
    Prelude(&'static str),
    /// Emit the Rust symbol as written.
    Rust(&'static str),
    /// Sema metadata only: execution must call the owning Jet source function.
    SourceOwned,
}

impl CoreCallSymbol {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Prelude(name) | Self::Rust(name) => name,
            Self::SourceOwned => panic!("source-owned Core declarations have no host symbol"),
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
    /// Checked Jet export name selected after special-call normalization.
    /// None means the declaration's member name. Ignored by host-owned rows.
    pub source_target: Option<&'static str>,
    pub coverage: CoreCallCoverage,
    /// Whether AOT can emit this row as one plain symbol call. Typed rows can
    /// still be canonical records while remaining on their typed emitter.
    pub aot_direct: bool,
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
            source_target: None,
            coverage,
            aot_direct: true,
            native_int_result: false,
            ui_capabilities: &[],
            marker: None,
        }
    }

    /// Preserve the declaration facts while requiring execution through Jet source.
    pub const fn source_owned(
        module: &'static str,
        member: &'static str,
        borrow_mask: &'static [bool],
    ) -> Self {
        let mut row = Self::new(module, member, member, false, borrow_mask);
        row.symbol = CoreCallSymbol::SourceOwned;
        row.coverage = CoreCallCoverage::from_bits(CoreCallCoverage::SEMA);
        row.interpreter_route = CoreCallInterpreterRoute::None;
        row.aot_direct = false;
        row
    }
    /// Mark an already-normalized declaration (including a receiver) as source
    /// owned without recomputing its signature, effects, or authority facts.
    pub const fn with_source_owned(mut self) -> Self {
        self.symbol = CoreCallSymbol::SourceOwned;
        self.coverage = CoreCallCoverage::from_bits(CoreCallCoverage::SEMA);
        self.interpreter_route = CoreCallInterpreterRoute::None;
        self.aot_direct = false;
        self
    }


    pub const fn with_source_target(mut self, target: &'static str) -> Self {
        self.source_target = Some(target);
        self
    }

    pub const fn source_member(self) -> Option<&'static str> {
        if !matches!(self.symbol, CoreCallSymbol::SourceOwned) {
            return None;
        }
        match self.source_target {
            Some(target) => Some(target),
            None => Some(self.member),
        }
    }

    /// Admit an implementation's checked signature without publishing another
    /// public Core member or changing ordinary import visibility.
    pub fn targets_source_function(self, module: &str, member: &str) -> bool {
        let Some(target) = self.source_member() else { return false };
        if target.starts_with("core.") {
            return target.rsplit_once('.').is_some_and(|(owner, name)| owner == module && name == member);
        }
        self.module == module && target == member
    }

    /// The existing checked plugin Authority-to-wire boundary, before forwarding.
    pub const fn authority_boundary_at(self, index: usize) -> bool {
        same_text(self.module, "core.plugin") && same_text(self.member, "load") && index == 1
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
            source_target: None,
            coverage: CoreCallCoverage::compiler_projections()
                .with_projection(CoreCallCoverage::INTERPRETER),
            aot_direct: false,
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
            ("core.watcher", "files" | "__files") => &[true],
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
// Source: Compiler/Prelude/Core.jet
// Source SHA-256: adbd7f9c759541a966120b29114424cecfc59ac917aef019ff3a9282c31d36d6
// Dispatcher rows and ambient routes are generated from Core.jet.
pub const CORE_CALL_AMBIENT_ROUTES: &[(&str, &str)] = &[
    ("core.crypto", "__jetc_open_directory"),
    ("core.crypto", "__jetc_open_child_directory"),
    ("core.crypto", "__jetc_open_source"),
    ("core.crypto", "__jetc_create_named"),
    ("core.crypto", "__jetc_tmpfile"),
    ("core.crypto", "__jetc_read"),
    ("core.crypto", "__jetc_write"),
    ("core.crypto", "__jetc_seek"),
    ("core.crypto", "__jetc_stat"),
    ("core.crypto", "__jetc_fsync"),
    ("core.crypto", "__jetc_link"),
    ("core.crypto", "__jetc_close"),
    ("core.data", "left_join"),
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
    ("core.testing", "compare"),
    ("core.testing", "assert_equal"),
    ("core.testing", "status"),
    ("core.testing", "world"),
    ("core.testing", "__evidence_read"),
    ("core.testing", "__record_failure"),
    ("core.testing", "__clear_failure"),
    ("core.testing", "__temp_facts"),
    ("core.testing", "__temp_join"),
    ("core.testing", "__temp_remove"),
    ("core.testing", "__temp_create"),
    ("core.testing", "__text_window"),
    ("core.testing", "__suite_snapshot"),
    ("core.testing", "__histories_with_provenance"),
    ("core.watcher", "files"),
    ("core.watcher", "process_pid"),
    ("core.watcher", "port"),
    ("core.watcher", "set"),
    ("core.service", "tree"),
    ("core.service", "tree_show"),
    ("core.service", "set_delivery"),
    ("core.service", "worker"),
    ("core.service", "start"),
    ("core.service", "stop"),
    ("core.service", "endpoint_send"),
    ("core.service", "endpoint_receive"),
    ("core.service", "endpoint_show"),
    ("core.service", "delivery_state_show"),
    ("core.service", "delivery_status"),
    ("core.devtools", "publish"),
    ("core.ui", "__gtk_backend"),
    ("core.log", "span"),
    ("core.log", "critical"),
    ("core.log", "enter"),
    ("core.log", "fatal"),
    ("core.log", "float"),
    ("core.log", "redact"),
    ("core.log", "close"),
    ("core.sys", "name"),
    ("core.sys", "__getuid"),
    ("core.sys", "__geteuid"),
    ("core.sys", "__getgid"),
    ("core.files", "__rename_at"),
    ("core.files", "fsync"),
    ("core.files", "symlink"),
    ("core.files", "walk"),
    ("core.files", "walk_parallel"),
    ("core.files", "walk_files"),
    ("core.files", "glob"),
    ("core.files", "canonicalize"),
    ("core.files", "__stat_at"),
    ("core.files", "__metadata_directory"),
    ("core.files", "__directory_fd"),
    ("core.files", "__host_path_is_absolute"),
    ("core.files", "__host_path_join"),
    ("core.files", "__temp_path_sample"),
    ("core.files", "__temp_directory"),
    ("core.files", "__path_kind"),
    ("core.files", "__create_dir_all_raw"),
    ("core.files", "set_mode"),
    ("core.files", "__open_reader"),
    ("core.files", "jet_std_files_open_reader_nonblocking"),
    ("core.files", "__open_writer"),
    ("core.files", "__open_directory"),
    ("core.files", "__close_directory"),
    ("core.files", "__fsync_directory"),
    ("core.files", "__create_directory"),
    ("core.files", "__list_directory"),
    ("core.files", "__remove_at"),
    ("core.files", "__read_link_at"),
    ("core.files", "__xattr_names"),
    ("core.files", "__xattr_get"),
    ("core.files", "__xattr_set"),
    ("core.files", "__reader_stat"),
    ("core.files", "__writer_stat"),
    ("core.http.client", "jet_http_build_resolve"),
    ("core.http.client", "jet_http_build_get"),
    ("core.files", "__fault_should_fail"),
    ("core.files", "__os_family"),
    ("core.files", "__map_raw"),
    ("core.files", "__remove_raw"),
    ("core.files", "__remove_dir_raw"),
    ("core.files", "__remove_all_raw"),
    ("core.files", "__copy_raw"),
    ("core.files", "__list_dir_raw"),
    ("core.files", "__hard_link_raw"),
    ("core.files", "__fsync_raw"),
    ("core.files", "__stat_mode_raw"),
    ("core.files", "__chown_raw"),
    ("core.files", "__set_mode_raw"),
    ("core.files", "__symlink_directory_raw"),
    ("core.files", "__symlink_raw"),
    ("core.files", "__mkdtemp_raw"),
    ("core.files", "__temp_dir_raw"),
    ("core.files", "__temp_file_raw"),
    ("core.files", "__lock_open"),
    ("core.files", "__lock_try"),
    ("core.files", "__lock_wait"),
    ("core.files", "__lock_close"),
    ("core.files", "__lock_fd_identity"),
    ("core.files", "__lock_path_identity"),
    ("core.files", "__lock_owner"),
    ("core.files", "__canonicalize_raw"),
    ("core.files", "__temp_clock_raw"),
    ("core.files", "__process_id_raw"),
    ("core.files", "__temp_directory_raw"),
    ("core.files", "__writer_set_mode"),
    ("core.files", "hard_link"),
    ("core.files", "lock"),
    ("core.files", "read_link"),
    ("core.files", "temp_dir"),
    ("core.files", "temp_file"),
    ("core.term", "choose"),
    ("core.term", "input_secret"),
    ("core.term", "read_all_input"),
    ("core.term", "readline"),
    ("core.term", "stdin"),
    ("core.term", "stdout"),
    ("core.term", "stderr"),
    ("core.term", "__stdio_write"),
    ("core.term", "__stdio_bytes"),
    ("core.term", "__stdio_flush"),
    ("core.term", "__stdio_fault"),
    ("core.term", "__stdout_terminal"),
    ("core.term", "__stderr_terminal"),
    ("core.term", "__stdin_line"),
    ("core.term", "terminal_width"),
    ("core.term", "terminal_height"),
    ("core.term", "jet_term_is_terminal_raw"),
    ("core.term", "style"),
    ("core.term", "progress"),
    ("core.term", "progress_iter"),
    ("core.sys", "get"),
    ("core.sys", "set"),
    ("core.sys", "family"),
    ("core.sys", "unset"),
    ("core.sys", "vars"),
    ("core.sys", "__current_dir"),
    ("core.sys", "arch"),
    ("core.sys", "__cpu_count"),
    ("core.sys", "__executable"),
    ("core.sys", "getpid"),
    ("core.sys", "__host_env"),
    ("core.sys", "__getppid"),
    ("core.sys", "__getegid"),
    ("core.sys", "__groups_count"),
    ("core.sys", "__groups_fill"),
    ("core.sys", "__getpgrp"),
    ("core.sys", "__loadavg"),
    ("core.sys", "__times"),
    ("core.sys", "sync"),
    ("core.sys", "__getpgid"),
    ("core.sys", "__getsid"),
    ("core.sys", "__umask"),
    ("core.sys", "__getpriority"),
    ("core.sys", "__setpriority"),
    ("core.sys", "__utime"),
    ("core.sys", "__errno_text"),
    ("core.sys", "__set_current_dir"),
    ("core.sys", "__fork"),
    ("core.sys", "__setuid"),
    ("core.sys", "__setgid"),
    ("core.sys", "__setpgid"),
    ("core.sys", "__setpgrp"),
    ("core.sys", "__setsid"),
    ("core.sys", "__initgroups"),
    ("core.sys", "__kill"),
    ("core.sys", "__wait"),
    ("core.sys", "__waitpid"),
    ("core.sys", "__pipe"),
    ("core.sys", "close_fd"),
    ("core.sys", "__mkfifo"),
    ("core.process", "__spec_fields"),
    ("core.process", "__with_directory_raw"),
    ("core.process", "pipeline"),
    ("core.process", "argv"),
    ("core.process", "jet_process_exit_status_raw"),
    ("core.plugin", "load"),
    ("core.mod", "load"),
    ("core.math", "from_bits"),
    ("core.math.random", "seed"),
    ("core.math.random", "__rng_state"),
    ("core.math.random", "__rng_try_update"),
    ("core.crypto.random", "__os_entropy"),
    ("core.time", "now"),
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
    ("core.log", "flush"),
    ("core.log", "field"),
    ("core.log", "int"),
    ("core.log", "bool"),
    ("core.log", "counter"),
    ("core.log", "info_fields"),
    ("core.log", "warn_fields"),
    ("core.log", "error_fields"),
    ("core.log", "debug_fields"),
    ("core.auth", "__store_load"),
    ("core.auth", "__store_replace"),
    ("core.net", "__resolve_ordered"),
    ("core.net", "__tcp_listen"),
    ("core.net", "__tcp_listen_addr"),
    ("core.net", "__tcp_accept"),
    ("core.net", "__tcp_connect"),
    ("core.net", "__tcp_connect_addr"),
    ("core.net", "__tcp_connect_timeout"),
    ("core.net", "__tcp_connect_happy"),
    ("core.net", "__listener_local_socket_addr"),
    ("core.net", "__nodelay"),
    ("core.net", "__set_nodelay"),
    ("core.net", "__ttl"),
    ("core.net", "__set_ttl"),
    ("core.net", "socket_type"),
    ("core.net", "__udp_bind"),
    ("core.net", "__udp_bind_addr"),
    ("core.net", "__udp_local_addr"),
    ("core.net", "__udp_set_timeout"),
    ("core.net", "__udp_send_to"),
    ("core.net", "__udp_recv_from"),
    ("core.net", "__udp_send_bytes_to"),
    ("core.net", "__udp_receive"),
    ("core.net.ws", "__connect"),
    ("core.net.ws", "__upgrade"),
    ("core.db", "policy_audit"),
    ("core.db", "pool"),
    ("core.db", "open"),
    ("core.db", "open_memory"),
    ("core.db", "policy"),
    ("core.ui", "__tui_backend"),
    ("core.ui", "button"),
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
    ("core.http.server", "__serve_once"),
    ("core.http.server", "__serve_once_listener"),
    ("core.http.server", "bind"),
    ("core.http.server", "__static_file"),
    ("core.http.server", "__static_file_range"),
    ("core.http.server", "__mux"),
    ("core.http.server", "__response_parts"),
    ("core.http.server", "__cors"),
    ("core.http.server", "__request_id"),
    ("core.files", "write_at"),
    ("core.files", "write_bytes"),
    ("core.files", "write_atomic"),
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
    ("core.net", "__tcp_local_socket_addr"),
    ("core.net", "__tcp_peer_socket_addr"),
    ("core.data", "inner_join"),
    ("core.net", "__unix_listen"),
    ("core.net", "__tcp_reply"),
    ("core.net", "__unix_accept"),
    ("core.log", "__record_emit"),
    ("core.log", "__flush"),
    ("core.log", "__process_exit"),
    ("core.log", "__span_allocate"),
    ("core.log", "__span_enter"),
    ("core.log", "__span_close"),
    ("core.log", "__metric_publish"),
    ("core.web.browser", "__path_join"),
    ("core.web.browser", "__read_text"),
    ("core.web.browser", "__is_file"),
    ("core.web.browser", "__file_size"),
    ("core.web.browser", "__modified_ms"),
    ("core.web.browser", "__html_path"),
];

// Evaluate the immutable bootstrap registry once, not in every consuming crate.
#[allow(long_running_const_eval)]
pub static CORE_CALLS: &[CoreCallRecord] = &[
    CoreCallRecord::new("core.crypto", "__jetc_open_directory", "jet_crypto_jetc_open_directory", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.crypto", "__jetc_open_child_directory", "jet_crypto_jetc_open_child_directory", true, &[false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.crypto", "__jetc_open_source", "jet_crypto_jetc_open_source", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.crypto", "__jetc_create_named", "jet_crypto_jetc_create_named", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.crypto", "__jetc_tmpfile", "jet_crypto_jetc_tmpfile", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.crypto", "__jetc_read", "jet_crypto_jetc_read", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.crypto", "__jetc_write", "jet_crypto_jetc_write", true, &[false, true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.crypto", "__jetc_seek", "jet_crypto_jetc_seek", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.crypto", "__jetc_stat", "jet_crypto_jetc_stat", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.crypto", "__jetc_fsync", "jet_crypto_jetc_fsync", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.crypto", "__jetc_link", "jet_crypto_jetc_link", true, &[false, false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.crypto", "__jetc_close", "jet_crypto_jetc_close", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.crypto", "__migrate_v1", &[true, true, true, true]),
    CoreCallRecord::new("core.data", "__host_path_is_file", "jet_data_host_path_is_file", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.http.server", "__serve", "jet_http_mux_serve", false, &[true, false, false, false]).without_direct_aot(),
    CoreCallRecord::new("core.http.server", "__bind", "jet_http_server_bind", false, &[true, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.http.server", "__static_files", "jet_http_srv_static_files_mount_defaulted", false, &[true, true, true, false, false, false]).without_direct_aot(),
    CoreCallRecord::new("core.http.server", "__middleware", "jet_http_mux_middleware", false, &[true, false]),
    CoreCallRecord::new("core.auth", "__state_load", "jet_auth_store_load", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.auth", "__state_replace", "jet_auth_store_replace", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__read_bytes_os", "jet_files_read_bytes_os", true, &[true]),
    CoreCallRecord::new("core.files", "__write_bytes_os", "jet_files_write_bytes_os", true, &[true, true]),
    CoreCallRecord::new("core.files", "__append_bytes_os", "jet_files_append_bytes_os", true, &[true, true]),
    CoreCallRecord::new("core.files", "__create_dirs_os", "jet_files_create_dirs_os", true, &[true]),
    CoreCallRecord::new("core.files", "__writer_refusal", "jet_std::jet_std_files_writer_refusal", true, &[true]),
    CoreCallRecord::new("core.files", "__read_at_os", "jet_files_read_at_os", true, &[true, false, false]),
    CoreCallRecord::new("core.files", "__write_at_os", "jet_files_write_at_os", true, &[true, false, true]),
    CoreCallRecord::new("core.files", "__read_link_os", "jet_files_read_link_os", true, &[true]),
    CoreCallRecord::new("core.testing", "__fault_should_fail", "jet_fault_should_fail", true, &[true]),
    CoreCallRecord::new("core.sys", "__env_value", "jet_sys_env_value_raw", true, &[true]),
    CoreCallRecord::new("core.sys", "__env_set", "jet_std_env_try_set", true, &[true, true]),
    CoreCallRecord::new("core.sys", "__env_remove", "jet_std_env_unset", true, &[true]),
    CoreCallRecord::new("core.sys", "__env_columns", "jet_sys_env_columns_raw", true, &[]),
    CoreCallRecord::new("core.sys", "__startup_env_columns", "jet_sys_startup_env_columns_raw", true, &[]),
    CoreCallRecord::new("core.sys", "__env_key_compare", "jet_sys_env_key_compare_raw", true, &[true, true]).with_native_int_result(),
    CoreCallRecord::new("core.sys", "__family_os", "jet_std_os_family", true, &[]),
    CoreCallRecord::new("core.sys", "__pid_os", "jet_std_os_pid", true, &[]).with_native_int_result(),
    CoreCallRecord::new("core.sys", "__sync_os", "jet_std_os_sync", true, &[]),
    CoreCallRecord::new("core.sys", "__close_fd_os", "jet_std_os_close_fd", true, &[false]),
    CoreCallRecord::new("core.sys", "__atexit_os", "jet_std_os_atexit", true, &[false]),
    CoreCallRecord::new("core.process", "__signal_arm", "jet_process_signal_arm_raw", true, &[false]),
    CoreCallRecord::new("core.process", "__pipeline_run", "jet_std_process_pipeline", true, &[true]),
    CoreCallRecord::new("core.files", "__path_windows", "jet_std::jet_files_path_windows_raw", true, &[]),
    CoreCallRecord::new("core.files", "__atomic_pid", "jet_std::jet_files_atomic_pid_raw", true, &[]).with_native_int_result(),
    CoreCallRecord::new("core.files", "__atomic_reserve", "jet_std::jet_files_atomic_reserve_raw", true, &[true, true]),
    CoreCallRecord::new("core.files", "__atomic_finish", "jet_std::jet_files_atomic_finish_raw", true, &[false, true, true, true]),
    CoreCallRecord::new("core.files", "__atomic_remove", "jet_std::jet_files_atomic_remove_raw", true, &[true]),
    CoreCallRecord::new("core.files", "__atomic_rename", "jet_std::jet_files_atomic_rename_raw", true, &[true, true]),
    CoreCallRecord::new("core.files", "__atomic_sync_parent", "jet_std::jet_files_atomic_sync_parent_raw", true, &[true, true]),
    CoreCallRecord::new("core.files", "__atomic_close", "jet_std::jet_fs_directory_close", true, &[false]),
    CoreCallRecord::new("core.files", "__scope_roots", "jet_files_scope_roots_raw", true, &[true]),
    CoreCallRecord::new("core.files", "__scope_read_at", "jet_files_scope_read_at_raw", true, &[true, false, true, true]),
    CoreCallRecord::new("core.sys", "__dotenv_read", "jet_sys_dotenv_read_raw", true, &[true]),
    CoreCallRecord::new("core.sys", "__config_object", "jet_sys_config_object_raw", true, &[false]),
    CoreCallRecord::new("core.files", "__reader_path", "jet_std_file_reader_path", true, &[true]),
    CoreCallRecord::new("core.files", "__reader_line_bytes", "jet_file_reader_line_bytes_raw", true, &[true]),
    CoreCallRecord::new("core.process", "__stdout_line_bytes", "jet_process_stream_line_bytes_raw", true, &[true]),
    CoreCallRecord::new("core.process", "__stderr_line_bytes", "jet_process_stream_line_bytes_raw", true, &[true]),
    CoreCallRecord::new("core.prelude", "keep", "jet_keep", true, &[false]),
    CoreCallRecord::new( "core.devtools", "publish", "jet_devtools_publish", true, &[false, false, false, false, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.mem", "volatile_read", "std::ptr::read_volatile", false, &[false], ) .without_direct_aot(),
    CoreCallRecord::new( "core.mem", "volatile_write", "std::ptr::write_volatile", false, &[false, false], ) .without_direct_aot(),
    CoreCallRecord::new("core.mem", "__list_capacity", "jet_raw_list_capacity", true, &[true]).with_native_int_result(),
    CoreCallRecord::new("core.mem", "__list_reserve_exact", "jet_raw_list_reserve_exact", true, &[true, false]),
    CoreCallRecord::new("core.mem", "__type_size", "jet_raw_type_size", true, &[]).with_native_int_result(),
    CoreCallRecord::new("core.mem", "__shared_revision", "jet_std::jet_shared_revision", true, &[true]).with_native_int_result(),
    CoreCallRecord::new("core.mem", "__snapshot_revision", "jet_std::jet_shared_snapshot_revision", true, &[true]).with_native_int_result(),
    CoreCallRecord::new("core.mem", "__query_plan", "jet_data_raw_query_plan", true, &[true]),
    CoreCallRecord::new("core.mem", "__watch_clock", "jet_live_now_ms", true, &[]).with_native_int_result(),
    CoreCallRecord::new("core.mem", "__track_plan", "jet_data_raw_track_plan", true, &[true]),
    CoreCallRecord::new("core.mem", "__extreme_key_finite", "jet_data_extreme_key_finite", true, &[true]),
    CoreCallRecord::new("core.encoding", "__read_byte", "jet_raw_encoding_read_byte", true, &[true]),
    CoreCallRecord::new("core.encoding", "__read_into", "jet_raw_encoding_read_into", true, &[true, true, false]).with_native_int_result(),
    CoreCallRecord::new("core.encoding", "__write_bytes", "jet_raw_encoding_write_bytes", true, &[true, true]),
    CoreCallRecord::new("core.encoding", "__flush", "jet_raw_encoding_flush", true, &[true]),
    CoreCallRecord::new("core.encoding", "__utf8_validate", "jet_string_validate_utf8", true, &[true]),
    CoreCallRecord::new("core.encoding", "__bytes_into_text", "jet_raw_bytes_into_text", true, &[false]),
    CoreCallRecord::new("core.encoding", "__string_byte", "jet_raw_string_byte", true, &[true, false]),
    CoreCallRecord::new("core.encoding", "__object_from_entries", "jet_raw_object_from_entries", true, &[false]),
    CoreCallRecord::new("core.encoding", "__write_text", "jet_raw_encoding_write_text", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.encoding", "__text_into_bytes", "jet_raw_text_into_bytes", true, &[false]),
    CoreCallRecord::new("core.encoding", "__typed_text", "jet_raw_typed_text", true, &[false]),
    CoreCallRecord::new("core.encoding", "__object_entries_view", "jet_raw_object_entries_view", true, &[true]),
    CoreCallRecord::new("core.encoding", "__bytes_view", "jet_raw_bytes_view", true, &[true]),
    CoreCallRecord::new("core.encoding", "__coerce_now", "jet_raw_json_coerce_now", true, &[]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.encoding", "__coerce_emit", "jet_raw_json_coerce_emit", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.encoding", "__query_read_bounded", "jet_data_query_read_bounded", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.encoding", "__query_validate_fields", "jet_data_query_validate_fields_raw", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.tasks", "interval", &[false]),
    CoreCallRecord::new("core.tasks", "__interval_ms", "jet_std::interval", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.tasks", "after", "jet_std::after_value", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.tasks", "yield_now", &[]),
    CoreCallRecord::new("core.tasks", "__yield_now", "jet_std::jet_task_yield", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.tasks", "__task_paused", "jet_scheduler_task_paused", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.tasks", "__task_cancelled", "jet_scheduler_task_cancelled_state", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.tasks", super::INTERNAL_CHANNEL_NEW_METHOD, "jet_std::channel", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.tasks", super::INTERNAL_CHANNEL_BOUNDED_METHOD, "jet_std::channel_bounded", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.tasks", "__channel_new", "jet_std::channel", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.tasks", "__channel_bounded", "jet_std::channel_bounded", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.tasks", "__condition_notify_one", "jet_std::JetCondition::notify_one", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.tasks", "__condition_notify_all", "jet_std::JetCondition::notify_all", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.prelude", "__receipt_attach", "jet_receipt_attach", true, &[true, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.reactive", "signal", "jet_std::JetSignal::new", true, &[false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.reactive.loadable", "idle", "jet_loadable_idle", false, &[]),
    CoreCallRecord::new("core.reactive.loadable", "loading", "jet_loadable_loading", false, &[]),
    CoreCallRecord::new("core.reactive.loadable", "loaded", "jet_loadable_loaded", false, &[false]),
    CoreCallRecord::new("core.reactive.loadable", "failed", "jet_loadable_failed", false, &[false]),
    CoreCallRecord::source_owned("core.event", "scope", &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.event", "decision_hook", &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.event", "hook", &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.event", "policy_sync", &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.event", "new", &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.event", "with_policy", &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.event", "async_result", &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.event", "__scope_new", "jet_std::JetEventScope::new", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.event", "__decision_hook_new", "jet_std::JetDecisionHook::new", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.event", "__hook_new", "jet_std::JetHook::new", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.event", "__policy_sync", "jet_std::JetEventPolicy::sync", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.event", "__event_with_policy", "jet_std::JetEvent::with_policy", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.event", "__async_new", "jet_std::JetAsyncEvent::new", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.math", "fraction", "jet_fraction_new", true, &[false, false], ) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "decimal", "jet_decimal_from_str", true, &[true], ) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "__numeric_stop", "jet_numeric_stop", true, &[true]).with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.files", "read", "jet_std_fs_read", true, &[true]),
    CoreCallRecord::new("core.files", "scope", "jet_std_fs_scope", true, &[true]),
    CoreCallRecord::new( "core.files", "read_bytes", "jet_std_fs_read_bytes", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.files", "map", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "write", "jet_std_fs_write", true, &[true, true], ),
    CoreCallRecord::new( "core.files", "write_bytes", "jet_std_fs_write_bytes", true, &[true, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "append_all", "jet_std_fs_append", true, &[true, true], ),
    CoreCallRecord::new("core.files", "__metadata_directory", "jet_std::jet_fs_metadata_directory_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.files", "remove", &[true]),
    CoreCallRecord::source_owned("core.files", "remove_dir", &[true]),
    CoreCallRecord::source_owned("core.files", "remove_all", &[true]),
    CoreCallRecord::source_owned("core.files", "list_dir", &[true]),
    CoreCallRecord::new( "core.files", "create_dir", "jet_std_fs_create_dir", true, &[true], ),
    CoreCallRecord::new( "core.files", "create_dir_all", "jet_std_fs_create_dir_all", true, &[true], ),
    CoreCallRecord::new("core.files", "__create_dir_all_raw", "jet_std_fs_create_dir_all_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.files", "copy", &[true, true]),
    CoreCallRecord::source_owned("core.files", "copy_dir", &[true, true]),
    CoreCallRecord::source_owned("core.files", "symlink", &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "read_link", "jet_std_fs_read_link", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.files", "hard_link", &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.files", "is_fifo", &[true]),
    CoreCallRecord::source_owned("core.files", "is_socket", &[true]),
    CoreCallRecord::source_owned("core.files", "chown", &[true, false, false]),
    CoreCallRecord::source_owned("core.files", "mkdtemp", &[true]),
    CoreCallRecord::new("core.files", "__temp_path_sample", "jet_std::jet_fs_temp_path_sample_raw", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__temp_directory", "jet_std::jet_fs_temp_directory_raw", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.files", "set_mode", &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.files", "canonicalize", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__host_path_is_absolute", "jet_std::jet_fs_host_path_is_absolute_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__host_path_join", "jet_std::jet_fs_host_path_join_raw", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.files", "walk", &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.files", "walk_parallel", &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.files", "walk_files", &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__walk_context", "jet_walk8_open", true, &[true, true, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__walk_close", "jet_walk8_close", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__walk_validation_path", "jet_walk8_validation_path", true, &[false, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__walk_ancestors", "jet_walk8_ancestors", true, &[false, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__walk_metadata", "jet_walk8_metadata", true, &[false, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__walk_shown", "jet_walk8_shown", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__walk_before", "jet_walk8_before", true, &[false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__walk_matcher_new", "jet_walk8_matcher_new", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__walk_rules", "jet_walk8_rules", true, &[false, false, true]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__walk_ignored", "jet_walk8_ignored", true, &[false, false, true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__walk_inherit", "jet_walk8_inherit", true, &[false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__walk_read_dir", "jet_walk8_read_dir", true, &[false, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__walk_next", "jet_walk8_next", true, &[false, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__entries_context", "jet_entries8_open", true, &[true, true, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__entries_close", "jet_entries8_close", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__entries_release", "jet_entries8_release", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__entries_read_dir", "jet_entries8_read_dir", true, &[false, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__entries_next", "jet_entries8_next", true, &[false, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__entries_file_type", "jet_entries8_file_type", true, &[false, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__entries_mkdir", "jet_entries8_mkdir", true, &[false, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__entries_copy", "jet_entries8_copy", true, &[false, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__entries_before", "jet_entries8_before", true, &[false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__entries_shown", "jet_entries8_shown", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.files", "glob", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "read_at", "jet_std_fs_read_at", true, &[true, false, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "write_at", "jet_std_fs_write_at", true, &[true, false, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.files", "fsync", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.files", "write_atomic", "jet_std_fs_write_atomic", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.files", "temp_dir", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.files", "temp_file", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.files", "lock", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "jet_files_link_reader_at_raw", "jet_files_link_reader_at_raw", true, &[true, false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__directory_fd", "jet_std::jet_fs_directory_fd_raw", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__fault_should_fail", "jet_fault_should_fail", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__os_family", "jet_std_os_family", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__map_raw", "jet_std::jet_std_files_map", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__remove_raw", "jet_std_fs_remove_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__remove_dir_raw", "jet_std_fs_remove_dir_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__remove_all_raw", "jet_std_fs_remove_all_raw", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__copy_raw", "jet_std_fs_copy_raw", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__list_dir_raw", "jet_std_fs_list_dir_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__hard_link_raw", "jet_std_fs_hard_link_raw", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__fsync_raw", "jet_std_fs_fsync_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__stat_mode_raw", "jet_std_fs_stat_mode_raw", true, &[true]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__chown_raw", "jet_std_fs_chown_raw", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__set_mode_raw", "jet_std_fs_set_mode_raw", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__symlink_directory_raw", "jet_std_fs_symlink_directory_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__symlink_raw", "jet_std_fs_symlink_raw", true, &[true, true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__mkdtemp_raw", "jet_std_fs_temp_dir_path_raw", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__temp_dir_raw", "jet_std_fs_temp_dir_raw", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__temp_file_raw", "jet_std_fs_temp_file_raw", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__lock_open", "jet_std::jet_fs_lock_open_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__lock_try", "jet_std::jet_fs_lock_try_raw", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__lock_wait", "jet_std::jet_fs_lock_wait_raw", true, &[false, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__lock_close", "jet_std::jet_fs_lock_close_raw", true, &[false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__lock_fd_identity", "jet_std::jet_fs_lock_fd_identity_raw", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__lock_path_identity", "jet_std::jet_fs_lock_path_identity_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__lock_owner", "jet_std::jet_fs_lock_owner_raw", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__canonicalize_raw", "jet_std_fs_canonicalize_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__temp_clock_raw", "jet_std::jet_fs_temp_path_sample_raw", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__process_id_raw", "jet_std_os_pid", true, &[]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__temp_directory_raw", "jet_std::jet_fs_temp_directory_raw", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.watcher", "files", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.watcher", "process_pid", &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.watcher", "port", &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.watcher", "set", &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.watcher", "__files", "jet_watcher_files", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.watcher", "__process_pid", "jet_watcher_process_pid", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.watcher", "__port", "jet_watcher_port", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.watcher", "__set", "jet_watcher_set", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.args", "decode", "jet_args_decode", true, &[]),
    CoreCallRecord::new("core.args", "merge", "jet_config_merge", true, &[true, true]),
    CoreCallRecord::new("core.args", "spec", "jet_args_spec", true, &[]),
    CoreCallRecord::source_owned("core.term", "choose", &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "input", "jet_std_io_input", true, &[]) .with_max_arity(1),
    CoreCallRecord::source_owned("core.term", "input_secret", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.term", "read_all_input", &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.term", "readline", &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.term", "read_until", &[true]),
    CoreCallRecord::source_owned("core.term", "take", &[false]).with_source_target("__take_bytes"),
    CoreCallRecord::new("core.term", "stdin", "jet_std_io_stdin", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "stdout", "jet_std_io_stdout", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "stderr", "jet_std_io_stderr", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__stdio_write", "jet_term_stdio_write_raw", true, &[true, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__stdio_bytes", "jet_term_stdio_bytes_raw", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__stdio_flush", "jet_term_stdio_flush_raw", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__stdio_fault", "jet_fault_should_fail", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__stdout_terminal", "jet_term_stdout_is_terminal", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__stderr_terminal", "jet_term_stderr_is_terminal", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__stdin_line", "jet_stdin_line_raw", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.term", "terminal_width", &[]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.term", "terminal_height", &[]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "jet_term_is_terminal_raw", "jet_term_is_terminal_raw", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.term", "style", &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.term", "style_force", "jet_std_io_style_force", true, &[true, true], ) .with_pure_route(CoreCallPureRoute::Io),
    CoreCallRecord::source_owned("core.term", "progress", &[true]).with_max_arity(3).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "progress_iter", "jet_std_io_progress_iter", true, &[false, true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "get", "jet_std_env_get", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "set", "jet_std_env_try_set", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "unset", "jet_std_env_unset", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "vars", "jet_std_env_vars", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__current_dir", "jet_sys_current_dir_raw", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "name", "jet_std_os_name", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "family", "jet_std_os_family", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "arch", "jet_std_os_arch", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__cpu_count", "jet_sys_cpu_count_raw", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__executable", "jet_sys_executable_raw", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "getpid", "jet_std_os_pid", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__host_env", "jet_sys_host_env_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__getppid", "jet_sys_getppid_raw", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__getuid", "jet_sys_getuid_raw", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__geteuid", "jet_sys_geteuid_raw", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__getgid", "jet_sys_getgid_raw", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__getegid", "jet_sys_getegid_raw", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__groups_count", "jet_sys_groups_count_raw", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__groups_fill", "jet_sys_groups_fill_raw", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__getpgrp", "jet_sys_getpgrp_raw", true, &[]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__loadavg", "jet_sys_loadavg_raw", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__times", "jet_sys_times_raw", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "sync", "jet_std_os_sync", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__getpgid", "jet_sys_getpgid_raw", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__getsid", "jet_sys_getsid_raw", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__umask", "jet_sys_umask_raw", true, &[false]) .with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__getpriority", "jet_sys_getpriority_raw", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__setpriority", "jet_sys_setpriority_raw", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__utime", "jet_sys_utime_raw", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__errno_text", "jet_sys_errno_text_raw", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "atexit", "jet_std_os_atexit", true, &[false]),
    CoreCallRecord::new("core.sys", "decode", "jet_std_env_decode", true, &[true, true, true]),
    CoreCallRecord::new("core.sys", "__set_current_dir", "jet_sys_set_current_dir_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__fork", "jet_sys_fork_raw", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__setuid", "jet_sys_setuid_raw", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__setgid", "jet_sys_setgid_raw", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__setpgid", "jet_sys_setpgid_raw", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__setpgrp", "jet_sys_setpgrp_raw", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__setsid", "jet_sys_setsid_raw", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__initgroups", "jet_sys_initgroups_raw", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__kill", "jet_sys_kill_raw", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__wait", "jet_sys_wait_raw", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__waitpid", "jet_sys_waitpid_raw", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__pipe", "jet_sys_pipe_raw", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.sys", "close_fd", "jet_std_os_close_fd", true, &[false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.sys", "__mkfifo", "jet_sys_mkfifo_raw", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.process", "exit", "jet_std_process_exit", true, &[false], ),
    CoreCallRecord::new("core.process", "__spec_fields", "jet_process_spec_fields_raw", true, &[false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false]).with_max_arity(26).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.process", "__with_directory_raw", "jet_process_spec_with_directory_raw", true, &[false, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.process", "pipeline", "jet_std_process_pipeline", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.process", "argv", "jet_std_io_args", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.process", "on_signal", "jet_process_on_signal", true, &[true], ),
    CoreCallRecord::new("core.process", "jet_process_exit_status_raw", "jet_process_exit_status_raw", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.plugin", "load", "jet_plugin_load", true, &[true, true]) .without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.mod", "load", "jet_mod_load", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.mod", "__supported", "jet_mod_supported_raw", true, &[]),
    CoreCallRecord::new("core.mod", "__os", "jet_mod_os_raw", true, &[]),
    CoreCallRecord::new("core.mod", "__read_context", "jet_mod_read_context_raw", true, &[true, true]).with_native_int_result(),
    CoreCallRecord::new("core.mod", "__parse_context", "jet_mod_parse_context_raw", true, &[false]),
    CoreCallRecord::new("core.mod", "__check_context", "jet_mod_check_context_raw", true, &[false]),
    CoreCallRecord::new("core.mod", "__context_name", "jet_mod_context_name_raw", true, &[false]),
    CoreCallRecord::new("core.mod", "__context_digest", "jet_mod_context_digest_raw", true, &[false]),
    CoreCallRecord::new("core.mod", "__stage_context", "jet_mod_stage_context_raw", true, &[false]),
    CoreCallRecord::new("core.mod", "__open_context", "jet_mod_open_context_raw", true, &[false]),
    CoreCallRecord::new("core.mod", "__exports_count", "jet_mod_exports_count_raw", true, &[false]).with_native_int_result(),
    CoreCallRecord::new("core.mod", "__export_info", "jet_mod_export_info_raw", true, &[false, false]),
    CoreCallRecord::new("core.mod", "__resolve_export", "jet_mod_resolve_export_raw", true, &[false, false]),
    CoreCallRecord::new("core.mod", "__resolve_symbol", "jet_mod_resolve_symbol_raw", true, &[false, true]),
    CoreCallRecord::new("core.mod", "__close_context", "jet_mod_close_context_raw", true, &[false]),
    CoreCallRecord::new("core.mod", "__finish_context", "jet_mod_finish_context_raw", true, &[false]),
    CoreCallRecord::new( "core.testing", "fake_clock", "jet_std_clock_new", true, &[false], ),
    CoreCallRecord::new( "core.testing", "fake_rng", "jet_std_rng_new", true, &[false], ),
    CoreCallRecord::new("core.testing", "compare", "jet_testing_compare", true, &[true, true, true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.testing", "assert_equal", "jet_testing_assert_equal", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.testing", "status", "jet_testing_status", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.testing", "world", "jet_testing_world", true, &[false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.testing", "__evidence_read", "jet_testing_evidence_read_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.testing", "__record_failure", "jet_testing_record_failure", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.testing", "__clear_failure", "jet_testing_clear_failure", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.testing", "__temp_facts", "jet_testing_temp_facts_raw", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.testing", "__temp_join", "jet_testing_temp_join_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.testing", "__temp_remove", "jet_testing_temp_remove_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.testing", "__temp_create", "jet_testing_temp_create_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.testing", "__text_window", "jet_testing_text_window_raw", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.testing", "__suite_snapshot", "jet_test_suite_new", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.testing", "__histories_with_provenance", "jet_testing_histories_with_provenance", true, &[true, true, false, true, true, true]).with_max_arity(6).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.math", "pi", "jet_std_math_pi", true, &[]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "cmp", "jet_std_math_cmp", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math", "is_finite", "jet_std_math_is_finite", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "is_nan", "jet_std_math_is_nan", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "is_inf", "jet_std_math_is_infinite", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "sin", "jet_std_math_sin", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "cos", "jet_std_math_cos", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "tan", "jet_std_math_tan", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "asin", "jet_std_math_asin", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "acos", "jet_std_math_acos", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "atan", "jet_std_math_atan", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "sinh", "jet_std_math_sinh", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "cosh", "jet_std_math_cosh", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "tanh", "jet_std_math_tanh", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "ln", "jet_std_math_ln", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "log10", "jet_std_math_log10", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "log2", "jet_std_math_log2", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "trunc", "jet_std_math_trunc", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "floor", "jet_std_math_floor", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "atan2", "jet_std_math_atan2", true, &[false, false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "ceil", "jet_std_math_ceil", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "hypot", "jet_std_math_hypot", true, &[false, false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "sqrt", "jet_std_math_sqrt", true, &[false]) .with_pure_route(CoreCallPureRoute::Math) .without_direct_aot(),
    CoreCallRecord::new( "core.math", "fabs", "jet_std_math_abs_f64", true, &[false], ) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "to_bits", "jet_std_math_to_bits", true, &[false], ) .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new( "core.math", "from_bits", "jet_std_math_from_bits", true, &[false], ) .with_pure_route(CoreCallPureRoute::Math) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.math", "factorial", "jet_std_math_factorial", true, &[false], ) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "factorial", "jet_std_math_factorial", true, &[false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "binomial", "jet_std_math_binomial", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "permutations_count", "jet_std_math_perm", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "combinations_count", "jet_std_math_binomial", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "npr", "jet_std_math_perm", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "ncr", "jet_std_math_binomial", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "falling_factorial", "jet_std_math_perm", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "rising_factorial", "jet_std_math_rising_factorial", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "multinomial", "jet_std_math_multinomial", true, &[true]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "chain", "jet_comb_chain", true, &[true, true]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "compress", "jet_comb_compress", true, &[true, true]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "drop", "jet_comb_drop", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "takewhile", "jet_comb_takewhile", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "dropwhile", "jet_comb_dropwhile", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "filterfalse", "jet_comb_filterfalse", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "islice", "jet_comb_islice", true, &[true, false, false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "unique", "jet_comb_unique", true, &[true]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "repeat", "jet_comb_repeat", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "count_from", "jet_comb_count_from", true, &[false, false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "count", "jet_comb_count_from", true, &[false, false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "cycle", "jet_comb_cycle", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "accumulate", "jet_comb_accumulate", true, &[true]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "reverse", "jet_comb_reverse", true, &[true]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "permutations", "jet_comb_permutations", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "combinations", "jet_comb_combinations", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "combinations_with_replacement", "jet_comb_combinations_with_replacement", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "product", "jet_comb_product", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "cartesian", "jet_comb_cartesian", true, &[true, true]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "pairwise", "jet_comb_pairwise", true, &[true]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "batched", "jet_comb_batched", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "groupby", "jet_comb_groupby", true, &[true]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "tee", "jet_comb_tee", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "zip_longest", "jet_comb_zip_longest", true, &[true, true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "powerset", "jet_comb_powerset", true, &[true]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "windows", "jet_comb_windows", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "flatten", "jet_comb_flatten", true, &[true]) .with_native_int_result(),
    CoreCallRecord::new("core.math.combinatorics", "starmap", "jet_comb_starmap", true, &[true]) .with_native_int_result(),
    CoreCallRecord::new("core.collections", "heapify", "jet_coll_heapify", true, &[true]) .with_native_int_result(),
    CoreCallRecord::new("core.collections", "heappush", "jet_coll_heappush", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.collections", "heappop", "jet_coll_heappop", true, &[true]),
    CoreCallRecord::new("core.collections", "heappushpop", "jet_coll_heappushpop", true, &[true, false]),
    CoreCallRecord::new("core.collections", "heapreplace", "jet_coll_heapreplace", true, &[true, false]),
    CoreCallRecord::new("core.collections", "bisect_left", "jet_coll_bisect_left", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.collections", "bisect_right", "jet_coll_bisect_right", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.collections", "insort_left", "jet_coll_insort_left", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.collections", "insort_right", "jet_coll_insort_right", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.collections", "merge_sorted", "jet_coll_merge_sorted", true, &[true, true]) .with_native_int_result(),
    CoreCallRecord::new("core.collections", "nsmallest", "jet_coll_nsmallest", true, &[false, true]) .with_native_int_result(),
    CoreCallRecord::new("core.collections", "nlargest", "jet_coll_nlargest", true, &[false, true]) .with_native_int_result(),
    CoreCallRecord::new("core.collections", "counter", "jet_coll_counter", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "counter_from", "jet_coll_counter_from", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "add", "jet_coll_counter_add", true, &[true, true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "inc", "jet_coll_counter_inc", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "dec", "jet_coll_counter_dec", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "get", "jet_coll_counter_get", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new("core.collections", "set_count", "jet_coll_counter_set_count", true, &[true, true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "total", "jet_coll_counter_total", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new("core.collections", "names", "jet_coll_counter_names", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "elements", "jet_coll_counter_elements", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "most_common", "jet_coll_counter_most_common", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "subtract", "jet_coll_counter_subtract", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "merge_add", "jet_coll_counter_merge_add", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "clear_counter", "jet_coll_counter_clear", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "ordered_map", "jet_coll_ordered_map", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "map_get", "jet_coll_map_get", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "map_set", "jet_coll_map_set", true, &[true, true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "map_remove", "jet_coll_map_remove", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "map_keys", "jet_coll_map_keys", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "map_values", "jet_coll_map_values", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "map_contains", "jet_coll_map_contains", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "map_len", "jet_coll_map_len", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new("core.collections", "chain", "jet_coll_chain", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "chain_push", "jet_coll_chain_push", true, &[true, true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "chain_get", "jet_coll_chain_get", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.collections", "chain_contains", "jet_coll_chain_contains", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.math", "sumprod", "jet_std_math_sumprod", true, &[false, false], ) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "binomial", "jet_std_math_binomial", true, &[false, false], ) .with_native_int_result(),
    CoreCallRecord::new( "core.math", "comb", "jet_std_math_binomial", true, &[false, false], ) .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new( "core.math", "perm", "jet_std_math_perm", true, &[false, false], ) .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new( "core.math", "fmod", "jet_std_math_fmod", true, &[false, false], ) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "checked_pow", "jet_std_math_checked_pow", true, &[false, false], ) .with_native_int_result(),
    CoreCallRecord::new("core.math", "asinh", "jet_std_math_asinh", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "acosh", "jet_std_math_acosh", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "atanh", "jet_std_math_atanh", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "cbrt", "jet_std_math_cbrt", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "exp", "jet_std_math_exp", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "exp2", "jet_std_math_exp2", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "exp_m1", "jet_std_math_exp_m1", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "expm1", "jet_std_math_exp_m1", true, &[false], ) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "fsum", "jet_std_math_fsum", true, &[false], ) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new( "core.math", "prod", "jet_std_math_prod_int", true, &[false], ) .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "ln_1p", "jet_std_math_ln_1p", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "checked_add", "jet_std_math_checked_add", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math", "checked_sub", "jet_std_math_checked_sub", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math", "checked_mul", "jet_std_math_checked_mul", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math", "saturating_sub", "jet_std_math_saturating_sub", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math", "saturating_mul", "jet_std_math_saturating_mul", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math", "tau_const", "jet_std_math_tau", true, &[]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "clamp_float", "jet_std_math_clamp_f64", true, &[false, false, false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "min_float", "jet_std_math_min_f64", true, &[false, false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "max_float", "jet_std_math_max_f64", true, &[false, false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "gcd_many", "jet_std_math_gcd_many", true, &[true]) .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "lcm_many", "jet_std_math_lcm_many", true, &[true]) .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "powmod", "jet_std_math_powmod", true, &[false, false, false]) .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "sum_int", "jet_std_math_sum_int", true, &[true]) .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "prod_int", "jet_std_math_prod_int_ref", true, &[true]) .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "abs_diff", "jet_std_math_abs_diff", true, &[false, false]) .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "even", "jet_std_math_is_even", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "odd", "jet_std_math_is_odd", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "in_range", "jet_std_math_in_range", true, &[false, false, false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "log1p", "jet_std_math_ln_1p", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "saturating_add", "jet_std_math_saturating_add", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math", "fma", "jet_std_math_fma", true, &[false, false, false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "muladd", "jet_std_math_fma", true, &[false, false, false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "truncate", "jet_std_math_trunc", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "checked_abs", "jet_std_math_checked_abs", true, &[false]) .with_native_int_result(),
    CoreCallRecord::new("core.math", "checked_neg", "jet_std_math_checked_neg", true, &[false]) .with_native_int_result(),
    CoreCallRecord::new("core.math", "checked_div", "jet_std_math_checked_div", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math", "checked_rem", "jet_std_math_checked_rem", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math", "zero", "jet_std_math_zero", true, &[]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "sin_cos", "jet_std_math_sin_cos", true, &[false]) .with_pure_route(CoreCallPureRoute::Math),
    CoreCallRecord::new("core.math", "div_mod", "jet_std_math_div_mod", true, &[false, false]) .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new("core.math", "div_rem", "jet_std_math_div_rem", true, &[false, false]) .with_pure_route(CoreCallPureRoute::Math) .with_native_int_result(),
    CoreCallRecord::new( "core.math.stats", "sum", "jet_stats_sum", true, &[true], ),
    CoreCallRecord::new( "core.math.stats", "prod", "jet_stats_prod", true, &[true], ),
    CoreCallRecord::new( "core.math.stats", "count", "jet_stats_count", true, &[true], ) .with_native_int_result(),
    CoreCallRecord::new( "core.math.stats", "cumsum", "jet_stats_cumsum", true, &[true], ),
    CoreCallRecord::new( "core.math.stats", "cumprod", "jet_stats_cumprod", true, &[true], ),
    CoreCallRecord::new( "core.math.stats", "diff", "jet_stats_diff", true, &[true], ),
    CoreCallRecord::new( "core.math.stats", "moving_average", "jet_stats_moving_average", true, &[true, false], ),
    CoreCallRecord::new( "core.math.stats", "ewma", "jet_stats_ewma", true, &[true, false], ),
    CoreCallRecord::new( "core.math.stats", "rank", "jet_stats_rank", true, &[true], ),
    CoreCallRecord::new( "core.math.stats", "zscore", "jet_stats_zscore", true, &[true], ),
    CoreCallRecord::new( "core.math.stats", "clip", "jet_stats_clip", true, &[true, false, false], ),
    CoreCallRecord::new("core.math.stats", "mean", "jet_stats_mean", true, &[true]),
    CoreCallRecord::new("core.math.stats", "fmean", "jet_stats_mean", true, &[true]),
    CoreCallRecord::new("core.math.stats", "geometric_mean", "jet_stats_geometric_mean", true, &[true]),
    CoreCallRecord::new("core.math.stats", "harmonic_mean", "jet_stats_harmonic_mean", true, &[true]),
    CoreCallRecord::new("core.math.stats", "median", "jet_stats_median", true, &[true]),
    CoreCallRecord::new("core.math.stats", "median_grouped", "jet_stats_median_grouped", true, &[true, false]),
    CoreCallRecord::new("core.math.stats", "median_low", "jet_stats_median_low", true, &[true]),
    CoreCallRecord::new("core.math.stats", "median_high", "jet_stats_median_high", true, &[true]),
    CoreCallRecord::new("core.math.stats", "quantile", "jet_stats_quantile", true, &[true, false]),
    CoreCallRecord::new("core.math.stats", "percentile", "jet_stats_percentile", true, &[true, false]),
    CoreCallRecord::new("core.math.stats", "mode", "jet_stats_mode", true, &[true]),
    CoreCallRecord::new("core.math.stats", "multimode", "jet_stats_multimode", true, &[true]),
    CoreCallRecord::new("core.math.stats", "pvariance", "jet_stats_pvariance", true, &[true]),
    CoreCallRecord::new("core.math.stats", "pstdev", "jet_stats_pstdev", true, &[true]),
    CoreCallRecord::new("core.math.stats", "variance", "jet_stats_variance", true, &[true]),
    CoreCallRecord::new("core.math.stats", "stdev", "jet_stats_stdev", true, &[true]),
    CoreCallRecord::new("core.math.stats", "covariance", "jet_stats_covariance", true, &[true, true]),
    CoreCallRecord::new("core.math.stats", "correlation", "jet_stats_correlation", true, &[true, true]),
    CoreCallRecord::new("core.math.stats", "kde", "jet_stats_kde", true, &[true, false]),
    CoreCallRecord::new("core.math.stats", "kde_random", "jet_stats_kde_random", true, &[true, false, false]),
    CoreCallRecord::new("core.math.stats", "min", "jet_stats_min", true, &[true]),
    CoreCallRecord::new("core.math.stats", "max", "jet_stats_max", true, &[true]),
    CoreCallRecord::new("core.math.stats", "range", "jet_stats_range", true, &[true]),
    CoreCallRecord::new("core.math.stats", "sumprod", "jet_stats_sumprod", true, &[true, true]),
    CoreCallRecord::new("core.math.stats", "weighted_mean", "jet_stats_weighted_mean", true, &[true, true]),
    CoreCallRecord::new("core.math.stats", "quantiles", "jet_stats_quantiles", true, &[true, false]),
    CoreCallRecord::new("core.math.stats", "iqr", "jet_stats_iqr", true, &[true]),
    CoreCallRecord::new("core.math.stats", "mad", "jet_stats_mad", true, &[true]),
    CoreCallRecord::new("core.math.stats", "mean_abs_deviation", "jet_stats_mean_abs_deviation", true, &[true]),
    CoreCallRecord::new("core.math.stats", "skew", "jet_stats_skew", true, &[true]),
    CoreCallRecord::new("core.math.stats", "kurtosis", "jet_stats_kurtosis", true, &[true]),
    CoreCallRecord::new("core.math.stats", "spearman", "jet_stats_spearman", true, &[true, true]),
    CoreCallRecord::new("core.math.stats", "residuals", "jet_stats_residuals", true, &[true, true]),
    CoreCallRecord::new("core.math.stats", "histogram", "jet_stats_histogram", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.math.stats", "winsorize", "jet_stats_winsorize", true, &[true, false]),
    CoreCallRecord::new("core.math.stats", "pearson", "jet_stats_correlation", true, &[true, true]),
    CoreCallRecord::new("core.math.stats", "covariance_population", "jet_stats_covariance_population", true, &[true, true]),
    CoreCallRecord::new( "core.math.random", "seed", "jet_std_random_seed", true, &[false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.math.random", "__rng_state", "jet_random_state_raw", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.math.random", "__rng_try_update", "jet_random_try_update_raw", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.crypto.random", "__os_entropy", "jet_random_os_entropy", true, &[false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.math.random", "rng", "jet_std_rng_new", true, &[false]),
    CoreCallRecord::new("core.time", "now", "jet_time_now_utc", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.time", "sleep_until", "jet_time_sleep_until", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.rt", "callback", &[false, false, false]),
    CoreCallRecord::new("core.rt", "__callback", "jet_rt_callback", true, &[false, false, false]),
    CoreCallRecord::new("core.rt", "__next_deadline", "jet_rt_next_deadline", true, &[true]),
    CoreCallRecord::new("core.rt", "__receipt", "jet_rt_receipt", true, &[true]),
    CoreCallRecord::new("core.rt", "__cancel", "jet_rt_cancel", true, &[true]),
    CoreCallRecord::new("core.rt", "__is_cancelled", "jet_rt_is_cancelled", true, &[true]),
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
    CoreCallRecord::new( "core.time", "parse_zoned", "jet_time_parse_zoned", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.time", "datetime", "jet_time_datetime", true, &[false, false, false, false, false, false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "time", "JetLocalTime::new", false, &[false, false, false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "local_time", "JetLocalTime::new", false, &[false, false, false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "days_in_month", "jet_time_days_in_month", true, &[false, false], ) .with_pure_route(CoreCallPureRoute::Time) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new( "core.time", "is_leap_year", "jet_time_is_leap_year", true, &[false], ) .with_pure_route(CoreCallPureRoute::Time) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.time", "period", "jet_time_period", true, &[false, false, false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "period_days", "jet_time_period_days", true, &[false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "period_months", "jet_time_period_months", true, &[false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "period_years", "jet_time_period_years", true, &[false], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "zone", "jet_time_zone_named", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.time", "utc", "jet_time_zone_utc", true, &[]) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "zoned", "jet_time_zoned", true, &[true, true]) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.time", "zoned_local", "jet_time_zoned_local", true, &[true, true, true, true], ) .with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new( "core.encoding.json", "parse", "jet_std_json_parse", true, &[true], ),
    CoreCallRecord::new( "core.encoding.json", "loads", "jet_std_json_parse", true, &[true], ),
    CoreCallRecord::source_owned("core.encoding.json", "decode", &[true]),
    CoreCallRecord::source_owned("core.encoding.json", "to_string", &[true]),
    CoreCallRecord::source_owned("core.encoding.json", "dumps", &[true]),
    CoreCallRecord::source_owned("core.encoding.json", "dump", &[true]),
    CoreCallRecord::source_owned("core.encoding.json", "to_string_pretty", &[true]),
    CoreCallRecord::new( "core.encoding.csv", "decode", "jet_enc_csv_decode", true, &[true], ),
    CoreCallRecord::new( "core.game", "run", "jet_game_run", true, &[true, true, true, true], ) .without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.encoding.json", "events", "jet_std_json_events", true, &[true], ),
    CoreCallRecord::new( "core.encoding.jsonl", "parse", "jet_std_jsonl_parse", true, &[true], ),
    CoreCallRecord::source_owned("core.encoding.jsonl", "to_string", &[true]),
    CoreCallRecord::new( "core.encoding.jsonl", "loads", "jet_std_jsonl_parse", true, &[true], ),
    CoreCallRecord::source_owned("core.encoding.jsonl", "dumps", &[true]),
    CoreCallRecord::new( "core.encoding.jsonl", "count_rows", "jet_std_jsonl_count_rows", true, &[true], ),
    CoreCallRecord::source_owned("core.encoding.jsonl", "append_line", &[true, true]),
    CoreCallRecord::new( "core.encoding.jsonl", "first", "jet_std_jsonl_first", true, &[true], ),
    CoreCallRecord::new( "core.encoding.csv", "parse", "jet_ring_csv_parse", true, &[true, true, false, false], ),
    CoreCallRecord::new( "core.encoding.csv", "rows", "jet_ring_csv_rows", true, &[true, true, false, false], ) .without_direct_aot(),
    CoreCallRecord::source_owned("core.encoding.csv", "query", &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.encoding.json", "reader", "jet_enc_json_reader", true, &[false, true]),
    CoreCallRecord::new("core.encoding.json", "writer", "jet_enc_json_writer", true, &[false, true, true]),
    CoreCallRecord::new("core.encoding.jsonl", "reader", "jet_enc_jsonl_reader", true, &[false, true]),
    CoreCallRecord::new("core.encoding.jsonl", "writer", "jet_enc_jsonl_writer", true, &[false, true]),
    CoreCallRecord::source_owned("core.encoding.cbor", "reader", &[false, true]),
    CoreCallRecord::source_owned("core.encoding.cbor", "writer", &[false, true]),
    CoreCallRecord::source_owned("core.encoding.xml", "reader", &[false, true, true]),
    CoreCallRecord::new("core.crypto.vault", "prepare_rotate", "jet_vault_prepare_rotate_impl", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "authorize_write", "jet_vault_authorize_write_impl", false, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "commit_rotate", "jet_vault_commit_rotate_impl", false, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "current", "jet_vault_current_impl", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "versions", "jet_vault_versions_impl", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "load", "jet_vault_load_impl", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "status", "jet_vault_status_impl", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "prepare_generate", "jet_vault_prepare_generate_impl", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "prepare_store", "jet_vault_prepare_store_impl", false, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "prepare_retire", "jet_vault_prepare_retire_impl", false, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "prepare_revoke", "jet_vault_prepare_revoke_impl", false, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "commit_generate", "jet_vault_commit_generate_impl", false, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "commit_store", "jet_vault_commit_store_impl", false, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "commit_retire", "jet_vault_commit_retire_impl", false, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "commit_revoke", "jet_vault_commit_revoke_impl", false, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "export_to_recipients", "jet_vault_export_to_recipients_impl", false, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "export_to_passphrase", "jet_vault_export_to_passphrase_impl", false, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "prepare_import_wrapped", "jet_vault_prepare_import_wrapped_impl", false, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "authorize_wrapped_import", "jet_vault_authorize_wrapped_import_impl", false, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "commit_import_wrapped", "jet_vault_commit_import_wrapped_impl", false, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "prepare_import_signing", "jet_vault_expert_prepare_import_signing_impl", false, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "commit_import_signing", "jet_vault_expert_commit_import_signing_impl", false, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "prepare_import_x25519", "jet_vault_expert_prepare_import_x25519_impl", false, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.crypto.vault", "commit_import_x25519", "jet_vault_expert_commit_import_x25519_impl", false, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::source_owned("core.crypto", "__vault_wrapped_from_bytes", &[false]),
    CoreCallRecord::source_owned("core.crypto", "__vault_wrapped_bytes", &[true]),
    CoreCallRecord::source_owned("core.crypto", "__vault_unlock_recipient", &[true]),
    CoreCallRecord::source_owned("core.crypto", "__vault_unlock_passphrase", &[true]),
    CoreCallRecord::new("core.crypto", "__zeroize", "jet_crypto_zeroize", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_frame_accesses(&[CoreCallResourceAccess::write(0)]),
    CoreCallRecord::new("core.compiler", "__reject", "jet_compiler_reject", true, &[true, true, true, true]).without_direct_aot(),
    CoreCallRecord::new( "core.web.storage.session", "get", "jet_web_storage_session_get", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot(),
    CoreCallRecord::new( "core.web.storage.session", "remove", "jet_web_storage_session_remove", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot(),
    CoreCallRecord::new( "core.web.storage.session", "set", "jet_web_storage_session_set", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot(),
    CoreCallRecord::new( "core.web.storage.session", "clear", "jet_web_storage_session_clear", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot(),
    CoreCallRecord::new( "core.web.storage.local", "get", "jet_web_storage_local_get", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot(),
    CoreCallRecord::new( "core.web.storage.local", "set", "jet_web_storage_local_set", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot(),
    CoreCallRecord::new( "core.web.storage.local", "remove", "jet_web_storage_local_remove", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot(),
    CoreCallRecord::new( "core.web.storage.local", "clear", "jet_web_storage_local_clear", true, &[], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot(),
    CoreCallRecord::new("core.data", "count", "jet_data_count", true, &[true]) .with_native_int_result(),
    CoreCallRecord::new("core.data", "query", "jet_data_query", true, &[true]),
    CoreCallRecord::new("core.data", "track", "jet_data_track", true, &[true, false]),
    CoreCallRecord::new("core.compute", "gradient", "jet_compute_curried_new", true, &[true]) .with_max_arity(128) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot(),
    CoreCallRecord::new("core.compute", "value_and_gradient", "jet_compute_curried_new", true, &[true]) .with_max_arity(128) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot(),
    CoreCallRecord::new("core.compute", "vjp", "jet_compute_curried_new", true, &[true]) .with_max_arity(128) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot(),
    CoreCallRecord::new("core.compute", "jvp", "jet_compute_curried_new", true, &[true]) .with_max_arity(128) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot(),
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
    CoreCallRecord::source_owned("core.service", "tree", &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.service", "tree_show", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.service", "set_delivery", &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.service", "worker", &[true, false, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "start", "jet_services_start", true, &[true]) .without_direct_aot() .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "stop", "jet_services_stop", true, &[true]) .without_direct_aot() .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "endpoint_send", "jet_services_endpoint_send", true, &[true, false]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.service", "endpoint_receive", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.service", "endpoint_show", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.service", "delivery_state_show", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.service", "delivery_status", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.service", "append_event", &[true, false]),
    CoreCallRecord::source_owned("core.service", "chaos_fail", &[true]),
    CoreCallRecord::source_owned("core.service", "commit_snapshot", &[true, false]),
    CoreCallRecord::new("core.service", "delivery_cancel", "jet_services_delivery_cancel", true, &[true]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.service", "delivery_events", &[true]),
    CoreCallRecord::source_owned("core.service", "delivery_receipt", &[true]),
    CoreCallRecord::source_owned("core.service", "delivery_retry", &[true]),
    CoreCallRecord::source_owned("core.service", "delivery_wait", &[true]),
    CoreCallRecord::new("core.service", "directory_register", "jet_services_directory_register", true, &[true, false, false]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.service", "directory_resolve", &[true, true]),
    CoreCallRecord::source_owned("core.service", "drain_dead_letters", &[true]),
    CoreCallRecord::new("core.service", "drain_worker", "jet_services_drain_worker", true, &[true, true]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.service", "fail_worker", &[true, true]),
    CoreCallRecord::source_owned("core.service", "group", &[true, false, false]),
    CoreCallRecord::new("core.service", "handoff_generation", "jet_services_handoff_generation", true, &[true]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::source_owned("core.service", "mailbox_depth", &[true, true]),
    CoreCallRecord::source_owned("core.service", "observe", &[true]),
    CoreCallRecord::new("core.service", "partition_worker", "jet_services_partition_worker", true, &[true, true]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.service", "receive", &[true, true]),
    CoreCallRecord::new("core.service", "reconcile_worker", "jet_services_reconcile_worker", true, &[true, true]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.service", "replay_events", &[true]),
    CoreCallRecord::source_owned("core.service", "restarts", &[true, true]),
    CoreCallRecord::source_owned("core.service", "restore_snapshot", &[true]),
    CoreCallRecord::new("core.service", "rollback_generation", "jet_services_rollback_generation", true, &[true]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::source_owned("core.service", "send", &[true, true, false]),
    CoreCallRecord::source_owned("core.service", "send_durable", &[true, true, false, false]),
    CoreCallRecord::source_owned("core.service", "set_restart", &[true, false]),
    CoreCallRecord::source_owned("core.service", "set_state_empty", &[true]),
    CoreCallRecord::source_owned("core.service", "set_state_event_log", &[true, false, false, false, false]),
    CoreCallRecord::source_owned("core.service", "set_state_snapshot", &[true, false, false, false, false]),
    CoreCallRecord::source_owned("core.service", "upgrade_receipt", &[true]),
    CoreCallRecord::source_owned("core.service", "workflow_activity", &[true, false, false, false, false]),
    CoreCallRecord::source_owned("core.service", "workflow_activity_complete", &[true, false, false, false]),
    CoreCallRecord::source_owned("core.service", "workflow_activity_retry", &[true, false, false, false]),
    CoreCallRecord::new("core.service", "workflow_activity_wait", "jet_services_workflow_activity_wait", true, &[true, false, false]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "workflow_all", "jet_services_workflow_all", true, &[true, false]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "workflow_sleep", "jet_services_workflow_sleep", true, &[true, false]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.service", "workflow_step", &[true, false, false]),
    CoreCallRecord::new("core.data", "file", "jet_data_loader_file", true, &[true, true, true]),
    CoreCallRecord::new( "core.data", "file_member", "jet_data_loader_file_member", true, &[true, true, true, true], ),
    CoreCallRecord::new("core.data", "url", "jet_data_loader_url", true, &[true, true, true, true]),
    CoreCallRecord::new( "core.data", "database", "jet_data_loader_database", true, &[true, true, true, true], ),
    CoreCallRecord::source_owned("core.data", "value", &[true, true]),
    CoreCallRecord::new("core.mem", "__loader_new", "jet_data_loader_new", true, &[false, false, false, false, false, false, false]),
    CoreCallRecord::new("core.data", "snapshot", "jet_data_loader_snapshot", true, &[true]),
    CoreCallRecord::new( "core.data.loader", "bind", "jet_data_loader_bind", true, &[true, true], ),
    CoreCallRecord::new( "core.data.loader", "bind_text", "jet_data_loader_bind_text", true, &[true, true], ),
    CoreCallRecord::new( "core.data.loader", "cancel", "jet_data_loader_cancel", true, &[true], ),
    CoreCallRecord::new( "core.data.loader", "offline", "jet_data_loader_offline", true, &[true, false], ),
    CoreCallRecord::new( "core.data.loader", "invalidate", "jet_data_loader_invalidate", true, &[true, false], ),
    CoreCallRecord::new( "core.data.loader", "needs_refresh", "jet_data_loader_needs_refresh", true, &[true], ),
    CoreCallRecord::new( "core.data.loader", "ready", "jet_data_loader_ready", true, &[true], ),
    CoreCallRecord::new( "core.data.loader", "status", "jet_data_loader_status", true, &[true], ),
    CoreCallRecord::new( "core.data.loader", "source_identity", "jet_data_loader_source_identity", true, &[true], ),
    CoreCallRecord::new( "core.data.loader", "authority", "jet_data_loader_authority", true, &[true], ),
    CoreCallRecord::new( "core.data.loader", "snapshot_reusable", "jet_data_snapshot_reusable", true, &[true, true], ),
    CoreCallRecord::new( "core.data.loader", "stream", "jet_data_loader_stream", true, &[true], ),
    CoreCallRecord::new( "core.data.stream", "next", "jet_data_stream_next", true, &[true], ),
    CoreCallRecord::new( "core.data.stream", "collect", "jet_data_stream_collect", true, &[true], ),
    CoreCallRecord::new( "core.data.stream", "cancel", "jet_data_stream_cancel", true, &[true], ),
    CoreCallRecord::new("core.data", "inspect", "jet_data_plot_inspect", true, &[true]),
    CoreCallRecord::new( "core.data", "inspect_json", "jet_data_plot_inspect_json", true, &[true], ),
    CoreCallRecord::new("core.data", "text", "jet_data_plot_text", true, &[true]),
    CoreCallRecord::new("core.data", "svg", "jet_data_plot_svg", true, &[true]),
    CoreCallRecord::new( "core.data", "line_text", "jet_data_line_text_plot_checked", true, &[true, true], ),
    CoreCallRecord::new( "core.data", "line_svg", "jet_data_line_svg_plot_checked", true, &[true, true], ),
    CoreCallRecord::new("core.data", "show", "jet_data_plot_show", true, &[true]),
    CoreCallRecord::new( "core.data", "render", "jet_data_plot_render", true, &[true, false], ),
    CoreCallRecord::new( "core.data.plot", "plot", "jet_data_plot", true, &[true], ),
    CoreCallRecord::new( "core.data.plot", "inspect", "jet_data_plot_inspect", true, &[true], ),
    CoreCallRecord::new( "core.data.plot", "inspect_json", "jet_data_plot_inspect_json", true, &[true], ),
    CoreCallRecord::new("core.data.plot", "text", "jet_data_plot_text", true, &[true]),
    CoreCallRecord::new("core.data.plot", "svg", "jet_data_plot_svg", true, &[true]),
    CoreCallRecord::new("core.data.plot", "show", "jet_data_plot_show", true, &[true]),
    CoreCallRecord::new( "core.data.plot", "render", "jet_data_plot_render", true, &[true, false], ),
    CoreCallRecord::new("core.data.plot", "__plot_aggregate_none", "jet_data_plot_aggregate_none", true, &[]),
    CoreCallRecord::new("core.data", "plot", "jet_data_plot", true, &[true]),
    CoreCallRecord::new( "core.data", "left_join", "jet_data_left_join_checked_default", true, &[true, true, false, false], ) .without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.data", "pivot_sum", "jet_data_pivot_sum_checked_default", true, &[true, false, false, false]).without_direct_aot(),
    CoreCallRecord::new("core.data", "describe", "jet_data_describe_checked", true, &[true]),
    CoreCallRecord::new("core.data", "rolling_mean", "jet_data_rolling_mean_checked", true, &[true, false]),
    CoreCallRecord::new("core.data", "quantile", "jet_data_quantile_checked", true, &[true, false]),
    CoreCallRecord::new("core.data", "variance", "jet_data_variance_checked", true, &[true]),
    CoreCallRecord::new("core.data", "sum", "jet_data_sum_checked", true, &[true]),
    CoreCallRecord::new("core.data", "bar_text", "jet_data_bar_text_checked", true, &[true]),
    CoreCallRecord::source_owned("core.data", "csv", &[true]),
    CoreCallRecord::source_owned("core.data", "json", &[true]),
    CoreCallRecord::new( "core.data", "csv_reader", "jet_data_csv_reader", true, &[false, false], ),
    CoreCallRecord::new( "core.data", "json_reader", "jet_data_json_reader", true, &[false, false], ),
    CoreCallRecord::new("core.text.fmt", "pretty", "jet_fmt_pretty", true, &[true]),
    CoreCallRecord::new( "core.text.fmt", "decimal", "jet_fmt_decimal", true, &[false, false], ),
    CoreCallRecord::new( "core.encoding.toml", "parse", "jet_std_toml_parse", true, &[true], ),
    CoreCallRecord::new( "core.encoding.toml", "load", "jet_std_toml_parse", true, &[true], ),
    CoreCallRecord::new( "core.encoding.toml", "loads", "jet_std_toml_parse", true, &[true], ),
    CoreCallRecord::new( "core.encoding.yaml", "parse", "jet_std_yaml_parse", true, &[true], ),
    CoreCallRecord::source_owned("core.encoding.xml", "parse", &[true]),
    CoreCallRecord::source_owned("core.encoding.xml", "parse_with", &[true, true]),
    CoreCallRecord::source_owned("core.encoding.xml", "parse_bytes", &[true, true]).with_max_arity(2),
    CoreCallRecord::source_owned("core.encoding.xml", "to_string", &[true]),
    CoreCallRecord::source_owned("core.encoding.xml", "canonical", &[true, true]),
    CoreCallRecord::source_owned("core.encoding.xml", "root", &[true]),
    CoreCallRecord::source_owned("core.encoding.xml", "attribute", &[true, true]),
    CoreCallRecord::source_owned("core.encoding.xml", "content", &[true]),
    CoreCallRecord::source_owned("core.encoding.cbor", "to_bytes", &[true]).with_marker(CoreMarkerApplication::deprecated("encode", "2027", "cbor.to_bytes", Some("2028"))),
    CoreCallRecord::source_owned("core.encoding.cbor", "parse", &[true]).with_max_arity(2).with_marker(CoreMarkerApplication::deprecated("decode", "2027", "cbor.parse", Some("2028"))),
    CoreCallRecord::new("core.encoding.binary", "pack_u8", "jet_std_binary_pack_u8", true, &[false]),
    CoreCallRecord::new("core.encoding.binary", "pack_i8", "jet_std_binary_pack_i8", true, &[false]),
    CoreCallRecord::new("core.encoding.binary", "pack_u16le", "jet_std_binary_pack_u16le", true, &[false]),
    CoreCallRecord::new("core.encoding.binary", "pack_u16be", "jet_std_binary_pack_u16be", true, &[false]),
    CoreCallRecord::new("core.encoding.binary", "pack_u32le", "jet_std_binary_pack_u32le", true, &[false]),
    CoreCallRecord::new("core.encoding.binary", "pack_u32be", "jet_std_binary_pack_u32be", true, &[false]),
    CoreCallRecord::new("core.encoding.binary", "pack_u64le", "jet_std_binary_pack_u64le", true, &[false]),
    CoreCallRecord::new("core.encoding.binary", "pack_u64be", "jet_std_binary_pack_u64be", true, &[false]),
    CoreCallRecord::new("core.encoding.binary", "unpack_u8", "jet_std_binary_unpack_u8", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "unpack_u16le", "jet_std_binary_unpack_u16le", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "unpack_u16be", "jet_std_binary_unpack_u16be", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "unpack_u32le", "jet_std_binary_unpack_u32le", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "unpack_u32be", "jet_std_binary_unpack_u32be", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "unpack_u64le", "jet_std_binary_unpack_u64le", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "unpack_u64be", "jet_std_binary_unpack_u64be", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "sign_extend", "jet_std_binary_sign_extend", true, &[false, false]) .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "pack_f64le", "jet_std_binary_pack_f64le", true, &[false]),
    CoreCallRecord::new("core.encoding.binary", "pack_f64be", "jet_std_binary_pack_f64be", true, &[false]),
    CoreCallRecord::new("core.encoding.binary", "unpack_f64le", "jet_std_binary_unpack_f64le", true, &[true, false]),
    CoreCallRecord::new("core.encoding.binary", "unpack_f64be", "jet_std_binary_unpack_f64be", true, &[true, false]),
    CoreCallRecord::new("core.encoding.binary", "calcsize", "jet_std_binary_calcsize", true, &[true]) .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "pack", "jet_std_binary_pack", true, &[true, true]),
    CoreCallRecord::new("core.encoding.binary", "unpack", "jet_std_binary_unpack", true, &[true, true]) .with_native_int_result(),
    CoreCallRecord::new("core.encoding.binary", "iter_unpack", "jet_std_binary_iter_unpack", true, &[true, true]) .with_native_int_result(),
    CoreCallRecord::new("core.files", "__open_reader", "jet_std_files_open_reader", true, &[false, true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "jet_std_files_open_reader_nonblocking", "jet_std_files_open_reader_nonblocking", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__open_writer", "jet_std_files_open_writer", true, &[false, true, false, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__open_directory", "jet_std::jet_fs_directory_open", true, &[false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_native_int_result(),
    CoreCallRecord::new("core.files", "__close_directory", "jet_std::jet_fs_directory_close", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__fsync_directory", "jet_std::jet_fs_directory_fsync", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__create_directory", "jet_std::jet_fs_directory_create", true, &[false, true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__list_directory", "jet_std::jet_fs_directory_list", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__stat_at", "jet_std::jet_fs_stat_at", true, &[false, true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_native_int_result(),
    CoreCallRecord::new("core.files", "__path_kind", "jet_std::jet_fs_path_kind", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_native_int_result(),
    CoreCallRecord::new("core.files", "__rename_at", "jet_std::jet_fs_rename_at", true, &[false, true, false, true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__remove_at", "jet_std::jet_fs_remove_at", true, &[false, true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__read_link_at", "jet_std::jet_fs_read_link_at", true, &[false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__xattr_names", "jet_std::jet_fs_xattr_names", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__xattr_get", "jet_std::jet_fs_xattr_get", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__xattr_set", "jet_std::jet_fs_xattr_set", true, &[true, true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__reader_stat", "jet_std_file_reader_stat", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_native_int_result(),
    CoreCallRecord::new("core.files", "__writer_stat", "jet_std_file_writer_stat", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_native_int_result(),
    CoreCallRecord::new("core.files", "__writer_set_mode", "jet_std_file_writer_set_mode", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.files", "__flock", "jet_std::jet_fs_flock", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net.url", "from_parts", "jet_url_from_parts", true, &[true, true, true, true, true], ),
    CoreCallRecord::new("core.net.url", "file", "jet_url_file", true, &[true]),
    CoreCallRecord::new("core.net.url", "data", "jet_url_data", true, &[true, true]),
    CoreCallRecord::new("core.net.url", "query", "jet_url_query", true, &[true]),
    CoreCallRecord::new( "core.net.url", "percent_encode", "jet_url_percent_encode_component", true, &[true], ),
    CoreCallRecord::new( "core.net.url", "percent_decode", "jet_url_percent_decode_component", true, &[true], ),
    CoreCallRecord::new("core.net.url", "geturl", "jet_url_to_string", true, &[true],),
    CoreCallRecord::new("core.net.url", "unparse", "jet_url_to_string", true, &[true],),
    CoreCallRecord::new("core.net.url", "urljoin", "jet_url_join", true, &[true, true],),
    CoreCallRecord::new("core.net.url", "parse_qsl", "jet_url_parse_qsl", true, &[true],),
    CoreCallRecord::new("core.net.url", "parse_qs", "jet_url_parse_qsl", true, &[true],),
    CoreCallRecord::new("core.net.url", "urlencode", "jet_url_urlencode", true, &[true],),
    CoreCallRecord::new("core.net.url", "split_fragment", "jet_url_split_fragment", true, &[true],),
    CoreCallRecord::new("core.net.url", "quote", "jet_url_quote", true, &[true],),
    CoreCallRecord::new("core.net.url", "quote_from_bytes", "jet_url_quote_from_bytes", true, &[true],),
    CoreCallRecord::new("core.net.url", "quote_plus", "jet_url_quote_plus", true, &[true],),
    CoreCallRecord::new("core.net.url", "unquote", "jet_url_unquote", true, &[true],),
    CoreCallRecord::new("core.net.url", "unquote_to_bytes", "jet_url_unquote_to_bytes", true, &[true],),
    CoreCallRecord::new("core.net.url", "unquote_plus", "jet_url_unquote_plus", true, &[true],),
    CoreCallRecord::new("core.net.url", "urlunparse", "jet_url_to_string", true, &[true],),
    CoreCallRecord::new("core.net.url", "urlunsplit", "jet_url_to_string", true, &[true],),
    CoreCallRecord::new("core.net.url", "urldefrag", "jet_url_urldefrag", true, &[true],),
    CoreCallRecord::new("core.net.mime", "parse", "jet_mime_parse", true, &[true]) .with_pure_route(CoreCallPureRoute::Mime),
    CoreCallRecord::new( "core.net.mime", "from_extension", "jet_mime_from_extension", true, &[true], ) .with_pure_route(CoreCallPureRoute::Mime),
    CoreCallRecord::new( "core.net.mime", "extension", "jet_mime_extension", true, &[true], ) .with_pure_route(CoreCallPureRoute::Mime),
    CoreCallRecord::new("core.email", "address", "jet_email::address", true, &[true]) .with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::new( "core.email", "attachment", "jet_email::attachment", true, &[true, true, true], ) .with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::source_owned("core.email", "message", &[true, true, true, true, true, true, true]),
    CoreCallRecord::new( "core.email", "envelope", "jet_email::envelope", true, &[true, true], ) .with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::source_owned("core.email", "serialize", &[true]),
    CoreCallRecord::new("core.email", "__address_parts", "jet_email::address_parts", true, &[false, false]).with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::new("core.email", "__attachment_parts", "jet_email::attachment_parts", true, &[false, false, false]).with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::new("core.email", "__envelope_parts", "jet_email::envelope_parts", true, &[false, false]).with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::new("core.email", "__address_view", "jet_email::address_view", true, &[true]).with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::new("core.email", "__attachment_view", "jet_email::attachment_view", true, &[true]).with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::new("core.email", "__message_view", "jet_email::message_view", true, &[true]).with_pure_route(CoreCallPureRoute::Email),
    CoreCallRecord::new("core.email", "__message_parts", "jet_email::message_parts", true, &[false, false, false, false, false, false, false, false, false]).with_pure_route(CoreCallPureRoute::Email),
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
    CoreCallRecord::new("core.text.parse", "starts_with", "jet_string_starts_with", true, &[true, true]),
    CoreCallRecord::new("core.text.parse", "ends_with", "jet_string_ends_with", true, &[true, true]),
    CoreCallRecord::new("core.text.parse", "strip_prefix", "jet_text_remove_prefix", true, &[true, true]),
    CoreCallRecord::new("core.text.parse", "strip_suffix", "jet_text_remove_suffix", true, &[true, true]),
    CoreCallRecord::new("core.text.parse", "is_digit", "jet_text_isdigit", true, &[true]),
    CoreCallRecord::new("core.text.parse", "is_alnum", "jet_text_isalnum", true, &[true]),
    CoreCallRecord::new("core.text.parse", "is_space", "jet_text_is_whitespace", true, &[true]),
    CoreCallRecord::new("core.text.parse", "is_lower", "jet_text_is_lower", true, &[true]),
    CoreCallRecord::new("core.text.parse", "is_upper", "jet_text_is_upper", true, &[true]),
    CoreCallRecord::new("core.text.parse", "is_title", "jet_text_istitle", true, &[true]),
    CoreCallRecord::new("core.text.parse", "is_ascii", "jet_text_unicode_is_ascii", true, &[true]),
    CoreCallRecord::new("core.text.parse", "is_identifier", "jet_text_isidentifier", true, &[true]),
    CoreCallRecord::new("core.text.parse", "expandtabs", "jet_text_expandtabs", true, &[true, false]),
    CoreCallRecord::new("core.text.parse", "capitalize", "jet_text_capitalize", true, &[true]),
    CoreCallRecord::new("core.text.parse", "lower", "jet_text_lower", true, &[true]),
    CoreCallRecord::new("core.text.parse", "swapcase", "jet_text_swapcase", true, &[true]),
    CoreCallRecord::new("core.text.parse", "title", "jet_text_title", true, &[true]),
    CoreCallRecord::new("core.text.parse", "upper", "jet_text_upper", true, &[true]),
    CoreCallRecord::new("core.text.parse", "center", "jet_text_center_i64", true, &[true, false, true]),
    CoreCallRecord::new("core.text.parse", "ljust", "jet_text_pad_end_i64", true, &[true, false, true]),
    CoreCallRecord::new("core.text.parse", "rjust", "jet_text_pad_start_i64", true, &[true, false, true]),
    CoreCallRecord::new("core.text.parse", "zfill", "jet_text_zfill", true, &[true, false]),
    CoreCallRecord::new("core.text.parse", "split", "jet_text_parse_split", true, &[true, true]),
    CoreCallRecord::new("core.text.parse", "find", "jet_text_parse_find", true, &[true, true]) .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "rfind", "jet_text_parse_rfind", true, &[true, true]) .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "count", "jet_text_parse_count", true, &[true, true]) .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "replace", "jet_text_parse_replace_n", true, &[true, true, true, false]),
    CoreCallRecord::new("core.text.parse", "rsplit", "jet_text_parse_rsplit", true, &[true, true, false]),
    CoreCallRecord::new("core.text.parse", "split_once", "jet_text_parse_split_once", true, &[true, true]),
    CoreCallRecord::new("core.text.parse", "partition", "jet_text_parse_partition", true, &[true, true]),
    CoreCallRecord::new("core.text.parse", "rpartition", "jet_text_parse_rpartition", true, &[true, true]),
    CoreCallRecord::new("core.text.parse", "split_ws", "jet_text_parse_split_ws", true, &[true]),
    CoreCallRecord::new("core.text.parse", "parse_bool", "jet_text_parse_bool", true, &[true]),
    CoreCallRecord::new("core.text.parse", "parse_int", "jet_text_parse_int", true, &[true]) .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "parse", "jet_text_parse_int", true, &[true]) .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "parse_int_base", "jet_text_parse_int_base", true, &[true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "parse_float", "jet_text_parse_float", true, &[true]),
    CoreCallRecord::new("core.text.parse", "parse_kv", "jet_text_parse_kv", true, &[true, true]),
    CoreCallRecord::new("core.text.parse", "join", "jet_text_parse_join", true, &[true, true]),
    CoreCallRecord::new("core.text.parse", "splitlines", "jet_text_parse_splitlines", true, &[true, false]),
    CoreCallRecord::new("core.text.parse", "capwords", "jet_text_parse_capwords", true, &[true]),
    CoreCallRecord::new("core.text.parse", "find_from", "jet_text_parse_find_from", true, &[true, true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "rfind_from", "jet_text_parse_rfind_from", true, &[true, true, false]) .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "index", "jet_text_parse_index", true, &[true, true]) .with_native_int_result(),
    CoreCallRecord::new("core.text.parse", "contains", "jet_text_parse_contains", true, &[true, true]),
    CoreCallRecord::new("core.text.parse", "lstrip", "jet_text_parse_lstrip", true, &[true]),
    CoreCallRecord::new("core.text.parse", "rstrip", "jet_text_parse_rstrip", true, &[true]),
    CoreCallRecord::new("core.text.parse", "strip", "jet_text_parse_strip", true, &[true]),
    CoreCallRecord::new("core.text.parse", "escape_c", "jet_text_parse_escape_c", true, &[true]),
    CoreCallRecord::new("core.text.parse", "unescape_c", "jet_text_parse_unescape_c", true, &[true]),
    CoreCallRecord::new("core.text.wrap", "wrap", "jet_text_wrap", true, &[true, false]),
    CoreCallRecord::new("core.text.wrap", "fill", "jet_text_fill", true, &[true, false]),
    CoreCallRecord::new("core.text.wrap", "shorten", "jet_text_shorten", true, &[true, false, true]),
    CoreCallRecord::new("core.text.wrap", "indent", "jet_text_indent", true, &[true, true]),
    CoreCallRecord::new("core.text.wrap", "dedent", "jet_text_dedent", true, &[true]),
    CoreCallRecord::new("core.text.wrap", "expand_tabs", "jet_text_expand_tabs", true, &[true, false]),
    CoreCallRecord::new("core.text.wrap", "html_escape", "jet_text_html_escape", true, &[true, false]),
    CoreCallRecord::new("core.text.wrap", "html_unescape", "jet_text_html_unescape", true, &[true]),
    CoreCallRecord::new("core.text.wrap", "indent_with", "jet_text_indent_with", true, &[true, true, false]),
    CoreCallRecord::new("core.text.wrap", "wrap_paragraphs", "jet_text_wrap_paragraphs", true, &[true, false]),
    CoreCallRecord::new("core.text.wrap", "hanging_indent", "jet_text_hanging_indent", true, &[true, true, true, false]),
    CoreCallRecord::new("core.text.html", "escape", "jet_text_html_escape_quoted", true, &[true]),
    CoreCallRecord::new("core.text.html", "escape_quote", "jet_text_html_escape", true, &[true, false]),
    CoreCallRecord::new("core.text.html", "unescape", "jet_text_html_unescape", true, &[true]),
    CoreCallRecord::new("core.text.html", "strip_tags", "jet_text_html_strip_tags", true, &[true]),
    CoreCallRecord::new("core.text.html", "unescape_and_strip", "jet_text_html_unescape_and_strip", true, &[true]),
    CoreCallRecord::new("core.text.html", "attr_escape", "jet_text_html_escape_quoted", true, &[true]),
    CoreCallRecord::new("core.text.html", "text_escape", "jet_text_html_escape_text", true, &[true]),
    CoreCallRecord::new("core.log", "info", "jet_ring_log_info", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "warn", "jet_ring_log_warn", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "error", "jet_ring_log_error", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "debug", "jet_ring_log_debug", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.log", "critical", "jet_ring_log_critical", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "fatal", "jet_ring_log_fatal", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "flush", "jet_ring_log_flush", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
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
    CoreCallRecord::new( "core.log", "counter", "jet_ring_log_counter", true, &[true, false], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.log", "__trace_store", "jet_ring_log_set_trace_id", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "__level_load", "jet_log_level_load", true, &[]) .with_native_int_result() .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "__level_store", "jet_log_level_store", true, &[false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "__disabled_load", "jet_log_disabled_load", true, &[]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "__disabled_store", "jet_log_disabled_store", true, &[false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "__format_store", "jet_log_format_store", true, &[false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "__sink_store", "jet_log_sink_store", true, &[true]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "__sample_store", "jet_log_sample_store", true, &[false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "__sample_count_store", "jet_log_sample_count_store", true, &[false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "__record_emit", "jet_log_record_emit_raw", true, &[true, true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "__flush", "jet_ring_log_flush", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "__process_exit", "jet_log_process_exit", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "__span_allocate", "jet_log_span_allocate_raw", true, &[true]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "__span_enter", "jet_ring_log_enter", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "__span_close", "jet_ring_log_close", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.log", "__metric_publish", "jet_log_publish_metric", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.auth", "__store_load", "jet_auth_store_load", true, &[]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.auth", "__store_replace", "jet_auth_store_replace", true, &[true, false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
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
    CoreCallRecord::new("core.net", "__resolve_ordered", "jet_net_raw_resolve_ordered", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "tcp_listen", &[true]),
    CoreCallRecord::source_owned("core.net", "tcp_listen_addr", &[true]),
    CoreCallRecord::source_owned("core.net", "tcp_accept", &[true]),
    CoreCallRecord::source_owned("core.net", "tcp_connect", &[true]),
    CoreCallRecord::source_owned("core.net", "tcp_connect_addr", &[true]),
    CoreCallRecord::source_owned("core.net", "tcp_connect_timeout", &[true, false]),
    CoreCallRecord::source_owned("core.net", "tcp_connect_happy", &[true, false, false]),
    CoreCallRecord::new("core.net", "__tcp_listen", "jet_net_tcp_listen", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__tcp_listen_addr", "jet_net_tcp_listen_addr", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__tcp_accept", "jet_net_tcp_accept", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__tcp_connect", "jet_net_tcp_connect", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__tcp_connect_addr", "jet_net_tcp_connect_addr", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__tcp_connect_timeout", "jet_net_tcp_connect_timeout", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__tcp_connect_happy", "jet_net_tcp_connect_happy", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "set_timeout", &[true, false]),
    CoreCallRecord::source_owned("core.net", "set_read_timeout", &[true, false]),
    CoreCallRecord::source_owned("core.net", "set_write_timeout", &[true, false]),
    CoreCallRecord::new("core.net", "__set_timeout", "jet_net_set_timeout", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__set_read_timeout", "jet_net_set_read_timeout", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__set_write_timeout", "jet_net_set_write_timeout", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "tcp_local_addr", "jet_net_tcp_local_addr", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "tcp_peer_addr", "jet_net_tcp_peer_addr", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "tcp_local_socket_addr", &[true]),
    CoreCallRecord::source_owned("core.net", "tcp_peer_socket_addr", &[true]),
    CoreCallRecord::source_owned("core.net", "listener_local_socket_addr", &[true]),
    CoreCallRecord::new("core.net", "__tcp_local_socket_addr", "jet_net_tcp_local_socket_addr", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__tcp_peer_socket_addr", "jet_net_tcp_peer_socket_addr", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__listener_local_socket_addr", "jet_net_listener_local_socket_addr", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "nodelay", &[true]),
    CoreCallRecord::source_owned("core.net", "set_nodelay", &[true, false]),
    CoreCallRecord::source_owned("core.net", "ttl", &[true]),
    CoreCallRecord::source_owned("core.net", "set_ttl", &[true, false]),
    CoreCallRecord::new("core.net", "__nodelay", "jet_net_nodelay", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__set_nodelay", "jet_net_set_nodelay", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__ttl", "jet_net_ttl", true, &[true]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__set_ttl", "jet_net_set_ttl", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "socket_type", "jet_net_socket_type", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "tcp_reply", &[false, true, true]),
    CoreCallRecord::source_owned("core.net", "udp_bind", &[true]),
    CoreCallRecord::source_owned("core.net", "udp_bind_addr", &[true]),
    CoreCallRecord::new("core.net", "__tcp_reply", "jet_net_tcp_reply", true, &[false, true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__udp_bind", "jet_net_udp_bind", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__udp_bind_addr", "jet_net_udp_bind_addr", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "udp_local_addr", &[true]),
    CoreCallRecord::new("core.net", "__udp_local_addr", "jet_net_udp_local_addr", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "udp_set_timeout", &[true, false]),
    CoreCallRecord::new("core.net", "__udp_set_timeout", "jet_net_udp_set_timeout", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "udp_send_to", &[true, true, true]),
    CoreCallRecord::new("core.net", "__udp_send_to", "jet_net_udp_send_to", true, &[true, true, true]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "udp_recv_from", &[true, false]),
    CoreCallRecord::new("core.net", "__udp_recv_from", "jet_net_udp_recv_from", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "udp_send_bytes_to", &[true, true, true]),
    CoreCallRecord::new("core.net", "__udp_send_bytes_to", "jet_net_udp_send_bytes_to", true, &[true, true, true]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "udp_receive", &[true, false]),
    CoreCallRecord::new("core.net", "__udp_receive", "jet_net_udp_receive", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "unix_listen", &[true]),
    CoreCallRecord::new("core.net", "__unix_listen", "jet_net_unix_listen", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "unix_accept", &[true]),
    CoreCallRecord::new("core.net", "__unix_accept", "jet_net_unix_accept", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "unix_read", &[true]),
    CoreCallRecord::new("core.net", "__unix_read", "jet_net_unix_read", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "unix_write_all_bytes", &[true, true]),
    CoreCallRecord::new("core.net", "__unix_write_all_bytes", "jet_net_unix_write_all_bytes", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "unix_connect", "jet_net_unix_connect", true, &[true], ) .with_max_arity(2) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_source_owned() .with_source_target("__unix_connect_entry"),
    CoreCallRecord::new("core.net", "__unix_connect_host", "jet_net_unix_connect", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__unix_connect_deadline_host", "jet_net_unix_connect_deadline", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "unix_shutdown", &[true, true]),
    CoreCallRecord::new("core.net", "__unix_shutdown", "jet_net_unix_shutdown", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "unix_close", &[true]),
    CoreCallRecord::new("core.net", "__unix_close", "jet_net_unix_close", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net", "dns_a", &[true, false]),
    CoreCallRecord::source_owned("core.net", "dns_a_at", &[true, true, false]),
    CoreCallRecord::source_owned("core.net", "dns_aaaa", &[true, false]),
    CoreCallRecord::source_owned("core.net", "dns_aaaa_at", &[true, true, false]),
    CoreCallRecord::source_owned("core.net", "dns_txt", &[true, false]),
    CoreCallRecord::source_owned("core.net", "dns_txt_at", &[true, true, false]),
    CoreCallRecord::source_owned("core.net", "dns_ptr", &[true, false]),
    CoreCallRecord::source_owned("core.net", "dns_srv", &[true, false]),
    CoreCallRecord::source_owned("core.net", "dns_srv_at", &[true, true, false]),
    CoreCallRecord::new("core.net", "__dns_a_lookup", "jet_net_dns_a_lookup", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__dns_a_at_lookup", "jet_net_dns_a_at_lookup", true, &[true, true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__dns_aaaa_lookup", "jet_net_dns_aaaa_lookup", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__dns_aaaa_at_lookup", "jet_net_dns_aaaa_at_lookup", true, &[true, true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__dns_txt_lookup", "jet_net_dns_txt_lookup", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__dns_txt_at_lookup", "jet_net_dns_txt_at_lookup", true, &[true, true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__dns_ptr_lookup", "jet_net_dns_ptr_lookup", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__dns_srv_lookup", "jet_net_dns_srv_lookup", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.net", "__dns_srv_at_lookup", "jet_net_dns_srv_at_lookup", true, &[true, true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net.tls", "client", "jet_net_tls_client", false, &[false, true], ) .with_max_arity(4) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .with_source_owned() .with_source_target("client"),
    CoreCallRecord::new("core.net.tls", "__client_stream", "jet_net_tls_client", false, &[false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.net.tls", "__client_deadline", "jet_net_tls_client_deadline", false, &[false, true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::new("core.net.tls", "__client_config_deadline", "jet_net_tls_client_config_deadline", false, &[false, true, true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).without_direct_aot(),
    CoreCallRecord::source_owned("core.net.tls", "read", &[true, false]),
    CoreCallRecord::new("core.net.tls", "__read", "jet_net_tls_read_bytes", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net.tls", "write", &[true, true]),
    CoreCallRecord::new("core.net.tls", "__write", "jet_net_tls_write_bytes", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_native_int_result(),
    CoreCallRecord::source_owned("core.net.tls", "write_all", &[true, true]),
    CoreCallRecord::new("core.net.tls", "__write_all", "jet_net_tls_write_all_bytes", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.net", "dns_srv_target", "jet_net_dns_srv_target", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new( "core.net", "dns_srv_port", "jet_net_dns_srv_port", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net) .with_native_int_result(),
    CoreCallRecord::new( "core.net", "dns_srv_priority", "jet_net_dns_srv_priority", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net) .with_native_int_result(),
    CoreCallRecord::new( "core.net", "dns_srv_weight", "jet_net_dns_srv_weight", true, &[true], ) .with_pure_route(CoreCallPureRoute::Net) .with_native_int_result(),
    CoreCallRecord::source_owned("core.http", "router", &[]),
    CoreCallRecord::new("core.http", "__router", "jet_http_router_new", true, &[]),
    CoreCallRecord::source_owned("core.http", "parse", &[true]),
    CoreCallRecord::new("core.http", "__parse", "jet_http_parse_request", true, &[true]),
    CoreCallRecord::source_owned("core.http", "dispatch", &[true, false]),
    CoreCallRecord::new("core.http", "__dispatch", "jet_http_router_dispatch", true, &[true, false]),
    CoreCallRecord::new("core.http", "reason_phrase", "jet_http_reason_phrase", true, &[false]),
    CoreCallRecord::new("core.http", "basic_auth", "jet_http_basic_auth", true, &[true, true]),
    CoreCallRecord::new("core.http", "bearer_auth", "jet_http_bearer_auth", true, &[true]),
    CoreCallRecord::new( "core.http", "serve", "jet_http_server_default", true, &[true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.http", "get", "jet_http_client_get", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.http.client", "get", &[true]),
    CoreCallRecord::new("core.http.client", "__get", "jet_http_client_get", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.http.client", "post", &[true, true]),
    CoreCallRecord::new("core.http.client", "__post", "jet_http_client_post", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.http.client", "request", &[true, true]),
    CoreCallRecord::new("core.http.client", "__request", "jet_http_client_request_new", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.http.client", "jet_http_build_resolve", "jet_http_build_resolve", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.http.client", "jet_http_build_get", "jet_http_build_get", true, &[true, true, true, false, true, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.regex", "flags", &[false, false, false]),
    CoreCallRecord::source_owned("core.regex", "escape", &[true]),
    CoreCallRecord::source_owned("core.regex", "compile", &[true]),
    CoreCallRecord::source_owned("core.regex", "compile_with", &[true, true]),
    CoreCallRecord::source_owned("core.regex", "is_match", &[true, true]),
    CoreCallRecord::source_owned("core.regex", "full_match", &[true, true]),
    CoreCallRecord::source_owned("core.regex", "match", &[true, true]),
    CoreCallRecord::source_owned("core.regex", "find", &[true, true]),
    CoreCallRecord::source_owned("core.regex", "find_all", &[true, true]),
    CoreCallRecord::source_owned("core.regex", "matches", &[true, true]),
    CoreCallRecord::source_owned("core.regex", "finditer", &[true, true]),
    CoreCallRecord::source_owned("core.regex", "split", &[true, true]),
    CoreCallRecord::source_owned("core.regex", "split_limit", &[true, true, false]),
    CoreCallRecord::source_owned("core.regex", "replace", &[true, true, true]),
    CoreCallRecord::source_owned("core.regex", "replace_first", &[true, true, true]),
    CoreCallRecord::source_owned("core.regex", "expand", &[true, true]),
    CoreCallRecord::source_owned("core.regex", "purge", &[]),
    CoreCallRecord::source_owned("core.regex", "join", &[true]),
    CoreCallRecord::source_owned("core.regex", "subn", &[true, true, true]),
    CoreCallRecord::source_owned("core.regex", "regex", &[true]),
    CoreCallRecord::source_owned("core.regex", "search", &[true, true]),
    CoreCallRecord::source_owned("core.regex", "findall", &[true, true]),
    CoreCallRecord::source_owned("core.regex", "fullmatch", &[true, true]),
    CoreCallRecord::source_owned("core.regex", "sub", &[true, true, true]),
    CoreCallRecord::source_owned("core.game.raylib", "window_open", &[false, false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.game.raylib", "window_should_close", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.game.raylib", "window_ready", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.game.raylib", "begin_drawing", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.game.raylib", "clear_background", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.game.raylib", "draw_text", &[true, false, false, false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.game.raylib", "draw_rectangle", &[false, false, false, false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.game.raylib", "end_drawing", &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.game.raylib", "close_window", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.game.raylib", "key_down", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.game.raylib", "set_target_fps", &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.game.raylib", "gamepad_down", &[false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.game.raylib", "gamepad_axis", &[false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.game.raylib", "load_texture_atlas", &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.game.raylib", "draw_sprite", &[true, true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.game.raylib", "__window_open", "jet_raylib_window_open", true, &[false, false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.game.raylib", "__window_should_close", "jet_raylib_window_should_close", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.game.raylib", "__window_ready", "jet_raylib_window_ready", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.game.raylib", "__begin_drawing", "jet_raylib_begin_drawing", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.game.raylib", "__clear_background", "jet_raylib_clear_background", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.game.raylib", "__draw_text", "jet_raylib_draw_text", true, &[true, false, false, false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.game.raylib", "__draw_rectangle", "jet_raylib_draw_rectangle", true, &[false, false, false, false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.game.raylib", "__end_drawing", "jet_raylib_end_drawing", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.game.raylib", "__close_window", "jet_raylib_close_window", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.game.raylib", "__key_down", "jet_raylib_key_down", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.game.raylib", "__set_target_fps", "jet_raylib_set_target_fps", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.game.raylib", "__gamepad_down", "jet_raylib_gamepad_down", true, &[false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.game.raylib", "__gamepad_axis", "jet_raylib_gamepad_axis", true, &[false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.game.raylib", "__load_texture_atlas", "jet_raylib_load_texture_atlas", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.game.raylib", "__draw_sprite", "jet_raylib_draw_sprite", true, &[true, true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.db", "open", "jet_db_open", true, &[true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.db", "open_memory", "jet_db_open_memory", true, &[], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.db", "pool", "jet_db_pool_new", true, &[true, false], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.db", "policy", "jet_db_policy_checked", true, &[true, true], ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.db", "__open", "jet_db_open", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.db", "__open_memory", "jet_db_open_memory", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.db", "__pool", "jet_db_pool_new", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.db", "__policy", "jet_db_policy_checked", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.db", "__policy_audit", "jet_db_policy_audit", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.db", "__transaction", "jet_db_scope_transaction", false, &[true, true, true]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_native_int_result(),
    CoreCallRecord::new("core.db", "__migrate", "jet_db_scope_migrate", false, &[true, true, true]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_native_int_result(),
    CoreCallRecord::new("core.db", "__queue_raw_open", "jet_db_queue_raw_open", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.db", "__queue_raw_close", "jet_db_queue_raw_close", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.db", "__queue_raw_begin", "jet_db_queue_raw_begin", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.db", "__queue_raw_commit", "jet_db_queue_raw_commit", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.db", "__queue_raw_rollback", "jet_db_queue_raw_rollback", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.db", "__queue_raw_execute", "jet_db_queue_raw_execute", true, &[false, true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_native_int_result(),
    CoreCallRecord::new("core.db", "__queue_raw_query", "jet_db_queue_raw_query", true, &[false, true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.jobs", "queue", &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.jobs", "__publish_parts", "jet_job_queue_publish_parts", true, &[false, true, true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.jobs", "__sleep_ms", "jet_job_queue_sleep_ms", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "enqueue", "", true, &[true, true, true], ) .with_max_arity(4).with_source_owned().with_source_target("core.jobs.__enqueue"),
    CoreCallRecord::receiver_with_symbol( &["JobQueue"], "delay", "", true, &[true, false, false, false], ).with_source_owned().with_source_target("core.jobs.__delay"),
    CoreCallRecord::new( "core.db", "policy_audit", "jet_db_policy_audit", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.db", "decode", "jet_db_decode", true, &[true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.db", "transaction", "jet_db_scope_transaction", false, &[true, true, true], ) .without_direct_aot() .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::new( "core.db", "migrate", "jet_db_scope_migrate", false, &[true, true, true], ) .without_direct_aot() .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .with_native_int_result(),
    CoreCallRecord::source_owned("core.term", "read_key", &[]),
    CoreCallRecord::new("core.term", "__terminal8_fault", "jet_terminal8_fault", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__terminal8_line", "jet_terminal8_line", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__terminal8_write_stdout", "jet_terminal8_write_stdout", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__terminal8_write_stderr", "jet_terminal8_write_stderr", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__terminal8_env", "jet_terminal8_env", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__terminal8_env_has", "jet_terminal8_env_has", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__terminal8_stty", "jet_terminal8_stty", true, &[]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__terminal8_color_mode", "jet_terminal8_color_mode", true, &[]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__terminal8_stdin_tty", "jet_term_stdin_is_terminal", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__terminal8_stdout_tty", "jet_term_stdout_is_terminal", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__terminal8_stderr_tty", "jet_term_stderr_is_terminal", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__terminal8_machine_output", "jet_term_machine_output", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__terminal8_mode_enter", "jet_term_mode_enter", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__terminal8_mode_leave", "jet_term_mode_leave", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__terminal8_read_all", "jet_terminal8_read_all", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__terminal8_read_until", "jet_terminal8_read_until", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__terminal8_take", "jet_terminal8_take", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.term", "__terminal8_key_bytes", "jet_terminal8_key_bytes", true, &[]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.perf", "__read_bits", "jet_perf_read_bits", false, &[]) .with_native_int_result() .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.perf", "__write_bits", "jet_perf_write_bits", false, &[false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.ui", "null_backend", &[]),
    CoreCallRecord::new("core.ui", "__null_backend", "jet_ui_null", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.ui", "tui_backend", &[]),
    CoreCallRecord::new("core.ui", "__tui_backend", "jet_ui_tui", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.ui", "gtk_backend", &[]),
    CoreCallRecord::new("core.ui", "__gtk_backend", "jet_ui_gtk", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui", "preview", "jet_ui_preview_with_viewport", true, &[true, false, false], ) .with_max_arity(3) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui", "playground", "jet_ui_playground_with_viewport", true, &[true, false, false], ) .with_max_arity(3) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "previews", "jet_ui_previews", true, &[true]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "playgrounds", "jet_ui_playgrounds", true, &[true]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "button", "jet_ui_button", true, &[true]) .with_max_arity(4) .with_pure_route(CoreCallPureRoute::Ui) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.font", "system", "jet_font_system", true, &[false]) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.font", "shape", &[true, true]).with_ui_capabilities(&["UI.FontShaping"]),
    CoreCallRecord::new("core.font", "__shape_text", "jet_font_shape_raw", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_ui_capabilities(&["UI.FontShaping"]),
    CoreCallRecord::new("core.ui", "text_input", "jet_ui_text_input", true, &[true, false]) .with_max_arity(3) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "reactive_render", "jet_ui_reactive_render", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.tui", "capabilities", &[]),
    CoreCallRecord::new("core.tui", "__cap_env_bytes", "jet_tui_cap_env_bytes", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.ui.host", "capabilities", &[]),
    CoreCallRecord::new("core.ui.host", "__capabilities", "jet_ui_host_capabilities", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.ui.host", "file_filter", "jet_ui_host_file_filter", true, &[true, true, true], ) .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.ui.host", "fs_grant", &[true, false]),
    CoreCallRecord::source_owned("core.ui.host", "open_file", &[false]).with_ui_capabilities(&["UI.FileDialog"]),
    CoreCallRecord::new("core.ui.host", "__open_file", "jet_ui_host_open_file", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_ui_capabilities(&["UI.FileDialog"]),
    CoreCallRecord::source_owned("core.ui.host", "save_file", &[false]).with_ui_capabilities(&["UI.FileDialog"]),
    CoreCallRecord::new("core.ui.host", "__save_file", "jet_ui_host_save_file", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_ui_capabilities(&["UI.FileDialog"]),
    CoreCallRecord::source_owned("core.ui.host", "shortcut", &[true, false]).with_ui_capabilities(&["UI.Shortcuts"]),
    CoreCallRecord::source_owned("core.ui.host", "accessibility", &[true, true]).with_ui_capabilities(&["UI.Accessibility"]),
    CoreCallRecord::source_owned("core.ui.host.clipboard", "read_text", &[]).with_ui_capabilities(&["UI.Clipboard"]),
    CoreCallRecord::new("core.ui.host.clipboard", "__read_text", "jet_ui_host_clipboard_read_text", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_ui_capabilities(&["UI.Clipboard"]),
    CoreCallRecord::source_owned("core.ui.host.clipboard", "write_text", &[true]).with_ui_capabilities(&["UI.Clipboard"]),
    CoreCallRecord::new("core.ui.host.clipboard", "__write_text", "jet_ui_host_clipboard_write_text", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_ui_capabilities(&["UI.Clipboard"]),
    CoreCallRecord::source_owned("core.ui.host.ime", "poll", &[]).with_ui_capabilities(&["UI.IME"]),
    CoreCallRecord::new("core.ui.host.ime", "__poll", "jet_ui_host_ime_poll", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_ui_capabilities(&["UI.IME"]),
    CoreCallRecord::source_owned("core.ui.host.drag_drop", "poll", &[]).with_ui_capabilities(&["UI.DragDrop"]),
    CoreCallRecord::new("core.ui.host.drag_drop", "__poll", "jet_ui_host_drag_poll", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_ui_capabilities(&["UI.DragDrop"]),
    CoreCallRecord::source_owned("core.ui.host.shortcuts", "binding", &[false, true]).with_ui_capabilities(&["UI.Shortcuts"]),
    CoreCallRecord::source_owned("core.ui.host.shortcuts", "register", &[false]).with_ui_capabilities(&["UI.Shortcuts"]),
    CoreCallRecord::new("core.ui.host.shortcuts", "__register", "jet_ui_host_shortcuts_register", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_ui_capabilities(&["UI.Shortcuts"]),
    CoreCallRecord::source_owned("core.ui.host.shortcuts", "dispatch", &[false]).with_ui_capabilities(&["UI.Shortcuts"]),
    CoreCallRecord::new("core.ui.host.shortcuts", "__dispatch", "jet_ui_host_shortcuts_dispatch", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_ui_capabilities(&["UI.Shortcuts"]),
    CoreCallRecord::source_owned("core.ui.host.accessibility", "attach", &[false, false]).with_ui_capabilities(&["UI.Accessibility"]),
    CoreCallRecord::new("core.ui.host.accessibility", "__attach", "jet_ui_host_attach_accessibility", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_ui_capabilities(&["UI.Accessibility"]),
    CoreCallRecord::source_owned("core.ui.host.accessibility", "project", &[false, false]).with_ui_capabilities(&["UI.Accessibility"]),
    CoreCallRecord::new("core.ui.host.accessibility", "__project", "jet_ui_host_project_accessibility", true, &[false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_ui_capabilities(&["UI.Accessibility"]),
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
    CoreCallRecord::new("core.web.browser", "__path_join", "jet_browser_path_join_raw", false, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.web.browser", "__read_text", "jet_browser_read_text_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.web.browser", "__is_file", "jet_browser_is_file_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.web.browser", "__file_size", "jet_browser_file_size_raw", true, &[true]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.web.browser", "__modified_ms", "jet_browser_modified_ms_raw", true, &[true]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.web.browser", "__html_path", "jet_browser_html_path_raw", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.web.browser", "__ws_set_timeout", "JetWsConn::set_timeout", false, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::TypedIntrinsic),
    CoreCallRecord::new("core.web.browser", "__from_connection", "jet_browser_from_connection_raw", true, &[false, true, false]).with_interpreter_route(CoreCallInterpreterRoute::TypedIntrinsic),
    CoreCallRecord::new("core.web.browser", "__command", "jet_browser_command", false, &[true, true, false]).with_interpreter_route(CoreCallInterpreterRoute::TypedIntrinsic),
    CoreCallRecord::new("core.web.browser", "__session_started", "jet_browser_session_started_raw", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::TypedIntrinsic),
    CoreCallRecord::new("core.web.browser", "__cdp", "jet_browser_cdp_raw", false, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::TypedIntrinsic),
    sema_web_call("core.web.browser", "__begin_fixture", "jet_browser_test_begin_named", &[true, true, true, false, true, false, false]),
    sema_web_call("core.web.browser", "__report_options", "jet_browser_report_options_raw", &[true]),
    sema_web_call("core.web.browser", "__report_json", "jet_browser_test_report_json", &[true]),
    sema_web_call("core.web.browser", "__report_html", "jet_browser_test_report_html", &[true]),
    sema_web_call("core.web.browser", "__report_text", "jet_browser_test_report_text", &[true]),
    sema_web_call("core.web.browser", "__write_report_bytes", "jet_browser_report_write_bytes_raw", &[true, true]),
    sema_web_call("core.web.browser", "__server_start", "jet_browser_test_server_start", &[true]),
    sema_web_call("core.web.browser", "__server_stop", "jet_browser_test_server_stop", &[true]),
    sema_web_call("core.web", "__openapi_inputs", "jet_openapi_router_inputs", &[true]),
    sema_web_call("core.web", "__openapi_render", "jet_web_openapi_from_routes", &[false, false, false, false]),
    CoreCallRecord::new("core.web", "__contract_stop", "jet_openapi_contract_stop_raw", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::TypedIntrinsic),
    sema_web_call("core.web", "__live_stats_snapshot", "jet_live_stats_snapshot_raw", &[]).with_native_int_result(),
    sema_web_call("core.web.store", "__transact", "jet_web_store_transaction", &[true, false, false, false]),
    sema_web_call("core.web.store", "__facts_snapshot", "jet_store_facts_snapshot_raw", &[true]).with_native_int_result(),
    sema_web_call("core.web.table", "__columns", "jet_table_columns_raw", &[true]),
    sema_web_call("core.web.table", "__replace_columns", "jet_table_replace_columns_raw", &[true, false]),
    sema_web_call("core.web.table", "__state", "jet_web_table_state", &[true]),
    sema_web_call("core.web.table", "__set_state", "jet_table_set_state_raw", &[true, false]),
    sema_web_call("core.web.table", "__alias", "jet_table_alias_raw", &[true]),
    sema_web_call("core.web.table", "__has_loader", "jet_table_has_loader_raw", &[true]),
    sema_web_call("core.web.table", "__invoke_loader", "jet_table_invoke_loader_raw", &[true, false]),
    sema_web_call("core.web.table", "__generation", "jet_table_generation_raw", &[true, false]),
    sema_web_call("core.web.table", "__query_facts", "jet_table_query_facts_raw", &[true]).with_native_int_result(),
    sema_web_call("core.web.table", "__state_equal", "jet_table_state_equal_raw", &[true, true]),
    sema_web_call("core.web.table", "__page_facts", "jet_table_page_facts_raw", &[true]).with_native_int_result(),
    sema_web_call("core.web.table", "__commit_keys", "jet_table_commit_keys_raw", &[true, true, false]),
    sema_web_call("core.web.table", "__loading_store", "jet_table_loading_store_raw", &[true, false]),
    sema_web_call("core.web.table", "__error_store", "jet_table_error_store_raw", &[true, false]),
    sema_web_call("core.web.table", "__page_store", "jet_table_page_store_raw", &[true, false]),
    sema_web_call("core.web.table", "__state_store", "jet_table_state_store_raw", &[true, false]),
    sema_web_call("core.web.table", "__has_key", "jet_table_has_key_raw", &[true]),
    sema_web_call("core.web.table", "__page_accessor_keys", "jet_table_page_accessor_keys_raw", &[true, false]),
    sema_web_call("core.web.table", "__page_keys", "jet_table_page_keys_raw", &[true]),
    sema_web_call("core.web.table", "__page_replace_keys", "jet_table_page_replace_keys_raw", &[false, false]),
    sema_web_call("core.web.table", "__column_info", "jet_table_column_info_raw", &[true]),
    sema_web_call("core.web.table", "__cell", "jet_table_cell_raw", &[true, true]),
    sema_web_call("core.web.table", "__rows", "jet_table_rows_raw", &[true]),
    sema_web_call("core.web.table", "__row_key", "jet_table_row_key_raw", &[true, true]),
    sema_web_call("core.web.table", "__make_page", "jet_table_make_page_raw", &[false, false, false, false, false, false]),
    CoreCallRecord::new("core.devtools", "__publish_payload", "jet_devtools_publish_payload", false, &[false, false, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::TypedIntrinsic),
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
    CoreCallRecord::source_owned("core.web.browser", "profile", &[true]),
    CoreCallRecord::new( "core.web.browser", "timeout", "jet_browser_timeout", false, &[false], ),
    CoreCallRecord::new( "core.web.browser", "locked", "jet_browser_locked", false, &[true], ),
    CoreCallRecord::new( "core.web.browser", "connect", "jet_browser_connect", false, &[true], ),
    CoreCallRecord::source_owned("core.web.browser", "connect_profile", &[true, true, false]),
    CoreCallRecord::new( "core.http.server", "serve", "jet_http_mux_serve", false, &[true, false, false, false], ) .with_max_arity(4) .without_direct_aot() .with_source_owned() .with_source_target("__serve_entry"),
    CoreCallRecord::source_owned("core.http.server", "serve_once", &[true, false]),
    CoreCallRecord::new("core.http.server", "__serve_once", "jet_http_mux_serve_once", false, &[true, false]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.http.server", "serve_once_listener", &[true, true]),
    CoreCallRecord::new("core.http.server", "__serve_once_listener", "jet_http_mux_serve_once_listener", false, &[true, true]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.http.server", "static_file", &[true, true]),
    CoreCallRecord::new("core.http.server", "__static_file", "jet_http_srv_static_file", false, &[true, true]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.http.server", "static_file_range", &[true, true, true]),
    CoreCallRecord::new("core.http.server", "__static_file_range", "jet_http_srv_static_file_range", false, &[true, true, true]).without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.http.server", "static_files", "jet_http_srv_static_files_mount_defaulted", false, &[true, true, true], ) .with_max_arity(6) .without_direct_aot() .with_source_owned() .with_source_target("__static_files_entry"),
    CoreCallRecord::source_owned("core.http.server", "mux", &[]),
    CoreCallRecord::new("core.http.server", "__mux", "jet_http_mux_new", false, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.http.server", "__response_parts", "jet_http_srv_raw_response_parts", true, &[false, false, false, false, false, false, false, false, false, false, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.http.server", "cors", &[true, true]),
    CoreCallRecord::new("core.http.server", "__cors", "jet_http_srv_install_cors", false, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.http.server", "__route_template", "jet_http_srv_raw_route_template", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.http.server", "request_id", &[true]),
    CoreCallRecord::new("core.http.server", "__request_id", "jet_http_srv_install_request_id", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::receiver_with_symbol(&["HTTPMux"], "middleware", "jet_http_mux_middleware", false, &[true, false]) .with_source_owned() .with_source_target("core.http.server.__middleware_entry"),
    CoreCallRecord::source_owned("core.net.ws", "connect", &[true]),
    CoreCallRecord::new("core.net.ws", "__connect", "jet_ws_connect", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.net.ws", "upgrade", &[true]),
    CoreCallRecord::new("core.net.ws", "__upgrade", "jet_ws_upgrade", false, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new( "core.time", "new", "JetDate::new", false, &[false, false, false], ) .with_pure_route(CoreCallPureRoute::Date) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot(),
    CoreCallRecord::new( "core.time", "from_timestamp", "JetDateTime::from_timestamp", false, &[false], ) .with_pure_route(CoreCallPureRoute::DateTime) .without_direct_aot(),
    CoreCallRecord::new( "core.time", "parse_time", "JetLocalTime::parse", false, &[true], ) .with_pure_route(CoreCallPureRoute::Time) .without_direct_aot(),
    CoreCallRecord::new("core.time", "parse", "JetDate::parse", false, &[true]) .with_pure_route(CoreCallPureRoute::Date) .without_direct_aot(),
    CoreCallRecord::source_owned("core.encoding.xml", "decode", &[true, true]).with_max_arity(2),
    CoreCallRecord::receiver_with_coverage( &["WebTable"], "with_column", &[false], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA), ),
    CoreCallRecord::receiver_with_coverage( &["WebTable"], "paginate", &[true, true], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA), ),
    CoreCallRecord::receiver_with_coverage( &["WebTable"], "page", &[], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA), ),
    CoreCallRecord::receiver_with_coverage( &["WebStore"], "set", &[false], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA), ),
    CoreCallRecord::receiver_with_coverage( &["WebStore"], "facts_json", &[], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA), ),
    CoreCallRecord::receiver_with_symbol( &[INTERNAL_RECEIPT_HANDLE], METHOD_RECEIPT_ATTACH, "jet_receipt_attach", true, &[true, false, false, false], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "line", "jet_data_plot_line", true, &[true], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "bar", "jet_data_plot_bar", true, &[true], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "point", "jet_data_plot_point", true, &[true], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "x", "jet_data_plot_x", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "y", "jet_data_plot_y", true, &[true, false, false], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "color", "jet_data_plot_color", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "size", "jet_data_plot_size", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "text_channel", "jet_data_plot_text_channel", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "detail", "jet_data_plot_detail", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "with_transform", "jet_data_plot_with_transform", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "with_scale", "jet_data_plot_with_scale", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "with_axis", "jet_data_plot_with_axis", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "with_legend", "jet_data_plot_with_legend", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "facet", "jet_data_plot_facet", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "with_layer", "jet_data_plot_with_layer", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "with_interaction", "jet_data_plot_with_interaction", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "accessibility", "jet_data_plot_accessibility", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "layout", "jet_data_plot_layout", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["JetDataPlot"], "select_indices", "jet_data_plot_select_indices", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &[crate::Syntax::INTERNAL_LIST_QUERY_HANDLE], "query", "jet_data_query_sql", true, &[true, true], ),
    CoreCallRecord::receiver_with_symbol( &["Query"], "filter", "jet_data_query_filter", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["Query"], "sort_by", "jet_data_query_sort_by", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["Query"], "map", "jet_data_query_map", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["Query"], "min", "jet_data_query_min", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["Query"], "max", "jet_data_query_max", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["Query"], "inner_join", "jet_data_query_inner_join", true, &[true, true, false, false], ),
    CoreCallRecord::receiver_with_symbol( &["Query"], "left_join", "jet_data_query_left_join", true, &[true, true, false, false], ),
    CoreCallRecord::receiver_with_symbol( &["Query"], "collect", "jet_data_query_collect", true, &[true], ),
    CoreCallRecord::receiver_with_symbol( &["Query"], "plan", "jet_data_query_plan", true, &[true], ),
    CoreCallRecord::receiver_with_symbol( &["Query"], "group_by", "jet_data_query_group_by", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["DataTracked"], "query", "jet_data_query_tracked", true, &[true], ),
    CoreCallRecord::receiver_with_symbol( &["DataTracked"], "insert", "jet_data_track_insert", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["DataTracked"], "replace", "jet_data_track_replace", true, &[true, false, false], ),
    CoreCallRecord::receiver_with_symbol( &["DataTracked"], "remove", "jet_data_track_remove", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["Query"], "watch", "jet_data_query_watch", true, &[true], ),
    CoreCallRecord::receiver_with_symbol( &["DataWatch"], "get", "jet_data_watch_get", true, &[true], ),
    CoreCallRecord::receiver_with_symbol( &["DataWatch"], "status", "jet_data_watch_status", true, &[true], ),
    CoreCallRecord::receiver_with_symbol( &["DataWatch"], "cancel", "jet_data_watch_cancel", true, &[true], ),
    CoreCallRecord::receiver_with_symbol( &["DataGroupedQuery"], "count", "jet_data_group_count_query", true, &[true], ),
    CoreCallRecord::receiver_with_symbol( &["DataGroupedQuery"], "sum", "jet_data_group_sum_query", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol( &["DataGroupedQuery"], "mean", "jet_data_group_mean_query", true, &[true, false], ),
    CoreCallRecord::receiver_with_symbol(&["RNG"], "int", "jet_rng_int", true, &[true, false, false]),
    CoreCallRecord::receiver(&["ProcessStdin"], "close", &[]),
    CoreCallRecord::receiver_with_symbol( &["FileScope"], "read", "jet_std_fs_scope_read", true, &[true, true], ),
    CoreCallRecord::receiver_with_coverage(&["RealtimeStream"], "next_deadline", &[], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA)),
    CoreCallRecord::receiver_with_coverage(&["RealtimeStream"], "receipt", &[], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA)),
    CoreCallRecord::receiver_with_coverage(&["RealtimeStream"], "cancel", &[], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA)),
    CoreCallRecord::receiver_with_coverage(&["RealtimeStream"], "is_cancelled", &[], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA)),
    CoreCallRecord::receiver_with_coverage(&["Condition"], "notify_one", &[], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA)),
    CoreCallRecord::receiver_with_coverage(&["Condition"], "notify_all", &[], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA)),
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
    CoreCallRecord::receiver_with_coverage(&["Solver"], "new", &[false], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA)),
    CoreCallRecord::receiver_with_coverage(&["Solver"], "require", &[false], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA)),
    CoreCallRecord::receiver_with_coverage(&["Solver"], "failure_count", &[], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA)),
    CoreCallRecord::receiver_with_coverage(&["Solver"], "status", &[], CoreCallCoverage::from_bits(CoreCallCoverage::SEMA)),
    CoreCallRecord::receiver( &[ "Solver", "ServiceUpgradeReceipt", "DataError", VAULT_KEY_REF_TYPE, ], "__display", &[], ),
    CoreCallRecord::new( "core.http.server", "bind", "jet_http_server_bind", false, &[true, false, false, false], ) .with_max_arity(4) .with_interpreter_route(CoreCallInterpreterRoute::Ambient) .without_direct_aot() .with_source_owned() .with_source_target("__bind_entry"),
    CoreCallRecord::new( "core.data", "inner_join", "jet_data_inner_join_checked_default", true, &[true, true, false, false], ) .without_direct_aot().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.data.arrow", "import", "jet_data_arrow_import", true, &[false]),
    CoreCallRecord::new("core.data.arrow", "query", "jet_data_query_arrow", true, &[false]),
    CoreCallRecord::new("core.data", "mean", "jet_data_mean_checked", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.encoding.xml", "to_bytes", &[true, true]),
    CoreCallRecord::source_owned("core.service", "workflow_start", &[true, false, false]),
    CoreCallRecord::new("core.sync", "text_edit", "jet_sync_text_edit", true, &[true, false, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.service", "workflow_outcome", &[true, false]),
    CoreCallRecord::new("core.sync", "list_push", "jet_sync_list_push", true, &[false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.service", "workflow_history", &[true, false]),
    CoreCallRecord::new("core.sync", "policy_new", "jet_db_policy_new", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    sema_web_call("core.web", "on", "jet_web_on", &[true, true, false]),
    CoreCallRecord::new("core.time", "__wall_sample", "jet_time_raw_wall_sample", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.time", "__world_wall_ms", "jet_time_raw_world_wall_ms", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.time", "__wall_override_ms", "jet_time_raw_wall_override_ms", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.time", "__controlled_monotonic_ns", "jet_time_raw_controlled_monotonic_ns", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.time", "__world_monotonic_ns", "jet_time_raw_world_monotonic_ns", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.time", "__host_monotonic_ns", "jet_time_raw_host_monotonic_ns", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.time", "__clock_mono_ns", "jet_time_raw_host_monotonic_ns", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient).with_pure_route(CoreCallPureRoute::Time),
    CoreCallRecord::new("core.time", "__sleep_ns", "jet_std_time_sleep_duration_ns", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.time", "__reject_uncontrolled", "jet_scheduler_world_reject_uncontrolled", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.time", "__zone_env", "jet_time_raw_zone_env", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.time", "__zone_file_bytes", "jet_time_raw_zone_file_bytes", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.time", "__embedded_zone_bytes", "jet_time_raw_embedded_zone_bytes", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "__node_transfer", "jet_ui_raw_node_transfer", true, &[true, false, false, false, false, false, false, true, false, false, false, false, false, false, false]).with_pure_route(CoreCallPureRoute::Ui).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.ui", "__viewport_transfer", "jet_ui_raw_viewport_transfer", true, &[false, false, false, false]).with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.ui.host", "__request_transfer", "jet_ui_host_raw_request_transfer", true, &[false, true, true, false, false, false]).with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.ui", "__accessibility_transfer", "jet_ui_raw_accessibility_transfer", true, &[false, false, false]).with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.ui", "__shortcut_transfer", "jet_ui_raw_shortcut_transfer", true, &[false, false, false]).with_pure_route(CoreCallPureRoute::Ui),
    CoreCallRecord::new("core.net", "__ip_from_bytes", "jet_net_raw_ip_from_bytes", true, &[false]).with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "__ip_bytes", "jet_net_raw_ip_bytes", true, &[true]).with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "__ip_is_v4", "jet_net_raw_ip_is_v4", true, &[true]).with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "__socket_from_parts", "jet_net_raw_socket_from_parts", true, &[false, false, false, false]).with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "__socket_ip", "jet_net_raw_socket_ip", true, &[true]).with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.net", "__socket_port", "jet_net_raw_socket_port", true, &[true]).with_pure_route(CoreCallPureRoute::Net).with_native_int_result(),
    CoreCallRecord::new("core.net", "__socket_scope", "jet_net_raw_socket_scope", true, &[true]).with_pure_route(CoreCallPureRoute::Net).with_native_int_result(),
    CoreCallRecord::new("core.net", "__udp_packet_addr", "jet_net_raw_udp_packet_addr", true, &[true]).with_pure_route(CoreCallPureRoute::Net),
    CoreCallRecord::new("core.service", "__tree_generation", "jet_services_raw_tree_generation", true, &[true]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_event_count", "jet_services_raw_tree_event_count", true, &[true]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_dead_letter_count", "jet_services_raw_tree_dead_letter_count", true, &[true]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_started", "jet_services_raw_tree_started", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_has_snapshot", "jet_services_raw_tree_has_snapshot", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_group_count", "jet_services_raw_tree_group_count", true, &[true]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_write_delivery", "jet_services_raw_tree_write_delivery", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_write_restart", "jet_services_raw_tree_write_restart", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__group_write_restart", "jet_services_raw_group_write_restart", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_clear_dead_letters", "jet_services_raw_tree_clear_dead_letters", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_write_empty_adapter", "jet_services_raw_tree_write_empty_adapter", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_clear_state_authority", "jet_services_raw_tree_clear_state_authority", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__endpoint_parts", "jet_services_raw_endpoint_parts", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__active_endpoint", "jet_services_active_execution_endpoint", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__endpoint_authority_parts", "jet_services_raw_endpoint_authority_parts", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::source_owned("core.service", "__authority_signing_key", &[true]),
    CoreCallRecord::source_owned("core.service", "__authority_validate", &[true]),
    CoreCallRecord::source_owned("core.service", "__upgrade_receipt_display", &[true]),
    CoreCallRecord::new("core.service", "__tree_write_state_adapter", "jet_services_raw_tree_write_state_adapter", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_write_state_authority", "jet_services_raw_tree_write_state_authority", true, &[true, false, false, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_state_authority", "jet_services_raw_tree_state_authority", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__state_load", "jet_services_raw_state_load", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_event_records", "jet_services_raw_tree_event_records", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_upgrade_receipt", "jet_services_raw_tree_upgrade_receipt", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__upgrade_receipt_parts", "jet_services_raw_upgrade_receipt_parts", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_display_parts", "jet_services_raw_tree_display_parts", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_replay_parts", "jet_services_raw_workflow_replay_parts", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_observation_parts", "jet_services_raw_tree_observation_parts", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__endpoint_identity_parts", "jet_services_raw_endpoint_identity_parts", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__endpoint_validation_snapshot", "jet_services_raw_endpoint_validation_snapshot", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_sync_tree", "jet_services_workflow_sync_tree", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_append", "jet_services_workflow_append", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_handle", "jet_services_workflow_handle", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_count", "jet_services_raw_workflow_count", true, &[true]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_id_at", "jet_services_raw_workflow_id_at", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_version_at", "jet_services_raw_workflow_version_at", true, &[true, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_run_at", "jet_services_raw_workflow_run_at", true, &[true, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_status_at", "jet_services_raw_workflow_status_at", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_outcome_at", "jet_services_raw_workflow_outcome_at", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_cursor_at", "jet_services_raw_workflow_cursor_at", true, &[true, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_history_len", "jet_services_raw_workflow_history_len", true, &[true, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_steps_len", "jet_services_raw_workflow_steps_len", true, &[true, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_event_at", "jet_services_raw_workflow_event_at", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_step_at", "jet_services_raw_workflow_step_at", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_activity_count", "jet_services_raw_workflow_activity_count", true, &[true, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_activity_key_at", "jet_services_raw_workflow_activity_key_at", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_activity_outcome_at", "jet_services_raw_workflow_activity_outcome_at", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__workflow_set_cursor", "jet_services_raw_workflow_set_cursor", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_allocate", "jet_services_raw_tree_allocate", true, &[false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_task_group_new", "jet_services_raw_tree_task_group_new", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_state_adapter", "jet_services_raw_tree_state_adapter", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_chaos_count", "jet_services_raw_tree_chaos_count", true, &[true]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_write_chaos_count", "jet_services_raw_tree_write_chaos_count", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__first_running_endpoint", "jet_services_raw_first_running_endpoint", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__group_parts", "jet_services_raw_group_parts", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__group_reparent", "jet_services_raw_group_reparent", true, &[true, true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__group_push", "jet_services_raw_group_push", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__event_store_append", "jet_services_raw_event_store_append", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_event_push", "jet_services_raw_tree_event_push", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__snapshot_store_write", "jet_services_raw_snapshot_store_write", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_route_parts", "jet_services_raw_tree_route_parts", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__directory_entry", "jet_services_raw_directory_entry", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__worker_route", "jet_services_raw_worker_route", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__worker_sent_account", "jet_services_raw_worker_sent_account", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__authority_try_send", "service_authority_try_send", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__validate_directory_proof", "service_authority_validate_directory", true, &[true, true, true, true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__delivery_read", "jet_services_raw_delivery_read", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__delivery_probe", "jet_services_raw_delivery_probe", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__delivery_store", "jet_services_raw_delivery_store", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__runtime_retry", "jet_services_runtime_retry", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__endpoint_mailbox", "jet_services_raw_endpoint_mailbox", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__mailbox_started", "jet_services_raw_mailbox_started", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__mailbox_draining", "jet_services_raw_mailbox_draining", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__mailbox_depth", "jet_services_raw_mailbox_depth", true, &[true]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__mailbox_receive", "jet_services_raw_mailbox_receive", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_delivery", "jet_services_raw_tree_delivery", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__mailbox_snapshot", "jet_services_raw_mailbox_snapshot", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__idempotency_records", "jet_services_raw_idempotency_records", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__worker_named_index", "jet_services_raw_worker_named_index", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__delivery_allocate", "jet_services_raw_delivery_allocate", true, &[false, false, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__delivery_identity", "jet_services_raw_delivery_identity", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__mailbox_restore", "jet_services_raw_mailbox_restore", true, &[true, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__dead_letter_push", "jet_services_raw_dead_letter_push", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__idempotency_push", "jet_services_raw_idempotency_push", true, &[true, false, true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__mailbox_try_send", "jet_services_raw_mailbox_try_send", true, &[true, false, true]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__mailbox_try_receive", "jet_services_raw_mailbox_try_receive", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__worker_wait_notify", "jet_services_raw_worker_wait_notify", true, &[true, false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__worker_receive_account", "jet_services_raw_worker_receive_account", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__delivery_id", "service_authority_id", true, &[true, true, true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__runtime_send", "jet_services_runtime_send", true, &[true, true, true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__runtime_dead_letter", "jet_services_runtime_dead_letter", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__mark_delivered", "jet_services_authority_mark_delivered", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__take_pending", "jet_services_authority_take_pending", true, &[true, false, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__requeue_pending", "jet_services_authority_requeue_pending", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__finish_drain", "jet_services_finish_drain", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__authority_issue", "service_authority_provider_issue", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_write_authority", "jet_services_raw_tree_write_authority", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__endpoint_unchecked", "service_authority_endpoint_unchecked", true, &[false, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__worker_mailbox_new", "jet_services_new_mailbox", true, &[false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__worker_task_new", "jet_services_raw_worker_task_new", true, &[false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__topology_worker_id", "jet_services_topology_worker_id", true, &[true, true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__topology_root_id", "jet_services_topology_root_id", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__task_bind_topology", "jet_services_bind_topology", true, &[true, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__worker_push", "jet_services_raw_worker_push", true, &[true, false, false, false, false, false, false, false, false, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__worker_tree_parts", "jet_services_raw_worker_tree_parts", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__authority_now", "service_authority_now", true, &[]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__task_escalate", "jet_services_task_escalate", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__task_fail", "jet_services_task_fail", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__task_restart", "jet_services_task_restart", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__authority_update", "jet_services_authority_update", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__authority_register", "service_authority_register", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__close_runtime_groups", "jet_services_close_runtime_groups", true, &[true]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__tree_write_started", "jet_services_raw_tree_write_started", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__publish_tree_topology", "jet_services_publish_tree_topology", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__worker_write_restarts", "jet_services_raw_worker_write_restarts", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__worker_write_running", "jet_services_raw_worker_write_running", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__mailbox_close", "jet_services_raw_mailbox_close", true, &[true, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__worker_mailbox_depth", "jet_services_raw_worker_mailbox_depth", true, &[true, false]).with_native_int_result().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__worker_write_mailbox", "jet_services_raw_worker_write_mailbox", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__record_topology_restart", "jet_services_record_topology_restart", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.service", "__task_write_partition", "jet_services_raw_task_write_partition", true, &[true, false, false]).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
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
    if matches!(row.symbol, CoreCallSymbol::SourceOwned) && projection != CoreCallCoverage::SEMA {
        return Err(CoreCallProjectionError::Uncovered { projection });
    }
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
        if !row.is_receiver()
            && !matches!(row.symbol, CoreCallSymbol::SourceOwned)
            && row.prelude_symbol().is_empty()
        {
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
    fn injected_clock_raw_sample_does_not_acquire_ambient_time() {
        let sample = core_call("core.time", "__clock_mono_ns")
            .expect("checked Clock raw monotonic seam");
        assert_eq!(sample.effect(), None);
        assert_eq!(sample.effect_leaf(), None);
        assert_eq!(sample.capability().kind, CoreCapabilityKind::Pure);
        assert!(sample.capability().is_well_formed());
        for method in ["now", "__host_monotonic_ns"] {
            let ambient = core_call("core.time", method)
                .expect("ambient wall/host clock row");
            assert_eq!(ambient.effect(), Some(Effect::Time), "{method}");
            assert_eq!(ambient.capability().kind, CoreCapabilityKind::NativeRequired);
        }
    }

    #[test]
    fn rng_int_returns_an_owned_exact_int_carrier() {
        let row = super::core_receiver_method("RNG", "int").expect("canonical RNG.int row");
        assert_eq!(row.symbol, super::CoreCallSymbol::Prelude("jet_rng_int"));
        assert_eq!(row.signature.borrow_mask, &[true, false, false]);
        assert!(!row.native_int_result, "the provider already tags and owns its Int result");
        assert!(core_call("core.sys", "pid").expect("native Int row").native_int_result);
    }

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

    #[test]
    fn source_targets_preserve_receiver_facts_without_publishing_an_export() {
        use super::{CoreCallRecord, CoreCallSymbol, CoreCallCoverage, CoreCallInterpreterRoute};
        let original = CoreCallRecord::receiver_with_symbol(
            &["JobQueue"], "enqueue", "jet_job_queue_enqueue", true, &[true, true, true],
        ).with_max_arity(4);
        let source = original.with_source_owned().with_source_target("core.jobs.__enqueue");
        assert_eq!(source.symbol, CoreCallSymbol::SourceOwned);
        assert_eq!(source.signature, original.signature);
        assert_eq!(source.effect(), original.effect());
        assert_eq!(source.capability(), original.capability());
        assert_eq!(source.receiver_types, original.receiver_types);
        assert_eq!(source.coverage, CoreCallCoverage::from_bits(CoreCallCoverage::SEMA));
        assert_eq!(source.interpreter_route, CoreCallInterpreterRoute::None);
        assert!(!source.aot_direct);
        assert!(source.targets_source_function("core.jobs", "__enqueue"));
        assert!(!source.targets_source_function("core.jobs", "enqueue"));
        assert!(!source.targets_source_function("core.term", "__enqueue"));
        assert!(!original.with_source_target("core.jobs.__enqueue").targets_source_function("core.jobs", "__enqueue"));
        let plain = CoreCallRecord::source_owned("core.term", "take", &[true, false])
            .with_source_target("__take_bytes");
        assert!(plain.targets_source_function("core.term", "__take_bytes"));
        assert!(!plain.targets_source_function("core.term", "take"));
    }

    #[test]
    fn process_stdin_close_registers_the_existing_provider_leaf() {
        use super::{core_receiver_method, CoreCallRecord, CoreCallSymbol};
        let row = core_receiver_method("ProcessStdin", "close")
            .expect("canonical ProcessStdin.close row");
        let original = CoreCallRecord::receiver(&["ProcessStdin"], "close", &[]);
        assert_eq!(row.symbol, CoreCallSymbol::Prelude("jet_process_stdin_close"));
        assert_eq!(row.signature, original.signature);
        assert_eq!(row.effect(), original.effect());
        assert_eq!(row.capability(), original.capability());
        assert_eq!(row.coverage, original.coverage);
        assert_eq!(row.interpreter_route, original.interpreter_route);
        assert_eq!(row.aot_direct, original.aot_direct);
        assert_eq!(row.receiver_types, original.receiver_types);
        assert!(row.source_target.is_none());
    }

}
