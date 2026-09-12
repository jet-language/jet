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
// Source SHA-256: 899a77cc91eadc9185e289e89a0dec593cbb1ce6f0834dadf77fdac0ae061648
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
    "core.data.sketch.cms",
    "core.data.sketch.hll",
    "core.data.sketch.reservoir",
    "core.data.sketch.tdigest",
    "core.db",
    "core.email",
    "core.encoding",
    "core.encoding.base32",
    "core.encoding.base64",
    "core.encoding.cbor",
    "core.encoding.csv",
    "core.encoding.hex",
    "core.encoding.json",
    "core.encoding.jsonl",
    "core.encoding.toml",
    "core.encoding.xml",
    "core.encoding.yaml",
    "core.event",
    "core.files",
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
    "core.mem",
    "core.mem.scope",
    "core.mod",
    "core.net",
    "core.net.mime",
    "core.net.tls",
    "core.net.url",
    "core.net.ws",
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
    "core.time",
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

const CORE_MODULE_2_MEMBERS: &[&str] = &["open"];
const CORE_MODULE_2_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_2_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_3_MEMBERS: &[&str] = &["publish"];
const CORE_MODULE_3_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_3_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_4_MEMBERS: &[&str] = &["adler32", "crc32", "deflate", "inflate", "tar_add", "tar_get", "tar_names_json", "unzip", "zip_close", "zip_compress", "zip_decompress", "zip_extract", "zip_names_json", "zip_next", "zip_open", "zip_read", "zip_write"];
const CORE_MODULE_4_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_4_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_5_MEMBERS: &[&str] = &["compress", "decompress"];
const CORE_MODULE_5_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_5_DEPENDENCIES: &[&str] = &["core.archive"];

const CORE_MODULE_6_MEMBERS: &[&str] = &["compress", "decompress"];
const CORE_MODULE_6_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_6_DEPENDENCIES: &[&str] = &["core.archive"];

const CORE_MODULE_7_MEMBERS: &[&str] = &["decode", "merge", "spec"];
const CORE_MODULE_7_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_7_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_8_MEMBERS: &[&str] = &["Auth", "AuthError", "Claims", "Session", "magic_link_consume", "magic_link_issue", "oauth_begin", "oauth_finish", "password_login", "register_user", "session_cookie", "session_id", "session_show", "session_user", "session_validate", "verify_jwt", "verify_paseto"];
const CORE_MODULE_8_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_8_DEPENDENCIES: &[&str] = &["core.crypto", "core.net"];

const CORE_MODULE_9_MEMBERS: &[&str] = &["BuildGraph", "BuildGraphAction", "BuildGraphActionKey", "BuildGraphCacheDelta", "BuildGraphDiff", "BuildGraphFile", "BuildGraphFileDelta", "BuildGraphInputDigest", "BuildGraphKeyDelta", "BuildGraphNode", "BuildGraphTarget", "graph", "receipt_diff"];
const CORE_MODULE_9_TYPES: &[(&str, CoreLeafKind)] = &[("BuildGraph", CoreLeafKind::Plain), ("BuildGraphTarget", CoreLeafKind::Plain), ("BuildGraphAction", CoreLeafKind::Plain), ("BuildGraphFile", CoreLeafKind::Plain), ("BuildGraphNode", CoreLeafKind::Plain), ("BuildGraphInputDigest", CoreLeafKind::Plain), ("BuildGraphActionKey", CoreLeafKind::Plain), ("BuildGraphFileDelta", CoreLeafKind::Plain), ("BuildGraphKeyDelta", CoreLeafKind::Plain), ("BuildGraphCacheDelta", CoreLeafKind::Plain), ("BuildGraphDiff", CoreLeafKind::Plain)];
const CORE_MODULE_9_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_10_MEMBERS: &[&str] = &["check", "lex", "lock", "manifest", "package", "parse", "profiles", "source_map"];
const CORE_MODULE_10_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_10_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_11_MEMBERS: &[&str] = &["ABI", "ArithmeticMode", "Effect", "FfiLanguage", "InlineMode", "JobScope", "KernelMode", "Layout", "Maturity", "MemoBound", "NamingCase", "ObligationMode", "Path", "PolicySetting", "Site", "State", "TaintKind", "Target", "Track"];
const CORE_MODULE_11_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_11_DEPENDENCIES: &[&str] = &["core.compiler"];

const CORE_MODULE_12_MEMBERS: &[&str] = &["ComputeDevice", "ComputeError", "ComputeStream", "SparseTensor", "Tensor", "VjpRun", "abs", "add", "broadcast_to", "deserialize", "det", "device", "device_auto", "device_cpu", "device_cuda", "device_metal", "device_vulkan", "device_webgpu", "div", "exp", "eye", "fft", "from_list", "full", "get", "gradient", "inv", "jvp", "kernel_bounds_ok", "log", "matmul", "matmul_f32_tile", "matrix", "maximum", "minimum", "mse_loss", "mul", "negate", "numel", "on_device", "ones", "placement", "profile_f32_strict", "profile_show", "rank", "reshape", "serialize", "set", "sgd_step", "shape", "solve", "sparse_mv", "sparse_nnz", "sparse_show", "sqrt", "stream_new", "stream_new_on", "stream_show", "stream_sync", "sub", "sum_axis", "to_list", "to_sparse", "transfer", "transfer_show", "transpose", "value_and_gradient", "vec", "vjp", "zeros"];
const CORE_MODULE_12_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_12_DEPENDENCIES: &[&str] = &["core.math", "core.mem"];

const CORE_MODULE_13_MEMBERS: &[&str] = &["Solver"];
const CORE_MODULE_13_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_13_DEPENDENCIES: &[&str] = &["core.compute"];

const CORE_MODULE_14_MEMBERS: &[&str] = &["CryptoError", "Digest256", "Digest512", "FileCryptoError", "Hasher", "KeyUnlock", "KeyWrapError", "PasswordHash", "Sealed", "Secret", "SharedSecret", "Signature", "SigningKey", "VerifyKey", "WrappedKey", "WrappedVaultKey", "X25519PublicKey", "X25519SecretKey", "blake3", "constant_time_equal", "constant_time_equal_bytes", "file_open", "file_seal", "hkdf_sha256", "hmac_sha256", "open", "password_hash", "password_hash_with_salt", "password_verify", "pbkdf2_hmac", "seal", "sha1", "sha224", "sha256", "sha384", "sha3_224", "sha3_256", "sha3_384", "sha3_512", "sha512", "sign", "unwrap", "verify", "wrap", "x25519", "x25519_public", "x25519_shared"];
const CORE_MODULE_14_TYPES: &[(&str, CoreLeafKind)] = &[("Secret", CoreLeafKind::CryptoNominal), ("SigningKey", CoreLeafKind::CryptoNominal), ("X25519SecretKey", CoreLeafKind::CryptoNominal), ("SharedSecret", CoreLeafKind::CryptoNominal), ("VerifyKey", CoreLeafKind::Plain), ("X25519PublicKey", CoreLeafKind::Plain), ("Signature", CoreLeafKind::Plain), ("Sealed", CoreLeafKind::Plain), ("WrappedKey", CoreLeafKind::Plain), ("WrappedVaultKey", CoreLeafKind::Plain), ("KeyUnlock", CoreLeafKind::Plain), ("PasswordHash", CoreLeafKind::Plain), ("Digest256", CoreLeafKind::Plain), ("Digest512", CoreLeafKind::Plain), ("Hasher", CoreLeafKind::Plain), ("CryptoError", CoreLeafKind::Plain), ("FileCryptoError", CoreLeafKind::Plain), ("KeyWrapError", CoreLeafKind::Plain)];
const CORE_MODULE_14_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_15_MEMBERS: &[&str] = &["aes256gcm_open", "aes256gcm_seal", "argon2id", "ed25519_sign", "ed25519_verify_strict", "hkdf_sha256_raw", "migrate_v1", "open_v1", "secret_bytes", "shared_secret_bytes", "signing_key_bytes", "x25519_raw", "x25519_secret_bytes", "xchacha20poly1305_open", "xchacha20poly1305_seal"];
const CORE_MODULE_15_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_15_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_16_MEMBERS: &[&str] = &["bytes"];
const CORE_MODULE_16_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_16_DEPENDENCIES: &[&str] = &["core.crypto"];

const CORE_MODULE_17_MEMBERS: &[&str] = &["parse", "v4", "v5", "v7"];
const CORE_MODULE_17_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_17_DEPENDENCIES: &[&str] = &["core.crypto", "core.encoding.hex"];

const CORE_MODULE_18_MEMBERS: &[&str] = &["ExpiringSecret", "KeyRef", "KeyStatus", "KeyUnlock", "KeyWrapError", "MutationPlan", "Rotation", "VaultError", "VaultWrite", "WrappedImportPlan", "WrappedVaultKey", "authorize_wrapped_import", "authorize_write", "commit_generate", "commit_import_signing", "commit_import_wrapped", "commit_import_x25519", "commit_retire", "commit_revoke", "commit_rotate", "commit_store", "current", "export_to_passphrase", "export_to_recipients", "get", "load", "prepare_generate", "prepare_import_signing", "prepare_import_wrapped", "prepare_import_x25519", "prepare_retire", "prepare_revoke", "prepare_rotate", "prepare_store", "status", "versions"];
const CORE_MODULE_18_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_18_DEPENDENCIES: &[&str] = &["core.crypto"];

const CORE_MODULE_19_MEMBERS: &[&str] = &["DataAuthority", "DataColumn", "DataError", "DataErrorKind", "DataFormat", "DataFreshness", "DataInvalidationCause", "DataLimits", "DataLineOptions", "DataLoader", "DataLoaderKind", "DataLoaderStatus", "DataPivotCell", "DataProvenance", "Query", "DataSchema", "DataSnapshot", "DataSnapshotIdentity", "DataSourceIdentity", "DataStatus", "DataStream", "DataTracked", "DataWatch", "DataWatchStatus", "Group", "track", "JetDataPlotAccessibility", "JetDataPlotAggregate", "JetDataPlotAxis", "JetDataPlotBackend", "JetDataPlotCapability", "JetDataPlotChannel", "JetDataPlotColumn", "JetDataPlotDomain", "JetDataPlotEncoding", "JetDataPlotError", "JetDataPlotErrorKind", "JetDataPlotFacet", "JetDataPlotFacetKind", "JetDataPlotField", "JetDataPlotFilterOp", "JetDataPlotInspection", "JetDataPlotInteraction", "JetDataPlotLayer", "JetDataPlotLayout", "JetDataPlotLegend", "JetDataPlotLegendPosition", "JetDataPlotMark", "JetDataPlotPlan", "JetDataPlotProjection", "JetDataPlotRender", "JetDataPlotRenderFormat", "JetDataPlotScale", "JetDataPlotScaleKind", "JetDataPlotSchema", "JetDataPlotSelectedRow", "JetDataPlotSourceFacts", "JetDataPlotSupport", "JetDataPlotTransform", "JetDataPlotValue", "bar_svg", "bar_text", "count", "csv", "csv_reader", "database", "describe", "file", "file_member", "inner_join", "inspect", "inspect_json", "json", "json_reader", "left_join", "line_svg", "line_text", "load", "load_default", "max", "mean", "median", "min", "pivot_sum", "plot", "quantile", "query", "render", "require_bridge", "rolling_mean", "schema", "show", "snapshot", "status", "stddev", "sum", "svg", "text", "url", "value", "variance"];
const CORE_MODULE_19_TYPES: &[(&str, CoreLeafKind)] = &[("Query", CoreLeafKind::Generic(1)), ("DataTracked", CoreLeafKind::Generic(2)), ("DataWatch", CoreLeafKind::Generic(1)), ("DataWatchStatus", CoreLeafKind::Plain), ("Group", CoreLeafKind::Generic(2))];
const CORE_MODULE_19_DEPENDENCIES: &[&str] = &["core.encoding", "core.files"];

const CORE_MODULE_20_MEMBERS: &[&str] = &["DataArrowBatch", "import", "query"];
const CORE_MODULE_20_TYPES: &[(&str, CoreLeafKind)] = &[("DataArrowBatch", CoreLeafKind::Generic(1))];
const CORE_MODULE_20_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_21_MEMBERS: &[&str] = &["authority", "bind", "bind_text", "cancel", "invalidate", "needs_refresh", "offline", "ready", "snapshot_reusable", "source_identity", "status", "stream"];
const CORE_MODULE_21_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_21_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_22_MEMBERS: &[&str] = &["cancel", "collect", "next"];
const CORE_MODULE_22_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_22_DEPENDENCIES: &[&str] = &["core.data.loader"];

const CORE_MODULE_23_MEMBERS: &[&str] = &["JetDataPlotAccessibility", "JetDataPlotAggregate", "JetDataPlotAxis", "JetDataPlotBackend", "JetDataPlotCapability", "JetDataPlotChannel", "JetDataPlotColumn", "JetDataPlotDomain", "JetDataPlotEncoding", "JetDataPlotError", "JetDataPlotErrorKind", "JetDataPlotFacet", "JetDataPlotFacetKind", "JetDataPlotField", "JetDataPlotFilterOp", "JetDataPlotInspection", "JetDataPlotInteraction", "JetDataPlotLayer", "JetDataPlotLayout", "JetDataPlotLegend", "JetDataPlotLegendPosition", "JetDataPlotMark", "JetDataPlotPlan", "JetDataPlotProjection", "JetDataPlotRender", "JetDataPlotRenderFormat", "JetDataPlotScale", "JetDataPlotScaleKind", "JetDataPlotSchema", "JetDataPlotSelectedRow", "JetDataPlotSourceFacts", "JetDataPlotSupport", "JetDataPlotTransform", "JetDataPlotValue", "bar_svg", "bar_text", "inspect", "inspect_json", "line_svg", "line_text", "plot", "render", "show", "svg", "text"];
const CORE_MODULE_23_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_23_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_24_MEMBERS: &[&str] = &["new"];
const CORE_MODULE_24_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_24_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_25_MEMBERS: &[&str] = &["new"];
const CORE_MODULE_25_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_25_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_26_MEMBERS: &[&str] = &["new"];
const CORE_MODULE_26_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_26_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_27_MEMBERS: &[&str] = &["new"];
const CORE_MODULE_27_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_27_DEPENDENCIES: &[&str] = &["core.data"];

const CORE_MODULE_28_MEMBERS: &[&str] = &["decode", "migrate", "open", "open_memory", "pool", "policy", "policy_audit", "row_bool", "row_float", "row_int", "row_text", "row_value", "transaction"];
const CORE_MODULE_28_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_28_DEPENDENCIES: &[&str] = &["core.files", "core.net"];

const CORE_MODULE_29_MEMBERS: &[&str] = &["Address", "Attachment", "DkimConfig", "EmailError", "Envelope", "Limits", "Mailer", "Message", "RecipientPolicy", "RecipientReport", "SMTPAuth", "SMTPConfig", "SMTPSecurity", "SendReport", "TLSTrust", "address", "attachment", "envelope", "message", "serialize", "smtp", "smtp_from_env"];
const CORE_MODULE_29_TYPES: &[(&str, CoreLeafKind)] = &[("Address", CoreLeafKind::Plain), ("Message", CoreLeafKind::Plain), ("Attachment", CoreLeafKind::Plain), ("Envelope", CoreLeafKind::Plain), ("SMTPSecurity", CoreLeafKind::Plain), ("RecipientPolicy", CoreLeafKind::Plain), ("RecipientReport", CoreLeafKind::Plain), ("SendReport", CoreLeafKind::Plain), ("EmailError", CoreLeafKind::Plain), ("Limits", CoreLeafKind::Plain), ("SMTPAuth", CoreLeafKind::Plain), ("TLSTrust", CoreLeafKind::Plain), ("DkimConfig", CoreLeafKind::Plain), ("SMTPConfig", CoreLeafKind::Plain), ("Mailer", CoreLeafKind::Plain)];
const CORE_MODULE_29_DEPENDENCIES: &[&str] = &["core.net", "core.text"];

const CORE_MODULE_30_MEMBERS: &[&str] = &["DataEvent", "DataTree", "EncodingCause", "EncodingError", "EncodingErrorKind", "EncodingFormat", "EncodingLimits", "Reader"];
const CORE_MODULE_30_TYPES: &[(&str, CoreLeafKind)] = &[("DataTree", CoreLeafKind::Plain), ("EncodingLimits", CoreLeafKind::Plain), ("EncodingError", CoreLeafKind::Plain), ("EncodingCause", CoreLeafKind::Plain), ("EncodingFormat", CoreLeafKind::Plain), ("EncodingErrorKind", CoreLeafKind::Plain), ("DataEvent", CoreLeafKind::Plain)];
const CORE_MODULE_30_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_31_MEMBERS: &[&str] = &["decode", "encode"];
const CORE_MODULE_31_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_31_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_32_MEMBERS: &[&str] = &["decode", "decode_url", "encode", "encode_url"];
const CORE_MODULE_32_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_32_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_33_MEMBERS: &[&str] = &["CBORError", "CBORErrorKind", "CBOROptions", "CBORReader", "CBORWriter", "decode", "parse", "reader", "to_bytes", "to_bytes_canonical", "writer"];
const CORE_MODULE_33_TYPES: &[(&str, CoreLeafKind)] = &[("CBORReader", CoreLeafKind::Plain), ("CBORWriter", CoreLeafKind::Plain), ("CBOROptions", CoreLeafKind::Plain), ("CBORError", CoreLeafKind::Plain), ("CBORErrorKind", CoreLeafKind::Plain)];
const CORE_MODULE_33_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_34_MEMBERS: &[&str] = &["CSVReader", "CSVRow", "CSVWriter", "decode", "parse", "query", "reader", "rows", "to_string", "writer"];
const CORE_MODULE_34_TYPES: &[(&str, CoreLeafKind)] = &[("CSVReader", CoreLeafKind::Plain), ("CSVWriter", CoreLeafKind::Plain), ("CSVRow", CoreLeafKind::Plain)];
const CORE_MODULE_34_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_35_MEMBERS: &[&str] = &["decode", "encode"];
const CORE_MODULE_35_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_35_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_36_MEMBERS: &[&str] = &["JSONReader", "JSONWriter", "canonical", "decode", "events", "parse", "reader", "to_string", "to_string_pretty", "writer"];
const CORE_MODULE_36_TYPES: &[(&str, CoreLeafKind)] = &[("JSONReader", CoreLeafKind::Plain), ("JSONWriter", CoreLeafKind::Plain)];
const CORE_MODULE_36_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_37_MEMBERS: &[&str] = &["JSONLReader", "JSONLWriter", "parse", "reader", "to_string", "writer"];
const CORE_MODULE_37_TYPES: &[(&str, CoreLeafKind)] = &[("JSONLReader", CoreLeafKind::Plain), ("JSONLWriter", CoreLeafKind::Plain)];
const CORE_MODULE_37_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_38_MEMBERS: &[&str] = &["decode", "parse", "to_string"];
const CORE_MODULE_38_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_38_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_39_MEMBERS: &[&str] = &["XMLCanonical", "XMLCanonicalMode", "XMLEncoding", "XMLEntityPolicy", "XMLError", "XMLLexicalPolicy", "XMLLimits", "XMLParseOptions", "XMLReader", "XMLReason", "XMLRenderOptions", "XMLWriter", "attribute", "canonical", "content", "decode", "decode_bytes", "expanded_name", "parse", "parse_bytes", "parse_with", "reader", "root", "to_bytes", "to_string", "writer"];
const CORE_MODULE_39_TYPES: &[(&str, CoreLeafKind)] = &[("XMLReader", CoreLeafKind::Plain), ("XMLWriter", CoreLeafKind::Plain), ("XMLError", CoreLeafKind::Plain), ("XMLReason", CoreLeafKind::Plain)];
const CORE_MODULE_39_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_40_MEMBERS: &[&str] = &["decode", "parse", "to_string"];
const CORE_MODULE_40_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_40_DEPENDENCIES: &[&str] = &["core.encoding"];

const CORE_MODULE_41_MEMBERS: &[&str] = &["async_result", "decision_hook", "hook", "new", "policy_sync", "scope", "with_policy"];
const CORE_MODULE_41_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_41_DEPENDENCIES: &[&str] = &["core.mem"];

const CORE_MODULE_42_MEMBERS: &[&str] = &["absolute", "append", "append_all", "canonicalize", "copy", "copy_dir", "create", "create_dir", "create_dir_all", "exists", "fsync", "glob", "hard_link", "is_dir", "list_dir", "lock", "map", "open", "read", "read_at", "read_bytes", "read_link", "remove", "remove_all", "remove_dir", "rename", "scope", "set_mode", "stat", "symlink", "temp_dir", "temp_file", "walk", "walk_files", "walk_parallel", "write", "write_at", "write_atomic", "write_bytes"];
const CORE_MODULE_42_TYPES: &[(&str, CoreLeafKind)] = &[("FileScope", CoreLeafKind::Plain)];
const CORE_MODULE_42_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_43_MEMBERS: &[&str] = &["FontFace", "FontStyle", "Glyph", "GlyphRun", "GlyphShaper", "shape", "system"];
const CORE_MODULE_43_TYPES: &[(&str, CoreLeafKind)] = &[("FontFace", CoreLeafKind::Plain), ("FontStyle", CoreLeafKind::Enum(&["Body", "Title", "Monospace"])), ("Glyph", CoreLeafKind::Plain), ("GlyphRun", CoreLeafKind::Plain), ("GlyphShaper", CoreLeafKind::Enum(&["HarfBuzz", "HeadlessFallback"]))];
const CORE_MODULE_43_DEPENDENCIES: &[&str] = &["core.ui"];

const CORE_MODULE_44_MEMBERS: &[&str] = &["Backend", "Replay", "Scene", "run"];
const CORE_MODULE_44_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_44_DEPENDENCIES: &[&str] = &["core.math", "core.mem"];

const CORE_MODULE_45_MEMBERS: &[&str] = &["begin_drawing", "clear_background", "close_window", "color", "draw_rectangle", "draw_sprite", "draw_text", "end_drawing", "gamepad_axis", "gamepad_down", "key_down", "load_sound", "load_texture_atlas", "play_sound", "set_target_fps", "window_open", "window_ready", "window_should_close"];
const CORE_MODULE_45_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_45_DEPENDENCIES: &[&str] = &["core.game"];

const CORE_MODULE_46_MEMBERS: &[&str] = &["get", "post", "serve"];
const CORE_MODULE_46_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_46_DEPENDENCIES: &[&str] = &["core.net", "core.text"];

const CORE_MODULE_47_MEMBERS: &[&str] = &["Client", "Proxy", "RedirectPolicy", "get", "post", "request"];
const CORE_MODULE_47_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_47_DEPENDENCIES: &[&str] = &["core.http"];

const CORE_MODULE_48_MEMBERS: &[&str] = &["access_log", "bind", "cors", "cors_policy", "json", "mux", "request_id", "response", "serve", "serve_once", "serve_once_listener", "sse", "static_file", "static_file_range", "static_files", "tls"];
const CORE_MODULE_48_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_48_DEPENDENCIES: &[&str] = &["core.http"];

const CORE_MODULE_49_MEMBERS: &[&str] = &["JobQueue", "JobPayload", "JobResult", "JobError", "JobQueueReceipt", "JobQueueClaim", "JobQueueDeliveryPolicy", "JobQueueEvent", "JobQueueRecord", "JobQueueState", "JobQueueStatus", "queue"];
const CORE_MODULE_49_TYPES: &[(&str, CoreLeafKind)] = &[("JobQueue", CoreLeafKind::Plain), ("JobPayload", CoreLeafKind::Plain), ("JobResult", CoreLeafKind::Plain), ("JobError", CoreLeafKind::Plain), ("JobQueueReceipt", CoreLeafKind::Plain), ("JobQueueClaim", CoreLeafKind::Plain), ("JobQueueDeliveryPolicy", CoreLeafKind::Enum(&["AtLeastOnce"])), ("JobQueueEvent", CoreLeafKind::Plain), ("JobQueueRecord", CoreLeafKind::Plain), ("JobQueueState", CoreLeafKind::Enum(&["Queued", "Running", "Retrying", "Completed", "Failed", "DeadLettered", "Cancelled"])), ("JobQueueStatus", CoreLeafKind::Plain)];
const CORE_MODULE_49_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_50_MEMBERS: &[&str] = &["bool", "close", "counter", "critical", "debug", "debug_fields", "disable", "enabled", "enter", "error", "error_fields", "fatal", "field", "float", "flush", "info", "info_fields", "int", "otlp_file", "redact", "sample_every", "set_level", "set_sink", "set_trace_id", "setup", "span", "warn", "warn_fields"];
const CORE_MODULE_50_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_50_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_51_MEMBERS: &[&str] = &["abs", "acos", "acosh", "asin", "asinh", "atan", "atan2", "atanh", "binomial", "cbrt", "ceil", "checked_abs", "checked_add", "checked_div", "checked_mul", "checked_neg", "checked_pow", "checked_rem", "checked_sub", "clamp", "cmp", "copy", "copysign", "cos", "cosh", "cot", "decimal", "degrees", "digits", "div_mod", "div_rem", "e", "erf", "erfc", "exp", "exp2", "exp_m1", "factorial", "floor", "fma", "fract", "fraction", "frexp", "from_bits", "gamma", "gcd", "hypot", "ilogb", "infinity", "int_pow", "inv", "is_canonical", "is_even", "is_finite", "is_inf", "is_integer", "is_nan", "is_normal", "is_odd", "is_signed", "is_subnormal", "is_zero", "isqrt", "lcm", "ldexp", "leading_ones", "lerp", "lgamma", "ln", "ln_1p", "log", "log10", "log2", "logb", "max", "min", "modf", "nan", "next_after", "next_down", "next_up", "pi", "pow", "radians", "radix", "round", "saturating_add", "saturating_mul", "saturating_sub", "scaleb", "sign", "sign_bit", "significand", "signum", "sin", "sin_cos", "sinh", "sqrt", "tan", "tanh", "tau", "to_bits", "trailing_ones", "trunc", "ulp", "zero"];
const CORE_MODULE_51_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_51_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_52_MEMBERS: &[&str] = &["bool", "bytes", "exponential", "float", "float_range", "int", "normal", "pick", "rng", "sample", "seed", "shuffle", "split", "weighted_pick"];
const CORE_MODULE_52_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_52_DEPENDENCIES: &[&str] = &["core.math"];

const CORE_MODULE_53_MEMBERS: &[&str] = &["AllocError", "Arena", "Atomic", "Bump", "Fixed", "Pin", "Pool", "Ptr", "address_of", "from_addr", "pin", "volatile_read", "volatile_write"];
const CORE_MODULE_53_TYPES: &[(&str, CoreLeafKind)] = &[("AllocError", CoreLeafKind::Plain), ("Atomic", CoreLeafKind::Plain)];
const CORE_MODULE_53_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_54_MEMBERS: &[&str] = &["guard"];
const CORE_MODULE_54_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_54_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_55_MEMBERS: &[&str] = &["load"];
const CORE_MODULE_55_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_55_DEPENDENCIES: &[&str] = &["core.compiler", "core.files"];

const CORE_MODULE_56_MEMBERS: &[&str] = &["dns_a", "dns_a_at", "dns_aaaa", "dns_aaaa_at", "dns_ptr", "dns_srv", "dns_srv_at", "dns_srv_port", "dns_srv_priority", "dns_srv_target", "dns_srv_weight", "dns_txt", "dns_txt_at", "error_address", "error_message", "error_name", "error_operation", "error_os_code", "getservbyname", "getservbyport", "ip_addr", "ip_is_ipv4", "ip_to_string", "listener_local_socket_addr", "nodelay", "ready_readable", "ready_writable", "sendfile", "set_nodelay", "set_read_timeout", "set_timeout", "set_ttl", "set_write_timeout", "socket_addr", "socket_addr_parse", "socket_host", "socket_port", "socket_to_string", "socket_type", "tcp_accept", "tcp_close", "tcp_connect", "tcp_connect_addr", "tcp_connect_happy", "tcp_connect_timeout", "tcp_listen", "tcp_listen_addr", "tcp_local_addr", "tcp_local_socket_addr", "tcp_peer_addr", "tcp_peer_socket_addr", "tcp_read", "tcp_read_bytes", "tcp_read_text", "tcp_ready", "tcp_reply", "tcp_shutdown", "tcp_write", "tcp_write_all_bytes", "tcp_write_bytes", "tcp_write_text", "tls_close", "tls_connect", "tls_read", "tls_write", "ttl", "udp_bind", "udp_bind_addr", "udp_local_addr", "udp_packet_addr", "udp_packet_bytes", "udp_packet_data", "udp_packet_original_len", "udp_packet_truncated", "udp_receive", "udp_recv_from", "udp_send_bytes_to", "udp_send_to", "udp_set_timeout", "unix_accept", "unix_close", "unix_connect", "unix_listen", "unix_read", "unix_read_bytes", "unix_shutdown", "unix_write", "unix_write_all_bytes"];
const CORE_MODULE_56_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_56_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_57_MEMBERS: &[&str] = &["extension", "from_extension", "parse"];
const CORE_MODULE_57_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_57_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_58_MEMBERS: &[&str] = &["ClientConfig", "ClientIdentity", "RootCertificates", "TLSCertificate", "TLSPeerIdentity", "TLSVersion", "client", "close", "read", "read_text", "write", "write_all", "write_text"];
const CORE_MODULE_58_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_58_DEPENDENCIES: &[&str] = &["core.crypto.random", "core.net"];

const CORE_MODULE_59_MEMBERS: &[&str] = &["data", "file", "from_parts", "parse", "percent_decode", "percent_encode", "query"];
const CORE_MODULE_59_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_59_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_60_MEMBERS: &[&str] = &["connect", "upgrade"];
const CORE_MODULE_60_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_60_DEPENDENCIES: &[&str] = &["core.net"];

const CORE_MODULE_61_MEMBERS: &[&str] = &["Perf", "default_fidelity", "fidelity", "override_fidelity", "reset_fidelity"];
const CORE_MODULE_61_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_61_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_62_MEMBERS: &[&str] = &["load"];
const CORE_MODULE_62_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_62_DEPENDENCIES: &[&str] = &["core.files", "core.process"];

const CORE_MODULE_63_MEMBERS: &[&str] = &["keep"];
const CORE_MODULE_63_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_63_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_64_MEMBERS: &[&str] = &["ProcessSignal", "args", "argv", "cmd", "exit", "on_signal", "pipeline", "run"];
const CORE_MODULE_64_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_64_DEPENDENCIES: &[&str] = &["core.args", "core.term"];

const CORE_MODULE_65_MEMBERS: &[&str] = &["computed", "derived", "effect", "signal"];
const CORE_MODULE_65_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_65_DEPENDENCIES: &[&str] = &["core.mem"];

const CORE_MODULE_66_MEMBERS: &[&str] = &["failed", "idle", "loaded", "loading"];
const CORE_MODULE_66_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_66_DEPENDENCIES: &[&str] = &["core.reactive"];

const CORE_MODULE_67_MEMBERS: &[&str] = &["of"];
const CORE_MODULE_67_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_67_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_68_MEMBERS: &[&str] = &["compile", "compile_with", "escape", "find", "find_all", "flags", "full_match", "is_match", "match", "matches", "replace", "replace_first", "split", "split_limit"];
const CORE_MODULE_68_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_68_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_69_MEMBERS: &[&str] = &["callback"];
const CORE_MODULE_69_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_69_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_70_MEMBERS: &[&str] = &["Delivery", "DeliveryEvent", "DeliveryReceipt", "DeliveryState", "ServiceDelivery", "ServiceEndpoint", "ServiceError", "ServiceRestart", "ServiceRuntime", "ServiceStateStore", "ServiceTree", "ServiceUpgradeReceipt", "ServiceWorkflow", "TaskOutcome", "TaskStatus", "delivery_at_most_once", "delivery_durable", "restart_one_for_all", "restart_one_for_one", "restart_rest_for_one", "runtime", "state_store", "tree", "tree_show", "workflow_start"];
const CORE_MODULE_70_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_70_DEPENDENCIES: &[&str] = &["core.net", "core.tasks"];

const CORE_MODULE_71_MEMBERS: &[&str] = &["RowPolicy", "SyncCounter", "SyncList", "SyncMap", "SyncText", "counter_inc", "counter_merge", "counter_new", "counter_value", "list_merge", "list_new", "list_push", "list_show", "map_get", "map_merge", "map_new", "map_set", "map_show", "policy_allows", "policy_new", "policy_show", "text_edit", "text_merge", "text_metadata", "text_new", "text_set", "text_show"];
const CORE_MODULE_71_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_71_DEPENDENCIES: &[&str] = &["core.data", "core.tasks"];

const CORE_MODULE_72_MEMBERS: &[&str] = &["arch", "atexit", "close_fd", "cpu_count", "current_dir", "decode", "executable", "exitcode", "expand", "family", "fork", "get", "getegid", "geteuid", "getgid", "getgroups", "getpgid", "getpgrp", "getpid", "getppid", "getpriority", "getsid", "getuid", "home_dir", "hostname", "initgroups", "kill", "loadavg", "mkfifo", "name", "on_interrupt", "pid", "pipe", "release", "set", "set_current_dir", "setgid", "setpgid", "setpgrp", "setpriority", "setsid", "setuid", "stop", "success", "sync", "temp_dir", "times", "umask", "unset", "uptime", "username", "utime", "vars", "version", "wait", "waitpid"];
const CORE_MODULE_72_TYPES: &[(&str, CoreLeafKind)] = &[("EnvError", CoreLeafKind::Plain)];
const CORE_MODULE_72_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_73_MEMBERS: &[&str] = &["after", "current_task", "interval", "yield_now"];
const CORE_MODULE_73_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_73_DEPENDENCIES: &[&str] = &["core.time"];

const CORE_MODULE_74_MEMBERS: &[&str] = &["Reader", "Writer", "binread", "binwrite", "buffered", "choose", "confirm", "eprint", "input", "input_secret", "print", "progress", "read_all_input", "read_key", "read_until", "readline", "stderr", "stdin", "stdout", "style", "style_force", "take", "terminal_height", "terminal_width"];
const CORE_MODULE_74_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_74_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_75_MEMBERS: &[&str] = &["assert_equal", "compare", "corpus", "fake_clock", "fake_data", "fake_rng", "fixture", "golden", "histories", "snap", "status", "temp_dir", "test_suite", "world"];
const CORE_MODULE_75_TYPES: &[(&str, CoreLeafKind)] = &[("Count", CoreLeafKind::Plain), ("DeterministicWorld", CoreLeafKind::Plain), ("EventId", CoreLeafKind::Plain), ("HandleId", CoreLeafKind::Plain), ("HistoryBounds", CoreLeafKind::Plain), ("HistoryCase", CoreLeafKind::Plain), ("HistoryDistribution", CoreLeafKind::Plain), ("HistoryOperation", CoreLeafKind::Plain), ("HistoryPrecondition", CoreLeafKind::Plain), ("HistoryRng", CoreLeafKind::Plain), ("HistoryScheduleChoice", CoreLeafKind::Plain), ("HistoryStrategy", CoreLeafKind::Generic(1)), ("HistoryValue", CoreLeafKind::Plain), ("TaskId", CoreLeafKind::Plain), ("TypedHistoryCase", CoreLeafKind::Generic(1)), ("TestComparison", CoreLeafKind::Plain)];
const CORE_MODULE_75_DEPENDENCIES: &[&str] = &["core.files", "core.math.random", "core.time"];

const CORE_MODULE_76_MEMBERS: &[&str] = &["Cursor", "byte_count", "byte_views", "casefold", "caseless_eq", "center", "char_indices", "display_width", "ends_any", "grapheme_views", "graphemes", "inspect", "is_alphabetic", "is_ascii", "is_numeric", "is_whitespace", "line_views", "lower", "nfc", "nfd", "nfkc", "nfkd", "pad_end", "pad_start", "rsplitn", "scalar_count", "scalars", "sentences", "splitn", "starts_any", "trim", "trim_end", "trim_start", "upper", "word_views", "words"];
const CORE_MODULE_76_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_76_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_77_MEMBERS: &[&str] = &["bin", "bytes", "decimal", "duration", "grouped", "hex", "number", "oct", "ordinal", "pad", "pad_center", "pad_left", "pad_right", "percent", "plural", "pretty", "sci"];
const CORE_MODULE_77_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_77_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_78_MEMBERS: &[&str] = &["datetime", "days_in_month", "from_iso_week", "from_timestamp", "from_unix_microseconds", "from_unix_ms", "from_unix_nanoseconds", "from_unix_seconds", "instant", "is_leap_year", "local_time", "new", "now", "now_utc", "parse", "parse_iso_week_date", "parse_rfc3339", "parse_time", "parse_zoned", "period", "period_days", "period_months", "period_years", "sleep", "sleep_until", "start", "time", "today", "utc", "zone", "zoned", "zoned_local"];
const CORE_MODULE_78_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_78_DEPENDENCIES: &[&str] = &["core"];

const CORE_MODULE_79_MEMBERS: &[&str] = &[];
const CORE_MODULE_79_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_79_DEPENDENCIES: &[&str] = &[];

const CORE_MODULE_80_MEMBERS: &[&str] = &["aria_role_button", "aria_role_container", "aria_role_label", "aria_role_text_input", "box", "button", "constraint", "desktop", "gtk_backend", "key_event", "mount", "node", "node_accessibility", "node_color", "node_role", "node_shortcut", "null_backend", "phone", "point", "playground", "playgrounds", "preview", "previews", "reactive_render", "rect", "resize_event", "size", "tablet", "text", "text_input", "tui_backend"];
const CORE_MODULE_80_TYPES: &[(&str, CoreLeafKind)] = &[("UiImeMode", CoreLeafKind::Enum(&["Native", "Disabled"])), ("UiPlayground", CoreLeafKind::Plain), ("UiPreview", CoreLeafKind::Plain), ("UiPreviewAccessibility", CoreLeafKind::Plain), ("UiPreviewAuthority", CoreLeafKind::Plain), ("UiPreviewContext", CoreLeafKind::Plain), ("UiPreviewDevice", CoreLeafKind::Enum(&["Phone", "Tablet", "Desktop"])), ("UiPreviewEffect", CoreLeafKind::Plain), ("UiPreviewInputOverride", CoreLeafKind::Plain), ("UiPreviewInputValue", CoreLeafKind::Enum(&["Text", "Bool", "Integer", "Float"])), ("UiPreviewKind", CoreLeafKind::Enum(&["Preview", "Playground"])), ("UiPreviewLifecycle", CoreLeafKind::Plain), ("UiPreviewRegistry", CoreLeafKind::Plain), ("UiPreviewSource", CoreLeafKind::Plain), ("UiPreviewTheme", CoreLeafKind::Plain), ("UiPreviewTraits", CoreLeafKind::Plain), ("UiPreviewViewport", CoreLeafKind::Plain)];
const CORE_MODULE_80_DEPENDENCIES: &[&str] = &["core.text"];

const CORE_MODULE_81_MEMBERS: &[&str] = &["TuiCapabilities", "TuiColor", "TuiColorProfile", "TuiConstraint", "TuiDirection", "TuiEvent", "TuiListState", "TuiStyle", "ascii", "capabilities", "close_event", "color_ansi16", "color_ansi256", "color_rgb", "display_width", "fill", "focus_event", "horizontal", "interrupt_event", "io_event", "key_event", "key_event_modifiers", "layout", "length", "list", "list_state", "list_state_offset", "list_state_select", "list_state_selected", "max", "min", "percent", "resize_event", "style", "style_background", "style_bold", "style_dim", "style_foreground", "style_text", "style_underline", "table", "timer_event", "vertical"];
const CORE_MODULE_81_TYPES: &[(&str, CoreLeafKind)] = &[("TuiEvent", CoreLeafKind::Enum(&["Key", "Resize", "Timer", "Io", "Focus", "Interrupt", "Close"])), ("TuiColorProfile", CoreLeafKind::Enum(&["Ansi16", "Ansi256", "TrueColor", "Ascii"])), ("TuiColor", CoreLeafKind::Enum(&["Ansi16", "Ansi256", "Rgb"])), ("TuiCapabilities", CoreLeafKind::Plain), ("TuiStyle", CoreLeafKind::Plain), ("TuiConstraint", CoreLeafKind::Enum(&["Length", "Min", "Max", "Percent", "Fill"])), ("TuiDirection", CoreLeafKind::Enum(&["Horizontal", "Vertical"])), ("TuiListState", CoreLeafKind::Plain)];
const CORE_MODULE_81_DEPENDENCIES: &[&str] = &["core.ui", "core.text"];

const CORE_MODULE_82_MEMBERS: &[&str] = &["accessibility", "capabilities", "file_filter", "file_filter_text", "fs_grant", "fs_rights_read", "fs_rights_read_write", "fs_rights_write", "open_file", "open_request", "save_file", "save_request", "shortcut"];
const CORE_MODULE_82_TYPES: &[(&str, CoreLeafKind)] = &[("UiCapability", CoreLeafKind::Enum(&["FileDialog", "Clipboard", "Ime", "DragDrop", "Shortcuts", "Accessibility"])), ("UiCapabilityFact", CoreLeafKind::Plain), ("UiCapabilityFacts", CoreLeafKind::Plain), ("UiCancellation", CoreLeafKind::Enum(&["User", "Closed", "Headless", "Superseded", "Programmatic"])), ("UiHostError", CoreLeafKind::Plain), ("UiServiceResult", CoreLeafKind::Plain), ("UiFileDialogKind", CoreLeafKind::Enum(&["Open", "Save"])), ("UiFsAccess", CoreLeafKind::Enum(&["Read", "Write"])), ("UiFsRights", CoreLeafKind::Plain), ("UiFsGrant", CoreLeafKind::Plain), ("UiGrantedPath", CoreLeafKind::Plain), ("UiFileFilter", CoreLeafKind::Plain), ("UiFileDialogRequest", CoreLeafKind::Plain), ("UiFileDialogSelection", CoreLeafKind::Plain), ("UiClipboardText", CoreLeafKind::Plain), ("UiClipboardWrite", CoreLeafKind::Plain), ("UiTextRange", CoreLeafKind::Plain), ("UiImePhase", CoreLeafKind::Enum(&["Start", "Update", "Commit", "Cancel"])), ("UiImeComposition", CoreLeafKind::Plain), ("UiImeEvent", CoreLeafKind::Plain), ("UiDragOperation", CoreLeafKind::Enum(&["Copy", "Move", "Link"])), ("UiDropItem", CoreLeafKind::Plain), ("UiDragPhase", CoreLeafKind::Enum(&["Enter", "Over", "Drop", "Leave", "Cancel"])), ("UiDragEvent", CoreLeafKind::Plain), ("UiShortcutModifier", CoreLeafKind::Enum(&["Control", "Alt", "Shift", "Meta"])), ("UiShortcutModifiers", CoreLeafKind::Plain), ("UiShortcut", CoreLeafKind::Plain), ("UiShortcutBinding", CoreLeafKind::Plain), ("UiShortcutDispatch", CoreLeafKind::Plain), ("UiAccessibilityState", CoreLeafKind::Plain), ("UiAccessibility", CoreLeafKind::Plain), ("UiNodeId", CoreLeafKind::Plain), ("UiAccessibilityProjection", CoreLeafKind::Plain), ("UiFileFilterResult", CoreLeafKind::Plain), ("UiFsGrantResult", CoreLeafKind::Plain), ("UiShortcutResult", CoreLeafKind::Plain), ("UiShortcutBindingResult", CoreLeafKind::Plain), ("UiAccessibilityResult", CoreLeafKind::Plain), ("UiFileDialogResult", CoreLeafKind::Plain), ("UiClipboardTextResult", CoreLeafKind::Plain), ("UiClipboardWriteResult", CoreLeafKind::Plain), ("UiImeResult", CoreLeafKind::Plain), ("UiDragResult", CoreLeafKind::Plain), ("UiShortcutDispatchResult", CoreLeafKind::Plain), ("UiAccessibilityNodeResult", CoreLeafKind::Plain), ("UiAccessibilityAttachResult", CoreLeafKind::Plain), ("UiAccessibilityProjectionResult", CoreLeafKind::Plain)];
const CORE_MODULE_82_DEPENDENCIES: &[&str] = &["core.ui", "core.files"];

const CORE_MODULE_83_MEMBERS: &[&str] = &["read_text", "write_text"];
const CORE_MODULE_83_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_83_DEPENDENCIES: &[&str] = &["core.ui.host"];

const CORE_MODULE_84_MEMBERS: &[&str] = &["poll"];
const CORE_MODULE_84_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_84_DEPENDENCIES: &[&str] = &["core.ui.host"];

const CORE_MODULE_85_MEMBERS: &[&str] = &["poll"];
const CORE_MODULE_85_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_85_DEPENDENCIES: &[&str] = &["core.ui.host"];

const CORE_MODULE_86_MEMBERS: &[&str] = &["binding", "dispatch", "register"];
const CORE_MODULE_86_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_86_DEPENDENCIES: &[&str] = &["core.ui.host"];

const CORE_MODULE_87_MEMBERS: &[&str] = &["attach", "project"];
const CORE_MODULE_87_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_87_DEPENDENCIES: &[&str] = &["core.ui.host"];

const CORE_MODULE_88_MEMBERS: &[&str] = &["from"];
const CORE_MODULE_88_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_88_DEPENDENCIES: &[&str] = &["core.math"];

const CORE_MODULE_89_MEMBERS: &[&str] = &["files", "port", "process_pid", "set"];
const CORE_MODULE_89_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_89_DEPENDENCIES: &[&str] = &["core.files", "core.process"];

const CORE_MODULE_90_MEMBERS: &[&str] = &["App", "Auth", "Context", "LiveQuery", "Mount", "Page", "Session", "app", "auth", "auth_oauth", "auth_routes", "auth_show", "form", "invalidate", "live", "live_get", "live_show", "live_stats", "on", "openapi", "page", "signal_push", "storage", "subscribe", "sync", "transact_invalidate", "value"];
const CORE_MODULE_90_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_90_DEPENDENCIES: &[&str] = &["core.http"];

const CORE_MODULE_91_MEMBERS: &[&str] = &["Browser", "BrowserAbilities", "BrowserContext", "BrowserError", "BrowserEvent", "BrowserFrame", "BrowserIntercept", "BrowserLocator", "BrowserLocked", "BrowserPage", "BrowserPrivacy", "BrowserProfile", "BrowserProtocol", "BrowserReceipt", "BrowserTimeout", "BrowserTrace", "begin_named", "config", "config_from_env", "connect", "connect_profile", "fixture_context", "fixture_page", "fixture_source", "generate_source", "locked", "profile", "report_add_case", "report_exit_code", "report_html", "report_json", "report_new", "report_text", "selected", "server_logs", "server_start", "server_stop", "server_url", "timeout", "watch_changed", "write_report"];
const CORE_MODULE_91_TYPES: &[(&str, CoreLeafKind)] = &[("BrowserTestConfig", CoreLeafKind::Plain), ("BrowserTestSource", CoreLeafKind::Plain), ("BrowserTestAction", CoreLeafKind::Plain), ("BrowserTestSnapshot", CoreLeafKind::Plain), ("BrowserTestEventFact", CoreLeafKind::Plain), ("BrowserTestArtifact", CoreLeafKind::Plain), ("BrowserTestAttempt", CoreLeafKind::Plain), ("BrowserTestCase", CoreLeafKind::Plain), ("BrowserTestReport", CoreLeafKind::Plain), ("BrowserTestFixture", CoreLeafKind::Plain), ("BrowserTestServer", CoreLeafKind::Plain)];
const CORE_MODULE_91_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_92_MEMBERS: &[&str] = &["app", "for_app"];
const CORE_MODULE_92_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_92_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_93_MEMBERS: &[&str] = &["action_error", "action_field_error", "action_form_error", "blur", "field", "html", "input", "input_exclude", "input_group", "input_rename", "input_replace", "new", "no_script", "set", "show", "submit", "typed", "typed_blur", "typed_cancel", "typed_decode_post", "typed_errors", "typed_focus", "typed_html", "typed_lifecycle", "typed_no_script", "typed_post", "typed_select_field", "typed_set", "typed_set_async_validator", "typed_set_action", "typed_show", "typed_state", "typed_submit", "typed_submit_async", "typed_validate", "typed_validate_async", "typed_validate_field", "typed_validation_render", "typed_submission_cancel", "typed_submission_wait", "typed_validation_cancel", "typed_validation_wait", "validate", "validate_async"];
const CORE_MODULE_93_TYPES: &[(&str, CoreLeafKind)] = &[("WebFormValueType", CoreLeafKind::Enum(&["String", "Int", "Bool", "Float"])), ("WebFormStatus", CoreLeafKind::Enum(&["Idle", "Dirty", "Validating", "Invalid", "Submitting", "Submitted", "Error"])), ("WebFormControl", CoreLeafKind::Enum(&["Text", "Email", "Url", "Password", "Number", "Date", "Checkbox", "Hidden"])), ("WebFormValidationTiming", CoreLeafKind::Enum(&["Change", "Blur", "Submit"])), ("WebFormFieldSpec", CoreLeafKind::Plain), ("WebFormInput", CoreLeafKind::Plain), ("WebFormDecodedInput", CoreLeafKind::Plain), ("WebFormActionError", CoreLeafKind::Plain), ("WebFormErrorState", CoreLeafKind::Plain), ("WebFormLifecycleStatus", CoreLeafKind::Enum(&["Idle", "Pending", "Submitting", "Success", "Failure", "Cancelled"])), ("WebFormLifecycle", CoreLeafKind::Plain), ("WebFormTyped", CoreLeafKind::Plain), ("WebFormValidationChain", CoreLeafKind::Plain), ("WebFormTypedValidation", CoreLeafKind::Plain), ("WebFormTypedSubmission", CoreLeafKind::Plain)];
const CORE_MODULE_93_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_94_MEMBERS: &[&str] = &["cancel", "facts", "get", "invalidate", "live", "mutate", "mutate_with_invalidations", "mutation_state", "mutation_signal", "new", "queue", "refresh", "retry", "set_mode", "set_online", "show", "state", "state_signal", "subscribe"];
const CORE_MODULE_94_TYPES: &[(&str, CoreLeafKind)] = &[("WebMutationState", CoreLeafKind::Plain), ("WebMutationStatus", CoreLeafKind::Enum(&["Idle", "Pending", "Success", "Error", "Settled"])), ("WebQueryStatus", CoreLeafKind::Enum(&["Pending", "Fresh", "Stale", "Fetching", "Error", "Offline"])), ("WebQueryNetworkMode", CoreLeafKind::Enum(&["Online", "Always", "OfflineFirst"]))];
const CORE_MODULE_94_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_95_MEMBERS: &[&str] = &["abort", "cache_show", "cache_state", "collect", "current", "invalidate", "link", "navigate", "new", "not_found", "preload", "route", "route_with_search_codec", "show", "stale"];
const CORE_MODULE_95_TYPES: &[(&str, CoreLeafKind)] = &[("WebRouterValueType", CoreLeafKind::Enum(&["String", "Int", "Bool", "Float", "JSON"])), ("WebRouterSearchCodec", CoreLeafKind::Enum(&["Query", "JSON"])), ("WebRouterCacheStatus", CoreLeafKind::Enum(&["Fresh", "Stale", "Invalidated", "Collected"])), ("WebNavigationStatus", CoreLeafKind::Enum(&["Idle", "Preloading", "Pending", "Ready", "Error", "Aborted"]))];
const CORE_MODULE_95_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_96_MEMBERS: &[&str] = &["local", "session"];
const CORE_MODULE_96_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_96_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_97_MEMBERS: &[&str] = &["clear", "get", "remove", "set"];
const CORE_MODULE_97_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_97_DEPENDENCIES: &[&str] = &["core.web.storage"];

const CORE_MODULE_98_MEMBERS: &[&str] = &["clear", "get", "remove", "set"];
const CORE_MODULE_98_TYPES: &[(&str, CoreLeafKind)] = &[];
const CORE_MODULE_98_DEPENDENCIES: &[&str] = &["core.web.storage"];

const CORE_MODULE_99_MEMBERS: &[&str] = &["back", "batch", "clear_history", "current_generation", "cursor", "derived", "event_json", "events", "events_since", "facts_json", "forward", "history", "history_at", "history_enabled", "history_limit", "inspect", "jump", "new", "optimistic", "patch", "patch_active", "patch_commit", "patch_generation", "patch_rollback", "patch_transaction", "restore", "scrub", "selector", "set", "set_history_limit", "set_state", "signal", "state_signal", "subscribe", "subscribe_selector", "subscription_active", "subscription_unsubscribe", "transaction", "update", "value", "with_history"];
const CORE_MODULE_99_TYPES: &[(&str, CoreLeafKind)] = &[("WebStoreTransaction", CoreLeafKind::Plain), ("WebStoreEvent", CoreLeafKind::Plain), ("WebStoreInspection", CoreLeafKind::Plain), ("WebStorePatch", CoreLeafKind::Plain), ("WebStore", CoreLeafKind::Plain), ("WebStoreSubscription", CoreLeafKind::Plain)];
const CORE_MODULE_99_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_100_MEMBERS: &[&str] = &["clear_focus", "clear_selection", "column", "facts", "filter", "filter_by", "first_page", "focus", "focused_key", "insert_row", "keys", "last_page", "new", "new_keyed", "next_page", "page", "page_state", "paginate", "remove_row", "replace_row", "selected_keys", "selected_rows", "set_rows", "set_selected", "sort", "sort_by", "state", "toggle_selection", "update_row", "visible_rows", "with_column", "with_server_page"];
const CORE_MODULE_100_TYPES: &[(&str, CoreLeafKind)] = &[("WebTableSortDirection", CoreLeafKind::Enum(&["Ascending", "Descending"])), ("WebTablePageMode", CoreLeafKind::Enum(&["Client", "Server"])), ("WebTableSort", CoreLeafKind::Plain), ("WebTableFilter", CoreLeafKind::Plain), ("WebTableState", CoreLeafKind::Plain), ("WebTableColumn", CoreLeafKind::Generic(1)), ("WebTablePage", CoreLeafKind::Generic(1)), ("WebTableRow", CoreLeafKind::Generic(1)), ("WebTable", CoreLeafKind::Generic(1))];
const CORE_MODULE_100_DEPENDENCIES: &[&str] = &["core.web"];

const CORE_MODULE_101_MEMBERS: &[&str] = &["indices", "plan", "plan_from_sizes", "plan_facts", "plan_indices", "plan_measure", "plan_measured", "plan_resize", "plan_scroll_to", "plan_slice", "plan_viewport", "plan_viewport_measure", "plan_viewport_state", "slice", "window", "window_measured"];
const CORE_MODULE_101_TYPES: &[(&str, CoreLeafKind)] = &[("WebVirtualWindow", CoreLeafKind::Plain), ("WebVirtualPlan", CoreLeafKind::Plain), ("WebVirtualPlanViewport", CoreLeafKind::Plain)];
const CORE_MODULE_101_DEPENDENCIES: &[&str] = &["core.web"];

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
    CoreModuleDeclaration { module: "core.compute", members: CORE_MODULE_12_MEMBERS, type_exports: CORE_MODULE_12_TYPES, dependencies: CORE_MODULE_12_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.compute.solve", members: CORE_MODULE_13_MEMBERS, type_exports: CORE_MODULE_13_TYPES, dependencies: CORE_MODULE_13_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.crypto", members: CORE_MODULE_14_MEMBERS, type_exports: CORE_MODULE_14_TYPES, dependencies: CORE_MODULE_14_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.crypto.expert", members: CORE_MODULE_15_MEMBERS, type_exports: CORE_MODULE_15_TYPES, dependencies: CORE_MODULE_15_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.crypto.random", members: CORE_MODULE_16_MEMBERS, type_exports: CORE_MODULE_16_TYPES, dependencies: CORE_MODULE_16_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.crypto.uuid", members: CORE_MODULE_17_MEMBERS, type_exports: CORE_MODULE_17_TYPES, dependencies: CORE_MODULE_17_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.crypto.vault", members: CORE_MODULE_18_MEMBERS, type_exports: CORE_MODULE_18_TYPES, dependencies: CORE_MODULE_18_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data", members: CORE_MODULE_19_MEMBERS, type_exports: CORE_MODULE_19_TYPES, dependencies: CORE_MODULE_19_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.arrow", members: CORE_MODULE_20_MEMBERS, type_exports: CORE_MODULE_20_TYPES, dependencies: CORE_MODULE_20_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.loader", members: CORE_MODULE_21_MEMBERS, type_exports: CORE_MODULE_21_TYPES, dependencies: CORE_MODULE_21_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.stream", members: CORE_MODULE_22_MEMBERS, type_exports: CORE_MODULE_22_TYPES, dependencies: CORE_MODULE_22_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.plot", members: CORE_MODULE_23_MEMBERS, type_exports: CORE_MODULE_23_TYPES, dependencies: CORE_MODULE_23_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.sketch.cms", members: CORE_MODULE_24_MEMBERS, type_exports: CORE_MODULE_24_TYPES, dependencies: CORE_MODULE_24_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.sketch.hll", members: CORE_MODULE_25_MEMBERS, type_exports: CORE_MODULE_25_TYPES, dependencies: CORE_MODULE_25_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.sketch.reservoir", members: CORE_MODULE_26_MEMBERS, type_exports: CORE_MODULE_26_TYPES, dependencies: CORE_MODULE_26_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.data.sketch.tdigest", members: CORE_MODULE_27_MEMBERS, type_exports: CORE_MODULE_27_TYPES, dependencies: CORE_MODULE_27_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.db", members: CORE_MODULE_28_MEMBERS, type_exports: CORE_MODULE_28_TYPES, dependencies: CORE_MODULE_28_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.email", members: CORE_MODULE_29_MEMBERS, type_exports: CORE_MODULE_29_TYPES, dependencies: CORE_MODULE_29_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding", members: CORE_MODULE_30_MEMBERS, type_exports: CORE_MODULE_30_TYPES, dependencies: CORE_MODULE_30_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.base32", members: CORE_MODULE_31_MEMBERS, type_exports: CORE_MODULE_31_TYPES, dependencies: CORE_MODULE_31_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.base64", members: CORE_MODULE_32_MEMBERS, type_exports: CORE_MODULE_32_TYPES, dependencies: CORE_MODULE_32_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.cbor", members: CORE_MODULE_33_MEMBERS, type_exports: CORE_MODULE_33_TYPES, dependencies: CORE_MODULE_33_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.csv", members: CORE_MODULE_34_MEMBERS, type_exports: CORE_MODULE_34_TYPES, dependencies: CORE_MODULE_34_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.hex", members: CORE_MODULE_35_MEMBERS, type_exports: CORE_MODULE_35_TYPES, dependencies: CORE_MODULE_35_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.json", members: CORE_MODULE_36_MEMBERS, type_exports: CORE_MODULE_36_TYPES, dependencies: CORE_MODULE_36_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.jsonl", members: CORE_MODULE_37_MEMBERS, type_exports: CORE_MODULE_37_TYPES, dependencies: CORE_MODULE_37_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.toml", members: CORE_MODULE_38_MEMBERS, type_exports: CORE_MODULE_38_TYPES, dependencies: CORE_MODULE_38_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.xml", members: CORE_MODULE_39_MEMBERS, type_exports: CORE_MODULE_39_TYPES, dependencies: CORE_MODULE_39_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.encoding.yaml", members: CORE_MODULE_40_MEMBERS, type_exports: CORE_MODULE_40_TYPES, dependencies: CORE_MODULE_40_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.event", members: CORE_MODULE_41_MEMBERS, type_exports: CORE_MODULE_41_TYPES, dependencies: CORE_MODULE_41_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.files", members: CORE_MODULE_42_MEMBERS, type_exports: CORE_MODULE_42_TYPES, dependencies: CORE_MODULE_42_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.font", members: CORE_MODULE_43_MEMBERS, type_exports: CORE_MODULE_43_TYPES, dependencies: CORE_MODULE_43_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.game", members: CORE_MODULE_44_MEMBERS, type_exports: CORE_MODULE_44_TYPES, dependencies: CORE_MODULE_44_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.game.raylib", members: CORE_MODULE_45_MEMBERS, type_exports: CORE_MODULE_45_TYPES, dependencies: CORE_MODULE_45_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.http", members: CORE_MODULE_46_MEMBERS, type_exports: CORE_MODULE_46_TYPES, dependencies: CORE_MODULE_46_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.http.client", members: CORE_MODULE_47_MEMBERS, type_exports: CORE_MODULE_47_TYPES, dependencies: CORE_MODULE_47_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.http.server", members: CORE_MODULE_48_MEMBERS, type_exports: CORE_MODULE_48_TYPES, dependencies: CORE_MODULE_48_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.jobs", members: CORE_MODULE_49_MEMBERS, type_exports: CORE_MODULE_49_TYPES, dependencies: CORE_MODULE_49_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.log", members: CORE_MODULE_50_MEMBERS, type_exports: CORE_MODULE_50_TYPES, dependencies: CORE_MODULE_50_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.math", members: CORE_MODULE_51_MEMBERS, type_exports: CORE_MODULE_51_TYPES, dependencies: CORE_MODULE_51_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.math.random", members: CORE_MODULE_52_MEMBERS, type_exports: CORE_MODULE_52_TYPES, dependencies: CORE_MODULE_52_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.mem", members: CORE_MODULE_53_MEMBERS, type_exports: CORE_MODULE_53_TYPES, dependencies: CORE_MODULE_53_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.mem.scope", members: CORE_MODULE_54_MEMBERS, type_exports: CORE_MODULE_54_TYPES, dependencies: CORE_MODULE_54_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.mod", members: CORE_MODULE_55_MEMBERS, type_exports: CORE_MODULE_55_TYPES, dependencies: CORE_MODULE_55_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.net", members: CORE_MODULE_56_MEMBERS, type_exports: CORE_MODULE_56_TYPES, dependencies: CORE_MODULE_56_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.net.mime", members: CORE_MODULE_57_MEMBERS, type_exports: CORE_MODULE_57_TYPES, dependencies: CORE_MODULE_57_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.net.tls", members: CORE_MODULE_58_MEMBERS, type_exports: CORE_MODULE_58_TYPES, dependencies: CORE_MODULE_58_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.net.url", members: CORE_MODULE_59_MEMBERS, type_exports: CORE_MODULE_59_TYPES, dependencies: CORE_MODULE_59_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.net.ws", members: CORE_MODULE_60_MEMBERS, type_exports: CORE_MODULE_60_TYPES, dependencies: CORE_MODULE_60_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.perf", members: CORE_MODULE_61_MEMBERS, type_exports: CORE_MODULE_61_TYPES, dependencies: CORE_MODULE_61_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.plugin", members: CORE_MODULE_62_MEMBERS, type_exports: CORE_MODULE_62_TYPES, dependencies: CORE_MODULE_62_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.prelude", members: CORE_MODULE_63_MEMBERS, type_exports: CORE_MODULE_63_TYPES, dependencies: CORE_MODULE_63_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.process", members: CORE_MODULE_64_MEMBERS, type_exports: CORE_MODULE_64_TYPES, dependencies: CORE_MODULE_64_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.reactive", members: CORE_MODULE_65_MEMBERS, type_exports: CORE_MODULE_65_TYPES, dependencies: CORE_MODULE_65_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.reactive.loadable", members: CORE_MODULE_66_MEMBERS, type_exports: CORE_MODULE_66_TYPES, dependencies: CORE_MODULE_66_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.reflect", members: CORE_MODULE_67_MEMBERS, type_exports: CORE_MODULE_67_TYPES, dependencies: CORE_MODULE_67_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.regex", members: CORE_MODULE_68_MEMBERS, type_exports: CORE_MODULE_68_TYPES, dependencies: CORE_MODULE_68_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.rt", members: CORE_MODULE_69_MEMBERS, type_exports: CORE_MODULE_69_TYPES, dependencies: CORE_MODULE_69_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.service", members: CORE_MODULE_70_MEMBERS, type_exports: CORE_MODULE_70_TYPES, dependencies: CORE_MODULE_70_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.sync", members: CORE_MODULE_71_MEMBERS, type_exports: CORE_MODULE_71_TYPES, dependencies: CORE_MODULE_71_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.sys", members: CORE_MODULE_72_MEMBERS, type_exports: CORE_MODULE_72_TYPES, dependencies: CORE_MODULE_72_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.tasks", members: CORE_MODULE_73_MEMBERS, type_exports: CORE_MODULE_73_TYPES, dependencies: CORE_MODULE_73_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.term", members: CORE_MODULE_74_MEMBERS, type_exports: CORE_MODULE_74_TYPES, dependencies: CORE_MODULE_74_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.testing", members: CORE_MODULE_75_MEMBERS, type_exports: CORE_MODULE_75_TYPES, dependencies: CORE_MODULE_75_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.text", members: CORE_MODULE_76_MEMBERS, type_exports: CORE_MODULE_76_TYPES, dependencies: CORE_MODULE_76_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.text.fmt", members: CORE_MODULE_77_MEMBERS, type_exports: CORE_MODULE_77_TYPES, dependencies: CORE_MODULE_77_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.time", members: CORE_MODULE_78_MEMBERS, type_exports: CORE_MODULE_78_TYPES, dependencies: CORE_MODULE_78_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.time.expiring", members: CORE_MODULE_79_MEMBERS, type_exports: CORE_MODULE_79_TYPES, dependencies: CORE_MODULE_79_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui", members: CORE_MODULE_80_MEMBERS, type_exports: CORE_MODULE_80_TYPES, dependencies: CORE_MODULE_80_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.tui", members: CORE_MODULE_81_MEMBERS, type_exports: CORE_MODULE_81_TYPES, dependencies: CORE_MODULE_81_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host", members: CORE_MODULE_82_MEMBERS, type_exports: CORE_MODULE_82_TYPES, dependencies: CORE_MODULE_82_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host.clipboard", members: CORE_MODULE_83_MEMBERS, type_exports: CORE_MODULE_83_TYPES, dependencies: CORE_MODULE_83_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host.ime", members: CORE_MODULE_84_MEMBERS, type_exports: CORE_MODULE_84_TYPES, dependencies: CORE_MODULE_84_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host.drag_drop", members: CORE_MODULE_85_MEMBERS, type_exports: CORE_MODULE_85_TYPES, dependencies: CORE_MODULE_85_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host.shortcuts", members: CORE_MODULE_86_MEMBERS, type_exports: CORE_MODULE_86_TYPES, dependencies: CORE_MODULE_86_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.ui.host.accessibility", members: CORE_MODULE_87_MEMBERS, type_exports: CORE_MODULE_87_TYPES, dependencies: CORE_MODULE_87_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.units", members: CORE_MODULE_88_MEMBERS, type_exports: CORE_MODULE_88_TYPES, dependencies: CORE_MODULE_88_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.watcher", members: CORE_MODULE_89_MEMBERS, type_exports: CORE_MODULE_89_TYPES, dependencies: CORE_MODULE_89_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web", members: CORE_MODULE_90_MEMBERS, type_exports: CORE_MODULE_90_TYPES, dependencies: CORE_MODULE_90_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.browser", members: CORE_MODULE_91_MEMBERS, type_exports: CORE_MODULE_91_TYPES, dependencies: CORE_MODULE_91_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.devserver", members: CORE_MODULE_92_MEMBERS, type_exports: CORE_MODULE_92_TYPES, dependencies: CORE_MODULE_92_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.forms", members: CORE_MODULE_93_MEMBERS, type_exports: CORE_MODULE_93_TYPES, dependencies: CORE_MODULE_93_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.query", members: CORE_MODULE_94_MEMBERS, type_exports: CORE_MODULE_94_TYPES, dependencies: CORE_MODULE_94_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.router", members: CORE_MODULE_95_MEMBERS, type_exports: CORE_MODULE_95_TYPES, dependencies: CORE_MODULE_95_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.storage", members: CORE_MODULE_96_MEMBERS, type_exports: CORE_MODULE_96_TYPES, dependencies: CORE_MODULE_96_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.storage.local", members: CORE_MODULE_97_MEMBERS, type_exports: CORE_MODULE_97_TYPES, dependencies: CORE_MODULE_97_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.storage.session", members: CORE_MODULE_98_MEMBERS, type_exports: CORE_MODULE_98_TYPES, dependencies: CORE_MODULE_98_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.store", members: CORE_MODULE_99_MEMBERS, type_exports: CORE_MODULE_99_TYPES, dependencies: CORE_MODULE_99_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.table", members: CORE_MODULE_100_MEMBERS, type_exports: CORE_MODULE_100_TYPES, dependencies: CORE_MODULE_100_DEPENDENCIES },
    CoreModuleDeclaration { module: "core.web.virtual", members: CORE_MODULE_101_MEMBERS, type_exports: CORE_MODULE_101_TYPES, dependencies: CORE_MODULE_101_DEPENDENCIES },
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
