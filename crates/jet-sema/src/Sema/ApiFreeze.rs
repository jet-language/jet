//! S2/D-MEM1 (was c129/D-CAP4/D-CAP6/D-CAP8): snapshot a library target's
//! public-fn surface into durable interface metadata, unconditionally on
//! every publish — pub-metadata semver diffing (feeds E1218/E2601). The
//! former `api: stable|explicit` opt-in gate and the capability-tier
//! freeze/drift check (E0912, elevation-based) are retired: a param's sigil
//! is decided at parse time and never drifts silently, so there is nothing
//! left to freeze at that tier — only an ordinary public-signature diff.
//!
//! Format (std-only, lockfile-style — no serde, I6):
//!
//! ```text
//! api_version = 3
//! package = mathkit
//! published_version = 1.2.0
//! fn scale(v: &Vec3, factor: Float)
//! fn length(v: Vec3) -> Float
//! ```
//!
//! Each `fn` line is the canonical signature: every public function's
//! parameter list carries its D-MEM1 sigil (`&`/`^`/`*`; plain read emits
//! none) plus the return type. The struct/enum/trait surface is diffed
//! separately by the SemVer API check (`Publish::diff_public_api`); this
//! snapshot is the *function-signature* surface specifically.
//!
//! Lives at `.jet/cache/api/<package>.api` (committed, durable contract — the same
//! discipline as the D-MIGRATE1 `@PublishedSchema` snapshot).

use crate::Syntax;
use crate::AST::{Dimension, Func, Item, TraitMethodSig, Type};
use crate::Sema::EffectSet;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const API_SNAPSHOT_VERSION: u32 = 3;

/// One frozen public function in a package's capability API.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FrozenFn {
    pub name: String,
    /// The canonical capability signature, e.g. `fn scale(v: &Vec3, factor: Float)`.
    /// Carries the resolved D-MEM1 sigils that the caller must honour.
    pub signature: String,
}

/// The frozen public-capability surface of one package's library target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiSnapshot {
    pub api_version: u32,
    pub package: String,
    pub published_version: String,
    /// Public functions, sorted by name for a stable diff.
    pub funcs: Vec<FrozenFn>,
}

impl ApiSnapshot {
    /// Serialise to the lockfile-style text format.
    pub fn write(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("api_version = {}\n", self.api_version));
        out.push_str(&format!("package = {}\n", self.package));
        out.push_str(&format!("published_version = {}\n", self.published_version));
        for f in &self.funcs {
            out.push_str(&f.signature);
            out.push('\n');
        }
        out
    }

    /// Parse from the lockfile-style text format.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let mut api_version: Option<u32> = None;
        let mut package: Option<String> = None;
        let mut published_version: Option<String> = None;
        let mut funcs = Vec::new();

        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("api_version = ") {
                api_version = Some(
                    rest.parse()
                        .map_err(|_| format!("invalid api_version: {}", rest))?,
                );
            } else if let Some(rest) = line.strip_prefix("package = ") {
                package = Some(rest.to_string());
            } else if let Some(rest) = line.strip_prefix("published_version = ") {
                published_version = Some(rest.to_string());
            } else if line.starts_with("fn ") {
                let name =
                    fn_name_of(line).ok_or_else(|| format!("malformed fn line: {}", line))?;
                funcs.push(FrozenFn {
                    name,
                    signature: line.to_string(),
                });
            } else {
                return Err(format!("unknown line in api snapshot: {}", line));
            }
        }

        Ok(ApiSnapshot {
            api_version: api_version.ok_or("missing api_version")?,
            package: package.ok_or("missing package")?,
            published_version: published_version.ok_or("missing published_version")?,
            funcs,
        })
    }

    /// The concatenated, canonical text of every frozen `fn` signature, sorted by
    /// name. Folded into the package pin/hash (c129) so a capability change shifts
    /// the lock fingerprint. Excludes the version/package header so the hash tracks
    /// the *shape* of the contract, not the release number.
    pub fn capability_digest(&self) -> String {
        let mut sigs: Vec<&str> = self.funcs.iter().map(|f| f.signature.as_str()).collect();
        sigs.sort_unstable();
        sigs.join("\n")
    }
}

/// The public function name in a `fn name(...) ...` line, or `None`.
fn fn_name_of(line: &str) -> Option<String> {
    let rest = line.strip_prefix("fn ")?;
    let end = rest.find('(').unwrap_or(rest.len());
    let name = rest[..end].trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// The canonical capability signature of a public function — its parameter list
/// with frozen D-CAP7 sigils and the return type. Shared with the SemVer
/// `Publish::API` extractor (same surface; this one is exposed for the freeze).
pub fn fn_signature(f: &Func) -> String {
    canonical_fn_signature(f, &HashMap::new())
}

/// Concrete inferred effects plus symbolic contract entries that cannot be
/// closed until a call site (`..E`, `via f`) or that prohibit an effect.
pub fn normalized_public_effect_row(f: &Func, inferred: &EffectSet) -> EffectSet {
    let mut row = inferred.clone();
    if let Some(declared) = &f.declared_effects {
        row.extend(
            declared
                .iter()
                .map(|(name, _)| name)
                .filter(|name| crate::Sema::effect_row_var(name).is_some() || name.starts_with('!'))
                .cloned(),
        );
    }
    if let Some((param, _)) = &f.effect_via {
        row.insert(format!("via {param}"));
    }
    row
}

/// Canonical public signature with the normalized inferred row. Source may
/// omit the row, but published metadata may not: effect drift is API drift.
pub fn fn_signature_with_effects(f: &Func, inferred: Option<&EffectSet>) -> String {
    canonical_fn_signature_with_effects(f, inferred, &HashMap::new())
}

pub type ApiUnitDimensions = HashMap<String, (String, Dimension)>;

pub fn canonical_api_type_name(ty: &Type, dimensions: &ApiUnitDimensions) -> String {
    match ty {
        Type::Named(name) => dimensions.get(name).map_or_else(
            || ty.name(),
            |(family, dimension)| {
                format!("{name}{{family={family}; base=Float; dimension={}}}", dimension.identity())
            },
        ),
        Type::List(inner) => format!("[{}]", canonical_api_type_name(inner, dimensions)),
        Type::Map { key, value, .. } => format!(
            "[{}: {}]",
            canonical_api_type_name(key, dimensions),
            canonical_api_type_name(value, dimensions)
        ),
        Type::Shared(inner) => format!("Shared<{}>", canonical_api_type_name(inner, dimensions)),
        Type::Option(inner) => format!("{}?", canonical_api_type_name(inner, dimensions)),
        Type::Result { ok, err } => format!(
            "{} ? {}",
            canonical_api_type_name(ok, dimensions),
            canonical_api_type_name(err, dimensions)
        ),
        Type::Fn { params, ret, effect_bound } => {
            let params = params.iter().map(|ty| canonical_api_type_name(ty, dimensions)).collect::<Vec<_>>().join(", ");
            let effects = effect_bound.as_ref().map(|row| row.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", "));
            match (effects, ret) {
                (Some(row), Some(ret)) => format!("fn({params}) --[{row}]-> {}", canonical_api_type_name(ret, dimensions)),
                (Some(row), None) => format!("fn({params}) --[{row}]->"),
                (None, Some(ret)) => format!("fn({params}) -> {}", canonical_api_type_name(ret, dimensions)),
                (None, None) => format!("fn({params})"),
            }
        }
        Type::Apply { name, args } if name == crate::Syntax::TYPE_PTR && args.len() == 1 => {
            format!("*{}", canonical_api_type_name(&args[0], dimensions))
        }
        Type::Apply { .. } if ty.quantity_parts().is_some() => ty.name(),
        Type::Apply { name, args } => format!("{}<{}>", name, args.iter().map(|ty| canonical_api_type_name(ty, dimensions)).collect::<Vec<_>>().join(", ")),
        Type::Tuple(fields) => format!("({})", fields.iter().map(|(name, ty)| format!("{name}: {}", canonical_api_type_name(ty, dimensions))).collect::<Vec<_>>().join(", ")),
        Type::FixedList { elem, len, len_symbol } => format!("[{}#{}]", canonical_api_type_name(elem, dimensions), len_symbol.as_ref().map(|v| v.0.as_str()).map_or_else(|| len.to_string(), str::to_string)),
        Type::Tagged { marker, inner } if marker == crate::AST::CORE_CRYPTO_NOMINAL_MARKER => canonical_api_type_name(inner, dimensions),
        Type::Tagged { marker, inner } => format!("#{marker} {}", canonical_api_type_name(inner, dimensions)),
        _ => ty.name(),
    }
}

pub fn canonical_fn_signature(
    f: &Func,
    dimensions: &ApiUnitDimensions,
) -> String {
    canonical_fn_signature_with_effects(f, None, dimensions)
}

pub fn canonical_fn_signature_with_effects(
    f: &Func,
    inferred: Option<&EffectSet>,
    dimensions: &ApiUnitDimensions,
) -> String {
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| format!("{}: {}{}", p.name, p.convention.sigil(), canonical_api_type_name(&p.ty, dimensions)))
        .collect();
    let ret = match inferred {
        Some(row) => format!(
            " --[{}]->{}",
            normalized_public_effect_row(f, row)
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
            f.return_type
                .as_ref()
                .map(|t| format!(" {}", canonical_api_type_name(t, dimensions)))
                .unwrap_or_default()
        ),
        None => f
            .return_type
            .as_ref()
            .map(|t| format!(" -> {}", canonical_api_type_name(t, dimensions)))
            .unwrap_or_default(),
    };
    let provenance = f.return_view_provenance.as_ref().map_or_else(String::new, |map| {
        if let Some(direct) = map.get(&Vec::<String>::new()).filter(|_| map.len() == 1) {
            format!(" ; view_source = {}", direct.canonical())
        } else {
            format!(
                " ; view_sources = {}",
                crate::AST::canonical_view_provenance_map(map)
            )
        }
    });
    format!("fn {}({}){}{}", f.name, params.join(", "), ret, provenance)
}

/// D-EFF3's public dynamic-dispatch contract. Unlike ordinary functions, a
/// trait method publishes its declared bound because callers cannot inspect a
/// concrete implementation through a trait value.
pub fn trait_method_signature(
    owner: &str,
    method: &TraitMethodSig,
    dimensions: &ApiUnitDimensions,
) -> String {
    let params = method
        .params
        .iter()
        .map(|param| {
            format!(
                "{}: {}{}",
                param.name,
                param.convention.sigil(),
                canonical_api_type_name(&param.ty, dimensions)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let arrow = method.declared_effects.as_ref().map_or_else(
        || {
            method
                .return_type
                .as_ref()
                .map(|_| " ->".to_string())
                .unwrap_or_default()
        },
        |row| {
            format!(
                " --[{}]->",
                row.iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        },
    );
    let result = method
        .return_type
        .as_ref()
        .map(|ty| format!(" {}", canonical_api_type_name(ty, dimensions)))
        .unwrap_or_default();
    format!("fn {owner}.{}({params}){arrow}{result}", method.name)
}

/// Compare a new effect-bearing signature to a pre-v3 snapshot without
/// treating the metadata-format upgrade itself as an API break.
pub fn signature_without_effect_row(signature: &str) -> String {
    let Some(start) = signature.find(" --[") else {
        return signature.to_string();
    };
    let Some(close) = signature[start + 4..].find("]->") else {
        return signature.to_string();
    };
    let end = start + 4 + close + 3;
    let suffix = &signature[end..];
    let arrow = if suffix.starts_with(' ') { " ->" } else { "" };
    format!("{}{}{}", &signature[..start], arrow, suffix)
}

/// Pre-v3 snapshots keyed inline-module functions only by their leaf name.
pub fn legacy_api_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

pub fn qualify_api_signature(namespace: Option<&str>, signature: &str) -> String {
    let Some(namespace) = namespace else {
        return signature.to_string();
    };
    signature
        .strip_prefix("fn ")
        .map(|rest| format!("fn {namespace}.{rest}"))
        .unwrap_or_else(|| signature.to_string())
}

pub fn legacy_api_signature(signature: &str) -> String {
    let signature = [
        ("Int (a whole number)", "Int"),
        ("Float (a decimal number)", "Float"),
        ("Bool (true or false)", "Bool"),
        ("String (text)", "String"),
        ("Char (one character)", "Char"),
        ("F32 (a 32-bit decimal number)", "F32"),
    ]
    .into_iter()
    .fold(signature_without_effect_row(signature), |signature, (old, new)| {
        signature.replace(old, new)
    });
    let Some(rest) = signature.strip_prefix("fn ") else {
        return signature;
    };
    let Some(open) = rest.find('(') else {
        return signature;
    };
    format!("fn {}{}", legacy_api_name(&rest[..open]), &rest[open..])
}

fn frozen_name(code_module: Option<&str>, name: &str) -> String {
    code_module
        .map(|module| format!("{module}.{name}"))
        .unwrap_or_else(|| name.to_string())
}

/// Build a pre-v3 compatibility snapshot from AST items without solved effect
/// facts. Current publish metadata must use [`snapshot_from_items_with_effects`].
pub fn snapshot_from_items(items: &[Item], package: &str, version: &str) -> ApiSnapshot {
    snapshot_from_items_with_effects(items, package, version, None, None)
}

/// Build the public snapshot after sema has solved the bundle effect graph.
pub fn snapshot_from_items_with_effects(
    items: &[Item],
    package: &str,
    version: &str,
    solved: Option<&std::collections::HashMap<String, EffectSet>>,
    module_alias: Option<&str>,
) -> ApiSnapshot {
    let mut funcs = Vec::new();
    let mut dimensions = HashMap::new();
    collect_api_unit_dimensions(items, &mut dimensions);
    collect_pub_fns(
        items,
        solved,
        module_alias,
        None,
        &dimensions,
        &mut funcs,
    );
    funcs.sort();
    ApiSnapshot {
        // Effect rows became mandatory in v3. AST-only callers have no solved
        // effect graph, so they may create compatibility fixtures but cannot
        // claim current-format metadata.
        api_version: if solved.is_some() {
            API_SNAPSHOT_VERSION
        } else {
            API_SNAPSHOT_VERSION - 1
        },
        package: package.to_string(),
        published_version: version.to_string(),
        funcs,
    }
}

/// Recursively collect `pub fn`s from `items`, descending into inline
/// `module { … }` bodies. The package's own module block carries the library's
/// surface (`module foo { pub fn … }`) and need not itself be marked `pub`; it is
/// the `pub` on the *function* that puts it on the contract.
pub fn collect_api_unit_dimensions(
    items: &[Item],
    out: &mut HashMap<String, (String, Dimension)>,
) {
    for item in items {
        match item {
            Item::UnitFamily(family) => {
                if let Some(dimension) = Dimension::for_family(&family.family) {
                    for member in family.distinct_defs() {
                        out.insert(member.name, (family.family.clone(), dimension));
                    }
                }
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    collect_api_unit_dimensions(body, out);
                }
            }
            _ => {}
        }
    }
}

fn collect_pub_fns(
    items: &[Item],
    solved: Option<&std::collections::HashMap<String, EffectSet>>,
    module_alias: Option<&str>,
    code_module: Option<&str>,
    dimensions: &HashMap<String, (String, Dimension)>,
    out: &mut Vec<FrozenFn>,
) {
    let empty_effects = EffectSet::new();
    for item in items {
        match item {
            Item::Func(f)
                if f.is_pub
                    && !f.is_package_pub
                    && crate::Syntax::classify_identifier(&f.name)
                        == crate::Syntax::IdentifierClass::Ordinary => out.push(FrozenFn {
                name: frozen_name(code_module, &f.name),
                signature: qualify_api_signature(
                    code_module,
                    &canonical_fn_signature_with_effects(
                        f,
                        solved.map(|sets| {
                            module_alias.and_then(|alias| {
                                let name = code_module
                                    .map(|module| format!("{module}__{}", f.name))
                                    .unwrap_or_else(|| f.name.clone());
                                sets.get(&format!("{alias}::{name}"))
                            })
                                .or_else(|| sets.get(&f.name))
                                .unwrap_or(&empty_effects)
                        }),
                        dimensions,
                    ),
                ),
            }),
            Item::Trait(trait_def) if trait_def.is_pub && !trait_def.is_package_pub => {
                for method in &trait_def.methods {
                    if crate::Syntax::classify_identifier(&method.name)
                        == crate::Syntax::IdentifierClass::Ordinary
                    {
                        out.push(FrozenFn {
                            name: frozen_name(
                                code_module,
                                &format!("{}.{}", trait_def.name, method.name),
                            ),
                            signature: qualify_api_signature(
                                code_module,
                                &trait_method_signature(&trait_def.name, method, dimensions),
                            ),
                        });
                    }
                }
            }
            Item::CodeModule(m) => {
                if let Some(body) = &m.body {
                    collect_pub_fns(
                        body,
                        solved,
                        module_alias,
                        Some(&m.name),
                        dimensions,
                        out,
                    );
                }
            }
            _ => {}
        }
    }
}

/// The API cache directory for a project (`<root>/.jet/cache/api/`), honouring the
/// `JET_API_CACHE_DIR` test override (mirrors the schema cache override).
pub fn api_cache_dir(project_root: &Path) -> PathBuf {
    if let Ok(override_dir) = std::env::var("JET_API_CACHE_DIR") {
        PathBuf::from(override_dir)
    } else {
        project_root
            .join(Syntax::SOURCE_ROOT_DIR)
            .join(Syntax::API_CACHE_SUBDIR)
    }
}

/// Load a package's frozen-API snapshot from disk, or `None` if no prior freeze.
pub fn load_snapshot(project_root: &Path, package: &str) -> Option<ApiSnapshot> {
    let path = api_cache_dir(project_root).join(format!("{}.api", package));
    let raw = std::fs::read_to_string(&path).ok()?;
    ApiSnapshot::parse(&raw).ok()
}

/// Write a snapshot to disk under `<project_root>/.jet/cache/api/`.
pub fn save_snapshot(project_root: &Path, snap: &ApiSnapshot) -> Result<(), String> {
    let dir = api_cache_dir(project_root);
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create api cache dir: {}", e))?;
    let path = dir.join(format!("{}.api", snap.package));
    std::fs::write(&path, snap.write()).map_err(|e| format!("could not write api snapshot: {}", e))
}

/// Load every frozen-API snapshot in the project's api cache. Used to fold the
/// capability contract into the package pin/hash (c129).
pub fn load_all_snapshots(project_root: &Path) -> Vec<ApiSnapshot> {
    let dir = api_cache_dir(project_root);
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("api") {
                continue;
            }
            if let Ok(raw) = std::fs::read_to_string(&path) {
                if let Ok(snap) = ApiSnapshot::parse(&raw) {
                    out.push(snap);
                }
            }
        }
    }
    out.sort_by(|a, b| a.package.cmp(&b.package));
    out
}

/// The combined capability digest of every frozen API in a project, sorted by
/// package then signature. Folded into the package fingerprint (`Lock`) so a
/// public capability change (read → `&`/`^`) shifts the lock hash even when
/// the source tree hash otherwise matches. Empty when nothing is frozen.
pub fn project_capability_digest(project_root: &Path) -> String {
    let snaps = load_all_snapshots(project_root);
    let mut parts = Vec::new();
    for s in &snaps {
        parts.push(format!("{}\n{}", s.package, s.capability_digest()));
    }
    parts.join("\n--\n")
}

// ──────────────────────────────────────────────
// Unit tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Diagnostics::Span;
    use crate::AST::{AccessConvention, Func, Param, Type, UnitFamilyDef};

    fn zero() -> Span {
        Span::new(0, 0)
    }

    fn param(name: &str, conv: AccessConvention, ty: Type) -> Param {
        Param {
            convention: conv,
            name: name.to_string(),
            name_span: zero(),
            ty,
            ty_span: zero(),
            default: None,
            variadic: false,
            variadic_bound_list: None,
        }
    }

    fn func(name: &str, is_pub: bool, params: Vec<Param>, ret: Option<Type>) -> Func {
        Func {
            span: zero(),
            is_pub,
            is_package_pub: false,
            external_type: None,
            name: name.to_string(),
            name_span: zero(),
            meta: None,
            type_params: vec![],
            params,
            return_type: ret,
            return_type_span: None,
            return_view_provenance: None,
            gc_return: false,
            gc_scope: false,
            is_unsafe: false,
            unsafe_reason: None,
            unsafe_span: None,
            is_pure: false,
            is_reactive: false,
            is_replayable: false,
            replayable_span: None,
            is_task: false,
            task_span: None,
            every: None,
            inline_foreign: None,
            is_sanitizer: false,
            declared_effects: None,
            effect_via: None,
            state_requires: None,
            state_transition: None,
            web_marker: None,
            pre: Vec::new(),
            post: Vec::new(),
            is_must_use: false,
            must_use_span: None,
            maturity: None,
            maturity_span: None,
            is_inline: false,
            is_inline_always: false,
            inline_span: None,
            body: vec![],
        }
    }

    #[test]
    fn sig_carries_resolved_sigil() {
        let f = func(
            "scale",
            true,
            vec![
                param("v", AccessConvention::Write, Type::Named("Vec3".into())),
                param("factor", AccessConvention::Read, Type::Float),
            ],
            None,
        );
        assert_eq!(fn_signature(&f), "fn scale(v: &Vec3, factor: Float)");
    }

    #[test]
    fn sig_carries_normalized_inferred_effects() {
        let f = func("load", true, vec![], Some(Type::String));
        let effects = EffectSet::from(["Fs.Read".to_string(), "Io".to_string()]);
        assert_eq!(
            fn_signature_with_effects(&f, Some(&effects)),
            "fn load() --[Fs.Read, Io]-> String"
        );
        assert_eq!(
            fn_signature_with_effects(&f, Some(&EffectSet::new())),
            "fn load() --[]-> String"
        );
    }

    #[test]
    fn inline_module_snapshot_uses_mangled_effect_identity() {
        let module = Item::CodeModule(crate::AST::CodeModule {
            name: "files".to_string(),
            name_span: zero(),
            is_pub: false,
            is_package_pub: false,
            body: Some(vec![Item::Func(func(
                "load",
                true,
                vec![],
                Some(Type::String),
            ))]),
            web_target: None,
            instance_identity: None,
            span: zero(),
        });
        let solved = std::collections::HashMap::from([(
            "main::files__load".to_string(),
            EffectSet::from(["Fs.Read".to_string()]),
        )]);
        let snapshot = snapshot_from_items_with_effects(
            &[module],
            "files",
            "1.0.0",
            Some(&solved),
            Some("main"),
        );
        assert_eq!(snapshot.funcs[0].name, "files.load");
        assert_eq!(snapshot.api_version, API_SNAPSHOT_VERSION);
        assert_eq!(
            snapshot.funcs[0].signature,
            "fn files.load() --[Fs.Read]-> String"
        );
        let parsed = ApiSnapshot::parse(&snapshot.write()).expect("v3 snapshot round trip");
        assert_eq!(parsed.funcs[0].name, "files.load");
    }

    #[test]
    fn pre_v3_comparison_normalizes_added_metadata_and_type_glosses() {
        assert_eq!(
            signature_without_effect_row("fn load(path: String) --[Fs.Read, Io]-> String"),
            "fn load(path: String) -> String"
        );
        assert_eq!(
            signature_without_effect_row("fn flush() --[]->"),
            "fn flush()"
        );
        assert_eq!(
            signature_without_effect_row("fn legacy() -> Int"),
            "fn legacy() -> Int"
        );
        assert_eq!(legacy_api_name("files.load"), "load");
        assert_eq!(
            legacy_api_signature("fn files.load() --[Fs.Read]-> String"),
            "fn load() -> String"
        );
        assert_eq!(
            legacy_api_signature("fn report(x: Int (a whole number))"),
            "fn report(x: Int)"
        );
    }

    #[test]
    fn sig_carries_normalized_physical_dimension_identity() {
        let speed = crate::AST::Dimension::for_family("Speed").unwrap();
        let f = func(
            "pace",
            true,
            vec![param(
                "value",
                AccessConvention::Read,
                Type::quantity(Type::Float, speed),
            )],
            Some(Type::quantity(Type::Float, speed)),
        );
        assert_eq!(
            fn_signature(&f),
            "fn pace(value: Quantity<Speed, Float; L1T-1>) -> Quantity<Speed, Float; L1T-1>"
        );
    }

    #[test]
    fn checked_source_unit_signature_carries_dimension_identity() {
        let family = UnitFamilyDef {
            is_pub: true,
            is_package_pub: false,
            family: "Length".into(),
            family_span: zero(),
            members: vec![("meter".into(), zero())],
            span: zero(),
        };
        let api = snapshot_from_items(
            &[
                Item::UnitFamily(family),
                Item::Func(func("distance", true, vec![], Some(Type::Named("Meter".into())))),
            ],
            "physics",
            "1.0.0",
        );
        assert_eq!(
            api.funcs[0].signature,
            "fn distance() -> Meter{family=Length; base=Float; dimension=L1T0}"
        );
        assert_eq!(api.api_version, API_SNAPSHOT_VERSION - 1);
    }

    #[test]
    fn sig_carries_parameter_view_provenance() {
        let mut f = func(
            "first",
            true,
            vec![param("xs", AccessConvention::Read, Type::List(Box::new(Type::Int)))],
            Some(Type::Apply { name: "View".into(), args: vec![Type::Int] }),
        );
        f.return_view_provenance = Some(std::collections::BTreeMap::from([(
            Vec::new(),
            crate::AST::ViewProvenance {
                source: crate::AST::ViewSource::Parameter(0),
                projections: vec![crate::AST::ViewSourceProjection::Range],
                mutable: false,
            },
        )]));
        assert!(fn_signature(&f).contains("view_source = parameter:0;access:read;path:range"));
    }

    #[test]
    fn returned_view_source_round_trips_and_changes_contract() {
        let snapshot = |source_index| {
            let mut f = func(
                "pick",
                true,
                vec![
                    param("left", AccessConvention::Read, Type::List(Box::new(Type::Int))),
                    param("right", AccessConvention::Read, Type::List(Box::new(Type::Int))),
                ],
                Some(Type::Apply { name: "View".into(), args: vec![Type::Int] }),
            );
            f.return_view_provenance = Some(std::collections::BTreeMap::from([(
                Vec::new(),
                crate::AST::ViewProvenance {
                    source: crate::AST::ViewSource::Parameter(source_index),
                    projections: vec![crate::AST::ViewSourceProjection::Range],
                    mutable: false,
                },
            )]));
            snapshot_from_items(&[Item::Func(f)], "views", "1.0.0")
        };

        let left = snapshot(0);
        let right = snapshot(1);
        assert_eq!(ApiSnapshot::parse(&left.write()).unwrap(), left);
        assert_eq!(ApiSnapshot::parse(&right.write()).unwrap(), right);
        assert_ne!(left.capability_digest(), right.capability_digest());
        assert!(left.funcs[0].signature.contains("view_source = parameter:0"));
        assert!(right.funcs[0].signature.contains("view_source = parameter:1"));
    }

    #[test]
    fn aggregate_view_sources_round_trip_and_change_contract() {
        let snapshot = |right_source| {
            let mut f = func(
                "pair",
                true,
                vec![
                    param("left", AccessConvention::Read, Type::List(Box::new(Type::Int))),
                    param("right", AccessConvention::Read, Type::List(Box::new(Type::Int))),
                ],
                Some(Type::Named("Pair".into())),
            );
            f.return_view_provenance = Some(std::collections::BTreeMap::from([
                (
                    vec!["left".into()],
                    crate::AST::ViewProvenance {
                        source: crate::AST::ViewSource::Parameter(0),
                        projections: vec![crate::AST::ViewSourceProjection::Range],
                        mutable: false,
                    },
                ),
                (
                    vec!["right".into()],
                    crate::AST::ViewProvenance {
                        source: crate::AST::ViewSource::Parameter(right_source),
                        projections: vec![crate::AST::ViewSourceProjection::Range],
                        mutable: false,
                    },
                ),
            ]));
            snapshot_from_items(&[Item::Func(f)], "views", "1.0.0")
        };

        let distinct = snapshot(1);
        let changed = snapshot(0);
        assert_eq!(ApiSnapshot::parse(&distinct.write()).unwrap(), distinct);
        assert_ne!(distinct.capability_digest(), changed.capability_digest());
        assert!(distinct.funcs[0].signature.contains(
            "view_sources = left=parameter:0;access:read;path:range|right=parameter:1;access:read;path:range"
        ));
    }

    #[test]
    fn round_trip() {
        let items = vec![
            Item::Func(func(
                "length",
                true,
                vec![param(
                    "v",
                    AccessConvention::Read,
                    Type::Named("Vec3".into()),
                )],
                Some(Type::Float),
            )),
            Item::Func(func(
                "scale",
                true,
                vec![param(
                    "v",
                    AccessConvention::Write,
                    Type::Named("Vec3".into()),
                )],
                None,
            )),
            // private fn — excluded
            Item::Func(func("helper", false, vec![], None)),
            // D-SHAPE-INTERNAL1: callable but not a compatibility promise.
            Item::Func(func("_unstable", true, vec![], None)),
        ];
        let snap = snapshot_from_items(&items, "mathkit", "1.0.0");
        assert_eq!(snap.funcs.len(), 2);
        let text = snap.write();
        let parsed = ApiSnapshot::parse(&text).expect("round trips");
        assert_eq!(parsed, snap);
        // Sorted by name: length before scale.
        assert_eq!(parsed.funcs[0].name, "length");
        assert_eq!(parsed.funcs[1].name, "scale");
    }

    #[test]
    fn digest_ignores_version() {
        let mk = |ver: &str| {
            let items = vec![Item::Func(func(
                "scale",
                true,
                vec![param(
                    "v",
                    AccessConvention::Write,
                    Type::Named("Vec3".into()),
                )],
                None,
            ))];
            snapshot_from_items(&items, "mathkit", ver).capability_digest()
        };
        assert_eq!(mk("1.0.0"), mk("2.5.9"), "digest tracks shape, not version");
    }

    #[test]
    fn digest_shifts_on_capability_change() {
        let read = snapshot_from_items(
            &[Item::Func(func(
                "scale",
                true,
                vec![param(
                    "v",
                    AccessConvention::Read,
                    Type::Named("Vec3".into()),
                )],
                None,
            ))],
            "mathkit",
            "1.0.0",
        );
        let write = snapshot_from_items(
            &[Item::Func(func(
                "scale",
                true,
                vec![param(
                    "v",
                    AccessConvention::Write,
                    Type::Named("Vec3".into()),
                )],
                None,
            ))],
            "mathkit",
            "1.0.0",
        );
        assert_ne!(read.capability_digest(), write.capability_digest());
    }

    #[test]
    fn parse_known_text() {
        let text = "api_version = 1\npackage = mk\npublished_version = 0.1.0\nfn f(x: &Int)\n";
        let snap = ApiSnapshot::parse(text).unwrap();
        assert_eq!(snap.package, "mk");
        assert_eq!(snap.funcs.len(), 1);
        assert_eq!(snap.funcs[0].name, "f");
        assert_eq!(snap.funcs[0].signature, "fn f(x: &Int)");
    }
}
