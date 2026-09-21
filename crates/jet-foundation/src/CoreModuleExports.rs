//! D-ONCE: one table of which types each Core module exports, so a qualified
//! import (`alias.Leaf` where `alias` names a Core module) resolves through
//! one generic lookup instead of a hand-written match arm per module.
//!
//! Most modules canonicalize a qualified leaf straight to `Leaf`. `core.crypto`
//! additionally marks a narrow secret-bearing subset so the
//! caller can wrap those in the nominal-provenance type instead of the plain
//! one — entries are `Plain`, an explicit unit-variant `Enum`, a generic
//! `Generic(arity)`, or the crypto-only `CryptoNominal` form.

/// How a resolved Core-module leaf should be represented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreLeafKind {
    /// Canonicalize straight to `Type::Named(leaf)`.
    Plain,
    /// A Core enum whose named variants all have unit payloads.
    Enum(&'static [&'static str]),
    /// A generic Core type; the value is its required type-parameter count.
    Generic(usize),
    /// D-CRYPTO-API1=A: secret-bearing crypto values keep distinct nominal
    /// provenance from a same-named local type.
    CryptoNominal,
}

// BEGIN GENERATED CORE DECLARATIONS
// Source: crates/jet-codegen/src/Prelude/Core.jet
// Source SHA-256: 468ebb2c72aaee16226f5d8aebaf30e6e386a3ce771451527dae110879c8107c
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreModuleDeclaration {
    pub module: &'static str,
    pub members: &'static [&'static str],
    pub type_exports: &'static [(&'static str, CoreLeafKind)],
    pub dependencies: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreBootstrapStep {
    pub name: &'static str,
    pub dependencies: &'static [&'static str],
}

pub const CORE_BOOTSTRAP_ORDER: &[CoreBootstrapStep] = &[
    CoreBootstrapStep { name: "Syntax", dependencies: &[] },
    CoreBootstrapStep { name: "Effects", dependencies: &["Syntax"] },
    CoreBootstrapStep { name: "Core", dependencies: &["Effects"] },
    CoreBootstrapStep { name: "Derives", dependencies: &["Core"] },
];

pub const CORE_MODULE_NAMES: &[&str] = &[
    "app",
    "core",
    "core.models",
    "core.devtools",
    "core.archive",
    "core.archive.gzip",
    "core.archive.zstd",
    "core.args",
    "core.auth",
    "core.build",
    "core.compiler",
    "core.compiler.lang",
    "core.collections",
    "core.collections.set",
    "core.compute",
    "core.compute.solve",
    "core.crypto",
    "core.crypto.expert",
    "core.crypto.random",
    "core.crypto.uuid",
    "core.crypto.vault",
    "core.data",
    "core.data.arrow",
    "core.data.loader",
    "core.data.stream",
    "core.data.plot",
    "core.data.sketch",
    "core.data.sketch.cms",
    "core.data.sketch.hll",
    "core.data.sketch.reservoir",
    "core.data.sketch.tdigest",
    "core.db",
    "core.email",
    "core.encoding",
    "core.encoding.base32",
    "core.encoding.base64",
    "core.encoding.binary",
    "core.encoding.cbor",
    "core.encoding.csv",
    "core.encoding.hex",
    "core.encoding.ini",
    "core.encoding.json",
    "core.encoding.jsonl",
    "core.encoding.toml",
    "core.encoding.xml",
    "core.encoding.yaml",
    "core.event",
    "core.files",
    "core.files.path",
    "core.font",
    "core.game",
    "core.game.raylib",
    "core.http",
    "core.http.client",
    "core.http.server",
    "core.io",
    "core.jobs",
    "core.log",
    "core.math",
    "core.math.random",
    "core.math.combinatorics",
    "core.math.stats",
    "core.mem",
    "core.mem.scope",
    "core.mod",
    "core.net",
    "core.net.mime",
    "core.net.tls",
    "core.net.url",
    "core.net.ws",
    "core.net.ip",
    "core.perf",
    "core.plugin",
    "core.prelude",
    "core.process",
    "core.reactive",
    "core.reactive.loadable",
    "core.reflect",
    "core.regex",
    "core.rt",
    "core.service",
    "core.sync",
    "core.sys",
    "core.tasks",
    "core.term",
    "core.testing",
    "core.text",
    "core.text.fmt",
    "core.text.html",
    "core.text.wrap",
    "core.text.parse",
    "core.time",
    "core.time.calendar",
    "core.time.expiring",
    "core.ui",
    "core.tui",
    "core.ui.host",
    "core.ui.host.clipboard",
    "core.ui.host.ime",
    "core.ui.host.drag_drop",
    "core.ui.host.shortcuts",
    "core.ui.host.accessibility",
    "core.units",
    "core.watcher",
    "core.web",
    "core.web.browser",
    "core.web.devserver",
    "core.web.forms",
    "core.web.query",
    "core.web.router",
    "core.web.storage",
    "core.web.storage.local",
    "core.web.storage.session",
    "core.web.store",
    "core.web.table",
    "core.web.virtual",
];

const CORE_MODULE_0_MEMBERS: &[&str] = &["Auth", "LiveQuery", "Session", "auth", "auth_oauth", "auth_routes", "auth_show", "invalidate", "live", "live_get", "live_show", "live_stats", "signal_push", "subscribe", "sync", "transact_invalidate"];
const CORE_MODULE_0_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_0_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_1_MEMBERS: &[&str] = &[];
const CORE_MODULE_1_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_1_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_2_MEMBERS: &[&str] = &["open", "Model", "is_open", "path", "size", "tensor_names"];
const CORE_MODULE_2_TYPES: &[(&str, CoreLeafKind)] = &[("Model", CoreLeafKind::Plain)];
const CORE_MODULE_2_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_3_MEMBERS: &[&str] = &["publish"];
const CORE_MODULE_3_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_3_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_4_MEMBERS: &[&str] = &["adler32", "crc32", "deflate", "inflate", "compress", "decompress", "tar_add", "tar_get", "tar_names_json", "unzip", "zip_close", "zip_compress", "zip_decompress", "zip_extract", "zip_names_json", "zip_next", "zip_open", "zip_read", "zip_write", "list", "create", "gunzip"];
const CORE_MODULE_4_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_4_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_5_MEMBERS: &[&str] = &["compress", "decompress", "is_gzip", "isize", "magic", "crc", "compress_text", "decompress_text", "peek_isize", "compress_file", "decompress_file"];
const CORE_MODULE_5_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_5_DEPENDENCIES: &[&str] = &["core.archive"];

const CORE_MODULE_6_MEMBERS: &[&str] = &["compress", "decompress", "is_zstd", "magic", "compress_text", "decompress_text", "compress_file", "decompress_file"];
const CORE_MODULE_6_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_6_DEPENDENCIES: &[&str] = &["core.archive"];

const CORE_MODULE_7_MEMBERS: &[&str] = &["decode", "decode_argv", "flag", "get_bool", "get_int", "get_text", "has", "help_text", "merge", "positionals", "program", "spec", "ArgDef", "ArgsSpec", "apply_defaults", "argument", "count_flag", "default_value", "define", "dest", "get_choice", "get_or", "help_from", "missing_required", "remainder", "required", "usage", "wants_help"];
const CORE_MODULE_7_TYPES: &[(&str, CoreLeafKind)] = &[("ArgDef", CoreLeafKind::Plain), ("ArgsSpec", CoreLeafKind::Plain)];
const CORE_MODULE_7_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_8_MEMBERS: &[&str] = &["Auth", "AuthError", "Claims", "Session", "magic_link_consume", "magic_link_issue", "oauth_begin", "oauth_finish", "password_login", "register_user", "session_cookie", "session_id", "session_show", "session_user", "session_validate", "verify_jwt", "verify_paseto"];
const CORE_MODULE_8_TYPES: &[(&str, CoreLeafKind)] = &[("Auth", CoreLeafKind::Plain), ("AuthError", CoreLeafKind::Enum(&["Rejected", "Expired", "Malformed", "Unavailable"])), ("Claims", CoreLeafKind::Plain), ("Session", CoreLeafKind::Plain)];
const CORE_MODULE_8_DEPENDENCIES: &[&str] = &["core.crypto", "core.net"];

const CORE_MODULE_9_MEMBERS: &[&str] = &["BuildGraph", "BuildGraphAction", "BuildGraphActionKey", "BuildGraphCacheDelta", "BuildGraphDiff", "BuildGraphFile", "BuildGraphFileDelta", "BuildGraphInputDigest", "BuildGraphKeyDelta", "BuildGraphNode", "BuildGraphTarget", "graph", "receipt_diff"];
const CORE_MODULE_9_TYPES: &[(&str, CoreLeafKind)] = &[("BuildGraph", CoreLeafKind::Plain), ("BuildGraphTarget", CoreLeafKind::Plain), ("BuildGraphAction", CoreLeafKind::Plain), ("BuildGraphFile", CoreLeafKind::Plain), ("BuildGraphNode", CoreLeafKind::Plain), ("BuildGraphInputDigest", CoreLeafKind::Plain), ("BuildGraphActionKey", CoreLeafKind::Plain), ("BuildGraphFileDelta", CoreLeafKind::Plain), ("BuildGraphKeyDelta", CoreLeafKind::Plain), ("BuildGraphCacheDelta", CoreLeafKind::Plain), ("BuildGraphDiff", CoreLeafKind::Plain)];
const CORE_MODULE_9_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_10_MEMBERS: &[&str] = &["check", "lex", "lock", "manifest", "package", "parse", "profiles", "source_map", "Pair", "Token"];
const CORE_MODULE_10_TYPES: &[(&str, CoreLeafKind)] = &[("Pair", CoreLeafKind::Plain), ("Token", CoreLeafKind::Plain)];
const CORE_MODULE_10_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_11_MEMBERS: &[&str] = &["ABI", "ArithmeticMode", "Effect", "FfiLanguage", "InlineMode", "JobScope", "KernelMode", "Layout", "Maturity", "MemoBound", "NamingCase", "ObligationMode", "Path", "PolicySetting", "Site", "State", "TaintKind", "Target", "Track"];
const CORE_MODULE_11_TYPES: &[(&str, CoreLeafKind)] = &[("ABI", CoreLeafKind::Enum(&["System", "Cdecl", "Stdcall", "Fastcall", "Win64", "Sysv64"])), ("ArithmeticMode", CoreLeafKind::Enum(&["Checked", "Wrapping", "Saturating"])), ("Effect", CoreLeafKind::Enum(&["FS", "Net", "Crypto", "Time", "Random", "Env", "Proc", "IO", "DB", "Exec", "Browser", "Secret"])), ("FfiLanguage", CoreLeafKind::Enum(&["C", "Cpp", "Asm"])), ("InlineMode", CoreLeafKind::Enum(&["Hint", "Always", "Never"])), ("JobScope", CoreLeafKind::Enum(&["Dev", "Ship", "Internal"])), ("KernelMode", CoreLeafKind::Enum(&["Parallel"])), ("Layout", CoreLeafKind::Enum(&["C", "Columnar"])), ("Maturity", CoreLeafKind::Enum(&["Experimental", "Tested", "Hardened"])), ("MemoBound", CoreLeafKind::Enum(&["Default", "Unset"])), ("NamingCase", CoreLeafKind::Enum(&["Camel", "Snake", "Pascal", "Kebab", "Screaming"])), ("ObligationMode", CoreLeafKind::Enum(&["Unspecified", "GateOnly", "Obligations", "PerSite", "Track", "Skip"])), ("Path", CoreLeafKind::Plain), ("PolicySetting", CoreLeafKind::Enum(&["Allow", "Deny", "Unsafe", "Gc", "ExplicitUnits", "Copies", "Sentries", "Explicit", "On", "Off"])), ("Site", CoreLeafKind::Enum(&["Package", "File", "Module", "Function", "Method", "Block", "Statement", "Expression", "Type", "Impl", "Declaration", "Constant", "Field", "Variant", "Parameter", "Test", "Operation", "Text"])), ("State", CoreLeafKind::Plain), ("TaintKind", CoreLeafKind::Enum(&["Input", "PII", "Secret", "Credential"])), ("Target", CoreLeafKind::Enum(&["Native", "Web", "Wasm", "JS", "Freestanding", "OS"])), ("Track", CoreLeafKind::Enum(&["Frontend", "Backend", "Runtime", "Tooling"]))];
const CORE_MODULE_11_DEPENDENCIES: &[&str] = &["core.compiler"];

const CORE_MODULE_12_MEMBERS: &[&str] = &["Counter", "Deque", "OrderedMap", "Layer", "Chain", "counter", "counter_from", "add", "inc", "dec", "get", "set_count", "total", "names", "elements", "most_common", "subtract", "merge_add", "clear_counter", "deque", "deque_from", "deque_len", "deque_is_empty", "append", "appendleft", "pop", "popleft", "peek", "peekleft", "extend", "extendleft", "rotate", "deque_items", "heapify", "heappush", "heappop", "heappushpop", "heapreplace", "nsmallest", "nlargest", "merge_sorted", "bisect_left", "bisect_right", "insort_left", "insort_right", "ordered_map", "map_get", "map_set", "map_remove", "map_keys", "map_values", "map_contains", "map_len", "chain", "chain_push", "chain_get", "chain_contains"];
const CORE_MODULE_12_TYPES: &[(&str, CoreLeafKind)] = &[("Counter", CoreLeafKind::Plain), ("Deque", CoreLeafKind::Plain), ("OrderedMap", CoreLeafKind::Plain), ("Layer", CoreLeafKind::Plain), ("Chain", CoreLeafKind::Plain)];
const CORE_MODULE_12_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_13_MEMBERS: &[&str] = &["StringSet", "new", "from_list", "add", "discard", "remove", "contains", "len", "is_empty", "to_list", "clear", "union", "intersection", "difference", "symmetric_difference", "issubset", "issuperset", "isdisjoint", "clone_set"];
const CORE_MODULE_13_TYPES: &[(&str, CoreLeafKind)] = &[("StringSet", CoreLeafKind::Plain)];
const CORE_MODULE_13_DEPENDENCIES: &[&str] = &["core.collections"];

const CORE_MODULE_14_MEMBERS: &[&str] = &["ComputeDevice", "ComputeError", "ComputeStream", "SparseTensor", "Tensor", "VjpRun", "abs", "add", "broadcast_to", "deserialize", "det", "device", "device_auto", "device_cpu", "device_cuda", "device_metal", "device_vulkan", "device_webgpu", "div", "exp", "eye", "fft", "from_list", "full", "get", "gradient", "inv", "jvp", "kernel_bounds_ok", "log", "matmul", "matmul_f32_tile", "matrix", "maximum", "minimum", "mse_loss", "mul", "negate", "numel", "on_device", "ones", "placement", "profile_f32_strict", "profile_show", "rank", "reshape", "serialize", "set", "sgd_step", "shape", "solve", "sparse_mv", "sparse_nnz", "sparse_show", "sqrt", "stream_new", "stream_new_on", "stream_show", "stream_sync", "sub", "sum_axis", "to_list", "to_sparse", "transfer", "transfer_show", "transpose", "value_and_gradient", "vec", "vjp", "zeros"];
const CORE_MODULE_14_TYPES: &[(&str, CoreLeafKind)] = &[("ComputeDevice", CoreLeafKind::Plain), ("ComputeError", CoreLeafKind::Enum(&["Shape", "Device", "Singular", "Index", "Parse"])), ("ComputeStream", CoreLeafKind::Plain), ("SparseTensor", CoreLeafKind::Plain), ("Tensor", CoreLeafKind::Plain), ("VjpRun", CoreLeafKind::Plain)];
const CORE_MODULE_14_DEPENDENCIES: &[&str] = &["core.math", "core.mem"];

const CORE_MODULE_15_MEMBERS: &[&str] = &["Solver", "dense", "lu"];
const CORE_MODULE_15_TYPES: &[(&str, CoreLeafKind)] = &[("Solver", CoreLeafKind::Plain)];
const CORE_MODULE_15_DEPENDENCIES: &[&str] = &["core.compute"];

const CORE_MODULE_16_MEMBERS: &[&str] = &["CryptoError", "Digest256", "Digest512", "FileCryptoError", "Hasher", "KeyUnlock", "KeyWrapError", "PasswordHash", "Sealed", "Secret", "SharedSecret", "Signature", "SigningKey", "VerifyKey", "WrappedKey", "WrappedVaultKey", "X25519PublicKey", "X25519SecretKey", "blake3", "constant_time_equal", "constant_time_equal_bytes", "file_open", "file_seal", "generatekey", "hkdf_sha256", "hmac_sha256", "open", "password_hash", "password_hash_with_salt", "password_verify", "pbkdf2_hmac", "privatedecrypt", "privateencrypt", "publicdecrypt", "publicencrypt", "seal", "sha1", "sha224", "sha256", "sha384", "sha3_224", "sha3_256", "sha3_384", "sha3_512", "sha512", "sign", "unwrap", "verify", "wrap", "x25519", "x25519_public", "x25519_shared", "EdPoint", "new", "update", "digest"];
const CORE_MODULE_16_TYPES: &[(&str, CoreLeafKind)] = &[("Secret", CoreLeafKind::CryptoNominal), ("SigningKey", CoreLeafKind::CryptoNominal), ("X25519SecretKey", CoreLeafKind::CryptoNominal), ("SharedSecret", CoreLeafKind::CryptoNominal), ("VerifyKey", CoreLeafKind::Plain), ("X25519PublicKey", CoreLeafKind::Plain), ("Signature", CoreLeafKind::Plain), ("Sealed", CoreLeafKind::Plain), ("WrappedKey", CoreLeafKind::Plain), ("WrappedVaultKey", CoreLeafKind::Plain), ("KeyUnlock", CoreLeafKind::Plain), ("PasswordHash", CoreLeafKind::Plain), ("Digest256", CoreLeafKind::Plain), ("Digest512", CoreLeafKind::Plain), ("Hasher", CoreLeafKind::Plain), ("CryptoError", CoreLeafKind::Plain), ("FileCryptoError", CoreLeafKind::Plain), ("KeyWrapError", CoreLeafKind::Plain), ("EdPoint", CoreLeafKind::Plain)];
const CORE_MODULE_16_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_17_MEMBERS: &[&str] = &["aes256gcm_open", "aes256gcm_seal", "argon2id", "ed25519_sign", "ed25519_verify_strict", "hkdf_sha256_raw", "migrate_v1", "open_v1", "secret_bytes", "shared_secret_bytes", "signing_key_bytes", "x25519_raw", "x25519_secret_bytes", "xchacha20poly1305_open", "xchacha20poly1305_seal"];
const CORE_MODULE_17_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_17_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_18_MEMBERS: &[&str] = &["bytes", "choice", "int_range", "token_hex", "token_urlsafe", "u32", "u64", "token_bytes", "randbelow", "randbits", "compare_digest", "choice_int", "shuffle_ints"];
const CORE_MODULE_18_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_18_DEPENDENCIES: &[&str] = &["core.crypto"];

const CORE_MODULE_19_MEMBERS: &[&str] = &["parse", "v4", "v5", "uuid5", "v7"];
const CORE_MODULE_19_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_19_DEPENDENCIES: &[&str] = &["core.crypto", "core.encoding.hex"];

const CORE_MODULE_20_MEMBERS: &[&str] = &["ExpiringSecret", "KeyRef", "KeyStatus", "KeyUnlock", "KeyWrapError", "MutationPlan", "Rotation", "VaultError", "VaultWrite", "WrappedImportPlan", "WrappedVaultKey", "authorize_wrapped_import", "authorize_write", "commit_generate", "commit_import_signing", "commit_import_wrapped", "commit_import_x25519", "commit_retire", "commit_revoke", "commit_rotate", "commit_store", "current", "export_to_passphrase", "export_to_recipients", "get", "load", "prepare_generate", "prepare_import_signing", "prepare_import_wrapped", "prepare_import_x25519", "prepare_retire", "prepare_revoke", "prepare_rotate", "prepare_store", "status", "versions", "PlanBits", "VaultRec"];
const CORE_MODULE_20_TYPES: &[(&str, CoreLeafKind)] = &[("ExpiringSecret", CoreLeafKind::Generic(1)), ("KeyRef", CoreLeafKind::Plain), ("KeyStatus", CoreLeafKind::Enum(&["Current", "Retired", "Revoked"])), ("KeyUnlock", CoreLeafKind::Plain), ("KeyWrapError", CoreLeafKind::Plain), ("MutationPlan", CoreLeafKind::Plain), ("Rotation", CoreLeafKind::Plain), ("VaultError", CoreLeafKind::Enum(&["NotFound", "Revoked", "Unauthorized", "Corrupt"])), ("VaultWrite", CoreLeafKind::Plain), ("WrappedImportPlan", CoreLeafKind::Plain), ("WrappedVaultKey", CoreLeafKind::Plain), ("PlanBits", CoreLeafKind::Plain), ("VaultRec", CoreLeafKind::Plain)];
const CORE_MODULE_20_DEPENDENCIES: &[&str] = &["core.crypto"];

const CORE_MODULE_21_MEMBERS: &[&str] = &["DataAuthority", "DataColumn", "DataError", "DataErrorKind", "DataFormat", "DataFreshness", "DataInvalidationCause", "DataLimits", "DataLineOptions", "DataLoader", "DataLoaderKind", "DataLoaderStatus", "DataPivotCell", "DataProvenance", "Query", "DataSchema", "DataSnapshot", "DataSnapshotIdentity", "DataSourceIdentity", "DataStatus", "DataStream", "DataTracked", "DataWatch", "DataWatchStatus", "Group", "track", "JetDataPlotAccessibility", "JetDataPlotAggregate", "JetDataPlotAxis", "JetDataPlotBackend", "JetDataPlotCapability", "JetDataPlotChannel", "JetDataPlotColumn", "JetDataPlotDomain", "JetDataPlotEncoding", "JetDataPlotError", "JetDataPlotErrorKind", "JetDataPlotFacet", "JetDataPlotFacetKind", "JetDataPlotField", "JetDataPlotFilterOp", "JetDataPlotInspection", "JetDataPlotInteraction", "JetDataPlotLayer", "JetDataPlotLayout", "JetDataPlotLegend", "JetDataPlotLegendPosition", "JetDataPlotMark", "JetDataPlotPlan", "JetDataPlotProjection", "JetDataPlotRender", "JetDataPlotRenderFormat", "JetDataPlotScale", "JetDataPlotScaleKind", "JetDataPlotSchema", "JetDataPlotSelectedRow", "JetDataPlotSourceFacts", "JetDataPlotSupport", "JetDataPlotTransform", "JetDataPlotValue", "bar_svg", "bar_text", "count", "csv", "csv_reader", "database", "describe", "file", "file_member", "inner_join", "inspect", "inspect_json", "json", "json_reader", "left_join", "line_svg", "line_text", "load", "load_default", "max", "mean", "median", "min", "pivot_sum", "plot", "quantile", "query", "render", "require_bridge", "rolling_mean", "schema", "show", "snapshot", "status", "stddev", "sum", "svg", "text", "url", "value", "variance"];
const CORE_MODULE_21_TYPES: &[(&str, CoreLeafKind)] = &[("Query", CoreLeafKind::Generic(1)), ("DataLoader", CoreLeafKind::Generic(1)), ("DataLoaderKind", CoreLeafKind::Plain), ("DataSnapshot", CoreLeafKind::Generic(1)), ("DataStream", CoreLeafKind::Generic(1)), ("DataTracked", CoreLeafKind::Generic(2)), ("DataWatch", CoreLeafKind::Generic(1)), ("DataWatchStatus", CoreLeafKind::Plain), ("Group", CoreLeafKind::Generic(2)), ("DataAuthority", CoreLeafKind::Plain), ("DataColumn", CoreLeafKind::Plain), ("DataError", CoreLeafKind::Plain), ("DataErrorKind", CoreLeafKind::Enum(&["Syntax", "Missing", "Type", "Limit", "IO", "Unsupported"])), ("DataFormat", CoreLeafKind::Enum(&["Csv", "Json", "Jsonl", "Arrow", "Unknown"])), ("DataFreshness", CoreLeafKind::Enum(&["Live", "Cached", "Stale"])), ("DataInvalidationCause", CoreLeafKind::Enum(&["Write", "Schema", "Manual"])), ("DataLimits", CoreLeafKind::Plain), ("DataLineOptions", CoreLeafKind::Plain), ("DataLoaderStatus", CoreLeafKind::Enum(&["Idle", "Ready", "Failed", "Offline"])), ("DataPivotCell", CoreLeafKind::Plain), ("DataProvenance", CoreLeafKind::Plain), ("DataSchema", CoreLeafKind::Plain), ("DataSnapshotIdentity", CoreLeafKind::Plain), ("DataSourceIdentity", CoreLeafKind::Plain), ("DataStatus", CoreLeafKind::Enum(&["Empty", "Ready", "Stale", "Error"])), ("JetDataPlotAccessibility", CoreLeafKind::Plain), ("JetDataPlotAggregate", CoreLeafKind::Enum(&["Sum", "Mean", "Count", "Min", "Max"])), ("JetDataPlotAxis", CoreLeafKind::Plain), ("JetDataPlotBackend", CoreLeafKind::Enum(&["Svg", "Text"])), ("JetDataPlotCapability", CoreLeafKind::Plain), ("JetDataPlotChannel", CoreLeafKind::Enum(&["X", "Y", "Color", "Size"])), ("JetDataPlotColumn", CoreLeafKind::Plain), ("JetDataPlotDomain", CoreLeafKind::Enum(&["Auto", "Explicit"])), ("JetDataPlotEncoding", CoreLeafKind::Plain), ("JetDataPlotError", CoreLeafKind::Plain), ("JetDataPlotErrorKind", CoreLeafKind::Enum(&["Schema", "Encoding", "Render"])), ("JetDataPlotFacet", CoreLeafKind::Plain), ("JetDataPlotFacetKind", CoreLeafKind::Enum(&["Wrap", "Grid"])), ("JetDataPlotField", CoreLeafKind::Plain), ("JetDataPlotFilterOp", CoreLeafKind::Enum(&["Eq", "Ne", "Lt", "Le", "Gt", "Ge"])), ("JetDataPlotInspection", CoreLeafKind::Plain), ("JetDataPlotInteraction", CoreLeafKind::Enum(&["Hover", "Select", "Pan"])), ("JetDataPlotLayer", CoreLeafKind::Plain), ("JetDataPlotLayout", CoreLeafKind::Plain), ("JetDataPlotLegend", CoreLeafKind::Plain), ("JetDataPlotLegendPosition", CoreLeafKind::Enum(&["Top", "Bottom", "Left", "Right"])), ("JetDataPlotMark", CoreLeafKind::Enum(&["Bar", "Line", "Point"])), ("JetDataPlotPlan", CoreLeafKind::Plain), ("JetDataPlotProjection", CoreLeafKind::Plain), ("JetDataPlotRender", CoreLeafKind::Plain), ("JetDataPlotRenderFormat", CoreLeafKind::Enum(&["Svg", "Text", "Json"])), ("JetDataPlotScale", CoreLeafKind::Plain), ("JetDataPlotScaleKind", CoreLeafKind::Enum(&["Linear", "Log", "Band"])), ("JetDataPlotSchema", CoreLeafKind::Plain), ("JetDataPlotSelectedRow", CoreLeafKind::Plain), ("JetDataPlotSourceFacts", CoreLeafKind::Plain), ("JetDataPlotSupport", CoreLeafKind::Enum(&["Full", "Partial", "Unsupported"])), ("JetDataPlotTransform", CoreLeafKind::Enum(&["Identity", "Filter", "Aggregate"])), ("JetDataPlotValue", CoreLeafKind::Enum(&["Number", "Text", "Bool"]))];
const CORE_MODULE_21_DEPENDENCIES: &[&str] = &["core.encoding", "core.files"];

const CORE_MODULE_22_MEMBERS: &[&str] = &["DataArrowBatch", "import", "query"];
const CORE_MODULE_22_TYPES: &[(&str, CoreLeafKind)] = &[("DataArrowBatch", CoreLeafKind::Generic(1))];
const CORE_MODULE_22_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_23_MEMBERS: &[&str] = &["authority", "bind", "bind_text", "cancel", "generation", "invalidate", "needs_refresh", "offline", "ready", "refresh", "snapshot_reusable", "source_identity", "status", "stream", "text", "Loader"];
const CORE_MODULE_23_TYPES: &[(&str, CoreLeafKind)] = &[("Loader", CoreLeafKind::Plain)];
const CORE_MODULE_23_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_24_MEMBERS: &[&str] = &["cancel", "collect", "is_empty", "len", "next", "skip", "take", "Stream", "from_items"];
const CORE_MODULE_24_TYPES: &[(&str, CoreLeafKind)] = &[("Stream", CoreLeafKind::Plain)];
const CORE_MODULE_24_DEPENDENCIES: &[&str] = &["core.data.loader"];

const CORE_MODULE_25_MEMBERS: &[&str] = &["JetDataPlotAccessibility", "JetDataPlotAggregate", "JetDataPlotAxis", "JetDataPlotBackend", "JetDataPlotCapability", "JetDataPlotChannel", "JetDataPlotColumn", "JetDataPlotDomain", "JetDataPlotEncoding", "JetDataPlotError", "JetDataPlotErrorKind", "JetDataPlotFacet", "JetDataPlotFacetKind", "JetDataPlotField", "JetDataPlotFilterOp", "JetDataPlotInspection", "JetDataPlotInteraction", "JetDataPlotLayer", "JetDataPlotLayout", "JetDataPlotLegend", "JetDataPlotLegendPosition", "JetDataPlotMark", "JetDataPlotPlan", "JetDataPlotProjection", "JetDataPlotRender", "JetDataPlotRenderFormat", "JetDataPlotScale", "JetDataPlotScaleKind", "JetDataPlotSchema", "JetDataPlotSelectedRow", "JetDataPlotSourceFacts", "JetDataPlotSupport", "JetDataPlotTransform", "JetDataPlotValue", "bar_svg", "bar_text", "inspect", "inspect_json", "line_svg", "line_text", "plot", "render", "show", "svg", "text"];
const CORE_MODULE_25_TYPES: &[(&str, CoreLeafKind)] = &[("JetDataPlotAccessibility", CoreLeafKind::Plain), ("JetDataPlotAggregate", CoreLeafKind::Enum(&["Sum", "Mean", "Count", "Min", "Max"])), ("JetDataPlotAxis", CoreLeafKind::Plain), ("JetDataPlotBackend", CoreLeafKind::Enum(&["Svg", "Text"])), ("JetDataPlotCapability", CoreLeafKind::Plain), ("JetDataPlotChannel", CoreLeafKind::Enum(&["X", "Y", "Color", "Size"])), ("JetDataPlotColumn", CoreLeafKind::Plain), ("JetDataPlotDomain", CoreLeafKind::Enum(&["Auto", "Explicit"])), ("JetDataPlotEncoding", CoreLeafKind::Plain), ("JetDataPlotError", CoreLeafKind::Plain), ("JetDataPlotErrorKind", CoreLeafKind::Enum(&["Schema", "Encoding", "Render"])), ("JetDataPlotFacet", CoreLeafKind::Plain), ("JetDataPlotFacetKind", CoreLeafKind::Enum(&["Wrap", "Grid"])), ("JetDataPlotField", CoreLeafKind::Plain), ("JetDataPlotFilterOp", CoreLeafKind::Enum(&["Eq", "Ne", "Lt", "Le", "Gt", "Ge"])), ("JetDataPlotInspection", CoreLeafKind::Plain), ("JetDataPlotInteraction", CoreLeafKind::Enum(&["Hover", "Select", "Pan"])), ("JetDataPlotLayer", CoreLeafKind::Plain), ("JetDataPlotLayout", CoreLeafKind::Plain), ("JetDataPlotLegend", CoreLeafKind::Plain), ("JetDataPlotLegendPosition", CoreLeafKind::Enum(&["Top", "Bottom", "Left", "Right"])), ("JetDataPlotMark", CoreLeafKind::Enum(&["Bar", "Line", "Point"])), ("JetDataPlotPlan", CoreLeafKind::Plain), ("JetDataPlotProjection", CoreLeafKind::Plain), ("JetDataPlotRender", CoreLeafKind::Plain), ("JetDataPlotRenderFormat", CoreLeafKind::Enum(&["Svg", "Text", "Json"])), ("JetDataPlotScale", CoreLeafKind::Plain), ("JetDataPlotScaleKind", CoreLeafKind::Enum(&["Linear", "Log", "Band"])), ("JetDataPlotSchema", CoreLeafKind::Plain), ("JetDataPlotSelectedRow", CoreLeafKind::Plain), ("JetDataPlotSourceFacts", CoreLeafKind::Plain), ("JetDataPlotSupport", CoreLeafKind::Enum(&["Full", "Partial", "Unsupported"])), ("JetDataPlotTransform", CoreLeafKind::Enum(&["Identity", "Filter", "Aggregate"])), ("JetDataPlotValue", CoreLeafKind::Enum(&["Number", "Text", "Bool"]))];
const CORE_MODULE_25_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_26_MEMBERS: &[&str] = &["empty", "merge"];
const CORE_MODULE_26_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_26_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_27_MEMBERS: &[&str] = &["new", "CountMin", "add", "estimate"];
const CORE_MODULE_27_TYPES: &[(&str, CoreLeafKind)] = &[("CountMin", CoreLeafKind::Plain)];
const CORE_MODULE_27_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_28_MEMBERS: &[&str] = &["new", "Hll", "add", "count"];
const CORE_MODULE_28_TYPES: &[(&str, CoreLeafKind)] = &[("Hll", CoreLeafKind::Plain)];
const CORE_MODULE_28_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_29_MEMBERS: &[&str] = &["new", "Reservoir", "add", "new_k", "sample"];
const CORE_MODULE_29_TYPES: &[(&str, CoreLeafKind)] = &[("Reservoir", CoreLeafKind::Plain)];
const CORE_MODULE_29_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_30_MEMBERS: &[&str] = &["new", "Centroid", "Packed", "TDigest", "add", "quantile"];
const CORE_MODULE_30_TYPES: &[(&str, CoreLeafKind)] = &[("Centroid", CoreLeafKind::Plain), ("Packed", CoreLeafKind::Plain), ("TDigest", CoreLeafKind::Plain)];
const CORE_MODULE_30_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_31_MEMBERS: &[&str] = &["decode", "migrate", "open", "open_memory", "pool", "policy", "policy_audit", "row_bool", "row_float", "row_int", "row_text", "row_value", "transaction", "DbHandle", "Pair", "Policy", "Pool", "Tx"];
const CORE_MODULE_31_TYPES: &[(&str, CoreLeafKind)] = &[("DbHandle", CoreLeafKind::Plain), ("Pair", CoreLeafKind::Plain), ("Policy", CoreLeafKind::Plain), ("Pool", CoreLeafKind::Plain), ("Tx", CoreLeafKind::Plain)];
const CORE_MODULE_31_DEPENDENCIES: &[&str] = &["core.files", "core.net"];

const CORE_MODULE_32_MEMBERS: &[&str] = &["Address", "Attachment", "DkimConfig", "EmailError", "Envelope", "Limits", "Mailer", "Message", "RecipientPolicy", "RecipientReport", "SMTPAuth", "SMTPConfig", "SMTPSecurity", "SendReport", "TLSTrust", "address", "attachment", "envelope", "message", "serialize", "smtp", "smtp_from_env", "HTML", "dkim", "limits", "send_report", "smtp_auth"];
const CORE_MODULE_32_TYPES: &[(&str, CoreLeafKind)] = &[("Address", CoreLeafKind::Plain), ("Message", CoreLeafKind::Plain), ("Attachment", CoreLeafKind::Plain), ("Envelope", CoreLeafKind::Plain), ("SMTPSecurity", CoreLeafKind::Plain), ("RecipientPolicy", CoreLeafKind::Plain), ("RecipientReport", CoreLeafKind::Plain), ("SendReport", CoreLeafKind::Plain), ("EmailError", CoreLeafKind::Plain), ("Limits", CoreLeafKind::Plain), ("SMTPAuth", CoreLeafKind::Plain), ("TLSTrust", CoreLeafKind::Plain), ("DkimConfig", CoreLeafKind::Plain), ("SMTPConfig", CoreLeafKind::Plain), ("Mailer", CoreLeafKind::Plain), ("HTML", CoreLeafKind::Plain)];
const CORE_MODULE_32_DEPENDENCIES: &[&str] = &["core.net", "core.text"];

const CORE_MODULE_33_MEMBERS: &[&str] = &["DataEvent", "DataTree", "EncodingCause", "EncodingError", "EncodingErrorKind", "EncodingFormat", "EncodingLimits", "Reader"];
const CORE_MODULE_33_TYPES: &[(&str, CoreLeafKind)] = &[("DataTree", CoreLeafKind::Plain), ("EncodingLimits", CoreLeafKind::Plain), ("EncodingError", CoreLeafKind::Plain), ("EncodingCause", CoreLeafKind::Plain), ("EncodingFormat", CoreLeafKind::Plain), ("EncodingErrorKind", CoreLeafKind::Plain), ("DataEvent", CoreLeafKind::Plain), ("Reader", CoreLeafKind::Plain)];
const CORE_MODULE_33_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_34_MEMBERS: &[&str] = &["decode", "encode", "is_base32", "b32encode", "b32decode", "b32hexencode", "b32hexdecode"];
const CORE_MODULE_34_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_34_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_35_MEMBERS: &[&str] = &["decode", "decode_url", "encode", "encode_url", "encode_url_padded", "decode_padded", "pad", "unpad", "is_base64", "b64encode", "standard_b64encode", "urlsafe_b64encode", "b64decode", "standard_b64decode", "urlsafe_b64decode", "b16encode", "b16decode", "b32encode", "b32decode", "b32hexencode", "b32hexdecode", "a85encode", "a85decode", "b85encode", "b85decode", "z85encode", "z85decode", "encodebytes", "decodebytes", "b2a_base64", "a2b_base64"];
const CORE_MODULE_35_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_35_DEPENDENCIES: &[&str] = &["core.encoding", "core.encoding.hex", "core.encoding.base32"];

const CORE_MODULE_36_MEMBERS: &[&str] = &["pack_u8", "pack_i8", "pack_u16le", "pack_u16be", "pack_u32le", "pack_u32be", "pack_u64le", "pack_u64be", "unpack_u8", "unpack_u16le", "unpack_u16be", "unpack_u32le", "unpack_u32be", "unpack_u64le", "unpack_u64be", "sign_extend", "pack_f64le", "pack_f64be", "unpack_f64le", "unpack_f64be", "calcsize", "pack", "unpack", "iter_unpack"];
const CORE_MODULE_36_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_36_DEPENDENCIES: &[&str] = &["core.math"];

const CORE_MODULE_37_MEMBERS: &[&str] = &["CBORError", "CBORErrorKind", "CBOROptions", "CBORReader", "CBORWriter", "decode", "parse", "reader", "to_bytes", "to_bytes_canonical", "writer"];
const CORE_MODULE_37_TYPES: &[(&str, CoreLeafKind)] = &[("CBORReader", CoreLeafKind::Plain), ("CBORWriter", CoreLeafKind::Plain), ("CBOROptions", CoreLeafKind::Plain), ("CBORError", CoreLeafKind::Plain), ("CBORErrorKind", CoreLeafKind::Plain)];
const CORE_MODULE_37_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_38_MEMBERS: &[&str] = &["CSVReader", "CSVRow", "CSVWriter", "decode", "parse", "query", "reader", "rows", "to_string", "writer", "dict_rows", "dict_get", "dict_get_or", "write_dict", "fieldnames"];
const CORE_MODULE_38_TYPES: &[(&str, CoreLeafKind)] = &[("CSVReader", CoreLeafKind::Plain), ("CSVWriter", CoreLeafKind::Plain), ("CSVRow", CoreLeafKind::Plain), ("DictRow", CoreLeafKind::Plain)];
const CORE_MODULE_38_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_39_MEMBERS: &[&str] = &["decode", "encode", "encode_prefixed", "encode_upper", "is_hex", "hexlify", "unhexlify", "b2a_hex", "a2b_hex", "crc_hqx", "crc32", "b2a_base64", "a2b_base64", "b2a_qp", "a2b_qp", "b2a_uu", "a2b_uu", "encode_sep", "dump"];
const CORE_MODULE_39_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_39_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_40_MEMBERS: &[&str] = &["Pair", "Section", "Ini", "empty", "parse", "to_string", "sections", "has_section", "has_option", "get", "get_or", "get_int", "get_bool", "get_float", "items", "options", "set", "remove_option", "remove_section", "defaults"];
const CORE_MODULE_40_TYPES: &[(&str, CoreLeafKind)] = &[("Pair", CoreLeafKind::Plain), ("Section", CoreLeafKind::Plain), ("Ini", CoreLeafKind::Plain)];
const CORE_MODULE_40_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_41_MEMBERS: &[&str] = &["JSONReader", "JSONWriter", "canonical", "decode", "dump", "dumps", "events", "load", "loads", "parse", "reader", "to_string", "to_string_pretty", "writer"];
const CORE_MODULE_41_TYPES: &[(&str, CoreLeafKind)] = &[("JSONReader", CoreLeafKind::Plain), ("JSONWriter", CoreLeafKind::Plain)];
const CORE_MODULE_41_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_42_MEMBERS: &[&str] = &["JSONLReader", "JSONLWriter", "parse", "reader", "to_string", "writer", "loads", "dumps", "count_rows", "append_line", "first"];
const CORE_MODULE_42_TYPES: &[(&str, CoreLeafKind)] = &[("JSONLReader", CoreLeafKind::Plain), ("JSONLWriter", CoreLeafKind::Plain)];
const CORE_MODULE_42_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_43_MEMBERS: &[&str] = &["decode", "load", "loads", "parse", "to_string"];
const CORE_MODULE_43_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_43_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_44_MEMBERS: &[&str] = &["XMLCanonical", "XMLCanonicalMode", "XMLEncoding", "XMLEntityPolicy", "XMLError", "XMLLexicalPolicy", "XMLLimits", "XMLParseOptions", "XMLReader", "XMLReason", "XMLRenderOptions", "XMLWriter", "attribute", "canonical", "content", "decode", "decode_bytes", "expanded_name", "parse", "parse_bytes", "parse_with", "reader", "root", "to_bytes", "to_string", "writer"];
const CORE_MODULE_44_TYPES: &[(&str, CoreLeafKind)] = &[("XMLReader", CoreLeafKind::Plain), ("XMLWriter", CoreLeafKind::Plain), ("XMLError", CoreLeafKind::Plain), ("XMLReason", CoreLeafKind::Plain), ("XMLCanonical", CoreLeafKind::Plain), ("XMLCanonicalMode", CoreLeafKind::Enum(&["Inclusive", "Exclusive"])), ("XMLEncoding", CoreLeafKind::Enum(&["Utf8", "Utf16"])), ("XMLEntityPolicy", CoreLeafKind::Enum(&["PredefinedOnly", "Reject"])), ("XMLLexicalPolicy", CoreLeafKind::Plain), ("XMLLimits", CoreLeafKind::Plain), ("XMLParseOptions", CoreLeafKind::Plain), ("XMLRenderOptions", CoreLeafKind::Plain)];
const CORE_MODULE_44_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_45_MEMBERS: &[&str] = &["decode", "parse", "to_string"];
const CORE_MODULE_45_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_45_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_46_MEMBERS: &[&str] = &["async_result", "clear", "contains", "decision_hook", "emit", "hook", "is_sync", "listen", "names", "new", "off", "once", "policy_async", "policy_sync", "scope", "with_policy", "EventScope", "Hook", "Policy", "len", "is_empty", "once_pending", "emit_all", "drain_once", "count", "rename", "merge"];
const CORE_MODULE_46_TYPES: &[(&str, CoreLeafKind)] = &[("EventScope", CoreLeafKind::Plain), ("Hook", CoreLeafKind::Plain), ("Policy", CoreLeafKind::Plain)];
const CORE_MODULE_46_DEPENDENCIES: &[&str] = &["core.mem"];

const CORE_MODULE_47_MEMBERS: &[&str] = &["absolute", "append", "append_all", "basename", "canonicalize", "close", "commonpath", "copy", "copy_dir", "create", "create_dir", "create_dir_all", "cwd", "getcwd", "dirname", "exists", "expanduser", "fnmatch", "fsync", "glob", "hard_link", "home", "is_absolute", "is_dir", "is_file", "is_symlink", "join", "list_dir", "listdir", "lock", "map", "open", "read", "read_at", "read_bytes", "read_lines", "read_link", "relative", "relocate", "remove", "remove_all", "remove_dir", "rename", "scope", "set_mode", "stat", "stem", "suffix", "symlink", "temp_dir", "temp_file", "touch", "walk", "walk_files", "walk_parallel", "which", "with_name", "with_suffix", "write", "write_at", "write_atomic", "write_bytes", "write_lines", "DirEntry", "FileLock", "FileReader", "FileScope", "FileWriter", "IOError", "MappedFile", "Stat", "TempDir", "TempFile", "WalkEntry", "collapse", "commonprefix", "copy2", "ensure_parent", "expandvars", "fnmatch_filter", "getmtime", "getsize", "glob_match", "glob_recursive", "is_abs", "is_mount", "lexists", "makedirs", "move", "normpath", "rmtree", "samefile", "split", "split_slash", "splitext", "replace", "relativeto", "chmod", "copyfile", "mkdir", "rmdir", "unlink", "joinpath", "chdir", "scandir", "gettempdir", "getenv", "chown", "is_fifo", "is_socket", "lstat", "mkdtemp", "mktemp", "readdir", "truncate", "walkdir", "createdirectory", "filesize", "delete", "cp", "rm", "realpath", "pwd", "tmpdir", "topath", "cd"];
const CORE_MODULE_47_TYPES: &[(&str, CoreLeafKind)] = &[("FileScope", CoreLeafKind::Plain), ("DirEntry", CoreLeafKind::Plain), ("FileLock", CoreLeafKind::Plain), ("FileReader", CoreLeafKind::Plain), ("FileWriter", CoreLeafKind::Plain), ("IOError", CoreLeafKind::Plain), ("MappedFile", CoreLeafKind::Plain), ("Stat", CoreLeafKind::Plain), ("TempDir", CoreLeafKind::Plain), ("TempFile", CoreLeafKind::Plain), ("WalkEntry", CoreLeafKind::Plain)];
const CORE_MODULE_47_DEPENDENCIES: &[&str] = &["core.text", "core.sys"];

const CORE_MODULE_48_MEMBERS: &[&str] = &["Path", "as_posix", "cwd", "exists", "glob", "home", "is_absolute", "is_dir", "is_file", "is_relative", "is_symlink", "iterdir", "join", "join_path", "mkdir", "name", "of", "parent", "parts", "read_bytes", "read_text", "relative_to", "rename", "replace", "resolve", "rmdir", "show", "stem", "suffix", "suffixes", "touch", "unlink", "with_name", "with_stem", "with_suffix", "write_bytes", "write_text", "absolute", "anchor", "append_text", "as_uri", "as_windows", "chmod", "collapse", "copy_file", "copy_into", "drive", "ensure_dir", "equals", "expanduser", "from_parts", "hardlink_to", "is_empty", "is_relative_to", "join_many", "match_glob", "match_path", "mtime", "open_read", "open_write", "parents", "read_lines", "readlink", "resolve_pure", "rglob", "rmtree", "root", "samefile", "size", "stat", "symlink_to", "walk", "which", "with_segments", "write_lines"];
const CORE_MODULE_48_TYPES: &[(&str, CoreLeafKind)] = &[("Path", CoreLeafKind::Plain)];
const CORE_MODULE_48_DEPENDENCIES: &[&str] = &["core.files"];

const CORE_MODULE_49_MEMBERS: &[&str] = &["FontFace", "FontStyle", "Glyph", "GlyphRun", "GlyphShaper", "shape", "system", "DecodePair", "shape_with"];
const CORE_MODULE_49_TYPES: &[(&str, CoreLeafKind)] = &[("FontFace", CoreLeafKind::Plain), ("FontStyle", CoreLeafKind::Enum(&["Body", "Title", "Monospace"])), ("Glyph", CoreLeafKind::Plain), ("GlyphRun", CoreLeafKind::Plain), ("GlyphShaper", CoreLeafKind::Enum(&["HarfBuzz", "HeadlessFallback"])), ("DecodePair", CoreLeafKind::Plain)];
const CORE_MODULE_49_DEPENDENCIES: &[&str] = &["core.ui"];

const CORE_MODULE_50_MEMBERS: &[&str] = &["Backend", "Replay", "Scene", "run", "ReplayFrame", "backend_null", "backend_raylib", "scene"];
const CORE_MODULE_50_TYPES: &[(&str, CoreLeafKind)] = &[("Backend", CoreLeafKind::Plain), ("Replay", CoreLeafKind::Plain), ("Scene", CoreLeafKind::Plain), ("ReplayFrame", CoreLeafKind::Plain)];
const CORE_MODULE_50_DEPENDENCIES: &[&str] = &["core.math", "core.mem"];

const CORE_MODULE_51_MEMBERS: &[&str] = &["begin_drawing", "clear_background", "close_window", "color", "draw_rectangle", "draw_sprite", "draw_text", "end_drawing", "gamepad_axis", "gamepad_down", "key_down", "load_sound", "load_texture_atlas", "play_sound", "set_target_fps", "window_open", "window_ready", "window_should_close", "RaylibColor", "RaylibWindow"];
const CORE_MODULE_51_TYPES: &[(&str, CoreLeafKind)] = &[("RaylibColor", CoreLeafKind::Plain), ("RaylibWindow", CoreLeafKind::Plain)];
const CORE_MODULE_51_DEPENDENCIES: &[&str] = &["core.game"];

const CORE_MODULE_52_MEMBERS: &[&str] = &["Body", "Cookie", "HTTPError", "HTTPRequest", "HTTPResponse", "Header", "Headers", "Method", "Query", "QueryPair", "Status", "StatusLine", "Version", "basic_auth", "bearer_auth", "body_as_bytes", "body_as_text", "body_bytes", "body_empty", "body_len", "body_text", "content_type", "cookie", "cookie_encode", "cookies_parse", "delete", "exchange", "form_decode", "form_encode", "get", "head", "headers", "headers_all", "headers_append", "headers_encode", "headers_get", "headers_parse", "headers_remove", "headers_set", "is_client_error", "is_informational", "is_redirect", "is_server_error", "is_success", "method_parse", "method_text", "parse_request", "parse_response", "patch", "post", "put", "query", "query_add", "query_all", "query_decode", "query_encode", "query_get", "query_set", "reason_phrase", "request", "send_request", "serve", "status", "status_bad_request", "status_created", "status_forbidden", "status_found", "status_moved", "status_no_content", "status_not_found", "status_ok", "status_server_error", "status_timeout", "status_unauthorized", "version_parse", "version_text", "with_body", "with_header", "with_query", "CorsPolicy", "HTTPMux", "HTTPRoute", "HTTPServerTls", "content_length", "has_header", "request_with", "response_json_headers", "response_ok", "response_text_headers", "status_accepted", "status_already_reported", "status_bad_gateway", "status_class", "status_conflict", "status_continue", "status_expectation_failed", "status_failed_dependency", "status_gateway_timeout", "status_gone", "status_http_version", "status_legal", "status_length_required", "status_locked", "status_method_not_allowed", "status_multi_status", "status_non_authoritative", "status_not_acceptable", "status_not_implemented", "status_not_modified", "status_partial", "status_payload_too_large", "status_payment_required", "status_permanent", "status_precondition_failed", "status_precondition_required", "status_range_unsat", "status_reset_content", "status_see_other", "status_switching", "status_teapot", "status_temporary", "status_too_early", "status_too_many", "status_unavailable", "status_unprocessable", "status_unsupported_media", "status_upgrade_required", "status_uri_too_long"];
const CORE_MODULE_52_TYPES: &[(&str, CoreLeafKind)] = &[("Body", CoreLeafKind::Enum(&["Empty", "Text", "Bytes"])), ("Cookie", CoreLeafKind::Plain), ("HTTPError", CoreLeafKind::Enum(&["InvalidMethod", "InvalidUrl", "InvalidHeader", "InvalidStatus", "BodyTooLarge", "InvalidFraming", "UnsupportedEncoding", "Timeout", "Redirect", "Protocol", "Transport", "Policy", "Cancelled", "Status"])), ("HTTPRequest", CoreLeafKind::Plain), ("HTTPResponse", CoreLeafKind::Plain), ("Header", CoreLeafKind::Plain), ("Headers", CoreLeafKind::Plain), ("Method", CoreLeafKind::Enum(&["Get", "Head", "Post", "Put", "Patch", "Delete", "Options", "Trace", "Connect", "Custom"])), ("Query", CoreLeafKind::Plain), ("QueryPair", CoreLeafKind::Plain), ("Status", CoreLeafKind::Plain), ("StatusLine", CoreLeafKind::Plain), ("Version", CoreLeafKind::Enum(&["Http10", "Http11", "Http2", "Http3"])), ("CorsPolicy", CoreLeafKind::Plain), ("HTTPMux", CoreLeafKind::Plain), ("HTTPRoute", CoreLeafKind::Plain), ("HTTPServerTls", CoreLeafKind::Plain)];
const CORE_MODULE_52_DEPENDENCIES: &[&str] = &["core.net", "core.text"];

const CORE_MODULE_53_MEMBERS: &[&str] = &["Client", "Proxy", "RedirectPolicy", "delete", "get", "head", "header", "patch", "post", "put", "request", "send", "set_header", "timeout_redirects", "session", "session_header", "session_timeout", "session_retries", "session_redirects", "session_auth", "session_proxy", "session_cookie", "cookie_header", "session_request", "session_get", "session_post", "session_put", "session_patch", "session_delete", "session_head", "session_json", "session_form", "redirect_limit", "with_proxy", "user_agent", "bearer", "accept", "content_type", "Session"];
const CORE_MODULE_53_TYPES: &[(&str, CoreLeafKind)] = &[("Session", CoreLeafKind::Plain), ("Client", CoreLeafKind::Plain), ("Proxy", CoreLeafKind::Plain), ("RedirectPolicy", CoreLeafKind::Enum(&["Disabled", "Limited", "All"]))];
const CORE_MODULE_53_DEPENDENCIES: &[&str] = &["core.http"];

const CORE_MODULE_54_MEMBERS: &[&str] = &["access_log", "bind", "cors", "cors_policy", "json", "mux", "request_id", "response", "serve", "serve_once", "serve_once_listener", "sse", "static_file", "static_file_range", "static_files", "tls"];
const CORE_MODULE_54_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_54_DEPENDENCIES: &[&str] = &["core.http"];

const CORE_MODULE_55_MEMBERS: &[&str] = &["StringBuf", "BytesBuf", "string_buf", "string_empty", "bytes_buf", "bytes_empty", "getvalue", "getbytes", "tell", "bytes_tell", "seek", "bytes_seek", "is_eof", "bytes_is_eof", "write", "write_line", "bytes_write", "read", "read_line", "read_all", "bytes_read", "truncate", "bytes_truncate", "flush", "close"];
const CORE_MODULE_55_TYPES: &[(&str, CoreLeafKind)] = &[("StringBuf", CoreLeafKind::Plain), ("BytesBuf", CoreLeafKind::Plain)];
const CORE_MODULE_55_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_56_MEMBERS: &[&str] = &["JobQueue", "JobPayload", "JobResult", "JobError", "JobQueueReceipt", "JobQueueClaim", "JobQueueDeliveryPolicy", "JobQueueEvent", "JobQueueRecord", "JobQueueState", "JobQueueStatus", "queue", "ack", "cancel", "claim", "enqueue", "fail", "named", "status"];
const CORE_MODULE_56_TYPES: &[(&str, CoreLeafKind)] = &[("JobQueue", CoreLeafKind::Plain), ("JobPayload", CoreLeafKind::Plain), ("JobResult", CoreLeafKind::Plain), ("JobError", CoreLeafKind::Plain), ("JobQueueReceipt", CoreLeafKind::Plain), ("JobQueueClaim", CoreLeafKind::Plain), ("JobQueueDeliveryPolicy", CoreLeafKind::Enum(&["AtLeastOnce"])), ("JobQueueEvent", CoreLeafKind::Plain), ("JobQueueRecord", CoreLeafKind::Plain), ("JobQueueState", CoreLeafKind::Enum(&["Queued", "Running", "Retrying", "Completed", "Failed", "DeadLettered", "Cancelled"])), ("JobQueueStatus", CoreLeafKind::Plain)];
const CORE_MODULE_56_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_57_MEMBERS: &[&str] = &["bool", "close", "counter", "critical", "debug", "debug_fields", "disable", "enabled", "enter", "error", "error_fields", "fatal", "field", "float", "flush", "info", "info_fields", "int", "otlp_file", "redact", "sample_every", "set_level", "set_sink", "set_trace_id", "setup", "span", "warn", "warn_fields", "LogField", "Pair", "LogRecord", "Formatter", "Handler", "record", "formatter", "handler", "handler_format", "format", "handle", "add", "group", "time", "clear", "log", "warning"];
const CORE_MODULE_57_TYPES: &[(&str, CoreLeafKind)] = &[("LogRecord", CoreLeafKind::Plain), ("Formatter", CoreLeafKind::Plain), ("Handler", CoreLeafKind::Plain), ("LogField", CoreLeafKind::Plain), ("Pair", CoreLeafKind::Plain)];
const CORE_MODULE_57_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_58_MEMBERS: &[&str] = &["abs", "acos", "acosh", "asin", "asinh", "atan", "atan2", "atanh", "binomial", "cbrt", "ceil", "checked_abs", "checked_add", "checked_div", "checked_mul", "checked_neg", "checked_pow", "checked_rem", "checked_sub", "clamp", "cmp", "copy", "copysign", "conj", "cos", "cosh", "cot", "decimal", "degrees", "digits", "div_mod", "div_rem", "e", "erf", "erfc", "exp", "exp2", "exp_m1", "factorial", "floor", "fma", "float32", "float64", "fract", "fraction", "frexp", "from_bits", "gamma", "gcd", "hypot", "ilogb", "imag", "infinity", "int_pow", "inv", "is_canonical", "is_even", "is_finite", "is_inf", "is_integer", "is_nan", "is_normal", "is_odd", "is_signed", "is_subnormal", "is_zero", "isqrt", "lcm", "ldexp", "leading_ones", "lerp", "lgamma", "ln", "ln_1p", "log", "log10", "log2", "logb", "max", "min", "modf", "muladd", "nan", "next_after", "nextafter", "next_down", "next_up", "pi", "pow", "radians", "radix", "real", "round", "saturating_add", "saturating_mul", "saturating_sub", "scaleb", "sign", "sign_bit", "significand", "signum", "sin", "sin_cos", "sinh", "sqrt", "tan", "tanh", "tau", "to_bits", "trailing_ones", "trunc", "truncate", "ulp", "zero", "Fraction", "abs_diff", "abs_float", "clamp_float", "comb", "dist", "even", "fmod", "fsum", "gcd_many", "hypot3", "in_range", "isclose", "isfinite", "isinf", "isnan", "lcm_many", "max_float", "midpoint", "min_float", "odd", "perm", "powmod", "prod", "prod_int", "remainder", "sum_int", "sumprod", "tau_const", "log1p", "xor", "random", "fabs", "expm1"];
const CORE_MODULE_58_TYPES: &[(&str, CoreLeafKind)] = &[("Fraction", CoreLeafKind::Plain)];
const CORE_MODULE_58_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_59_MEMBERS: &[&str] = &["bool", "bytes", "exponential", "float", "float_range", "int", "normal", "pick", "rng", "sample", "seed", "shuffle", "split", "weighted_pick", "random", "randint", "uniform", "normalvariate", "gauss", "expovariate", "randbytes", "getrandbits", "randrange", "choice", "choices", "triangular", "gammavariate", "betavariate", "lognormvariate", "paretovariate", "weibullvariate", "vonmisesvariate", "binomialvariate"];
const CORE_MODULE_59_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_59_DEPENDENCIES: &[&str] = &["core.math"];

const CORE_MODULE_60_MEMBERS: &[&str] = &["binomial", "cartesian", "combinations", "combinations_count", "combinations_with_replacement", "factorial", "permutations", "permutations_count", "product", "accumulate", "batched", "chain", "chain_from", "compress", "count_from", "count", "cycle", "drop", "dropwhile", "filterfalse", "groupby", "islice", "starmap", "tee", "falling_factorial", "flatten", "multinomial", "ncr", "npr", "pairwise", "powerset", "repeat", "reverse", "rising_factorial", "take", "takewhile", "unique", "windows", "zip_longest"];
const CORE_MODULE_60_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_60_DEPENDENCIES: &[&str] = &["core.math"];

const CORE_MODULE_61_MEMBERS: &[&str] = &["NormalDist", "SlopeIntercept", "correlation", "covariance", "cumsum", "fmean", "geometric_mean", "harmonic_mean", "kde", "kde_random", "linear_regression", "max", "mean", "median", "median_grouped", "median_high", "median_low", "min", "mode", "multimode", "percentile", "prod", "pstdev", "pvariance", "quantile", "range", "stdev", "sum", "variance", "zscore", "clip", "count", "covariance_population", "cumprod", "describe", "diff", "ewma", "histogram", "iqr", "kurtosis", "mad", "mean_abs_deviation", "moving_average", "pearson", "quantiles", "r_squared", "rank", "residuals", "skew", "spearman", "sumprod", "weighted_mean", "winsorize"];
const CORE_MODULE_61_TYPES: &[(&str, CoreLeafKind)] = &[("NormalDist", CoreLeafKind::Plain), ("SlopeIntercept", CoreLeafKind::Plain)];
const CORE_MODULE_61_DEPENDENCIES: &[&str] = &["core.math"];

const CORE_MODULE_62_MEMBERS: &[&str] = &["AllocError", "Arena", "Atomic", "Bump", "Fixed", "Pin", "Pool", "Ptr", "address_of", "from_addr", "pin", "volatile_read", "volatile_write"];
const CORE_MODULE_62_TYPES: &[(&str, CoreLeafKind)] = &[("AllocError", CoreLeafKind::Plain), ("Atomic", CoreLeafKind::Plain), ("Arena", CoreLeafKind::Plain), ("Bump", CoreLeafKind::Plain), ("Fixed", CoreLeafKind::Plain), ("Pin", CoreLeafKind::Plain), ("Pool", CoreLeafKind::Plain)];
const CORE_MODULE_62_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_63_MEMBERS: &[&str] = &["drop", "guard", "is_live", "pin_of", "release", "Guard"];
const CORE_MODULE_63_TYPES: &[(&str, CoreLeafKind)] = &[("Guard", CoreLeafKind::Plain)];
const CORE_MODULE_63_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_64_MEMBERS: &[&str] = &["is_loaded", "load", "path", "unload", "CompiledModule"];
const CORE_MODULE_64_TYPES: &[(&str, CoreLeafKind)] = &[("CompiledModule", CoreLeafKind::Plain)];
const CORE_MODULE_64_DEPENDENCIES: &[&str] = &["core.compiler", "core.files"];

const CORE_MODULE_65_MEMBERS: &[&str] = &["dns_a", "dns_a_at", "dns_aaaa", "dns_aaaa_at", "dns_ptr", "dns_srv", "dns_srv_at", "dns_srv_port", "dns_srv_priority", "dns_srv_target", "dns_srv_weight", "dns_txt", "dns_txt_at", "error_address", "error_message", "error_name", "error_operation", "error_os_code", "getservbyname", "getservbyport", "ip_addr", "ip_is_ipv4", "ip_to_string", "listener_local_socket_addr", "nodelay", "ready_readable", "ready_writable", "sendfile", "set_nodelay", "set_read_timeout", "set_timeout", "set_ttl", "set_write_timeout", "socket_addr", "socket_addr_parse", "socket_host", "socket_port", "socket_to_string", "socket_type", "tcp_accept", "tcp_close", "tcp_connect", "tcp_connect_addr", "tcp_connect_happy", "tcp_connect_timeout", "tcp_listen", "tcp_listen_addr", "tcp_local_addr", "tcp_local_socket_addr", "tcp_peer_addr", "tcp_peer_socket_addr", "tcp_read", "tcp_read_bytes", "tcp_read_text", "tcp_ready", "tcp_reply", "tcp_shutdown", "tcp_write", "tcp_write_all_bytes", "tcp_write_bytes", "tcp_write_text", "tls_close", "tls_connect", "tls_read", "tls_write", "ttl", "udp_bind", "udp_bind_addr", "udp_local_addr", "udp_packet_addr", "udp_packet_bytes", "udp_packet_data", "udp_packet_original_len", "udp_packet_truncated", "udp_receive", "udp_recv_from", "udp_send_bytes_to", "udp_send_to", "udp_set_timeout", "unix_accept", "unix_close", "unix_connect", "unix_listen", "unix_read", "unix_read_bytes", "unix_shutdown", "unix_write", "unix_write_all_bytes", "IOError", "SocketAddr", "SrvRecord", "TcpListener", "TcpStream", "TlsStream", "UdpPacket", "UdpSocket", "UnixListener", "UnixStream", "create_connection", "create_server", "send", "gethostbyname", "gethostbyaddr", "gethostname", "addressfamily"];
const CORE_MODULE_65_TYPES: &[(&str, CoreLeafKind)] = &[("IOError", CoreLeafKind::Plain), ("SocketAddr", CoreLeafKind::Plain), ("SrvRecord", CoreLeafKind::Plain), ("TcpListener", CoreLeafKind::Plain), ("TcpStream", CoreLeafKind::Plain), ("TlsStream", CoreLeafKind::Plain), ("UdpPacket", CoreLeafKind::Plain), ("UdpSocket", CoreLeafKind::Plain), ("UnixListener", CoreLeafKind::Plain), ("UnixStream", CoreLeafKind::Plain)];
const CORE_MODULE_65_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_66_MEMBERS: &[&str] = &["extension", "from_extension", "parse", "Mime"];
const CORE_MODULE_66_TYPES: &[(&str, CoreLeafKind)] = &[("Mime", CoreLeafKind::Plain)];
const CORE_MODULE_66_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_67_MEMBERS: &[&str] = &["ClientConfig", "ClientIdentity", "RootCertificates", "TLSCertificate", "TLSPeerIdentity", "TLSVersion", "client", "close", "read", "read_text", "write", "write_all", "write_text", "TLSStream", "sni", "peer_port", "unwrap", "version_name", "parse_version", "is_tls13", "is_tls12", "config", "with_alpn", "alpn_h2", "identity", "roots", "connect_host", "flags", "has_alpn"];
const CORE_MODULE_67_TYPES: &[(&str, CoreLeafKind)] = &[("ClientConfig", CoreLeafKind::Plain), ("ClientIdentity", CoreLeafKind::Plain), ("RootCertificates", CoreLeafKind::Plain), ("TLSCertificate", CoreLeafKind::Plain), ("TLSPeerIdentity", CoreLeafKind::Plain), ("TLSVersion", CoreLeafKind::Enum(&["TLS12", "TLS13"])), ("TLSStream", CoreLeafKind::Plain)];
const CORE_MODULE_67_DEPENDENCIES: &[&str] = &["core.crypto.random", "core.net"];

const CORE_MODULE_68_MEMBERS: &[&str] = &["data", "file", "from_parts", "parse", "percent_decode", "percent_encode", "query", "Url", "geturl", "unparse", "urljoin", "parse_qsl", "parse_qs", "urlencode", "split_fragment", "quote", "quote_from_bytes", "quote_plus", "unquote", "unquote_to_bytes", "unquote_plus", "urlparse", "urlsplit", "urlunparse", "urlunsplit", "urldefrag"];
const CORE_MODULE_68_TYPES: &[(&str, CoreLeafKind)] = &[("Url", CoreLeafKind::Plain)];
const CORE_MODULE_68_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_69_MEMBERS: &[&str] = &["connect", "upgrade", "WsSocket"];
const CORE_MODULE_69_TYPES: &[(&str, CoreLeafKind)] = &[("WsSocket", CoreLeafKind::Plain)];
const CORE_MODULE_69_DEPENDENCIES: &[&str] = &["core.net"];

const CORE_MODULE_70_MEMBERS: &[&str] = &["IPv4", "IPv6", "Network", "is_global", "is_link_local", "is_loopback", "is_multicast", "is_private", "is_reserved", "is_unspecified", "ipv4", "ipv4_from_int", "ipv4_int", "ipv4_to_string", "ipv6_is_link_local", "ipv6_is_loopback", "ipv6_is_unspecified", "ipv6_to_string", "network", "network_broadcast", "network_contains", "network_hosts", "parse_ipv4", "parse_ipv6", "IPv4Interface", "compare_ipv4", "from_packed_ipv4", "hosts", "interface", "ipv4_equals", "ipv6_compressed", "ipv6_is_global", "ipv6_is_multicast", "ipv6_is_private", "ipv6_packed", "is_benchmarking", "is_carrier_grade_nat", "is_documentation", "is_shared", "network_first", "network_last", "network_overlaps", "num_addresses", "packed_ipv4", "reverse_pointer", "subnet_of", "subnets", "supernet", "supernet_of", "with_hostmask", "with_netmask", "with_prefixlen"];
const CORE_MODULE_70_TYPES: &[(&str, CoreLeafKind)] = &[("IPv4", CoreLeafKind::Plain), ("IPv6", CoreLeafKind::Plain), ("IPv4Interface", CoreLeafKind::Plain), ("Network", CoreLeafKind::Plain)];
const CORE_MODULE_70_DEPENDENCIES: &[&str] = &["core.net"];

const CORE_MODULE_71_MEMBERS: &[&str] = &["Perf", "default_fidelity", "fidelity", "is_full", "is_low", "of", "override_fidelity", "reset_fidelity", "scale"];
const CORE_MODULE_71_TYPES: &[(&str, CoreLeafKind)] = &[("Perf", CoreLeafKind::Plain)];
const CORE_MODULE_71_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_72_MEMBERS: &[&str] = &["call", "is_loaded", "load", "path", "unload", "Plugin"];
const CORE_MODULE_72_TYPES: &[(&str, CoreLeafKind)] = &[("Plugin", CoreLeafKind::Plain)];
const CORE_MODULE_72_DEPENDENCIES: &[&str] = &["core.files", "core.process"];

const CORE_MODULE_73_MEMBERS: &[&str] = &["keep", "always", "identity", "identity_int", "identity_float", "identity_bool", "const_int", "const_bool", "not_bool", "min_int", "max_int"];
const CORE_MODULE_73_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_73_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_74_MEMBERS: &[&str] = &["ProcessSignal", "args", "argv", "check", "cmd", "current_pid", "cwd", "env", "env_get", "exit", "on_signal", "pipeline", "run", "run_spec", "status_ok", "stdin_text", "which", "Completed", "ProcessReceipt", "ProcessSpec", "arg", "args_extend", "call", "capture", "check_call", "check_output", "combined_output", "env_get_or", "env_keys", "env_set", "env_truthy", "failed", "getoutput", "getstatusoutput", "list2cmdline", "shell", "signal_number", "stderr_lines", "stdout_lines", "exited"];
const CORE_MODULE_74_TYPES: &[(&str, CoreLeafKind)] = &[("ProcessSignal", CoreLeafKind::Enum(&["Interrupt", "Terminate", "Hangup", "Child", "User1", "User2"])), ("Completed", CoreLeafKind::Plain), ("ProcessReceipt", CoreLeafKind::Plain), ("ProcessSpec", CoreLeafKind::Plain)];
const CORE_MODULE_74_DEPENDENCIES: &[&str] = &["core.args", "core.term"];

const CORE_MODULE_75_MEMBERS: &[&str] = &["computed", "computed_get", "computed_set", "derived", "effect", "effect_last", "effect_run", "get", "set", "signal", "update", "version", "Computed", "Effect", "Signal"];
const CORE_MODULE_75_TYPES: &[(&str, CoreLeafKind)] = &[("Computed", CoreLeafKind::Plain), ("Effect", CoreLeafKind::Plain), ("Signal", CoreLeafKind::Plain)];
const CORE_MODULE_75_DEPENDENCIES: &[&str] = &["core.mem"];

const CORE_MODULE_76_MEMBERS: &[&str] = &["failed", "idle", "loaded", "loading", "Loadable"];
const CORE_MODULE_76_TYPES: &[(&str, CoreLeafKind)] = &[("Loadable", CoreLeafKind::Enum(&["Idle", "Loading", "Loaded", "Failed"]))];
const CORE_MODULE_76_DEPENDENCIES: &[&str] = &["core.reactive"];

const CORE_MODULE_77_MEMBERS: &[&str] = &["of", "ReflectValue", "inspect"];
const CORE_MODULE_77_TYPES: &[(&str, CoreLeafKind)] = &[("ReflectValue", CoreLeafKind::Plain)];
const CORE_MODULE_77_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_78_MEMBERS: &[&str] = &["compile", "compile_with", "escape", "find", "find_all", "finditer", "flags", "full_match", "is_match", "match", "matches", "purge", "replace", "replace_first", "split", "split_limit", "Match", "Node", "Pattern", "Regex", "RegexFlag", "RegexFlags", "expand", "regex", "join", "search", "findall", "fullmatch", "sub", "subn"];
const CORE_MODULE_78_TYPES: &[(&str, CoreLeafKind)] = &[("Pattern", CoreLeafKind::Plain), ("RegexFlag", CoreLeafKind::Plain), ("Match", CoreLeafKind::Plain), ("Node", CoreLeafKind::Plain), ("Regex", CoreLeafKind::Plain), ("RegexFlags", CoreLeafKind::Plain)];
const CORE_MODULE_78_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_79_MEMBERS: &[&str] = &["callback", "channels", "is_running", "period", "start", "stop", "RealtimeStream"];
const CORE_MODULE_79_TYPES: &[(&str, CoreLeafKind)] = &[("RealtimeStream", CoreLeafKind::Plain)];
const CORE_MODULE_79_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_80_MEMBERS: &[&str] = &["Delivery", "DeliveryEvent", "DeliveryReceipt", "DeliveryState", "ServiceDelivery", "ServiceEndpoint", "ServiceError", "ServiceRestart", "ServiceRuntime", "ServiceStateStore", "ServiceTree", "ServiceUpgradeReceipt", "ServiceWorkflow", "TaskOutcome", "TaskStatus", "delivery_at_most_once", "delivery_durable", "restart_one_for_all", "restart_one_for_one", "restart_rest_for_one", "runtime", "state_store", "tree", "tree_show", "workflow_start", "DeliveryStateWrap", "deliver", "delivery", "delivery_event", "endpoint", "outcome", "upgrade", "workflow_advance", "workflow_finish"];
const CORE_MODULE_80_TYPES: &[(&str, CoreLeafKind)] = &[("Delivery", CoreLeafKind::Plain), ("DeliveryEvent", CoreLeafKind::Plain), ("DeliveryReceipt", CoreLeafKind::Plain), ("DeliveryState", CoreLeafKind::Enum(&["Pending", "Delivered", "Failed"])), ("ServiceDelivery", CoreLeafKind::Plain), ("ServiceEndpoint", CoreLeafKind::Plain), ("ServiceError", CoreLeafKind::Enum(&["Missing", "Failed", "Policy"])), ("ServiceRestart", CoreLeafKind::Enum(&["OneForOne", "OneForAll", "RestForOne"])), ("ServiceRuntime", CoreLeafKind::Plain), ("ServiceStateStore", CoreLeafKind::Plain), ("ServiceTree", CoreLeafKind::Plain), ("ServiceUpgradeReceipt", CoreLeafKind::Plain), ("ServiceWorkflow", CoreLeafKind::Plain), ("TaskOutcome", CoreLeafKind::Enum(&["Ok", "Err"])), ("TaskStatus", CoreLeafKind::Enum(&["Idle", "Running", "Succeeded", "Failed"])), ("DeliveryStateWrap", CoreLeafKind::Plain)];
const CORE_MODULE_80_DEPENDENCIES: &[&str] = &["core.net", "core.tasks"];

const CORE_MODULE_81_MEMBERS: &[&str] = &["RowPolicy", "SyncCounter", "SyncList", "SyncMap", "SyncText", "counter_inc", "counter_merge", "counter_new", "counter_value", "list_merge", "list_new", "list_push", "list_show", "map_get", "map_merge", "map_new", "map_set", "map_show", "policy_allows", "policy_new", "policy_show", "text_edit", "text_merge", "text_metadata", "text_new", "text_set", "text_show", "Lww", "RgaItem"];
const CORE_MODULE_81_TYPES: &[(&str, CoreLeafKind)] = &[("RowPolicy", CoreLeafKind::Plain), ("SyncCounter", CoreLeafKind::Plain), ("SyncList", CoreLeafKind::Plain), ("SyncMap", CoreLeafKind::Plain), ("SyncText", CoreLeafKind::Plain), ("Lww", CoreLeafKind::Plain), ("RgaItem", CoreLeafKind::Plain)];
const CORE_MODULE_81_DEPENDENCIES: &[&str] = &["core.data", "core.tasks"];

const CORE_MODULE_82_MEMBERS: &[&str] = &["arch", "atexit", "close_fd", "cpu_count", "current_dir", "decode", "executable", "exitcode", "expand", "family", "fork", "get", "getegid", "geteuid", "getgid", "getgroups", "getpgid", "getpgrp", "getpid", "getppid", "getpriority", "getsid", "getuid", "home_dir", "hostname", "initgroups", "kill", "loadavg", "mkfifo", "name", "on_interrupt", "pid", "pipe", "release", "set", "set_current_dir", "setgid", "setpgid", "setpgrp", "setpriority", "setsid", "setuid", "stop", "success", "sync", "temp_dir", "times", "umask", "unset", "uptime", "username", "utime", "vars", "version", "wait", "waitpid", "EnvError", "platform", "is_windows", "is_unix", "is_linux", "is_macos", "uname", "sysname", "machine", "getenv", "pathsep", "linesep"];
const CORE_MODULE_82_TYPES: &[(&str, CoreLeafKind)] = &[("EnvError", CoreLeafKind::Plain)];
const CORE_MODULE_82_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_83_MEMBERS: &[&str] = &["after", "current_task", "interval", "yield_now", "Receiver", "TaskState", "TaskFailure", "delay_ms", "is_interval", "is_timer", "recv", "get", "result", "wait", "channel", "put", "clear", "shutdown", "stop", "reset", "lock", "acquire", "release", "notify", "exception", "start", "run", "waitall", "waitany"];
const CORE_MODULE_83_TYPES: &[(&str, CoreLeafKind)] = &[("Receiver", CoreLeafKind::Plain), ("TaskState", CoreLeafKind::Plain), ("TaskFailure", CoreLeafKind::Plain)];
const CORE_MODULE_83_DEPENDENCIES: &[&str] = &["core.time"];

const CORE_MODULE_84_MEMBERS: &[&str] = &["Reader", "Writer", "binread", "binwrite", "buffered", "choose", "confirm", "eprint", "input", "input_secret", "print", "progress", "read_all_input", "read_key", "read_until", "readline", "stderr", "stdin", "stdout", "style", "style_force", "take", "terminal_height", "terminal_width", "Key", "Stderr", "StdinHandle", "Stdout", "WinSize"];
const CORE_MODULE_84_TYPES: &[(&str, CoreLeafKind)] = &[("Reader", CoreLeafKind::Plain), ("Writer", CoreLeafKind::Plain), ("Key", CoreLeafKind::Enum(&["Char", "Escape", "Backspace", "Tab", "Up", "Down", "Left", "Right", "Unknown"])), ("Stderr", CoreLeafKind::Plain), ("StdinHandle", CoreLeafKind::Plain), ("Stdout", CoreLeafKind::Plain), ("WinSize", CoreLeafKind::Plain)];
const CORE_MODULE_84_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_85_MEMBERS: &[&str] = &["assert_equal", "compare", "corpus", "fake_clock", "fake_data", "fake_rng", "fixture", "golden", "histories", "snap", "status", "temp_dir", "test_suite", "world", "Clock", "Fake", "Rng", "TestSuite"];
const CORE_MODULE_85_TYPES: &[(&str, CoreLeafKind)] = &[("Count", CoreLeafKind::Plain), ("DeterministicWorld", CoreLeafKind::Plain), ("EventId", CoreLeafKind::Plain), ("HandleId", CoreLeafKind::Plain), ("HistoryBounds", CoreLeafKind::Plain), ("HistoryCase", CoreLeafKind::Plain), ("HistoryDistribution", CoreLeafKind::Plain), ("HistoryOperation", CoreLeafKind::Plain), ("HistoryPrecondition", CoreLeafKind::Plain), ("HistoryRng", CoreLeafKind::Plain), ("HistoryScheduleChoice", CoreLeafKind::Plain), ("HistoryStrategy", CoreLeafKind::Generic(1)), ("HistoryValue", CoreLeafKind::Plain), ("TaskId", CoreLeafKind::Plain), ("TypedHistoryCase", CoreLeafKind::Generic(1)), ("TestComparison", CoreLeafKind::Plain), ("Clock", CoreLeafKind::Plain), ("Fake", CoreLeafKind::Plain), ("Rng", CoreLeafKind::Plain), ("TestSuite", CoreLeafKind::Plain)];
const CORE_MODULE_85_DEPENDENCIES: &[&str] = &["core.files", "core.math.random", "core.time"];

const CORE_MODULE_86_MEMBERS: &[&str] = &["Cursor", "byte_count", "byte_views", "casefold", "caseless_eq", "center", "char_indices", "display_width", "ends_any", "grapheme_views", "graphemes", "inspect", "is_alphabetic", "is_ascii", "is_numeric", "is_whitespace", "line_views", "lower", "nfc", "nfd", "nfkc", "nfkd", "pad_end", "pad_start", "rsplitn", "scalar_count", "scalars", "sentences", "splitn", "starts_any", "trim", "trim_end", "trim_start", "upper", "word_views", "words", "IndexedCp", "cursor", "cursor_advance"];
const CORE_MODULE_86_TYPES: &[(&str, CoreLeafKind)] = &[("Cursor", CoreLeafKind::Plain), ("IndexedCp", CoreLeafKind::Plain)];
const CORE_MODULE_86_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_87_MEMBERS: &[&str] = &["bin", "bytes", "decimal", "duration", "grouped", "hex", "number", "oct", "ordinal", "pad", "pad_center", "pad_left", "pad_right", "percent", "plural", "pretty", "sci"];
const CORE_MODULE_87_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_87_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_88_MEMBERS: &[&str] = &["escape", "escape_quote", "unescape", "strip_tags", "unescape_and_strip", "attr_escape", "text_escape"];
const CORE_MODULE_88_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_88_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_89_MEMBERS: &[&str] = &["dedent", "expand_tabs", "fill", "html_escape", "html_unescape", "indent", "shorten", "wrap", "Wrapper", "fill_with", "hanging_indent", "indent_with", "wrap_paragraphs", "wrap_with", "wrapper"];
const CORE_MODULE_89_TYPES: &[(&str, CoreLeafKind)] = &[("Wrapper", CoreLeafKind::Plain)];
const CORE_MODULE_89_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_90_MEMBERS: &[&str] = &["count", "ends_with", "find", "is_alnum", "is_digit", "parse", "parse_float", "parse_int", "parse_int_base", "partition", "replace", "rfind", "rpartition", "rsplit", "split", "split_once", "starts_with", "strip_prefix", "strip_suffix", "capitalize", "capwords", "center", "contains", "escape_c", "find_from", "index", "is_ascii", "is_identifier", "is_lower", "is_space", "is_title", "is_upper", "join", "ljust", "lower", "lstrip", "parse_bool", "parse_kv", "rfind_from", "rjust", "rstrip", "split_ws", "splitlines", "strip", "swapcase", "title", "unescape_c", "upper", "zfill", "startswith", "endswith", "removeprefix", "removesuffix", "rindex", "encode", "isalpha", "isdecimal", "isnumeric", "isalnum", "isascii", "isdigit", "isidentifier", "islower", "isspace", "istitle", "isprintable", "isupper", "expandtabs"];
const CORE_MODULE_90_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_90_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_91_MEMBERS: &[&str] = &["datetime", "days_in_month", "from_iso_week", "from_timestamp", "from_unix_microseconds", "from_unix_ms", "from_unix_nanoseconds", "from_unix_seconds", "instant", "is_leap_year", "local_time", "new", "now", "now_utc", "parse", "parse_iso_week_date", "parse_rfc3339", "parse_time", "parse_zoned", "period", "period_days", "period_months", "period_years", "sleep", "sleep_until", "start", "time", "today", "utc", "zone", "zoned", "add_days", "add_duration", "compare_date", "date_equals", "duration_as_seconds", "duration_ms", "duration_ns", "duration_seconds", "elapsed", "isoformat", "isoformat_date", "isoformat_time", "since", "unix_ms", "unix_seconds", "weekday", "zoned_local", "Clock", "DateTime", "Duration", "Instant", "LocalDate", "LocalTime", "Period", "Stopwatch", "Zone", "ZonedDateTime", "add_hours", "add_minutes", "add_seconds", "between", "combine", "date_after", "date_before", "date_of", "days_between", "duration_abs", "duration_add", "duration_as_hours", "duration_as_minutes", "duration_as_ms", "duration_days", "duration_hours", "duration_is_zero", "duration_minutes", "duration_sub", "duration_zero", "end_of_day", "isoweekday", "max_date", "min_date", "replace_date", "replace_time", "start_of_day", "time_of", "weekday_sun0", "asctime", "ctime", "gmtime", "strftime", "utcoffset", "fromhours", "before", "after", "dayofweek", "todate", "totime"];
const CORE_MODULE_91_TYPES: &[(&str, CoreLeafKind)] = &[("Clock", CoreLeafKind::Plain), ("DateTime", CoreLeafKind::Plain), ("Duration", CoreLeafKind::Plain), ("Instant", CoreLeafKind::Plain), ("LocalDate", CoreLeafKind::Plain), ("LocalTime", CoreLeafKind::Plain), ("Period", CoreLeafKind::Plain), ("Stopwatch", CoreLeafKind::Plain), ("Zone", CoreLeafKind::Plain), ("ZonedDateTime", CoreLeafKind::Plain)];
const CORE_MODULE_91_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_92_MEMBERS: &[&str] = &["isleap", "leapdays", "weekday", "monthrange", "monthcalendar", "monthcalendar_start", "yearcalendar", "day_name", "day_abbr", "month_name", "month_abbr", "weekheader", "formatmonth", "formatyear", "timegm"];
const CORE_MODULE_92_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_92_DEPENDENCIES: &[&str] = &["core.time"];

const CORE_MODULE_93_MEMBERS: &[&str] = &["expired", "remaining_ms"];
const CORE_MODULE_93_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_93_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_94_MEMBERS: &[&str] = &["aria_role_button", "aria_role_container", "aria_role_label", "aria_role_text_input", "box", "button", "constraint", "desktop", "gtk_backend", "key_event", "mount", "node", "node_accessibility", "node_color", "node_role", "node_shortcut", "null_backend", "phone", "point", "playground", "playgrounds", "preview", "previews", "reactive_render", "rect", "resize_event", "size", "tablet", "text", "text_input", "tui_backend", "UiAriaRole", "UiBackend", "UiConstraint", "UiImeMode", "UiKeyEvent", "UiMount", "UiNode", "UiPlayground", "UiPoint", "UiPreview", "UiPreviewDevice", "UiPreviewKind", "UiRect", "UiResizeEvent", "UiSize"];
const CORE_MODULE_94_TYPES: &[(&str, CoreLeafKind)] = &[("UiImeMode", CoreLeafKind::Enum(&["Native", "Disabled"])), ("UiPlayground", CoreLeafKind::Plain), ("UiPreview", CoreLeafKind::Plain), ("UiPreviewAccessibility", CoreLeafKind::Plain), ("UiPreviewAuthority", CoreLeafKind::Plain), ("UiPreviewContext", CoreLeafKind::Plain), ("UiPreviewDevice", CoreLeafKind::Enum(&["Phone", "Tablet", "Desktop"])), ("UiPreviewEffect", CoreLeafKind::Plain), ("UiPreviewInputOverride", CoreLeafKind::Plain), ("UiPreviewInputValue", CoreLeafKind::Enum(&["Text", "Bool", "Integer", "Float"])), ("UiPreviewKind", CoreLeafKind::Enum(&["Preview", "Playground"])), ("UiPreviewLifecycle", CoreLeafKind::Plain), ("UiPreviewRegistry", CoreLeafKind::Plain), ("UiPreviewSource", CoreLeafKind::Plain), ("UiPreviewTheme", CoreLeafKind::Plain), ("UiPreviewTraits", CoreLeafKind::Plain), ("UiPreviewViewport", CoreLeafKind::Plain), ("UiAriaRole", CoreLeafKind::Enum(&["Button", "Container", "Label", "TextInput", "Unset"])), ("UiBackend", CoreLeafKind::Plain), ("UiConstraint", CoreLeafKind::Plain), ("UiKeyEvent", CoreLeafKind::Plain), ("UiMount", CoreLeafKind::Plain), ("UiNode", CoreLeafKind::Plain), ("UiPoint", CoreLeafKind::Plain), ("UiRect", CoreLeafKind::Plain), ("UiResizeEvent", CoreLeafKind::Plain), ("UiSize", CoreLeafKind::Plain)];
const CORE_MODULE_94_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_95_MEMBERS: &[&str] = &["TuiCapabilities", "TuiColor", "TuiColorProfile", "TuiConstraint", "TuiDirection", "TuiEvent", "TuiListState", "TuiStyle", "ascii", "capabilities", "close_event", "color_ansi16", "color_ansi256", "color_rgb", "display_width", "fill", "focus_event", "horizontal", "interrupt_event", "io_event", "key_event", "key_event_modifiers", "layout", "length", "list", "list_state", "list_state_offset", "list_state_select", "list_state_selected", "max", "min", "percent", "resize_event", "style", "style_background", "style_bold", "style_dim", "style_foreground", "style_text", "style_underline", "table", "timer_event", "vertical", "DecodePair", "TuiFocusEvent", "TuiIoEvent", "TuiKeyEvent", "TuiLayout", "TuiResizeEvent", "TuiTimerEvent", "split"];
const CORE_MODULE_95_TYPES: &[(&str, CoreLeafKind)] = &[("TuiEvent", CoreLeafKind::Enum(&["Key", "Resize", "Timer", "Io", "Focus", "Interrupt", "Close"])), ("TuiColorProfile", CoreLeafKind::Enum(&["Ansi16", "Ansi256", "TrueColor", "Ascii"])), ("TuiColor", CoreLeafKind::Enum(&["Ansi16", "Ansi256", "Rgb"])), ("TuiCapabilities", CoreLeafKind::Plain), ("TuiStyle", CoreLeafKind::Plain), ("TuiConstraint", CoreLeafKind::Enum(&["Length", "Min", "Max", "Percent", "Fill"])), ("TuiDirection", CoreLeafKind::Enum(&["Horizontal", "Vertical"])), ("TuiListState", CoreLeafKind::Plain), ("DecodePair", CoreLeafKind::Plain), ("TuiFocusEvent", CoreLeafKind::Plain), ("TuiIoEvent", CoreLeafKind::Plain), ("TuiKeyEvent", CoreLeafKind::Plain), ("TuiLayout", CoreLeafKind::Plain), ("TuiResizeEvent", CoreLeafKind::Plain), ("TuiTimerEvent", CoreLeafKind::Plain)];
const CORE_MODULE_95_DEPENDENCIES: &[&str] = &["core.ui", "core.text"];

const CORE_MODULE_96_MEMBERS: &[&str] = &["accessibility", "capabilities", "file_filter", "file_filter_text", "fs_grant", "fs_rights_read", "fs_rights_read_write", "fs_rights_write", "open_file", "open_request", "save_file", "save_request", "shortcut", "UiAccessibilityTree", "UiCancellation", "UiCapability", "UiCapabilityFact", "UiCapabilityFacts", "UiFileDialogKind", "UiFileDialogRequest", "UiFileDialogSelection", "UiFileFilter", "UiFsAccess", "UiFsGrant", "UiFsRights", "UiGrantedPath", "UiHostError", "UiShortcut"];
const CORE_MODULE_96_TYPES: &[(&str, CoreLeafKind)] = &[("UiCapability", CoreLeafKind::Enum(&["FileDialog", "Clipboard", "Ime", "DragDrop", "Shortcuts", "Accessibility", "FontShaping"])), ("UiCapabilityFact", CoreLeafKind::Plain), ("UiCapabilityFacts", CoreLeafKind::Plain), ("UiCancellation", CoreLeafKind::Enum(&["User", "Closed", "Headless", "Superseded", "Programmatic"])), ("UiHostError", CoreLeafKind::Plain), ("UiServiceResult", CoreLeafKind::Plain), ("UiFileDialogKind", CoreLeafKind::Enum(&["Open", "Save"])), ("UiFsAccess", CoreLeafKind::Enum(&["Read", "Write"])), ("UiFsRights", CoreLeafKind::Plain), ("UiFsGrant", CoreLeafKind::Plain), ("UiGrantedPath", CoreLeafKind::Plain), ("UiFileFilter", CoreLeafKind::Plain), ("UiFileDialogRequest", CoreLeafKind::Plain), ("UiFileDialogSelection", CoreLeafKind::Plain), ("UiClipboardText", CoreLeafKind::Plain), ("UiClipboardWrite", CoreLeafKind::Plain), ("UiTextRange", CoreLeafKind::Plain), ("UiImePhase", CoreLeafKind::Enum(&["Start", "Update", "Commit", "Cancel"])), ("UiImeComposition", CoreLeafKind::Plain), ("UiImeEvent", CoreLeafKind::Plain), ("UiDragOperation", CoreLeafKind::Enum(&["Copy", "Move", "Link"])), ("UiDropItem", CoreLeafKind::Plain), ("UiDragPhase", CoreLeafKind::Enum(&["Enter", "Over", "Drop", "Leave", "Cancel"])), ("UiDragEvent", CoreLeafKind::Plain), ("UiShortcutModifier", CoreLeafKind::Enum(&["Control", "Alt", "Shift", "Meta"])), ("UiShortcutModifiers", CoreLeafKind::Plain), ("UiShortcut", CoreLeafKind::Plain), ("UiShortcutBinding", CoreLeafKind::Plain), ("UiShortcutDispatch", CoreLeafKind::Plain), ("UiAccessibilityState", CoreLeafKind::Plain), ("UiAccessibility", CoreLeafKind::Plain), ("UiNodeId", CoreLeafKind::Plain), ("UiAccessibilityProjection", CoreLeafKind::Plain), ("UiFileFilterResult", CoreLeafKind::Plain), ("UiFsGrantResult", CoreLeafKind::Plain), ("UiShortcutResult", CoreLeafKind::Plain), ("UiAccessibilityResult", CoreLeafKind::Plain), ("UiFileDialogResult", CoreLeafKind::Plain), ("UiClipboardTextResult", CoreLeafKind::Plain), ("UiClipboardWriteResult", CoreLeafKind::Plain), ("UiImeResult", CoreLeafKind::Plain), ("UiDragResult", CoreLeafKind::Plain), ("UiShortcutDispatchResult", CoreLeafKind::Plain), ("UiAccessibilityNodeResult", CoreLeafKind::Plain), ("UiAccessibilityAttachResult", CoreLeafKind::Plain), ("UiAccessibilityProjectionResult", CoreLeafKind::Plain), ("UiAccessibilityTree", CoreLeafKind::Plain)];
const CORE_MODULE_96_DEPENDENCIES: &[&str] = &["core.ui", "core.files"];

const CORE_MODULE_97_MEMBERS: &[&str] = &["clear", "is_empty", "read_text", "write_text"];
const CORE_MODULE_97_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_97_DEPENDENCIES: &[&str] = &["core.ui.host"];

const CORE_MODULE_98_MEMBERS: &[&str] = &["poll", "UiImeEvent", "UiImePhase"];
const CORE_MODULE_98_TYPES: &[(&str, CoreLeafKind)] = &[("UiImeEvent", CoreLeafKind::Plain), ("UiImePhase", CoreLeafKind::Enum(&["Start", "Update", "Commit", "Cancel"]))];
const CORE_MODULE_98_DEPENDENCIES: &[&str] = &["core.ui.host"];

const CORE_MODULE_99_MEMBERS: &[&str] = &["poll", "UiDropEvent"];
const CORE_MODULE_99_TYPES: &[(&str, CoreLeafKind)] = &[("UiDropEvent", CoreLeafKind::Plain)];
const CORE_MODULE_99_DEPENDENCIES: &[&str] = &["core.ui.host"];

const CORE_MODULE_100_MEMBERS: &[&str] = &["binding", "dispatch", "register", "UiBinding", "UiShortcutMap"];
const CORE_MODULE_100_TYPES: &[(&str, CoreLeafKind)] = &[("UiBinding", CoreLeafKind::Plain), ("UiShortcutMap", CoreLeafKind::Plain)];
const CORE_MODULE_100_DEPENDENCIES: &[&str] = &["core.ui.host"];

const CORE_MODULE_101_MEMBERS: &[&str] = &["attach", "project", "UiA11yNode"];
const CORE_MODULE_101_TYPES: &[(&str, CoreLeafKind)] = &[("UiA11yNode", CoreLeafKind::Plain)];
const CORE_MODULE_101_DEPENDENCIES: &[&str] = &["core.ui.host"];

const CORE_MODULE_102_MEMBERS: &[&str] = &["Measurement", "abs", "add", "centi", "convert", "div", "from", "giga", "kilo", "mega", "micro", "milli", "mul", "nano", "scale", "show", "si", "sub", "to_si", "metres", "kilometres", "grams", "kilograms", "seconds", "milliseconds", "bytes_of", "kibibytes", "equals", "ratio", "is_zero", "metre", "gram", "second", "byte_unit", "kibibyte", "mebibyte", "gibibyte"];
const CORE_MODULE_102_TYPES: &[(&str, CoreLeafKind)] = &[("Measurement", CoreLeafKind::Plain)];
const CORE_MODULE_102_DEPENDENCIES: &[&str] = &["core.math"];

const CORE_MODULE_103_MEMBERS: &[&str] = &["add", "contains", "files", "kind", "len", "port", "process_pid", "remove", "set", "target", "WatchHandle", "WatchSet"];
const CORE_MODULE_103_TYPES: &[(&str, CoreLeafKind)] = &[("WatchHandle", CoreLeafKind::Plain), ("WatchSet", CoreLeafKind::Plain)];
const CORE_MODULE_103_DEPENDENCIES: &[&str] = &["core.files", "core.process"];

const CORE_MODULE_104_MEMBERS: &[&str] = &["App", "Auth", "Context", "LiveQuery", "Mount", "Page", "Session", "app", "auth", "auth_oauth", "auth_routes", "auth_show", "form", "invalidate", "live", "live_get", "live_show", "live_stats", "on", "openapi", "page", "signal_push", "storage", "subscribe", "sync", "transact_invalidate", "value"];
const CORE_MODULE_104_TYPES: &[(&str, CoreLeafKind)] = &[("App", CoreLeafKind::Plain), ("Auth", CoreLeafKind::Plain), ("Context", CoreLeafKind::Plain), ("LiveQuery", CoreLeafKind::Plain), ("Mount", CoreLeafKind::Plain), ("Page", CoreLeafKind::Plain), ("Session", CoreLeafKind::Plain)];
const CORE_MODULE_104_DEPENDENCIES: &[&str] = &["core.http"];

const CORE_MODULE_105_MEMBERS: &[&str] = &["Browser", "BrowserAbilities", "BrowserContext", "BrowserError", "BrowserEvent", "BrowserFrame", "BrowserIntercept", "BrowserLocator", "BrowserLocked", "BrowserPage", "BrowserPrivacy", "BrowserProfile", "BrowserProtocol", "BrowserReceipt", "BrowserTimeout", "BrowserTrace", "begin_named", "config", "config_from_env", "connect", "connect_profile", "fixture_context", "fixture_page", "fixture_source", "generate_source", "locked", "profile", "report_add_case", "report_exit_code", "report_html", "report_json", "report_new", "report_text", "selected", "server_logs", "server_start", "server_stop", "server_url", "timeout", "watch_changed", "write_report", "BrowserReport", "BrowserServer", "BrowserTestCase", "BrowserTestConfig"];
const CORE_MODULE_105_TYPES: &[(&str, CoreLeafKind)] = &[("BrowserTestConfig", CoreLeafKind::Plain), ("BrowserTestSource", CoreLeafKind::Plain), ("BrowserTestAction", CoreLeafKind::Plain), ("BrowserTestSnapshot", CoreLeafKind::Plain), ("BrowserTestEventFact", CoreLeafKind::Plain), ("BrowserTestArtifact", CoreLeafKind::Plain), ("BrowserTestAttempt", CoreLeafKind::Plain), ("BrowserTestCase", CoreLeafKind::Plain), ("BrowserTestReport", CoreLeafKind::Plain), ("BrowserTestFixture", CoreLeafKind::Plain), ("BrowserTestServer", CoreLeafKind::Plain), ("Browser", CoreLeafKind::Plain), ("BrowserAbilities", CoreLeafKind::Plain), ("BrowserContext", CoreLeafKind::Plain), ("BrowserError", CoreLeafKind::Enum(&["Connect", "Headless", "Timeout", "Protocol", "Closed"])), ("BrowserEvent", CoreLeafKind::Plain), ("BrowserFrame", CoreLeafKind::Plain), ("BrowserIntercept", CoreLeafKind::Plain), ("BrowserLocator", CoreLeafKind::Plain), ("BrowserLocked", CoreLeafKind::Plain), ("BrowserPage", CoreLeafKind::Plain), ("BrowserPrivacy", CoreLeafKind::Enum(&["Default", "Isolated"])), ("BrowserProfile", CoreLeafKind::Plain), ("BrowserProtocol", CoreLeafKind::Enum(&["Cdp", "WebDriver"])), ("BrowserReceipt", CoreLeafKind::Plain), ("BrowserTimeout", CoreLeafKind::Plain), ("BrowserTrace", CoreLeafKind::Plain), ("BrowserReport", CoreLeafKind::Plain), ("BrowserServer", CoreLeafKind::Plain)];
const CORE_MODULE_105_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_106_MEMBERS: &[&str] = &["app", "for_app", "DevServer", "is_local", "url"];
const CORE_MODULE_106_TYPES: &[(&str, CoreLeafKind)] = &[("DevServer", CoreLeafKind::Plain)];
const CORE_MODULE_106_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_107_MEMBERS: &[&str] = &["action_error", "action_field_error", "action_form_error", "blur", "field", "html", "input", "input_exclude", "input_group", "input_rename", "input_replace", "new", "no_script", "set", "show", "submit", "typed", "typed_blur", "typed_cancel", "typed_decode_post", "typed_errors", "typed_focus", "typed_html", "typed_lifecycle", "typed_no_script", "typed_post", "typed_select_field", "typed_set", "typed_set_async_validator", "typed_set_action", "typed_show", "typed_state", "typed_submit", "typed_submit_async", "typed_validate", "typed_validate_async", "typed_validate_field", "typed_validation_render", "typed_submission_cancel", "typed_submission_wait", "typed_validation_cancel", "typed_validation_wait", "validate", "validate_async", "Pair", "WebForm", "WebFormActionError", "WebFormField", "WebFormStatus", "WebFormValueType"];
const CORE_MODULE_107_TYPES: &[(&str, CoreLeafKind)] = &[("WebFormValueType", CoreLeafKind::Enum(&["String", "Int", "Bool", "Float"])), ("WebFormStatus", CoreLeafKind::Enum(&["Idle", "Dirty", "Validating", "Invalid", "Submitting", "Submitted", "Error"])), ("WebFormControl", CoreLeafKind::Enum(&["Text", "Email", "Url", "Password", "Number", "Date", "Checkbox", "Hidden"])), ("WebFormValidationTiming", CoreLeafKind::Enum(&["Change", "Blur", "Submit"])), ("WebFormFieldSpec", CoreLeafKind::Plain), ("WebFormInput", CoreLeafKind::Plain), ("WebFormDecodedInput", CoreLeafKind::Plain), ("WebFormActionError", CoreLeafKind::Plain), ("WebFormErrorState", CoreLeafKind::Plain), ("WebFormLifecycleStatus", CoreLeafKind::Enum(&["Idle", "Pending", "Submitting", "Success", "Failure", "Cancelled"])), ("WebFormLifecycle", CoreLeafKind::Plain), ("WebFormTyped", CoreLeafKind::Plain), ("WebFormValidationChain", CoreLeafKind::Plain), ("WebFormTypedValidation", CoreLeafKind::Plain), ("WebFormTypedSubmission", CoreLeafKind::Plain), ("Pair", CoreLeafKind::Plain), ("WebForm", CoreLeafKind::Plain), ("WebFormField", CoreLeafKind::Plain)];
const CORE_MODULE_107_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_108_MEMBERS: &[&str] = &["cancel", "facts", "get", "invalidate", "live", "mutate", "mutate_with_invalidations", "mutation_state", "mutation_signal", "new", "queue", "refresh", "retry", "set_mode", "set_online", "show", "state", "state_signal", "subscribe", "WebMutationState", "WebMutationStatus", "WebQuery", "WebQueryNetworkMode", "WebQueryStatus"];
const CORE_MODULE_108_TYPES: &[(&str, CoreLeafKind)] = &[("WebMutationState", CoreLeafKind::Plain), ("WebMutationStatus", CoreLeafKind::Enum(&["Idle", "Pending", "Success", "Error", "Settled"])), ("WebQueryStatus", CoreLeafKind::Enum(&["Pending", "Fresh", "Stale", "Fetching", "Error", "Offline"])), ("WebQueryNetworkMode", CoreLeafKind::Enum(&["Online", "Always", "OfflineFirst"])), ("WebQuery", CoreLeafKind::Plain)];
const CORE_MODULE_108_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_109_MEMBERS: &[&str] = &["abort", "cache_show", "cache_state", "collect", "current", "invalidate", "link", "navigate", "new", "not_found", "preload", "route", "route_with_search_codec", "show", "stale", "WebMatch", "WebNavigationStatus", "WebRoute", "WebRouter", "WebRouterCacheStatus", "WebRouterSearchCodec", "WebRouterValueType"];
const CORE_MODULE_109_TYPES: &[(&str, CoreLeafKind)] = &[("WebRouterValueType", CoreLeafKind::Enum(&["String", "Int", "Bool", "Float", "JSON"])), ("WebRouterSearchCodec", CoreLeafKind::Enum(&["Query", "JSON"])), ("WebRouterCacheStatus", CoreLeafKind::Enum(&["Fresh", "Stale", "Invalidated", "Collected"])), ("WebNavigationStatus", CoreLeafKind::Enum(&["Idle", "Preloading", "Pending", "Ready", "Error", "Aborted"])), ("WebMatch", CoreLeafKind::Plain), ("WebRoute", CoreLeafKind::Plain), ("WebRouter", CoreLeafKind::Plain)];
const CORE_MODULE_109_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_110_MEMBERS: &[&str] = &["local", "session", "WebStorage", "kind_local", "kind_session"];
const CORE_MODULE_110_TYPES: &[(&str, CoreLeafKind)] = &[("WebStorage", CoreLeafKind::Plain)];
const CORE_MODULE_110_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_111_MEMBERS: &[&str] = &["clear", "get", "get_or", "has", "remove", "set"];
const CORE_MODULE_111_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_111_DEPENDENCIES: &[&str] = &["core.web.storage"];

const CORE_MODULE_112_MEMBERS: &[&str] = &["clear", "get", "get_or", "has", "remove", "set"];
const CORE_MODULE_112_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_112_DEPENDENCIES: &[&str] = &["core.web.storage"];

const CORE_MODULE_113_MEMBERS: &[&str] = &["back", "batch", "clear_history", "current_generation", "cursor", "derived", "event_json", "events", "events_since", "facts_json", "forward", "history", "history_at", "history_enabled", "history_limit", "inspect", "jump", "new", "optimistic", "patch", "patch_active", "patch_commit", "patch_generation", "patch_rollback", "patch_transaction", "restore", "scrub", "selector", "set", "set_history_limit", "set_state", "signal", "state_signal", "subscribe", "subscribe_selector", "subscription_active", "subscription_unsubscribe", "transaction", "update", "value", "with_history", "WebStore", "WebStoreEvent", "WebStoreInspection", "WebStorePatch", "WebStoreSubscription", "WebStoreTransaction", "get"];
const CORE_MODULE_113_TYPES: &[(&str, CoreLeafKind)] = &[("WebStoreTransaction", CoreLeafKind::Plain), ("WebStoreEvent", CoreLeafKind::Plain), ("WebStoreInspection", CoreLeafKind::Plain), ("WebStorePatch", CoreLeafKind::Plain), ("WebStore", CoreLeafKind::Plain), ("WebStoreSubscription", CoreLeafKind::Plain)];
const CORE_MODULE_113_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_114_MEMBERS: &[&str] = &["clear_focus", "clear_selection", "column", "facts", "filter", "filter_by", "first_page", "focus", "focused_key", "insert_row", "keys", "last_page", "new", "new_keyed", "next_page", "page", "page_state", "paginate", "remove_row", "replace_row", "selected_keys", "selected_rows", "set_rows", "set_selected", "sort", "sort_by", "state", "toggle_selection", "update_row", "visible_rows", "with_column", "with_server_page", "WebTable", "WebTableColumn", "WebTableFilter", "WebTablePageMode", "WebTableRow", "WebTableSort", "WebTableSortDirection", "WebTableState"];
const CORE_MODULE_114_TYPES: &[(&str, CoreLeafKind)] = &[("WebTableSortDirection", CoreLeafKind::Enum(&["Ascending", "Descending"])), ("WebTablePageMode", CoreLeafKind::Enum(&["Client", "Server"])), ("WebTableSort", CoreLeafKind::Plain), ("WebTableFilter", CoreLeafKind::Plain), ("WebTableState", CoreLeafKind::Plain), ("WebTableColumn", CoreLeafKind::Generic(1)), ("WebTablePage", CoreLeafKind::Generic(1)), ("WebTableRow", CoreLeafKind::Generic(1)), ("WebTable", CoreLeafKind::Generic(1))];
const CORE_MODULE_114_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_115_MEMBERS: &[&str] = &["indices", "plan", "plan_from_sizes", "plan_facts", "plan_indices", "plan_measure", "plan_measured", "plan_resize", "plan_scroll_to", "plan_slice", "plan_viewport", "plan_viewport_measure", "plan_viewport_state", "slice", "window", "window_measured", "WebVirtualPlan", "WebVirtualPlanViewport", "WebVirtualWindow"];
const CORE_MODULE_115_TYPES: &[(&str, CoreLeafKind)] = &[("WebVirtualWindow", CoreLeafKind::Plain), ("WebVirtualPlan", CoreLeafKind::Plain), ("WebVirtualPlanViewport", CoreLeafKind::Plain)];
const CORE_MODULE_115_DEPENDENCIES: &[&str] = &["core.web"];

pub const CORE_MODULE_DECLARATIONS: &[CoreModuleDeclaration] = &[
    CoreModuleDeclaration { module: "app", members: CORE_MODULE_0_MEMBERS, type_exports: CORE_MODULE_0_TYPES, dependencies: CORE_MODULE_0_DEPENDENCIES },
    CoreModuleDeclaration { module: "core", members: CORE_MODULE_1_MEMBERS, type_exports: CORE_MODULE_1_TYPES, dependencies: CORE_MODULE_1_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.models", members: CORE_MODULE_2_MEMBERS, type_exports: CORE_MODULE_2_TYPES, dependencies: CORE_MODULE_2_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.devtools", members: CORE_MODULE_3_MEMBERS, type_exports: CORE_MODULE_3_TYPES, dependencies: CORE_MODULE_3_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.archive", members: CORE_MODULE_4_MEMBERS, type_exports: CORE_MODULE_4_TYPES, dependencies: CORE_MODULE_4_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.archive.gzip", members: CORE_MODULE_5_MEMBERS, type_exports: CORE_MODULE_5_TYPES, dependencies: CORE_MODULE_5_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.archive.zstd", members: CORE_MODULE_6_MEMBERS, type_exports: CORE_MODULE_6_TYPES, dependencies: CORE_MODULE_6_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.args", members: CORE_MODULE_7_MEMBERS, type_exports: CORE_MODULE_7_TYPES, dependencies: CORE_MODULE_7_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.auth", members: CORE_MODULE_8_MEMBERS, type_exports: CORE_MODULE_8_TYPES, dependencies: CORE_MODULE_8_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.build", members: CORE_MODULE_9_MEMBERS, type_exports: CORE_MODULE_9_TYPES, dependencies: CORE_MODULE_9_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.compiler", members: CORE_MODULE_10_MEMBERS, type_exports: CORE_MODULE_10_TYPES, dependencies: CORE_MODULE_10_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.compiler.lang", members: CORE_MODULE_11_MEMBERS, type_exports: CORE_MODULE_11_TYPES, dependencies: CORE_MODULE_11_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.collections", members: CORE_MODULE_12_MEMBERS, type_exports: CORE_MODULE_12_TYPES, dependencies: CORE_MODULE_12_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.collections.set", members: CORE_MODULE_13_MEMBERS, type_exports: CORE_MODULE_13_TYPES, dependencies: CORE_MODULE_13_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.compute", members: CORE_MODULE_14_MEMBERS, type_exports: CORE_MODULE_14_TYPES, dependencies: CORE_MODULE_14_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.compute.solve", members: CORE_MODULE_15_MEMBERS, type_exports: CORE_MODULE_15_TYPES, dependencies: CORE_MODULE_15_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.crypto", members: CORE_MODULE_16_MEMBERS, type_exports: CORE_MODULE_16_TYPES, dependencies: CORE_MODULE_16_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.crypto.expert", members: CORE_MODULE_17_MEMBERS, type_exports: CORE_MODULE_17_TYPES, dependencies: CORE_MODULE_17_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.crypto.random", members: CORE_MODULE_18_MEMBERS, type_exports: CORE_MODULE_18_TYPES, dependencies: CORE_MODULE_18_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.crypto.uuid", members: CORE_MODULE_19_MEMBERS, type_exports: CORE_MODULE_19_TYPES, dependencies: CORE_MODULE_19_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.crypto.vault", members: CORE_MODULE_20_MEMBERS, type_exports: CORE_MODULE_20_TYPES, dependencies: CORE_MODULE_20_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data", members: CORE_MODULE_21_MEMBERS, type_exports: CORE_MODULE_21_TYPES, dependencies: CORE_MODULE_21_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.arrow", members: CORE_MODULE_22_MEMBERS, type_exports: CORE_MODULE_22_TYPES, dependencies: CORE_MODULE_22_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.loader", members: CORE_MODULE_23_MEMBERS, type_exports: CORE_MODULE_23_TYPES, dependencies: CORE_MODULE_23_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.stream", members: CORE_MODULE_24_MEMBERS, type_exports: CORE_MODULE_24_TYPES, dependencies: CORE_MODULE_24_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.plot", members: CORE_MODULE_25_MEMBERS, type_exports: CORE_MODULE_25_TYPES, dependencies: CORE_MODULE_25_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.sketch", members: CORE_MODULE_26_MEMBERS, type_exports: CORE_MODULE_26_TYPES, dependencies: CORE_MODULE_26_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.sketch.cms", members: CORE_MODULE_27_MEMBERS, type_exports: CORE_MODULE_27_TYPES, dependencies: CORE_MODULE_27_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.sketch.hll", members: CORE_MODULE_28_MEMBERS, type_exports: CORE_MODULE_28_TYPES, dependencies: CORE_MODULE_28_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.sketch.reservoir", members: CORE_MODULE_29_MEMBERS, type_exports: CORE_MODULE_29_TYPES, dependencies: CORE_MODULE_29_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.sketch.tdigest", members: CORE_MODULE_30_MEMBERS, type_exports: CORE_MODULE_30_TYPES, dependencies: CORE_MODULE_30_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.db", members: CORE_MODULE_31_MEMBERS, type_exports: CORE_MODULE_31_TYPES, dependencies: CORE_MODULE_31_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.email", members: CORE_MODULE_32_MEMBERS, type_exports: CORE_MODULE_32_TYPES, dependencies: CORE_MODULE_32_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding", members: CORE_MODULE_33_MEMBERS, type_exports: CORE_MODULE_33_TYPES, dependencies: CORE_MODULE_33_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.base32", members: CORE_MODULE_34_MEMBERS, type_exports: CORE_MODULE_34_TYPES, dependencies: CORE_MODULE_34_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.base64", members: CORE_MODULE_35_MEMBERS, type_exports: CORE_MODULE_35_TYPES, dependencies: CORE_MODULE_35_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.binary", members: CORE_MODULE_36_MEMBERS, type_exports: CORE_MODULE_36_TYPES, dependencies: CORE_MODULE_36_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.cbor", members: CORE_MODULE_37_MEMBERS, type_exports: CORE_MODULE_37_TYPES, dependencies: CORE_MODULE_37_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.csv", members: CORE_MODULE_38_MEMBERS, type_exports: CORE_MODULE_38_TYPES, dependencies: CORE_MODULE_38_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.hex", members: CORE_MODULE_39_MEMBERS, type_exports: CORE_MODULE_39_TYPES, dependencies: CORE_MODULE_39_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.ini", members: CORE_MODULE_40_MEMBERS, type_exports: CORE_MODULE_40_TYPES, dependencies: CORE_MODULE_40_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.json", members: CORE_MODULE_41_MEMBERS, type_exports: CORE_MODULE_41_TYPES, dependencies: CORE_MODULE_41_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.jsonl", members: CORE_MODULE_42_MEMBERS, type_exports: CORE_MODULE_42_TYPES, dependencies: CORE_MODULE_42_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.toml", members: CORE_MODULE_43_MEMBERS, type_exports: CORE_MODULE_43_TYPES, dependencies: CORE_MODULE_43_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.xml", members: CORE_MODULE_44_MEMBERS, type_exports: CORE_MODULE_44_TYPES, dependencies: CORE_MODULE_44_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.yaml", members: CORE_MODULE_45_MEMBERS, type_exports: CORE_MODULE_45_TYPES, dependencies: CORE_MODULE_45_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.event", members: CORE_MODULE_46_MEMBERS, type_exports: CORE_MODULE_46_TYPES, dependencies: CORE_MODULE_46_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.files", members: CORE_MODULE_47_MEMBERS, type_exports: CORE_MODULE_47_TYPES, dependencies: CORE_MODULE_47_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.files.path", members: CORE_MODULE_48_MEMBERS, type_exports: CORE_MODULE_48_TYPES, dependencies: CORE_MODULE_48_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.font", members: CORE_MODULE_49_MEMBERS, type_exports: CORE_MODULE_49_TYPES, dependencies: CORE_MODULE_49_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.game", members: CORE_MODULE_50_MEMBERS, type_exports: CORE_MODULE_50_TYPES, dependencies: CORE_MODULE_50_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.game.raylib", members: CORE_MODULE_51_MEMBERS, type_exports: CORE_MODULE_51_TYPES, dependencies: CORE_MODULE_51_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.http", members: CORE_MODULE_52_MEMBERS, type_exports: CORE_MODULE_52_TYPES, dependencies: CORE_MODULE_52_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.http.client", members: CORE_MODULE_53_MEMBERS, type_exports: CORE_MODULE_53_TYPES, dependencies: CORE_MODULE_53_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.http.server", members: CORE_MODULE_54_MEMBERS, type_exports: CORE_MODULE_54_TYPES, dependencies: CORE_MODULE_54_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.io", members: CORE_MODULE_55_MEMBERS, type_exports: CORE_MODULE_55_TYPES, dependencies: CORE_MODULE_55_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.jobs", members: CORE_MODULE_56_MEMBERS, type_exports: CORE_MODULE_56_TYPES, dependencies: CORE_MODULE_56_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.log", members: CORE_MODULE_57_MEMBERS, type_exports: CORE_MODULE_57_TYPES, dependencies: CORE_MODULE_57_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.math", members: CORE_MODULE_58_MEMBERS, type_exports: CORE_MODULE_58_TYPES, dependencies: CORE_MODULE_58_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.math.random", members: CORE_MODULE_59_MEMBERS, type_exports: CORE_MODULE_59_TYPES, dependencies: CORE_MODULE_59_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.math.combinatorics", members: CORE_MODULE_60_MEMBERS, type_exports: CORE_MODULE_60_TYPES, dependencies: CORE_MODULE_60_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.math.stats", members: CORE_MODULE_61_MEMBERS, type_exports: CORE_MODULE_61_TYPES, dependencies: CORE_MODULE_61_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.mem", members: CORE_MODULE_62_MEMBERS, type_exports: CORE_MODULE_62_TYPES, dependencies: CORE_MODULE_62_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.mem.scope", members: CORE_MODULE_63_MEMBERS, type_exports: CORE_MODULE_63_TYPES, dependencies: CORE_MODULE_63_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.mod", members: CORE_MODULE_64_MEMBERS, type_exports: CORE_MODULE_64_TYPES, dependencies: CORE_MODULE_64_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.net", members: CORE_MODULE_65_MEMBERS, type_exports: CORE_MODULE_65_TYPES, dependencies: CORE_MODULE_65_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.net.mime", members: CORE_MODULE_66_MEMBERS, type_exports: CORE_MODULE_66_TYPES, dependencies: CORE_MODULE_66_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.net.tls", members: CORE_MODULE_67_MEMBERS, type_exports: CORE_MODULE_67_TYPES, dependencies: CORE_MODULE_67_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.net.url", members: CORE_MODULE_68_MEMBERS, type_exports: CORE_MODULE_68_TYPES, dependencies: CORE_MODULE_68_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.net.ws", members: CORE_MODULE_69_MEMBERS, type_exports: CORE_MODULE_69_TYPES, dependencies: CORE_MODULE_69_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.net.ip", members: CORE_MODULE_70_MEMBERS, type_exports: CORE_MODULE_70_TYPES, dependencies: CORE_MODULE_70_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.perf", members: CORE_MODULE_71_MEMBERS, type_exports: CORE_MODULE_71_TYPES, dependencies: CORE_MODULE_71_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.plugin", members: CORE_MODULE_72_MEMBERS, type_exports: CORE_MODULE_72_TYPES, dependencies: CORE_MODULE_72_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.prelude", members: CORE_MODULE_73_MEMBERS, type_exports: CORE_MODULE_73_TYPES, dependencies: CORE_MODULE_73_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.process", members: CORE_MODULE_74_MEMBERS, type_exports: CORE_MODULE_74_TYPES, dependencies: CORE_MODULE_74_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.reactive", members: CORE_MODULE_75_MEMBERS, type_exports: CORE_MODULE_75_TYPES, dependencies: CORE_MODULE_75_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.reactive.loadable", members: CORE_MODULE_76_MEMBERS, type_exports: CORE_MODULE_76_TYPES, dependencies: CORE_MODULE_76_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.reflect", members: CORE_MODULE_77_MEMBERS, type_exports: CORE_MODULE_77_TYPES, dependencies: CORE_MODULE_77_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.regex", members: CORE_MODULE_78_MEMBERS, type_exports: CORE_MODULE_78_TYPES, dependencies: CORE_MODULE_78_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.rt", members: CORE_MODULE_79_MEMBERS, type_exports: CORE_MODULE_79_TYPES, dependencies: CORE_MODULE_79_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.service", members: CORE_MODULE_80_MEMBERS, type_exports: CORE_MODULE_80_TYPES, dependencies: CORE_MODULE_80_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.sync", members: CORE_MODULE_81_MEMBERS, type_exports: CORE_MODULE_81_TYPES, dependencies: CORE_MODULE_81_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.sys", members: CORE_MODULE_82_MEMBERS, type_exports: CORE_MODULE_82_TYPES, dependencies: CORE_MODULE_82_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.tasks", members: CORE_MODULE_83_MEMBERS, type_exports: CORE_MODULE_83_TYPES, dependencies: CORE_MODULE_83_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.term", members: CORE_MODULE_84_MEMBERS, type_exports: CORE_MODULE_84_TYPES, dependencies: CORE_MODULE_84_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.testing", members: CORE_MODULE_85_MEMBERS, type_exports: CORE_MODULE_85_TYPES, dependencies: CORE_MODULE_85_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.text", members: CORE_MODULE_86_MEMBERS, type_exports: CORE_MODULE_86_TYPES, dependencies: CORE_MODULE_86_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.text.fmt", members: CORE_MODULE_87_MEMBERS, type_exports: CORE_MODULE_87_TYPES, dependencies: CORE_MODULE_87_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.text.html", members: CORE_MODULE_88_MEMBERS, type_exports: CORE_MODULE_88_TYPES, dependencies: CORE_MODULE_88_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.text.wrap", members: CORE_MODULE_89_MEMBERS, type_exports: CORE_MODULE_89_TYPES, dependencies: CORE_MODULE_89_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.text.parse", members: CORE_MODULE_90_MEMBERS, type_exports: CORE_MODULE_90_TYPES, dependencies: CORE_MODULE_90_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.time", members: CORE_MODULE_91_MEMBERS, type_exports: CORE_MODULE_91_TYPES, dependencies: CORE_MODULE_91_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.time.calendar", members: CORE_MODULE_92_MEMBERS, type_exports: CORE_MODULE_92_TYPES, dependencies: CORE_MODULE_92_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.time.expiring", members: CORE_MODULE_93_MEMBERS, type_exports: CORE_MODULE_93_TYPES, dependencies: CORE_MODULE_93_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui", members: CORE_MODULE_94_MEMBERS, type_exports: CORE_MODULE_94_TYPES, dependencies: CORE_MODULE_94_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.tui", members: CORE_MODULE_95_MEMBERS, type_exports: CORE_MODULE_95_TYPES, dependencies: CORE_MODULE_95_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host", members: CORE_MODULE_96_MEMBERS, type_exports: CORE_MODULE_96_TYPES, dependencies: CORE_MODULE_96_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host.clipboard", members: CORE_MODULE_97_MEMBERS, type_exports: CORE_MODULE_97_TYPES, dependencies: CORE_MODULE_97_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host.ime", members: CORE_MODULE_98_MEMBERS, type_exports: CORE_MODULE_98_TYPES, dependencies: CORE_MODULE_98_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host.drag_drop", members: CORE_MODULE_99_MEMBERS, type_exports: CORE_MODULE_99_TYPES, dependencies: CORE_MODULE_99_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host.shortcuts", members: CORE_MODULE_100_MEMBERS, type_exports: CORE_MODULE_100_TYPES, dependencies: CORE_MODULE_100_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host.accessibility", members: CORE_MODULE_101_MEMBERS, type_exports: CORE_MODULE_101_TYPES, dependencies: CORE_MODULE_101_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.units", members: CORE_MODULE_102_MEMBERS, type_exports: CORE_MODULE_102_TYPES, dependencies: CORE_MODULE_102_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.watcher", members: CORE_MODULE_103_MEMBERS, type_exports: CORE_MODULE_103_TYPES, dependencies: CORE_MODULE_103_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web", members: CORE_MODULE_104_MEMBERS, type_exports: CORE_MODULE_104_TYPES, dependencies: CORE_MODULE_104_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.browser", members: CORE_MODULE_105_MEMBERS, type_exports: CORE_MODULE_105_TYPES, dependencies: CORE_MODULE_105_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.devserver", members: CORE_MODULE_106_MEMBERS, type_exports: CORE_MODULE_106_TYPES, dependencies: CORE_MODULE_106_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.forms", members: CORE_MODULE_107_MEMBERS, type_exports: CORE_MODULE_107_TYPES, dependencies: CORE_MODULE_107_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.query", members: CORE_MODULE_108_MEMBERS, type_exports: CORE_MODULE_108_TYPES, dependencies: CORE_MODULE_108_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.router", members: CORE_MODULE_109_MEMBERS, type_exports: CORE_MODULE_109_TYPES, dependencies: CORE_MODULE_109_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.storage", members: CORE_MODULE_110_MEMBERS, type_exports: CORE_MODULE_110_TYPES, dependencies: CORE_MODULE_110_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.storage.local", members: CORE_MODULE_111_MEMBERS, type_exports: CORE_MODULE_111_TYPES, dependencies: CORE_MODULE_111_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.storage.session", members: CORE_MODULE_112_MEMBERS, type_exports: CORE_MODULE_112_TYPES, dependencies: CORE_MODULE_112_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.store", members: CORE_MODULE_113_MEMBERS, type_exports: CORE_MODULE_113_TYPES, dependencies: CORE_MODULE_113_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.table", members: CORE_MODULE_114_MEMBERS, type_exports: CORE_MODULE_114_TYPES, dependencies: CORE_MODULE_114_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.virtual", members: CORE_MODULE_115_MEMBERS, type_exports: CORE_MODULE_115_TYPES, dependencies: CORE_MODULE_115_DEPENDENCIES },
];

pub const CORE_ROOT_TYPES: &[&str] = &["Date", "LocalDate", "LocalTime", "Decimal", "Fraction", "Duration", "Instant", "Authority"];
// END GENERATED CORE DECLARATIONS

/// Look up how `module.leaf` should resolve, or `None` if that module/leaf
/// pair isn't a registered Core export (the caller falls through to other
/// resolution paths, e.g. the generic file-module registry or
/// `core.compiler.lang`'s dynamic rule check).
pub fn core_leaf_kind(module: &str, leaf: &str) -> Option<CoreLeafKind> {
    lookup(CORE_MODULE_DECLARATIONS, module, leaf)
}
/// Return the required type-parameter count for a canonical Core generic
/// export, or `None` for a non-generic/unknown name.
pub fn core_generic_arity(name: &str) -> Option<usize> {
    let mut arity = None;
    for entry in CORE_MODULE_DECLARATIONS {
        for &(type_name, kind) in entry.type_exports {
            if type_name != name {
                continue;
            }
            if let CoreLeafKind::Generic(count) = kind {
                if arity.is_some_and(|previous| previous != count) {
                    return None;
                }
                arity = Some(count);
            }
        }
    }
    arity
}
/// Return every Core module that exports the leaf of `name`.
///
/// Type names can reach sema as a qualified alias (`email.Address`) or as a
/// canonical bare leaf (`Address`). The generated export table is the only
/// owner registry; callers must still reject a user declaration before asking
/// for these modules.
pub fn core_type_modules(name: &str) -> Vec<&'static str> {
    let leaf = name
        .rsplit_once("::")
        .or_else(|| name.rsplit_once('.'))
        .map_or(name, |(_, leaf)| leaf);
    CORE_MODULE_DECLARATIONS
        .iter()
        .filter_map(|entry| {
            entry
                .type_exports
                .iter()
                .any(|&(type_name, _)| type_name == leaf)
                .then_some(entry.module)
        })
        .collect()
}


/// Return the unit-variant names for a canonical Core enum type.
pub fn core_enum_variants(name: &str) -> Option<&'static [&'static str]> {
    for entry in CORE_MODULE_DECLARATIONS {
        for &(type_name, kind) in entry.type_exports {
            if type_name == name {
                if let CoreLeafKind::Enum(variants) = kind {
                    return Some(variants);
                }
            }
        }
    }
    None
}

/// Whether `name` is a canonical root or module-exported Core type.
///
/// Generated declarations use this same Core-name table when choosing a
/// user-visible type name. Keeping the query here prevents binders from
/// carrying a second, inevitably stale list of Core names.
pub fn is_core_type_name(name: &str) -> bool {
    CORE_ROOT_TYPES.contains(&name)
        || CORE_MODULE_DECLARATIONS
            .iter()
            .any(|entry| entry.type_exports.iter().any(|(leaf, _)| *leaf == name))
}

/// The canonical module declarations consumed by sema and future MIR.
pub const fn core_modules() -> &'static [CoreModuleDeclaration] {
    CORE_MODULE_DECLARATIONS
}

/// The canonical bootstrap ordering consumed by sema and future MIR.
pub const fn core_bootstrap_order() -> &'static [CoreBootstrapStep] {
    CORE_BOOTSTRAP_ORDER
}

/// The one generic lookup every module table (production or test) resolves
/// through — adding a module is a data row here, never a new match arm.
fn lookup(
    table: &[CoreModuleDeclaration],
    module: &str,
    leaf: &str,
) -> Option<CoreLeafKind> {
    table
        .iter()
        .find(|entry| entry.module == module)
        .and_then(|entry| entry.type_exports.iter().find(|(name, _)| *name == leaf))
        .map(|(_, kind)| *kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_modules_resolve_without_a_bespoke_arm() {
        assert_eq!(
            core_leaf_kind("core.email", "Address"),
            Some(CoreLeafKind::Plain)
        );
        assert_eq!(
            core_leaf_kind("core.crypto", "Secret"),
            Some(CoreLeafKind::CryptoNominal)
        );
        assert_eq!(
            core_leaf_kind("core.crypto", "VerifyKey"),
            Some(CoreLeafKind::Plain)
        );
        assert_eq!(
            core_leaf_kind("core.sys", "EnvError"),
            Some(CoreLeafKind::Plain)
        );
        assert_eq!(
            core_leaf_kind("core.encoding.cbor", "CBORReader"),
            Some(CoreLeafKind::Plain)
        );
        assert_eq!(core_leaf_kind("core.email", "NoSuchLeaf"), None);
        assert_eq!(core_leaf_kind("core.nonexistent", "Address"), None);
    }

    #[test]
    fn web_enum_variants_come_from_the_canonical_declaration() {
        const EXPECTED: &[&str] = &["Pending", "Fresh", "Stale", "Fetching", "Error", "Offline"];
        assert_eq!(core_enum_variants("WebQueryStatus"), Some(EXPECTED));
        assert_eq!(core_enum_variants("Address"), None);
    }

    /// Criterion #2: a brand-new Core module needs one data row, not a new
    /// Rust match arm. `lookup` is the one function every module (production
    /// or, here, a module added purely for this test) resolves through.
    #[test]
    fn adding_a_module_is_one_data_row_no_new_match_arm() {
        const NEW_MODULE_TYPES: &[(&str, CoreLeafKind)] = &[("Widget", CoreLeafKind::Plain)];
        const TABLE_WITH_NEW_MODULE: &[CoreModuleDeclaration] = &[CoreModuleDeclaration {
            module: "core.widgets",
            members: &["Widget"],
            type_exports: NEW_MODULE_TYPES,
            dependencies: &[],
        }];

        assert_eq!(
            lookup(TABLE_WITH_NEW_MODULE, "core.widgets", "Widget"),
            Some(CoreLeafKind::Plain)
        );
    }

    #[test]
    fn declarations_expose_bootstrap_and_dependency_order() {
        assert_eq!(
            core_bootstrap_order().iter().map(|step| step.name).collect::<Vec<_>>(),
            vec!["Syntax", "Effects", "Core", "Derives"]
        );
        let web = core_modules()
            .iter()
            .find(|entry| entry.module == "core.web")
            .expect("core.web declaration");
        assert_eq!(web.dependencies, &["core.http"]);
    }
}
