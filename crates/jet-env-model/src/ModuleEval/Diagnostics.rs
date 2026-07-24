//! The diagnostic constructors for module evaluation (E0966–E0978, E1242–E1245,
//! E1266–E1267, E1269) plus the merge-conflict (§6) wrapper
//! (`merge_error_to_diagnostic`). Each carries its pinned what/why/fix copy (I4).

use std::path::Path;

use crate::Diagnostics::{Diagnostic, Span};

use super::super::Merge::MergeError;

pub(super) fn bad_source_ref(ref_text: &str, span: Option<Span>) -> Diagnostic {
    Diagnostic::error(
        "E0968",
        format!("`{ref_text}` isn't a `target@provider` source ref or bare path"),
        "D-JPK-REF1 puts the upstream target before `@` and its provider after it; local `./`, `../`, and `/` paths stay bare".to_string(),
        "write `NixOS/nixpkgs/nixos-24.05@github`, `nixos-unstable@nixpkgs`, or a bare path such as `../local`".to_string(),
        span,
    )
}

/// E0969: an `imports:` directive must be `find("<dir>")` with a literal path.
pub(super) fn bad_import_directive(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0969",
        "an `imports:` directive must be `find(\"<dir>\")`".to_string(),
        "imports auto-discover a directory of modules (U4); the only directive is `find` with a single string-literal path, e.g. `find(\"./modules\")`".to_string(),
        "write `imports: find(\"./modules\")`".to_string(),
        Some(span),
    )
}

/// E0970: `imports: find("<dir>")` points at a directory that doesn't exist.
pub(super) fn find_dir_missing(dir: &Path, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0970",
        format!("`find` can't read the directory `{}`", dir.display()),
        "`imports: find(\"<dir>\")` walks that directory for `.jet` modules (U4); it must exist relative to this file".to_string(),
        "create the directory, or fix the path so it points at your modules folder".to_string(),
        Some(span),
    )
}

/// E0971: a discovered module imports — forbidden by the liftability law (U4):
/// modules contribute to the merged whole, they don't import each other.
pub(super) fn discovered_module_imports(file: &Path) -> Diagnostic {
    Diagnostic::error(
        "E0971",
        format!(
            "the discovered module `{}` has its own `imports:`",
            file.display()
        ),
        "a module found by `find(…)` may not import (the liftability law, U4) — modules only contribute to the merged whole, they never import each other; nesting `find` would make composition explode".to_string(),
        "remove the `imports:` from this module; declare all `find(…)` directives in the top-level env.jet".to_string(),
        None,
    )
}

pub(super) fn not_a_namespace_literal(expected: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0966",
        format!("a module contribution must be a `{expected}` literal"),
        format!(
            "a contribution's value describes its namespace with a typed struct literal, e.g. `env.dev: {expected}.{{…}}`"
        ),
        format!("wrap the value in `{expected}.{{…}}`"),
        Some(span),
    )
}

pub(super) fn wrong_namespace_type(expected: &str, got: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0966",
        format!("expected a `{expected}` literal here, found `{got}`"),
        format!("a contribution to this namespace must use the matching type `{expected}`"),
        format!("change `{got}.{{…}}` to `{expected}.{{…}}`"),
        Some(span),
    )
}

pub(super) fn packages_not_a_list(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0966",
        "the `packages` field must be a list literal".to_string(),
        "`packages: [ … ]` lists the packages this contribution adds, using the Pkg sugar (U6)"
            .to_string(),
        "write `packages: [ … ]`".to_string(),
        Some(span),
    )
}

pub(super) fn prompt_bad_field(field: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0966",
        format!("`{field}` isn't a field of `Prompt`"),
        "`Prompt` config has fixed fields: `label`, `path`, and `strip`".to_string(),
        "remove this field, or write `label`, `path`, or `strip`".to_string(),
        Some(span),
    )
}

pub(super) fn prompt_bad_value(field: &str, expected: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0966",
        format!("`Prompt.{field}` isn't shaped like {expected}"),
        "the shell prompt is source-owned by `env.jet`; each prompt setting has one typed shape"
            .to_string(),
        format!("write `{field}: {expected}`"),
        Some(span),
    )
}

/// E0972: an unknown field on a `System` / `Image` / `Service` record (U11/U14).
pub(super) fn unknown_record_field(ty: &str, field: &str, known: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0972",
        format!("`{field}` isn't a field of `{ty}`"),
        format!("a `{ty}` has a fixed set of fields: {known}"),
        format!("remove `{field}`, or use one of {known}"),
        Some(span),
    )
}

/// E0973: a `target:` (or cross-compile platform) names an unknown platform (U13).
pub(super) fn unknown_platform(os: &str, arch: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0973",
        format!("`{os}.{arch}` isn't a platform Jet knows"),
        "U13: a `target` is a typed platform value, not a piece of quoted text — it must be `linux.x64` or `linux.arm64`".to_string(),
        "write `target: linux.x64` or `target: linux.arm64`".to_string(),
        Some(span),
    )
}

/// E0974: a `System` with no `target` field (U11).
pub(super) fn missing_system_target(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0974",
        "this `System` has no `target`".to_string(),
        "U11: every machine names the platform it runs on with a typed `target` value".to_string(),
        "add `target: linux.x64` (or `linux.arm64`)".to_string(),
        Some(span),
    )
}

/// E0975: a `Service` record's `enable` field is not a yes/no value (U12).
pub(super) fn service_enable_not_bool(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0975",
        "a service's `enable` must be `true` or `false`".to_string(),
        "U12: a `Service` turns on or off with a yes/no `enable` flag".to_string(),
        "write `enable: true` or `enable: false`".to_string(),
        Some(span),
    )
}

/// E0975: a `Service` record with no `enable` field (U12).
pub(super) fn service_missing_enable(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0975",
        format!("the service `{name}` has no `enable`"),
        "U12: every `Service` says whether it is on with a required `enable` flag, then any further settings".to_string(),
        format!("add `enable: true` (or `false`) to `{name}`"),
        Some(span),
    )
}

/// E0976: an `Image` `format:` that isn't `iso` / `qcow` / `raw` (U14).
pub(super) fn image_bad_format(word: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0976",
        format!("`{word}` isn't a disk-image format"),
        "D-JETOS-FREEZE1: disk-image formats are frozen jetos research; Jetpack only builds `.Oci` images today".to_string(),
        "use `kind: .Oci` for an active image, or keep disk-image notes in the jetos research appendix".to_string(),
        Some(span),
    )
}

/// E0977: an `Image` with no `from:` field (U14).
pub(super) fn image_missing_from(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0977",
        "this `Image` has no `from`".to_string(),
        "D-JPK-IMAGE1: active Jetpack images are OCI containers built from a package; `system.*` disk images are frozen jetos research".to_string(),
        "add `from: packages.<name>` for an `.Oci` image".to_string(),
        Some(span),
    )
}

/// E0977: an `Image` restates a field it inherits from its system (U14).
pub(super) fn image_restated_field(field: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0977",
        format!("an image doesn't restate `{field}`"),
        "D-JETOS-FREEZE1: fields inherited from `system.*` belong to frozen jetos disk-image research, not active `.Oci` images".to_string(),
        format!("remove `{field}` from the active image; use package/env inputs instead"),
        Some(span),
    )
}

/// E0978: an `Image` `from:` references a system that no contribution defines (U14).
pub(super) fn image_from_unknown_system(image: &str, system: &str, known: &[String]) -> Diagnostic {
    let hint = if known.is_empty() {
        "no `system.<name>:` contribution is defined".to_string()
    } else {
        format!("known systems: {}", known.join(", "))
    };
    Diagnostic::error(
        "E0978",
        format!("the image `{image}` is built from an unknown system `{system}`"),
        "D-JETOS-FREEZE1: `from: system.<name>` is frozen jetos disk-image research; Jetpack's active image path uses `from: packages.<name>`".to_string(),
        format!("use `from: packages.<name>` for an `.Oci` image, or keep the system image as research capture ({hint})"),
        None,
    )
}

/// E1244: an unknown field on a `Fleet` record (U15). The one known field is
/// `hosts`.
pub(super) fn fleet_unknown_field(field: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1244",
        format!("`{field}` isn't a field of `Fleet`"),
        "D-JETOS-FREEZE1: fleet deployment remains frozen jetos research; only `hosts` is captured for planning".to_string(),
        format!("remove `{field}`; captured fleets use `hosts: {{ … }}`"),
        Some(span),
    )
}

/// E1245: a `Fleet` with no `hosts:` field (U15).
pub(super) fn fleet_missing_hosts(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1245",
        "this `Fleet` has no `hosts`".to_string(),
        "D-JETOS-FREEZE1: fleet deployment is frozen jetos research, but captured fleets still name hosts for later planning".to_string(),
        "add `hosts: { web1: system.<name> }` if this is research capture".to_string(),
        Some(span),
    )
}

/// E1242: a fleet host references a system that no contribution defines (U15).
pub(super) fn fleet_unknown_system(
    fleet: &str,
    host: &str,
    system: &str,
    known: &[String],
) -> Diagnostic {
    let hint = if known.is_empty() {
        "no `system.<name>:` contribution is defined".to_string()
    } else {
        format!("known systems: {}", known.join(", "))
    };
    Diagnostic::error(
        "E1242",
        format!("the fleet `{fleet}` host `{host}` names an unknown system `{system}`"),
        "D-JETOS-FREEZE1: fleets are frozen jetos research capture; captured hosts must still point at a captured system so the plan is coherent".to_string(),
        format!("define captured `system.{system}: {{ … }}`, or point the host at an existing captured system ({hint})"),
        None,
    )
}

/// E1266: an `Image`'s `kind:` doesn't name `.Oci`/`.Iso`, or names one that
/// disagrees with what `from:` actually references (D-JPK-IMAGE1).
pub(super) fn image_unknown_kind(word: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1266",
        format!("`{word}` isn't an image kind"),
        "D-JPK-IMAGE1 + D-JETOS-FREEZE1: active Jetpack images use `.Oci`; `.Iso` disk images are frozen jetos research capture".to_string(),
        "write `kind: .Oci` for active Jetpack images".to_string(),
        Some(span),
    )
}

/// E1266: an explicit `kind:` disagrees with what this image's `from:` names
/// (D-JPK-IMAGE1) — e.g. `kind: .Oci` alongside `from: system.<name>`.
pub(super) fn image_kind_from_mismatch(kind: &str, span: Span) -> Diagnostic {
    let (wants, got, status) = if kind == crate::Syntax::IMAGE_KIND_OCI {
        (
            "packages.<name>",
            "system.<name>",
            "active `.Oci` images are built from packages",
        )
    } else {
        (
            "system.<name>",
            "packages.<name>",
            "`.Iso` disk images are frozen jetos research capture",
        )
    };
    Diagnostic::error(
        "E1266",
        format!("`kind: .{kind}` doesn't match this image's `from:`"),
        format!("D-JPK-IMAGE1: {status}; this `from:` names a `{got}`"),
        format!("change `from:` to `{wants}`, or change `kind:` to match what `from:` names"),
        Some(span),
    )
}

/// E1267: an `.Oci` image's `from: packages.<name>` doesn't name a package
/// declared `executable` in this project's `pkg.jet` (D-JPK-IMAGE1) — a
/// library has no binary to containerize, and an undeclared name can't be
/// confirmed either way.
pub(super) fn oci_from_non_executable(image: &str, package: &str, is_library: bool) -> Diagnostic {
    let why = if is_library {
        format!("`{package}` is declared `library` in `pkg.jet` — a library has no binary to containerize")
    } else {
        format!("`{package}` isn't declared in this project's `pkg.jet` `packages:` block")
    };
    Diagnostic::error(
        "E1267",
        format!("the image `{image}` is built from a non-executable package `{package}`"),
        format!("D-JPK-IMAGE1: an `.Oci` image's `from: packages.<name>` must name an `executable` package — {why}"),
        format!("declare `{package}: executable` in `pkg.jet`, or point `from:` at an existing executable package"),
        None,
    )
}

/// E1269: an `.Oci` image field (`kind`/`expose`/`env_vars`/`files`/`base`)
/// is written, but not shaped the way D-JPK-IMAGE1 spells it.
pub(super) fn image_field_shape(field: &str, expected: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1269",
        format!("`{field}` isn't shaped like {expected}"),
        format!("D-JPK-IMAGE1: `{field}` is always written as {expected}"),
        format!("rewrite `{field}:` as {expected}"),
        Some(span),
    )
}

/// Wrap a merge conflict (§6) as a user-facing diagnostic (I4).
pub fn merge_error_to_diagnostic(err: &MergeError) -> Diagnostic {
    match err {
        MergeError::SourceConflict { name, a, b } => Diagnostic::error(
            "E0967",
            format!("source `{name}` is declared with two different refs"),
            format!("one module declares `{name}` as `{a}`, another as `{b}` — sources merge by name, so the refs must agree"),
            "make every declaration of this source agree, or rename one of them".to_string(),
            None,
        ),
        MergeError::ScalarConflict { key, values } => Diagnostic::error(
            "E0967",
            format!("`{key}` got conflicting values: {}", values.join(", ")),
            "scalar settings merge to one value; without a priority marker, modules contributing different values can't be reconciled".to_string(),
            "make every module agree on this value, or remove the conflicting contribution".to_string(),
            None,
        ),
    }
}
