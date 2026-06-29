//! Module evaluation (computed-modules arc, Stages 2–4): reduces an
//! `env.jet`/`config.jet` file's `module name { ns.path: Type { … } }`
//! contributions to typed values and feeds them through the §6 merge engine
//! (`super::merge`).
//!
//! Two evaluation paths, by field:
//! - **`packages`** reuses the already-tested text-level Pkg-sugar parser
//!   (`Merge::parse_package_list`) on the field's source span. The
//!   `default.[ripgrep, fd]` grammar (U6) is static sugar, not a runtime
//!   computation, so this stays a text slice rather than re-deriving the
//!   same rules at the AST level.
//! - **Every other field** runs through `Comptime::evaluate` — the M9.5
//!   pure-eval interpreter, extended (this arc) with `if … else` expression
//!   support — so a module field may hold any pure, deterministic
//!   expression, not just a literal.
//!
//! `Item::Module` is otherwise invisible to sema/codegen (a deliberate no-op,
//! see commit 2b3825e), so this module owns the only pass that gives module
//! bodies meaning.

mod Diagnostics;
mod Eval;
mod Source;
mod System;
mod Types;

pub use Diagnostics::merge_error_to_diagnostic;
pub use Eval::{evaluate_modules, evaluate_source, merge_all, pkg_ref};
pub use Source::{evaluate_env, is_module_surface};
pub use Types::{EnvPlan, EvaluatedModule, ImagePlan, OptionPlan, ServicePlan, SystemPlan};

#[cfg(test)]
use std::path::Path;

#[cfg(test)]
use super::Merge::{self, Scalar};
#[cfg(test)]
use super::RefSpec::ProviderKind;
#[cfg(test)]
use crate::AST::Namespace;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn base_dir() -> PathBuf {
        std::env::temp_dir()
    }

    #[test]
    fn evaluates_plain_scalar_and_packages() {
        let src = r#"
module dev {
    env.dev: Env.{
        packages: [default.[ripgrep, fd], unstable.neovim],
        prompt: "wordstats",
    }
}
"#;
        let modules = evaluate_source(src, &base_dir()).unwrap();
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].name, "dev");
        let (key, entry) = &modules[0].entries[0];
        assert_eq!(key.0, Namespace::Env);
        assert_eq!(key.1, "dev");
        assert_eq!(
            entry.packages,
            vec![
                Merge::Pkg::new("default", "ripgrep"),
                Merge::Pkg::new("default", "fd"),
                Merge::Pkg::new("unstable", "neovim"),
            ]
        );
        assert_eq!(
            entry.settings.get("prompt"),
            Some(&vec![Scalar::normal("wordstats")])
        );
    }

    #[test]
    fn evaluates_computed_scalar_via_if_else() {
        let src = r#"
module dev {
    env.dev: Env.{
        prompt: if 3 > 2 { "yes" } else { "no" },
    }
}
"#;
        let modules = evaluate_source(src, &base_dir()).unwrap();
        let (_, entry) = &modules[0].entries[0];
        assert_eq!(
            entry.settings.get("prompt"),
            Some(&vec![Scalar::normal("yes")])
        );
    }

    #[test]
    fn disabled_module_is_skipped() {
        let src = r#"
module _gaming {
    env.gaming: Env.{
        prompt: "should not appear",
    }
}
"#;
        let modules = evaluate_source(src, &base_dir()).unwrap();
        assert!(modules.is_empty());
    }

    #[test]
    fn wrong_namespace_type_is_a_pinned_diagnostic() {
        let src =
            "\nmodule dev {\n    env.dev: System.{\n        prompt: \"wrong type\",\n    }\n}\n";
        let err = evaluate_source(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0966");
        let rendered = crate::Diagnostics::render_all("env.jet", src, std::slice::from_ref(&err));
        assert_eq!(
            rendered,
            "Error [E0966]: expected a `Env` literal here, found `System`\n  --> env.jet:3:14\n    |\n  3 |     env.dev: System.{\n    |              ^^^^^^^^\n Why: a contribution to this namespace must use the matching type `Env`\n Fix: change `System.{…}` to `Env.{…}`\n"
        );
    }

    #[test]
    fn ambient_io_in_build_is_e3402() {
        let src = "\nmodule dev {\n    env.dev: Env.{\n        prompt: read_file(\"/etc/hostname\"),\n    }\n}\n";
        let err = evaluate_source(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E3402");
        let rendered = crate::Diagnostics::render_all("env.jet", src, std::slice::from_ref(&err));
        assert!(
            rendered.contains("`read_file` is not allowed during a sandboxed package build"),
            "unexpected render:\n{rendered}"
        );
        assert!(rendered
            .contains("package builds run with ambient I/O and network access disabled (D-PURE2)"));
    }

    /// A fresh, empty directory under the system temp dir, unique per call.
    fn fresh_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("modeval-{tag}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn evaluate_env_builds_plan_from_typed_surface() {
        let src = r#"
module dev {
    sources: { default: github@NixOS/nixpkgs/nixos-24.05 }
    env.dev: Env.{
        packages: [default.[ripgrep, fd]],
        prompt: "wordstats",
    }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.prompt.as_deref(), Some("wordstats"));
        assert_eq!(plan.package_refs, vec!["default:ripgrep", "default:fd"]);
        // The `provider@target` ref is translated to the colon/flake upstream the
        // provider realizes (`github:…#pkg`).
        assert_eq!(
            plan.table.upstream("default"),
            Some("github:NixOS/nixpkgs/nixos-24.05")
        );
    }

    #[test]
    fn github_source_kind_is_left_to_inference() {
        // U9: a `github@…` source can't be classified core-vs-nix at pure
        // evaluation time (it depends on a remote `pkg.jet` peek), so the table
        // records `Infer`; `Provider::resolve_kind` decides at realize time.
        let src = r#"
module dev {
    sources: { up: github@acme/jet-pkgs/v1 }
    env.dev: Env.{ packages: [up.hello] }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.table.provider("up"), ProviderKind::Infer);
        assert_eq!(plan.table.upstream("up"), Some("github:acme/jet-pkgs/v1"));
    }

    #[test]
    fn nixpkgs_source_kind_stays_nix() {
        let src = r#"
module dev {
    sources: { default: nixpkgs@nixpkgs-unstable }
    env.dev: Env.{ packages: [default.fd] }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.table.provider("default"), ProviderKind::Nix);
    }

    #[test]
    fn evaluate_env_bare_package_resolves_to_default_source() {
        let src = r#"
module dev {
    sources: { default: nixpkgs@nixpkgs-unstable }
    env.dev: Env.{ packages: [ripgrep] }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.package_refs, vec!["default:ripgrep"]);
    }

    #[test]
    fn evaluate_env_rejects_non_provider_source_ref() {
        let src = "\nmodule dev {\n    sources: { default: nixos-24.05 }\n    env.dev: Env.{ packages: [default.ripgrep] }\n}\n";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0968");
        let rendered = crate::Diagnostics::render_all("env.jet", src, std::slice::from_ref(&err));
        assert_eq!(
            rendered,
            "Error [E0968]: `nixos-24.05` isn't a `provider@target` source ref\n  --> env.jet:3:25\n    |\n  3 |     sources: { default: nixos-24.05 }\n    |                         ^^^^^^^^^^^\n Why: a named source resolves to an upstream written as `provider@target` (U6) — `github@owner/repo/rev`, `path@../local`, `nixpkgs@channel`\n Fix: write the ref as `provider@target`, e.g. `github@NixOS/nixpkgs/nixos-24.05`\n"
        );
    }

    #[test]
    fn evaluate_env_conflicting_sources_are_a_merge_error() {
        let src = r#"
module a {
    sources: { default: github@NixOS/nixpkgs/nixos-24.05 }
    env.dev: Env.{ packages: [default.ripgrep] }
}
module b {
    sources: { default: github@NixOS/nixpkgs/nixos-23.11 }
    env.dev: Env.{ packages: [default.fd] }
}
"#;
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0967");
    }

    #[test]
    fn merges_packages_across_modules_and_dedupes() {
        let src = r#"
module a {
    env.dev: Env.{
        packages: [default.ripgrep],
    }
}
module b {
    env.dev: Env.{
        packages: [default.ripgrep, default.fd],
    }
}
"#;
        let modules = evaluate_source(src, &base_dir()).unwrap();
        let merged = merge_all(&modules).unwrap();
        let entry = merged.get(&(Namespace::Env, "dev".to_string())).unwrap();
        assert_eq!(
            entry.packages,
            vec![
                Merge::Pkg::new("default", "ripgrep"),
                Merge::Pkg::new("default", "fd"),
            ]
        );
    }

    #[test]
    fn conflicting_scalar_contributions_are_a_merge_error() {
        let src = r#"
module a {
    env.dev: Env.{
        prompt: "one",
    }
}
module b {
    env.dev: Env.{
        prompt: "two",
    }
}
"#;
        let modules = evaluate_source(src, &base_dir()).unwrap();
        let err = merge_all(&modules).unwrap_err();
        let diag = merge_error_to_diagnostic(&err);
        assert_eq!(diag.code, "E0967");
        let rendered = crate::Diagnostics::render_all("env.jet", src, std::slice::from_ref(&diag));
        assert_eq!(
            rendered,
            "Error [E0967]: `prompt` got conflicting values: one, two\n Why: scalar settings merge to one value; without a priority marker, modules contributing different values can't be reconciled\n Fix: make every module agree on this value, or remove the conflicting contribution\n"
        );
    }

    #[test]
    fn find_discovers_modules_and_merges_their_packages() {
        // U4: a `find("./modules")` import walks the dir, parses each `.jet`, and
        // folds its modules into the same merge — the discovered `jq` joins the
        // root's `ripgrep`, reusing the root-declared `default` source.
        let dir = fresh_dir("find-discovers");
        std::fs::create_dir_all(dir.join("modules")).unwrap();
        std::fs::write(
            dir.join("modules/tools.jet"),
            "module tools { env.dev: Env.{ packages: [default.jq] } }",
        )
        .unwrap();
        let src = "module dev {\n    sources: { default: nixpkgs@nixpkgs-unstable }\n    imports: find(\"./modules\")\n    env.dev: Env.{ packages: [default.ripgrep] }\n}\n";
        let plan = evaluate_env(src, &dir).unwrap();
        assert_eq!(plan.package_refs, vec!["default:ripgrep", "default:jq"]);
        assert_eq!(
            plan.table.upstream("default"),
            Some("nixpkgs:nixpkgs-unstable")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn find_missing_directory_is_a_pinned_diagnostic() {
        let src = "\nmodule dev {\n    imports: find(\"./nope\")\n    env.dev: Env.{ packages: [default.ripgrep] }\n}\n";
        let dir = fresh_dir("find-missing");
        let err = evaluate_env(src, &dir).unwrap_err();
        assert_eq!(err.code, "E0970");
        // The span points at the `find(…)` call in the root file.
        let rendered = crate::Diagnostics::render_all("env.jet", src, std::slice::from_ref(&err));
        assert!(rendered.contains("Error [E0970]:"), "{rendered}");
        assert!(
            rendered.contains("3 |     imports: find(\"./nope\")"),
            "{rendered}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_find_import_directive_is_e0969() {
        let src = "\nmodule dev {\n    imports: gather(\"./modules\")\n    env.dev: Env.{ packages: [default.ripgrep] }\n}\n";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0969");
        let rendered = crate::Diagnostics::render_all("env.jet", src, std::slice::from_ref(&err));
        assert_eq!(
            rendered,
            "Error [E0969]: an `imports:` directive must be `find(\"<dir>\")`\n  --> env.jet:3:14\n    |\n  3 |     imports: gather(\"./modules\")\n    |              ^^^^^^\n Why: imports auto-discover a directory of modules (U4); the only directive is `find` with a single string-literal path, e.g. `find(\"./modules\")`\n Fix: write `imports: find(\"./modules\")`\n"
        );
    }

    // ── gap #5: System / Service / Image (U11–U14, U18) ──────────────────

    /// The brief's worked example parses, elaborates (U18 bare `{ … }`), and
    /// field-checks clean, capturing a `SystemPlan` + `ImagePlan` (not discarded).
    #[test]
    fn worked_example_captures_system_and_image() {
        let src = r#"
module halcyon {
    sources: { default: github@NixOS/nixpkgs/nixos-24.05 }
    system.halcyon: {
        target: linux.x64,
        packages: [default.[firefox, btop, ripgrep]],
        services: {
            pipewire: { enable: true },
            openssh: { enable: true, ports: [22] },
        },
        options: [
            net.hostName: halcyon,
            time.timeZone: "Europe/London",
            users.nate.shell: default.fish,
        ],
    }
}
module installer {
    image.halcyon-iso: { from: system.halcyon, format: iso }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.systems.len(), 1);
        let sys = &plan.systems[0];
        assert_eq!(sys.name, "halcyon");
        assert_eq!(sys.target, "linux.x64");
        assert_eq!(
            sys.packages,
            vec![
                Merge::Pkg::new("default", "firefox"),
                Merge::Pkg::new("default", "btop"),
                Merge::Pkg::new("default", "ripgrep"),
            ]
        );
        assert_eq!(sys.services.len(), 2);
        assert_eq!(sys.services[0].name, "pipewire");
        assert!(sys.services[0].enable);
        assert_eq!(sys.services[1].name, "openssh");
        assert_eq!(
            sys.services[1].extra,
            vec![("ports".to_string(), "[22]".to_string())]
        );
        assert_eq!(
            sys.options,
            vec![
                OptionPlan {
                    key: "net.hostName".into(),
                    value: "halcyon".into()
                },
                OptionPlan {
                    key: "time.timeZone".into(),
                    value: "\"Europe/London\"".into()
                },
                OptionPlan {
                    key: "users.nate.shell".into(),
                    value: "default.fish".into()
                },
            ]
        );
        assert_eq!(plan.images.len(), 1);
        assert_eq!(plan.images[0].name, "halcyon-iso");
        assert_eq!(plan.images[0].from, "halcyon");
        assert_eq!(plan.images[0].format, "iso");
        assert_eq!(plan.images[0].target, None);
    }

    /// S84: hyphenated System name + hyphenated `from:` reference parse,
    /// elaborate, field-check, and cross-match (E0978 still string-matches the
    /// kebab-case name end-to-end).
    #[test]
    fn s61_hyphenated_system_and_image_names() {
        let src = r#"
module net {
    system.my-host: {
        target: linux.x64,
    }
    image.halcyon-iso: { from: system.my-host, format: iso }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.systems.len(), 1);
        assert_eq!(plan.systems[0].name, "my-host");
        assert_eq!(plan.images.len(), 1);
        assert_eq!(plan.images[0].name, "halcyon-iso");
        assert_eq!(plan.images[0].from, "my-host");
        assert_eq!(plan.images[0].format, "iso");
    }

    /// S84 (regression): a leading-hyphen name is rejected cleanly (the ordinary
    /// `expect_ident` teaching diagnostic, never an ICE). The `-` is not glued to
    /// a preceding ident, so it never starts a dashed name.
    #[test]
    fn s61_leading_hyphen_name_is_clean_error() {
        let src = "module m { image.-iso: { from: system.halcyon, format: iso } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0003");
    }

    /// S84 (regression): a doubled hyphen stops the dashed name at the first gap;
    /// the trailing `-` then fails to find an adjacent ident, so the contribution
    /// name ends and the next `expect` reports cleanly (no ICE).
    #[test]
    fn s61_double_hyphen_name_is_clean_error() {
        let src = "module m { image.a--b: { from: system.halcyon, format: iso } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        // Reaches a parser diagnostic, not a panic; the exact code is the
        // "expected `:`" family from the stalled name.
        assert!(err.code.starts_with('E'), "code: {}", err.code);
    }

    /// I5: the committed jetpack-typed fixture is the executable spec.
    #[test]
    fn committed_system_example_field_checks_clean() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/jetpack-typed/system.jet");
        let src = std::fs::read_to_string(&path).unwrap();
        let dir = path.parent().unwrap();
        let plan = evaluate_env(&src, dir).unwrap();
        assert_eq!(plan.systems.len(), 1);
        // S84: the example uses kebab-case names in the System and Image positions.
        assert_eq!(plan.systems[0].name, "my-host");
        assert_eq!(plan.images.len(), 1);
        assert_eq!(plan.images[0].name, "halcyon-iso");
        assert_eq!(plan.images[0].from, "my-host");
    }

    /// U18: an explicit `System.{ … }` / `Service { … }` / `Image { … }` is still
    /// legal alongside the inferred bare form.
    #[test]
    fn explicit_type_names_still_parse() {
        let src = r#"
module m {
    system.box: System.{
        target: linux.arm64,
        services: { sshd: Service.{ enable: false } },
    }
    image.box_iso: Image.{ from: system.box }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.systems[0].target, "linux.arm64");
        assert!(!plan.systems[0].services[0].enable);
        assert_eq!(plan.images[0].format, "iso");
    }

    #[test]
    fn unknown_system_field_is_e0972() {
        let src = "module m { system.s: { target: linux.x64, gpu: true } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0972");
        let rendered =
            crate::Diagnostics::render_all("config.jet", src, std::slice::from_ref(&err));
        assert_eq!(
            rendered,
            "Error [E0972]: `gpu` isn't a field of `System`\n  --> config.jet:1:43\n    |\n  1 | module m { system.s: { target: linux.x64, gpu: true } }\n    |                                           ^^^\n Why: a `System` has a fixed set of fields: `target`, `packages`, `services`, `options`\n Fix: remove `gpu`, or use one of `target`, `packages`, `services`, `options`\n"
        );
    }

    #[test]
    fn unknown_platform_target_is_e0973() {
        let src = "module m { system.s: { target: windows.x64 } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0973");
        let rendered =
            crate::Diagnostics::render_all("config.jet", src, std::slice::from_ref(&err));
        assert!(
            rendered.contains("`windows.x64` isn't a platform"),
            "{rendered}"
        );
    }

    #[test]
    fn system_without_target_is_e0974() {
        let src = "module m { system.s: { packages: [default.fd] } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0974");
    }

    #[test]
    fn service_without_enable_is_e0975() {
        let src =
            "module m { system.s: { target: linux.x64, services: { ssh: { ports: [22] } } } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0975");
        let rendered =
            crate::Diagnostics::render_all("config.jet", src, std::slice::from_ref(&err));
        assert!(rendered.contains("`ssh` has no `enable`"), "{rendered}");
    }

    #[test]
    fn service_enable_not_bool_is_e0975() {
        let src = "module m { system.s: { target: linux.x64, services: { ssh: { enable: 1 } } } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0975");
    }

    #[test]
    fn bad_image_format_is_e0976() {
        let src =
            "module m { system.s: { target: linux.x64 } image.i: { from: system.s, format: dmg } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0976");
        let rendered =
            crate::Diagnostics::render_all("config.jet", src, std::slice::from_ref(&err));
        assert!(
            rendered.contains("`dmg` isn't a disk-image format"),
            "{rendered}"
        );
    }

    #[test]
    fn image_without_from_is_e0977() {
        let src = "module m { image.i: { format: iso } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0977");
    }

    #[test]
    fn image_restating_inherited_field_is_e0977() {
        let src = "module m { system.s: { target: linux.x64 } image.i: { from: system.s, packages: [default.fd] } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0977");
        let rendered =
            crate::Diagnostics::render_all("config.jet", src, std::slice::from_ref(&err));
        assert!(
            rendered.contains("doesn't restate `packages`"),
            "{rendered}"
        );
    }

    #[test]
    fn image_cross_compile_target_is_allowed() {
        let src = "module m { system.s: { target: linux.x64 } image.i: { from: system.s, target: linux.arm64 } }";
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.images[0].target.as_deref(), Some("linux.arm64"));
    }

    #[test]
    fn image_from_unknown_system_is_e0978() {
        let src = "module m { image.i: { from: system.nope } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0978");
        let rendered =
            crate::Diagnostics::render_all("config.jet", src, std::slice::from_ref(&err));
        assert!(rendered.contains("unknown system `nope`"), "{rendered}");
    }

    #[test]
    fn discovered_module_that_imports_is_e0971() {
        // Liftability law (U4): a discovered module may not itself import.
        let dir = fresh_dir("find-liftability");
        std::fs::create_dir_all(dir.join("modules")).unwrap();
        std::fs::write(
            dir.join("modules/nested.jet"),
            "module nested {\n    imports: find(\"./more\")\n    env.dev: Env.{ packages: [default.jq] }\n}\n",
        )
        .unwrap();
        let src = "module dev {\n    imports: find(\"./modules\")\n    env.dev: Env.{ packages: [default.ripgrep] }\n}\n";
        let err = evaluate_env(src, &dir).unwrap_err();
        assert_eq!(err.code, "E0971");
        let rendered = crate::Diagnostics::render_all("env.jet", src, std::slice::from_ref(&err));
        assert!(rendered.contains("Error [E0971]:"), "{rendered}");
        assert!(rendered.contains("liftability law"), "{rendered}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
