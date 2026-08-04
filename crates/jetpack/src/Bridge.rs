//! `jetpack bridge flake` — a bounded native projection from an existing
//! `flake.nix`'s default devShell into jetpack's
//! own `env.*` module form (D-JPK-MODBODY1's already-ratified `module
//! env.dev { packages: […] }` shape — no new syntax needed).
//!
//! Writes the validated foreign graph to the project's `.jet/lock`, but never
//! touches `env.jet`: the shim prints to stdout so the user reviews and merges
//! it themselves (I8 — one canonical env surface, no silent second manifest).
//! Fields the shim can't express (`shellHook`, a second named devShell, …)
//! remain in the lock as `unmapped` facts and fire L0204, one warning per
//! field, without blocking the print.
//!
//! Determinism for tests: a captured provider payload can still stand in for
//! the foreign result, but the product path evaluates the supported literal
//! devShell surface natively and never shells out to `nix`.

use super::Output::Theme;
use super::Provider::ProviderError;
use super::SemanticLock::FlakeGraph;
use super::JSON;
use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use std::path::Path;
use std::rc::Rc;

/// Facts pulled from a flake's default devShell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DevShellFacts {
    /// `buildInputs` package names, sorted + deduped for a stable shim.
    pub packages: Vec<String>,
    /// Fields present in the devShell that `env.*` has no spelling for yet
    /// (U16's L0204) — today just `shellHook` when it's non-empty.
    pub unmapped: Vec<String>,
}

/// The fixture filename `--fixtures`/`JETPACK_FIXTURES` supplies in place of a
/// live `nix eval` (mirrors `Provider::fixture_name`'s one-file-per-ref
/// convention, but the bridge has exactly one fixture per flake).
pub const FIXTURE_FILE: &str = "flake-devshell.json";

/// Read devshell facts for the flake at `flake_dir`, either from a captured
/// fixture (offline/test path) or through the bounded native evaluator. The
/// native path reads literal `packages`, `buildInputs`, and
/// `nativeBuildInputs` lists, resolves imports only below the flake project
/// root, and retains unsupported hooks as loss facts.
pub fn read_devshell_facts(
    flake_dir: &Path,
    fixtures: Option<&Path>,
) -> Result<DevShellFacts, ProviderError> {
    match fixtures {
        Some(dir) => {
            let path = dir.join(FIXTURE_FILE);
            let stdout = std::fs::read_to_string(&path)
                .map_err(|_| ProviderError::FixtureMissing(path))?;
            parse_facts_json(&stdout)
        }
        None => {
            let flake_path = [
                flake_dir.join(Syntax::FOREIGN_FLAKE_FILE),
                flake_dir.join(Syntax::FOREIGN_DEVENV_FILE),
            ]
            .into_iter()
            .find(|path| path.is_file())
            .ok_or_else(|| {
                ProviderError::Unsupported(
                    "no foreign flake file was found for native evaluation".to_string(),
                )
            })?;
            let source = std::fs::read_to_string(&flake_path).map_err(|error| {
                ProviderError::Unsupported(format!(
                    "couldn't read `{}`: {error}",
                    flake_path.display()
                ))
            })?;
            let system = host_system();
            let source_root = flake_dir.canonicalize().map_err(|error| {
                ProviderError::Unsupported(format!(
                    "couldn't resolve flake project root `{}`: {error}",
                    flake_dir.display()
                ))
            })?;
            let authority = crate::NixEval::ProjectImportAuthority::open(&source_root)
                .map_err(|error| {
                    ProviderError::Unsupported(format!(
                        "couldn't open flake project root for imports: {error}"
                    ))
                })?;
            let import_authority: Rc<dyn Fn(&str) -> Result<String, String>> =
                Rc::new(move |relative: &str| {
                    authority.read(relative).map_err(|error| error.to_string())
                });
            let evaluation = crate::NixEval::evaluate_devshell_with_import_authority(
                &source,
                &system,
                Some(import_authority),
            )
                .map_err(|error| {
                    let reason = error.to_string();
                    // Project-root authority failures are boundary violations,
                    // not unsupported foreign semantics. Keep them on the
                    // existing safety diagnostic; semantic and budget limits
                    // are the E1256 projection surface.
                    if reason.contains("project-root authority")
                        || reason.contains("project-root")
                        || reason.contains("symlink")
                    {
                        ProviderError::Unsupported(reason)
                    } else {
                        ProviderError::ForeignProjection(reason)
                    }
                })?;
            Ok(DevShellFacts {
                packages: evaluation.packages().to_vec(),
                unmapped: evaluation.unsupported().to_vec(),
            })
        }
    }
}

fn host_system() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    format!("{}-{os}", std::env::consts::ARCH)
}

fn parse_facts_json(text: &str) -> Result<DevShellFacts, ProviderError> {
    let parsed = JSON::parse_lenient(text).map_err(ProviderError::BadOutput)?;
    let bad_output = |reason: String| ProviderError::BadOutput(parsed.diagnostic(reason));
    let obj = parsed.value.as_object().map_err(&bad_output)?;

    let mut packages: Vec<String> = obj
        .get("buildInputs")
        .ok_or_else(|| bad_output("missing key `buildInputs`".into()))?
        .as_array()
        .map_err(&bad_output)?
        .iter()
        .map(|value| value.as_str().map(str::to_string).map_err(&bad_output))
        .collect::<Result<_, _>>()?;
    packages.sort();
    packages.dedup();

    let mut unmapped = Vec::new();
    let shell_hook = obj
        .get("shellHook")
        .ok_or_else(|| bad_output("missing key `shellHook`".into()))?
        .as_str()
        .map_err(&bad_output)?;
    if !shell_hook.trim().is_empty() {
        unmapped.push("shellHook".to_string());
    }

    Ok(DevShellFacts { packages, unmapped })
}

/// Render the generated `env.jet` shim text. Deterministic: a sorted,
/// deduped package list means re-running the bridge on an unchanged flake
/// reproduces the exact same bytes (the drift-check test).
pub fn render_shim(facts: &DevShellFacts) -> String {
    let pkgs = facts.packages.join(", ");
    format!(
        "// Generated by `jet bridge flake` — bounded native devShell projection.\n\
         // Review before committing; see stderr for any fields it couldn't map.\n\
         module env.dev {{\n\
         \x20   packages: [{pkgs}]\n\
         }}\n"
    )
}

/// L0204 (U16): one field the shim couldn't translate. Spanless — the source
/// is a foreign `flake.nix`, not `.jet` text this compiler parsed, so there is
/// no snippet to underline (diagnostics.md's spanless render).
pub fn l0204_unmappable(field: &str, file: &str) -> Diagnostic {
    Diagnostic::lint(
        "L0204",
        format!("`{field}` in `{file}` has no `env.*` equivalent yet"),
        "`jet bridge flake` (U16) is a bounded translator; some flake.nix/devenv.nix fields \
         (shellHook, multiple named devShells, buildInputs vs nativeBuildInputs) have no ratified \
         `env.*` spelling."
            .to_string(),
        format!(
            "review the generated shim and add `{field}`'s effect by hand if you need it — the \
             shim is a starting point, not a full translation."
        ),
        None,
    )
}

/// `jetpack bridge flake` — read `flake.nix` in `dir`, commit its typed graph
/// to `.jet/lock`, print the generated shim to stdout, and print any L0204
/// warnings to stderr. Returns the exit code.
pub fn cmd_flake(theme: &Theme, dir: &Path, fixtures: Option<&Path>) -> i32 {
    let flake_path = [
        dir.join(Syntax::FOREIGN_FLAKE_FILE),
        dir.join(Syntax::FOREIGN_DEVENV_FILE),
    ]
    .into_iter()
    .find(|path| path.is_file());
    let Some(flake_path) = flake_path else {
        theme.error(
            "no foreign flake here",
            &format!(
                "`jet bridge flake` translates `{}` or `{}` in the current directory; neither was found.",
                Syntax::FOREIGN_FLAKE_FILE,
                Syntax::FOREIGN_DEVENV_FILE
            ),
            "run this from the directory that has the foreign file, or write env.* by hand.",
        );
        return 2;
    };
    let mut facts = match read_devshell_facts(dir, fixtures) {
        Ok(f) => f,
        Err(e) => {
            super::CLI::report_provider_error(theme, &e);
            return 1;
        }
    };
    // The bridge consumes the same typed foreign graph used by `.jet/lock`.
    // Keep the bounded devShell translation usable when the graph contains
    // unsupported fields, but surface every known projection loss instead of
    // silently dropping it.
    match FlakeGraph::load(&flake_path) {
        Ok(graph) => {
            let system = host_system();
            let mut native_derivations = Vec::new();
            let source = match std::fs::read_to_string(&flake_path) {
                Ok(source) => source,
                Err(error) => {
                    theme.error(
                        "couldn't read the foreign flake",
                        &format!("couldn't evaluate package derivations: {error}"),
                        "fix the flake file, then run `jet bridge flake` again.",
                    );
                    return 1;
                }
            };
            for output in graph.outputs.iter().filter(|output| {
                matches!(
                    &output.kind,
                    super::SemanticLock::FlakeOutputKind::Package
                )
            }) {
                if output.system.is_empty() {
                    theme.error(
                        "couldn't identify a package system",
                        &format!("{} has no supported system-qualified package output", output.name),
                        "write the package under `packages.<system>.<name>`, then run `jet bridge flake` again.",
                    );
                    return 1;
                }
                if output.system != system {
                    continue;
                }
                match crate::NixEval::evaluate_derivation_output(
                    &source,
                    &system,
                    &output.attribute,
                ) {
                    Ok(derivation) => native_derivations.push((output.name.clone(), derivation)),
                    Err(error) => {
                        theme.error(
                            "couldn't evaluate a package derivation",
                            &format!("{}: {error}", output.name),
                            "use a supported pure derivation, or wait for the later evaluator breadth card.",
                        );
                        return 1;
                    }
                }
            }
            if graph.named_dev_shells().len() > 1
                && !facts.unmapped.iter().any(|item| item == "named devShells")
            {
                facts.unmapped.push("named devShells".to_string());
            }
            for field in &graph.unsupported {
                if !facts.unmapped.iter().any(|item| item == field) {
                    facts.unmapped.push(field.clone());
                }
            }
            for output in &graph.outputs {
                let label = if output.system.is_empty() {
                    format!("{}:{}", output.kind.as_str(), output.attribute)
                } else {
                    format!("{}:{}:{}", output.kind.as_str(), output.system, output.attribute)
                };
                match &output.kind {
                    super::SemanticLock::FlakeOutputKind::Package =>
                        record_package_output_fact(&mut facts, output, &system),
                    super::SemanticLock::FlakeOutputKind::DevShell => {
                        if output.attribute != "default"
                            && !facts.unmapped.iter().any(|item| item == &label)
                        {
                            facts.unmapped.push(label);
                        }
                    }
                    super::SemanticLock::FlakeOutputKind::App
                    | super::SemanticLock::FlakeOutputKind::Check
                    | super::SemanticLock::FlakeOutputKind::Formatter
                    | super::SemanticLock::FlakeOutputKind::Other(_) => {
                        if !facts.unmapped.iter().any(|item| item == &label) {
                            facts.unmapped.push(label);
                        }
                    }
                }
            }
            let evaluator = match crate::NixEval::evaluator_identity(&system) {
                Ok(identity) => identity,
                Err(error) => {
                    theme.error(
                        "couldn't identify the native evaluator",
                        &error.to_string(),
                        "use a supported host system for native flake evaluation.",
                    );
                    return 1;
                }
            };
            let mut lock = graph.semantic_lock();
            lock.records.push(super::SemanticLock::SemanticRecord::new(
                super::SemanticLock::LockIdentity {
                    kind: super::SemanticLock::LockRecordKind::FlakeEvaluator,
                    key: "flake-evaluator".to_string(),
                    exact: evaluator.clone(),
                    hash: crate::SHA256::sha256_hex(evaluator.as_bytes()),
                    platform: system.clone(),
                },
                super::SemanticLock::LockRationale {
                    source_ref: flake_path.display().to_string(),
                    provider: "native-nix-evaluator".to_string(),
                    exact_output: evaluator,
                    reason: "bounded native devShell and derivation evaluation identity".to_string(),
                    ..super::SemanticLock::LockRationale::default()
                },
            ));
            for (output_name, derivation) in native_derivations {
                let Some(out) = derivation.outputs().get("out") else {
                    facts
                        .unmapped
                        .push(format!("derivation output {output_name} has no out path"));
                    continue;
                };
                let exact = format!("drvPath={};out={out}", derivation.drv_path());
                lock.records.push(super::SemanticLock::SemanticRecord::new(
                    super::SemanticLock::LockIdentity {
                        kind: super::SemanticLock::LockRecordKind::FlakeEvaluator,
                        key: format!("flake-derivation:{output_name}"),
                        hash: crate::SHA256::sha256_hex(exact.as_bytes()),
                        exact: exact.clone(),
                        platform: system.clone(),
                    },
                    super::SemanticLock::LockRationale {
                        source_ref: flake_path.display().to_string(),
                        provider: "native-nix-evaluator".to_string(),
                        exact_output: exact,
                        reason: "bounded package derivation identity".to_string(),
                        ..super::SemanticLock::LockRationale::default()
                    },
                ));
            }
            if let Err(error) = super::SemanticLock::atomic_commit(dir, &lock) {
                theme.error(
                    "couldn't commit foreign graph",
                    &format!(
                        "couldn't update `{}`: {}",
                        super::SemanticLock::live_path(dir).display(),
                        error.message()
                    ),
                    "fix the lock or project permissions, then run `jet bridge flake` again.",
                );
                return 1;
            }
        }
        Err(error) => {
            theme.error(
                "couldn't load the foreign graph",
                &error.to_string(),
                "refresh the semantic lock after confirming the flake source is correct.",
            );
            return 1;
        }
    }
    facts.unmapped.sort();
    facts.unmapped.dedup();
    facts.packages.sort();
    facts.packages.dedup();
    for field in &facts.unmapped {
        eprint!(
            "{}",
            crate::Diagnostics::render_all(
                flake_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(Syntax::FOREIGN_FLAKE_FILE),
                "",
                std::slice::from_ref(&l0204_unmappable(
                    field,
                    flake_path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(Syntax::FOREIGN_FLAKE_FILE),
                ))
            )
        );
    }
    println!("{}", render_shim(&facts));
    0
}

fn record_package_output_fact(
    facts: &mut DevShellFacts,
    output: &super::SemanticLock::FlakeOutput,
    system: &str,
) {
    if output.system == system {
        if !output.attribute.is_empty()
            && !facts.packages.iter().any(|package| package == &output.attribute)
        {
            facts.packages.push(output.attribute.clone());
        }
        return;
    }
    let label = if output.system.is_empty() {
        format!("packages:{}", output.attribute)
    } else {
        format!("packages:{}:{}", output.system, output.attribute)
    };
    if !facts.unmapped.iter().any(|item| item == &label) {
        facts.unmapped.push(label);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jetpack_bridge_{tag}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn parses_facts_from_fixture() {
        let facts =
            parse_facts_json(r#"{"buildInputs": ["ripgrep", "nodejs", "fd"], "shellHook": ""}"#)
                .unwrap();
        assert_eq!(facts.packages, vec!["fd", "nodejs", "ripgrep"]);
        assert!(facts.unmapped.is_empty());
    }

    #[test]
    fn nonempty_shell_hook_is_unmapped() {
        let facts =
            parse_facts_json(r#"{"buildInputs": [], "shellHook": "export FOO=1"}"#).unwrap();
        assert_eq!(facts.unmapped, vec!["shellHook".to_string()]);
    }

    #[test]
    fn dedupes_and_sorts_packages() {
        let facts =
            parse_facts_json(r#"{"buildInputs": ["fd", "ripgrep", "fd"], "shellHook": ""}"#)
                .unwrap();
        assert_eq!(facts.packages, vec!["fd", "ripgrep"]);
    }

    #[test]
    fn fact_schema_missing_or_wrong_typed_required_fields_fail_closed() {
        for input in [
            r#"{"shellHook":""}"#,
            r#"{"buildInputs":{},"shellHook":""}"#,
            r#"{"buildInputs":["fd",1],"shellHook":""}"#,
            r#"{"buildInputs":[]}"#,
            r#"{"buildInputs":[],"shellHook":false}"#,
            "{\"buildInputs\":[],\"shellHook\":\"raw\nnewline\"}",
            r#"{"buildInputs":1,"buildInputs":[],"shellHook":""}"#,
        ] {
            assert!(
                matches!(parse_facts_json(input), Err(ProviderError::BadOutput(_))),
                "schema-wrong provider output passed: {input}"
            );
        }
    }

    #[test]
    fn fact_schema_error_retains_filtered_provider_noise() {
        let noise = "warning: ignoring untrusted substituter";
        let error = parse_facts_json(&format!("{noise}\n{{\"shellHook\":\"\"}}\n"))
            .unwrap_err();
        let ProviderError::BadOutput(reason) = error else {
            panic!("expected BadOutput, got {error:?}");
        };
        assert!(reason.contains("missing key `buildInputs`"));
        assert!(reason.contains(noise));
    }

    #[test]
    fn render_shim_is_deterministic() {
        let facts = DevShellFacts {
            packages: vec!["fd".to_string(), "ripgrep".to_string()],
            unmapped: Vec::new(),
        };
        let a = render_shim(&facts);
        let b = render_shim(&facts);
        assert_eq!(a, b);
        assert!(a.contains("module env.dev {"));
        assert!(a.contains("packages: [fd, ripgrep]"));
    }

    #[test]
    fn cmd_flake_twice_on_same_fixture_is_byte_identical() {
        // The drift-check test (U16 plan doc): the bridge is a pure function
        // of the flake's facts, so two runs against the same fixture produce
        // the same shim.
        let dir = scratch("drift");
        std::fs::write(dir.join("flake.nix"), "{ }").unwrap();
        let fixtures = scratch("drift_fx");
        std::fs::write(
            fixtures.join(FIXTURE_FILE),
            r#"{"buildInputs": ["ripgrep", "fd"], "shellHook": ""}"#,
        )
        .unwrap();
        let facts_a = read_devshell_facts(&dir, Some(&fixtures)).unwrap();
        let facts_b = read_devshell_facts(&dir, Some(&fixtures)).unwrap();
        assert_eq!(render_shim(&facts_a), render_shim(&facts_b));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&fixtures).ok();
    }

    #[test]
    fn cmd_flake_commits_and_reloads_unsupported_graph_facts() {
        let dir = scratch("commit");
        std::fs::write(
            dir.join("flake.nix"),
            "{ outputs = { devShells.x86_64-linux.default = { shellHook = \"export FOO=1\"; }; }; }",
        )
        .unwrap();
        let fixtures = scratch("commit_fx");
        std::fs::write(
            fixtures.join(FIXTURE_FILE),
            r#"{"buildInputs": ["ripgrep"], "shellHook": "export FOO=1"}"#,
        )
        .unwrap();

        let code = cmd_flake(&Theme::resolve(true), &dir, Some(&fixtures));
        assert_eq!(code, 0);
        let raw = std::fs::read_to_string(crate::SemanticLock::live_path(&dir)).unwrap();
        assert!(raw.contains("flake-unsupported:shellHook"), "{raw}");
        assert!(raw.contains("flake-evaluator"), "{raw}");
        assert!(raw.contains("native-nix:2.34.8:"), "{raw}");
        let graph = FlakeGraph::load(&dir.join("flake.nix")).unwrap();
        assert_eq!(graph.unsupported, vec!["shellHook".to_string()]);

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&fixtures).ok();
    }

    #[test]
    fn cmd_flake_commits_locked_transitive_nodes_and_indirect_registry() {
        let dir = scratch("locked_graph");
        std::fs::write(
            dir.join("flake.nix"),
            r#"{
  inputs = {
    nixpkgs.url = "nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
  };
  outputs = { ... }: { devShells.x86_64-linux.default = {}; };
}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("flake.lock"),
            include_str!("../../../tests/fixtures/nix-compat/stage-a-flake.lock"),
        )
        .unwrap();
        let fixtures = scratch("locked_graph_fx");
        std::fs::write(
            fixtures.join(FIXTURE_FILE),
            r#"{"buildInputs": [], "shellHook": ""}"#,
        )
        .unwrap();

        assert_eq!(cmd_flake(&Theme::resolve(true), &dir, Some(&fixtures)), 0);
        let graph = FlakeGraph::load(&dir.join("flake.nix")).unwrap();
        assert_eq!(graph.lock_nodes.len(), 4);
        assert_eq!(graph.registries.len(), 1);
        assert_eq!(graph.registries[0].alias, "nixpkgs");
        let raw = std::fs::read_to_string(crate::SemanticLock::live_path(&dir)).unwrap();
        assert!(raw.contains("flake-lock-node:flake-utils"), "{raw}");
        assert!(raw.contains("flake-registry:nixpkgs"), "{raw}");

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&fixtures).ok();
    }

    #[test]
    fn cmd_flake_rejects_a_stale_semantic_lock() {
        let dir = scratch("stale_graph");
        let system = host_system();
        let source = format!(
            "{{ devShells.{system}.default = {{ packages = [ ]; }}; }}"
        );
        std::fs::write(dir.join("flake.nix"), &source).unwrap();
        let fixtures = scratch("stale_graph_fx");
        std::fs::write(
            fixtures.join(FIXTURE_FILE),
            r#"{"buildInputs": [], "shellHook": ""}"#,
        )
        .unwrap();

        assert_eq!(cmd_flake(&Theme::resolve(true), &dir, Some(&fixtures)), 0);
        std::fs::write(dir.join("flake.nix"), format!("{source}\n# changed\n")).unwrap();
        assert_eq!(cmd_flake(&Theme::resolve(true), &dir, Some(&fixtures)), 1);

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&fixtures).ok();
    }

    #[test]
    fn cmd_flake_excludes_cross_system_package_from_host_shim() {
        const CHILD: &str = "JETPACK_BRIDGE_CROSS_SYSTEM_CHILD";
        let host = host_system();
        let other = if host == "aarch64-linux" {
            "x86_64-linux"
        } else {
            "aarch64-linux"
        };
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "Bridge::tests::cmd_flake_excludes_cross_system_package_from_host_shim",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "child failed\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stdout.contains("packages: []"), "stdout: {stdout}");
            assert!(!stdout.contains("foreign"), "stdout: {stdout}");
            assert!(
                stderr.contains(&format!("packages:{other}:default")),
                "stderr: {stderr}"
            );
            return;
        }
        let source = format!("{{ packages.{other}.default = \"foreign\"; }}");
        let dir = scratch("cross_system");
        std::fs::write(dir.join("flake.nix"), source).unwrap();
        let fixtures = scratch("cross_system_fx");
        std::fs::write(
            fixtures.join(FIXTURE_FILE),
            r#"{"buildInputs": [], "shellHook": ""}"#,
        )
        .unwrap();
        assert_eq!(cmd_flake(&Theme::resolve(true), &dir, Some(&fixtures)), 0);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&fixtures).ok();
    }

    #[test]
    fn cmd_flake_records_native_package_derivation_identity() {
        let dir = scratch("derivation");
        let system = host_system();
        std::fs::write(
            dir.join("flake.nix"),
            format!(
                "{{ packages.{system}.default = builtins.derivation {{ name = \"hello\"; system = \"{system}\"; builder = \"/bin/sh\"; args = [ \"-c\" \"echo hi > $out\" ]; }}; devShells.{system}.default = {{ packages = [ pkgs.fd ]; }}; }}"
            ),
        )
        .unwrap();
        let fixtures = scratch("derivation_fx");
        std::fs::write(
            fixtures.join(FIXTURE_FILE),
            r#"{"buildInputs": ["fd"], "shellHook": ""}"#,
        )
        .unwrap();

        let code = cmd_flake(&Theme::resolve(true), &dir, Some(&fixtures));
        assert_eq!(code, 0);
        let raw = std::fs::read_to_string(crate::SemanticLock::live_path(&dir)).unwrap();
        assert!(raw.contains("flake-derivation:packages:"));
        assert!(raw.contains("76w21n1f03fs5kw8fnffphx7qrqffw6r-hello.drv"));

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&fixtures).ok();
    }

    #[test]
    fn cmd_flake_rejects_unsupported_package_derivation() {
        let dir = scratch("derivation_error");
        let system = host_system();
        std::fs::write(
            dir.join("flake.nix"),
            format!(
                "{{ packages.{system}.default = builtins.derivation {{ name = \"bad\"; system = \"{system}\"; builder = \"/bin/sh\"; args = [ /tmp/outside ]; }}; }}"
            ),
        )
        .unwrap();
        let fixtures = scratch("derivation_error_fx");
        std::fs::write(
            fixtures.join(FIXTURE_FILE),
            r#"{"buildInputs": [], "shellHook": ""}"#,
        )
        .unwrap();

        let code = cmd_flake(&Theme::resolve(true), &dir, Some(&fixtures));
        assert_eq!(code, 1);
        assert!(!crate::SemanticLock::live_path(&dir).exists());

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&fixtures).ok();
    }

    #[test]
    fn cmd_flake_rejects_systemless_package_output() {
        let dir = scratch("systemless_derivation");
        std::fs::write(
            dir.join("flake.nix"),
            "{ packages.default = builtins.derivation { name = \"bad\"; system = \"x86_64-linux\"; builder = \"/bin/sh\"; }; }",
        )
        .unwrap();
        let fixtures = scratch("systemless_derivation_fx");
        std::fs::write(
            fixtures.join(FIXTURE_FILE),
            r#"{"buildInputs": [], "shellHook": ""}"#,
        )
        .unwrap();

        let code = cmd_flake(&Theme::resolve(true), &dir, Some(&fixtures));
        assert_eq!(code, 1);
        assert!(!crate::SemanticLock::live_path(&dir).exists());

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&fixtures).ok();
    }

    #[test]
    fn missing_fixture_is_fixture_missing_error() {
        let dir = scratch("nofx");
        let fixtures = scratch("nofx_fx"); // exists but has no flake-devshell.json
        let err = read_devshell_facts(&dir, Some(&fixtures)).unwrap_err();
        assert!(matches!(err, ProviderError::FixtureMissing(_)));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&fixtures).ok();
    }

    #[test]
    fn native_path_reads_only_authorized_project_imports() {
        let dir = scratch("native_import");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(
            dir.join("flake.nix"),
            "{ outputs = { devShells.x86_64-linux.default = (import ./sub/shell.nix) { pkgs = pkgs; }; }; }",
        )
        .unwrap();
        std::fs::write(
            dir.join("sub/shell.nix"),
            "{ pkgs }: { packages = [ pkgs.fd ]; buildInputs = import ../inputs.nix pkgs; }",
        )
        .unwrap();
        std::fs::write(dir.join("inputs.nix"), "pkgs: [ pkgs.ripgrep ]").unwrap();

        let facts = read_devshell_facts(&dir, None).expect("native project imports must work");
        assert_eq!(facts.packages, vec!["fd", "ripgrep"]);

        std::fs::write(
            dir.join("sub/shell.nix"),
            "{ pkgs }: { packages = import ../../outside.nix pkgs; }",
        )
        .unwrap();
        let error = read_devshell_facts(&dir, None).expect_err("project-root escape must fail");
        assert!(matches!(
            error,
            ProviderError::Unsupported(reason)
                if reason.contains("escapes the flake project-root authority")
        ));

        #[cfg(unix)]
        {
            let outside = scratch("native_import_outside");
            std::fs::write(outside.join("escape.nix"), "pkgs: [ pkgs.fd ]").unwrap();
            std::fs::write(
                dir.join("sub/shell.nix"),
                "{ pkgs }: { packages = import ./escape.nix pkgs; }",
            )
            .unwrap();
            std::os::unix::fs::symlink(outside.join("escape.nix"), dir.join("sub/escape.nix"))
                .unwrap();
            let error = read_devshell_facts(&dir, None)
                .expect_err("symlink imports must remain below the project root");
            assert!(matches!(
                error,
                ProviderError::Unsupported(reason)
                    if reason.contains("symlinks are not allowed")
            ));
            std::fs::remove_dir_all(&outside).ok();
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn native_path_works_without_nix_on_path() {
        const CHILD: &str = "JETPACK_NATIVE_NIX_NO_PATH_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "Bridge::tests::native_path_works_without_nix_on_path",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env("PATH", "")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "child failed\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        assert!(!crate::Provider::nix_on_path());
        let dir = scratch("native_no_nix");
        std::fs::write(
            dir.join("flake.nix"),
            "{ devShells.x86_64-linux.default = { packages = [ pkgs.fd ]; }; }",
        )
        .unwrap();
        let facts = read_devshell_facts(&dir, None).expect("native evaluator must not need Nix");
        assert_eq!(facts.packages, vec!["fd"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn native_projection_budget_failure_is_e1256() {
        let dir = scratch("native_budget");
        let source = "x".repeat((1 << 20) + 1);
        std::fs::write(dir.join("flake.nix"), source).unwrap();
        let error = read_devshell_facts(&dir, None).expect_err("oversized foreign input must fail");
        assert_eq!(error.code(), Some("E1256"));
        assert!(matches!(error, ProviderError::ForeignProjection(reason) if reason.contains("evaluator limit")));
        std::fs::remove_dir_all(&dir).ok();
    }
}
