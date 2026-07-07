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

mod DevService;
mod Diagnostics;
mod Eval;
mod Source;
mod System;
mod Types;

pub use Diagnostics::merge_error_to_diagnostic;
pub use Eval::{evaluate_modules, evaluate_source, merge_all, pkg_ref};
pub use Source::{evaluate_env, is_module_surface};
pub use Types::{
    AdapterPlan, AdapterRecipe, DevServicePlan, EnvPlan, EvaluatedModule, FleetPlan, HostPlan,
    ImageKind, ImagePlan, OptionPlan, ServicePlan, SystemPlan,
};

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
    fn evaluate_env_captures_adapt_package() {
        let src = r#"
module dev {
    env.dev: Env.{
        packages: [
            Pkg.adapt(
                name: "weirdctl",
                source: path@vendor/weirdctl,
                deps: [default.cmake],
                recipe: Recipe.prebuilt(bin: "weirdctl", as: "weirdctl")
            ),
            default.ripgrep,
        ],
    }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.package_refs, vec!["default:ripgrep"]);
        assert_eq!(plan.adapters.len(), 1);
        assert_eq!(plan.adapters[0].name, "weirdctl");
        assert_eq!(plan.adapters[0].source, "path@vendor/weirdctl");
        assert_eq!(
            plan.adapters[0].deps,
            vec![Merge::Pkg::new("default", "cmake")]
        );
        match &plan.adapters[0].recipe {
            AdapterRecipe::Prebuilt { bin, as_name } => {
                assert_eq!(bin, "weirdctl");
                assert_eq!(as_name, "weirdctl");
            }
            other => panic!("expected prebuilt recipe, got {other:?}"),
        }
    }

    #[test]
    fn evaluate_env_captures_copy_adapter() {
        let src = r#"
module dev {
    env.dev: Env.{
        packages: [
            Pkg.adapt(
                name: "tool",
                source: path@vendor/tool,
                recipe: Recipe.copy()
            )
        ],
    }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.adapters.len(), 1);
        assert!(matches!(plan.adapters[0].recipe, AdapterRecipe::Copy));
    }

    #[test]
    fn evaluate_env_bad_adapter_is_e1270() {
        let src = r#"
module dev {
    env.dev: Env.{
        packages: [
            Pkg.adapt(
                name: "tool",
                source: path@vendor/tool,
                recipe: Recipe.build()
            )
        ],
    }
}
"#;
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E1270");
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
            network.hostName: halcyon,
            filesystem.timeZone: "Europe/London",
            packages.shell: default.fish,
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
                    key: "network.hostName".into(),
                    value: "halcyon".into()
                },
                OptionPlan {
                    key: "filesystem.timeZone".into(),
                    value: "\"Europe/London\"".into()
                },
                OptionPlan {
                    key: "packages.shell".into(),
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
            .join("../../tests/fixtures/jetpack-typed/system.jet");
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

    // ── U14/D-JPK-IMAGE1: `.Oci` container images ────────────────────────

    /// A fresh temp dir with a `pkg.jet` declaring `app: executable` — the
    /// package an `.Oci` image's `from: packages.app` cross-checks against
    /// (E1267). Unique per call (thread + nanos) so parallel test threads
    /// never race on the same directory the way a shared `base_dir()` would.
    fn oci_base_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "jet-oci-modeval-{tag}-{nanos}-{:?}",
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("pkg.jet"),
            "payload: { name: \"t\", version: \"0.1.0\" }\npackages: { app: executable }\n",
        )
        .unwrap();
        dir
    }

    /// `kind: .Oci` + `from: packages.<name>` captures an `ImagePlan` with
    /// `ImageKind::Oci` and every `.Oci`-only field, cross-checking clean
    /// against a `pkg.jet`-declared executable package.
    #[test]
    fn worked_example_captures_oci_image() {
        let dir = oci_base_dir("worked-example");
        let src = r#"
module image.server {
    kind: .Oci
    from: packages.app
    expose: [8080, 443, 8080]
    env_vars: ["RUST_LOG": "info", "PORT": "8080"]
    files: ["b.txt", "a.txt"]
}
"#;
        let plan = evaluate_env(src, &dir).unwrap();
        assert_eq!(plan.images.len(), 1);
        let image = &plan.images[0];
        assert_eq!(image.name, "server");
        assert_eq!(image.kind, ImageKind::Oci);
        assert_eq!(image.from, "app");
        // Sorted + deduped, not source order.
        assert_eq!(image.expose, vec![443, 8080]);
        assert_eq!(
            image.env_vars,
            vec![
                ("PORT".to_string(), "8080".to_string()),
                ("RUST_LOG".to_string(), "info".to_string()),
            ]
        );
        // Sorted, not source order.
        assert_eq!(image.files, vec!["a.txt".to_string(), "b.txt".to_string()]);
        assert_eq!(image.base, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `kind:` is optional — omitted, it infers `Oci` from `from: packages.*`
    /// (mirroring the pre-existing `from: system.*` → `Iso` inference).
    #[test]
    fn oci_kind_is_inferred_when_omitted() {
        let dir = oci_base_dir("inferred-kind");
        let src = "module image.server { from: packages.app }";
        let plan = evaluate_env(src, &dir).unwrap();
        assert_eq!(plan.images[0].kind, ImageKind::Oci);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `base: oci("<ref>")` is captured (not yet realized — no registry-pull
    /// client exists).
    #[test]
    fn oci_base_is_captured() {
        let dir = oci_base_dir("base-captured");
        let src = r#"module image.server { from: packages.app, base: oci("debian:12") }"#;
        let plan = evaluate_env(src, &dir).unwrap();
        assert_eq!(plan.images[0].base.as_deref(), Some("debian:12"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `kind: .Docker` (or any word that isn't `Oci`/`Iso`) is E1266.
    #[test]
    fn image_unknown_kind_is_e1266() {
        let src = "module image.server { kind: .Docker, from: packages.app }";
        let err = evaluate_env(src, &std::env::temp_dir()).unwrap_err();
        assert_eq!(err.code, "E1266");
    }

    /// An explicit `kind:` that disagrees with what `from:` names is E1266.
    #[test]
    fn image_kind_from_mismatch_is_e1266() {
        let src = "module image.server { kind: .Iso, from: packages.app }";
        let err = evaluate_env(src, &std::env::temp_dir()).unwrap_err();
        assert_eq!(err.code, "E1266");
        let rendered =
            crate::Diagnostics::render_all("config.jet", src, std::slice::from_ref(&err));
        assert!(rendered.contains("doesn't match"), "{rendered}");
    }

    /// An `.Oci` image's `from:` naming a `library`-kind package is E1267 —
    /// the one diagnostic D-JPK-IMAGE1 calls out by name (`oci-from-non-
    /// executable`): a library has no binary to containerize.
    #[test]
    fn oci_from_library_package_is_e1267() {
        let dir = oci_base_dir("library-rejected");
        std::fs::write(
            dir.join("pkg.jet"),
            "payload: { name: \"t\", version: \"0.1.0\" }\npackages: { app: library }\n",
        )
        .unwrap();
        let src = "module image.server { from: packages.app }";
        let err = evaluate_env(src, &dir).unwrap_err();
        assert_eq!(err.code, "E1267");
        let rendered =
            crate::Diagnostics::render_all("config.jet", src, std::slice::from_ref(&err));
        assert!(rendered.contains("declared `library`"), "{rendered}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// An `.Oci` image's `from:` naming a package `pkg.jet` never declares is
    /// also E1267 (there's nothing to confirm as executable).
    #[test]
    fn oci_from_undeclared_package_is_e1267() {
        let dir = oci_base_dir("undeclared-rejected");
        let src = "module image.server { from: packages.ghost }";
        let err = evaluate_env(src, &dir).unwrap_err();
        assert_eq!(err.code, "E1267");
        let rendered =
            crate::Diagnostics::render_all("config.jet", src, std::slice::from_ref(&err));
        assert!(rendered.contains("isn't declared"), "{rendered}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `expose:` written as anything but a list of `Int` is E1269 (the OCI
    /// field-shape catch-all).
    #[test]
    fn oci_expose_wrong_shape_is_e1269() {
        let dir = oci_base_dir("expose-shape");
        let src = r#"module image.server { from: packages.app, expose: "8080" }"#;
        let err = evaluate_env(src, &dir).unwrap_err();
        assert_eq!(err.code, "E1269");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `.Oci` never reads the `.Iso`-only `format:`/`target:` fields.
    #[test]
    fn oci_image_rejects_iso_only_field() {
        let dir = oci_base_dir("rejects-format");
        let src = "module image.server { from: packages.app, format: iso }";
        let err = evaluate_env(src, &dir).unwrap_err();
        assert_eq!(err.code, "E0977");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `.Iso` never reads the `.Oci`-only `expose`/`env_vars`/`files`/`base` fields.
    #[test]
    fn iso_image_rejects_oci_only_field() {
        let src = "module m { system.s: { target: linux.x64 } image.i: { from: system.s, expose: [8080] } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0977");
    }

    // ── U15: Fleet (D-JPK-FLEET1) ───────────────────────────────────────

    /// A fleet captures its hosts (canonical role-declaration form
    /// `module fleet.<name> { … }`), each cross-checked to a known system, and
    /// records the raw `.{ … }` override text verbatim.
    #[test]
    fn fleet_captures_hosts_and_cross_checks() {
        let src = r#"
module system.web { target: linux.x64 }
module fleet.prod {
    hosts: {
        web1: system.web.{ region: "us-east" },
        web2: system.web.{ region: "eu-west" },
    }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.fleets.len(), 1);
        let fleet = &plan.fleets[0];
        assert_eq!(fleet.name, "prod");
        assert_eq!(fleet.hosts.len(), 2);
        assert_eq!(fleet.hosts[0].name, "web1");
        assert_eq!(fleet.hosts[0].system, "web");
        assert_eq!(
            fleet.hosts[0].overrides.as_deref(),
            Some("{ region: \"us-east\" }")
        );
        assert_eq!(fleet.hosts[1].name, "web2");
        assert_eq!(fleet.hosts[1].system, "web");
    }

    /// A bare host ref (no override tail) captures `None` for overrides.
    #[test]
    fn fleet_bare_host_ref_has_no_overrides() {
        let src = r#"
module system.web { target: linux.x64 }
module fleet.prod { hosts: { only: system.web } }
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.fleets[0].hosts[0].overrides, None);
    }

    /// A host referencing a system no contribution defines is E1242.
    #[test]
    fn fleet_host_unknown_system_is_e1242() {
        let src = "module fleet.prod { hosts: { web1: system.nope } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E1242");
        let rendered =
            crate::Diagnostics::render_all("config.jet", src, std::slice::from_ref(&err));
        assert!(
            rendered.contains("host `web1` names an unknown system `nope`"),
            "{rendered}"
        );
    }

    /// An unknown `Fleet` field is E1244.
    #[test]
    fn fleet_unknown_field_is_e1244() {
        let src = "module fleet.prod { region: \"us\" }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E1244");
    }

    /// A `Fleet` with no `hosts:` is E1245.
    #[test]
    fn fleet_missing_hosts_is_e1245() {
        let src = "module fleet.prod { }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E1245");
    }

    // ── U12: dev-supervised services (`env.<name> { services: { … } }`) ──

    /// The canonical role-module form captures a `services:` map into
    /// `DevServicePlan`s, distinct from (and alongside) ordinary scalar/
    /// `packages:` fields — the recognized control fields (`ports`/`init`/
    /// `ready`) come back typed, not as display-string `extra`.
    #[test]
    fn dev_services_are_captured_with_typed_fields() {
        let src = r#"
module env.dev {
    prompt: "wordstats",
    services: {
        redis: { enable: true, ports: [6380], init: "redis-server --port 6380", ready: "redis-cli -p 6380 ping" },
        worker: { enable: false },
    }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.prompt.as_deref(), Some("wordstats"));
        assert_eq!(plan.dev_services.len(), 2);
        let redis = &plan.dev_services[0];
        assert_eq!(redis.name, "redis");
        assert!(redis.enable);
        assert_eq!(redis.ports, vec![6380]);
        assert_eq!(redis.init.as_deref(), Some("redis-server --port 6380"));
        assert_eq!(redis.ready.as_deref(), Some("redis-cli -p 6380 ping"));
        assert!(redis.extra.is_empty());
        let worker = &plan.dev_services[1];
        assert_eq!(worker.name, "worker");
        assert!(!worker.enable);
    }

    /// A field jetpack's dev-runtime tier doesn't recognize is captured in
    /// `extra` (open record, U12) rather than rejected at field-check time —
    /// `Jetpack::Services` is the one that flags it (E1262), not modeval.
    #[test]
    fn dev_service_unrecognized_field_lands_in_extra() {
        let src = r#"
module env.dev {
    services: { redis: { enable: true, prot: 6380 } }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(
            plan.dev_services[0].extra,
            vec![("prot".to_string(), "6380".to_string())]
        );
    }

    /// A dev service with no `enable` is E0975 — the exact same diagnostic
    /// (and required-field rule) as a jetos `system.*.services` entry (U12's
    /// `Service` is one ratified grammar either way).
    #[test]
    fn dev_service_without_enable_is_e0975() {
        let src = "module env.dev { services: { redis: { ports: [6379] } } }";
        let err = evaluate_env(src, &base_dir()).unwrap_err();
        assert_eq!(err.code, "E0975");
    }

    /// `services:` under `env.*` never contaminates the jetos `system.*`
    /// capture (`ServicePlan`) — they're wholly separate lists on the plan.
    #[test]
    fn dev_services_and_jetos_services_are_independent() {
        let src = r#"
module env.dev {
    services: { redis: { enable: true } }
}
module system.box {
    target: linux.x64,
    services: { openssh: { enable: true } }
}
"#;
        let plan = evaluate_env(src, &base_dir()).unwrap();
        assert_eq!(plan.dev_services.len(), 1);
        assert_eq!(plan.dev_services[0].name, "redis");
        assert_eq!(plan.systems[0].services.len(), 1);
        assert_eq!(plan.systems[0].services[0].name, "openssh");
    }

    /// I5: the committed jetpack-typed fixture is the executable spec for
    /// U12 dev services.
    #[test]
    fn committed_services_example_field_checks_clean() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/jetpack-typed/services.jet");
        let src = std::fs::read_to_string(&path).unwrap();
        let dir = path.parent().unwrap();
        let plan = evaluate_env(&src, dir).unwrap();
        assert_eq!(plan.dev_services.len(), 3);
        let redis = &plan.dev_services[0];
        assert_eq!(redis.name, "redis");
        assert!(redis.enable);
        assert!(redis.init.is_none(), "redis relies on the built-in catalog");
        let worker = &plan.dev_services[1];
        assert_eq!(worker.name, "worker");
        assert_eq!(worker.ports, vec![8080]);
        assert_eq!(worker.init.as_deref(), Some("worker --port 8080"));
        assert!(worker.ready.is_some());
        let cache = &plan.dev_services[2];
        assert_eq!(cache.name, "cache");
        assert!(!cache.enable);
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
