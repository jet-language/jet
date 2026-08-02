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
/// `nativeBuildInputs` lists and retains unsupported hooks as loss facts.
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
            let evaluation = crate::NixEval::evaluate_devshell(&source, &system)
                .map_err(|error| ProviderError::Unsupported(error.to_string()))?;
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
                    super::SemanticLock::FlakeOutputKind::Package => {
                        if !output.attribute.is_empty()
                            && !facts.packages.iter().any(|package| package == &output.attribute)
                        {
                            facts.packages.push(output.attribute.clone());
                        }
                    }
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
            let system = host_system();
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
                    platform: system,
                },
                super::SemanticLock::LockRationale {
                    source_ref: flake_path.display().to_string(),
                    provider: "native-nix-evaluator".to_string(),
                    exact_output: evaluator,
                    reason: "bounded native devShell evaluation identity".to_string(),
                    ..super::SemanticLock::LockRationale::default()
                },
            ));
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
        Err(error) => facts
            .unmapped
            .push(format!("flake graph: {error}")),
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
    fn missing_fixture_is_fixture_missing_error() {
        let dir = scratch("nofx");
        let fixtures = scratch("nofx_fx"); // exists but has no flake-devshell.json
        let err = read_devshell_facts(&dir, Some(&fixtures)).unwrap_err();
        assert!(matches!(err, ProviderError::FixtureMissing(_)));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&fixtures).ok();
    }
}
