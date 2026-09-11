// core.plugin runtime (D-DEP-WASM1=A, c81) — sandboxed WASM Component Model
// plugin loader, via wasmtime's dynamic component API.
//
// This file is emitted verbatim into the hidden FFI bridge crate (see
// Source/FFI.rs) when a Jet program uses `core.plugin`. The compiler crate
// (`Source/`) never depends on `wasmtime`; it only ships this text. Owner-
// approved I6 bootstrap exception (D-DEP-WASM1=A): wasmtime + the Component
// Model is the plugin sandbox engine, runtime-side only.
//
// Host import admission is checked against typed HostImportFacts and the one
// canonical Authority lent at load time. Every file read uses a
// resource-scoped, no-follow FS.Read root; unregistered imports are denied
// before the guest can call them. A plugin with no declared/imported
// capability still runs with a zero-grant scope (I1).
//
// Handles are u64 keys into a thread-local HashMap, mirroring `DB.rs`. Handle
// 0 is the error sentinel (never a live plugin instance).
//
// Wire protocol (mirrors `DB.rs`'s tagged-length encoding for leaves and uses
// canonical recursive component values for aggregates):
// `I<len>:<int>`, `F<len>:<float>`, `B<len>:<true|false>`, `T<len>:<utf8>`,
// `L<count>:<values>`, `R<count>:<field-name><value>`, `Q<count>:<values>`,
// `N`/`P<value>` for none/some, `K`/`X<value>` for ok/error, `V<case><value>`
// for variants, `E<len>:<case>` for enums, and `G<count>:<flag-names>` for
// flags. A call result or error is `O:<value>` / `E:<message>`. The wire is
// only a transport envelope; values are checked against the actual Component
// Model `Type` before every call and after every return.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Component as PathComponent, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use wasmtime::component::{
    types::{ComponentFunc, ComponentInstance, ComponentItem},
    Component, Linker, LinkerInstance, Type, Val,
};
use wasmtime::{Config, Engine, Store, StoreContextMut, StoreLimits, StoreLimitsBuilder, Trap};
use jet_foundation::PluginWire::{
    plugin_decode_params, plugin_encode_value, PluginParamDecodeError, PluginValue,
};
use jet_foundation::Authority::{
    Authority as CanonicalAuthority, HostImportDecision, HostImportFact, Verdict,
};
const PLUGIN_MAX_FUEL: u64 = 10_000_000;
const PLUGIN_MAX_MEMORY_BYTES: usize = 16 * 1024 * 1024;
const PLUGIN_MAX_TABLE_ELEMENTS: u32 = 10_000;
const PLUGIN_MAX_WIRE_BYTES: usize = 16 * 1024 * 1024;
const PLUGIN_TIMEOUT_MS: u64 = 2_000;
struct PluginHostState {
    limits: StoreLimits,
    /// The one canonical grant lent to this guest.  Host imports consult this
    /// value; they do not parse a second wire policy.
    authority: CanonicalAuthority,
    /// The typed load decision inherited by all exports of this instance.
    authority_decision: HostImportDecision,
    /// Component import name to typed fact, populated before instantiation.
    imports: BTreeMap<String, HostImportFact>,
}

struct PluginInstance {
    engine: Engine,
    store: Store<PluginHostState>,
    instance: wasmtime::component::Instance,
    authority: CanonicalAuthority,
    authority_decision: HostImportDecision,
    imports: BTreeMap<String, HostImportFact>,
}

thread_local! {
    static PLUGINS: RefCell<HashMap<u64, PluginInstance>> = RefCell::new(HashMap::new());
}

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

fn plugin_engine() -> Result<Engine, String> {
    let mut config = Config::new();
    config.consume_fuel(true);
    config.epoch_interruption(true);
    Engine::new(&config).map_err(|error| format!("plugin engine: {error}"))
}

fn plugin_limits() -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(PLUGIN_MAX_MEMORY_BYTES)
        .table_elements(PLUGIN_MAX_TABLE_ELEMENTS as _)
        .instances(1)
        .memories(1)
        .tables(1)
        .build()
}

fn plugin_relative_candidate(
    path: &Path,
    root_text: &str,
) -> Result<Option<PathBuf>, String> {
    let explicit_root = root_text != "repo";
    if path.is_absolute() {
        if !explicit_root {
            return Ok(None);
        }
        let root = Path::new(root_text);
        if !root.is_absolute() {
            return Err(
                "an absolute plugin path requires an absolute explicit FS.Read root".to_string(),
            );
        }
        let Some(relative) = path.strip_prefix(root).ok() else {
            return Ok(None);
        };
        if relative.as_os_str().is_empty() {
            return Err("plugin path names the FS.Read root, not a file".to_string());
        }
        if relative.components().any(|component| {
            matches!(
                component,
                PathComponent::ParentDir | PathComponent::RootDir | PathComponent::Prefix(_)
            )
        }) {
            return Err("plugin path escapes its explicit FS.Read root".to_string());
        }
        return Ok(Some(relative.to_path_buf()));
    }
    if path.components().any(|component| {
        matches!(
            component,
            PathComponent::ParentDir | PathComponent::RootDir | PathComponent::Prefix(_)
        )
    }) {
        return Err("plugin path must be relative and must not contain `..`".to_string());
    }
    Ok(Some(path.to_path_buf()))
}

fn plugin_authority_from_wire(wire: &str) -> Result<CanonicalAuthority, String> {
    let mut rights = Vec::new();
    for raw in wire.split('\n') {
        let right = raw.trim();
        if right.is_empty() {
            continue;
        }
        if jet_foundation::Authority::parse_right(right).is_none() {
            return Err(format!("plugin authority contains an unknown right `{right}`"));
        }
        rights.push(right.to_string());
    }
    Ok(CanonicalAuthority::from_rights(rights))
}
/// The package declaration is emitted by the hidden bridge beside this
/// runtime. It is a checked fact, not a second authority grant.
fn plugin_declared_authority_needs() -> Result<Vec<String>, String> {
    crate::jet_plugin_declared_authority_needs()
}

fn plugin_load_import_fact() -> HostImportFact {
    jet_foundation::MIR::plugin_authority_import_fact()
}

fn plugin_import_right(path: &str) -> Option<&'static str> {
    let leaf = path
        .split(|character: char| matches!(character, ':' | '/' | '.' | '@' | '-'))
        .rev()
        .find(|part| !part.is_empty())
        .unwrap_or(path)
        .to_ascii_lowercase();
    match leaf.as_str() {
        "read" | "read_bytes" | "readbytes" => Some("FS.Read"),
        "write" | "write_bytes" | "writebytes" => Some("FS.Write"),
        "connect" | "request" => Some("Net.Connect"),
        "run" | "exec" => Some("Exec"),
        _ => None,
    }
}

fn plugin_import_fact(path: &str, function: &ComponentFunc) -> Result<HostImportFact, String> {
    let Some(required_right) = plugin_import_right(path) else {
        return Err(format!(
            "plugin import `{path}` has no declared authority fact"
        ));
    };
    let parameters = function
        .params()
        .map(|ty| plugin_type_name(&ty))
        .collect::<Vec<_>>();
    let results = function.results().collect::<Vec<_>>();
    if results.len() > 1 {
        return Err(format!(
            "plugin import `{path}` has {} results; host imports require one result or none",
            results.len()
        ));
    }
    Ok(HostImportFact::new(
        path.to_string(),
        path.to_string(),
        required_right,
        parameters,
        results.first().map(plugin_type_name),
    ))
}

fn plugin_scoped_candidates(
    path: &str,
    authority: &CanonicalAuthority,
    required_right: &str,
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let path_ref = Path::new(path);
    let prefix = format!("{required_right}:");
    let mut saw_root = false;
    let mut candidates = Vec::new();
    for right in authority.rights() {
        let Some(root_text) = right.strip_prefix(&prefix) else {
            continue;
        };
        saw_root = true;
        if root_text.is_empty() {
            return Err(format!(
                "plugin {required_right} authority names an empty root"
            ));
        }
        let root = if root_text == "repo" {
            PathBuf::from(".")
        } else {
            PathBuf::from(root_text)
        };
        let Some(relative) = plugin_relative_candidate(path_ref, root_text)? else {
            continue;
        };
        candidates.push((root, relative));
    }
    if !saw_root {
        return Err(format!(
            "plugin operation requires an explicit resource-scoped {required_right} authority"
        ));
    }
    if candidates.is_empty() {
        return Err(format!(
            "path `{path}` is outside the granted {required_right} roots"
        ));
    }
    Ok(candidates)
}

fn plugin_bytes(value: &Val) -> Option<Vec<u8>> {
    match value {
        Val::String(text) => Some(text.as_bytes().to_vec()),
        Val::List(values) => values
            .iter()
            .map(|value| match value {
                Val::U8(byte) => Some(*byte),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn plugin_set_bytes_result(
    import: &HostImportFact,
    results: &mut [Val],
    bytes: Vec<u8>,
) -> Result<(), String> {
    if results.len() != 1 {
        return Err(format!(
            "plugin host import `{}` must return Text or list<u8>",
            import.operation
        ));
    }
    match import.result_type_id.as_deref() {
        Some("Text") => {
            let text = String::from_utf8(bytes).map_err(|error| {
                format!(
                    "plugin host import `{}` returned non-UTF-8 data for Text: {error}",
                    import.operation
                )
            })?;
            results[0] = Val::String(text);
            Ok(())
        }
        Some("list") => {
            results[0] = Val::List(bytes.into_iter().map(Val::U8).collect());
            Ok(())
        }
        Some(result_type) => Err(format!(
            "plugin host import `{}` has unsupported byte result `{result_type}`",
            import.operation
        )),
        None => Err(format!(
            "plugin host import `{}` must return Text or list<u8>",
            import.operation
        )),
    }
}
fn plugin_io_error_value(
    operation: &str,
    resource: Option<&str>,
    error: &std::io::Error,
) -> Val {
    let variant = match error.kind() {
        std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => "invalid-input",
        std::io::ErrorKind::NotFound => "not-found",
        std::io::ErrorKind::PermissionDenied => "permission-denied",
        std::io::ErrorKind::TimedOut => "timed-out",
        std::io::ErrorKind::NotConnected | std::io::ErrorKind::BrokenPipe => "closed",
        _ => "other",
    };
    let context = Val::Record(vec![
        (
            "operation".to_string(),
            Val::Enum(operation.to_string()),
        ),
        (
            "resource".to_string(),
            Val::Option(resource.map(|value| Box::new(Val::String(value.to_string())))),
        ),
        (
            "os-code".to_string(),
            Val::Option(
                error
                    .raw_os_error()
                    .map(|value| Box::new(Val::S64(i64::from(value)))),
            ),
        ),
        (
            "cause".to_string(),
            Val::Option(Some(Box::new(Val::String(error.to_string())))),
        ),
    ]);
    Val::Variant(variant.to_string(), Some(Box::new(context)))
}

fn plugin_require_io_result(
    import: &HostImportFact,
    results: &[Val],
) -> Result<(), String> {
    if results.len() != 1 || import.result_type_id.as_deref() != Some("result") {
        return Err(format!(
            "plugin host import `{}` must return the declared typed result",
            import.operation
        ));
    }
    Ok(())
}

fn plugin_set_io_result_error(
    import: &HostImportFact,
    results: &mut [Val],
    operation: &str,
    resource: Option<&str>,
    error: &std::io::Error,
) -> Result<(), String> {
    plugin_require_io_result(import, results)?;
    results[0] = Val::Result(Err(Some(Box::new(plugin_io_error_value(
        operation, resource, error,
    )))));
    Ok(())
}

fn plugin_set_io_text_result(
    import: &HostImportFact,
    results: &mut [Val],
    bytes: Vec<u8>,
    resource: Option<&str>,
) -> Result<(), String> {
    plugin_require_io_result(import, results)?;
    match String::from_utf8(bytes) {
        Ok(text) => {
            results[0] = Val::Result(Ok(Some(Box::new(Val::String(text)))));
        }
        Err(error) => {
            let error = std::io::Error::new(std::io::ErrorKind::InvalidData, error);
            results[0] = Val::Result(Err(Some(Box::new(plugin_io_error_value(
                "read", resource, &error,
            )))));
        }
    }
    Ok(())
}

fn plugin_set_io_unit_result(
    import: &HostImportFact,
    results: &mut [Val],
) -> Result<(), String> {
    plugin_require_io_result(import, results)?;
    results[0] = Val::Result(Ok(None));
    Ok(())
}

fn plugin_host_read(
    import: &HostImportFact,
    params: &[Val],
    results: &mut [Val],
    authority: &CanonicalAuthority,
) -> Result<(), String> {
    if params.len() != 1 {
        return Err(format!(
            "plugin host import `{}` expects one path parameter",
            import.operation
        ));
    }
    let Some(Val::String(path)) = params.first() else {
        return Err(format!(
            "plugin host import `{}` requires a Text path",
            import.operation
        ));
    };
    let candidates = plugin_scoped_candidates(path, authority, "FS.Read").map_err(|error| {
        format!(
            "plugin host import `{}` rejected path `{path}`: {error}",
            import.operation
        )
    })?;
    let mut last_error = None;
    for (root, relative) in candidates {
        match jet_foundation::SHA256::read_file_nofollow_at_root(
            &root,
            &relative,
            jet_foundation::SHA256::MAX_TREE_FILE_BYTES,
        ) {
            Ok(bytes) => return plugin_set_io_text_result(import, results, bytes, Some(path)),
            Err(error) => {
                last_error = Some(error);
            }
        }
    }
    let Some(error) = last_error else {
        return Err(format!(
            "plugin host import `{}` rejected path `{path}`: no granted FS.Read root matched",
            import.operation
        ));
    };
    plugin_set_io_result_error(import, results, "read", Some(path), &error)
}

fn plugin_host_write(
    import: &HostImportFact,
    params: &[Val],
    results: &mut [Val],
    authority: &CanonicalAuthority,
) -> Result<(), String> {
    if params.len() != 2 {
        return Err(format!(
            "plugin host import `{}` expects a Text path and Text/list<u8> contents",
            import.operation
        ));
    }
    let Some(Val::String(path)) = params.first() else {
        return Err(format!(
            "plugin host import `{}` requires a Text path",
            import.operation
        ));
    };
    let Some(contents) = params.get(1).and_then(plugin_bytes) else {
        return Err(format!(
            "plugin host import `{}` requires Text or list<u8> contents",
            import.operation
        ));
    };
    plugin_require_io_result(import, results)?;
    let candidates = plugin_scoped_candidates(path, authority, "FS.Write").map_err(|error| {
        format!(
            "plugin host import `{}` rejected path `{path}`: {error}",
            import.operation
        )
    })?;
    let mut last_error = None;
    for (root, relative) in candidates {
        match jet_foundation::SHA256::write_file_nofollow_at_root(&root, &relative, &contents) {
            Ok(()) => return plugin_set_io_unit_result(import, results),
            Err(error) => {
                last_error = Some(error);
            }
        }
    }
    let Some(error) = last_error else {
        return Err(format!(
            "plugin host import `{}` rejected path `{path}`: no granted FS.Write root matched",
            import.operation
        ));
    };
    plugin_set_io_result_error(import, results, "write", Some(path), &error)
}


fn plugin_network_target(endpoint: &str) -> Result<(String, String), String> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        return Err("network endpoint is empty".to_string());
    }
    let (scheme, authority) = endpoint
        .split_once("://")
        .map(|(scheme, rest)| (scheme.to_ascii_lowercase(), rest))
        .unwrap_or_else(|| (String::new(), endpoint));
    let authority = authority
        .split('/')
        .next()
        .unwrap_or_default()
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    let default_port = match scheme.as_str() {
        "https" | "wss" => 443,
        _ => 80,
    };
    if authority.starts_with('[') {
        let end = authority
            .find(']')
            .ok_or_else(|| "network endpoint has an unterminated IPv6 address".to_string())?;
        let host = &authority[1..end];
        let port = match authority.get(end + 1..).unwrap_or_default() {
            "" => default_port,
            suffix if suffix.starts_with(':') => suffix[1..]
                .parse::<u16>()
                .map_err(|_| "network endpoint has an invalid port".to_string())?,
            _ => return Err("network endpoint has an invalid IPv6 authority".to_string()),
        };
        return Ok((host.to_string(), format!("[{host}]:{port}")));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !port.is_empty() => (
            host,
            port.parse::<u16>()
                .map_err(|_| "network endpoint has an invalid port".to_string())?,
        ),
        _ => (authority, default_port),
    };
    if host.is_empty() {
        return Err("network endpoint has no host".to_string());
    }
    Ok((host.to_string(), format!("{host}:{port}")))
}

fn plugin_host_connect(
    import: &HostImportFact,
    params: &[Val],
    results: &mut [Val],
    authority: &CanonicalAuthority,
) -> Result<(), String> {
    if params.len() != 1 {
        return Err(format!(
            "plugin host import `{}` expects one Text endpoint",
            import.operation
        ));
    }
    let Some(Val::String(endpoint)) = params.first() else {
        return Err(format!(
            "plugin host import `{}` requires a Text endpoint",
            import.operation
        ));
    };
    let (host, target) = plugin_network_target(endpoint)?;
    let requested = format!("Net.Connect:{host}");
    if !authority.allows(&requested) {
        return Err(format!(
            "plugin host import `{}` denied endpoint `{endpoint}`: missing `{requested}`",
            import.operation
        ));
    }
    let addresses = std::net::ToSocketAddrs::to_socket_addrs(&target)
        .map_err(|error| format!("couldn't resolve `{endpoint}`: {error}"))?;
    let mut last_error = None;
    for address in addresses {
        match std::net::TcpStream::connect_timeout(
            &address,
            std::time::Duration::from_millis(PLUGIN_TIMEOUT_MS),
        ) {
            Ok(stream) => {
                drop(stream);
                if results.len() != 1 {
                    return Err(format!(
                        "plugin host import `{}` must return one Bool or Text result",
                        import.operation
                    ));
                }
                match import.result_type_id.as_deref() {
                    Some("Bool") => results[0] = Val::Bool(true),
                    Some("Text") => results[0] = Val::String("connected".to_string()),
                    Some(result_type) => {
                        return Err(format!(
                            "plugin host import `{}` has unsupported Net.Connect result `{result_type}`",
                            import.operation
                        ));
                    }
                    None => {
                        return Err(format!(
                            "plugin host import `{}` must return one Bool or Text result",
                            import.operation
                        ));
                    }
                }
                return Ok(());
            }
            Err(error) => last_error = Some(error.to_string()),
        }
    }
    Err(format!(
        "plugin host import `{}` couldn't connect to `{endpoint}`: {}",
        import.operation,
        last_error.unwrap_or_else(|| "no address resolved".to_string())
    ))
}

fn plugin_host_exec(
    import: &HostImportFact,
    params: &[Val],
    results: &mut [Val],
) -> Result<(), String> {
    if params.len() != 2 {
        return Err(format!(
            "plugin host import `{}` expects a Text program and list<Text> arguments",
            import.operation
        ));
    }
    if results.len() != 1 || import.result_type_id.as_deref() != Some("Text") {
        return Err(format!(
            "plugin host import `{}` must return one Text result",
            import.operation
        ));
    }
    let Some(Val::String(program)) = params.first() else {
        return Err(format!(
            "plugin host import `{}` requires a Text program",
            import.operation
        ));
    };
    let mut command = std::process::Command::new(program);
    let Some(Val::List(arguments)) = params.get(1) else {
        return Err(format!(
            "plugin host import `{}` requires list<Text> arguments",
            import.operation
        ));
    };
    let mut owned = Vec::with_capacity(arguments.len());
    for argument in arguments {
        let Val::String(argument) = argument else {
            return Err(format!(
                "plugin host import `{}` requires list<Text> arguments",
                import.operation
            ));
        };
        owned.push(argument);
    }
    command.args(owned);
    let mut child = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("couldn't execute `{program}`: {error}"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(PLUGIN_TIMEOUT_MS);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("execution of `{program}` exceeded the plugin time limit"));
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(1)),
            Err(error) => return Err(format!("couldn't wait for `{program}`: {error}")),
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("couldn't collect `{program}` output: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "plugin host import `{}` command `{program}` failed: {}",
            import.operation,
            stderr.trim()
        ));
    }
    plugin_set_bytes_result(import, results, output.stdout)
}

fn plugin_host_import_call(
    mut store: StoreContextMut<'_, PluginHostState>,
    import_id: &str,
    params: &[Val],
    results: &mut [Val],
) -> Result<(), wasmtime::Error> {
    let Some(import) = store.data().imports.get(import_id).cloned() else {
        return Err(wasmtime::Error::msg(format!("unregistered plugin host import `{import_id}`")));
    };
    let authority = store.data().authority.clone();
    let decision = import.decision(&authority);
    if !decision.is_allowed() {
        return Err(wasmtime::Error::msg(format!(
            "plugin host import `{}` denied: missing `{}`",
            import.operation,
            import.required_grant()
        )));
    }
    let result = match import.required_grant() {
        "FS.Read" => plugin_host_read(&import, params, results, &authority),
        "FS.Write" => plugin_host_write(&import, params, results, &authority),
        "Net.Connect" => plugin_host_connect(&import, params, results, &authority),
        "Exec" => plugin_host_exec(&import, params, results),
        required => Err(format!(
            "plugin host import `{}` has no runtime adapter for `{required}`",
            import.operation
        )),
    };
    result.map_err(wasmtime::Error::msg)
}

fn plugin_register_function(
    scope: &mut LinkerInstance<'_, PluginHostState>,
    name: &str,
    path: &str,
    function: &ComponentFunc,
    authority: &CanonicalAuthority,
    declared_needs: &[String],
    imports: &mut BTreeMap<String, HostImportFact>,
) -> Result<(), String> {
    let fact = plugin_import_fact(path, function)?;
    if !declared_needs
        .iter()
        .any(|need| jet_foundation::Authority::covers(need.as_str(), fact.required_grant()))
    {
        return Err(format!(
            "plugin host import `{path}` requires `{}` in authority.needs",
            fact.required_grant()
        ));
    }
    let decision = fact.decision(authority);
    if !decision.is_allowed() {
        return Err(format!(
            "plugin host import `{path}` denied: missing `{}`",
            fact.required_grant()
        ));
    }
    let import_id = fact.id.clone();
    scope
        .func_new(name, move |store, params, results| {
            plugin_host_import_call(store, &import_id, params, results)
        })
        .map_err(|error| format!("register plugin host import `{path}`: {error}"))?;
    imports.insert(fact.id.clone(), fact);
    Ok(())
}

fn plugin_register_instance_imports(
    scope: &mut LinkerInstance<'_, PluginHostState>,
    instance: &ComponentInstance,
    engine: &Engine,
    authority: &CanonicalAuthority,
    declared_needs: &[String],
    imports: &mut BTreeMap<String, HostImportFact>,
    prefix: &str,
) -> Result<(), String> {
    for (name, item) in instance.exports(engine) {
        let path = format!("{prefix}::{name}");
        match &item {
            ComponentItem::ComponentFunc(function) => {
                plugin_register_function(
                    scope,
                    name,
                    &path,
                    function,
                    authority,
                    declared_needs,
                    imports,
                )?;
            }
            ComponentItem::ComponentInstance(nested) => {
                let mut nested_scope = scope
                    .instance(name)
                    .map_err(|error| format!("register plugin host instance `{path}`: {error}"))?;
                plugin_register_instance_imports(
                    &mut nested_scope,
                    nested,
                    engine,
                    authority,
                    declared_needs,
                    imports,
                    &path,
                )?;
            }
            _ => {
                return Err(format!(
                    "plugin host import `{path}` is not a component function or instance"
                ));
            }
        }
    }
    Ok(())
}

fn plugin_register_imports(
    linker: &mut Linker<PluginHostState>,
    component: &Component,
    engine: &Engine,
    authority: &CanonicalAuthority,
    declared_needs: &[String],
    imports: &mut BTreeMap<String, HostImportFact>,
) -> Result<(), String> {
    let component_type = component.component_type();
    let mut root = linker.root();
    for (name, item) in component_type.imports(engine) {
        match &item {
            ComponentItem::ComponentFunc(function) => {
                plugin_register_function(
                    &mut root,
                    name,
                    name,
                    function,
                    authority,
                    declared_needs,
                    imports,
                )?;
            }
            ComponentItem::ComponentInstance(instance) => {
                let mut scope = root
                    .instance(name)
                    .map_err(|error| format!("register plugin host instance `{name}`: {error}"))?;
                plugin_register_instance_imports(
                    &mut scope,
                    instance,
                    engine,
                    authority,
                    declared_needs,
                    imports,
                    name,
                )?;
            }
            _ => {
                return Err(format!(
                    "plugin host import `{name}` is not a component function or instance"
                ));
            }
        }
    }
    Ok(())
}

fn plugin_read_module(
    path: &str,
    authority: &CanonicalAuthority,
) -> Result<Vec<u8>, String> {
    let import = plugin_load_import_fact();
    if authority.decide_import(&import) != Verdict::Allowed {
        return Err("plugin load requires an FS.Read authority".to_string());
    }
    let candidates = plugin_scoped_candidates(path, authority, "FS.Read")?;
    let mut last_error = None;
    for (root, relative) in candidates {
        match jet_foundation::SHA256::read_file_nofollow_at_root(
            &root,
            &relative,
            jet_foundation::SHA256::MAX_TREE_FILE_BYTES,
        ) {
            Ok(bytes) => return Ok(bytes),
            Err(error) => {
                last_error = Some(format!(
                    "couldn't read plugin `{path}` below the granted FS.Read root: {error}"
                ));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| {
        format!(
            "couldn't read plugin `{path}` from the granted FS.Read roots"
        )
    }))
}

/// Load a plugin `.wasm` Component Model module from `path` with the checked
/// package authority declaration supplied by the caller. The authority wire is
/// parsed once into the canonical rights carrier and the typed load decision
/// stored with the resulting handle. Every host import and export call reuses
/// that decision; an omitted/empty wire is an explicit zero-grant scope, never
/// an ambient fallback.
pub fn jet_plugin_load_declared(
    path: &str,
    authority_wire: &str,
    declared_needs: &[String],
) -> String {
    let authority = match plugin_authority_from_wire(authority_wire) {
        Ok(authority) => authority,
        Err(error) => return format!("E:{error}"),
    };
    let authority_decision = plugin_load_import_fact().decision(&authority);
    if authority_decision.verdict != Verdict::Allowed {
        return "E:plugin load requires an FS.Read authority".to_string();
    }
    let component_bytes = match plugin_read_module(path, &authority) {
        Ok(bytes) => bytes,
        Err(error) => return format!("E:{error}"),
    };
    let engine = match plugin_engine() {
        Ok(engine) => engine,
        Err(error) => return format!("E:{error}"),
    };
    let component = match Component::new(&engine, &component_bytes) {
        Ok(c) => c,
        Err(e) => return format!("E:couldn't load plugin `{path}`: {e}"),
    };
    // Host-import registration is driven by typed component facts and this
    // one authority decision.  An import with no checked host definition
    // remains a load error; it is never silently ambient.
    let mut linker: Linker<PluginHostState> = Linker::new(&engine);
    let mut imports = BTreeMap::new();
    if let Err(error) = plugin_register_imports(
        &mut linker,
        &component,
        &engine,
        &authority,
        declared_needs,
        &mut imports,
    ) {
        return format!("E:plugin `{path}` host-import preflight failed: {error}");
    }
    let pre_instance = match linker.instantiate_pre(&component) {
        Ok(instance) => instance,
        Err(error) => {
            return format!(
                "E:plugin `{path}` host-import preflight failed during component linking: {error}"
            )
        }
    };
    let mut store = Store::new(
        &engine,
        PluginHostState {
            limits: plugin_limits(),
            authority: authority.clone(),
            authority_decision: authority_decision.clone(),
            imports: imports.clone(),
        },
    );
    store.limiter(|state| &mut state.limits);
    store.set_epoch_deadline(1_000_000_000);
    store.epoch_deadline_trap();
    if let Err(error) = store.set_fuel(PLUGIN_MAX_FUEL) {
        return format!("E:plugin fuel setup failed: {error}");
    }
    let instance = match pre_instance.instantiate(&mut store) {
        Ok(i) => i,
        Err(e) => {
            return format!(
                "E:plugin `{path}` couldn't be instantiated under its authority scope: {e}"
            )
        }
    };
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    PLUGINS.with(|m| {
        m.borrow_mut().insert(
            handle,
            PluginInstance {
                engine,
                store,
                instance,
                authority,
                authority_decision,
                imports,
            },
        )
    });
    format!("O:{handle}")
}
/// Load using the generated package declaration used by the AOT bridge.
pub fn jet_plugin_load(path: &str, authority_wire: &str) -> String {
    let declared_needs = match plugin_declared_authority_needs() {
        Ok(needs) => needs,
        Err(error) => return format!("E:{error}"),
    };
    jet_plugin_load_declared(path, authority_wire, &declared_needs)
}

/// Close a loaded plugin. Returns `true` if the handle was valid, `false`
/// otherwise (already closed, or never opened).
pub fn jet_plugin_close(handle: u64) -> bool {
    PLUGINS.with(|m| m.borrow_mut().remove(&handle).is_some())
}

/// Call exported function `name` on `handle` with the wire-encoded
/// `[PluginValue]` argument list `params_wire`. Returns `"O:"` + the
/// wire-encoded `PluginValue` result, or `"E:"` + a plain message — a missing
/// export, a param-count/type mismatch against the plugin's actual `.wit`
/// signature, or a trap during the call. Every path is a `Result`; nothing
/// here can panic the host program (I2).
pub fn jet_plugin_call(handle: u64, name: &str, params_wire: &str) -> String {
    PLUGINS.with(|m| {
        let mut map = m.borrow_mut();
        let Some(plugin) = map.get_mut(&handle) else {
            return "E:no plugin loaded for this handle".to_string();
        };
        let call_decision = plugin.authority_decision.import.decision(&plugin.authority);
        if !call_decision.is_allowed() {
            return format!(
                "E:plugin call `{name}` is outside the authority granted at load time"
            );
        }
        if params_wire.len() > PLUGIN_MAX_WIRE_BYTES {
            return "E:plugin call arguments exceed the 16 MiB resource budget".to_string();
        }
        let Some(func) = plugin.instance.get_func(&mut plugin.store, name) else {
            return format!("E:plugin has no exported function `{name}`");
        };
        let want_params = func.params(&plugin.store);
        let decoded = plugin_decode_params(params_wire);
        let args = match decoded {
            Ok(args) => args,
            Err(PluginParamDecodeError::TooMany { .. }) => {
                // Preserve the established arity diagnostic while keeping the
                // rejection typed internally; this must never become an empty
                // argument list that can satisfy a zero-parameter export.
                return format!(
                    "E:`{name}` expects {} argument(s), got 0",
                    want_params.len()
                );
            }
            Err(error) => {
                return format!("E:plugin call arguments rejected: {error}");
            }
        };
        if args.len() != want_params.len() {
            return format!(
                "E:`{name}` expects {} argument(s), got {}",
                want_params.len(),
                args.len()
            );
        }
        let mut call_args = Vec::with_capacity(args.len());
        for (i, (arg, ty)) in args.iter().zip(want_params.iter()).enumerate() {
            match plugin_to_val(arg, ty) {
                Some(v) => call_args.push(v),
                None => {
                    return format!(
                        "E:argument {} to `{name}` doesn't match the plugin's declared type ({})",
                        i + 1,
                        plugin_type_name(ty)
                    );
                }
            }
        }
        if let Err(error) = plugin.store.set_fuel(PLUGIN_MAX_FUEL) {
            return format!("E:plugin fuel setup failed: {error}");
        }
        plugin.store.set_epoch_deadline(1);
        plugin.store.epoch_deadline_trap();
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancelled_timer = std::sync::Arc::clone(&cancelled);
        let timed_out = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let timed_out_timer = std::sync::Arc::clone(&timed_out);
        let timer_engine = plugin.engine.clone();
        let timer = std::thread::spawn(move || {
            let deadline = std::time::Instant::now()
                + std::time::Duration::from_millis(PLUGIN_TIMEOUT_MS);
            while std::time::Instant::now() < deadline {
                if cancelled_timer.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            if !cancelled_timer.load(Ordering::Relaxed) {
                timed_out_timer.store(true, Ordering::Relaxed);
                timer_engine.increment_epoch();
            }
        });
        let want_results = func.results(&plugin.store);
        if want_results.len() != 1 {
            cancelled.store(true, Ordering::Relaxed);
            let _ = timer.join();
            plugin.store.set_epoch_deadline(1_000_000_000);
            return format!(
                "E:`{name}` returns {} values — v1 plugin calls support exactly one return value",
                want_results.len()
            );
        }
        let Some(zero) = plugin_zero_val(&want_results[0]) else {
            cancelled.store(true, Ordering::Relaxed);
            let _ = timer.join();
            plugin.store.set_epoch_deadline(1_000_000_000);
            return format!(
                "E:`{name}` returns an unsupported Component Model type ({})",
                plugin_type_name(&want_results[0])
            );
        };
        let mut results = vec![zero];
        let call_result = func.call(&mut plugin.store, &call_args, &mut results);
        cancelled.store(true, Ordering::Relaxed);
        let _ = timer.join();
        plugin.store.set_epoch_deadline(1_000_000_000);
        if let Err(error) = call_result {
            let fuel_exhausted = matches!(plugin.store.get_fuel(), Ok(0));
            let deadline_elapsed = timed_out.load(Ordering::Relaxed);
            return plugin_call_trap_error(name, error, fuel_exhausted, deadline_elapsed);
        }
        // Copy the result before `post_return`: strings, lists, and nested
        // records own guest allocations until the canonical post-return hook
        // releases them.  Returning a borrowed `Val` past that hook would make
        // aggregate results a use-after-free at the host boundary.
        let copied = plugin_from_val(&results[0]);
        // Component Model contract: `post_return` must run after every call
        // before the instance can be called again.
        if let Err(error) = func.post_return(&mut plugin.store) {
            return format!("E:plugin `{name}` post-return failed: {error}");
        }
        match copied {
            Some(value) => {
                let wire = plugin_encode_value(&value);
                if wire.len() <= PLUGIN_MAX_WIRE_BYTES {
                    format!("O:{wire}")
                } else {
                    "E:plugin result exceeds the 16 MiB resource budget".to_string()
                }
            }
            None => format!(
                "E:`{name}` returned an unsupported Component Model value ({})",
                plugin_type_name(&want_results[0])
            ),
        }
    })
}

fn plugin_call_trap_error(
    name: &str,
    error: wasmtime::Error,
    fuel_exhausted: bool,
    deadline_elapsed: bool,
) -> String {
    let limit_trap = error
        .downcast_ref::<Trap>()
        .is_some_and(|trap| matches!(*trap, Trap::OutOfFuel | Trap::Interrupt));
    if fuel_exhausted || deadline_elapsed || limit_trap {
        return format!(
            "E:calling `{name}` trapped: plugin execution limit reached (fuel/epoch interrupt)"
        );
    }
    format!("E:calling `{name}` trapped: {error}")
}


fn plugin_type_name(ty: &Type) -> String {
    match ty {
        Type::S8 => "s8",
        Type::U8 => "u8",
        Type::S16 => "s16",
        Type::U16 => "u16",
        Type::S32 => "s32",
        Type::U32 => "u32",
        Type::S64 => "Int",
        Type::U64 => "u64",
        Type::Float32 => "f32",
        Type::Float64 => "Float",
        Type::Bool => "Bool",
        Type::Char => "char",
        Type::String => "Text",
        Type::List(_) => "list",
        Type::Record(_) => "record",
        Type::Tuple(_) => "tuple",
        Type::Variant(_) => "variant",
        Type::Enum(_) => "enum",
        Type::Option(_) => "option",
        Type::Result(_) => "result",
        Type::Flags(_) => "flags",
        Type::Own(_) => "own resource",
        Type::Borrow(_) => "borrowed resource",
    }
    .to_string()
}

fn plugin_integer(value: i64, ty: &Type) -> Option<Val> {
    match ty {
        Type::S8 if i8::try_from(value).is_ok() => Some(Val::S8(value as i8)),
        Type::U8 if u8::try_from(value).is_ok() => Some(Val::U8(value as u8)),
        Type::S16 if i16::try_from(value).is_ok() => Some(Val::S16(value as i16)),
        Type::U16 if u16::try_from(value).is_ok() => Some(Val::U16(value as u16)),
        Type::S32 if i32::try_from(value).is_ok() => Some(Val::S32(value as i32)),
        Type::U32 if u32::try_from(value).is_ok() => Some(Val::U32(value as u32)),
        Type::S64 => Some(Val::S64(value)),
        Type::U64 if value >= 0 => Some(Val::U64(value as u64)),
        _ => None,
    }
}

fn plugin_to_val(value: &PluginValue, ty: &Type) -> Option<Val> {
    match (value, ty) {
        (PluginValue::Int(value), ty) => plugin_integer(*value, ty),
        (PluginValue::Float(value), Type::Float32) => {
            let value = *value as f32;
            value.is_finite().then_some(Val::Float32(value))
        }
        (PluginValue::Float(value), Type::Float64) => Some(Val::Float64(*value)),
        (PluginValue::Bool(value), Type::Bool) => Some(Val::Bool(*value)),
        (PluginValue::Text(value), Type::String) => Some(Val::String(value.clone())),
        (PluginValue::Text(value), Type::Char) => {
            let mut chars = value.chars();
            let ch = chars.next()?;
            chars.next().is_none().then_some(Val::Char(ch))
        }
        (PluginValue::List(values), Type::List(list)) => values
            .iter()
            .map(|value| plugin_to_val(value, &list.ty()))
            .collect::<Option<Vec<_>>>()
            .map(Val::List),
        (PluginValue::Record(values), Type::Record(record)) => {
            let fields = record.fields().collect::<Vec<_>>();
            (values.len() == fields.len()).then_some(())?;
            let mut out = Vec::with_capacity(values.len());
            for ((name, value), field) in values.iter().zip(fields) {
                if name != field.name {
                    return None;
                }
                out.push((name.clone(), plugin_to_val(value, &field.ty)?));
            }
            Some(Val::Record(out))
        }
        (PluginValue::Tuple(values), Type::Tuple(tuple)) => {
            let types = tuple.types().collect::<Vec<_>>();
            (values.len() == types.len()).then_some(())?;
            values
                .iter()
                .zip(types.iter())
                .map(|(value, ty)| plugin_to_val(value, ty))
                .collect::<Option<Vec<_>>>()
                .map(Val::Tuple)
        }
        (PluginValue::Option(value), Type::Option(option)) => Some(Val::Option(match value.as_ref() {
            Some(value) => Some(Box::new(plugin_to_val(value, &option.ty())?)),
            None => None,
        })),
        (PluginValue::ResultOk(value), Type::Result(result)) => {
            let value = match (value.as_ref(), result.ok()) {
                (None, None) => None,
                (Some(value), Some(ty)) => Some(Box::new(plugin_to_val(value, &ty)?)),
                _ => return None,
            };
            Some(Val::Result(Ok(value)))
        }
        (PluginValue::ResultErr(value), Type::Result(result)) => {
            let value = match (value.as_ref(), result.err()) {
                (None, None) => None,
                (Some(value), Some(ty)) => Some(Box::new(plugin_to_val(value, &ty)?)),
                _ => return None,
            };
            Some(Val::Result(Err(value)))
        }
        (PluginValue::Variant(name, value), Type::Variant(variant)) => {
            let case = variant.cases().find(|case| case.name == name)?;
            let value = match (value.as_ref(), case.ty.as_ref()) {
                (None, None) => None,
                (Some(value), Some(ty)) => Some(Box::new(plugin_to_val(value, ty)?)),
                _ => return None,
            };
            Some(Val::Variant(name.clone(), value))
        }
        (PluginValue::Enum(name), Type::Enum(enum_type))
            if enum_type.names().any(|candidate| candidate == name) =>
        {
            Some(Val::Enum(name.clone()))
        }
        (PluginValue::Flags(names), Type::Flags(flags))
            if names
                .iter()
                .all(|name| flags.names().any(|candidate| candidate == name)) =>
        {
            Some(Val::Flags(names.clone()))
        }
        _ => None,
    }
}

fn plugin_zero_val(ty: &Type) -> Option<Val> {
    match ty {
        Type::S8 => Some(Val::S8(0)),
        Type::U8 => Some(Val::U8(0)),
        Type::S16 => Some(Val::S16(0)),
        Type::U16 => Some(Val::U16(0)),
        Type::S32 => Some(Val::S32(0)),
        Type::U32 => Some(Val::U32(0)),
        Type::S64 => Some(Val::S64(0)),
        Type::U64 => Some(Val::U64(0)),
        Type::Float32 => Some(Val::Float32(0.0)),
        Type::Float64 => Some(Val::Float64(0.0)),
        Type::Bool => Some(Val::Bool(false)),
        Type::Char => Some(Val::Char('\0')),
        Type::String => Some(Val::String(String::new())),
        Type::List(_) => Some(Val::List(Vec::new())),
        Type::Record(record) => record
            .fields()
            .map(|field| Some((field.name.to_string(), plugin_zero_val(&field.ty)?)))
            .collect::<Option<Vec<_>>>()
            .map(Val::Record),
        Type::Tuple(tuple) => tuple
            .types()
            .map(|ty| plugin_zero_val(&ty))
            .collect::<Option<Vec<_>>>()
            .map(Val::Tuple),
        Type::Variant(variant) => {
            let case = variant.cases().next()?;
            let value = match case.ty.as_ref() {
                Some(ty) => Some(Box::new(plugin_zero_val(ty)?)),
                None => None,
            };
            Some(Val::Variant(case.name.to_string(), value))
        }
        Type::Enum(enum_type) => enum_type.names().next().map(|name| Val::Enum(name.to_string())),
        Type::Option(_) => Some(Val::Option(None)),
        Type::Result(result) => Some(Val::Result(Ok(match result.ok() {
            Some(ty) => Some(Box::new(plugin_zero_val(&ty)?)),
            None => None,
        }))),
        Type::Flags(_) => Some(Val::Flags(Vec::new())),
        Type::Own(_) | Type::Borrow(_) => None,
    }
}


fn plugin_from_val(value: &Val) -> Option<PluginValue> {
    match value {
        Val::S8(value) => Some(PluginValue::Int(*value as i64)),
        Val::U8(value) => Some(PluginValue::Int(*value as i64)),
        Val::S16(value) => Some(PluginValue::Int(*value as i64)),
        Val::U16(value) => Some(PluginValue::Int(*value as i64)),
        Val::S32(value) => Some(PluginValue::Int(*value as i64)),
        Val::U32(value) => Some(PluginValue::Int(*value as i64)),
        Val::S64(value) => Some(PluginValue::Int(*value)),
        Val::U64(value) => Some(PluginValue::Int(i64::try_from(*value).ok()?)),
        Val::Float32(value) => Some(PluginValue::Float(*value as f64)),
        Val::Float64(value) => Some(PluginValue::Float(*value)),
        Val::Bool(value) => Some(PluginValue::Bool(*value)),
        Val::Char(value) => Some(PluginValue::Text(value.to_string())),
        Val::String(value) => Some(PluginValue::Text(value.clone())),
        Val::List(values) => Some(PluginValue::List(
            values.iter().map(plugin_from_val).collect::<Option<Vec<_>>>()?,
        )),
        Val::Record(values) => Some(PluginValue::Record(
            values
                .iter()
                .map(|(name, value)| Some((name.clone(), plugin_from_val(value)?)))
                .collect::<Option<Vec<_>>>()?,
        )),
        Val::Tuple(values) => Some(PluginValue::Tuple(
            values.iter().map(plugin_from_val).collect::<Option<Vec<_>>>()?,
        )),
        Val::Variant(name, value) => Some(PluginValue::Variant(
            name.clone(),
            match value.as_ref() {
                Some(value) => Some(Box::new(plugin_from_val(value)?)),
                None => None,
            },
        )),
        Val::Enum(name) => Some(PluginValue::Enum(name.clone())),
        Val::Option(value) => Some(PluginValue::Option(match value.as_ref() {
            Some(value) => Some(Box::new(plugin_from_val(value)?)),
            None => None,
        })),
        Val::Result(Ok(value)) => Some(PluginValue::ResultOk(match value.as_ref() {
            Some(value) => Some(Box::new(plugin_from_val(value)?)),
            None => None,
        })),
        Val::Result(Err(value)) => Some(PluginValue::ResultErr(match value.as_ref() {
            Some(value) => Some(Box::new(plugin_from_val(value)?)),
            None => None,
        })),
        Val::Flags(names) => Some(PluginValue::Flags(names.clone())),
        Val::Resource(_) => None,
    }
}

