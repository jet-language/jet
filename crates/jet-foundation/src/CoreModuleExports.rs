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
    /// A Core enum with canonical variant names. Payload shapes come from the
    /// checked source declaration when one is loaded.
    Enum(&'static [&'static str]),
    /// A generic Core type; the value is its required type-parameter count.
    Generic(usize),
    /// D-CRYPTO-API1=A: secret-bearing crypto values keep distinct nominal
    /// provenance from a same-named local type.
    CryptoNominal,
}

// BEGIN GENERATED CORE DECLARATIONS
// Source: crates/jet-codegen/src/Prelude/Core.jet
// Source SHA-256: 0a86db9f137d7f3fe3d4aeb36b8eace763327ec1a2ee55c73cf46d8d575f7060
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreModuleDeclaration {
    pub module: &'static str,
    pub members: &'static [&'static str],
    pub type_exports: &'static [(&'static str, CoreLeafKind)],
    pub dependencies: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreSourceModule {
    pub module: &'static str,
    pub alias: &'static str,
    pub path: &'static str,
    pub owned_members: &'static [&'static str],
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
    "core.event",
    "core.files",
    "core.encoding.yaml",
    "core.files.path",
    "core.font",
    "core.game",
    "core.game.raylib",
    "core.http",
    "core.http.client",
    "core.http.server",
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

const CORE_MODULE_0_MEMBERS: &[&str] = &["AppError", "Auth", "LiveQuery", "Session", "auth", "auth_oauth", "auth_routes", "auth_show", "invalidate", "live", "live_get", "live_show", "live_stats", "signal_push", "subscribe", "sync", "transact_invalidate"];
const CORE_MODULE_0_TYPES: &[(&str, CoreLeafKind)] = &[("AppError", CoreLeafKind::Enum(&["Invalid", "Unsupported"])), ("Auth", CoreLeafKind::Plain), ("LiveQuery", CoreLeafKind::Plain), ("Session", CoreLeafKind::Plain)];
const CORE_MODULE_0_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_1_MEMBERS: &[&str] = &[];
const CORE_MODULE_1_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_1_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_2_MEMBERS: &[&str] = &["publish"];
const CORE_MODULE_2_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_2_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_3_MEMBERS: &[&str] = &["ArchiveError", "adler32", "crc32", "deflate", "inflate", "compress", "decompress", "tar_add", "tar_get", "tar_names_json", "unzip", "zip_close", "zip_decompress", "zip_extract", "zip_names_json", "zip_next", "zip_open", "zip_read", "zip_write", "list", "create"];
const CORE_MODULE_3_TYPES: &[(&str, CoreLeafKind)] = &[("ArchiveError", CoreLeafKind::Enum(&["Malformed", "Unsupported", "Checksum", "NotFound"]))];
const CORE_MODULE_3_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_4_MEMBERS: &[&str] = &["compress", "decompress", "is_gzip", "isize", "magic", "crc", "compress_text", "decompress_text", "peek_isize", "compress_file", "decompress_file", "GzipFileError"];
const CORE_MODULE_4_TYPES: &[(&str, CoreLeafKind)] = &[("GzipFileError", CoreLeafKind::Enum(&["Read", "Write", "Malformed", "Unsupported", "Checksum"]))];
const CORE_MODULE_4_DEPENDENCIES: &[&str] = &["core.archive"];

const CORE_MODULE_5_MEMBERS: &[&str] = &["compress", "decompress", "is_zstd", "magic", "compress_text", "decompress_text", "compress_file", "decompress_file", "ZstdFileError"];
const CORE_MODULE_5_TYPES: &[(&str, CoreLeafKind)] = &[("ZstdFileError", CoreLeafKind::Enum(&["Read", "Write", "Malformed", "Unsupported", "Checksum"]))];
const CORE_MODULE_5_DEPENDENCIES: &[&str] = &["core.archive"];

const CORE_MODULE_6_MEMBERS: &[&str] = &["decode", "decode_argv", "flag", "get_bool", "get_int", "get_text", "has", "help_text", "merge", "positionals", "program", "spec", "ArgDef", "ArgsSpec", "apply_defaults", "argument", "count_flag", "default_value", "define", "dest", "get_choice", "get_or", "help_from", "missing_required", "remainder", "required", "usage", "wants_help"];
const CORE_MODULE_6_TYPES: &[(&str, CoreLeafKind)] = &[("ArgDef", CoreLeafKind::Plain), ("ArgsSpec", CoreLeafKind::Plain)];
const CORE_MODULE_6_DEPENDENCIES: &[&str] = &["core.process"];

const CORE_MODULE_7_MEMBERS: &[&str] = &["Auth", "AuthError", "Claims", "Session", "magic_link_consume", "magic_link_issue", "oauth_begin", "oauth_finish", "password_login", "register_user", "session_cookie", "session_id", "session_show", "session_user", "session_validate", "verify_jwt", "verify_paseto"];
const CORE_MODULE_7_TYPES: &[(&str, CoreLeafKind)] = &[("Auth", CoreLeafKind::Plain), ("AuthError", CoreLeafKind::Enum(&["Rejected", "Expired", "Malformed", "Unavailable"])), ("Claims", CoreLeafKind::Plain), ("Session", CoreLeafKind::Plain)];
const CORE_MODULE_7_DEPENDENCIES: &[&str] = &["core.crypto", "core.net", "core.time"];

const CORE_MODULE_8_MEMBERS: &[&str] = &["BuildGraph", "BuildGraphAction", "BuildGraphActionKey", "BuildGraphCacheDelta", "BuildGraphDiff", "BuildGraphFile", "BuildGraphFileDelta", "BuildGraphInputDigest", "BuildGraphKeyDelta", "BuildGraphNode", "BuildGraphTarget", "graph", "receipt_diff"];
const CORE_MODULE_8_TYPES: &[(&str, CoreLeafKind)] = &[("BuildGraph", CoreLeafKind::Plain), ("BuildGraphTarget", CoreLeafKind::Plain), ("BuildGraphAction", CoreLeafKind::Plain), ("BuildGraphFile", CoreLeafKind::Plain), ("BuildGraphNode", CoreLeafKind::Plain), ("BuildGraphInputDigest", CoreLeafKind::Plain), ("BuildGraphActionKey", CoreLeafKind::Plain), ("BuildGraphFileDelta", CoreLeafKind::Plain), ("BuildGraphKeyDelta", CoreLeafKind::Plain), ("BuildGraphCacheDelta", CoreLeafKind::Plain), ("BuildGraphDiff", CoreLeafKind::Plain)];
const CORE_MODULE_8_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_9_MEMBERS: &[&str] = &["check", "lex", "lock", "manifest", "package", "parse", "profiles", "source_map", "Pair", "Token"];
const CORE_MODULE_9_TYPES: &[(&str, CoreLeafKind)] = &[("Pair", CoreLeafKind::Plain), ("Token", CoreLeafKind::Plain)];
const CORE_MODULE_9_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_10_MEMBERS: &[&str] = &["ABI", "ArithmeticMode", "Effect", "FFILanguage", "InlineMode", "JobScope", "KernelMode", "Layout", "Maturity", "MemoBound", "NamingCase", "ObligationMode", "Path", "PolicySetting", "Site", "State", "TaintKind", "Target", "Track"];
const CORE_MODULE_10_TYPES: &[(&str, CoreLeafKind)] = &[("ABI", CoreLeafKind::Enum(&["System", "Cdecl", "Stdcall", "Fastcall", "Win64", "Sysv64"])), ("ArithmeticMode", CoreLeafKind::Enum(&["Checked", "Wrapping", "Saturating"])), ("Effect", CoreLeafKind::Enum(&["FS", "Net", "Crypto", "Time", "Random", "Env", "Proc", "IO", "DB", "Exec", "Browser", "Secret"])), ("FFILanguage", CoreLeafKind::Enum(&["C", "Cpp", "Asm"])), ("InlineMode", CoreLeafKind::Enum(&["Hint", "Always", "Never"])), ("JobScope", CoreLeafKind::Enum(&["Dev", "Ship", "Internal"])), ("KernelMode", CoreLeafKind::Enum(&["Parallel"])), ("Layout", CoreLeafKind::Enum(&["C", "Columnar"])), ("Maturity", CoreLeafKind::Enum(&["Experimental", "Tested", "Hardened"])), ("MemoBound", CoreLeafKind::Enum(&["Default", "Unset"])), ("NamingCase", CoreLeafKind::Enum(&["Camel", "Snake", "Pascal", "Kebab", "Screaming"])), ("ObligationMode", CoreLeafKind::Enum(&["Unspecified", "GateOnly", "Obligations", "PerSite", "Track", "Skip"])), ("Path", CoreLeafKind::Plain), ("PolicySetting", CoreLeafKind::Enum(&["Allow", "Deny", "Unsafe", "GC", "ExplicitUnits", "Copies", "Sentries", "Explicit", "On", "Off"])), ("Site", CoreLeafKind::Enum(&["Package", "File", "Module", "Function", "Method", "Block", "Statement", "Expression", "Type", "Impl", "Declaration", "Constant", "Field", "Variant", "Parameter", "Test", "Operation", "Text"])), ("State", CoreLeafKind::Plain), ("TaintKind", CoreLeafKind::Enum(&["Input", "PII", "Secret", "Credential"])), ("Target", CoreLeafKind::Enum(&["Native", "Web", "Wasm", "JS", "Freestanding", "OS"])), ("Track", CoreLeafKind::Enum(&["Frontend", "Backend", "Runtime", "Tooling"]))];
const CORE_MODULE_10_DEPENDENCIES: &[&str] = &["core.compiler"];

const CORE_MODULE_11_MEMBERS: &[&str] = &["Counter", "OrderedMap", "Layer", "Chain", "counter", "counter_from", "add", "inc", "dec", "get", "set_count", "total", "names", "elements", "most_common", "subtract", "merge_add", "clear_counter", "heapify", "heappush", "heappop", "heappushpop", "heapreplace", "nsmallest", "nlargest", "merge_sorted", "bisect_left", "bisect_right", "insort_left", "insort_right", "ordered_map", "map_get", "map_set", "map_remove", "map_keys", "map_values", "map_contains", "map_len", "chain", "chain_push", "chain_get", "chain_contains"];
const CORE_MODULE_11_TYPES: &[(&str, CoreLeafKind)] = &[("Counter", CoreLeafKind::Plain), ("OrderedMap", CoreLeafKind::Plain), ("Layer", CoreLeafKind::Plain), ("Chain", CoreLeafKind::Plain)];
const CORE_MODULE_11_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_12_MEMBERS: &[&str] = &["StringSet", "new", "from_list", "add", "discard", "remove", "contains", "len", "is_empty", "to_list", "clear", "union", "intersection", "difference", "symmetric_difference", "issubset", "issuperset", "isdisjoint", "clone_set"];
const CORE_MODULE_12_TYPES: &[(&str, CoreLeafKind)] = &[("StringSet", CoreLeafKind::Plain)];
const CORE_MODULE_12_DEPENDENCIES: &[&str] = &["core.collections"];

const CORE_MODULE_13_MEMBERS: &[&str] = &["ComputeDevice", "ComputeError", "ComputeStream", "SparseTensor", "Tensor", "VJPRun", "abs", "add", "broadcast_to", "deserialize", "det", "device", "device_auto", "device_cpu", "device_cuda", "device_metal", "device_vulkan", "device_webgpu", "div", "exp", "eye", "fft", "from_list", "full", "get", "gradient", "inv", "jvp", "kernel_bounds_ok", "log", "matmul", "matmul_f32_tile", "matrix", "maximum", "minimum", "mse_loss", "mul", "negate", "numel", "on_device", "ones", "placement", "profile_f32_strict", "profile_show", "rank", "reshape", "serialize", "set", "sgd_step", "shape", "solve", "sparse_mv", "sparse_nnz", "sparse_show", "sqrt", "stream_new", "stream_new_on", "stream_show", "stream_sync", "sub", "sum_axis", "to_list", "to_sparse", "transfer", "transfer_show", "transpose", "value_and_gradient", "vec", "vjp", "zeros"];
const CORE_MODULE_13_TYPES: &[(&str, CoreLeafKind)] = &[("ComputeDevice", CoreLeafKind::Plain), ("ComputeError", CoreLeafKind::Enum(&["Shape", "RankMismatch", "OutOfBounds", "Device", "Unsupported", "Arithmetic", "Singular", "Index", "Parse", "Serialization"])), ("ComputeStream", CoreLeafKind::Plain), ("SparseTensor", CoreLeafKind::Plain), ("Tensor", CoreLeafKind::Plain), ("VJPRun", CoreLeafKind::Plain)];
const CORE_MODULE_13_DEPENDENCIES: &[&str] = &["core.math", "core.text.fmt"];

const CORE_MODULE_14_MEMBERS: &[&str] = &["Solver", "LinearSolver", "dense", "lu"];
const CORE_MODULE_14_TYPES: &[(&str, CoreLeafKind)] = &[("Solver", CoreLeafKind::Plain), ("LinearSolver", CoreLeafKind::Plain)];
const CORE_MODULE_14_DEPENDENCIES: &[&str] = &["core.compute", "core.math"];

const CORE_MODULE_15_MEMBERS: &[&str] = &["CryptoError", "OpenFailed", "VerifyFailed", "KeyRejected", "Length", "Unavailable", "Digest256", "Digest512", "FileCryptoError", "PublishFailed", "SourceMissing", "Hasher", "KeyUnlock", "KeyWrapError", "PasswordHash", "Sealed", "Secret", "SharedSecret", "Signature", "SigningKey", "VerifyKey", "WrappedKey", "WrappedVaultKey", "X25519PublicKey", "X25519SecretKey", "blake3", "constant_time_equal", "constant_time_equal_bytes", "file_open", "file_seal", "generatekey", "hkdf_sha256", "hmac_sha256", "open", "password_hash", "password_hash_with_salt", "password_verify", "pbkdf2_hmac", "privatedecrypt", "privateencrypt", "publicdecrypt", "publicencrypt", "seal", "sealed_bytes", "sha1", "sha224", "sha256", "sha384", "sha3_224", "sha3_256", "sha3_384", "sha3_512", "sha512", "sign", "unwrap", "verify", "wrap", "x25519", "x25519_public", "x25519_shared", "EdPoint", "new", "update", "digest"];
const CORE_MODULE_15_TYPES: &[(&str, CoreLeafKind)] = &[("Secret", CoreLeafKind::Plain), ("SigningKey", CoreLeafKind::Plain), ("X25519SecretKey", CoreLeafKind::Plain), ("SharedSecret", CoreLeafKind::Plain), ("VerifyKey", CoreLeafKind::Plain), ("X25519PublicKey", CoreLeafKind::Plain), ("Signature", CoreLeafKind::Plain), ("Sealed", CoreLeafKind::Plain), ("WrappedKey", CoreLeafKind::Plain), ("WrappedVaultKey", CoreLeafKind::Plain), ("KeyUnlock", CoreLeafKind::Plain), ("PasswordHash", CoreLeafKind::Plain), ("Digest256", CoreLeafKind::Plain), ("Digest512", CoreLeafKind::Plain), ("Hasher", CoreLeafKind::Plain), ("CryptoError", CoreLeafKind::Enum(&["OpenFailed", "VerifyFailed", "KeyRejected", "Length", "Unavailable"])), ("FileCryptoError", CoreLeafKind::Enum(&["OpenFailed", "PublishFailed", "SourceMissing"])), ("KeyWrapError", CoreLeafKind::Plain), ("EdPoint", CoreLeafKind::Plain)];
const CORE_MODULE_15_DEPENDENCIES: &[&str] = &["core", "core.files"];

const CORE_MODULE_16_MEMBERS: &[&str] = &["aes256gcm_open", "aes256gcm_seal", "argon2id", "ed25519_sign", "ed25519_verify_strict", "hkdf_sha256_raw", "migrate_v1", "open_v1", "secret_bytes", "shared_secret_bytes", "signing_key_bytes", "x25519_raw", "x25519_secret_bytes", "xchacha20poly1305_open", "xchacha20poly1305_seal"];
const CORE_MODULE_16_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_16_DEPENDENCIES: &[&str] = &["core.crypto"];

const CORE_MODULE_17_MEMBERS: &[&str] = &["bytes", "choice", "int_range", "token_hex", "token_urlsafe", "u32", "u64", "token_bytes", "randbelow", "randbits", "compare_digest", "choice_int", "shuffle_ints"];
const CORE_MODULE_17_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_17_DEPENDENCIES: &[&str] = &["core.crypto"];

const CORE_MODULE_18_MEMBERS: &[&str] = &["UUIDError", "parse", "v4", "v5", "uuid5", "v7"];
const CORE_MODULE_18_TYPES: &[(&str, CoreLeafKind)] = &[("UUIDError", CoreLeafKind::Plain)];
const CORE_MODULE_18_DEPENDENCIES: &[&str] = &["core.crypto"];

const CORE_MODULE_19_MEMBERS: &[&str] = &["ExpiringSecret", "KeyRef", "KeyStatus", "KeyUnlock", "KeyWrapError", "MutationPlan", "Rotation", "VaultError", "VaultWrite", "WrappedImportPlan", "WrappedVaultKey", "authorize_wrapped_import", "authorize_write", "commit_generate", "commit_import_signing", "commit_import_wrapped", "commit_import_x25519", "commit_retire", "commit_revoke", "commit_rotate", "commit_store", "current", "export_to_passphrase", "export_to_recipients", "get", "load", "prepare_generate", "prepare_import_signing", "prepare_import_wrapped", "prepare_import_x25519", "prepare_retire", "prepare_revoke", "prepare_rotate", "prepare_store", "status", "versions", "PlanBits", "VaultRec"];
const CORE_MODULE_19_TYPES: &[(&str, CoreLeafKind)] = &[("ExpiringSecret", CoreLeafKind::Generic(1)), ("KeyRef", CoreLeafKind::Generic(1)), ("KeyStatus", CoreLeafKind::Enum(&["Current", "Retired", "Revoked"])), ("KeyUnlock", CoreLeafKind::Plain), ("KeyWrapError", CoreLeafKind::Plain), ("MutationPlan", CoreLeafKind::Generic(1)), ("Rotation", CoreLeafKind::Generic(1)), ("VaultError", CoreLeafKind::Enum(&["NotFound", "Revoked", "Unauthorized", "Corrupt"])), ("VaultWrite", CoreLeafKind::Generic(1)), ("WrappedImportPlan", CoreLeafKind::Generic(1)), ("WrappedVaultKey", CoreLeafKind::Plain), ("PlanBits", CoreLeafKind::Plain), ("VaultRec", CoreLeafKind::Plain)];
const CORE_MODULE_19_DEPENDENCIES: &[&str] = &["core.crypto"];

const CORE_MODULE_20_MEMBERS: &[&str] = &["DataAuthority", "DataColumn", "DataError", "DataErrorKind", "DataFormat", "DataFreshness", "DataInvalidationCause", "DataLimits", "DataLineOptions", "DataLoader", "DataLoaderKind", "DataLoaderStatus", "DataPivotCell", "DataProvenance", "Query", "DataSchema", "DataSnapshot", "DataSnapshotIdentity", "DataSourceIdentity", "DataStatus", "DataStream", "DataTracked", "DataWatch", "DataWatchStatus", "Group", "JetDataPlotAccessibility", "JetDataPlotAggregate", "JetDataPlotAxis", "JetDataPlotBackend", "JetDataPlotCapability", "JetDataPlotChannel", "JetDataPlotColumn", "JetDataPlotDomain", "JetDataPlotEncoding", "JetDataPlotError", "JetDataPlotErrorKind", "JetDataPlotFacet", "JetDataPlotFacetKind", "JetDataPlotField", "JetDataPlotFilterOp", "JetDataPlotInspection", "JetDataPlotInteraction", "JetDataPlotLayer", "JetDataPlotLayout", "JetDataPlotLegend", "JetDataPlotLegendPosition", "JetDataPlotMark", "JetDataPlotPlan", "JetDataPlotProjection", "JetDataPlotRender", "JetDataPlotRenderFormat", "JetDataPlotScale", "JetDataPlotScaleKind", "JetDataPlotSchema", "JetDataPlotSelectedRow", "JetDataPlotSourceFacts", "JetDataPlotSupport", "JetDataPlotTransform", "JetDataPlotValue", "bar_svg", "bar_text", "count", "csv", "csv_reader", "database", "describe", "file", "file_member", "inner_join", "inspect", "inspect_json", "json", "json_reader", "left_join", "line_svg", "line_text", "load", "load_default", "max", "mean", "median", "min", "pivot_sum", "plot", "quantile", "query", "render", "require_bridge", "rolling_mean", "schema", "show", "snapshot", "status", "stddev", "sum", "svg", "text", "track", "url", "value", "variance", "DataSummary", "DataJoin", "JetDataPlot"];
const CORE_MODULE_20_TYPES: &[(&str, CoreLeafKind)] = &[("Query", CoreLeafKind::Generic(1)), ("DataLoader", CoreLeafKind::Generic(1)), ("DataLoaderKind", CoreLeafKind::Enum(&["File", "URL", "Database", "Value"])), ("DataSnapshot", CoreLeafKind::Generic(1)), ("DataStream", CoreLeafKind::Generic(1)), ("DataTracked", CoreLeafKind::Generic(2)), ("DataWatch", CoreLeafKind::Generic(1)), ("DataWatchStatus", CoreLeafKind::Plain), ("Group", CoreLeafKind::Generic(2)), ("DataAuthority", CoreLeafKind::Plain), ("DataColumn", CoreLeafKind::Plain), ("DataError", CoreLeafKind::Plain), ("DataErrorKind", CoreLeafKind::Enum(&["Decode", "Limit", "IO", "Empty", "InvalidArgument", "NonFinite", "Overflow", "State", "Bridge", "Unsupported", "DuplicateKey", "MissingKey", "WrongOwner", "StaleRevision", "InvalidValue"])), ("DataFormat", CoreLeafKind::Enum(&["CSV", "JSON", "JSONL", "Parquet", "Arrow"])), ("DataFreshness", CoreLeafKind::Enum(&["Pending", "Fresh", "Stale", "Error", "Offline", "Cancelled"])), ("DataInvalidationCause", CoreLeafKind::Enum(&["NoCause", "Loader", "Input", "ArchiveMember", "Parameters", "Credential", "Capability", "Manual"])), ("DataLimits", CoreLeafKind::Plain), ("DataLineOptions", CoreLeafKind::Plain), ("DataLoaderStatus", CoreLeafKind::Plain), ("DataPivotCell", CoreLeafKind::Plain), ("DataProvenance", CoreLeafKind::Plain), ("DataSchema", CoreLeafKind::Plain), ("DataSnapshotIdentity", CoreLeafKind::Plain), ("DataSourceIdentity", CoreLeafKind::Plain), ("DataStatus", CoreLeafKind::Plain), ("JetDataPlotAccessibility", CoreLeafKind::Plain), ("JetDataPlotAggregate", CoreLeafKind::Enum(&["Sum", "Mean", "Count", "Min", "Max"])), ("JetDataPlotAxis", CoreLeafKind::Plain), ("JetDataPlotBackend", CoreLeafKind::Enum(&["SVG", "Text"])), ("JetDataPlotCapability", CoreLeafKind::Plain), ("JetDataPlotChannel", CoreLeafKind::Enum(&["X", "Y", "Color", "Size"])), ("JetDataPlotColumn", CoreLeafKind::Plain), ("JetDataPlotDomain", CoreLeafKind::Enum(&["Auto", "Explicit"])), ("JetDataPlotEncoding", CoreLeafKind::Plain), ("JetDataPlotError", CoreLeafKind::Plain), ("JetDataPlotErrorKind", CoreLeafKind::Enum(&["Schema", "Encoding", "Render"])), ("JetDataPlotFacet", CoreLeafKind::Plain), ("JetDataPlotFacetKind", CoreLeafKind::Enum(&["Wrap", "Grid"])), ("JetDataPlotField", CoreLeafKind::Plain), ("JetDataPlotFilterOp", CoreLeafKind::Enum(&["Eq", "Ne", "Lt", "Le", "Gt", "Ge"])), ("JetDataPlotInspection", CoreLeafKind::Plain), ("JetDataPlotInteraction", CoreLeafKind::Enum(&["Hover", "Select", "Pan"])), ("JetDataPlotLayer", CoreLeafKind::Plain), ("JetDataPlotLayout", CoreLeafKind::Plain), ("JetDataPlotLegend", CoreLeafKind::Plain), ("JetDataPlotLegendPosition", CoreLeafKind::Enum(&["Top", "Bottom", "Left", "Right"])), ("JetDataPlotMark", CoreLeafKind::Enum(&["Bar", "Line", "Point"])), ("JetDataPlotPlan", CoreLeafKind::Plain), ("JetDataPlotProjection", CoreLeafKind::Plain), ("JetDataPlotRender", CoreLeafKind::Plain), ("JetDataPlotScale", CoreLeafKind::Plain), ("JetDataPlotScaleKind", CoreLeafKind::Enum(&["Linear", "Log", "Band"])), ("JetDataPlotSchema", CoreLeafKind::Plain), ("JetDataPlotSelectedRow", CoreLeafKind::Plain), ("JetDataPlotSourceFacts", CoreLeafKind::Plain), ("JetDataPlotSupport", CoreLeafKind::Enum(&["Full", "Partial", "Unsupported"])), ("JetDataPlotTransform", CoreLeafKind::Enum(&["Identity", "Filter", "Aggregate"])), ("JetDataPlotValue", CoreLeafKind::Enum(&["Number", "Text", "Bool"])), ("DataSummary", CoreLeafKind::Plain), ("DataJoin", CoreLeafKind::Generic(2)), ("JetDataPlot", CoreLeafKind::Generic(1))];
const CORE_MODULE_20_DEPENDENCIES: &[&str] = &["core.encoding", "core.files"];

const CORE_MODULE_21_MEMBERS: &[&str] = &["DataArrowBatch", "import", "query"];
const CORE_MODULE_21_TYPES: &[(&str, CoreLeafKind)] = &[("DataArrowBatch", CoreLeafKind::Generic(1))];
const CORE_MODULE_21_DEPENDENCIES: &[&str] = &["core.data", "core.encoding"];

const CORE_MODULE_22_MEMBERS: &[&str] = &["authority", "bind", "bind_text", "cancel", "invalidate", "needs_refresh", "offline", "ready", "snapshot_reusable", "source_identity", "status", "stream", "DataLoader"];
const CORE_MODULE_22_TYPES: &[(&str, CoreLeafKind)] = &[("DataLoader", CoreLeafKind::Generic(1))];
const CORE_MODULE_22_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_23_MEMBERS: &[&str] = &["cancel", "collect", "from_items", "is_empty", "len", "next", "skip", "take_n", "DataStream"];
const CORE_MODULE_23_TYPES: &[(&str, CoreLeafKind)] = &[("DataStream", CoreLeafKind::Generic(1))];
const CORE_MODULE_23_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_24_MEMBERS: &[&str] = &["JetDataPlotAccessibility", "JetDataPlotAggregate", "JetDataPlotAxis", "JetDataPlotBackend", "JetDataPlotCapability", "JetDataPlotChannel", "JetDataPlotColumn", "JetDataPlotDomain", "JetDataPlotEncoding", "JetDataPlotError", "JetDataPlotErrorKind", "JetDataPlotFacet", "JetDataPlotFacetKind", "JetDataPlotField", "JetDataPlotFilterOp", "JetDataPlotInspection", "JetDataPlotInteraction", "JetDataPlotLayer", "JetDataPlotLayout", "JetDataPlotLegend", "JetDataPlotLegendPosition", "JetDataPlotMark", "JetDataPlotPlan", "JetDataPlotProjection", "JetDataPlotRender", "JetDataPlotRenderFormat", "JetDataPlotScale", "JetDataPlotScaleKind", "JetDataPlotSchema", "JetDataPlotSelectedRow", "JetDataPlotSourceFacts", "JetDataPlotSupport", "JetDataPlotTransform", "JetDataPlotValue", "bar_svg", "bar_text", "inspect", "inspect_json", "line_svg", "line_text", "plot", "render", "show", "svg", "text"];
const CORE_MODULE_24_TYPES: &[(&str, CoreLeafKind)] = &[("JetDataPlotAccessibility", CoreLeafKind::Plain), ("JetDataPlotAggregate", CoreLeafKind::Enum(&["Sum", "Mean", "Count", "Min", "Max"])), ("JetDataPlotAxis", CoreLeafKind::Plain), ("JetDataPlotBackend", CoreLeafKind::Enum(&["SVG", "Text"])), ("JetDataPlotCapability", CoreLeafKind::Plain), ("JetDataPlotChannel", CoreLeafKind::Enum(&["X", "Y", "Color", "Size"])), ("JetDataPlotColumn", CoreLeafKind::Plain), ("JetDataPlotDomain", CoreLeafKind::Enum(&["Auto", "Explicit"])), ("JetDataPlotEncoding", CoreLeafKind::Plain), ("JetDataPlotError", CoreLeafKind::Plain), ("JetDataPlotErrorKind", CoreLeafKind::Enum(&["Schema", "Encoding", "Render"])), ("JetDataPlotFacet", CoreLeafKind::Plain), ("JetDataPlotFacetKind", CoreLeafKind::Enum(&["Wrap", "Grid"])), ("JetDataPlotField", CoreLeafKind::Plain), ("JetDataPlotFilterOp", CoreLeafKind::Enum(&["Eq", "Ne", "Lt", "Le", "Gt", "Ge"])), ("JetDataPlotInspection", CoreLeafKind::Plain), ("JetDataPlotInteraction", CoreLeafKind::Enum(&["Hover", "Select", "Pan"])), ("JetDataPlotLayer", CoreLeafKind::Plain), ("JetDataPlotLayout", CoreLeafKind::Plain), ("JetDataPlotLegend", CoreLeafKind::Plain), ("JetDataPlotLegendPosition", CoreLeafKind::Enum(&["Top", "Bottom", "Left", "Right"])), ("JetDataPlotMark", CoreLeafKind::Enum(&["Bar", "Line", "Point"])), ("JetDataPlotPlan", CoreLeafKind::Plain), ("JetDataPlotProjection", CoreLeafKind::Plain), ("JetDataPlotRender", CoreLeafKind::Plain), ("JetDataPlotRenderFormat", CoreLeafKind::Enum(&["SVG", "Text", "JSON"])), ("JetDataPlotScale", CoreLeafKind::Plain), ("JetDataPlotScaleKind", CoreLeafKind::Enum(&["Linear", "Log", "Band"])), ("JetDataPlotSchema", CoreLeafKind::Plain), ("JetDataPlotSelectedRow", CoreLeafKind::Plain), ("JetDataPlotSourceFacts", CoreLeafKind::Plain), ("JetDataPlotSupport", CoreLeafKind::Enum(&["Full", "Partial", "Unsupported"])), ("JetDataPlotTransform", CoreLeafKind::Enum(&["Identity", "Filter", "Aggregate"])), ("JetDataPlotValue", CoreLeafKind::Enum(&["Number", "Text", "Bool"]))];
const CORE_MODULE_24_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_25_MEMBERS: &[&str] = &["empty", "merge"];
const CORE_MODULE_25_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_25_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_26_MEMBERS: &[&str] = &["new", "CountMin", "merge"];
const CORE_MODULE_26_TYPES: &[(&str, CoreLeafKind)] = &[("CountMin", CoreLeafKind::Plain)];
const CORE_MODULE_26_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_27_MEMBERS: &[&str] = &["new", "HLL"];
const CORE_MODULE_27_TYPES: &[(&str, CoreLeafKind)] = &[("HLL", CoreLeafKind::Plain)];
const CORE_MODULE_27_DEPENDENCIES: &[&str] = &["core.data", "core.math"];

const CORE_MODULE_28_MEMBERS: &[&str] = &["new", "Reservoir"];
const CORE_MODULE_28_TYPES: &[(&str, CoreLeafKind)] = &[("Reservoir", CoreLeafKind::Plain)];
const CORE_MODULE_28_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_29_MEMBERS: &[&str] = &["new", "Centroid", "Packed", "TDigest"];
const CORE_MODULE_29_TYPES: &[(&str, CoreLeafKind)] = &[("Centroid", CoreLeafKind::Plain), ("Packed", CoreLeafKind::Plain), ("TDigest", CoreLeafKind::Plain)];
const CORE_MODULE_29_DEPENDENCIES: &[&str] = &["core.data", "core.math"];

const CORE_MODULE_30_MEMBERS: &[&str] = &["decode", "migrate", "open", "open_memory", "pool", "policy", "policy_audit", "row_bool", "row_float", "row_int", "row_text", "row_value", "transaction", "DBConnection", "DBScope", "DBPool", "DBLease", "DBPoolReceipt", "DBError", "DBValue", "SQL"];
const CORE_MODULE_30_TYPES: &[(&str, CoreLeafKind)] = &[("DBConnection", CoreLeafKind::Plain), ("DBScope", CoreLeafKind::Plain), ("DBPool", CoreLeafKind::Plain), ("DBLease", CoreLeafKind::Plain), ("DBPoolReceipt", CoreLeafKind::Plain), ("DBError", CoreLeafKind::Plain), ("DBValue", CoreLeafKind::Enum(&["Null", "Int", "Float", "Text", "Bool", "Blob"])), ("SQL", CoreLeafKind::Plain)];
const CORE_MODULE_30_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_31_MEMBERS: &[&str] = &["Address", "Attachment", "DKIMConfig", "EmailError", "Envelope", "Limits", "Mailer", "Message", "RecipientPolicy", "RecipientReport", "SMTPAuth", "SMTPConfig", "SMTPSecurity", "SendReport", "TLSTrust", "address", "attachment", "envelope", "message", "serialize", "smtp", "smtp_from_env", "HTML", "dkim", "limits", "send_report", "smtp_auth"];
const CORE_MODULE_31_TYPES: &[(&str, CoreLeafKind)] = &[("Address", CoreLeafKind::Plain), ("Message", CoreLeafKind::Plain), ("Attachment", CoreLeafKind::Plain), ("Envelope", CoreLeafKind::Plain), ("SMTPSecurity", CoreLeafKind::Enum(&["StartTLS", "TLS"])), ("RecipientPolicy", CoreLeafKind::Enum(&["RequireAll", "DeliverAccepted"])), ("RecipientReport", CoreLeafKind::Plain), ("SendReport", CoreLeafKind::Plain), ("EmailError", CoreLeafKind::Enum(&["Configuration", "DNS", "Connect", "TLS", "Auth", "Protocol", "Rejected", "Transient", "TimedOut", "Cancelled", "DeliveryUnknown"])), ("Limits", CoreLeafKind::Plain), ("SMTPAuth", CoreLeafKind::Enum(&["None", "Password"])), ("TLSTrust", CoreLeafKind::Enum(&["System", "SystemPlusCa"])), ("DKIMConfig", CoreLeafKind::Plain), ("SMTPConfig", CoreLeafKind::Plain), ("Mailer", CoreLeafKind::Plain), ("HTML", CoreLeafKind::Plain)];
const CORE_MODULE_31_DEPENDENCIES: &[&str] = &["core.crypto", "core.crypto.expert", "core.encoding.base64", "core.encoding.hex", "core.net", "core.sys", "core.text"];

const CORE_MODULE_32_MEMBERS: &[&str] = &["DataEvent", "DataTree", "EncodingCause", "EncodingError", "EncodingErrorKind", "EncodingFormat", "EncodingLimits", "Reader", "hex_nibble", "hex_value", "bytes_to_hex", "hex_to_bytes", "wrap32", "HexError"];
const CORE_MODULE_32_TYPES: &[(&str, CoreLeafKind)] = &[("DataTree", CoreLeafKind::Enum(&["Null", "Bool", "Int", "Float", "Text", "Array", "Object"])), ("EncodingLimits", CoreLeafKind::Plain), ("EncodingError", CoreLeafKind::Plain), ("EncodingCause", CoreLeafKind::Plain), ("EncodingFormat", CoreLeafKind::Enum(&["JSON", "JSONL", "CSV", "TOML", "YAML", "XML", "CBOR"])), ("EncodingErrorKind", CoreLeafKind::Enum(&["Syntax", "Truncated", "Unsupported", "Limit", "IO", "State"])), ("DataEvent", CoreLeafKind::Enum(&["Null", "Bool", "Int", "Float", "Number", "Text", "Bytes", "ArrayStart", "ArrayEnd", "ObjectStart", "Key", "ObjectEnd"])), ("Reader", CoreLeafKind::Plain), ("HexError", CoreLeafKind::Plain)];
const CORE_MODULE_32_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_33_MEMBERS: &[&str] = &["decode", "encode", "is_base32", "b32encode", "b32decode", "b32hexencode", "b32hexdecode", "Base32Error"];
const CORE_MODULE_33_TYPES: &[(&str, CoreLeafKind)] = &[("Base32Error", CoreLeafKind::Plain)];
const CORE_MODULE_33_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_34_MEMBERS: &[&str] = &["decode", "decode_url", "encode", "encode_url", "encode_url_padded", "decode_padded", "pad", "unpad", "is_base64", "b64encode", "standard_b64encode", "urlsafe_b64encode", "b64decode", "standard_b64decode", "urlsafe_b64decode", "b16encode", "b16decode", "b32encode", "b32decode", "b32hexencode", "b32hexdecode", "a85encode", "a85decode", "b85encode", "b85decode", "z85encode", "z85decode", "encodebytes", "decodebytes", "b2a_base64", "a2b_base64", "Base64Error"];
const CORE_MODULE_34_TYPES: &[(&str, CoreLeafKind)] = &[("Base64Error", CoreLeafKind::Plain)];
const CORE_MODULE_34_DEPENDENCIES: &[&str] = &["core.encoding", "core.encoding.hex", "core.encoding.base32"];

const CORE_MODULE_35_MEMBERS: &[&str] = &["pack_u8", "pack_i8", "pack_u16le", "pack_u16be", "pack_u32le", "pack_u32be", "pack_u64le", "pack_u64be", "unpack_u8", "unpack_u16le", "unpack_u16be", "unpack_u32le", "unpack_u32be", "unpack_u64le", "unpack_u64be", "sign_extend", "pack_f64le", "pack_f64be", "unpack_f64le", "unpack_f64be", "calcsize", "pack", "unpack", "iter_unpack", "BinaryError"];
const CORE_MODULE_35_TYPES: &[(&str, CoreLeafKind)] = &[("BinaryError", CoreLeafKind::Enum(&["Range", "Format", "Values", "Buffer"]))];
const CORE_MODULE_35_DEPENDENCIES: &[&str] = &["core.math"];

const CORE_MODULE_36_MEMBERS: &[&str] = &["CBORError", "CBORErrorKind", "CBOROptions", "CBORReader", "CBORWriter", "decode", "parse", "reader", "to_bytes", "to_bytes_canonical", "writer"];
const CORE_MODULE_36_TYPES: &[(&str, CoreLeafKind)] = &[("CBORReader", CoreLeafKind::Plain), ("CBORWriter", CoreLeafKind::Plain), ("CBOROptions", CoreLeafKind::Plain), ("CBORError", CoreLeafKind::Plain), ("CBORErrorKind", CoreLeafKind::Enum(&["Syntax", "Truncated", "Unsupported", "Limit", "TypeMismatch", "TrailingData", "NonCanonical"]))];
const CORE_MODULE_36_DEPENDENCIES: &[&str] = &["core.encoding", "core.math"];

const CORE_MODULE_37_MEMBERS: &[&str] = &["CSVReader", "CSVRow", "CSVWriter", "decode", "parse", "query", "reader", "rows", "to_string", "writer", "dict_rows", "dict_get", "dict_get_or", "write_dict", "fieldnames", "DictRow"];
const CORE_MODULE_37_TYPES: &[(&str, CoreLeafKind)] = &[("CSVReader", CoreLeafKind::Plain), ("CSVWriter", CoreLeafKind::Plain), ("CSVRow", CoreLeafKind::Plain), ("DictRow", CoreLeafKind::Plain)];
const CORE_MODULE_37_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_38_MEMBERS: &[&str] = &["HexError", "decode", "encode", "encode_prefixed", "encode_upper", "is_hex", "hexlify", "unhexlify", "b2a_hex", "a2b_hex", "crc_hqx", "crc32", "b2a_base64", "a2b_base64", "b2a_qp", "a2b_qp", "b2a_uu", "a2b_uu", "encode_sep", "dump"];
const CORE_MODULE_38_TYPES: &[(&str, CoreLeafKind)] = &[("HexError", CoreLeafKind::Plain)];
const CORE_MODULE_38_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_39_MEMBERS: &[&str] = &["Pair", "Section", "Ini", "empty", "parse", "to_string", "sections", "has_section", "has_option", "get", "get_or", "get_int", "get_bool", "get_float", "items", "options", "set", "remove_option", "remove_section", "defaults", "INIError"];
const CORE_MODULE_39_TYPES: &[(&str, CoreLeafKind)] = &[("Pair", CoreLeafKind::Plain), ("Section", CoreLeafKind::Plain), ("Ini", CoreLeafKind::Plain), ("INIError", CoreLeafKind::Plain)];
const CORE_MODULE_39_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_40_MEMBERS: &[&str] = &["JSONReader", "JSONWriter", "canonical", "decode", "dump", "dumps", "events", "load", "loads", "parse", "parse_allow_duplicates", "patch", "patch_with_limits", "pointer", "reader", "reader_allow_duplicates", "to_string", "to_string_pretty", "writer"];
const CORE_MODULE_40_TYPES: &[(&str, CoreLeafKind)] = &[("JSONReader", CoreLeafKind::Plain), ("JSONWriter", CoreLeafKind::Plain)];
const CORE_MODULE_40_DEPENDENCIES: &[&str] = &["core.encoding", "core.math"];

const CORE_MODULE_41_MEMBERS: &[&str] = &["JSONLReader", "JSONLWriter", "parse", "reader", "to_string", "writer", "loads", "dumps", "count_rows", "append_line", "first"];
const CORE_MODULE_41_TYPES: &[(&str, CoreLeafKind)] = &[("JSONLReader", CoreLeafKind::Plain), ("JSONLWriter", CoreLeafKind::Plain)];
const CORE_MODULE_41_DEPENDENCIES: &[&str] = &["core.encoding", "core.encoding.json"];

const CORE_MODULE_42_MEMBERS: &[&str] = &["decode", "load", "loads", "parse", "to_string"];
const CORE_MODULE_42_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_42_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_43_MEMBERS: &[&str] = &["XMLCanonical", "XMLCanonicalMode", "XMLEncoding", "XMLEntityPolicy", "XMLError", "XMLLexicalPolicy", "XMLLimits", "XMLParseOptions", "XMLReader", "XMLReason", "XMLRenderOptions", "XMLWriter", "attribute", "canonical", "content", "decode", "expanded_name", "parse", "parse_bytes", "parse_with", "reader", "root", "to_bytes", "to_string"];
const CORE_MODULE_43_TYPES: &[(&str, CoreLeafKind)] = &[("XMLReader", CoreLeafKind::Plain), ("XMLWriter", CoreLeafKind::Plain), ("XMLError", CoreLeafKind::Plain), ("XMLReason", CoreLeafKind::Enum(&["Syntax", "Truncated", "ForbiddenDTD", "UnknownEntity"])), ("XMLCanonical", CoreLeafKind::Plain), ("XMLCanonicalMode", CoreLeafKind::Enum(&["Inclusive", "Exclusive"])), ("XMLEncoding", CoreLeafKind::Enum(&["UTF8", "UTF16"])), ("XMLEntityPolicy", CoreLeafKind::Enum(&["PredefinedOnly", "Reject"])), ("XMLLexicalPolicy", CoreLeafKind::Plain), ("XMLLimits", CoreLeafKind::Plain), ("XMLParseOptions", CoreLeafKind::Plain), ("XMLRenderOptions", CoreLeafKind::Plain)];
const CORE_MODULE_43_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_44_MEMBERS: &[&str] = &["async_result", "decision_hook", "hook", "new", "policy_sync", "scope", "with_policy", "Event", "AsyncEvent", "Hook", "DecisionHook", "HookDecision", "HookOutcome", "Subscription", "EventScope", "EventPolicy", "EventTrace", "AsyncPolicy", "Overflow", "FailurePolicy", "DispatchReport", "DispatchFailure", "DispatchState", "HookPolicy", "EventConfigError"];
const CORE_MODULE_44_TYPES: &[(&str, CoreLeafKind)] = &[("Event", CoreLeafKind::Generic(1)), ("AsyncEvent", CoreLeafKind::Generic(2)), ("Hook", CoreLeafKind::Generic(2)), ("DecisionHook", CoreLeafKind::Generic(2)), ("HookDecision", CoreLeafKind::Generic(2)), ("HookOutcome", CoreLeafKind::Generic(2)), ("Subscription", CoreLeafKind::Plain), ("EventScope", CoreLeafKind::Plain), ("EventPolicy", CoreLeafKind::Plain), ("EventTrace", CoreLeafKind::Plain), ("AsyncPolicy", CoreLeafKind::Plain), ("Overflow", CoreLeafKind::Enum(&["Block", "DropNewest", "DropOldest"])), ("FailurePolicy", CoreLeafKind::Enum(&["StopFirst", "Collect", "Log", "Ignore"])), ("DispatchReport", CoreLeafKind::Generic(1)), ("DispatchFailure", CoreLeafKind::Generic(1)), ("DispatchState", CoreLeafKind::Enum(&["Delivered", "HandlerFailed", "DroppedNewest", "DroppedOldest", "Closed", "Cancelled", "DeadlineExceeded"])), ("HookPolicy", CoreLeafKind::Enum(&["FirstCancelElseTransform"])), ("EventConfigError", CoreLeafKind::Enum(&["InvalidCapacity"]))];
const CORE_MODULE_44_DEPENDENCIES: &[&str] = &["core.mem"];

const CORE_MODULE_45_MEMBERS: &[&str] = &["absolute", "append", "append_all", "basename", "canonicalize", "commonpath", "copy", "copy_dir", "create", "create_dir", "create_dir_all", "cwd", "getcwd", "dirname", "exists", "expanduser", "fnmatch", "fsync", "glob", "hard_link", "home", "is_absolute", "is_dir", "is_file", "is_symlink", "join", "list_dir", "listdir", "lock", "map", "open", "read", "read_at", "read_bytes", "read_lines", "read_link", "relative", "relocate", "remove", "remove_all", "remove_dir", "rename", "scope", "set_mode", "stat", "stem", "suffix", "symlink", "temp_dir", "temp_file", "touch", "walk", "walk_files", "walk_parallel", "which", "with_name", "with_suffix", "write", "write_at", "write_atomic", "write_bytes", "write_lines", "DirEntry", "FileLock", "FileReader", "FileScope", "FileWriter", "MappedFile", "Stat", "TempDir", "TempFile", "WalkEntry", "collapse", "commonprefix", "copy2", "ensure_parent", "expandvars", "fnmatch_filter", "getmtime", "getsize", "glob_match", "glob_recursive", "is_abs", "is_mount", "lexists", "makedirs", "move", "normpath", "rmtree", "samefile", "split", "split_slash", "splitext", "replace", "relativeto", "chmod", "copyfile", "mkdir", "rmdir", "unlink", "joinpath", "chdir", "scandir", "gettempdir", "getenv", "chown", "is_fifo", "is_socket", "lstat", "mkdtemp", "mktemp", "readdir", "truncate", "walkdir", "createdirectory", "filesize", "delete", "cp", "rm", "realpath", "pwd", "tmpdir", "topath", "cd"];
const CORE_MODULE_45_TYPES: &[(&str, CoreLeafKind)] = &[("FileScope", CoreLeafKind::Plain), ("DirEntry", CoreLeafKind::Plain), ("FileLock", CoreLeafKind::Plain), ("FileReader", CoreLeafKind::Plain), ("FileWriter", CoreLeafKind::Plain), ("MappedFile", CoreLeafKind::Plain), ("Stat", CoreLeafKind::Plain), ("TempDir", CoreLeafKind::Plain), ("TempFile", CoreLeafKind::Plain), ("WalkEntry", CoreLeafKind::Plain)];
const CORE_MODULE_45_DEPENDENCIES: &[&str] = &["core.text", "core.sys"];

const CORE_MODULE_46_MEMBERS: &[&str] = &["decode", "parse", "to_string"];
const CORE_MODULE_46_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_46_DEPENDENCIES: &[&str] = &["core.encoding", "core.encoding.json"];

const CORE_MODULE_47_MEMBERS: &[&str] = &["Path", "as_posix", "cwd", "exists", "glob", "home", "is_absolute", "is_dir", "is_file", "is_relative", "is_symlink", "iterdir", "join", "join_path", "mkdir", "name", "of", "parent", "parts", "read_bytes", "read_text", "relative_to", "rename", "replace", "resolve", "rmdir", "show", "stem", "suffix", "suffixes", "touch", "unlink", "with_name", "with_stem", "with_suffix", "write_bytes", "write_text", "absolute", "anchor", "append_text", "as_uri", "as_windows", "chmod", "collapse", "copy_file", "copy_into", "drive", "ensure_dir", "equals", "expanduser", "from_parts", "hardlink_to", "is_empty", "is_relative_to", "join_many", "match_glob", "match_path", "mtime", "open_read", "open_write", "parents", "read_lines", "readlink", "resolve_pure", "rglob", "rmtree", "root", "samefile", "size", "stat", "symlink_to", "walk", "which", "with_segments", "write_lines", "path"];
const CORE_MODULE_47_TYPES: &[(&str, CoreLeafKind)] = &[("Path", CoreLeafKind::Plain)];
const CORE_MODULE_47_DEPENDENCIES: &[&str] = &["core.files"];

const CORE_MODULE_48_MEMBERS: &[&str] = &["FontFace", "FontStyle", "Glyph", "GlyphRun", "GlyphShaper", "shape", "system", "shape_with"];
const CORE_MODULE_48_TYPES: &[(&str, CoreLeafKind)] = &[("FontFace", CoreLeafKind::Plain), ("FontStyle", CoreLeafKind::Enum(&["Body", "Title", "Monospace"])), ("Glyph", CoreLeafKind::Plain), ("GlyphRun", CoreLeafKind::Plain), ("GlyphShaper", CoreLeafKind::Enum(&["HarfBuzz", "HeadlessFallback"]))];
const CORE_MODULE_48_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_49_MEMBERS: &[&str] = &["Backend", "Replay", "Scene", "run"];
const CORE_MODULE_49_TYPES: &[(&str, CoreLeafKind)] = &[("Backend", CoreLeafKind::Plain), ("Replay", CoreLeafKind::Plain), ("Scene", CoreLeafKind::Plain)];
const CORE_MODULE_49_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_50_MEMBERS: &[&str] = &["begin_drawing", "clear_background", "close_window", "color", "draw_rectangle", "draw_sprite", "draw_text", "end_drawing", "gamepad_axis", "gamepad_down", "key_down", "load_sound", "load_texture_atlas", "play_sound", "set_target_fps", "window_open", "window_ready", "window_should_close", "RaylibColor", "RaylibWindow", "RaylibTextureAtlas", "RaylibSound"];
const CORE_MODULE_50_TYPES: &[(&str, CoreLeafKind)] = &[("RaylibColor", CoreLeafKind::Plain), ("RaylibWindow", CoreLeafKind::Plain), ("RaylibTextureAtlas", CoreLeafKind::Plain), ("RaylibSound", CoreLeafKind::Plain)];
const CORE_MODULE_50_DEPENDENCIES: &[&str] = &["core.game"];

const CORE_MODULE_51_MEMBERS: &[&str] = &["Body", "Cookie", "HTTPError", "HTTPOperation", "HTTPRequest", "HTTPResponse", "Header", "Headers", "Method", "Query", "QueryPair", "Status", "StatusLine", "Version", "basic_auth", "bearer_auth", "body_as_bytes", "body_as_text", "body_bytes", "body_empty", "body_len", "body_text", "cookie", "cookie_encode", "cookies_parse", "delete", "exchange", "form_decode", "form_encode", "get", "head", "headers", "headers_all", "headers_append", "headers_encode", "headers_get", "headers_parse", "headers_remove", "headers_set", "is_client_error", "is_informational", "is_redirect", "is_server_error", "is_success", "method_parse", "method_text", "parse_request", "parse_response", "patch", "post", "put", "query", "query_add", "query_all", "query_decode", "query_encode", "query_get", "query_set", "reason_phrase", "request", "send_request", "serve", "status", "status_bad_request", "status_created", "status_forbidden", "status_found", "status_moved", "status_no_content", "status_not_found", "status_ok", "status_server_error", "status_timeout", "status_unauthorized", "version_parse", "version_text", "with_body", "with_header", "with_query", "CorsPolicy", "HTTPMux", "HTTPRoute", "HTTPServerTLS", "content_length", "has_header", "request_with", "response_json_headers", "response_ok", "response_text_headers", "status_accepted", "status_already_reported", "status_bad_gateway", "status_class", "status_conflict", "status_continue", "status_expectation_failed", "status_failed_dependency", "status_gateway_timeout", "status_gone", "status_http_version", "status_legal", "status_length_required", "status_locked", "status_method_not_allowed", "status_multi_status", "status_non_authoritative", "status_not_acceptable", "status_not_implemented", "status_not_modified", "status_partial", "status_payload_too_large", "status_payment_required", "status_permanent", "status_precondition_failed", "status_precondition_required", "status_range_unsat", "status_reset_content", "status_see_other", "status_switching", "status_teapot", "status_temporary", "status_too_early", "status_too_many", "status_unavailable", "status_unprocessable", "status_unsupported_media", "status_upgrade_required", "status_uri_too_long"];
const CORE_MODULE_51_TYPES: &[(&str, CoreLeafKind)] = &[("Body", CoreLeafKind::Enum(&["Empty", "Text", "Bytes"])), ("Cookie", CoreLeafKind::Plain), ("HTTPError", CoreLeafKind::Enum(&["InvalidMethod", "InvalidURL", "InvalidHeader", "InvalidStatus", "BodyConsumed", "InvalidFraming", "UnsupportedEncoding", "Cancelled", "BodyTooLarge", "Resolve", "Connect", "TLS", "Timeout", "Proxy", "Redirect", "Protocol", "IO", "Policy", "ResourceUnavailable", "Internal", "UnsupportedTarget"])), ("HTTPOperation", CoreLeafKind::Enum(&["ClientConnect", "ServerBind", "ServeListener"])), ("HTTPRequest", CoreLeafKind::Plain), ("HTTPResponse", CoreLeafKind::Plain), ("Header", CoreLeafKind::Plain), ("Headers", CoreLeafKind::Plain), ("Method", CoreLeafKind::Enum(&["Get", "Head", "Post", "Put", "Patch", "Delete", "Options", "Trace", "Connect", "Custom"])), ("Query", CoreLeafKind::Plain), ("QueryPair", CoreLeafKind::Plain), ("Status", CoreLeafKind::Plain), ("StatusLine", CoreLeafKind::Plain), ("Version", CoreLeafKind::Enum(&["HTTP10", "HTTP11", "HTTP2", "HTTP3"])), ("CorsPolicy", CoreLeafKind::Plain), ("HTTPMux", CoreLeafKind::Plain), ("HTTPRoute", CoreLeafKind::Plain), ("HTTPServerTLS", CoreLeafKind::Plain)];
const CORE_MODULE_51_DEPENDENCIES: &[&str] = &["core.net"];

const CORE_MODULE_52_MEMBERS: &[&str] = &["Client", "Proxy", "RedirectPolicy", "delete", "get", "head", "header", "patch", "post", "put", "request", "send", "set_header", "timeout_redirects", "session", "session_header", "session_timeout", "session_retries", "session_redirects", "session_auth", "session_proxy", "session_cookie", "cookie_header", "session_request", "session_get", "session_post", "session_put", "session_patch", "session_delete", "session_head", "session_json", "session_form", "redirect_limit", "with_proxy", "user_agent", "bearer", "accept", "content_type", "Session"];
const CORE_MODULE_52_TYPES: &[(&str, CoreLeafKind)] = &[("Session", CoreLeafKind::Plain), ("Client", CoreLeafKind::Plain), ("Proxy", CoreLeafKind::Plain), ("RedirectPolicy", CoreLeafKind::Enum(&["Disabled", "Limited", "All"]))];
const CORE_MODULE_52_DEPENDENCIES: &[&str] = &["core.http"];

const CORE_MODULE_53_MEMBERS: &[&str] = &["access_log", "bind", "cors", "cors_policy", "json", "mux", "request_id", "response", "serve", "serve_once", "serve_once_listener", "sse", "static_file", "static_file_range", "static_files", "tls"];
const CORE_MODULE_53_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_53_DEPENDENCIES: &[&str] = &["core.http"];

const CORE_MODULE_54_MEMBERS: &[&str] = &["JobQueue", "JobPayload", "JobResult", "JobError", "JobQueueReceipt", "JobQueueClaim", "JobQueueDeliveryPolicy", "JobQueueEvent", "JobQueueRecord", "JobQueueState", "JobQueueStatus", "queue", "ack", "cancel", "claim", "enqueue", "fail", "named", "status", "with_max_attempts", "queue_name", "state", "attempts", "payload", "event", "events", "is_terminal", "is_cancelled"];
const CORE_MODULE_54_TYPES: &[(&str, CoreLeafKind)] = &[("JobQueue", CoreLeafKind::Plain), ("JobPayload", CoreLeafKind::Plain), ("JobResult", CoreLeafKind::Plain), ("JobError", CoreLeafKind::Enum(&["Empty", "Duplicate", "Failed", "Missing"])), ("JobQueueReceipt", CoreLeafKind::Plain), ("JobQueueClaim", CoreLeafKind::Plain), ("JobQueueDeliveryPolicy", CoreLeafKind::Enum(&["AtLeastOnce"])), ("JobQueueEvent", CoreLeafKind::Plain), ("JobQueueRecord", CoreLeafKind::Plain), ("JobQueueState", CoreLeafKind::Enum(&["Queued", "Running", "Retrying", "Completed", "Failed", "DeadLettered", "Cancelled"])), ("JobQueueStatus", CoreLeafKind::Plain)];
const CORE_MODULE_54_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_55_MEMBERS: &[&str] = &["bool", "close", "counter", "critical", "debug", "debug_fields", "disable", "enabled", "enter", "error", "error_fields", "fatal", "field", "float", "flush", "info", "info_fields", "int", "otlp_file", "redact", "sample_every", "set_level", "set_sink", "set_trace_id", "setup", "span", "warn", "warn_fields", "LogField", "Pair", "LogRecord", "Formatter", "Handler", "record", "formatter", "handler", "handler_format", "format", "handle", "add", "group", "time", "clear", "log", "warning"];
const CORE_MODULE_55_TYPES: &[(&str, CoreLeafKind)] = &[("LogRecord", CoreLeafKind::Plain), ("Formatter", CoreLeafKind::Plain), ("Handler", CoreLeafKind::Plain), ("LogField", CoreLeafKind::Plain), ("Pair", CoreLeafKind::Plain)];
const CORE_MODULE_55_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_56_MEMBERS: &[&str] = &["abs", "acos", "acosh", "asin", "asinh", "atan", "atan2", "atanh", "binomial", "cbrt", "ceil", "checked_abs", "checked_add", "checked_div", "checked_mul", "checked_neg", "checked_pow", "checked_rem", "checked_sub", "clamp", "cmp", "copysign", "conj", "cos", "cosh", "cot", "decimal", "degrees", "digits", "div_mod", "div_rem", "e", "erf", "erfc", "exp", "exp2", "exp_m1", "factorial", "floor", "fma", "float32", "float64", "fract", "fraction", "frexp", "from_bits", "gamma", "gcd", "hypot", "ilogb", "imag", "infinity", "int_pow", "inv", "is_canonical", "is_even", "is_finite", "is_inf", "is_integer", "is_nan", "is_normal", "is_odd", "is_signed", "is_subnormal", "is_zero", "isqrt", "lcm", "ldexp", "leading_ones", "lerp", "lgamma", "ln", "ln_1p", "log", "log10", "log2", "logb", "max", "min", "modf", "muladd", "nan", "next_after", "nextafter", "next_down", "next_up", "pi", "pow", "radians", "radix", "real", "round", "saturating_add", "saturating_mul", "saturating_sub", "scaleb", "sign", "sign_bit", "significand", "signum", "sin", "sin_cos", "sinh", "sqrt", "tan", "tanh", "tau", "to_bits", "trailing_ones", "trunc", "truncate", "ulp", "zero", "Fraction", "abs_diff", "abs_float", "clamp_float", "comb", "dist", "even", "fmod", "fsum", "gcd_many", "hypot3", "in_range", "isclose", "isfinite", "isinf", "isnan", "lcm_many", "max_float", "midpoint", "min_float", "odd", "perm", "powmod", "prod", "prod_int", "remainder", "sum_int", "sumprod", "tau_const", "log1p", "xor", "random", "fabs", "expm1"];
const CORE_MODULE_56_TYPES: &[(&str, CoreLeafKind)] = &[("Fraction", CoreLeafKind::Plain)];
const CORE_MODULE_56_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_57_MEMBERS: &[&str] = &["bool", "bytes", "exponential", "float", "float_range", "int", "normal", "pick", "rng", "sample", "seed", "shuffle", "split", "weighted_pick", "random", "randint", "uniform", "normalvariate", "gauss", "expovariate", "randbytes", "getrandbits", "randrange", "choice", "choices", "triangular", "gammavariate", "betavariate", "lognormvariate", "paretovariate", "weibullvariate", "vonmisesvariate", "binomialvariate"];
const CORE_MODULE_57_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_57_DEPENDENCIES: &[&str] = &["core.math"];

const CORE_MODULE_58_MEMBERS: &[&str] = &["binomial", "cartesian", "combinations", "combinations_count", "combinations_with_replacement", "factorial", "permutations", "permutations_count", "product", "accumulate", "batched", "chain", "chain_from", "compress", "count_from", "count", "cycle", "drop", "dropwhile", "filterfalse", "groupby", "islice", "starmap", "tee", "falling_factorial", "flatten", "multinomial", "ncr", "npr", "pairwise", "powerset", "repeat", "reverse", "rising_factorial", "takewhile", "unique", "windows", "zip_longest", "take_n"];
const CORE_MODULE_58_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_58_DEPENDENCIES: &[&str] = &["core.math"];

const CORE_MODULE_59_MEMBERS: &[&str] = &["NormalDist", "SlopeIntercept", "correlation", "covariance", "cumsum", "fmean", "geometric_mean", "harmonic_mean", "kde", "kde_random", "linear_regression", "max", "mean", "median", "median_grouped", "median_high", "median_low", "min", "mode", "multimode", "percentile", "prod", "pstdev", "pvariance", "quantile", "range", "stdev", "sum", "variance", "zscore", "clip", "count", "covariance_population", "cumprod", "describe", "diff", "ewma", "histogram", "iqr", "kurtosis", "mad", "mean_abs_deviation", "moving_average", "pearson", "quantiles", "r_squared", "rank", "residuals", "skew", "spearman", "sumprod", "weighted_mean", "winsorize", "normal_dist"];
const CORE_MODULE_59_TYPES: &[(&str, CoreLeafKind)] = &[("NormalDist", CoreLeafKind::Plain), ("SlopeIntercept", CoreLeafKind::Plain)];
const CORE_MODULE_59_DEPENDENCIES: &[&str] = &["core.math", "core.math.random"];

const CORE_MODULE_60_MEMBERS: &[&str] = &["AllocError", "Arena", "Atomic", "Bump", "Fixed", "Pin", "Pool", "Ptr", "address_of", "from_addr", "pin", "volatile_read", "volatile_write"];
const CORE_MODULE_60_TYPES: &[(&str, CoreLeafKind)] = &[("AllocError", CoreLeafKind::Plain), ("Atomic", CoreLeafKind::Plain), ("Arena", CoreLeafKind::Plain), ("Bump", CoreLeafKind::Plain), ("Fixed", CoreLeafKind::Plain), ("Pin", CoreLeafKind::Generic(1)), ("Pool", CoreLeafKind::Generic(1))];
const CORE_MODULE_60_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_61_MEMBERS: &[&str] = &["guard"];
const CORE_MODULE_61_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_61_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_62_MEMBERS: &[&str] = &["is_loaded", "load", "path", "unload", "CompiledModule"];
const CORE_MODULE_62_TYPES: &[(&str, CoreLeafKind)] = &[("CompiledModule", CoreLeafKind::Plain)];
const CORE_MODULE_62_DEPENDENCIES: &[&str] = &["core.compiler", "core.files"];

const CORE_MODULE_63_MEMBERS: &[&str] = &["dns_a", "dns_a_at", "dns_aaaa", "dns_aaaa_at", "dns_ptr", "dns_srv", "dns_srv_at", "dns_srv_port", "dns_srv_priority", "dns_srv_target", "dns_srv_weight", "dns_txt", "dns_txt_at", "error_address", "error_message", "error_name", "error_operation", "error_os_code", "getservbyname", "getservbyport", "ip_addr", "ip_is_ipv4", "ip_to_string", "listener_local_socket_addr", "nodelay", "ready_readable", "ready_writable", "sendfile", "set_nodelay", "set_read_timeout", "set_timeout", "set_ttl", "set_write_timeout", "socket_addr", "socket_addr_parse", "socket_host", "socket_port", "socket_to_string", "socket_type", "tcp_accept", "tcp_close", "tcp_connect", "tcp_connect_addr", "tcp_connect_happy", "tcp_connect_timeout", "tcp_listen", "tcp_listen_addr", "tcp_local_addr", "tcp_local_socket_addr", "tcp_peer_addr", "tcp_peer_socket_addr", "tcp_read", "tcp_read_bytes", "tcp_read_text", "tcp_ready", "tcp_reply", "tcp_shutdown", "tcp_write", "tcp_write_all_bytes", "tcp_write_bytes", "tcp_write_text", "ttl", "udp_bind", "udp_bind_addr", "udp_local_addr", "udp_packet_addr", "udp_packet_bytes", "udp_packet_data", "udp_packet_original_len", "udp_packet_truncated", "udp_receive", "udp_recv_from", "udp_send_bytes_to", "udp_send_to", "udp_set_timeout", "unix_accept", "unix_close", "unix_connect", "unix_listen", "unix_read", "unix_read_bytes", "unix_shutdown", "unix_write", "unix_write_all_bytes", "TCPListener", "TCPStream", "UDPPacket", "UDPSocket", "SocketAddr", "SRVRecord", "UnixListener", "UnixStream", "create_connection", "create_server", "send", "gethostbyname", "gethostbyaddr", "gethostname", "addressfamily"];
const CORE_MODULE_63_TYPES: &[(&str, CoreLeafKind)] = &[("SocketAddr", CoreLeafKind::Plain), ("SRVRecord", CoreLeafKind::Plain), ("TCPListener", CoreLeafKind::Plain), ("TCPStream", CoreLeafKind::Plain), ("UDPPacket", CoreLeafKind::Plain), ("UDPSocket", CoreLeafKind::Plain), ("UnixListener", CoreLeafKind::Plain), ("UnixStream", CoreLeafKind::Plain)];
const CORE_MODULE_63_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_64_MEMBERS: &[&str] = &["extension", "from_extension", "parse", "MIMEError", "MIME"];
const CORE_MODULE_64_TYPES: &[(&str, CoreLeafKind)] = &[("MIMEError", CoreLeafKind::Enum(&["Syntax"])), ("MIME", CoreLeafKind::Plain)];
const CORE_MODULE_64_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_65_MEMBERS: &[&str] = &["ClientConfig", "ClientIdentity", "RootCertificates", "TLSCertificate", "TLSPeerIdentity", "TLSVersion", "client", "close", "read", "read_text", "write", "write_all", "write_text", "TLSStream", "sni", "peer_port", "unwrap", "version_name", "parse_version", "is_tls13", "is_tls12", "config", "with_alpn", "alpn_h2", "identity", "roots", "connect_host", "flags", "has_alpn"];
const CORE_MODULE_65_TYPES: &[(&str, CoreLeafKind)] = &[("ClientConfig", CoreLeafKind::Plain), ("ClientIdentity", CoreLeafKind::Plain), ("RootCertificates", CoreLeafKind::Plain), ("TLSCertificate", CoreLeafKind::Plain), ("TLSPeerIdentity", CoreLeafKind::Plain), ("TLSVersion", CoreLeafKind::Enum(&["TLS12", "TLS13"])), ("TLSStream", CoreLeafKind::Plain)];
const CORE_MODULE_65_DEPENDENCIES: &[&str] = &["core.crypto.random", "core.net"];

const CORE_MODULE_66_MEMBERS: &[&str] = &["data", "file", "from_parts", "parse", "percent_decode", "percent_encode", "query", "URL", "geturl", "unparse", "urljoin", "parse_qsl", "parse_qs", "urlencode", "split_fragment", "quote", "quote_from_bytes", "quote_plus", "unquote", "unquote_to_bytes", "unquote_plus", "urlparse", "urlsplit", "urlunparse", "urlunsplit", "urldefrag", "URLError"];
const CORE_MODULE_66_TYPES: &[(&str, CoreLeafKind)] = &[("URL", CoreLeafKind::Plain), ("URLError", CoreLeafKind::Enum(&["Syntax", "Percent"]))];
const CORE_MODULE_66_DEPENDENCIES: &[&str] = &["core.net.mime"];

const CORE_MODULE_67_MEMBERS: &[&str] = &["connect", "upgrade", "WsConn", "WsError"];
const CORE_MODULE_67_TYPES: &[(&str, CoreLeafKind)] = &[("WsConn", CoreLeafKind::Plain), ("WsError", CoreLeafKind::Enum(&["InvalidURL", "InvalidHandshake", "Protocol", "Timeout", "Closed", "Cancelled", "UnsupportedTarget", "MessageTooLarge", "IO"]))];
const CORE_MODULE_67_DEPENDENCIES: &[&str] = &["core.net", "core.net.url"];

const CORE_MODULE_68_MEMBERS: &[&str] = &["IPv4", "IPv6", "Network", "is_global", "is_link_local", "is_loopback", "is_multicast", "is_private", "is_reserved", "is_unspecified", "ipv4", "ipv4_from_int", "ipv4_int", "ipv4_to_string", "ipv6_is_link_local", "ipv6_is_loopback", "ipv6_is_unspecified", "ipv6_to_string", "network", "network_broadcast", "network_contains", "network_hosts", "parse_ipv4", "parse_ipv6", "IPv4Interface", "compare_ipv4", "from_packed_ipv4", "hosts", "interface", "ipv4_equals", "ipv6_compressed", "ipv6_is_global", "ipv6_is_multicast", "ipv6_is_private", "ipv6_packed", "is_benchmarking", "is_carrier_grade_nat", "is_documentation", "is_shared", "network_first", "network_last", "network_overlaps", "num_addresses", "packed_ipv4", "reverse_pointer", "subnet_of", "subnets", "supernet", "supernet_of", "with_hostmask", "with_netmask", "with_prefixlen"];
const CORE_MODULE_68_TYPES: &[(&str, CoreLeafKind)] = &[("IPv4", CoreLeafKind::Plain), ("IPv6", CoreLeafKind::Plain), ("IPv4Interface", CoreLeafKind::Plain), ("Network", CoreLeafKind::Plain)];
const CORE_MODULE_68_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_69_MEMBERS: &[&str] = &["Perf", "default_fidelity", "fidelity", "is_full", "is_low", "of", "override_fidelity", "reset_fidelity", "scale"];
const CORE_MODULE_69_TYPES: &[(&str, CoreLeafKind)] = &[("Perf", CoreLeafKind::Plain)];
const CORE_MODULE_69_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_70_MEMBERS: &[&str] = &["load"];
const CORE_MODULE_70_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_70_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_71_MEMBERS: &[&str] = &["keep", "always", "identity", "identity_int", "identity_float", "identity_bool", "const_int", "const_bool", "not_bool", "min_int", "max_int"];
const CORE_MODULE_71_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_71_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_72_MEMBERS: &[&str] = &["ProcessSignal", "args", "argv", "check", "cmd", "current_pid", "cwd", "env", "env_get", "exit", "on_signal", "pipeline", "run_spec", "status_ok", "stdin_text", "which", "Completed", "ProcessReceipt", "ProcessSpec", "arg", "args_extend", "call", "capture", "check_call", "check_output", "combined_output", "env_get_or", "env_keys", "env_set", "env_truthy", "failed", "getoutput", "getstatusoutput", "list2cmdline", "shell", "signal_number", "stderr_lines", "stdout_lines", "exited"];
const CORE_MODULE_72_TYPES: &[(&str, CoreLeafKind)] = &[("ProcessSignal", CoreLeafKind::Enum(&["Interrupt", "Terminate", "Hangup", "Child", "User1", "User2"])), ("Completed", CoreLeafKind::Plain), ("ProcessReceipt", CoreLeafKind::Plain), ("ProcessSpec", CoreLeafKind::Plain)];
const CORE_MODULE_72_DEPENDENCIES: &[&str] = &["core.files", "core.sys", "core.text"];

const CORE_MODULE_73_MEMBERS: &[&str] = &["computed", "computed_get", "computed_set", "derived", "effect_last", "effect_run", "get", "set", "signal", "update", "version", "Computed", "Effect", "Signal", "Derived", "set_if_changed", "changed", "computed_update", "computed_version", "effect_run_if", "effect_changed"];
const CORE_MODULE_73_TYPES: &[(&str, CoreLeafKind)] = &[("Computed", CoreLeafKind::Generic(1)), ("Effect", CoreLeafKind::Plain), ("Signal", CoreLeafKind::Generic(1)), ("Derived", CoreLeafKind::Generic(1))];
const CORE_MODULE_73_DEPENDENCIES: &[&str] = &["core.mem"];

const CORE_MODULE_74_MEMBERS: &[&str] = &["failed", "idle", "loaded", "loading", "Loadable", "is_idle", "is_loading", "is_loaded", "is_failed", "value", "reason", "retry"];
const CORE_MODULE_74_TYPES: &[(&str, CoreLeafKind)] = &[("Loadable", CoreLeafKind::Generic(2))];
const CORE_MODULE_74_DEPENDENCIES: &[&str] = &["core.reactive"];

const CORE_MODULE_75_MEMBERS: &[&str] = &["of", "ReflectValue", "inspect"];
const CORE_MODULE_75_TYPES: &[(&str, CoreLeafKind)] = &[("ReflectValue", CoreLeafKind::Plain)];
const CORE_MODULE_75_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_76_MEMBERS: &[&str] = &["compile", "compile_with", "escape", "find", "find_all", "finditer", "flags", "full_match", "is_match", "match", "matches", "purge", "replace", "replace_first", "split", "split_limit", "Match", "Node", "Pattern", "Regex", "RegexFlag", "RegexFlags", "expand", "regex", "join", "search", "findall", "fullmatch", "sub", "subn", "RegexError"];
const CORE_MODULE_76_TYPES: &[(&str, CoreLeafKind)] = &[("Pattern", CoreLeafKind::Plain), ("RegexFlag", CoreLeafKind::Plain), ("Match", CoreLeafKind::Plain), ("Node", CoreLeafKind::Plain), ("Regex", CoreLeafKind::Plain), ("RegexFlags", CoreLeafKind::Plain), ("RegexError", CoreLeafKind::Enum(&["Syntax", "Cache"]))];
const CORE_MODULE_76_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_77_MEMBERS: &[&str] = &["callback", "RealtimeStream"];
const CORE_MODULE_77_TYPES: &[(&str, CoreLeafKind)] = &[("RealtimeStream", CoreLeafKind::Plain)];
const CORE_MODULE_77_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_78_MEMBERS: &[&str] = &["Delivery", "DeliveryEvent", "DeliveryReceipt", "DeliveryState", "ServiceDelivery", "ServiceEndpoint", "ServiceError", "ServiceRestart", "ServiceRuntime", "ServiceStateStore", "ServiceTree", "ServiceUpgradeReceipt", "ServiceWorkflow", "TaskOutcome", "TaskStatus", "delivery_at_most_once", "delivery_durable", "restart_one_for_all", "restart_one_for_one", "restart_rest_for_one", "runtime", "state_store", "tree"];
const CORE_MODULE_78_TYPES: &[(&str, CoreLeafKind)] = &[("Delivery", CoreLeafKind::Plain), ("DeliveryEvent", CoreLeafKind::Plain), ("DeliveryReceipt", CoreLeafKind::Plain), ("DeliveryState", CoreLeafKind::Enum(&["Pending", "Accepted", "Delivering", "Delivered", "DeadLettered", "Cancelled"])), ("ServiceDelivery", CoreLeafKind::Plain), ("ServiceEndpoint", CoreLeafKind::Plain), ("ServiceError", CoreLeafKind::Enum(&["Full", "Ambiguous", "Unknown", "NotStarted", "Policy", "Unavailable", "Partitioned", "Revoked", "Stale", "Expired"])), ("ServiceRestart", CoreLeafKind::Enum(&["OneForOne", "OneForAll", "RestForOne"])), ("ServiceRuntime", CoreLeafKind::Plain), ("ServiceStateStore", CoreLeafKind::Plain), ("ServiceTree", CoreLeafKind::Plain), ("ServiceUpgradeReceipt", CoreLeafKind::Plain), ("ServiceWorkflow", CoreLeafKind::Plain), ("TaskOutcome", CoreLeafKind::Enum(&["Finished", "Panicked", "Cancelled", "DeadlineBlown"])), ("TaskStatus", CoreLeafKind::Enum(&["Running", "Paused", "CancelRequested"]))];
const CORE_MODULE_78_DEPENDENCIES: &[&str] = &["core.net", "core.tasks"];

const CORE_MODULE_79_MEMBERS: &[&str] = &["RowPolicy", "SyncCounter", "SyncList", "SyncMap", "SyncText", "counter_inc", "counter_merge", "counter_new", "counter_value", "list_merge", "list_new", "list_push", "list_show", "map_get", "map_merge", "map_new", "map_set", "map_show", "policy_allows", "policy_new", "policy_show", "text_edit", "text_merge", "text_metadata", "text_new", "text_set", "text_show", "LWW", "RGAItem", "counter_for", "map_for", "map_delete", "map_contains", "map_keys", "list_for", "list_remove", "list_contains", "text_for", "text_append", "text_clock", "text_replica", "policy_grant", "policy_deny", "policy_revoke"];
const CORE_MODULE_79_TYPES: &[(&str, CoreLeafKind)] = &[("RowPolicy", CoreLeafKind::Plain), ("SyncCounter", CoreLeafKind::Plain), ("SyncList", CoreLeafKind::Plain), ("SyncMap", CoreLeafKind::Plain), ("SyncText", CoreLeafKind::Plain), ("LWW", CoreLeafKind::Plain), ("RGAItem", CoreLeafKind::Plain)];
const CORE_MODULE_79_DEPENDENCIES: &[&str] = &["core.data", "core.tasks"];

const CORE_MODULE_80_MEMBERS: &[&str] = &["arch", "atexit", "close_fd", "cpu_count", "current_dir", "decode", "executable", "exitcode", "expand", "family", "fork", "get", "getegid", "geteuid", "getgid", "getgroups", "getpgid", "getpgrp", "getpid", "getppid", "getpriority", "getsid", "getuid", "home_dir", "hostname", "initgroups", "kill", "loadavg", "mkfifo", "name", "pid", "pipe", "release", "set", "set_current_dir", "setgid", "setpgid", "setpgrp", "setpriority", "setsid", "setuid", "stop", "success", "sync", "temp_dir", "times", "umask", "unset", "uptime", "username", "utime", "vars", "version", "wait", "waitpid", "EnvError", "platform", "is_windows", "is_unix", "is_linux", "is_macos", "uname", "sysname", "machine", "getenv", "pathsep", "linesep"];
const CORE_MODULE_80_TYPES: &[(&str, CoreLeafKind)] = &[("EnvError", CoreLeafKind::Plain)];
const CORE_MODULE_80_DEPENDENCIES: &[&str] = &["core.files"];

const CORE_MODULE_81_MEMBERS: &[&str] = &["after", "current_task", "interval", "yield_now", "Receiver", "TaskState", "delay_ms", "is_interval", "is_timer", "recv", "get", "result", "wait", "channel", "put", "clear", "shutdown", "stop", "reset", "lock", "acquire", "release", "notify", "start", "run", "waitall", "waitany", "is_cancelled", "is_ready", "timeout", "sleep", "try_recv", "cancel", "is_closed", "is_locked", "size", "capacity", "generation", "spawn_name"];
const CORE_MODULE_81_TYPES: &[(&str, CoreLeafKind)] = &[("Receiver", CoreLeafKind::Plain), ("TaskState", CoreLeafKind::Plain)];
const CORE_MODULE_81_DEPENDENCIES: &[&str] = &["core.time"];

const CORE_MODULE_82_MEMBERS: &[&str] = &["Reader", "Writer", "binread", "binwrite", "buffered", "choose", "confirm", "eprint", "input", "input_secret", "print", "progress", "read_all_input", "read_key", "read_until", "readline", "stderr", "stdin", "stdout", "style", "style_force", "take", "terminal_height", "terminal_width", "Key", "Stderr", "StdinHandle", "Stdout", "WinSize"];
const CORE_MODULE_82_TYPES: &[(&str, CoreLeafKind)] = &[("Reader", CoreLeafKind::Plain), ("Writer", CoreLeafKind::Plain), ("Key", CoreLeafKind::Enum(&["Char", "Escape", "Backspace", "Tab", "Up", "Down", "Left", "Right", "Unknown"])), ("Stderr", CoreLeafKind::Plain), ("StdinHandle", CoreLeafKind::Plain), ("Stdout", CoreLeafKind::Plain), ("WinSize", CoreLeafKind::Plain)];
const CORE_MODULE_82_DEPENDENCIES: &[&str] = &["core.files"];

const CORE_MODULE_83_MEMBERS: &[&str] = &["assert_equal", "compare", "corpus", "fake_clock", "fake_data", "fake_rng", "fixture", "golden", "histories", "snap", "status", "temp_dir", "test_suite", "world", "Clock", "Fake", "RNG", "TestSuite"];
const CORE_MODULE_83_TYPES: &[(&str, CoreLeafKind)] = &[("Count", CoreLeafKind::Plain), ("DeterministicWorld", CoreLeafKind::Plain), ("EventID", CoreLeafKind::Plain), ("HandleID", CoreLeafKind::Plain), ("HistoryBounds", CoreLeafKind::Plain), ("HistoryCase", CoreLeafKind::Plain), ("HistoryDistribution", CoreLeafKind::Plain), ("HistoryOperation", CoreLeafKind::Plain), ("HistoryPrecondition", CoreLeafKind::Plain), ("HistoryRNG", CoreLeafKind::Plain), ("HistoryScheduleChoice", CoreLeafKind::Plain), ("HistoryStrategy", CoreLeafKind::Generic(1)), ("HistoryValue", CoreLeafKind::Plain), ("TaskID", CoreLeafKind::Plain), ("TypedHistoryCase", CoreLeafKind::Generic(1)), ("TestComparison", CoreLeafKind::Plain), ("Clock", CoreLeafKind::Plain), ("Fake", CoreLeafKind::Plain), ("RNG", CoreLeafKind::Plain), ("TestSuite", CoreLeafKind::Plain)];
const CORE_MODULE_83_DEPENDENCIES: &[&str] = &["core.files", "core.math.random", "core.time"];

const CORE_MODULE_84_MEMBERS: &[&str] = &["Cursor", "byte_count", "byte_views", "casefold", "caseless_eq", "center", "char_indices", "display_width", "ends_any", "grapheme_views", "graphemes", "inspect", "is_alphabetic", "is_ascii", "is_numeric", "is_whitespace", "line_views", "lower", "nfc", "nfd", "nfkc", "nfkd", "pad_end", "pad_start", "rsplitn", "scalar_count", "scalars", "sentences", "splitn", "starts_any", "trim", "trim_end", "trim_start", "upper", "word_views", "words", "IndexedCp", "cursor", "cursor_advance"];
const CORE_MODULE_84_TYPES: &[(&str, CoreLeafKind)] = &[("Cursor", CoreLeafKind::Plain), ("IndexedCp", CoreLeafKind::Plain)];
const CORE_MODULE_84_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_85_MEMBERS: &[&str] = &["bin", "bytes", "decimal", "duration", "grouped", "hex", "number", "oct", "ordinal", "pad", "pad_center", "pad_left", "pad_right", "percent", "plural", "pretty", "sci"];
const CORE_MODULE_85_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_85_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_86_MEMBERS: &[&str] = &["escape", "escape_quote", "unescape", "strip_tags", "unescape_and_strip", "attr_escape", "text_escape"];
const CORE_MODULE_86_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_86_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_87_MEMBERS: &[&str] = &["dedent", "expand_tabs", "fill", "html_escape", "html_unescape", "indent", "shorten", "wrap", "Wrapper", "fill_with", "hanging_indent", "indent_with", "wrap_paragraphs", "wrap_with", "wrapper"];
const CORE_MODULE_87_TYPES: &[(&str, CoreLeafKind)] = &[("Wrapper", CoreLeafKind::Plain)];
const CORE_MODULE_87_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_88_MEMBERS: &[&str] = &["count", "ends_with", "find", "is_alnum", "is_digit", "parse", "parse_float", "parse_int", "parse_int_base", "partition", "replace", "rfind", "rpartition", "rsplit", "split", "split_once", "starts_with", "strip_prefix", "strip_suffix", "capitalize", "capwords", "center", "contains", "escape_c", "find_from", "index", "is_ascii", "is_identifier", "is_lower", "is_space", "is_title", "is_upper", "join", "ljust", "lower", "lstrip", "parse_bool", "parse_kv", "rfind_from", "rjust", "rstrip", "split_ws", "splitlines", "strip", "swapcase", "title", "unescape_c", "upper", "zfill", "startswith", "endswith", "removeprefix", "removesuffix", "rindex", "encode", "isalpha", "isdecimal", "isnumeric", "isalnum", "isascii", "isdigit", "isidentifier", "islower", "isspace", "istitle", "isprintable", "isupper", "expandtabs"];
const CORE_MODULE_88_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_88_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_89_MEMBERS: &[&str] = &["datetime", "days_in_month", "from_iso_week", "from_timestamp", "from_unix_microseconds", "from_unix_ms", "from_unix_nanoseconds", "from_unix_seconds", "instant", "is_leap_year", "local_time", "new", "now", "now_utc", "parse", "parse_iso_week_date", "parse_rfc3339", "parse_time", "parse_zoned", "period", "period_days", "period_months", "period_years", "sleep", "sleep_until", "start", "time", "today", "utc", "zone", "zoned", "add_days", "add_duration", "compare_date", "date_equals", "duration_as_seconds", "duration_ms", "duration_ns", "duration_seconds", "elapsed", "isoformat", "isoformat_date", "isoformat_time", "since", "unix_ms", "unix_ns", "unix_seconds", "weekday", "zoned_local", "Clock", "Date", "DateTime", "Duration", "Instant", "LocalDate", "LocalTime", "Period", "Stopwatch", "Zone", "ZonedDateTime", "add_hours", "add_minutes", "add_seconds", "between", "combine", "date_after", "date_before", "date_of", "days_between", "duration_abs", "duration_add", "duration_as_hours", "duration_as_minutes", "duration_as_ms", "duration_days", "duration_hours", "duration_is_zero", "duration_minutes", "duration_sub", "duration_zero", "end_of_day", "isoweekday", "max_date", "min_date", "replace_date", "replace_time", "start_of_day", "time_of", "weekday_sun0", "asctime", "ctime", "gmtime", "strftime", "utcoffset", "fromhours", "before", "after", "dayofweek", "todate", "totime", "TimeError"];
const CORE_MODULE_89_TYPES: &[(&str, CoreLeafKind)] = &[("Clock", CoreLeafKind::Plain), ("Date", CoreLeafKind::Plain), ("DateTime", CoreLeafKind::Plain), ("Duration", CoreLeafKind::Plain), ("Instant", CoreLeafKind::Plain), ("LocalDate", CoreLeafKind::Plain), ("LocalTime", CoreLeafKind::Plain), ("Period", CoreLeafKind::Plain), ("Stopwatch", CoreLeafKind::Plain), ("Zone", CoreLeafKind::Plain), ("ZonedDateTime", CoreLeafKind::Plain), ("TimeError", CoreLeafKind::Plain)];
const CORE_MODULE_89_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_90_MEMBERS: &[&str] = &["isleap", "leapdays", "weekday", "monthrange", "monthcalendar", "monthcalendar_start", "yearcalendar", "day_name", "day_abbr", "month_name", "month_abbr", "weekheader", "formatmonth", "formatyear", "timegm"];
const CORE_MODULE_90_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_90_DEPENDENCIES: &[&str] = &["core.time"];

const CORE_MODULE_91_MEMBERS: &[&str] = &["expired", "remaining_ms"];
const CORE_MODULE_91_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_91_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_92_MEMBERS: &[&str] = &["aria_role_button", "aria_role_container", "aria_role_label", "aria_role_text_input", "box", "button", "constraint", "desktop", "gtk_backend", "key_event", "mount", "node", "node_accessibility", "node_color", "node_role", "node_shortcut", "null_backend", "phone", "point", "playground", "playgrounds", "preview", "previews", "reactive_render", "rect", "resize_event", "size", "tablet", "text", "text_input", "tui_backend", "UIAriaRole", "UIIMEMode", "UINode", "UIPreview", "UIPreviewDevice", "UIPreviewKind", "UINodeKind", "Point", "Size", "Rect", "SizeConstraint", "UIPreviewViewport", "InputEvent", "NullBackend", "TUIBackend", "GtkBackend"];
const CORE_MODULE_92_TYPES: &[(&str, CoreLeafKind)] = &[("UIIMEMode", CoreLeafKind::Enum(&["Native", "Disabled"])), ("UIPreview", CoreLeafKind::Plain), ("UIPreviewAccessibility", CoreLeafKind::Plain), ("UIPreviewAuthority", CoreLeafKind::Plain), ("UIPreviewContext", CoreLeafKind::Plain), ("UIPreviewDevice", CoreLeafKind::Enum(&["Phone", "Tablet", "Desktop"])), ("UIPreviewEffect", CoreLeafKind::Plain), ("UIPreviewInputOverride", CoreLeafKind::Plain), ("UIPreviewInputValue", CoreLeafKind::Enum(&["Text", "Bool", "Integer", "Float"])), ("UIPreviewKind", CoreLeafKind::Enum(&["Preview", "Playground"])), ("UIPreviewLifecycle", CoreLeafKind::Plain), ("UIPreviewRegistry", CoreLeafKind::Plain), ("UIPreviewSource", CoreLeafKind::Plain), ("UIPreviewTheme", CoreLeafKind::Plain), ("UIPreviewTraits", CoreLeafKind::Plain), ("UIPreviewViewport", CoreLeafKind::Plain), ("UIAriaRole", CoreLeafKind::Enum(&["Button", "Container", "Label", "TextInput"])), ("UINode", CoreLeafKind::Plain), ("UINodeKind", CoreLeafKind::Enum(&["Custom", "Text", "Box", "Button", "TextInput"])), ("Point", CoreLeafKind::Plain), ("Size", CoreLeafKind::Plain), ("Rect", CoreLeafKind::Plain), ("SizeConstraint", CoreLeafKind::Plain), ("InputEvent", CoreLeafKind::Enum(&["Key", "Resize"])), ("NullBackend", CoreLeafKind::Plain), ("TUIBackend", CoreLeafKind::Plain), ("GtkBackend", CoreLeafKind::Plain)];
const CORE_MODULE_92_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_93_MEMBERS: &[&str] = &["TUICapabilities", "TUIColor", "TUIColorProfile", "TUIConstraint", "TUIDirection", "TUIEvent", "TUIListState", "TUIStyle", "ascii", "capabilities", "close_event", "color_ansi16", "color_ansi256", "color_rgb", "display_width", "fill", "focus_event", "horizontal", "interrupt_event", "io_event", "key_event", "key_event_modifiers", "layout", "length", "list", "list_state", "list_state_offset", "list_state_select", "list_state_selected", "max", "min", "percent", "resize_event", "style", "style_background", "style_bold", "style_dim", "style_foreground", "style_text", "style_underline", "table", "timer_event", "vertical"];
const CORE_MODULE_93_TYPES: &[(&str, CoreLeafKind)] = &[("TUIEvent", CoreLeafKind::Enum(&["Key", "Resize", "Timer", "IO", "Focus", "Interrupt", "Close"])), ("TUIColorProfile", CoreLeafKind::Enum(&["ANSI16", "ANSI256", "TrueColor", "ASCII"])), ("TUIColor", CoreLeafKind::Enum(&["ANSI16", "ANSI256", "RGB"])), ("TUICapabilities", CoreLeafKind::Plain), ("TUIStyle", CoreLeafKind::Plain), ("TUIConstraint", CoreLeafKind::Enum(&["Length", "Min", "Max", "Percent", "Fill"])), ("TUIDirection", CoreLeafKind::Enum(&["Horizontal", "Vertical"])), ("TUIListState", CoreLeafKind::Plain)];
const CORE_MODULE_93_DEPENDENCIES: &[&str] = &["core.ui"];

const CORE_MODULE_94_MEMBERS: &[&str] = &["accessibility", "capabilities", "file_filter", "file_filter_text", "fs_grant", "fs_rights_read", "fs_rights_read_write", "fs_rights_write", "open_file", "open_request", "save_file", "save_request", "shortcut", "UICancellation", "UICapability", "UICapabilityFact", "UICapabilityFacts", "UIFileDialogKind", "UIFileDialogRequest", "UIFileDialogSelection", "UIFileFilter", "UIFSAccess", "UIFSGrant", "UIFSRights", "UIGrantedPath", "UIHostError", "UIShortcut", "UIShortcutModifiers", "UITextRange", "UIAccessibility", "UIClipboardText", "UIClipboardWrite", "UINodeID", "UIAccessibilityProjection", "UIShortcutBinding", "UIShortcutDispatch"];
const CORE_MODULE_94_TYPES: &[(&str, CoreLeafKind)] = &[("UICapability", CoreLeafKind::Enum(&["FileDialog", "Clipboard", "IME", "DragDrop", "Shortcuts", "Accessibility", "FontShaping"])), ("UICapabilityFact", CoreLeafKind::Plain), ("UICapabilityFacts", CoreLeafKind::Plain), ("UICancellation", CoreLeafKind::Enum(&["User", "Closed", "Headless", "Superseded", "Programmatic"])), ("UIHostError", CoreLeafKind::Enum(&["Cancelled", "Headless", "Denied", "Missing", "InvalidRequest", "CapabilityUnavailable", "CapabilityDenied", "HostFailure", "ResourceDenied", "ShortcutConflict", "QueueFull"])), ("UIServiceResult", CoreLeafKind::Plain), ("UIFileDialogKind", CoreLeafKind::Enum(&["Open", "Save"])), ("UIFSAccess", CoreLeafKind::Enum(&["Read", "Write"])), ("UIFSRights", CoreLeafKind::Plain), ("UIFSGrant", CoreLeafKind::Plain), ("UIGrantedPath", CoreLeafKind::Plain), ("UIFileFilter", CoreLeafKind::Plain), ("UIFileDialogRequest", CoreLeafKind::Plain), ("UIFileDialogSelection", CoreLeafKind::Plain), ("UIClipboardText", CoreLeafKind::Plain), ("UIClipboardWrite", CoreLeafKind::Plain), ("UITextRange", CoreLeafKind::Plain), ("UIIMEPhase", CoreLeafKind::Enum(&["Start", "Update", "Commit", "Cancel"])), ("UIIMEComposition", CoreLeafKind::Plain), ("UIIMEEvent", CoreLeafKind::Plain), ("UIDragOperation", CoreLeafKind::Enum(&["Copy", "Move", "Link"])), ("UIDropItem", CoreLeafKind::Plain), ("UIDragPhase", CoreLeafKind::Enum(&["Enter", "Over", "Drop", "Leave", "Cancel"])), ("UIDragEvent", CoreLeafKind::Plain), ("UIShortcutModifier", CoreLeafKind::Enum(&["Control", "Alt", "Shift", "Meta"])), ("UIShortcutModifiers", CoreLeafKind::Plain), ("UIShortcut", CoreLeafKind::Plain), ("UIShortcutBinding", CoreLeafKind::Plain), ("UIShortcutDispatch", CoreLeafKind::Enum(&["Dispatched", "Unhandled"])), ("UIAccessibilityState", CoreLeafKind::Plain), ("UIAccessibility", CoreLeafKind::Plain), ("UINodeID", CoreLeafKind::Plain), ("UIAccessibilityProjection", CoreLeafKind::Plain), ("UIFileFilterResult", CoreLeafKind::Plain), ("UIFSGrantResult", CoreLeafKind::Plain), ("UIShortcutResult", CoreLeafKind::Plain), ("UIAccessibilityResult", CoreLeafKind::Plain), ("UIFileDialogResult", CoreLeafKind::Plain), ("UIClipboardTextResult", CoreLeafKind::Plain), ("UIClipboardWriteResult", CoreLeafKind::Plain), ("UIIMEResult", CoreLeafKind::Plain), ("UIDragResult", CoreLeafKind::Plain), ("UIShortcutDispatchResult", CoreLeafKind::Plain), ("UIAccessibilityNodeResult", CoreLeafKind::Plain), ("UIAccessibilityAttachResult", CoreLeafKind::Plain), ("UIAccessibilityProjectionResult", CoreLeafKind::Plain)];
const CORE_MODULE_94_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_95_MEMBERS: &[&str] = &["clear", "is_empty", "read_text", "write_text"];
const CORE_MODULE_95_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_95_DEPENDENCIES: &[&str] = &["core.ui.host"];

const CORE_MODULE_96_MEMBERS: &[&str] = &["poll", "UIIMEEvent", "UIIMEPhase", "UIIMEComposition"];
const CORE_MODULE_96_TYPES: &[(&str, CoreLeafKind)] = &[("UIIMEEvent", CoreLeafKind::Plain), ("UIIMEPhase", CoreLeafKind::Enum(&["Start", "Update", "Commit", "Cancel"])), ("UIIMEComposition", CoreLeafKind::Plain)];
const CORE_MODULE_96_DEPENDENCIES: &[&str] = &["core.ui.host"];

const CORE_MODULE_97_MEMBERS: &[&str] = &["poll", "UIDragPhase", "UIDragOperation", "UIDropItem", "UIDragEvent"];
const CORE_MODULE_97_TYPES: &[(&str, CoreLeafKind)] = &[("UIDragPhase", CoreLeafKind::Enum(&["Enter", "Over", "Drop", "Leave", "Cancel"])), ("UIDragOperation", CoreLeafKind::Enum(&["Copy", "Move", "Link"])), ("UIDropItem", CoreLeafKind::Enum(&["Text", "URI", "File"])), ("UIDragEvent", CoreLeafKind::Plain)];
const CORE_MODULE_97_DEPENDENCIES: &[&str] = &["core.ui.host"];

const CORE_MODULE_98_MEMBERS: &[&str] = &["binding", "dispatch", "register"];
const CORE_MODULE_98_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_98_DEPENDENCIES: &[&str] = &["core.ui.host"];

const CORE_MODULE_99_MEMBERS: &[&str] = &["attach", "project"];
const CORE_MODULE_99_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_99_DEPENDENCIES: &[&str] = &["core.ui.host"];

const CORE_MODULE_100_MEMBERS: &[&str] = &["Measurement", "abs", "add", "centi", "convert", "div", "from", "giga", "kilo", "mega", "micro", "milli", "mul", "nano", "scale", "show", "si", "sub", "to_si", "meters", "kilometers", "grams", "kilograms", "seconds", "milliseconds", "bytes_of", "kibibytes", "equals", "ratio", "is_zero", "meter", "gram", "second", "byte_unit", "kibibyte", "mebibyte", "gibibyte", "metres", "kilometres"];
const CORE_MODULE_100_TYPES: &[(&str, CoreLeafKind)] = &[("Measurement", CoreLeafKind::Generic(1))];
const CORE_MODULE_100_DEPENDENCIES: &[&str] = &["core.math"];

const CORE_MODULE_101_MEMBERS: &[&str] = &["add", "contains", "files", "kind", "len", "port", "process_pid", "remove", "set", "target", "WatchHandle", "WatchSet", "recursive", "debounce", "poll", "events", "cancel", "is_active", "summary", "drain", "WatchDomain", "WatchEventKind", "WatchEvent"];
const CORE_MODULE_101_TYPES: &[(&str, CoreLeafKind)] = &[("WatchHandle", CoreLeafKind::Plain), ("WatchSet", CoreLeafKind::Plain), ("WatchDomain", CoreLeafKind::Enum(&["File", "Process", "Port"])), ("WatchEventKind", CoreLeafKind::Enum(&["Created", "Modified", "Removed", "Error", "Exited", "Ready"])), ("WatchEvent", CoreLeafKind::Plain)];
const CORE_MODULE_101_DEPENDENCIES: &[&str] = &["core.files"];

const CORE_MODULE_102_MEMBERS: &[&str] = &["App", "Auth", "Context", "LiveQuery", "Mount", "Session", "app", "auth", "auth_oauth", "auth_routes", "auth_show", "form", "invalidate", "live", "live_get", "live_show", "live_stats", "on", "openapi", "page", "signal_push", "storage", "subscribe", "sync", "transact_invalidate", "value", "WebPage", "WebEvent"];
const CORE_MODULE_102_TYPES: &[(&str, CoreLeafKind)] = &[("App", CoreLeafKind::Plain), ("Auth", CoreLeafKind::Plain), ("Context", CoreLeafKind::Plain), ("LiveQuery", CoreLeafKind::Plain), ("Mount", CoreLeafKind::Plain), ("Session", CoreLeafKind::Plain), ("WebPage", CoreLeafKind::Plain), ("WebEvent", CoreLeafKind::Plain)];
const CORE_MODULE_102_DEPENDENCIES: &[&str] = &["core.http", "core.web.forms"];

const CORE_MODULE_103_MEMBERS: &[&str] = &["Browser", "BrowserAbilities", "BrowserContext", "BrowserError", "BrowserEvent", "BrowserFrame", "BrowserIntercept", "BrowserLocator", "BrowserLocked", "BrowserPage", "BrowserPrivacy", "BrowserProfile", "BrowserProtocol", "BrowserReceipt", "BrowserTimeout", "BrowserTrace", "begin_named", "config", "config_from_env", "connect", "connect_browser_profile", "connect_profile", "fixture_context", "fixture_page", "fixture_source", "generate_source", "locked", "make_profile", "profile", "report_add_case", "report_exit_code", "report_html", "report_json", "report_new", "report_text", "selected", "server_logs", "server_start", "server_stop", "server_url", "timeout", "watch_changed", "write_report", "BrowserReport", "BrowserServer", "BrowserTestCase", "BrowserTestConfig"];
const CORE_MODULE_103_TYPES: &[(&str, CoreLeafKind)] = &[("BrowserTestConfig", CoreLeafKind::Plain), ("BrowserTestSource", CoreLeafKind::Plain), ("BrowserTestAction", CoreLeafKind::Plain), ("BrowserTestSnapshot", CoreLeafKind::Plain), ("BrowserTestEventFact", CoreLeafKind::Plain), ("BrowserTestArtifact", CoreLeafKind::Plain), ("BrowserTestAttempt", CoreLeafKind::Plain), ("BrowserTestCase", CoreLeafKind::Plain), ("BrowserTestReport", CoreLeafKind::Plain), ("BrowserTestFixture", CoreLeafKind::Plain), ("BrowserTestServer", CoreLeafKind::Plain), ("Browser", CoreLeafKind::Plain), ("BrowserAbilities", CoreLeafKind::Plain), ("BrowserContext", CoreLeafKind::Plain), ("BrowserError", CoreLeafKind::Enum(&["Connect", "Headless", "Timeout", "Protocol", "Closed"])), ("BrowserEvent", CoreLeafKind::Plain), ("BrowserFrame", CoreLeafKind::Plain), ("BrowserIntercept", CoreLeafKind::Plain), ("BrowserLocator", CoreLeafKind::Plain), ("BrowserLocked", CoreLeafKind::Plain), ("BrowserPage", CoreLeafKind::Plain), ("BrowserPrivacy", CoreLeafKind::Enum(&["Default", "Isolated"])), ("BrowserProfile", CoreLeafKind::Plain), ("BrowserProtocol", CoreLeafKind::Enum(&["CDP", "WebDriver"])), ("BrowserReceipt", CoreLeafKind::Plain), ("BrowserTimeout", CoreLeafKind::Plain), ("BrowserTrace", CoreLeafKind::Plain), ("BrowserReport", CoreLeafKind::Plain), ("BrowserServer", CoreLeafKind::Plain)];
const CORE_MODULE_103_DEPENDENCIES: &[&str] = &["core.web", "core.files"];

const CORE_MODULE_104_MEMBERS: &[&str] = &["app", "for_app", "DevServer", "is_local", "url"];
const CORE_MODULE_104_TYPES: &[(&str, CoreLeafKind)] = &[("DevServer", CoreLeafKind::Plain)];
const CORE_MODULE_104_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_105_MEMBERS: &[&str] = &["action_error", "action_field_error", "action_form_error", "blur", "field", "html", "input", "input_exclude", "input_group", "input_rename", "input_replace", "new", "no_script", "set", "show", "submit", "typed", "typed_blur", "typed_cancel", "typed_decode_post", "typed_errors", "typed_focus", "typed_html", "typed_lifecycle", "typed_no_script", "typed_post", "typed_select_field", "typed_set", "typed_set_async_validator", "typed_set_action", "typed_show", "typed_state", "typed_submit", "typed_submit_async", "typed_validate", "typed_validate_async", "typed_validate_field", "typed_validation_render", "typed_submission_cancel", "typed_submission_wait", "typed_validation_cancel", "typed_validation_wait", "validate", "validate_async", "WebForm", "WebFormActionError", "WebFormControl", "WebFormDecodedInput", "WebFormError", "WebFormErrorState", "WebFormFieldSpec", "WebFormFieldState", "WebFormInput", "WebFormLifecycle", "WebFormLifecycleStatus", "WebFormStatus", "WebFormState", "WebFormTyped", "WebFormTypedSubmission", "WebFormValidation", "WebFormValidationChain", "WebFormValidationTiming", "WebFormValueType"];
const CORE_MODULE_105_TYPES: &[(&str, CoreLeafKind)] = &[("WebFormValueType", CoreLeafKind::Enum(&["String", "Int", "Bool", "Float"])), ("WebFormControl", CoreLeafKind::Enum(&["Text", "Email", "URL", "Password", "Number", "Date", "Checkbox", "Hidden"])), ("WebFormStatus", CoreLeafKind::Enum(&["Idle", "Dirty", "Validating", "Invalid", "Submitting", "Submitted", "Error"])), ("WebFormValidationTiming", CoreLeafKind::Enum(&["Immediate", "Debounced", "OnBlur"])), ("WebFormFieldSpec", CoreLeafKind::Plain), ("WebFormInput", CoreLeafKind::Plain), ("WebFormDecodedInput", CoreLeafKind::Plain), ("WebFormActionError", CoreLeafKind::Plain), ("WebFormErrorState", CoreLeafKind::Plain), ("WebFormLifecycle", CoreLeafKind::Enum(&["Idle", "Pending", "Submitting", "Success", "Failure", "Cancelled"])), ("WebFormLifecycleStatus", CoreLeafKind::Enum(&["Idle", "Pending", "Submitting", "Success", "Failure", "Cancelled"])), ("WebFormTyped", CoreLeafKind::Plain), ("WebFormValidationChain", CoreLeafKind::Plain), ("WebFormValidation", CoreLeafKind::Plain), ("WebFormTypedSubmission", CoreLeafKind::Plain), ("WebForm", CoreLeafKind::Plain), ("WebFormState", CoreLeafKind::Plain), ("WebFormError", CoreLeafKind::Enum(&["Invalid", "Missing", "Validation"])), ("WebFormFieldState", CoreLeafKind::Plain)];
const CORE_MODULE_105_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_106_MEMBERS: &[&str] = &["cancel", "facts", "get", "invalidate", "live", "mutate", "mutate_with_invalidations", "mutation_state", "mutation_signal", "new", "queue", "refresh", "retry", "set_mode", "set_online", "show", "state", "state_signal", "subscribe", "WebMutationState", "WebMutationStatus", "WebQuery", "WebQueryNetworkMode", "WebQueryStatus", "WebQueryError", "WebQuerySeed", "WebQueryState"];
const CORE_MODULE_106_TYPES: &[(&str, CoreLeafKind)] = &[("WebMutationState", CoreLeafKind::Plain), ("WebMutationStatus", CoreLeafKind::Enum(&["Idle", "Pending", "Success", "Error", "Settled"])), ("WebQueryStatus", CoreLeafKind::Enum(&["Pending", "Fresh", "Stale", "Fetching", "Error", "Offline"])), ("WebQueryNetworkMode", CoreLeafKind::Enum(&["Online", "Always", "OfflineFirst"])), ("WebQuery", CoreLeafKind::Plain), ("WebQueryError", CoreLeafKind::Enum(&["Offline", "Conflict", "Invalid"])), ("WebQueryState", CoreLeafKind::Plain), ("WebQuerySeed", CoreLeafKind::Plain)];
const CORE_MODULE_106_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_107_MEMBERS: &[&str] = &["abort", "cache_show", "cache_state", "collect", "current", "invalidate", "link", "navigate", "new", "not_found", "preload", "route", "route_with_search_codec", "stale", "WebMatch", "WebNavigationStatus", "WebRoute", "WebRouter", "WebRouterCacheStatus", "WebRouterSearchCodec", "WebRouterValueType", "WebRouterError", "WebRouterField", "WebNavigation", "WebNavigationState"];
const CORE_MODULE_107_TYPES: &[(&str, CoreLeafKind)] = &[("WebRouterValueType", CoreLeafKind::Enum(&["String", "Int", "Bool", "Float", "JSON"])), ("WebRouterSearchCodec", CoreLeafKind::Enum(&["Query", "JSON"])), ("WebRouterCacheStatus", CoreLeafKind::Enum(&["Fresh", "Stale", "Invalidated", "Collected"])), ("WebNavigationStatus", CoreLeafKind::Enum(&["Idle", "Preloading", "Pending", "Ready", "Error", "Aborted"])), ("WebMatch", CoreLeafKind::Plain), ("WebRoute", CoreLeafKind::Plain), ("WebRouter", CoreLeafKind::Plain), ("WebRouterError", CoreLeafKind::Enum(&["NotFound", "Callback", "Invalid"])), ("WebRouterField", CoreLeafKind::Plain), ("WebNavigation", CoreLeafKind::Plain), ("WebNavigationState", CoreLeafKind::Plain)];
const CORE_MODULE_107_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_108_MEMBERS: &[&str] = &["local", "session", "WebStorage", "kind_local", "kind_session"];
const CORE_MODULE_108_TYPES: &[(&str, CoreLeafKind)] = &[("WebStorage", CoreLeafKind::Plain)];
const CORE_MODULE_108_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_109_MEMBERS: &[&str] = &["clear", "get", "get_or", "has", "remove", "set"];
const CORE_MODULE_109_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_109_DEPENDENCIES: &[&str] = &["core.web.storage"];

const CORE_MODULE_110_MEMBERS: &[&str] = &["clear", "get", "get_or", "has", "remove", "set"];
const CORE_MODULE_110_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_110_DEPENDENCIES: &[&str] = &["core.web.storage"];

const CORE_MODULE_111_MEMBERS: &[&str] = &["back", "batch", "clear_history", "current_generation", "cursor", "derived", "event_json", "events", "events_since", "facts_json", "forward", "history", "history_at", "history_enabled", "history_limit", "inspect", "jump", "new", "optimistic", "patch", "patch_active", "patch_commit", "patch_generation", "patch_rollback", "patch_transaction", "restore", "scrub", "selector", "set", "set_history_limit", "set_state", "signal", "state_signal", "subscribe", "subscribe_selector", "subscription_active", "subscription_unsubscribe", "transaction", "update", "value", "with_history", "WebStore", "WebStoreEvent", "WebStoreInspection", "WebStorePatch", "WebStoreSubscription", "WebStoreTransaction", "get", "WebStoreError", "Derived"];
const CORE_MODULE_111_TYPES: &[(&str, CoreLeafKind)] = &[("WebStoreTransaction", CoreLeafKind::Generic(1)), ("WebStoreEvent", CoreLeafKind::Plain), ("WebStoreInspection", CoreLeafKind::Plain), ("WebStorePatch", CoreLeafKind::Generic(1)), ("WebStore", CoreLeafKind::Generic(1)), ("WebStoreSubscription", CoreLeafKind::Plain), ("WebStoreError", CoreLeafKind::Enum(&["NotFound", "Invalid"])), ("Derived", CoreLeafKind::Generic(1))];
const CORE_MODULE_111_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_112_MEMBERS: &[&str] = &["clear_focus", "clear_selection", "column", "facts", "filter", "filter_by", "first_page", "focus", "focused_key", "insert_row", "keys", "last_page", "new", "new_keyed", "next_page", "page", "page_state", "paginate", "remove_row", "replace_row", "selected_keys", "selected_rows", "set_rows", "set_selected", "sort", "sort_by", "state", "toggle_selection", "update_row", "visible_rows", "with_column", "with_server_page", "WebTable", "WebTableColumn", "WebTableFilter", "WebTablePageMode", "WebTableRow", "WebTableSort", "WebTableSortDirection", "WebTableState", "WebTableError", "WebTablePage"];
const CORE_MODULE_112_TYPES: &[(&str, CoreLeafKind)] = &[("WebTableSortDirection", CoreLeafKind::Enum(&["Ascending", "Descending"])), ("WebTablePageMode", CoreLeafKind::Enum(&["Client", "Server"])), ("WebTableSort", CoreLeafKind::Plain), ("WebTableFilter", CoreLeafKind::Plain), ("WebTableState", CoreLeafKind::Plain), ("WebTableColumn", CoreLeafKind::Generic(1)), ("WebTablePage", CoreLeafKind::Generic(1)), ("WebTableRow", CoreLeafKind::Generic(1)), ("WebTable", CoreLeafKind::Generic(1)), ("WebTableError", CoreLeafKind::Enum(&["NotFound", "Invalid"]))];
const CORE_MODULE_112_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_113_MEMBERS: &[&str] = &["indices", "plan", "plan_from_sizes", "plan_facts", "plan_indices", "plan_measure", "plan_measured", "plan_resize", "plan_scroll_to", "plan_slice", "plan_viewport", "plan_viewport_measure", "plan_viewport_state", "slice", "window", "window_measured", "WebVirtualPlan", "WebVirtualPlanViewport", "WebVirtualWindow"];
const CORE_MODULE_113_TYPES: &[(&str, CoreLeafKind)] = &[("WebVirtualWindow", CoreLeafKind::Plain), ("WebVirtualPlan", CoreLeafKind::Plain), ("WebVirtualPlanViewport", CoreLeafKind::Plain)];
const CORE_MODULE_113_DEPENDENCIES: &[&str] = &["core.web"];

pub const CORE_MODULE_DECLARATIONS: &[CoreModuleDeclaration] = &[
    CoreModuleDeclaration { module: "app", members: CORE_MODULE_0_MEMBERS, type_exports: CORE_MODULE_0_TYPES, dependencies: CORE_MODULE_0_DEPENDENCIES },
    CoreModuleDeclaration { module: "core", members: CORE_MODULE_1_MEMBERS, type_exports: CORE_MODULE_1_TYPES, dependencies: CORE_MODULE_1_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.devtools", members: CORE_MODULE_2_MEMBERS, type_exports: CORE_MODULE_2_TYPES, dependencies: CORE_MODULE_2_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.archive", members: CORE_MODULE_3_MEMBERS, type_exports: CORE_MODULE_3_TYPES, dependencies: CORE_MODULE_3_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.archive.gzip", members: CORE_MODULE_4_MEMBERS, type_exports: CORE_MODULE_4_TYPES, dependencies: CORE_MODULE_4_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.archive.zstd", members: CORE_MODULE_5_MEMBERS, type_exports: CORE_MODULE_5_TYPES, dependencies: CORE_MODULE_5_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.args", members: CORE_MODULE_6_MEMBERS, type_exports: CORE_MODULE_6_TYPES, dependencies: CORE_MODULE_6_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.auth", members: CORE_MODULE_7_MEMBERS, type_exports: CORE_MODULE_7_TYPES, dependencies: CORE_MODULE_7_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.build", members: CORE_MODULE_8_MEMBERS, type_exports: CORE_MODULE_8_TYPES, dependencies: CORE_MODULE_8_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.compiler", members: CORE_MODULE_9_MEMBERS, type_exports: CORE_MODULE_9_TYPES, dependencies: CORE_MODULE_9_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.compiler.lang", members: CORE_MODULE_10_MEMBERS, type_exports: CORE_MODULE_10_TYPES, dependencies: CORE_MODULE_10_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.collections", members: CORE_MODULE_11_MEMBERS, type_exports: CORE_MODULE_11_TYPES, dependencies: CORE_MODULE_11_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.collections.set", members: CORE_MODULE_12_MEMBERS, type_exports: CORE_MODULE_12_TYPES, dependencies: CORE_MODULE_12_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.compute", members: CORE_MODULE_13_MEMBERS, type_exports: CORE_MODULE_13_TYPES, dependencies: CORE_MODULE_13_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.compute.solve", members: CORE_MODULE_14_MEMBERS, type_exports: CORE_MODULE_14_TYPES, dependencies: CORE_MODULE_14_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.crypto", members: CORE_MODULE_15_MEMBERS, type_exports: CORE_MODULE_15_TYPES, dependencies: CORE_MODULE_15_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.crypto.expert", members: CORE_MODULE_16_MEMBERS, type_exports: CORE_MODULE_16_TYPES, dependencies: CORE_MODULE_16_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.crypto.random", members: CORE_MODULE_17_MEMBERS, type_exports: CORE_MODULE_17_TYPES, dependencies: CORE_MODULE_17_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.crypto.uuid", members: CORE_MODULE_18_MEMBERS, type_exports: CORE_MODULE_18_TYPES, dependencies: CORE_MODULE_18_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.crypto.vault", members: CORE_MODULE_19_MEMBERS, type_exports: CORE_MODULE_19_TYPES, dependencies: CORE_MODULE_19_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data", members: CORE_MODULE_20_MEMBERS, type_exports: CORE_MODULE_20_TYPES, dependencies: CORE_MODULE_20_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.arrow", members: CORE_MODULE_21_MEMBERS, type_exports: CORE_MODULE_21_TYPES, dependencies: CORE_MODULE_21_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.loader", members: CORE_MODULE_22_MEMBERS, type_exports: CORE_MODULE_22_TYPES, dependencies: CORE_MODULE_22_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.stream", members: CORE_MODULE_23_MEMBERS, type_exports: CORE_MODULE_23_TYPES, dependencies: CORE_MODULE_23_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.plot", members: CORE_MODULE_24_MEMBERS, type_exports: CORE_MODULE_24_TYPES, dependencies: CORE_MODULE_24_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.sketch", members: CORE_MODULE_25_MEMBERS, type_exports: CORE_MODULE_25_TYPES, dependencies: CORE_MODULE_25_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.sketch.cms", members: CORE_MODULE_26_MEMBERS, type_exports: CORE_MODULE_26_TYPES, dependencies: CORE_MODULE_26_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.sketch.hll", members: CORE_MODULE_27_MEMBERS, type_exports: CORE_MODULE_27_TYPES, dependencies: CORE_MODULE_27_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.sketch.reservoir", members: CORE_MODULE_28_MEMBERS, type_exports: CORE_MODULE_28_TYPES, dependencies: CORE_MODULE_28_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.sketch.tdigest", members: CORE_MODULE_29_MEMBERS, type_exports: CORE_MODULE_29_TYPES, dependencies: CORE_MODULE_29_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.db", members: CORE_MODULE_30_MEMBERS, type_exports: CORE_MODULE_30_TYPES, dependencies: CORE_MODULE_30_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.email", members: CORE_MODULE_31_MEMBERS, type_exports: CORE_MODULE_31_TYPES, dependencies: CORE_MODULE_31_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding", members: CORE_MODULE_32_MEMBERS, type_exports: CORE_MODULE_32_TYPES, dependencies: CORE_MODULE_32_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.base32", members: CORE_MODULE_33_MEMBERS, type_exports: CORE_MODULE_33_TYPES, dependencies: CORE_MODULE_33_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.base64", members: CORE_MODULE_34_MEMBERS, type_exports: CORE_MODULE_34_TYPES, dependencies: CORE_MODULE_34_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.binary", members: CORE_MODULE_35_MEMBERS, type_exports: CORE_MODULE_35_TYPES, dependencies: CORE_MODULE_35_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.cbor", members: CORE_MODULE_36_MEMBERS, type_exports: CORE_MODULE_36_TYPES, dependencies: CORE_MODULE_36_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.csv", members: CORE_MODULE_37_MEMBERS, type_exports: CORE_MODULE_37_TYPES, dependencies: CORE_MODULE_37_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.hex", members: CORE_MODULE_38_MEMBERS, type_exports: CORE_MODULE_38_TYPES, dependencies: CORE_MODULE_38_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.ini", members: CORE_MODULE_39_MEMBERS, type_exports: CORE_MODULE_39_TYPES, dependencies: CORE_MODULE_39_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.json", members: CORE_MODULE_40_MEMBERS, type_exports: CORE_MODULE_40_TYPES, dependencies: CORE_MODULE_40_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.jsonl", members: CORE_MODULE_41_MEMBERS, type_exports: CORE_MODULE_41_TYPES, dependencies: CORE_MODULE_41_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.toml", members: CORE_MODULE_42_MEMBERS, type_exports: CORE_MODULE_42_TYPES, dependencies: CORE_MODULE_42_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.xml", members: CORE_MODULE_43_MEMBERS, type_exports: CORE_MODULE_43_TYPES, dependencies: CORE_MODULE_43_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.event", members: CORE_MODULE_44_MEMBERS, type_exports: CORE_MODULE_44_TYPES, dependencies: CORE_MODULE_44_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.files", members: CORE_MODULE_45_MEMBERS, type_exports: CORE_MODULE_45_TYPES, dependencies: CORE_MODULE_45_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.yaml", members: CORE_MODULE_46_MEMBERS, type_exports: CORE_MODULE_46_TYPES, dependencies: CORE_MODULE_46_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.files.path", members: CORE_MODULE_47_MEMBERS, type_exports: CORE_MODULE_47_TYPES, dependencies: CORE_MODULE_47_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.font", members: CORE_MODULE_48_MEMBERS, type_exports: CORE_MODULE_48_TYPES, dependencies: CORE_MODULE_48_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.game", members: CORE_MODULE_49_MEMBERS, type_exports: CORE_MODULE_49_TYPES, dependencies: CORE_MODULE_49_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.game.raylib", members: CORE_MODULE_50_MEMBERS, type_exports: CORE_MODULE_50_TYPES, dependencies: CORE_MODULE_50_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.http", members: CORE_MODULE_51_MEMBERS, type_exports: CORE_MODULE_51_TYPES, dependencies: CORE_MODULE_51_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.http.client", members: CORE_MODULE_52_MEMBERS, type_exports: CORE_MODULE_52_TYPES, dependencies: CORE_MODULE_52_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.http.server", members: CORE_MODULE_53_MEMBERS, type_exports: CORE_MODULE_53_TYPES, dependencies: CORE_MODULE_53_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.jobs", members: CORE_MODULE_54_MEMBERS, type_exports: CORE_MODULE_54_TYPES, dependencies: CORE_MODULE_54_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.log", members: CORE_MODULE_55_MEMBERS, type_exports: CORE_MODULE_55_TYPES, dependencies: CORE_MODULE_55_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.math", members: CORE_MODULE_56_MEMBERS, type_exports: CORE_MODULE_56_TYPES, dependencies: CORE_MODULE_56_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.math.random", members: CORE_MODULE_57_MEMBERS, type_exports: CORE_MODULE_57_TYPES, dependencies: CORE_MODULE_57_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.math.combinatorics", members: CORE_MODULE_58_MEMBERS, type_exports: CORE_MODULE_58_TYPES, dependencies: CORE_MODULE_58_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.math.stats", members: CORE_MODULE_59_MEMBERS, type_exports: CORE_MODULE_59_TYPES, dependencies: CORE_MODULE_59_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.mem", members: CORE_MODULE_60_MEMBERS, type_exports: CORE_MODULE_60_TYPES, dependencies: CORE_MODULE_60_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.mem.scope", members: CORE_MODULE_61_MEMBERS, type_exports: CORE_MODULE_61_TYPES, dependencies: CORE_MODULE_61_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.mod", members: CORE_MODULE_62_MEMBERS, type_exports: CORE_MODULE_62_TYPES, dependencies: CORE_MODULE_62_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.net", members: CORE_MODULE_63_MEMBERS, type_exports: CORE_MODULE_63_TYPES, dependencies: CORE_MODULE_63_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.net.mime", members: CORE_MODULE_64_MEMBERS, type_exports: CORE_MODULE_64_TYPES, dependencies: CORE_MODULE_64_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.net.tls", members: CORE_MODULE_65_MEMBERS, type_exports: CORE_MODULE_65_TYPES, dependencies: CORE_MODULE_65_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.net.url", members: CORE_MODULE_66_MEMBERS, type_exports: CORE_MODULE_66_TYPES, dependencies: CORE_MODULE_66_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.net.ws", members: CORE_MODULE_67_MEMBERS, type_exports: CORE_MODULE_67_TYPES, dependencies: CORE_MODULE_67_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.net.ip", members: CORE_MODULE_68_MEMBERS, type_exports: CORE_MODULE_68_TYPES, dependencies: CORE_MODULE_68_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.perf", members: CORE_MODULE_69_MEMBERS, type_exports: CORE_MODULE_69_TYPES, dependencies: CORE_MODULE_69_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.plugin", members: CORE_MODULE_70_MEMBERS, type_exports: CORE_MODULE_70_TYPES, dependencies: CORE_MODULE_70_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.prelude", members: CORE_MODULE_71_MEMBERS, type_exports: CORE_MODULE_71_TYPES, dependencies: CORE_MODULE_71_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.process", members: CORE_MODULE_72_MEMBERS, type_exports: CORE_MODULE_72_TYPES, dependencies: CORE_MODULE_72_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.reactive", members: CORE_MODULE_73_MEMBERS, type_exports: CORE_MODULE_73_TYPES, dependencies: CORE_MODULE_73_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.reactive.loadable", members: CORE_MODULE_74_MEMBERS, type_exports: CORE_MODULE_74_TYPES, dependencies: CORE_MODULE_74_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.reflect", members: CORE_MODULE_75_MEMBERS, type_exports: CORE_MODULE_75_TYPES, dependencies: CORE_MODULE_75_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.regex", members: CORE_MODULE_76_MEMBERS, type_exports: CORE_MODULE_76_TYPES, dependencies: CORE_MODULE_76_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.rt", members: CORE_MODULE_77_MEMBERS, type_exports: CORE_MODULE_77_TYPES, dependencies: CORE_MODULE_77_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.service", members: CORE_MODULE_78_MEMBERS, type_exports: CORE_MODULE_78_TYPES, dependencies: CORE_MODULE_78_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.sync", members: CORE_MODULE_79_MEMBERS, type_exports: CORE_MODULE_79_TYPES, dependencies: CORE_MODULE_79_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.sys", members: CORE_MODULE_80_MEMBERS, type_exports: CORE_MODULE_80_TYPES, dependencies: CORE_MODULE_80_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.tasks", members: CORE_MODULE_81_MEMBERS, type_exports: CORE_MODULE_81_TYPES, dependencies: CORE_MODULE_81_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.term", members: CORE_MODULE_82_MEMBERS, type_exports: CORE_MODULE_82_TYPES, dependencies: CORE_MODULE_82_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.testing", members: CORE_MODULE_83_MEMBERS, type_exports: CORE_MODULE_83_TYPES, dependencies: CORE_MODULE_83_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.text", members: CORE_MODULE_84_MEMBERS, type_exports: CORE_MODULE_84_TYPES, dependencies: CORE_MODULE_84_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.text.fmt", members: CORE_MODULE_85_MEMBERS, type_exports: CORE_MODULE_85_TYPES, dependencies: CORE_MODULE_85_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.text.html", members: CORE_MODULE_86_MEMBERS, type_exports: CORE_MODULE_86_TYPES, dependencies: CORE_MODULE_86_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.text.wrap", members: CORE_MODULE_87_MEMBERS, type_exports: CORE_MODULE_87_TYPES, dependencies: CORE_MODULE_87_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.text.parse", members: CORE_MODULE_88_MEMBERS, type_exports: CORE_MODULE_88_TYPES, dependencies: CORE_MODULE_88_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.time", members: CORE_MODULE_89_MEMBERS, type_exports: CORE_MODULE_89_TYPES, dependencies: CORE_MODULE_89_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.time.calendar", members: CORE_MODULE_90_MEMBERS, type_exports: CORE_MODULE_90_TYPES, dependencies: CORE_MODULE_90_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.time.expiring", members: CORE_MODULE_91_MEMBERS, type_exports: CORE_MODULE_91_TYPES, dependencies: CORE_MODULE_91_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui", members: CORE_MODULE_92_MEMBERS, type_exports: CORE_MODULE_92_TYPES, dependencies: CORE_MODULE_92_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.tui", members: CORE_MODULE_93_MEMBERS, type_exports: CORE_MODULE_93_TYPES, dependencies: CORE_MODULE_93_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host", members: CORE_MODULE_94_MEMBERS, type_exports: CORE_MODULE_94_TYPES, dependencies: CORE_MODULE_94_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host.clipboard", members: CORE_MODULE_95_MEMBERS, type_exports: CORE_MODULE_95_TYPES, dependencies: CORE_MODULE_95_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host.ime", members: CORE_MODULE_96_MEMBERS, type_exports: CORE_MODULE_96_TYPES, dependencies: CORE_MODULE_96_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host.drag_drop", members: CORE_MODULE_97_MEMBERS, type_exports: CORE_MODULE_97_TYPES, dependencies: CORE_MODULE_97_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host.shortcuts", members: CORE_MODULE_98_MEMBERS, type_exports: CORE_MODULE_98_TYPES, dependencies: CORE_MODULE_98_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host.accessibility", members: CORE_MODULE_99_MEMBERS, type_exports: CORE_MODULE_99_TYPES, dependencies: CORE_MODULE_99_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.units", members: CORE_MODULE_100_MEMBERS, type_exports: CORE_MODULE_100_TYPES, dependencies: CORE_MODULE_100_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.watcher", members: CORE_MODULE_101_MEMBERS, type_exports: CORE_MODULE_101_TYPES, dependencies: CORE_MODULE_101_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web", members: CORE_MODULE_102_MEMBERS, type_exports: CORE_MODULE_102_TYPES, dependencies: CORE_MODULE_102_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.browser", members: CORE_MODULE_103_MEMBERS, type_exports: CORE_MODULE_103_TYPES, dependencies: CORE_MODULE_103_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.devserver", members: CORE_MODULE_104_MEMBERS, type_exports: CORE_MODULE_104_TYPES, dependencies: CORE_MODULE_104_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.forms", members: CORE_MODULE_105_MEMBERS, type_exports: CORE_MODULE_105_TYPES, dependencies: CORE_MODULE_105_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.query", members: CORE_MODULE_106_MEMBERS, type_exports: CORE_MODULE_106_TYPES, dependencies: CORE_MODULE_106_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.router", members: CORE_MODULE_107_MEMBERS, type_exports: CORE_MODULE_107_TYPES, dependencies: CORE_MODULE_107_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.storage", members: CORE_MODULE_108_MEMBERS, type_exports: CORE_MODULE_108_TYPES, dependencies: CORE_MODULE_108_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.storage.local", members: CORE_MODULE_109_MEMBERS, type_exports: CORE_MODULE_109_TYPES, dependencies: CORE_MODULE_109_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.storage.session", members: CORE_MODULE_110_MEMBERS, type_exports: CORE_MODULE_110_TYPES, dependencies: CORE_MODULE_110_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.store", members: CORE_MODULE_111_MEMBERS, type_exports: CORE_MODULE_111_TYPES, dependencies: CORE_MODULE_111_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.table", members: CORE_MODULE_112_MEMBERS, type_exports: CORE_MODULE_112_TYPES, dependencies: CORE_MODULE_112_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.virtual", members: CORE_MODULE_113_MEMBERS, type_exports: CORE_MODULE_113_TYPES, dependencies: CORE_MODULE_113_DEPENDENCIES },
];

pub const CORE_SOURCE_MODULES: &[CoreSourceModule] = &[
    CoreSourceModule { module: "app", alias: "app", path: "Core/app/app.jet", owned_members: &["auth", "auth_oauth", "auth_routes", "auth_show", "live", "live_get", "live_show", "live_stats", "subscribe", "invalidate", "transact_invalidate", "signal_push", "sync"] },
    CoreSourceModule { module: "core.archive", alias: "core_archive", path: "Core/archive/archive.jet", owned_members: &["crc32", "adler32", "deflate", "inflate", "compress", "decompress", "zip_decompress", "zip_names_json", "list", "zip_open", "zip_next", "zip_read", "zip_write", "zip_close", "zip_extract", "unzip", "tar_add", "tar_get", "tar_names_json", "create"] },
    CoreSourceModule { module: "core.archive.gzip", alias: "core_core_archive_gzip", path: "Core/archive/gzip.jet", owned_members: &["compress", "compress_file", "compress_text", "crc", "decompress", "decompress_file", "decompress_text", "is_gzip", "isize", "magic", "peek_isize"] },
    CoreSourceModule { module: "core.archive.zstd", alias: "core_core_archive_zstd", path: "Core/archive/zstd.jet", owned_members: &["compress", "compress_file", "compress_text", "decompress", "decompress_file", "decompress_text", "is_zstd", "magic"] },
    CoreSourceModule { module: "core.args", alias: "core_core_args", path: "Core/args/args.jet", owned_members: &["decode", "decode_argv", "get_bool", "get_int", "get_text", "merge", "spec", "positionals", "program", "help_text", "flag", "has", "argument", "define", "dest", "required", "default_value", "get_or", "get_choice", "missing_required", "apply_defaults", "usage", "help_from", "remainder", "count_flag", "wants_help"] },
    CoreSourceModule { module: "core.auth", alias: "core_core_auth", path: "Core/auth/auth.jet", owned_members: &["magic_link_consume", "magic_link_issue", "oauth_begin", "oauth_finish", "password_login", "register_user", "session_cookie", "session_id", "session_show", "session_user", "session_validate", "verify_jwt", "verify_paseto"] },
    CoreSourceModule { module: "core.build", alias: "core_core_build", path: "Core/build/build.jet", owned_members: &["graph", "receipt_diff"] },
    CoreSourceModule { module: "core.compiler", alias: "core_core_compiler", path: "Core/compiler/compiler.jet", owned_members: &["check", "lex", "lock", "manifest", "package", "parse", "profiles", "source_map"] },
    CoreSourceModule { module: "core.compiler.lang", alias: "core_core_compiler_lang", path: "Core/compiler/lang.jet", owned_members: &[] },
    CoreSourceModule { module: "core.collections", alias: "core_core_collections", path: "Core/collections/collections.jet", owned_members: &["add", "clear_counter", "counter", "counter_from", "dec", "elements", "get", "inc", "merge_add", "most_common", "names", "set_count", "subtract", "total", "heapify", "heappush", "heappop", "heappushpop", "heapreplace", "nsmallest", "nlargest", "merge_sorted", "bisect_left", "bisect_right", "insort_left", "insort_right", "ordered_map", "map_get", "map_set", "map_remove", "map_keys", "map_values", "map_contains", "map_len", "chain", "chain_push", "chain_get", "chain_contains"] },
    CoreSourceModule { module: "core.collections.set", alias: "core_collections_set", path: "Core/collections/set.jet", owned_members: &["new", "from_list", "add", "discard", "remove", "contains", "len", "is_empty", "to_list", "clear", "union", "intersection", "difference", "symmetric_difference", "issubset", "issuperset", "isdisjoint", "clone_set"] },
    CoreSourceModule { module: "core.compute", alias: "core_core_compute", path: "Core/compute/compute.jet", owned_members: &["abs", "add", "broadcast_to", "det", "device", "device_auto", "device_cpu", "device_cuda", "device_metal", "device_vulkan", "device_webgpu", "div", "exp", "eye", "from_list", "full", "get", "gradient", "jvp", "kernel_bounds_ok", "log", "matmul", "matmul_f32_tile", "matrix", "maximum", "minimum", "mse_loss", "mul", "negate", "numel", "on_device", "ones", "placement", "profile_f32_strict", "profile_show", "rank", "reshape", "set", "sgd_step", "shape", "sqrt", "sub", "sum_axis", "to_list", "transfer", "transfer_show", "transpose", "value_and_gradient", "vec", "vjp", "zeros", "inv", "solve", "fft", "to_sparse", "sparse_nnz", "sparse_show", "sparse_mv", "serialize", "deserialize", "stream_new", "stream_new_on", "stream_show", "stream_sync"] },
    CoreSourceModule { module: "core.compute.solve", alias: "core_core_compute_solve", path: "Core/compute/solve.jet", owned_members: &["dense", "lu"] },
    CoreSourceModule { module: "core.crypto", alias: "core_core_crypto", path: "Core/crypto/crypto.jet", owned_members: &["blake3", "constant_time_equal", "constant_time_equal_bytes", "digest", "hkdf_sha256", "hmac_sha256", "new", "pbkdf2_hmac", "seal", "sealed_bytes", "sha1", "sha224", "sha256", "sha384", "sha3_224", "sha3_256", "sha3_384", "sha3_512", "sha512", "update", "open", "file_seal", "file_open", "sign", "verify", "x25519", "x25519_public", "x25519_shared", "generatekey", "privateencrypt", "privatedecrypt", "publicencrypt", "publicdecrypt", "wrap", "unwrap", "password_hash", "password_hash_with_salt", "password_verify"] },
    CoreSourceModule { module: "core.crypto.expert", alias: "core_core_crypto_expert", path: "Core/crypto/expert.jet", owned_members: &["aes256gcm_open", "aes256gcm_seal", "argon2id", "ed25519_sign", "ed25519_verify_strict", "hkdf_sha256_raw", "migrate_v1", "open_v1", "secret_bytes", "shared_secret_bytes", "signing_key_bytes", "x25519_raw", "x25519_secret_bytes", "xchacha20poly1305_open", "xchacha20poly1305_seal"] },
    CoreSourceModule { module: "core.crypto.random", alias: "core_core_crypto_random", path: "Core/crypto/random.jet", owned_members: &["bytes", "choice", "choice_int", "compare_digest", "int_range", "randbelow", "randbits", "shuffle_ints", "token_bytes", "token_hex", "token_urlsafe", "u32", "u64"] },
    CoreSourceModule { module: "core.crypto.uuid", alias: "core_core_crypto_uuid", path: "Core/crypto/uuid.jet", owned_members: &["parse", "uuid5", "v4", "v5", "v7"] },
    CoreSourceModule { module: "core.crypto.vault", alias: "core_core_crypto_vault", path: "Core/crypto/vault.jet", owned_members: &["authorize_wrapped_import", "authorize_write", "commit_generate", "commit_import_signing", "commit_import_wrapped", "commit_import_x25519", "commit_retire", "commit_revoke", "commit_rotate", "commit_store", "current", "export_to_passphrase", "export_to_recipients", "get", "load", "prepare_generate", "prepare_import_signing", "prepare_import_wrapped", "prepare_import_x25519", "prepare_retire", "prepare_revoke", "prepare_rotate", "prepare_store", "status", "versions"] },
    CoreSourceModule { module: "core.data", alias: "core_core_data", path: "Core/data/data.jet", owned_members: &["bar_svg", "bar_text", "count", "csv", "csv_reader", "database", "describe", "file", "file_member", "inner_join", "inspect", "inspect_json", "json", "json_reader", "left_join", "line_svg", "line_text", "load", "max", "mean", "median", "min", "pivot_sum", "plot", "quantile", "query", "render", "require_bridge", "rolling_mean", "schema", "show", "snapshot", "status", "stddev", "sum", "svg", "text", "track", "url", "value", "variance", "load_default"] },
    CoreSourceModule { module: "core.data.arrow", alias: "core_core_data_arrow", path: "Core/data/arrow.jet", owned_members: &["import", "query"] },
    CoreSourceModule { module: "core.data.loader", alias: "core_core_data_loader", path: "Core/data/loader.jet", owned_members: &["authority", "bind", "bind_text", "cancel", "invalidate", "needs_refresh", "offline", "ready", "snapshot_reusable", "source_identity", "status", "stream"] },
    CoreSourceModule { module: "core.data.stream", alias: "core_core_data_stream", path: "Core/data/stream.jet", owned_members: &["cancel", "collect", "from_items", "is_empty", "len", "next", "skip", "take_n"] },
    CoreSourceModule { module: "core.data.plot", alias: "core_core_data_plot", path: "Core/data/plot.jet", owned_members: &["bar_svg", "bar_text", "inspect", "inspect_json", "line_svg", "line_text", "plot", "render", "show", "svg", "text"] },
    CoreSourceModule { module: "core.data.sketch", alias: "core_data_sketch", path: "Core/data/sketch.jet", owned_members: &["empty", "merge"] },
    CoreSourceModule { module: "core.data.sketch.cms", alias: "core_core_data_sketch_cms", path: "Core/data/sketch/cms.jet", owned_members: &["new", "merge"] },
    CoreSourceModule { module: "core.data.sketch.hll", alias: "core_core_data_sketch_hll", path: "Core/data/sketch/hll.jet", owned_members: &["new"] },
    CoreSourceModule { module: "core.data.sketch.reservoir", alias: "core_core_data_sketch_reservoir", path: "Core/data/sketch/reservoir.jet", owned_members: &["new"] },
    CoreSourceModule { module: "core.data.sketch.tdigest", alias: "core_core_data_sketch_tdigest", path: "Core/data/sketch/tdigest.jet", owned_members: &["new"] },
    CoreSourceModule { module: "core.db", alias: "core_core_db", path: "Core/db/db.jet", owned_members: &["decode", "migrate", "open", "open_memory", "policy", "policy_audit", "pool", "row_bool", "row_float", "row_int", "row_text", "row_value", "transaction"] },
    CoreSourceModule { module: "core.email", alias: "core_core_email", path: "Core/email/email.jet", owned_members: &["address", "attachment", "dkim", "envelope", "limits", "message", "send_report", "serialize", "smtp", "smtp_auth", "smtp_from_env"] },
    CoreSourceModule { module: "core.encoding", alias: "core_encoding", path: "Core/encoding/encoding.jet", owned_members: &["hex_nibble", "hex_value", "bytes_to_hex", "hex_to_bytes", "wrap32"] },
    CoreSourceModule { module: "core.encoding.base32", alias: "core_core_encoding_base32", path: "Core/encoding/base32.jet", owned_members: &["b32decode", "b32encode", "b32hexdecode", "b32hexencode", "decode", "encode", "is_base32"] },
    CoreSourceModule { module: "core.encoding.base64", alias: "core_encoding_base64", path: "Core/encoding/base64.jet", owned_members: &["is_base64", "a2b_base64", "a85decode", "a85encode", "b16decode", "b16encode", "b2a_base64", "b32decode", "b32encode", "b32hexdecode", "b32hexencode", "b64decode", "b64encode", "b85decode", "b85encode", "decode", "decode_padded", "decode_url", "decodebytes", "encode", "encode_url", "encode_url_padded", "encodebytes", "pad", "standard_b64decode", "standard_b64encode", "unpad", "urlsafe_b64decode", "urlsafe_b64encode", "z85decode", "z85encode"] },
    CoreSourceModule { module: "core.encoding.binary", alias: "core_core_encoding_binary", path: "Core/encoding/binary.jet", owned_members: &["calcsize", "iter_unpack", "pack", "pack_f64be", "pack_f64le", "pack_i8", "pack_u16be", "pack_u16le", "pack_u32be", "pack_u32le", "pack_u64be", "pack_u64le", "pack_u8", "sign_extend", "unpack", "unpack_f64be", "unpack_f64le", "unpack_u16be", "unpack_u16le", "unpack_u32be", "unpack_u32le", "unpack_u64be", "unpack_u64le", "unpack_u8"] },
    CoreSourceModule { module: "core.encoding.cbor", alias: "core_core_encoding_cbor", path: "Core/encoding/cbor.jet", owned_members: &["reader", "writer"] },
    CoreSourceModule { module: "core.encoding.csv", alias: "core_core_encoding_csv", path: "Core/encoding/csv.jet", owned_members: &["dict_get", "dict_get_or", "dict_rows", "fieldnames", "parse", "rows", "write_dict"] },
    CoreSourceModule { module: "core.encoding.hex", alias: "core_core_encoding_hex", path: "Core/encoding/hex.jet", owned_members: &["a2b_base64", "a2b_hex", "a2b_qp", "a2b_uu", "b2a_base64", "b2a_hex", "b2a_qp", "b2a_uu", "crc32", "crc_hqx", "decode", "dump", "encode", "encode_prefixed", "encode_sep", "encode_upper", "hexlify", "is_hex", "unhexlify"] },
    CoreSourceModule { module: "core.encoding.ini", alias: "core_core_encoding_ini", path: "Core/encoding/ini.jet", owned_members: &["defaults", "empty", "get", "get_bool", "get_float", "get_int", "get_or", "has_option", "has_section", "items", "options", "parse", "remove_option", "remove_section", "sections", "set", "to_string"] },
    CoreSourceModule { module: "core.encoding.json", alias: "core_core_encoding_json", path: "Core/encoding/json.jet", owned_members: &["canonical", "dump", "dumps", "events", "load", "loads", "parse", "parse_allow_duplicates", "patch", "patch_with_limits", "pointer", "reader", "reader_allow_duplicates", "writer"] },
    CoreSourceModule { module: "core.encoding.jsonl", alias: "core_core_encoding_jsonl", path: "Core/encoding/jsonl.jet", owned_members: &["append_line", "count_rows", "dumps", "first", "loads", "parse", "reader", "to_string", "writer"] },
    CoreSourceModule { module: "core.encoding.toml", alias: "core_core_encoding_toml", path: "Core/encoding/toml.jet", owned_members: &["load", "loads", "parse"] },
    CoreSourceModule { module: "core.encoding.xml", alias: "core_core_encoding_xml", path: "Core/encoding/xml.jet", owned_members: &["attribute", "canonical", "content", "parse", "parse_bytes", "parse_with", "reader", "root", "to_bytes", "to_string"] },
    CoreSourceModule { module: "core.event", alias: "core_core_event", path: "Core/event/event.jet", owned_members: &["async_result", "decision_hook", "hook", "new", "policy_sync", "scope", "with_policy"] },
    CoreSourceModule { module: "core.files", alias: "core_core_files", path: "Core/files/files.jet", owned_members: &["basename", "is_absolute", "is_file", "scope", "stem", "suffix", "with_name", "with_suffix", "is_abs", "split_slash", "collapse", "join", "dirname", "glob_match", "relative", "is_symlink", "cwd", "home", "expanduser", "touch", "relocate", "replace", "relativeto", "chmod", "copy2", "copyfile", "mkdir", "rmdir", "unlink", "joinpath", "getcwd", "chdir", "scandir", "gettempdir", "getenv", "read_lines", "write_lines", "commonpath", "fnmatch", "which", "splitext", "split", "normpath", "getsize", "getmtime", "lexists", "samefile", "commonprefix", "expandvars", "move", "rmtree", "makedirs", "listdir", "glob_recursive", "fnmatch_filter", "is_mount", "ensure_parent", "readdir", "walkdir", "truncate", "createdirectory", "filesize", "delete", "cp", "rm", "realpath", "pwd", "tmpdir", "topath", "cd"] },
    CoreSourceModule { module: "core.encoding.yaml", alias: "core_core_encoding_yaml", path: "Core/encoding/yaml.jet", owned_members: &["parse"] },
    CoreSourceModule { module: "core.files.path", alias: "core_core_files_path", path: "Core/files/path.jet", owned_members: &["absolute", "anchor", "append_text", "as_posix", "as_uri", "as_windows", "chmod", "collapse", "copy_file", "copy_into", "cwd", "drive", "ensure_dir", "exists", "expanduser", "from_parts", "glob", "hardlink_to", "home", "is_absolute", "is_dir", "is_empty", "is_file", "is_relative", "is_relative_to", "is_symlink", "iterdir", "join", "join_many", "join_path", "match_glob", "match_path", "mkdir", "mtime", "name", "of", "parent", "parents", "parts", "read_bytes", "read_lines", "read_text", "readlink", "relative_to", "rename", "replace", "resolve", "resolve_pure", "rglob", "rmdir", "rmtree", "root", "samefile", "size", "stat", "stem", "suffix", "suffixes", "symlink_to", "touch", "unlink", "with_name", "with_segments", "with_stem", "with_suffix", "write_bytes", "write_lines", "write_text", "path", "walk", "which", "open_read", "open_write", "show", "equals"] },
    CoreSourceModule { module: "core.font", alias: "core_core_font", path: "Core/font/font.jet", owned_members: &["shape", "shape_with", "system"] },
    CoreSourceModule { module: "core.game", alias: "core_core_game", path: "Core/game/game.jet", owned_members: &[] },
    CoreSourceModule { module: "core.game.raylib", alias: "core_core_game_raylib", path: "Core/game/raylib.jet", owned_members: &["begin_drawing", "clear_background", "close_window", "color", "draw_rectangle", "draw_sprite", "draw_text", "end_drawing", "gamepad_axis", "gamepad_down", "key_down", "load_sound", "load_texture_atlas", "play_sound", "set_target_fps", "window_open", "window_ready", "window_should_close"] },
    CoreSourceModule { module: "core.http", alias: "core_core_http", path: "Core/http/http.jet", owned_members: &["body_as_text", "body_bytes", "body_empty", "body_text", "headers", "headers_all", "headers_append", "headers_encode", "headers_get", "headers_parse", "headers_remove", "headers_set", "is_client_error", "is_informational", "is_redirect", "is_server_error", "is_success", "method_parse", "method_text", "status", "status_accepted", "status_already_reported", "status_bad_gateway", "status_bad_request", "status_conflict", "status_continue", "status_created", "status_expectation_failed", "status_failed_dependency", "status_forbidden", "status_found", "status_gateway_timeout", "status_gone", "status_http_version", "status_legal", "status_length_required", "status_locked", "status_method_not_allowed", "status_moved", "status_multi_status", "status_no_content", "status_non_authoritative", "status_not_acceptable", "status_not_found", "status_not_implemented", "status_not_modified", "status_ok", "status_partial", "status_payload_too_large", "status_payment_required", "status_permanent", "status_precondition_failed", "status_precondition_required", "status_range_unsat", "status_reset_content", "status_see_other", "status_server_error", "status_switching", "status_teapot", "status_temporary", "status_timeout", "status_too_early", "status_too_many", "status_unauthorized", "status_unavailable", "status_unprocessable", "status_unsupported_media", "status_upgrade_required", "status_uri_too_long", "version_parse", "version_text", "body_as_bytes", "body_len", "reason_phrase", "get", "post", "put", "patch", "delete", "head", "serve", "exchange", "send_request", "parse_response", "parse_request", "query", "query_get", "query_all", "query_set", "query_encode", "query_decode", "form_encode", "form_decode", "cookie", "cookie_encode", "cookies_parse", "basic_auth", "bearer_auth", "with_header", "with_body", "with_query", "status_class", "request_with", "response_ok", "response_json_headers", "response_text_headers", "has_header", "content_length"] },
    CoreSourceModule { module: "core.http.client", alias: "core_core_http_client", path: "Core/http/client.jet", owned_members: &["accept", "bearer", "content_type", "cookie_header", "header", "redirect_limit", "session", "session_auth", "session_cookie", "session_delete", "session_form", "session_get", "session_head", "session_header", "session_json", "session_patch", "session_post", "session_proxy", "session_put", "session_redirects", "session_request", "session_retries", "session_timeout", "set_header", "timeout_redirects", "user_agent", "with_proxy", "get", "post", "request", "send"] },
    CoreSourceModule { module: "core.http.server", alias: "core_core_http_server", path: "Core/http/server.jet", owned_members: &["access_log", "cors", "cors_policy", "json", "mux", "request_id", "response", "serve_once", "serve_once_listener", "sse", "static_file", "static_file_range", "static_files", "tls"] },
    CoreSourceModule { module: "core.jobs", alias: "core_core_jobs", path: "Core/jobs/jobs.jet", owned_members: &["ack", "cancel", "claim", "enqueue", "fail", "named", "queue", "status", "with_max_attempts", "queue_name", "state", "attempts", "payload", "event", "events", "is_terminal", "is_cancelled"] },
    CoreSourceModule { module: "core.log", alias: "core_core_log", path: "Core/log/log.jet", owned_members: &["bool", "close", "counter", "critical", "debug", "debug_fields", "disable", "enabled", "enter", "error", "error_fields", "fatal", "field", "float", "flush", "format", "formatter", "handle", "handler", "handler_format", "info", "info_fields", "int", "log", "otlp_file", "record", "redact", "sample_every", "set_level", "set_sink", "set_trace_id", "setup", "span", "warn", "warn_fields", "warning", "add", "group", "time", "clear"] },
    CoreSourceModule { module: "core.math", alias: "core_math", path: "Core/math/math.jet", owned_members: &["abs", "abs_float", "min", "max", "clamp", "is_even", "is_odd", "sign", "isqrt", "gcd", "lcm", "acos", "acosh", "asin", "asinh", "atan", "atan2", "atanh", "cbrt", "ceil", "checked_abs", "checked_add", "checked_div", "checked_mul", "checked_neg", "checked_pow", "checked_rem", "checked_sub", "conj", "copysign", "cos", "cosh", "cot", "degrees", "e", "erf", "erfc", "exp", "exp2", "exp_m1", "expm1", "fabs", "factorial", "float32", "float64", "floor", "fma", "fract", "frexp", "from_bits", "gamma", "hypot", "ilogb", "imag", "infinity", "int_pow", "inv", "is_canonical", "is_finite", "is_inf", "is_integer", "is_nan", "is_normal", "is_signed", "is_subnormal", "is_zero", "isfinite", "isinf", "isnan", "ldexp", "lerp", "lgamma", "ln", "ln_1p", "log", "log10", "log1p", "log2", "logb", "modf", "muladd", "nan", "next_after", "next_down", "next_up", "pi", "pow", "radians", "radix", "real", "round", "saturating_add", "saturating_mul", "saturating_sub", "scaleb", "sign_bit", "significand", "signum", "sin", "sin_cos", "sinh", "sqrt", "tan", "tanh", "tau", "tau_const", "to_bits", "trunc", "truncate", "ulp", "zero", "binomial", "comb", "perm", "gcd_many", "lcm_many", "powmod", "div_mod", "div_rem", "digits", "leading_ones", "trailing_ones", "cmp", "abs_diff", "even", "odd", "xor", "in_range", "sum_int", "prod_int", "prod", "fmod", "remainder", "isclose", "dist", "hypot3", "clamp_float", "min_float", "max_float", "midpoint", "fsum", "sumprod", "random", "fraction", "decimal"] },
    CoreSourceModule { module: "core.math.random", alias: "core_core_math_random", path: "Core/math/random.jet", owned_members: &["betavariate", "binomialvariate", "bool", "bytes", "choice", "choices", "exponential", "expovariate", "float", "float_range", "gammavariate", "gauss", "getrandbits", "int", "lognormvariate", "normal", "normalvariate", "paretovariate", "pick", "randbytes", "randint", "random", "randrange", "rng", "sample", "seed", "shuffle", "split", "triangular", "uniform", "vonmisesvariate", "weibullvariate", "weighted_pick"] },
    CoreSourceModule { module: "core.math.combinatorics", alias: "core_core_math_combinatorics", path: "Core/math/combinatorics.jet", owned_members: &["batched", "binomial", "cartesian", "chain", "chain_from", "combinations", "combinations_count", "combinations_with_replacement", "compress", "count_from", "drop", "dropwhile", "factorial", "falling_factorial", "multinomial", "ncr", "npr", "pairwise", "permutations", "permutations_count", "product", "repeat", "rising_factorial", "takewhile", "unique", "take_n", "count", "filterfalse", "islice", "groupby", "starmap", "tee", "cycle", "zip_longest", "accumulate", "powerset", "windows", "reverse", "flatten"] },
    CoreSourceModule { module: "core.math.stats", alias: "core_core_math_stats", path: "Core/math/stats.jet", owned_members: &["correlation", "covariance", "cumsum", "fmean", "geometric_mean", "harmonic_mean", "kde", "kde_random", "linear_regression", "max", "mean", "median", "median_grouped", "median_high", "median_low", "min", "mode", "multimode", "percentile", "prod", "pstdev", "pvariance", "quantile", "range", "stdev", "sum", "variance", "zscore", "normal_dist", "count", "sumprod", "weighted_mean", "quantiles", "iqr", "mad", "mean_abs_deviation", "skew", "kurtosis", "moving_average", "ewma", "cumprod", "diff", "rank", "spearman", "r_squared", "residuals", "histogram", "describe", "clip", "winsorize", "pearson", "covariance_population"] },
    CoreSourceModule { module: "core.mem", alias: "core_core_mem", path: "Core/mem/mem.jet", owned_members: &["address_of", "from_addr", "volatile_read", "volatile_write"] },
    CoreSourceModule { module: "core.mod", alias: "core_core_mod", path: "Core/mod/mod.jet", owned_members: &["is_loaded", "load", "path", "unload"] },
    CoreSourceModule { module: "core.net", alias: "core_core_net", path: "Core/net/net.jet", owned_members: &["addressfamily", "create_connection", "create_server", "dns_a", "dns_aaaa", "dns_a_at", "dns_aaaa_at", "dns_ptr", "dns_srv", "dns_srv_at", "dns_srv_port", "dns_srv_priority", "dns_srv_target", "dns_srv_weight", "dns_txt", "dns_txt_at", "error_address", "error_message", "error_name", "error_operation", "error_os_code", "gethostbyaddr", "gethostbyname", "gethostname", "getservbyname", "getservbyport", "ip_addr", "ip_is_ipv4", "ip_to_string", "listener_local_socket_addr", "nodelay", "ready_readable", "ready_writable", "send", "sendfile", "set_nodelay", "set_read_timeout", "set_timeout", "set_ttl", "set_write_timeout", "socket_addr", "socket_addr_parse", "socket_host", "socket_port", "socket_to_string", "socket_type", "tcp_accept", "tcp_close", "tcp_connect", "tcp_connect_addr", "tcp_connect_happy", "tcp_connect_timeout", "tcp_listen", "tcp_listen_addr", "tcp_local_addr", "tcp_local_socket_addr", "tcp_peer_addr", "tcp_peer_socket_addr", "tcp_read", "tcp_read_bytes", "tcp_read_text", "tcp_ready", "tcp_reply", "tcp_shutdown", "tcp_write", "tcp_write_all_bytes", "tcp_write_bytes", "tcp_write_text", "ttl", "udp_bind", "udp_bind_addr", "udp_local_addr", "udp_packet_addr", "udp_packet_bytes", "udp_packet_data", "udp_packet_original_len", "udp_packet_truncated", "udp_receive", "udp_recv_from", "udp_send_bytes_to", "udp_send_to", "udp_set_timeout", "unix_accept", "unix_close", "unix_connect", "unix_listen", "unix_read", "unix_read_bytes", "unix_shutdown", "unix_write", "unix_write_all_bytes"] },
    CoreSourceModule { module: "core.net.mime", alias: "core_core_net_mime", path: "Core/net/mime.jet", owned_members: &["extension", "from_extension", "parse"] },
    CoreSourceModule { module: "core.net.tls", alias: "core_core_net_tls", path: "Core/net/tls.jet", owned_members: &["alpn_h2", "client", "close", "config", "connect_host", "flags", "has_alpn", "identity", "is_tls12", "is_tls13", "parse_version", "peer_port", "read", "read_text", "roots", "sni", "unwrap", "version_name", "with_alpn", "write", "write_all", "write_text"] },
    CoreSourceModule { module: "core.net.url", alias: "core_core_net_url", path: "Core/net/url.jet", owned_members: &["data", "file", "from_parts", "geturl", "parse", "percent_decode", "percent_encode", "query", "unparse", "urljoin", "parse_qsl", "urlencode", "split_fragment", "quote", "quote_from_bytes", "quote_plus", "unquote", "unquote_to_bytes", "unquote_plus", "urlparse", "urlsplit", "urlunparse", "urlunsplit", "urldefrag", "parse_qs"] },
    CoreSourceModule { module: "core.net.ws", alias: "core_core_net_ws", path: "Core/net/ws.jet", owned_members: &["connect", "upgrade"] },
    CoreSourceModule { module: "core.net.ip", alias: "core_core_net_ip", path: "Core/net/ip.jet", owned_members: &["ipv4", "ipv4_from_int", "ipv4_int", "ipv4_to_string", "ipv6_is_link_local", "ipv6_is_loopback", "ipv6_is_unspecified", "ipv6_to_string", "is_global", "is_link_local", "is_loopback", "is_multicast", "is_private", "is_reserved", "is_unspecified", "network", "network_broadcast", "network_contains", "network_hosts", "parse_ipv4", "parse_ipv6", "packed_ipv4", "from_packed_ipv4", "reverse_pointer", "with_prefixlen", "with_netmask", "with_hostmask", "num_addresses", "network_first", "network_last", "network_overlaps", "subnet_of", "supernet_of", "supernet", "subnets", "hosts", "interface", "is_carrier_grade_nat", "is_benchmarking", "is_documentation", "is_shared", "ipv6_is_multicast", "ipv6_is_private", "ipv6_is_global", "ipv6_packed", "ipv6_compressed", "compare_ipv4", "ipv4_equals"] },
    CoreSourceModule { module: "core.perf", alias: "core_core_perf", path: "Core/perf/perf.jet", owned_members: &["default_fidelity", "fidelity", "is_full", "is_low", "of", "override_fidelity", "reset_fidelity", "scale"] },
    CoreSourceModule { module: "core.prelude", alias: "core_core_prelude", path: "Core/prelude/prelude.jet", owned_members: &["always", "const_bool", "const_int", "identity", "identity_bool", "identity_float", "identity_int", "keep", "max_int", "min_int", "not_bool"] },
    CoreSourceModule { module: "core.process", alias: "core_process", path: "Core/process/process.jet", owned_members: &["status_ok", "arg", "args", "args_extend", "argv", "call", "capture", "check", "check_call", "check_output", "cmd", "combined_output", "current_pid", "cwd", "env", "env_get", "env_get_or", "env_keys", "env_set", "env_truthy", "exit", "exited", "failed", "getoutput", "getstatusoutput", "list2cmdline", "on_signal", "pipeline", "run_spec", "shell", "signal_number", "stderr_lines", "stdin_text", "stdout_lines", "which"] },
    CoreSourceModule { module: "core.reactive", alias: "core_core_reactive", path: "Core/reactive/reactive.jet", owned_members: &["computed_get", "computed_set", "effect_last", "effect_run", "get", "set", "update", "version", "set_if_changed", "changed", "computed_update", "computed_version", "effect_run_if", "effect_changed"] },
    CoreSourceModule { module: "core.reactive.loadable", alias: "core_core_reactive_loadable", path: "Core/reactive/loadable.jet", owned_members: &["failed", "idle", "loaded", "loading", "is_idle", "is_loading", "is_loaded", "is_failed", "value", "reason", "retry"] },
    CoreSourceModule { module: "core.reflect", alias: "core_core_reflect", path: "Core/reflect/reflect.jet", owned_members: &["inspect"] },
    CoreSourceModule { module: "core.regex", alias: "core_core_regex", path: "Core/regex/regex.jet", owned_members: &["compile", "compile_with", "escape", "expand", "find", "find_all", "finditer", "flags", "full_match", "is_match", "match", "matches", "purge", "replace", "replace_first", "split", "split_limit", "regex", "join", "search", "findall", "fullmatch", "sub", "subn"] },
    CoreSourceModule { module: "core.rt", alias: "core_core_rt", path: "Core/rt/rt.jet", owned_members: &["callback"] },
    CoreSourceModule { module: "core.service", alias: "core_core_service", path: "Core/service/service.jet", owned_members: &["tree", "runtime", "state_store", "restart_one_for_one", "restart_one_for_all", "restart_rest_for_one", "delivery_at_most_once", "delivery_durable"] },
    CoreSourceModule { module: "core.sync", alias: "core_core_sync", path: "Core/sync/sync.jet", owned_members: &["counter_inc", "counter_merge", "counter_new", "counter_value", "list_merge", "list_new", "list_push", "list_show", "map_get", "map_merge", "map_new", "map_set", "map_show", "policy_allows", "policy_new", "policy_show", "text_edit", "text_merge", "text_metadata", "text_new", "text_set", "text_show", "counter_for", "map_for", "map_delete", "map_contains", "map_keys", "list_for", "list_remove", "list_contains", "text_for", "text_append", "text_clock", "text_replica", "policy_grant", "policy_deny", "policy_revoke"] },
    CoreSourceModule { module: "core.sys", alias: "core_sys", path: "Core/sys/sys.jet", owned_members: &["success", "arch", "close_fd", "cpu_count", "current_dir", "executable", "exitcode", "expand", "family", "fork", "get", "getegid", "getenv", "geteuid", "getgid", "getgroups", "getpgid", "getpgrp", "getpid", "getppid", "getpriority", "getsid", "getuid", "home_dir", "hostname", "initgroups", "is_linux", "is_macos", "is_unix", "is_windows", "kill", "linesep", "loadavg", "machine", "mkfifo", "name", "pathsep", "pid", "pipe", "platform", "release", "set", "set_current_dir", "setgid", "setpgid", "setpgrp", "setpriority", "setsid", "setuid", "stop", "sync", "sysname", "temp_dir", "times", "umask", "uname", "unset", "uptime", "username", "utime", "vars", "version", "wait", "waitpid"] },
    CoreSourceModule { module: "core.tasks", alias: "core_core_tasks", path: "Core/tasks/tasks.jet", owned_members: &["acquire", "after", "channel", "clear", "current_task", "delay_ms", "get", "interval", "is_interval", "is_timer", "lock", "notify", "put", "recv", "release", "reset", "result", "run", "shutdown", "start", "stop", "wait", "waitall", "waitany", "yield_now", "is_cancelled", "is_ready", "timeout", "sleep", "try_recv", "cancel", "is_closed", "is_locked", "size", "capacity", "generation", "spawn_name"] },
    CoreSourceModule { module: "core.term", alias: "core_core_term", path: "Core/term/term.jet", owned_members: &["binread", "binwrite", "buffered", "choose", "confirm", "eprint", "input", "input_secret", "print", "progress", "read_all_input", "read_key", "read_until", "readline", "stderr", "stdin", "stdout", "style", "style_force", "terminal_height", "terminal_width"] },
    CoreSourceModule { module: "core.testing", alias: "core_core_testing", path: "Core/testing/testing.jet", owned_members: &["assert_equal", "compare", "corpus", "fake_clock", "fake_data", "fake_rng", "fixture", "golden", "snap", "status", "temp_dir", "test_suite", "world"] },
    CoreSourceModule { module: "core.text", alias: "core_text", path: "Core/text/text.jet", owned_members: &["byte_count", "byte_views", "casefold", "caseless_eq", "center", "char_indices", "cursor", "cursor_advance", "display_width", "ends_any", "grapheme_views", "graphemes", "inspect", "is_alphabetic", "is_ascii", "is_numeric", "is_whitespace", "line_views", "lower", "nfc", "nfd", "nfkc", "nfkd", "pad_end", "pad_start", "rsplitn", "scalar_count", "scalars", "sentences", "splitn", "starts_any", "trim", "trim_end", "trim_start", "upper", "word_views", "words"] },
    CoreSourceModule { module: "core.text.fmt", alias: "core_text_fmt", path: "Core/text/fmt.jet", owned_members: &["bin", "bytes", "decimal", "duration", "grouped", "hex", "number", "oct", "ordinal", "pad", "pad_center", "pad_left", "pad_right", "percent", "plural", "pretty", "sci"] },
    CoreSourceModule { module: "core.text.html", alias: "core_text_html", path: "Core/text/html.jet", owned_members: &["attr_escape", "escape", "escape_quote", "strip_tags", "text_escape", "unescape", "unescape_and_strip"] },
    CoreSourceModule { module: "core.text.parse", alias: "core_text_parse", path: "Core/text/parse.jet", owned_members: &["capitalize", "capwords", "center", "contains", "count", "encode", "ends_with", "endswith", "expandtabs", "find", "find_from", "index", "is_alnum", "isalpha", "isalnum", "isascii", "isdecimal", "is_digit", "isdigit", "is_identifier", "isidentifier", "is_lower", "islower", "isprintable", "is_space", "isspace", "is_title", "istitle", "is_upper", "isupper", "join", "ljust", "lower", "lstrip", "parse", "parse_bool", "parse_float", "parse_int", "parse_int_base", "parse_kv", "partition", "removeprefix", "removesuffix", "replace", "rfind", "rfind_from", "rindex", "rjust", "rpartition", "rstrip", "split", "split_once", "split_ws", "splitlines", "starts_with", "startswith", "strip", "strip_prefix", "strip_suffix", "swapcase", "title", "unescape_c", "upper", "zfill", "rsplit", "is_ascii", "isnumeric", "escape_c"] },
    CoreSourceModule { module: "core.text.wrap", alias: "core_text_wrap", path: "Core/text/wrap.jet", owned_members: &["dedent", "expand_tabs", "fill", "fill_with", "hanging_indent", "html_escape", "html_unescape", "indent", "indent_with", "shorten", "wrap", "wrap_paragraphs", "wrap_with", "wrapper"] },
    CoreSourceModule { module: "core.time", alias: "core_time", path: "Core/time/time.jet", owned_members: &["unix_seconds", "unix_ns", "start", "elapsed", "asctime", "ctime", "gmtime", "utcoffset", "weekday", "isoformat_date", "isoformat_time", "isoformat", "strftime", "unix_ms", "add_days", "add_duration", "since", "duration_seconds", "duration_ms", "duration_ns", "duration_as_seconds", "compare_date", "date_equals", "combine", "date_of", "time_of", "replace_date", "replace_time", "isoweekday", "weekday_sun0", "duration_minutes", "duration_hours", "duration_days", "duration_as_ms", "duration_as_minutes", "duration_as_hours", "add_seconds", "add_minutes", "add_hours", "start_of_day", "end_of_day", "date_before", "date_after", "between", "duration_zero", "duration_is_zero", "duration_abs", "duration_add", "duration_sub", "min_date", "max_date", "days_between", "fromhours", "before", "after", "dayofweek", "todate", "totime"] },
    CoreSourceModule { module: "core.time.calendar", alias: "core_core_time_calendar", path: "Core/time/calendar.jet", owned_members: &["day_abbr", "day_name", "formatmonth", "formatyear", "isleap", "leapdays", "month_abbr", "month_name", "monthcalendar", "monthcalendar_start", "monthrange", "timegm", "weekday", "weekheader", "yearcalendar"] },
    CoreSourceModule { module: "core.time.expiring", alias: "core_time_expiring", path: "Core/time/expiring.jet", owned_members: &["expired", "remaining_ms"] },
    CoreSourceModule { module: "core.ui", alias: "core_core_ui", path: "Core/ui/ui.jet", owned_members: &["aria_role_button", "aria_role_container", "aria_role_label", "aria_role_text_input", "box", "constraint", "desktop", "gtk_backend", "key_event", "node", "node_accessibility", "node_color", "node_role", "node_shortcut", "null_backend", "phone", "point", "rect", "resize_event", "size", "tablet", "text", "tui_backend"] },
    CoreSourceModule { module: "core.tui", alias: "core_core_tui", path: "Core/tui/tui.jet", owned_members: &["ascii", "capabilities", "close_event", "color_ansi16", "color_ansi256", "color_rgb", "display_width", "fill", "focus_event", "horizontal", "interrupt_event", "io_event", "key_event", "key_event_modifiers", "layout", "length", "list", "list_state", "list_state_offset", "list_state_select", "list_state_selected", "max", "min", "percent", "resize_event", "style", "style_background", "style_bold", "style_dim", "style_foreground", "style_text", "style_underline", "table", "timer_event", "vertical"] },
    CoreSourceModule { module: "core.ui.host", alias: "core_core_ui_host", path: "Core/ui/host.jet", owned_members: &["accessibility", "capabilities", "file_filter", "file_filter_text", "fs_grant", "fs_rights_read", "fs_rights_read_write", "fs_rights_write", "open_file", "open_request", "save_file", "save_request", "shortcut"] },
    CoreSourceModule { module: "core.ui.host.clipboard", alias: "core_core_ui_host_clipboard", path: "Core/ui/host/clipboard.jet", owned_members: &["clear", "is_empty", "read_text", "write_text"] },
    CoreSourceModule { module: "core.ui.host.ime", alias: "core_core_ui_host_ime", path: "Core/ui/host/ime.jet", owned_members: &["poll"] },
    CoreSourceModule { module: "core.ui.host.drag_drop", alias: "core_core_ui_host_drag_drop", path: "Core/ui/host/drag_drop.jet", owned_members: &["poll"] },
    CoreSourceModule { module: "core.ui.host.shortcuts", alias: "core_core_ui_host_shortcuts", path: "Core/ui/host/shortcuts.jet", owned_members: &["binding", "dispatch", "register"] },
    CoreSourceModule { module: "core.ui.host.accessibility", alias: "core_core_ui_host_accessibility", path: "Core/ui/host/accessibility.jet", owned_members: &["attach", "project"] },
    CoreSourceModule { module: "core.units", alias: "core_units", path: "Core/units/units.jet", owned_members: &["from", "abs", "add", "bytes_of", "convert", "div", "equals", "grams", "is_zero", "kibibytes", "kilograms", "kilometers", "meters", "milliseconds", "mul", "ratio", "scale", "seconds", "show", "si", "sub", "to_si", "metres", "kilometres"] },
    CoreSourceModule { module: "core.watcher", alias: "core_core_watcher", path: "Core/watcher/watcher.jet", owned_members: &["add", "contains", "files", "kind", "len", "port", "process_pid", "remove", "set", "target", "recursive", "debounce", "poll", "events", "cancel", "is_active", "summary", "drain"] },
    CoreSourceModule { module: "core.web", alias: "core_core_web", path: "Core/web/web.jet", owned_members: &["app", "auth", "auth_oauth", "auth_routes", "auth_show", "form", "invalidate", "live", "live_get", "live_show", "live_stats", "on", "openapi", "page", "signal_push", "storage", "subscribe", "sync", "transact_invalidate", "value"] },
    CoreSourceModule { module: "core.web.browser", alias: "core_core_web_browser", path: "Core/web/browser.jet", owned_members: &["begin_named", "config", "config_from_env", "connect", "connect_browser_profile", "fixture_context", "fixture_page", "fixture_source", "generate_source", "locked", "make_profile", "report_add_case", "report_exit_code", "report_html", "report_json", "report_new", "report_text", "selected", "server_logs", "server_start", "server_stop", "server_url", "timeout", "watch_changed", "write_report"] },
    CoreSourceModule { module: "core.web.devserver", alias: "core_core_web_devserver", path: "Core/web/devserver.jet", owned_members: &["app", "for_app", "is_local", "url"] },
    CoreSourceModule { module: "core.web.forms", alias: "core_core_web_forms", path: "Core/web/forms.jet", owned_members: &["action_error", "action_field_error", "action_form_error", "blur", "field", "html", "input", "input_exclude", "input_group", "input_rename", "input_replace", "new", "no_script", "set", "show", "submit", "typed", "typed_blur", "typed_cancel", "typed_decode_post", "typed_errors", "typed_focus", "typed_html", "typed_lifecycle", "typed_no_script", "typed_post", "typed_select_field", "typed_set", "typed_set_action", "typed_set_async_validator", "typed_show", "typed_state", "typed_submission_cancel", "typed_submission_wait", "typed_submit", "typed_submit_async", "typed_validate", "typed_validate_async", "typed_validate_field", "typed_validation_cancel", "typed_validation_render", "typed_validation_wait", "validate", "validate_async"] },
    CoreSourceModule { module: "core.web.query", alias: "core_core_web_query", path: "Core/web/query.jet", owned_members: &["cancel", "facts", "get", "invalidate", "live", "mutate", "mutate_with_invalidations", "mutation_signal", "mutation_state", "new", "queue", "refresh", "retry", "set_mode", "set_online", "show", "state", "state_signal", "subscribe"] },
    CoreSourceModule { module: "core.web.router", alias: "core_core_web_router", path: "Core/web/router.jet", owned_members: &["abort", "cache_show", "cache_state", "collect", "current", "invalidate", "link", "navigate", "new", "not_found", "preload", "route", "route_with_search_codec", "stale"] },
    CoreSourceModule { module: "core.web.storage", alias: "core_web_storage", path: "Core/web/storage.jet", owned_members: &["local", "session", "kind_local", "kind_session"] },
    CoreSourceModule { module: "core.web.storage.local", alias: "core_core_web_storage_local", path: "Core/web/storage/local.jet", owned_members: &["clear", "get", "get_or", "has", "remove", "set"] },
    CoreSourceModule { module: "core.web.storage.session", alias: "core_core_web_storage_session", path: "Core/web/storage/session.jet", owned_members: &["clear", "get", "get_or", "has", "remove", "set"] },
    CoreSourceModule { module: "core.web.store", alias: "core_core_web_store", path: "Core/web/store.jet", owned_members: &["back", "batch", "clear_history", "current_generation", "cursor", "derived", "event_json", "events", "events_since", "facts_json", "forward", "get", "history", "history_at", "history_enabled", "history_limit", "inspect", "jump", "new", "optimistic", "patch", "patch_active", "patch_commit", "patch_generation", "patch_rollback", "patch_transaction", "restore", "scrub", "selector", "set", "set_history_limit", "set_state", "signal", "state_signal", "subscribe", "subscribe_selector", "subscription_active", "subscription_unsubscribe", "transaction", "update", "value", "with_history"] },
    CoreSourceModule { module: "core.web.table", alias: "core_core_web_table", path: "Core/web/table.jet", owned_members: &["clear_focus", "clear_selection", "column", "facts", "filter", "filter_by", "first_page", "focus", "focused_key", "insert_row", "keys", "last_page", "new", "new_keyed", "next_page", "page", "page_state", "paginate", "remove_row", "replace_row", "selected_keys", "selected_rows", "set_rows", "set_selected", "sort", "sort_by", "state", "toggle_selection", "update_row", "visible_rows", "with_column", "with_server_page"] },
    CoreSourceModule { module: "core.web.virtual", alias: "core_core_web_virtual", path: "Core/web/virtual.jet", owned_members: &["indices", "plan", "plan_facts", "plan_from_sizes", "plan_indices", "plan_measure", "plan_measured", "plan_resize", "plan_scroll_to", "plan_slice", "plan_viewport", "plan_viewport_measure", "plan_viewport_state", "slice", "window", "window_measured"] },
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


/// Return the canonical variant names for a Core enum. Payload shape is kept
/// by the checked source declaration, not this export-name table.
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

/// Source-owned Core packages loaded through the ordinary Jet frontend.
pub const fn core_source_modules() -> &'static [CoreSourceModule] {
    CORE_SOURCE_MODULES
}

/// Return the source package metadata for a Core module, if it has one.
pub fn core_source_module(module: &str) -> Option<&'static CoreSourceModule> {
    CORE_SOURCE_MODULES
        .iter()
        .find(|source| source.module == module)
}

/// Return the source package metadata for an emitted private alias.
pub fn core_source_module_by_alias(alias: &str) -> Option<&'static CoreSourceModule> {
    CORE_SOURCE_MODULES
        .iter()
        .find(|source| source.alias == alias)
}

/// Whether one public member routes to its source package. Transparent math
/// aliases keep the fixed, infallible Core primitive carrier at every tier.
pub fn core_source_owns(module: &str, member: &str) -> bool {
    if module == "core.math"
        && matches!(
            member,
            "cmp" | "cos" | "exp" | "fabs" | "is_finite" | "ln" | "sin" | "sqrt"
        )
    {
        return false;
    }
    core_source_module(module)
        .is_some_and(|source| source.owned_members.contains(&member))
}

/// Whether one call of `member` with `argc` arguments routes to its source
/// package. D-VERDICT-1321-1: `core.term` owns only the one-argument
/// `print`/`eprint`/`progress` wrappers; the variadic print forms and the
/// three-argument progress adapter keep the fixed Core route.
pub fn core_source_owns_call(module: &str, member: &str, argc: usize) -> bool {
    if module == "core.term" && matches!(member, "print" | "eprint" | "progress") && argc != 1 {
        return false;
    }
    core_source_owns(module, member)
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
        // D-CORE-TREE1: `core.crypto` moved to Jet source (09-23), and its
        // nominal identity comes from the crypto leaf table, not a leaf kind.
        assert_eq!(
            core_leaf_kind("core.crypto", "Secret"),
            Some(CoreLeafKind::Plain)
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

    #[test]
    fn transparent_math_primitives_keep_the_fixed_provider_route() {
        assert!(!core_source_owns("core.math", "is_finite"));
        assert!(!core_source_owns("core.math", "sqrt"));
        assert!(core_source_owns("core.math", "is_even"));
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
        assert_eq!(web.dependencies, &["core.http", "core.web.forms"]);
    }
}
