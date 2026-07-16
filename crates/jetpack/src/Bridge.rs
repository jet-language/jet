//! `jetpack bridge flake` (U16, card c9jetpackgates) — a best-effort
//! translator from an existing `flake.nix`'s default devShell into jetpack's
//! own `env.*` module form (D-JPK-MODBODY1's already-ratified `module
//! env.dev { packages: […] }` shape — no new syntax needed).
//!
//! Never touches the project's `env.jet`: the shim prints to stdout so the
//! user reviews and merges it themselves (I8 — one canonical env surface, no
//! silent second manifest). Fields the shim can't express (`shellHook`, a
//! second named devShell, …) come back as `unmapped` and fire L0204, one
//! warning per field, without blocking the print.
//!
//! Determinism for tests: mirrors `Provider.rs`'s fixture convention — a
//! captured `nix eval --json` payload can stand in for the real binary, so
//! the drift check (same flake.nix twice → same shim) never needs Nix
//! installed.

use super::Output::Theme;
use super::Provider::ProviderError;
use super::JSON;
use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use std::path::Path;
use std::process::Command;

/// Facts pulled from a flake's default devShell, best-effort.
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
/// fixture (offline/test path) or by shelling out to `nix eval`. The eval
/// expression reads the default devShell's `buildInputs` (mapped to each
/// derivation's `pname`/`name`) and `shellHook`, the two fields every
/// `pkgs.mkShell`-built devShell carries.
pub fn read_devshell_facts(
    flake_dir: &Path,
    fixtures: Option<&Path>,
) -> Result<DevShellFacts, ProviderError> {
    let stdout = match fixtures {
        Some(dir) => {
            let path = dir.join(FIXTURE_FILE);
            std::fs::read_to_string(&path).map_err(|_| ProviderError::FixtureMissing(path))?
        }
        None => run_nix_eval(flake_dir)?,
    };
    parse_facts_json(&stdout)
}

fn run_nix_eval(flake_dir: &Path) -> Result<String, ProviderError> {
    let expr = format!(
        "let f = builtins.getFlake {:?}; s = builtins.currentSystem; \
         ds = f.devShells.${{s}}.default; in {{ \
         buildInputs = map (p: p.pname or p.name or \"?\") (ds.buildInputs or []); \
         shellHook = ds.shellHook or \"\"; }}",
        flake_dir.display().to_string()
    );
    let output = Command::new("nix")
        .args(["eval", "--impure", "--json", "--expr", &expr])
        .output();
    let output = match output {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ProviderError::NixMissing)
        }
        Err(e) => return Err(ProviderError::BuildFailed(e.to_string())),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr
            .trim()
            .lines()
            .last()
            .unwrap_or("nix eval failed")
            .to_string();
        return Err(ProviderError::BuildFailed(reason));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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
        "// Generated by `jet bridge flake` — best-effort flake.nix translation.\n\
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
        "`jet bridge flake` (U16) is a best-effort translator; some flake.nix/devenv.nix fields \
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

/// `jetpack bridge flake` — read `flake.nix` in `dir`, print the generated
/// shim to stdout, and print any L0204 warnings to stderr. Returns the exit
/// code. `nix` missing is E1256, checked by the caller before this runs (so
/// the message names the right command).
pub fn cmd_flake(theme: &Theme, dir: &Path, fixtures: Option<&Path>) -> i32 {
    let flake_path = dir.join(Syntax::FOREIGN_FLAKE_FILE);
    if !flake_path.is_file() {
        theme.error(
            "no flake.nix here",
            &format!(
                "`jet bridge flake` translates a {} in the current directory; none was found.",
                Syntax::FOREIGN_FLAKE_FILE
            ),
            "run this from the directory that has the flake.nix, or write env.* by hand.",
        );
        return 2;
    }
    let facts = match read_devshell_facts(dir, fixtures) {
        Ok(f) => f,
        Err(e) => {
            super::CLI::report_provider_error(theme, &e);
            return 1;
        }
    };
    for field in &facts.unmapped {
        eprint!(
            "{}",
            crate::Diagnostics::render_all(
                Syntax::FOREIGN_FLAKE_FILE,
                "",
                std::slice::from_ref(&l0204_unmappable(field, Syntax::FOREIGN_FLAKE_FILE))
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
    fn missing_fixture_is_fixture_missing_error() {
        let dir = scratch("nofx");
        let fixtures = scratch("nofx_fx"); // exists but has no flake-devshell.json
        let err = read_devshell_facts(&dir, Some(&fixtures)).unwrap_err();
        assert!(matches!(err, ProviderError::FixtureMissing(_)));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&fixtures).ok();
    }
}
