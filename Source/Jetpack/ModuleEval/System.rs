//! U11–U14/U18: the jetos-tier field checks. Validate and capture
//! `system.<name>: { … }` and `image.<name>: { … }` contributions as
//! `SystemPlan`/`ServicePlan`/`OptionPlan`/`ImagePlan` (pure data — realize
//! logic lives in the jetos tier, gap #4).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::AST::{
    Func, ImageFieldValue, ImageLit, ServiceEntry, SystemFieldValue, SystemLit,
};
use crate::Comptime;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;

use super::Diagnostics::{
    image_bad_format, image_missing_from, image_restated_field, missing_system_target,
    service_enable_not_bool, service_missing_enable, unknown_platform, unknown_record_field,
};
use super::Eval::{check_build_io, extract_packages};
use super::Types::{ImagePlan, OptionPlan, ServicePlan, SystemPlan};

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
                packages.extend(extract_packages(value, src)?);
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
    let known_arch =
        matches!(arch, _ if arch == Syntax::PLATFORM_ARCH_X64 || arch == Syntax::PLATFORM_ARCH_ARM64);
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

/// U14/U18: field-check an `image.<name>: { … }` record and capture it as an
/// `ImagePlan`. Requires `from: system.<name>`; `format` defaults to `iso` and
/// must be one of the three known formats; only `target:` may be restated (for
/// cross-compile) — every other inherited field is rejected.
pub(super) fn evaluate_image(path: &str, lit: &ImageLit) -> Result<ImagePlan, Diagnostic> {
    let mut from = None;
    let mut format = None;
    let mut target = None;
    for field in &lit.fields {
        match &field.value {
            ImageFieldValue::From { system, .. } => from = Some(system.clone()),
            ImageFieldValue::Format { word, span } => format = Some(check_format(word, *span)?),
            ImageFieldValue::Platform { os, arch, span } => {
                target = Some(check_platform(os, arch, *span)?);
            }
            ImageFieldValue::Other(_) => {
                return Err(image_restated_field(&field.name, field.name_span));
            }
        }
    }
    let from = from.ok_or_else(|| image_missing_from(lit.span))?;
    Ok(ImagePlan {
        name: path.to_string(),
        from,
        format: format.unwrap_or_else(|| Syntax::IMAGE_FORMAT_ISO.to_string()),
        target,
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
