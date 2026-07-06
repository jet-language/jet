//! U11–U14/U18: the jetos-tier field checks. Validate and capture
//! `system.<name>: { … }` and `image.<name>: { … }` contributions as
//! `SystemPlan`/`ServicePlan`/`OptionPlan`/`ImagePlan` (pure data — realize
//! logic lives in the jetos tier, gap #4).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::Comptime::{self, CtValue};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::AST::{
    Expr, FleetFieldValue, FleetLit, Func, ImageFieldValue, ImageFromRef, ImageLit, ServiceEntry,
    SystemFieldValue, SystemLit,
};

use super::Diagnostics::{
    fleet_missing_hosts, fleet_unknown_field, image_bad_format, image_field_shape,
    image_kind_from_mismatch, image_missing_from, image_restated_field, image_unknown_kind,
    missing_system_target, service_enable_not_bool, service_missing_enable, unknown_platform,
    unknown_record_field,
};
use super::Eval::{check_build_io, extract_packages};
use super::Types::{
    FleetPlan, HostPlan, ImageKind, ImagePlan, OptionPlan, ServicePlan, SystemPlan,
};

/// U11/U12/U13/U18: field-check a `system.<name>: { … }` record and capture it as
/// a `SystemPlan`. Validates that every field is one of the four known `System`
/// fields, that `target` names a known platform, that `services` records carry a
/// `Bool` `enable`, and that `options` is a list of `key: value` entries.
pub(super) fn evaluate_system(
    path: &str,
    lit: &SystemLit,
    src: &str,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
) -> Result<SystemPlan, Diagnostic> {
    let mut target = None;
    let mut packages = Vec::new();
    let mut services = Vec::new();
    let mut options = Vec::new();
    for field in &lit.fields {
        match &field.value {
            SystemFieldValue::Platform { os, arch, span } => {
                target = Some(check_platform(os, arch, *span)?);
            }
            SystemFieldValue::Packages(value) => {
                packages.extend(extract_packages(value, src)?.packages);
            }
            SystemFieldValue::Services(entries) => {
                for e in entries {
                    services.push(evaluate_service(e, base_dir, funcs)?);
                }
            }
            SystemFieldValue::Options(entries) => {
                // U13: option values are typed values / identifiers (`halcyon`,
                // `default.fish`) or free-form quoted strings — not computed
                // expressions. Capture the written value by slicing its source
                // span, the same technique `sources:`/`packages:` use.
                for e in entries {
                    let span = e.value_span;
                    options.push(OptionPlan {
                        key: e.key.clone(),
                        value: src[span.start..span.end].trim().to_string(),
                    });
                }
            }
            SystemFieldValue::Other(_) => {
                return Err(unknown_record_field(
                    Syntax::TYPE_SYSTEM,
                    &field.name,
                    "`target`, `packages`, `services`, `options`",
                    field.name_span,
                ));
            }
        }
    }
    let target = target.ok_or_else(|| missing_system_target(lit.span))?;
    Ok(SystemPlan {
        name: path.to_string(),
        target,
        packages,
        services,
        options,
    })
}

/// U13: a `target`/cross-compile platform must name a known OS+arch pair.
fn check_platform(os: &str, arch: &str, span: Span) -> Result<String, Diagnostic> {
    let known_arch = matches!(arch, _ if arch == Syntax::PLATFORM_ARCH_X64 || arch == Syntax::PLATFORM_ARCH_ARM64);
    if os == Syntax::PLATFORM_OS_LINUX && known_arch {
        Ok(format!("{os}.{arch}"))
    } else {
        Err(unknown_platform(os, arch, span))
    }
}

/// U12: field-check one `Service` record (open record; requires a `Bool`
/// `enable`) and capture it as a `ServicePlan`.
fn evaluate_service(
    entry: &ServiceEntry,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
) -> Result<ServicePlan, Diagnostic> {
    let mut enable = None;
    let mut extra = Vec::new();
    for (name, span, value) in &entry.fields {
        check_build_io(value)?;
        let v = Comptime::evaluate(value, funcs, &HashSet::new(), base_dir, &HashMap::new())?;
        if name == Syntax::SERVICE_FIELD_ENABLE {
            match v {
                Comptime::CtValue::Bool(b) => enable = Some(b),
                _ => return Err(service_enable_not_bool(*span)),
            }
        } else {
            extra.push((name.clone(), v.jet_show()));
        }
    }
    let enable = enable.ok_or_else(|| service_missing_enable(&entry.name, entry.span))?;
    Ok(ServicePlan {
        name: entry.name.clone(),
        enable,
        extra,
    })
}

/// U14/U18/D-JPK-IMAGE1: field-check an `image.<name>: { … }` record and capture
/// it as an `ImagePlan`. `from:` is required and names either a `System`
/// (`.Iso` disk-image tier) or a `Package` (`.Oci` container tier); `kind:` is
/// optional and, when written, must agree with which one `from:` names.
/// `format`/`target` are `.Iso`-only (`format` defaults to `iso`); `expose`/
/// `env_vars`/`files`/`base` are `.Oci`-only. Every other field is rejected —
/// `Image` stays a closed record (unlike the open `Service`).
pub(super) fn evaluate_image(
    path: &str,
    lit: &ImageLit,
    _src: &str,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
) -> Result<ImagePlan, Diagnostic> {
    let mut from: Option<ImageFromRef> = None;
    let mut from_span = lit.span;
    let mut format = None;
    let mut target = None;
    let mut kind_word: Option<(String, Span)> = None;
    let mut expose = Vec::new();
    let mut env_vars = Vec::new();
    let mut files = Vec::new();
    let mut base = None;
    for field in &lit.fields {
        match &field.value {
            ImageFieldValue::From { source, span } => {
                from = Some(clone_from_ref(source));
                from_span = *span;
            }
            ImageFieldValue::Format { word, span } => format = Some(check_format(word, *span)?),
            ImageFieldValue::Platform { os, arch, span } => {
                target = Some(check_platform(os, arch, *span)?);
            }
            ImageFieldValue::Other(expr) if field.name == Syntax::IMAGE_FIELD_KIND => {
                kind_word = Some((bare_enum_word(expr, field.name_span)?, field.name_span));
            }
            ImageFieldValue::Other(expr) if field.name == Syntax::IMAGE_FIELD_EXPOSE => {
                expose = eval_expose(expr, base_dir, funcs, field.name_span)?;
            }
            ImageFieldValue::Other(expr) if field.name == Syntax::IMAGE_FIELD_ENV_VARS => {
                env_vars = eval_env_vars(expr, base_dir, funcs, field.name_span)?;
            }
            ImageFieldValue::Other(expr) if field.name == Syntax::IMAGE_FIELD_FILES => {
                files = eval_files(expr, base_dir, funcs, field.name_span)?;
            }
            ImageFieldValue::Other(expr) if field.name == Syntax::IMAGE_FIELD_BASE => {
                base = Some(eval_base(expr, base_dir, funcs, field.name_span)?);
            }
            ImageFieldValue::Other(_) => {
                return Err(image_restated_field(&field.name, field.name_span));
            }
        }
    }
    let from = from.ok_or_else(|| image_missing_from(lit.span))?;
    let inferred_kind = match &from {
        ImageFromRef::System(_) => ImageKind::Iso,
        ImageFromRef::Package(_) => ImageKind::Oci,
    };
    let kind = match &kind_word {
        None => inferred_kind,
        Some((word, span)) if word == Syntax::IMAGE_KIND_ISO => {
            if inferred_kind != ImageKind::Iso {
                return Err(image_kind_from_mismatch(Syntax::IMAGE_KIND_ISO, *span));
            }
            ImageKind::Iso
        }
        Some((word, span)) if word == Syntax::IMAGE_KIND_OCI => {
            if inferred_kind != ImageKind::Oci {
                return Err(image_kind_from_mismatch(Syntax::IMAGE_KIND_OCI, *span));
            }
            ImageKind::Oci
        }
        Some((word, span)) => return Err(image_unknown_kind(word, *span)),
    };
    let name = match &from {
        ImageFromRef::System(s) => s.clone(),
        ImageFromRef::Package(p) => p.clone(),
    };
    if kind == ImageKind::Oci {
        // `.Oci` never reads `format:`/`target:` — those are the `.Iso` disk-image
        // fields, and an `.Oci` image has no system to cross-compile for.
        if format.is_some() {
            return Err(image_restated_field(Syntax::IMAGE_FIELD_FORMAT, from_span));
        }
        if target.is_some() {
            return Err(image_restated_field(Syntax::SYSTEM_FIELD_TARGET, from_span));
        }
    } else if !expose.is_empty() || !env_vars.is_empty() || !files.is_empty() || base.is_some() {
        // `.Iso` never reads the `.Oci`-only fields.
        return Err(image_restated_field(Syntax::IMAGE_FIELD_EXPOSE, from_span));
    }
    Ok(ImagePlan {
        name: path.to_string(),
        kind,
        from: name,
        format: format.unwrap_or_else(|| Syntax::IMAGE_FORMAT_ISO.to_string()),
        target,
        expose,
        env_vars,
        files,
        base,
    })
}

fn clone_from_ref(r: &ImageFromRef) -> ImageFromRef {
    match r {
        ImageFromRef::System(s) => ImageFromRef::System(s.clone()),
        ImageFromRef::Package(p) => ImageFromRef::Package(p.clone()),
    }
}

/// D-JPK-IMAGE1: `kind: .Oci` / `kind: .Iso` — a bare leading-dot enum literal
/// (D-ENUMDOT2) with no arguments. Anything else (a call, a string, `.Foo(x)`)
/// is a shape error, not a semantic one — `image_unknown_kind` covers a bare
/// word that just isn't `Oci`/`Iso`.
fn bare_enum_word(expr: &Expr, span: Span) -> Result<String, Diagnostic> {
    match expr {
        Expr::EnumLit { variant, args, .. } if args.is_empty() => Ok(variant.clone()),
        _ => Err(image_field_shape(
            Syntax::IMAGE_FIELD_KIND,
            "a bare leading-dot value, e.g. `kind: .Oci`",
            span,
        )),
    }
}

/// D-JPK-IMAGE1: `expose: [Int]` — TCP ports, sorted + deduped so the OCI
/// config's `ExposedPorts` (and so the built image's digest) don't depend on
/// declaration order.
fn eval_expose(
    expr: &Expr,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    span: Span,
) -> Result<Vec<i64>, Diagnostic> {
    check_build_io(expr)?;
    let v = Comptime::evaluate(expr, funcs, &HashSet::new(), base_dir, &HashMap::new())?;
    let CtValue::List(items) = v else {
        return Err(image_field_shape(
            Syntax::IMAGE_FIELD_EXPOSE,
            "a list of ports, e.g. `expose: [8080, 443]`",
            span,
        ));
    };
    let mut ports = Vec::new();
    for item in items {
        let CtValue::Int(n) = item else {
            return Err(image_field_shape(
                Syntax::IMAGE_FIELD_EXPOSE,
                "a list of ports, e.g. `expose: [8080, 443]`",
                span,
            ));
        };
        ports.push(n);
    }
    ports.sort_unstable();
    ports.dedup();
    Ok(ports)
}

/// D-JPK-IMAGE1: `env_vars: [KEY: "value"]` — a map literal; `CtValue::Map` is
/// a `BTreeMap`, so this is already sorted by key.
fn eval_env_vars(
    expr: &Expr,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    span: Span,
) -> Result<Vec<(String, String)>, Diagnostic> {
    check_build_io(expr)?;
    let v = Comptime::evaluate(expr, funcs, &HashSet::new(), base_dir, &HashMap::new())?;
    let CtValue::Map(m) = v else {
        return Err(image_field_shape(
            Syntax::IMAGE_FIELD_ENV_VARS,
            "a map, e.g. `env_vars: [RUST_LOG: \"info\"]`",
            span,
        ));
    };
    let mut out = Vec::new();
    for (k, val) in m {
        let crate::AST::CtKey::Str(key) = k else {
            return Err(image_field_shape(
                Syntax::IMAGE_FIELD_ENV_VARS,
                "a map with string keys, e.g. `env_vars: [RUST_LOG: \"info\"]`",
                span,
            ));
        };
        let CtValue::Str(s) = val else {
            return Err(image_field_shape(
                Syntax::IMAGE_FIELD_ENV_VARS,
                "a map of string values, e.g. `env_vars: [RUST_LOG: \"info\"]`",
                span,
            ));
        };
        out.push((key, s));
    }
    Ok(out)
}

/// D-JPK-IMAGE1: `files: [String]` — extra project-relative paths, sorted so
/// the tar layer is byte-identical regardless of declaration order.
fn eval_files(
    expr: &Expr,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    span: Span,
) -> Result<Vec<String>, Diagnostic> {
    check_build_io(expr)?;
    let v = Comptime::evaluate(expr, funcs, &HashSet::new(), base_dir, &HashMap::new())?;
    let CtValue::List(items) = v else {
        return Err(image_field_shape(
            Syntax::IMAGE_FIELD_FILES,
            "a list of paths, e.g. `files: [\"config/app.toml\"]`",
            span,
        ));
    };
    let mut out = Vec::new();
    for item in items {
        let CtValue::Str(s) = item else {
            return Err(image_field_shape(
                Syntax::IMAGE_FIELD_FILES,
                "a list of paths, e.g. `files: [\"config/app.toml\"]`",
                span,
            ));
        };
        out.push(s);
    }
    out.sort();
    Ok(out)
}

/// D-JPK-IMAGE1: `base: oci("<ref>")` — captured, not yet realized (no native
/// registry-pull client exists yet; `jet image` gates on it honestly rather
/// than silently building from scratch instead).
fn eval_base(
    expr: &Expr,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    span: Span,
) -> Result<String, Diagnostic> {
    let shape_err = || {
        Err(image_field_shape(
            Syntax::IMAGE_FIELD_BASE,
            "`oci(\"<ref>\")`, e.g. `base: oci(\"debian:12\")`",
            span,
        ))
    };
    let Expr::Call(call) = expr else {
        return shape_err();
    };
    if call.name != Syntax::IMAGE_BASE_FN || call.args.len() != 1 {
        return shape_err();
    }
    check_build_io(&call.args[0].expr)?;
    let v = Comptime::evaluate(
        &call.args[0].expr,
        funcs,
        &HashSet::new(),
        base_dir,
        &HashMap::new(),
    )?;
    let CtValue::Str(s) = v else {
        return shape_err();
    };
    Ok(s)
}

/// U15: field-check a `fleet.<name>: { hosts: { … } }` record and capture it as
/// a `FleetPlan`. The one known field is `hosts:`; each host references a
/// `System` (cross-checked against the known systems at plan assembly, E1242).
/// Override records are captured verbatim (sliced from `src`), not applied.
pub(super) fn evaluate_fleet(
    path: &str,
    lit: &FleetLit,
    src: &str,
) -> Result<FleetPlan, Diagnostic> {
    let mut hosts: Option<Vec<HostPlan>> = None;
    for field in &lit.fields {
        match &field.value {
            FleetFieldValue::Hosts(entries) => {
                let mut captured = Vec::new();
                for e in entries {
                    let overrides = e
                        .overrides
                        .map(|span| src[span.start..span.end].trim().to_string());
                    captured.push(HostPlan {
                        name: e.name.clone(),
                        system: e.system.clone(),
                        overrides,
                    });
                }
                hosts = Some(captured);
            }
            FleetFieldValue::Other(_) => {
                return Err(fleet_unknown_field(&field.name, field.name_span));
            }
        }
    }
    let hosts = hosts.ok_or_else(|| fleet_missing_hosts(lit.span))?;
    Ok(FleetPlan {
        name: path.to_string(),
        hosts,
    })
}

/// U14: an image `format:` must be one of `iso` / `qcow` / `raw`.
fn check_format(word: &str, span: Span) -> Result<String, Diagnostic> {
    if word == Syntax::IMAGE_FORMAT_ISO
        || word == Syntax::IMAGE_FORMAT_QCOW
        || word == Syntax::IMAGE_FORMAT_RAW
    {
        Ok(word.to_string())
    } else {
        Err(image_bad_format(word, span))
    }
}
